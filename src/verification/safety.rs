use crate::ast::{DeviceType, PlcProgram};
use crate::ir::{
    ConstraintSet, SafetyExpr, SafetyRelation, State, StateMachine, Transition, TransitionAction,
    TransitionGuard,
};
use petgraph::algo::kosaraju_scc;
use petgraph::graph::DiGraph;
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, VecDeque};
use std::fmt;

#[cfg(feature = "z3-solver")]
use z3::ast::Bool;
#[cfg(feature = "z3-solver")]
use z3::{Config, Context, SatResult, Solver};

#[derive(Debug, Clone, Default)]
pub struct SafetyConfig {
    pub bmc_max_depth: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SafetyProofLevel {
    Complete,
    Bounded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafetyReport {
    pub level: SafetyProofLevel,
    pub explored_depth: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafetyDiagnostic {
    pub line: usize,
    pub constraint: String,
    pub reason: String,
    pub violation_path: Vec<String>,
    pub suggestion: String,
}

impl fmt::Display for SafetyDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "ERROR [safety] 安全约束违反")?;
        writeln!(f, "  位置: <input>:{}:1", self.line)?;
        writeln!(f, "  约束: {}", self.constraint)?;
        writeln!(f, "  原因: {}", self.reason)?;
        writeln!(f, "  违反路径:")?;
        for (index, step) in self.violation_path.iter().enumerate() {
            writeln!(f, "    {}. {step}", index + 1)?;
        }
        write!(f, "  建议: {}", self.suggestion)
    }
}

#[derive(Debug, Clone)]
struct DeviceDomain {
    name: String,
    states: Vec<String>,
    default_state: usize,
}

#[derive(Debug, Clone)]
struct ModelEdge {
    from: usize,
    to: usize,
    effects: HashMap<usize, usize>,
    label: String,
}

#[derive(Debug, Clone)]
struct SafetyModel {
    states: Vec<State>,
    initial_state: usize,
    edges: Vec<ModelEdge>,
    outgoing: Vec<Vec<usize>>,
    devices: Vec<DeviceDomain>,
    device_index: HashMap<String, usize>,
    device_state_index: Vec<HashMap<String, usize>>,
    suggested_depth: usize,
    max_scc_depth: usize,
}

#[derive(Debug, Clone)]
struct RuleBinding {
    relation: SafetyRelation,
    left_device: usize,
    left_state: usize,
    right_device: usize,
    right_state: usize,
}

#[derive(Debug, Clone)]
struct DepthPlan {
    effective_depth: usize,
    warnings: Vec<String>,
    truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ConcreteState {
    control_state: usize,
    device_states: Vec<usize>,
}

#[derive(Debug, Clone)]
struct SearchNode {
    state: ConcreteState,
    depth: usize,
    parent: Option<usize>,
    via_edge: Option<usize>,
}

#[derive(Debug, Clone)]
struct SearchOutcome {
    counterexample: Option<Counterexample>,
    fully_explored: bool,
}

#[derive(Debug, Clone)]
struct Counterexample {
    path: Vec<String>,
}

pub fn verify_safety(
    program: &PlcProgram,
    constraints: &ConstraintSet,
    state_machine: &StateMachine,
) -> Result<SafetyReport, Vec<SafetyDiagnostic>> {
    verify_safety_with_config(program, constraints, state_machine, SafetyConfig::default())
}

pub fn verify_safety_with_config(
    program: &PlcProgram,
    constraints: &ConstraintSet,
    state_machine: &StateMachine,
    config: SafetyConfig,
) -> Result<SafetyReport, Vec<SafetyDiagnostic>> {
    let model = SafetyModel::from_inputs(program, constraints, state_machine);
    let depth_plan = build_depth_plan(&model, &config);

    #[cfg(feature = "z3-solver")]
    z3_sanity_probe();

    let mut diagnostics = Vec::new();
    let mut all_complete = true;
    let mut checked_rules = 0usize;
    let mut skipped_threshold_rules = Vec::new();

    for (index, rule) in constraints.safety.iter().enumerate() {
        let relation = relation_text(&rule.relation);
        let left_text = safety_expr_text(&rule.left);
        let right_text = safety_expr_text(&rule.right);
        let rule_text = format!("{left_text} {relation} {right_text}");
        let line = program
            .constraints
            .safety
            .get(index)
            .map(|node| node.line.max(1))
            .unwrap_or(1);

        let Some(binding) = bind_safety_expr_rule(&model, rule) else {
            if safety_rule_has_threshold(rule) {
                skipped_threshold_rules.push(format!(
                    "WARNING: Safety 规则 <input>:{} {} 含模拟量阈值，当前未建模，已跳过验证",
                    line, rule_text
                ));
            }
            continue;
        };

        checked_rules += 1;

        let outcome = analyze_rule(&model, binding, depth_plan.effective_depth);
        if let Some(counterexample) = outcome.counterexample {
            let (reason, suggestion) = match rule.relation {
                SafetyRelation::ConflictsWith => (
                    format!(
                        "{} 与 {} 在可达状态同时成立",
                        left_text, right_text
                    ),
                    format!(
                        "请在触发 {} 之前确保 {} 已复位，或调整并行/跳转逻辑避免两者同时成立",
                        right_text, left_text
                    ),
                ),
                SafetyRelation::Requires => (
                    format!(
                        "{} 成立时 {} 未满足",
                        left_text, right_text
                    ),
                    format!(
                        "请在触发 {} 之前先确保 {} 成立，必要时添加 wait 或调整 step 顺序",
                        left_text, right_text
                    ),
                ),
            };

            diagnostics.push(SafetyDiagnostic {
                line,
                constraint: rule_text,
                reason,
                violation_path: counterexample.path,
                suggestion,
            });
            continue;
        }

        if !outcome.fully_explored {
            all_complete = false;
        }
    }

    if depth_plan.truncated {
        all_complete = false;
    }

    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let has_skipped_thresholds = !skipped_threshold_rules.is_empty();
    let mut warnings = depth_plan.warnings;
    warnings.extend(skipped_threshold_rules.into_iter());

    let level = if !has_skipped_thresholds && (checked_rules == 0 || all_complete) {
        SafetyProofLevel::Complete
    } else {
        if !all_complete {
            warnings.push(format!(
                "WARNING: Safety 在深度 {} 内未发现反例，但未获得完备证明。建议增大 bmc_max_depth 以提升有界覆盖，或调整模型以帮助 k-induction 收敛",
                depth_plan.effective_depth
            ));
        }
        SafetyProofLevel::Bounded
    };

    Ok(SafetyReport {
        level,
        explored_depth: depth_plan.effective_depth,
        warnings,
    })
}

impl SafetyModel {
    fn from_inputs(
        program: &PlcProgram,
        constraints: &ConstraintSet,
        state_machine: &StateMachine,
    ) -> Self {
        let mut states = state_machine.states.clone();
        if states.is_empty() {
            states.push(state_machine.initial.clone());
        }

        let mut state_index = HashMap::<(String, String), usize>::new();
        for (index, state) in states.iter().enumerate() {
            state_index.insert((state.task_name.clone(), state.step_name.clone()), index);
        }

        let initial_state = state_index
            .get(&(
                state_machine.initial.task_name.clone(),
                state_machine.initial.step_name.clone(),
            ))
            .copied()
            .unwrap_or(0);

        let (devices, device_index, device_state_index) =
            collect_device_domains(program, constraints);

        let mut edges = Vec::new();
        let mut outgoing = vec![Vec::new(); states.len()];

        for transition in &state_machine.transitions {
            let Some(from) = state_index
                .get(&(
                    transition.from.task_name.clone(),
                    transition.from.step_name.clone(),
                ))
                .copied()
            else {
                continue;
            };
            let Some(to) = state_index
                .get(&(
                    transition.to.task_name.clone(),
                    transition.to.step_name.clone(),
                ))
                .copied()
            else {
                continue;
            };

            let effects = transition_effects(transition, &device_index, &device_state_index);
            let edge_index = edges.len();
            edges.push(ModelEdge {
                from,
                to,
                effects,
                label: transition_label(transition),
            });
            outgoing[from].push(edge_index);
        }

        for state_id in 0..states.len() {
            if !outgoing[state_id].is_empty() {
                continue;
            }

            let edge_index = edges.len();
            edges.push(ModelEdge {
                from: state_id,
                to: state_id,
                effects: HashMap::new(),
                label: "无出边，保持当前状态".to_string(),
            });
            outgoing[state_id].push(edge_index);
        }

        merge_parallel_join_effects(&states, &mut edges);

        let max_scc_depth = scc_minimum_depth(states.len(), &edges);
        let suggested_depth = states.len().max(max_scc_depth).max(1);

        Self {
            states,
            initial_state,
            edges,
            outgoing,
            devices,
            device_index,
            device_state_index,
            suggested_depth,
            max_scc_depth,
        }
    }
}

fn merge_parallel_join_effects(states: &[State], edges: &mut [ModelEdge]) {
    let mut join_effects = HashMap::<usize, HashMap<usize, usize>>::new();

    for edge in edges.iter() {
        if !is_parallel_branch_state(states.get(edge.from))
            || !is_parallel_join_state(states.get(edge.to))
        {
            continue;
        }

        let merged = join_effects.entry(edge.to).or_default();
        for (&device_id, &state_id) in &edge.effects {
            merged.insert(device_id, state_id);
        }
    }

    for edge in edges.iter_mut() {
        if !is_parallel_branch_state(states.get(edge.from))
            || !is_parallel_join_state(states.get(edge.to))
        {
            continue;
        }

        if let Some(merged) = join_effects.get(&edge.to) {
            edge.effects = merged.clone();
        }
    }
}

fn is_parallel_branch_state(state: Option<&State>) -> bool {
    state.is_some_and(|state| {
        state.step_name.contains("__parallel_") && state.step_name.contains("_branch_")
    })
}

fn is_parallel_join_state(state: Option<&State>) -> bool {
    state.is_some_and(|state| {
        state.step_name.contains("__parallel_") && state.step_name.ends_with("_join")
    })
}

fn collect_device_domains(
    program: &PlcProgram,
    constraints: &ConstraintSet,
) -> (
    Vec<DeviceDomain>,
    HashMap<String, usize>,
    Vec<HashMap<String, usize>>,
) {
    let mut devices = Vec::<DeviceDomain>::new();
    let mut device_index = HashMap::<String, usize>::new();

    for device in &program.topology.devices {
        let states = match device.device_type {
            DeviceType::Cylinder => vec!["extended".to_string(), "retracted".to_string()],
            DeviceType::DigitalOutput
            | DeviceType::DigitalInput
            | DeviceType::SolenoidValve
            | DeviceType::Sensor
            | DeviceType::Motor => vec!["on".to_string(), "off".to_string()],
            DeviceType::AnalogInput | DeviceType::AnalogOutput => {
                vec!["analog_active".to_string()]
            }
        };

        let default_state_name = match device.device_type {
            DeviceType::Cylinder => "retracted",
            DeviceType::AnalogInput | DeviceType::AnalogOutput => "analog_active",
            _ => "off",
        };

        let default_state = states
            .iter()
            .position(|state| state == default_state_name)
            .unwrap_or(0);

        let index = devices.len();
        devices.push(DeviceDomain {
            name: device.name.clone(),
            states,
            default_state,
        });
        device_index.insert(device.name.clone(), index);
    }

    for rule in &constraints.safety {
        if let SafetyExpr::State(ref expr) = rule.left {
            if let Some(left_device) = device_index.get(&expr.device).copied() {
                ensure_device_state(&mut devices[left_device], &expr.state);
            }
        }

        if let SafetyExpr::State(ref expr) = rule.right {
            if let Some(right_device) = device_index.get(&expr.device).copied() {
                ensure_device_state(&mut devices[right_device], &expr.state);
            }
        }
    }

    let mut state_index = Vec::with_capacity(devices.len());
    for domain in &devices {
        let mut map = HashMap::new();
        for (idx, state) in domain.states.iter().enumerate() {
            map.insert(state.clone(), idx);
        }
        state_index.push(map);
    }

    (devices, device_index, state_index)
}

fn ensure_device_state(domain: &mut DeviceDomain, state_name: &str) {
    if domain.states.iter().any(|state| state == state_name) {
        return;
    }

    domain.states.push(state_name.to_string());
}

fn transition_effects(
    transition: &Transition,
    device_index: &HashMap<String, usize>,
    device_state_index: &[HashMap<String, usize>],
) -> HashMap<usize, usize> {
    let mut effects = HashMap::<usize, usize>::new();

    for action in &transition.actions {
        let Some((target_device, target_state)) = action_effect(action) else {
            continue;
        };

        let Some(device_id) = device_index.get(target_device).copied() else {
            continue;
        };

        let Some(state_id) = device_state_index[device_id].get(target_state).copied() else {
            continue;
        };

        effects.insert(device_id, state_id);
    }

    effects
}

fn action_effect(action: &TransitionAction) -> Option<(&str, &str)> {
    match action {
        TransitionAction::Extend { target } => Some((target.as_str(), "extended")),
        TransitionAction::Retract { target } => Some((target.as_str(), "retracted")),
        TransitionAction::Set { target, value } => {
            let state = match value {
                crate::ir::BinaryValue::On => "on",
                crate::ir::BinaryValue::Off => "off",
            };
            Some((target.as_str(), state))
        }
        TransitionAction::SetAnalog { target, .. } => Some((target.as_str(), "analog_active")),
        TransitionAction::Log { .. } => None,
    }
}

fn transition_label(transition: &Transition) -> String {
    let guard = guard_name(&transition.guard);
    let action_text = transition
        .actions
        .iter()
        .filter_map(action_name)
        .collect::<Vec<_>>();

    if action_text.is_empty() {
        guard.to_string()
    } else {
        format!("{}；动作: {}", guard, action_text.join(", "))
    }
}

fn guard_name(guard: &TransitionGuard) -> &'static str {
    match guard {
        TransitionGuard::Always => "always",
        TransitionGuard::Condition { .. } => "condition",
        TransitionGuard::Timeout { .. } => "timeout",
        TransitionGuard::Delay { .. } => "delay",
    }
}

fn action_name(action: &TransitionAction) -> Option<String> {
    match action {
        TransitionAction::Extend { target } => Some(format!("extend {target}")),
        TransitionAction::Retract { target } => Some(format!("retract {target}")),
        TransitionAction::Set { target, value } => Some(format!(
            "set {} {}",
            target,
            match value {
                crate::ir::BinaryValue::On => "on",
                crate::ir::BinaryValue::Off => "off",
            }
        )),
        TransitionAction::SetAnalog { target, value_raw } => {
            Some(format!("set_analog {target} {value_raw}"))
        }
        TransitionAction::Log { message } => Some(format!("log \"{message}\"")),
    }
}

fn scc_minimum_depth(state_count: usize, edges: &[ModelEdge]) -> usize {
    if state_count == 0 {
        return 1;
    }

    let mut graph = DiGraph::<usize, ()>::new();
    let mut nodes = Vec::with_capacity(state_count);
    for index in 0..state_count {
        nodes.push(graph.add_node(index));
    }

    for edge in edges {
        if edge.from >= state_count || edge.to >= state_count {
            continue;
        }
        graph.add_edge(nodes[edge.from], nodes[edge.to], ());
    }

    let mut depth_requirement = 0usize;
    for component in kosaraju_scc(&graph) {
        if component.is_empty() {
            continue;
        }

        let has_cycle = component.len() > 1
            || graph
                .edges(component[0])
                .any(|edge| edge.target() == component[0]);

        if !has_cycle {
            continue;
        }

        depth_requirement = depth_requirement.max(component.len() + 1);
    }

    depth_requirement
}

fn build_depth_plan(model: &SafetyModel, config: &SafetyConfig) -> DepthPlan {
    let target_depth = model.suggested_depth;
    let mut warnings = Vec::new();
    let mut truncated = false;

    let effective_depth = if let Some(user_limit) = config.bmc_max_depth {
        if user_limit < target_depth {
            truncated = true;
            let reason = if model.max_scc_depth > 0 && user_limit < model.max_scc_depth {
                format!(
                    "WARNING: bmc_max_depth={} 小于 SCC 建议深度 {}，Safety 搜索将截断至 {}（有界验证）",
                    user_limit, model.max_scc_depth, user_limit
                )
            } else {
                format!(
                    "WARNING: bmc_max_depth={} 小于建议展开深度 {}，Safety 搜索将截断至 {}（有界验证）",
                    user_limit, target_depth, user_limit
                )
            };
            warnings.push(reason);
            user_limit
        } else {
            user_limit
        }
    } else {
        target_depth
    };

    DepthPlan {
        effective_depth: effective_depth.max(1),
        warnings,
        truncated,
    }
}

fn bind_rule(
    model: &SafetyModel,
    relation: SafetyRelation,
    left_device: &str,
    left_state: &str,
    right_device: &str,
    right_state: &str,
) -> Option<RuleBinding> {
    let left_device_id = model.device_index.get(left_device).copied()?;
    let right_device_id = model.device_index.get(right_device).copied()?;

    let left_state_id = model.device_state_index[left_device_id]
        .get(left_state)
        .copied()?;
    let right_state_id = model.device_state_index[right_device_id]
        .get(right_state)
        .copied()?;

    Some(RuleBinding {
        relation,
        left_device: left_device_id,
        left_state: left_state_id,
        right_device: right_device_id,
        right_state: right_state_id,
    })
}

fn bind_safety_expr_rule(
    model: &SafetyModel,
    rule: &crate::ir::SafetyRule,
) -> Option<RuleBinding> {
    let (left_device, left_state) = safety_expr_device_state(&rule.left)?;
    let (right_device, right_state) = safety_expr_device_state(&rule.right)?;
    bind_rule(
        model,
        rule.relation.clone(),
        left_device,
        left_state,
        right_device,
        right_state,
    )
}

fn safety_expr_device_state(expr: &SafetyExpr) -> Option<(&str, &str)> {
    match expr {
        SafetyExpr::State(state_expr) => Some((state_expr.device.as_str(), state_expr.state.as_str())),
        SafetyExpr::Threshold { .. } => {
            // Analog threshold constraints are not yet tracked in BMC state space.
            // Skip them gracefully so discrete rules still work.
            None
        }
    }
}

fn safety_expr_text(expr: &SafetyExpr) -> String {
    match expr {
        SafetyExpr::State(state_expr) => format!("{}.{}", state_expr.device, state_expr.state),
        SafetyExpr::Threshold {
            device,
            operator,
            value,
        } => format!("{device} {operator} {value}"),
    }
}

fn safety_rule_has_threshold(rule: &crate::ir::SafetyRule) -> bool {
    matches!(rule.left, SafetyExpr::Threshold { .. })
        || matches!(rule.right, SafetyExpr::Threshold { .. })
}

fn analyze_rule(model: &SafetyModel, rule: RuleBinding, max_depth: usize) -> SearchOutcome {
    let initial_state = initial_concrete_state(model);
    let mut nodes = vec![SearchNode {
        state: initial_state.clone(),
        depth: 0,
        parent: None,
        via_edge: None,
    }];
    let mut queue = VecDeque::from([0usize]);
    let mut shortest_depth = HashMap::<ConcreteState, usize>::new();
    shortest_depth.insert(initial_state, 0);

    let mut fully_explored = true;

    while let Some(node_id) = queue.pop_front() {
        let node = nodes[node_id].clone();

        if violates_rule(&node.state, &rule) {
            let path = render_path(model, &nodes, node_id, &rule);
            return SearchOutcome {
                counterexample: Some(Counterexample { path }),
                fully_explored,
            };
        }

        let outgoing = &model.outgoing[node.state.control_state];
        if node.depth == max_depth {
            for &edge_id in outgoing {
                let edge = &model.edges[edge_id];
                let candidate = apply_edge(edge, &node.state);
                if !shortest_depth.contains_key(&candidate) {
                    fully_explored = false;
                }
            }
            continue;
        }

        for &edge_id in outgoing {
            let edge = &model.edges[edge_id];
            let next_state = apply_edge(edge, &node.state);
            let next_depth = node.depth + 1;

            if shortest_depth
                .get(&next_state)
                .is_some_and(|depth| *depth <= next_depth)
            {
                continue;
            }

            shortest_depth.insert(next_state.clone(), next_depth);
            let next_id = nodes.len();
            nodes.push(SearchNode {
                state: next_state,
                depth: next_depth,
                parent: Some(node_id),
                via_edge: Some(edge_id),
            });
            queue.push_back(next_id);
        }
    }

    SearchOutcome {
        counterexample: None,
        fully_explored,
    }
}

fn initial_concrete_state(model: &SafetyModel) -> ConcreteState {
    let device_states = model
        .devices
        .iter()
        .map(|device| device.default_state)
        .collect::<Vec<_>>();

    ConcreteState {
        control_state: model.initial_state,
        device_states,
    }
}

fn apply_edge(edge: &ModelEdge, current: &ConcreteState) -> ConcreteState {
    let mut device_states = current.device_states.clone();
    for (&device_id, &state_id) in &edge.effects {
        if device_id < device_states.len() {
            device_states[device_id] = state_id;
        }
    }

    ConcreteState {
        control_state: edge.to,
        device_states,
    }
}

fn violates_rule(state: &ConcreteState, rule: &RuleBinding) -> bool {
    let left_matches = state.device_states[rule.left_device] == rule.left_state;
    let right_matches = state.device_states[rule.right_device] == rule.right_state;

    match rule.relation {
        SafetyRelation::ConflictsWith => left_matches && right_matches,
        SafetyRelation::Requires => left_matches && !right_matches,
    }
}

fn render_path(
    model: &SafetyModel,
    nodes: &[SearchNode],
    terminal_node: usize,
    rule: &RuleBinding,
) -> Vec<String> {
    let mut order = Vec::new();
    let mut cursor = Some(terminal_node);
    while let Some(node_id) = cursor {
        order.push(node_id);
        cursor = nodes[node_id].parent;
    }
    order.reverse();

    let initial = &nodes[order[0]].state;
    let mut lines = vec![format!(
        "初始状态 {}",
        state_name(&model.states[initial.control_state])
    )];

    for window in order.windows(2) {
        let from = &nodes[window[0]].state;
        let to_node = &nodes[window[1]];
        let to = &to_node.state;

        let edge_id = to_node.via_edge.unwrap_or_else(|| {
            model.outgoing[from.control_state]
                .first()
                .copied()
                .unwrap_or(0)
        });
        let edge = &model.edges[edge_id];

        let from_name = state_name(&model.states[from.control_state]);
        let to_name = state_name(&model.states[to.control_state]);
        lines.push(format!("{from_name} --[{}]--> {to_name}", edge.label));
    }

    let conflict_state = &nodes[terminal_node].state;
    let conflict_state_name = state_name(&model.states[conflict_state.control_state]);
    let left_text = format!(
        "{}.{}",
        model.devices[rule.left_device].name,
        model.devices[rule.left_device].states[rule.left_state]
    );
    let right_text = format!(
        "{}.{}",
        model.devices[rule.right_device].name,
        model.devices[rule.right_device].states[rule.right_state]
    );

    match rule.relation {
        SafetyRelation::ConflictsWith => {
            lines.push(format!(
                "在 {conflict_state_name} 检测到冲突：{left_text} 与 {right_text} 同时为真"
            ));
        }
        SafetyRelation::Requires => {
            lines.push(format!(
                "在 {conflict_state_name} 检测到依赖违反：{left_text} 为真但 {right_text} 不为真"
            ));
        }
    }

    lines
}

fn state_name(state: &State) -> String {
    format!("{}.{}", state.task_name, state.step_name)
}



fn relation_text(relation: &SafetyRelation) -> &'static str {
    match relation {
        SafetyRelation::ConflictsWith => "conflicts_with",
        SafetyRelation::Requires => "requires",
    }
}

#[cfg(feature = "z3-solver")]
fn z3_sanity_probe() {
    // Keep a minimal Z3 interaction enabled behind feature-gating so this module
    // can run in toolchains without system cmake/libz3 while still supporting Z3 runs.
    let mut cfg = Config::new();
    cfg.set_model_generation(false);
    let ctx = Context::new(&cfg);
    let solver = Solver::new(&ctx);
    solver.assert(&Bool::from_bool(&ctx, true));
    let _ = solver.check() == SatResult::Sat;
}

#[cfg(test)]
mod tests {
    use super::{SafetyConfig, SafetyProofLevel, verify_safety, verify_safety_with_config};
    use crate::parser::parse_plc;
    use crate::semantic::{build_constraint_set, build_state_machine};

    #[test]
    fn proves_two_cylinder_sequence_without_parallel_conflict() {
        let source = r#"
[topology]

device Y0: digital_output
device Y1: digital_output

device valve_A: solenoid_valve {
    connected_to: Y0
    response_time: 20ms
}

device valve_B: solenoid_valve {
    connected_to: Y1
    response_time: 20ms
}

device cyl_A: cylinder {
    connected_to: valve_A
    stroke_time: 300ms
    retract_time: 300ms
}

device cyl_B: cylinder {
    connected_to: valve_B
    stroke_time: 300ms
    retract_time: 300ms
}

[constraints]

safety: cyl_A.extended conflicts_with cyl_B.extended

[tasks]

task init:
    step extend_A:
        action: extend cyl_A
    step retract_A:
        action: retract cyl_A
    step extend_B:
        action: extend cyl_B
    step retract_B:
        action: retract cyl_B
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let report = verify_safety(&program, &constraints, &state_machine)
            .expect("顺序双气缸逻辑不应违反互斥约束");

        assert!(
            matches!(
                report.level,
                SafetyProofLevel::Complete | SafetyProofLevel::Bounded
            ),
            "验证结果应返回有效级别"
        );
        assert!(report.explored_depth >= state_machine.states.len());
    }

    #[test]
    fn reports_conflict_for_parallel_extend_actions() {
        let source = r#"
[topology]

device Y0: digital_output
device Y1: digital_output

device valve_A: solenoid_valve {
    connected_to: Y0
}

device valve_B: solenoid_valve {
    connected_to: Y1
}

device cyl_A: cylinder {
    connected_to: valve_A
    stroke_time: 200ms
    retract_time: 200ms
}

device cyl_B: cylinder {
    connected_to: valve_B
    stroke_time: 200ms
    retract_time: 200ms
}

[constraints]

safety: cyl_A.extended conflicts_with cyl_B.extended

[tasks]

task parallel_demo:
    step move_together:
        parallel:
            branch_A:
                action: extend cyl_A
            branch_B:
                action: extend cyl_B
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let errors = verify_safety(&program, &constraints, &state_machine)
            .expect_err("并行伸出冲突气缸时应触发 safety 错误");

        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("conflicts_with")),
            "错误应包含冲突约束说明"
        );
        assert!(errors.iter().all(|error| error.line > 0), "错误应携带行号");
    }

    #[test]
    fn uses_scc_size_plus_one_as_default_depth_floor() {
        let source = r#"
[topology]

device Y0: digital_output
device valve_A: solenoid_valve {
    connected_to: Y0
}
device cyl_A: cylinder {
    connected_to: valve_A
    stroke_time: 100ms
    retract_time: 100ms
}

device Y1: digital_output
device valve_B: solenoid_valve {
    connected_to: Y1
}
device cyl_B: cylinder {
    connected_to: valve_B
    stroke_time: 100ms
    retract_time: 100ms
}

[constraints]

safety: cyl_A.extended conflicts_with cyl_B.extended

[tasks]

task init:
    step a:
        action: retract cyl_A
    on_complete: goto loop

task loop:
    step b:
        action: retract cyl_B
    on_complete: goto init
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let report = verify_safety(&program, &constraints, &state_machine)
            .expect("不含冲突动作时 safety 应通过");

        assert!(
            report.explored_depth >= 3,
            "SCC(2节点) 场景默认深度应至少为 |SCC|+1=3"
        );
    }

    #[test]
    fn warns_when_bmc_max_depth_caps_default_search_depth() {
        let source = r#"
[topology]

device Y0: digital_output
device valve_A: solenoid_valve {
    connected_to: Y0
}
device cyl_A: cylinder {
    connected_to: valve_A
    stroke_time: 100ms
    retract_time: 100ms
}

device Y1: digital_output
device valve_B: solenoid_valve {
    connected_to: Y1
}
device cyl_B: cylinder {
    connected_to: valve_B
    stroke_time: 100ms
    retract_time: 100ms
}

[constraints]

safety: cyl_A.extended conflicts_with cyl_B.extended

[tasks]

task init:
    step one:
        action: retract cyl_A
    step two:
        action: retract cyl_B
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let report = verify_safety_with_config(
            &program,
            &constraints,
            &state_machine,
            SafetyConfig {
                bmc_max_depth: Some(1),
            },
        )
        .expect("应返回有界验证结果");

        assert_eq!(report.explored_depth, 1);
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("bmc_max_depth=1")),
            "当用户上限截断默认展开深度时应输出警告"
        );
    }

    #[test]
    fn warns_when_bmc_limit_is_lower_than_scc_requirement() {
        let source = r#"
[topology]

device Y0: digital_output
device valve_A: solenoid_valve {
    connected_to: Y0
}
device cyl_A: cylinder {
    connected_to: valve_A
    stroke_time: 100ms
    retract_time: 100ms
}

device Y1: digital_output
device valve_B: solenoid_valve {
    connected_to: Y1
}
device cyl_B: cylinder {
    connected_to: valve_B
    stroke_time: 100ms
    retract_time: 100ms
}

[constraints]

safety: cyl_A.extended conflicts_with cyl_B.extended

[tasks]

task init:
    step a:
        action: retract cyl_A
    on_complete: goto loop

task loop:
    step b:
        action: retract cyl_B
    on_complete: goto init
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let report = verify_safety_with_config(
            &program,
            &constraints,
            &state_machine,
            SafetyConfig {
                bmc_max_depth: Some(2),
            },
        )
        .expect("应返回有界验证结果");

        assert_eq!(report.explored_depth, 2);
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("SCC")),
            "bmc_max_depth 小于 |SCC|+1 时应输出 SCC 截断警告"
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("WARNING: Safety 在深度 2 内未发现反例")),
            "截断后应输出有界验证警告"
        );
    }

    #[test]
    fn reports_requires_violation_when_press_extends_without_clamp() {
        let source = r#"
[topology]

device Y0: digital_output
device Y1: digital_output

device valve_clamp: solenoid_valve { connected_to: Y0 }
device valve_press: solenoid_valve { connected_to: Y1 }

device cyl_clamp: cylinder {
    connected_to: valve_clamp
    stroke_time: 120ms
    retract_time: 120ms
}

device cyl_press: cylinder {
    connected_to: valve_press
    stroke_time: 140ms
    retract_time: 140ms
}

[constraints]

safety: cyl_press.extended requires cyl_clamp.extended

[tasks]

task press:
    step press_down:
        action: extend cyl_press
    step retract_press:
        action: retract cyl_press
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let errors = verify_safety(&program, &constraints, &state_machine)
            .expect_err("未夹紧即下压时应触发 requires 违反");

        assert!(
            errors
                .iter()
                .any(|error| error.constraint.contains("requires")),
            "错误应包含 requires 约束文本"
        );
        assert!(
            errors
                .iter()
                .any(|error| error.reason.contains("未满足") || error.reason.contains("不为真")),
            "错误原因应说明 requires 前置条件未满足"
        );
        assert!(
            errors
                .iter()
                .any(|error| !error.violation_path.is_empty() && error.line > 0),
            "requires 错误应包含路径和位置"
        );
    }

    #[test]
    fn passes_requires_constraint_when_clamp_precedes_press() {
        let source = r#"
[topology]

device Y0: digital_output
device Y1: digital_output

device valve_clamp: solenoid_valve { connected_to: Y0 }
device valve_press: solenoid_valve { connected_to: Y1 }

device cyl_clamp: cylinder {
    connected_to: valve_clamp
    stroke_time: 120ms
    retract_time: 120ms
}

device cyl_press: cylinder {
    connected_to: valve_press
    stroke_time: 140ms
    retract_time: 140ms
}

[constraints]

safety: cyl_press.extended requires cyl_clamp.extended

[tasks]

task press:
    step clamp:
        action: extend cyl_clamp
    step press_down:
        action: extend cyl_press
    step retract_press:
        action: retract cyl_press
    step release_clamp:
        action: retract cyl_clamp
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let report = verify_safety(&program, &constraints, &state_machine)
            .expect("先夹紧后下压应满足 requires 约束");

        assert!(
            matches!(
                report.level,
                SafetyProofLevel::Complete | SafetyProofLevel::Bounded
            ),
            "requires 满足场景应通过 safety"
        );
    }

    #[test]
    fn handles_and_or_wait_guards_in_bmc_state_exploration() {
        let source = r#"
[topology]

device Y0: digital_output
device Y1: digital_output
device X0: digital_input
device X1: digital_input

device valve_A: solenoid_valve { connected_to: Y0 }
device valve_B: solenoid_valve { connected_to: Y1 }

device cyl_A: cylinder {
    connected_to: valve_A
    stroke_time: 120ms
    retract_time: 120ms
}

device cyl_B: cylinder {
    connected_to: valve_B
    stroke_time: 120ms
    retract_time: 120ms
}

device sensor_A_ext: sensor {
    connected_to: X0
    detects: cyl_A.extended
}

device sensor_B_ext: sensor {
    connected_to: X1
    detects: cyl_B.extended
}

[constraints]

safety: cyl_A.extended conflicts_with cyl_B.extended

[tasks]

task main:
    step move_a:
        action: extend cyl_A
        wait: sensor_A_ext == true AND sensor_B_ext == true
    step return_a:
        action: retract cyl_A
        wait: sensor_A_ext == true OR sensor_B_ext == true
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let report = verify_safety(&program, &constraints, &state_machine)
            .expect("AND/OR wait 守卫不应导致 safety BMC 崩溃或误报");

        assert!(
            matches!(
                report.level,
                SafetyProofLevel::Complete | SafetyProofLevel::Bounded
            ),
            "含 AND/OR wait 的场景应得到有效 safety 结论"
        );
    }

    #[test]
    fn warns_and_downgrades_when_analog_threshold_rules_are_skipped() {
        let source = r#"
[topology]

device pressure_sensor: analog_input { range: 0..100, unit: "bar" }

device Y0: digital_output
device valve_A: solenoid_valve { connected_to: Y0 }
device cyl_A: cylinder {
    connected_to: valve_A
    stroke_time: 120ms
    retract_time: 120ms
}

[constraints]

safety: pressure_sensor > 50 conflicts_with pressure_sensor < 10

[tasks]

task demo:
    step extend:
        action: extend cyl_A
    step retract:
        action: retract cyl_A
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let report = verify_safety(&program, &constraints, &state_machine)
            .expect("含模拟量阈值规则的场景应返回可用 safety 结果");

        assert_eq!(
            report.level,
            SafetyProofLevel::Bounded,
            "跳过阈值规则时 safety 证明等级应降级为有界验证"
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("pressure_sensor > 50 conflicts_with pressure_sensor < 10")),
            "warnings 应记录被跳过的阈值规则"
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("阈值")),
            "warnings 应说明阈值规则被跳过"
        );
    }
}
