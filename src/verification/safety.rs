use crate::ast::{
    ActionStatement, ConditionExpression, DeviceType, LiteralValue, PlcProgram, PortType,
    StepStatement,
    WaitCondition, WaitStatement,
};
use crate::ir::{
    ConstraintSet, SafetyExpr, SafetyRelation, State, StateMachine, Transition, TransitionAction,
    TransitionGuard,
};
use petgraph::algo::kosaraju_scc;
use petgraph::graph::DiGraph;
use petgraph::visit::EdgeRef;
use serde::Serialize;
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SafetyRuleStatusKind {
    Bound,
    Skipped,
    Degraded,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SafetyCoverage {
    pub bound_rules: usize,
    pub degraded_rules: usize,
    pub skipped_rules: usize,
    pub total_rules: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SafetyAnalogThresholdDetail {
    pub expr: String,
    pub device: String,
    pub operator: String,
    pub value: String,
    pub split_points: Vec<f64>,
    pub hit_intervals: usize,
    pub total_intervals: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SafetyRuleStatus {
    pub line: usize,
    pub rule: String,
    pub status: SafetyRuleStatusKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub analog_thresholds: Vec<SafetyAnalogThresholdDetail>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SafetyReport {
    pub level: SafetyProofLevel,
    pub explored_depth: usize,
    pub warnings: Vec<String>,
    pub checked_rules: usize,
    pub skipped_rules: usize,
    pub coverage: SafetyCoverage,
    pub rule_statuses: Vec<SafetyRuleStatus>,
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
    is_analog: bool,
    region_bounds: Option<Vec<(f64, f64)>>,
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
    device_index: HashMap<(String, String), usize>,
    device_state_index: Vec<HashMap<String, usize>>,
    suggested_depth: usize,
    max_scc_depth: usize,
}

#[derive(Debug, Clone)]
struct RuleBinding {
    relation: SafetyRelation,
    left_device: usize,
    left_states: Vec<usize>,
    right_device: usize,
    right_states: Vec<usize>,
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
    let mut rule_statuses = Vec::with_capacity(constraints.safety.len());
    let mut bound_rules = 0usize;
    let mut degraded_rules = 0usize;
    let mut skipped_rules = 0usize;

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

        let analog_thresholds = collect_analog_threshold_details(&model, rule);

        let binding = match bind_safety_expr_rule_with_reason(&model, rule) {
            Ok(binding) => binding,
            Err(reason) => {
                skipped_rules += 1;
                rule_statuses.push(SafetyRuleStatus {
                    line,
                    rule: rule_text,
                    status: SafetyRuleStatusKind::Skipped,
                    reason: Some(reason),
                    analog_thresholds,
                });
                continue;
            }
        };

        checked_rules += 1;

        let outcome = analyze_rule(&model, binding, depth_plan.effective_depth);
        if let Some(counterexample) = outcome.counterexample {
            let (reason, suggestion) = match rule.relation {
                SafetyRelation::ConflictsWith => (
                    format!("{} 与 {} 在可达状态同时成立", left_text, right_text),
                    format!(
                        "请在触发 {} 之前确保 {} 已复位，或调整并行/跳转逻辑避免两者同时成立",
                        right_text, left_text
                    ),
                ),
                SafetyRelation::Requires => (
                    format!("{} 成立时 {} 未满足", left_text, right_text),
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

        let has_threshold = safety_rule_has_threshold(rule);
        let mut status = SafetyRuleStatusKind::Bound;
        let mut reason: Option<String> = None;

        if has_threshold {
            status = SafetyRuleStatusKind::Degraded;
            reason = Some("模拟量阈值采用区间离散抽象（非连续完备模型）".to_string());
        }

        if depth_plan.truncated || !outcome.fully_explored {
            if matches!(status, SafetyRuleStatusKind::Bound) {
                status = SafetyRuleStatusKind::Degraded;
                reason = Some(format!(
                    "有界搜索深度上限导致覆盖不完备（max_depth={}）",
                    depth_plan.effective_depth
                ));
            } else if reason.is_some() {
                // Preserve stable message ordering for CI; append bounded-depth note.
                reason = Some(format!(
                    "{}；有界搜索深度上限（max_depth={}）",
                    reason.unwrap_or_default(),
                    depth_plan.effective_depth
                ));
            }
        }

        match status {
            SafetyRuleStatusKind::Bound => bound_rules += 1,
            SafetyRuleStatusKind::Degraded => degraded_rules += 1,
            SafetyRuleStatusKind::Skipped => skipped_rules += 1,
        }

        rule_statuses.push(SafetyRuleStatus {
            line,
            rule: rule_text,
            status,
            reason,
            analog_thresholds,
        });

        if depth_plan.truncated || !outcome.fully_explored || has_threshold {
            all_complete = false;
        }
    }

    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let mut warnings = depth_plan.warnings;
    let total_rules = constraints.safety.len();
    let skipped_rules_count = total_rules.saturating_sub(checked_rules).max(skipped_rules);

    for status in &rule_statuses {
        if matches!(status.status, SafetyRuleStatusKind::Skipped) {
            warnings.push(format!(
                "WARNING: Safety 规则 <input>:{} {} 已跳过：{}",
                status.line,
                status.rule,
                status.reason.as_deref().unwrap_or("无可用建模")
            ));
        }
    }

    let coverage = SafetyCoverage {
        bound_rules,
        degraded_rules,
        skipped_rules: skipped_rules_count,
        total_rules,
    };

    let level = if degraded_rules == 0
        && skipped_rules_count == 0
        && (checked_rules == 0 || all_complete)
    {
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
        checked_rules,
        skipped_rules: skipped_rules_count,
        coverage,
        rule_statuses,
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
            collect_device_domains(program, constraints, state_machine);

        let mut edges = Vec::new();
        let mut outgoing = vec![Vec::new(); states.len()];

        let analog_inputs = collect_analog_input_states(program, &device_index, &devices);

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

            let effects =
                transition_effects(transition, &device_index, &device_state_index, &devices);
            let expanded_effects = expand_analog_input_effects(effects, &analog_inputs);
            let label = transition_label(transition);

            for effects in expanded_effects {
                let edge_index = edges.len();
                edges.push(ModelEdge {
                    from,
                    to,
                    effects,
                    label: label.clone(),
                });
                outgoing[from].push(edge_index);
            }
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

fn analog_region_state_name(index: usize) -> String {
    format!("region_{index}")
}

fn compute_analog_regions(
    program: &PlcProgram,
    constraints: &ConstraintSet,
) -> HashMap<String, Vec<(f64, f64)>> {
    let mut values_by_device: HashMap<String, Vec<f64>> = HashMap::new();

    for rule in &constraints.safety {
        for expr in [&rule.left, &rule.right] {
            if let SafetyExpr::Threshold { device, value, .. } = expr {
                if let Ok(parsed) = value.parse::<f64>() {
                    add_threshold_value(&mut values_by_device, device, parsed);
                }
            }
        }
    }

    for task in &program.tasks.tasks {
        for step in &task.steps {
            collect_threshold_values_from_statements(&step.statements, &mut values_by_device);
        }
    }

    let mut regions_by_device = HashMap::new();

    for device in &program.topology.devices {
        if !matches!(
            device.device_type,
            DeviceType::AnalogInput | DeviceType::AnalogOutput
        ) {
            continue;
        }

        let Some(range) = &device.attributes.range else {
            continue;
        };

        let (min, max) = if range.min <= range.max {
            (range.min, range.max)
        } else {
            (range.max, range.min)
        };

        let mut bounds = vec![min, max];
        if let Some(values) = values_by_device.get(&device.name) {
            for value in values {
                if *value >= min && *value <= max {
                    bounds.push(*value);
                }
            }
        }

        bounds.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        bounds.dedup_by(|a, b| (*a - *b).abs() <= f64::EPSILON);

        if bounds.len() < 2 {
            bounds.push(max);
        }

        let mut regions = Vec::new();
        for window in bounds.windows(2) {
            regions.push((window[0], window[1]));
        }

        if regions.is_empty() {
            regions.push((min, max));
        }

        regions_by_device.insert(device.name.clone(), regions);
    }

    for (target, values) in values_by_device {
        if regions_by_device.contains_key(&target) {
            continue;
        }
        let Some((device, port)) = split_device_port_ref(&target) else {
            continue;
        };
        if !is_analog_port_target(program, device, port) {
            continue;
        }
        regions_by_device.insert(target, synthetic_regions_from_threshold_values(&values));
    }

    regions_by_device
}

fn split_device_port_ref(target: &str) -> Option<(&str, &str)> {
    let mut parts = target.split('.');
    let device = parts.next()?.trim();
    let port = parts.next()?.trim();
    if device.is_empty() || port.is_empty() || parts.next().is_some() {
        return None;
    }
    Some((device, port))
}

fn is_analog_port_target(program: &PlcProgram, device: &str, port: &str) -> bool {
    let Some(decl) = program.topology.devices.iter().find(|entry| entry.name == device) else {
        return false;
    };

    if let Some(explicit_port) = decl.attributes.ports.iter().find(|entry| entry.id == port) {
        return matches!(explicit_port.port_type, PortType::Analog);
    }

    default_analog_port_for_device_type(&decl.device_type, port)
}

fn default_analog_port_for_device_type(device_type: &DeviceType, port: &str) -> bool {
    match device_type {
        DeviceType::CamCoupling => matches!(port, "following_error" | "master_pos" | "slave_cmd"),
        DeviceType::AnalogInput => port == "in",
        DeviceType::AnalogOutput => port == "out",
        DeviceType::Pid => matches!(port, "in" | "out"),
        _ => false,
    }
}

fn synthetic_regions_from_threshold_values(values: &[f64]) -> Vec<(f64, f64)> {
    if values.is_empty() {
        return vec![(0.0, 1.0)];
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    sorted.dedup_by(|a, b| (*a - *b).abs() <= f64::EPSILON);

    let min = sorted[0];
    let max = *sorted.last().unwrap_or(&min);
    let span = (max - min).abs();
    let pad = if span > f64::EPSILON {
        span
    } else {
        max.abs().max(1.0)
    };

    let mut bounds = vec![min - pad, max + pad];
    bounds.extend(sorted);
    bounds.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    bounds.dedup_by(|a, b| (*a - *b).abs() <= f64::EPSILON);

    let mut regions = Vec::new();
    for window in bounds.windows(2) {
        regions.push((window[0], window[1]));
    }
    if regions.is_empty() {
        regions.push((min - pad, max + pad));
    }
    regions
}

fn add_threshold_value(values_by_device: &mut HashMap<String, Vec<f64>>, device: &str, value: f64) {
    values_by_device
        .entry(device.to_string())
        .or_default()
        .push(value);
}

fn collect_threshold_values_from_statements(
    statements: &[StepStatement],
    values_by_device: &mut HashMap<String, Vec<f64>>,
) {
    for statement in statements {
        match statement {
            StepStatement::Wait(wait) => {
                collect_threshold_values_from_wait(wait, values_by_device);
            }
            StepStatement::Action(ActionStatement::SetAnalog { target, value }) => {
                add_threshold_value(values_by_device, &target.device, *value);
            }
            StepStatement::Action(ActionStatement::SetAnalogExpr { .. })
            | StepStatement::Action(ActionStatement::Compute { .. }) => {}
            StepStatement::Repeat { body, .. } => {
                collect_threshold_values_from_statements(body, values_by_device);
            }
            StepStatement::Parallel(parallel) => {
                for branch in &parallel.branches {
                    collect_threshold_values_from_statements(&branch.statements, values_by_device);
                }
            }
            StepStatement::Race(race) => {
                for branch in &race.branches {
                    collect_threshold_values_from_statements(&branch.statements, values_by_device);
                }
            }
            _ => {}
        }
    }
}

fn collect_threshold_values_from_wait(
    wait: &WaitStatement,
    values_by_device: &mut HashMap<String, Vec<f64>>,
) {
    let terms: Vec<&ConditionExpression> = match &wait.condition {
        WaitCondition::Single(condition) => vec![condition],
        WaitCondition::And(conditions) | WaitCondition::Or(conditions) => {
            conditions.iter().collect()
        }
    };

    for condition in terms {
        if condition.is_expression_compare() {
            continue;
        }
        if let LiteralValue::Number(value) = &condition.right {
            add_threshold_value(values_by_device, &condition.left, *value);
        }
        if let LiteralValue::Measured(measured) = &condition.right {
            add_threshold_value(values_by_device, &condition.left, measured.value);
        }
    }
}

fn collect_device_domains(
    program: &PlcProgram,
    constraints: &ConstraintSet,
    state_machine: &StateMachine,
) -> (
    Vec<DeviceDomain>,
    HashMap<(String, String), usize>,
    Vec<HashMap<String, usize>>,
) {
    let analog_regions = compute_analog_regions(program, constraints);
    let mut devices = Vec::<DeviceDomain>::new();
    let mut device_index = HashMap::<(String, String), usize>::new();

    for device in &program.topology.devices {
        let (states, default_state, is_analog, region_bounds) = match device.device_type {
            DeviceType::Cylinder => {
                let states = vec!["extended".to_string(), "retracted".to_string()];
                let default_state = states
                    .iter()
                    .position(|state| state == "retracted")
                    .unwrap_or(0);
                (states, default_state, false, None)
            }
            DeviceType::DigitalOutput
            | DeviceType::DigitalInput
            | DeviceType::Plc
            | DeviceType::SolenoidValve
            | DeviceType::Sensor
            | DeviceType::Motor
            | DeviceType::StepperMotor
            | DeviceType::Vfd
            | DeviceType::ServoDrive
            | DeviceType::CamCoupling
            | DeviceType::Pid => {
                let states = vec!["on".to_string(), "off".to_string()];
                let default_state = states.iter().position(|state| state == "off").unwrap_or(0);
                (states, default_state, false, None)
            }
            DeviceType::AnalogInput | DeviceType::AnalogOutput => {
                let regions = analog_regions.get(&device.name);
                if let Some(regions) = regions {
                    let states = regions
                        .iter()
                        .enumerate()
                        .map(|(index, _)| analog_region_state_name(index))
                        .collect::<Vec<_>>();
                    (states, 0, true, Some(regions.clone()))
                } else {
                    let states = vec!["analog_active".to_string()];
                    (states, 0, true, None)
                }
            }
        };

        let index = devices.len();
        devices.push(DeviceDomain {
            name: device.name.clone(),
            states,
            default_state,
            is_analog,
            region_bounds,
        });
        device_index.insert((device.name.clone(), "self".to_string()), index);
    }

    let mut referenced_ports = HashMap::<String, Vec<String>>::new();
    for rule in &constraints.safety {
        if let SafetyExpr::State(ref expr) = rule.left {
            if expr.port != "self" {
                referenced_ports
                    .entry(expr.device.clone())
                    .or_default()
                    .push(expr.port.clone());
            }
            if let Some(left_device) =
                lookup_device_domain_id(&device_index, &expr.device, &expr.port, false)
            {
                ensure_device_state(&mut devices[left_device], &expr.state);
            }
        } else if let SafetyExpr::Threshold { device, .. } = &rule.left {
            let (device_name, port_name) = split_threshold_target(device);
            if port_name != "self" {
                referenced_ports
                    .entry(device_name.to_string())
                    .or_default()
                    .push(port_name.to_string());
            }
        }

        if let SafetyExpr::State(ref expr) = rule.right {
            if expr.port != "self" {
                referenced_ports
                    .entry(expr.device.clone())
                    .or_default()
                    .push(expr.port.clone());
            }
            if let Some(right_device) =
                lookup_device_domain_id(&device_index, &expr.device, &expr.port, false)
            {
                ensure_device_state(&mut devices[right_device], &expr.state);
            }
        } else if let SafetyExpr::Threshold { device, .. } = &rule.right {
            let (device_name, port_name) = split_threshold_target(device);
            if port_name != "self" {
                referenced_ports
                    .entry(device_name.to_string())
                    .or_default()
                    .push(port_name.to_string());
            }
        }
    }

    for transition in &state_machine.transitions {
        for action in &transition.actions {
            let (target, port) = match action {
                TransitionAction::Extend { target, port }
                | TransitionAction::Retract { target, port }
                | TransitionAction::Set { target, port, .. }
                | TransitionAction::SetAnalog { target, port, .. }
                | TransitionAction::SetAnalogExpr { target, port, .. } => (target, port),
                TransitionAction::CamEngage { .. }
                | TransitionAction::CamDisengage { .. }
                | TransitionAction::CamSwitch { .. }
                | TransitionAction::CamPhase { .. } => continue,
                TransitionAction::Compute { .. } | TransitionAction::Log { .. } => continue,
            };
            if port != "self" {
                referenced_ports
                    .entry(target.clone())
                    .or_default()
                    .push(port.clone());
            }
        }
    }

    for device in &program.topology.devices {
        let mut ports = referenced_ports.remove(&device.name).unwrap_or_default();
        for port in &device.attributes.ports {
            ports.push(port.id.clone());
        }

        ports.sort();
        ports.dedup();
        ports.retain(|port| port != "self");

        for port in ports {
            if device_index.contains_key(&(device.name.clone(), port.clone())) {
                continue;
            }

            let declared_port = device
                .attributes
                .ports
                .iter()
                .find(|candidate| candidate.id == port);

            let is_analog = declared_port
                .map(|candidate| matches!(candidate.port_type, PortType::Analog))
                .unwrap_or_else(|| default_analog_port_for_device_type(&device.device_type, &port));
            let display_name = format!("{}.{}", device.name, port);
            let region_bounds = if is_analog {
                analog_regions.get(&display_name).cloned()
            } else {
                None
            };
            let states = if let Some(bounds) = &region_bounds {
                bounds
                    .iter()
                    .enumerate()
                    .map(|(index, _)| analog_region_state_name(index))
                    .collect::<Vec<_>>()
            } else {
                let mut out = declared_port
                    .map(|candidate| candidate.states.clone())
                    .unwrap_or_default();
                if out.is_empty() {
                    out = if is_analog {
                        vec!["analog_active".to_string()]
                    } else {
                        inferred_states_for_port(&port)
                    };
                }
                out
            };

            let mut default_state = 0usize;
            if region_bounds.is_none() {
                let default_state_name = declared_port
                    .and_then(|candidate| {
                        if candidate.default_state.is_empty() {
                            None
                        } else {
                            Some(candidate.default_state.clone())
                        }
                    })
                    .or_else(|| inferred_default_state_for_port(&states));
                if let Some(name) = default_state_name.as_deref() {
                    if let Some(idx) = states.iter().position(|state| state == name) {
                        default_state = idx;
                    }
                } else if let Some(idx) = states.iter().position(|state| state == "off") {
                    default_state = idx;
                }
            }

            let index = devices.len();
            devices.push(DeviceDomain {
                name: display_name,
                states,
                default_state,
                is_analog,
                region_bounds,
            });
            device_index.insert((device.name.clone(), port), index);
        }
    }

    for rule in &constraints.safety {
        if let SafetyExpr::State(ref expr) = rule.left
            && let Some(left_device) =
                lookup_device_domain_id(&device_index, &expr.device, &expr.port, false)
        {
            ensure_device_state(&mut devices[left_device], &expr.state);
        }

        if let SafetyExpr::State(ref expr) = rule.right
            && let Some(right_device) =
                lookup_device_domain_id(&device_index, &expr.device, &expr.port, false)
        {
            ensure_device_state(&mut devices[right_device], &expr.state);
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

fn collect_analog_input_states(
    program: &PlcProgram,
    device_index: &HashMap<(String, String), usize>,
    devices: &[DeviceDomain],
) -> Vec<(usize, Vec<usize>)> {
    let mut inputs = Vec::new();

    for device in &program.topology.devices {
        if !matches!(device.device_type, DeviceType::AnalogInput) {
            continue;
        }

        let Some(device_id) = lookup_device_domain_id(device_index, &device.name, "self", false)
        else {
            continue;
        };

        let state_count = devices
            .get(device_id)
            .map(|domain| domain.states.len())
            .unwrap_or(0);

        if state_count == 0 {
            continue;
        }

        let states = (0..state_count).collect::<Vec<_>>();
        inputs.push((device_id, states));
    }

    inputs
}

fn expand_analog_input_effects(
    base_effects: HashMap<usize, usize>,
    analog_inputs: &[(usize, Vec<usize>)],
) -> Vec<HashMap<usize, usize>> {
    let mut expanded = vec![base_effects];

    for (device_id, states) in analog_inputs {
        if states.is_empty() {
            continue;
        }

        let mut next = Vec::new();
        for effects in expanded {
            if effects.contains_key(device_id) {
                next.push(effects);
                continue;
            }

            for state_id in states {
                let mut cloned = effects.clone();
                cloned.insert(*device_id, *state_id);
                next.push(cloned);
            }
        }
        expanded = next;
    }

    expanded
}

fn ensure_device_state(domain: &mut DeviceDomain, state_name: &str) {
    if domain.states.iter().any(|state| state == state_name) {
        return;
    }

    domain.states.push(state_name.to_string());
}

fn transition_effects(
    transition: &Transition,
    device_index: &HashMap<(String, String), usize>,
    device_state_index: &[HashMap<String, usize>],
    device_domains: &[DeviceDomain],
) -> HashMap<usize, usize> {
    let mut effects = HashMap::<usize, usize>::new();

    for action in &transition.actions {
        match action {
            TransitionAction::SetAnalog {
                target,
                port,
                value_raw,
            } => {
                let Some(device_id) = lookup_device_domain_id(device_index, target, port, true)
                else {
                    continue;
                };
                let Some(state_id) = analog_state_for_value(device_domains, device_id, value_raw)
                else {
                    continue;
                };
                effects.insert(device_id, state_id);
            }
            TransitionAction::SetAnalogExpr { .. } => {}
            TransitionAction::Set {
                target,
                port,
                value,
            } => {
                let Some(device_id) = lookup_device_domain_id(device_index, target, port, true)
                else {
                    continue;
                };
                let Some(state_id) = binary_state_for_domain(
                    &device_domains[device_id],
                    &device_state_index[device_id],
                    value,
                ) else {
                    continue;
                };
                effects.insert(device_id, state_id);
            }
            TransitionAction::Compute { .. } => {}
            TransitionAction::CamEngage { .. }
            | TransitionAction::CamDisengage { .. }
            | TransitionAction::CamSwitch { .. }
            | TransitionAction::CamPhase { .. } => {}
            TransitionAction::Extend { target, port } => {
                let Some(device_id) = lookup_device_domain_id(device_index, target, port, true)
                else {
                    continue;
                };
                let Some(state_id) = device_state_index[device_id].get("extended").copied() else {
                    continue;
                };
                effects.insert(device_id, state_id);
            }
            TransitionAction::Retract { target, port } => {
                let Some(device_id) = lookup_device_domain_id(device_index, target, port, true)
                else {
                    continue;
                };
                let Some(state_id) = device_state_index[device_id].get("retracted").copied() else {
                    continue;
                };
                effects.insert(device_id, state_id);
            }
            TransitionAction::Log { .. } => {}
        }
    }

    effects
}

fn lookup_device_domain_id(
    device_index: &HashMap<(String, String), usize>,
    device: &str,
    port: &str,
    allow_self_fallback: bool,
) -> Option<usize> {
    if let Some(id) = device_index.get(&(device.to_string(), port.to_string())).copied() {
        return Some(id);
    }
    if allow_self_fallback && port != "self" {
        return device_index
            .get(&(device.to_string(), "self".to_string()))
            .copied();
    }
    None
}

fn inferred_states_for_port(port: &str) -> Vec<String> {
    let lowered = port.to_ascii_lowercase();
    if lowered.contains("direction") || lowered.ends_with("_dir") || lowered == "dir" {
        return vec!["forward".to_string(), "reverse".to_string()];
    }
    if lowered.contains("pulse") {
        return vec!["active".to_string(), "idle".to_string()];
    }
    vec!["on".to_string(), "off".to_string()]
}

fn inferred_default_state_for_port(states: &[String]) -> Option<String> {
    for candidate in ["off", "idle", "retracted", "reverse"] {
        if states.iter().any(|state| state == candidate) {
            return Some(candidate.to_string());
        }
    }
    states.first().cloned()
}

fn binary_state_for_domain(
    domain: &DeviceDomain,
    state_index: &HashMap<String, usize>,
    value: &crate::ir::BinaryValue,
) -> Option<usize> {
    let candidates = match value {
        crate::ir::BinaryValue::On => ["on", "forward", "active", "extended"],
        crate::ir::BinaryValue::Off => ["off", "reverse", "idle", "retracted"],
    };

    for candidate in candidates {
        if let Some(state_id) = state_index.get(candidate).copied() {
            return Some(state_id);
        }
    }

    if domain.states.len() == 2 {
        return Some(match value {
            crate::ir::BinaryValue::On => {
                if domain.default_state == 0 {
                    1
                } else {
                    0
                }
            }
            crate::ir::BinaryValue::Off => domain.default_state.min(1),
        });
    }

    if domain.states.len() == 1 {
        return Some(0);
    }

    None
}

fn analog_state_for_value(
    device_domains: &[DeviceDomain],
    device_id: usize,
    value_raw: &str,
) -> Option<usize> {
    let domain = device_domains.get(device_id)?;
    if !domain.is_analog {
        return None;
    }
    let bounds = domain.region_bounds.as_ref()?;
    let value = value_raw.parse::<f64>().ok()?;

    bounds.iter().enumerate().find_map(|(index, (min, max))| {
        if value >= *min && value <= *max {
            Some(index)
        } else {
            None
        }
    })
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
        TransitionAction::Extend { target, .. } => Some(format!("extend {target}")),
        TransitionAction::Retract { target, .. } => Some(format!("retract {target}")),
        TransitionAction::Set { target, value, .. } => Some(format!(
            "set {} {}",
            target,
            match value {
                crate::ir::BinaryValue::On => "on",
                crate::ir::BinaryValue::Off => "off",
            }
        )),
        TransitionAction::SetAnalog { target, value_raw, .. } => {
            Some(format!("set_analog {target} {value_raw}"))
        }
        TransitionAction::SetAnalogExpr {
            target, expr_raw, ..
        } => Some(format!("set_analog {target} {expr_raw}")),
        TransitionAction::Compute { target, expr_raw } => Some(format!("compute {target}={expr_raw}")),
        TransitionAction::CamEngage { target } => Some(format!("cam_engage {target}")),
        TransitionAction::CamDisengage { target } => Some(format!("cam_disengage {target}")),
        TransitionAction::CamSwitch { target, new_table } => {
            Some(format!("cam_switch {target} {new_table}"))
        }
        TransitionAction::CamPhase {
            target,
            offset_expr_raw,
        } => Some(format!("cam_phase {target} {offset_expr_raw}")),
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

fn bind_safety_expr_rule_with_reason(
    model: &SafetyModel,
    rule: &crate::ir::SafetyRule,
) -> Result<RuleBinding, String> {
    let (left_device, left_states) =
        safety_expr_states_with_reason(model, &rule.left).map_err(|r| format!("左侧：{r}"))?;
    let (right_device, right_states) =
        safety_expr_states_with_reason(model, &rule.right).map_err(|r| format!("右侧：{r}"))?;

    Ok(RuleBinding {
        relation: rule.relation.clone(),
        left_device,
        left_states,
        right_device,
        right_states,
    })
}

fn safety_expr_states_with_reason(
    model: &SafetyModel,
    expr: &SafetyExpr,
) -> Result<(usize, Vec<usize>), String> {
    match expr {
        SafetyExpr::State(state_expr) => {
            let device_id = lookup_device_domain_id(
                &model.device_index,
                &state_expr.device,
                &state_expr.port,
                false,
            )
            .ok_or_else(|| {
                if state_expr.port == "self" {
                    format!("未知设备 {}", state_expr.device)
                } else {
                    format!("未知设备端口 {}.{}", state_expr.device, state_expr.port)
                }
            })?;
            let state_id = model.device_state_index[device_id]
                .get(&state_expr.state)
                .copied()
                .ok_or_else(|| {
                    if state_expr.port == "self" {
                        format!("设备 {} 不存在状态 {}", state_expr.device, state_expr.state)
                    } else {
                        format!(
                            "设备端口 {}.{} 不存在状态 {}",
                            state_expr.device, state_expr.port, state_expr.state
                        )
                    }
                })?;
            Ok((device_id, vec![state_id]))
        }
        SafetyExpr::Threshold {
            device,
            operator,
            value,
        } => {
            let (device_name, port_name) = split_threshold_target(device);
            let device_id =
                lookup_device_domain_id(&model.device_index, device_name, port_name, false)
                    .ok_or_else(|| {
                        if port_name == "self" {
                            format!("未知设备 {device_name}")
                        } else {
                            format!("未知设备端口 {device_name}.{port_name}")
                        }
                    })?;
            let domain = model
                .devices
                .get(device_id)
                .ok_or_else(|| format!("内部错误：设备 {device_name} 未注册"))?;
            if !domain.is_analog {
                return Err(format!("设备 {device} 非模拟量设备，无法进行阈值建模"));
            }
            if domain.region_bounds.is_none() {
                return Err(format!("设备 {device} 缺少 range，无法进行区间离散建模"));
            }
            if comparison_op_from_str(operator).is_none() {
                return Err(format!("不支持的比较运算符 {operator}"));
            }
            if value.parse::<f64>().is_err() {
                return Err(format!("阈值值无法解析为数字：{value}"));
            }
            let states =
                threshold_states_for_expr(model, device_id, operator, value).ok_or_else(|| {
                    format!("无法将阈值表达式映射到离散区间：{device} {operator} {value}")
                })?;
            Ok((device_id, states))
        }
    }
}

#[derive(Clone, Copy)]
enum ComparisonOp {
    Eq,
    Neq,
    Gt,
    Lt,
    Gte,
    Lte,
}

fn comparison_op_from_str(op: &str) -> Option<ComparisonOp> {
    match op {
        "==" => Some(ComparisonOp::Eq),
        "!=" => Some(ComparisonOp::Neq),
        ">" => Some(ComparisonOp::Gt),
        "<" => Some(ComparisonOp::Lt),
        ">=" => Some(ComparisonOp::Gte),
        "<=" => Some(ComparisonOp::Lte),
        _ => None,
    }
}

fn threshold_states_for_expr(
    model: &SafetyModel,
    device_id: usize,
    operator: &str,
    value: &str,
) -> Option<Vec<usize>> {
    let domain = model.devices.get(device_id)?;
    let bounds = domain.region_bounds.as_ref()?;
    let op = comparison_op_from_str(operator)?;
    let value = value.parse::<f64>().ok()?;

    let mut states = Vec::new();
    for (index, (min, max)) in bounds.iter().enumerate() {
        if region_intersects(op, value, *min, *max) {
            states.push(index);
        }
    }

    Some(states)
}

fn region_intersects(op: ComparisonOp, value: f64, min: f64, max: f64) -> bool {
    match op {
        ComparisonOp::Eq => value >= min && value <= max,
        ComparisonOp::Neq => !(min == max && value == min),
        ComparisonOp::Gt => max > value,
        ComparisonOp::Gte => max >= value,
        ComparisonOp::Lt => min < value,
        ComparisonOp::Lte => min <= value,
    }
}

fn safety_expr_text(expr: &SafetyExpr) -> String {
    match expr {
        SafetyExpr::State(state_expr) => {
            if state_expr.port == "self" {
                format!("{}.{}", state_expr.device, state_expr.state)
            } else {
                format!(
                    "{}.{}.{}",
                    state_expr.device, state_expr.port, state_expr.state
                )
            }
        }
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

fn collect_analog_threshold_details(
    model: &SafetyModel,
    rule: &crate::ir::SafetyRule,
) -> Vec<SafetyAnalogThresholdDetail> {
    let mut out = Vec::new();
    for expr in [&rule.left, &rule.right] {
        let SafetyExpr::Threshold {
            device,
            operator,
            value,
        } = expr
        else {
            continue;
        };

        let (device_name, port_name) = split_threshold_target(device);
        let Some(device_id) =
            lookup_device_domain_id(&model.device_index, device_name, port_name, false)
        else {
            continue;
        };
        let Some(domain) = model.devices.get(device_id) else {
            continue;
        };
        if !domain.is_analog {
            continue;
        }
        let Some(bounds) = domain.region_bounds.as_ref() else {
            continue;
        };
        let split_points = split_points_from_region_bounds(bounds);
        let hit_intervals = threshold_states_for_expr(model, device_id, operator, value)
            .map(|states| states.len())
            .unwrap_or(0);
        out.push(SafetyAnalogThresholdDetail {
            expr: safety_expr_text(expr),
            device: device.clone(),
            operator: operator.clone(),
            value: value.clone(),
            split_points,
            hit_intervals,
            total_intervals: bounds.len(),
        });
    }
    out
}

fn split_threshold_target(device_ref: &str) -> (&str, &str) {
    let mut parts = device_ref.split('.');
    let Some(device) = parts.next() else {
        return (device_ref, "self");
    };
    let Some(port) = parts.next() else {
        return (device_ref, "self");
    };
    if parts.next().is_some() {
        return (device_ref, "self");
    }
    (device, port)
}

fn split_points_from_region_bounds(bounds: &[(f64, f64)]) -> Vec<f64> {
    if bounds.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(bounds.len() + 1);
    out.push(bounds[0].0);
    for (_, max) in bounds {
        out.push(*max);
    }
    out
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
    let left_state = state.device_states[rule.left_device];
    let right_state = state.device_states[rule.right_device];
    let left_matches = rule.left_states.contains(&left_state);
    let right_matches = rule.right_states.contains(&right_state);

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
    let left_state_id = conflict_state.device_states[rule.left_device];
    let right_state_id = conflict_state.device_states[rule.right_device];
    let left_text = format!(
        "{}.{}",
        model.devices[rule.left_device].name, model.devices[rule.left_device].states[left_state_id]
    );
    let right_text = format!(
        "{}.{}",
        model.devices[rule.right_device].name,
        model.devices[rule.right_device].states[right_state_id]
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
    use super::{
        SafetyConfig, SafetyModel, SafetyProofLevel, SafetyRuleStatusKind, analog_state_for_value,
        verify_safety, verify_safety_with_config,
    };
    use crate::ir::{SafetyExpr, SafetyRelation, SafetyRule, StateExpr};
    use crate::parser::parse_plc;
    use crate::semantic::{build_constraint_set, build_state_machine};

    #[test]
    fn proves_two_cylinder_sequence_without_parallel_conflict() {
        let source = r#"
[topology]

device Y0: digital_output
device Y1: digital_output

device valve_A: solenoid_valve {
    response_time: 20ms
}

device valve_B: solenoid_valve {
    response_time: 20ms
}

device cyl_A: cylinder {
    stroke_time: 300ms
    retract_time: 300ms
}

device cyl_B: cylinder {
    stroke_time: 300ms
    retract_time: 300ms
}

relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: Y1.out, to: valve_B.coil, via: driven_by }
relation { from: valve_B.out, to: cyl_B.cmd, via: driven_by }

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

device valve_A: solenoid_valve
device valve_B: solenoid_valve

device cyl_A: cylinder {
    stroke_time: 200ms
    retract_time: 200ms
}

device cyl_B: cylinder {
    stroke_time: 200ms
    retract_time: 200ms
}

relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: Y1.out, to: valve_B.coil, via: driven_by }
relation { from: valve_B.out, to: cyl_B.cmd, via: driven_by }

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
device valve_A: solenoid_valve
device cyl_A: cylinder {
    stroke_time: 100ms
    retract_time: 100ms
}

device Y1: digital_output
device valve_B: solenoid_valve
device cyl_B: cylinder {
    stroke_time: 100ms
    retract_time: 100ms
}

relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: Y1.out, to: valve_B.coil, via: driven_by }
relation { from: valve_B.out, to: cyl_B.cmd, via: driven_by }

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
device valve_A: solenoid_valve
device cyl_A: cylinder {
    stroke_time: 100ms
    retract_time: 100ms
}

device Y1: digital_output
device valve_B: solenoid_valve
device cyl_B: cylinder {
    stroke_time: 100ms
    retract_time: 100ms
}

relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: Y1.out, to: valve_B.coil, via: driven_by }
relation { from: valve_B.out, to: cyl_B.cmd, via: driven_by }

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
device valve_A: solenoid_valve
device cyl_A: cylinder {
    stroke_time: 100ms
    retract_time: 100ms
}

device Y1: digital_output
device valve_B: solenoid_valve
device cyl_B: cylinder {
    stroke_time: 100ms
    retract_time: 100ms
}

relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: Y1.out, to: valve_B.coil, via: driven_by }
relation { from: valve_B.out, to: cyl_B.cmd, via: driven_by }

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

device valve_clamp: solenoid_valve
device valve_press: solenoid_valve

device cyl_clamp: cylinder {
    stroke_time: 120ms
    retract_time: 120ms
}

device cyl_press: cylinder {
    stroke_time: 140ms
    retract_time: 140ms
}

relation { from: Y0.out, to: valve_clamp.coil, via: driven_by }
relation { from: valve_clamp.out, to: cyl_clamp.cmd, via: driven_by }
relation { from: Y1.out, to: valve_press.coil, via: driven_by }
relation { from: valve_press.out, to: cyl_press.cmd, via: driven_by }

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

device valve_clamp: solenoid_valve
device valve_press: solenoid_valve

device cyl_clamp: cylinder {
    stroke_time: 120ms
    retract_time: 120ms
}

device cyl_press: cylinder {
    stroke_time: 140ms
    retract_time: 140ms
}

relation { from: Y0.out, to: valve_clamp.coil, via: driven_by }
relation { from: valve_clamp.out, to: cyl_clamp.cmd, via: driven_by }
relation { from: Y1.out, to: valve_press.coil, via: driven_by }
relation { from: valve_press.out, to: cyl_press.cmd, via: driven_by }

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
    fn reports_rule_statuses_and_coverage_for_all_bound_rules() {
        let source = r#"
[topology]

device out_a: digital_output
device out_b: digital_output

[constraints]

safety: out_a.on conflicts_with out_b.on
safety: out_a.on requires out_a.on

[tasks]

task main:
    step s1:
        action: log "tick"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let report = verify_safety(&program, &constraints, &state_machine)
            .expect("无动作变更场景不应违反安全约束");

        assert_eq!(report.coverage.total_rules, 2);
        assert_eq!(report.coverage.bound_rules, 2);
        assert_eq!(report.coverage.degraded_rules, 0);
        assert_eq!(report.coverage.skipped_rules, 0);
        assert_eq!(report.rule_statuses.len(), 2);
        assert!(
            report
                .rule_statuses
                .iter()
                .all(|status| matches!(status.status, SafetyRuleStatusKind::Bound))
        );
        assert!(report.rule_statuses.iter().all(|s| s.reason.is_none()));
    }

    #[test]
    fn reports_rule_statuses_and_coverage_with_skipped_rule() {
        let source = r#"
[topology]

device out_a: digital_output
device out_b: digital_output

[constraints]

safety: out_a.on conflicts_with out_b.on

[tasks]

task main:
    step s1:
        action: log "tick"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let mut constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        constraints.safety.push(SafetyRule {
            left: SafetyExpr::State(StateExpr {
                device: "unknown_device".to_string(),
                state: "on".to_string(),
                port: String::new(),
            }),
            relation: SafetyRelation::ConflictsWith,
            right: SafetyExpr::State(StateExpr {
                device: "out_a".to_string(),
                state: "on".to_string(),
                port: String::new(),
            }),
            reason: None,
            source: None,
        });

        let report = verify_safety(&program, &constraints, &state_machine)
            .expect("跳过绑定失败规则时应仍返回可用安全报告");

        assert_eq!(report.coverage.total_rules, 2);
        assert_eq!(report.coverage.bound_rules, 1);
        assert_eq!(report.coverage.skipped_rules, 1);
        assert!(
            report
                .rule_statuses
                .iter()
                .any(|status| matches!(status.status, SafetyRuleStatusKind::Skipped))
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("已跳过")),
            "跳过规则时应输出可读告警"
        );
    }

    #[test]
    fn reports_rule_statuses_and_coverage_when_all_rules_skipped() {
        let source = r#"
[topology]

device out_a: digital_output

[constraints]

[tasks]

task main:
    step s1:
        action: log "tick"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let mut constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        constraints.safety.push(SafetyRule {
            left: SafetyExpr::State(StateExpr {
                device: "unknown_device".to_string(),
                state: "on".to_string(),
                port: String::new(),
            }),
            relation: SafetyRelation::ConflictsWith,
            right: SafetyExpr::State(StateExpr {
                device: "out_a".to_string(),
                state: "on".to_string(),
                port: String::new(),
            }),
            reason: None,
            source: None,
        });
        constraints.safety.push(SafetyRule {
            left: SafetyExpr::State(StateExpr {
                device: "unknown_device_2".to_string(),
                state: "on".to_string(),
                port: String::new(),
            }),
            relation: SafetyRelation::Requires,
            right: SafetyExpr::State(StateExpr {
                device: "out_a".to_string(),
                state: "on".to_string(),
                port: String::new(),
            }),
            reason: None,
            source: None,
        });

        let report = verify_safety(&program, &constraints, &state_machine)
            .expect("全部规则跳过时仍应返回可用安全报告");

        assert_eq!(report.coverage.total_rules, 2);
        assert_eq!(report.coverage.bound_rules, 0);
        assert_eq!(report.coverage.degraded_rules, 0);
        assert_eq!(report.coverage.skipped_rules, 2);
        assert!(
            report
                .rule_statuses
                .iter()
                .all(|status| matches!(status.status, SafetyRuleStatusKind::Skipped))
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

device valve_A: solenoid_valve
device valve_B: solenoid_valve

device cyl_A: cylinder {
    stroke_time: 120ms
    retract_time: 120ms
}

device cyl_B: cylinder {
    stroke_time: 120ms
    retract_time: 120ms
}

device sensor_A_ext: sensor
device sensor_B_ext: sensor

relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: Y1.out, to: valve_B.coil, via: driven_by }
relation { from: valve_B.out, to: cyl_B.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_A_ext.sense, via: detects }
relation { from: sensor_A_ext.out, to: X0.in, via: reports_to }
relation { from: cyl_B.extended, to: sensor_B_ext.sense, via: detects }
relation { from: sensor_B_ext.out, to: X1.in, via: reports_to }

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
    fn models_analog_threshold_rules_with_region_abstraction() {
        let source = r#"
[topology]

device pressure_sensor: analog_input { range: 0..100, unit: "bar" }

device Y0: digital_output
device valve_A: solenoid_valve
device cyl_A: cylinder {
    stroke_time: 120ms
    retract_time: 120ms
}

relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }

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

        assert!(
            report
                .warnings
                .iter()
                .all(|warning| !warning.contains("阈值") && !warning.contains("未建模")),
            "阈值规则已纳入离散抽象时不应输出跳过告警"
        );
        assert!(
            matches!(
                report.level,
                SafetyProofLevel::Complete | SafetyProofLevel::Bounded
            ),
            "阈值规则纳入建模后应产生有效证明等级"
        );
    }

    #[test]
    fn reports_analog_threshold_split_points_and_hit_intervals() {
        let source = r#"
[topology]

device pressure_sensor: analog_input { range: 0..100, unit: "bar" }

[constraints]

safety: pressure_sensor > 50 conflicts_with pressure_sensor < 10

[tasks]

task main:
    step s1:
        action: log "tick"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let report = verify_safety(&program, &constraints, &state_machine)
            .expect("含模拟量阈值规则的场景应返回可用 safety 结果");

        assert_eq!(report.rule_statuses.len(), 1);
        let status = &report.rule_statuses[0];
        assert!(matches!(status.status, SafetyRuleStatusKind::Degraded));
        assert!(
            status.reason.as_deref().unwrap_or("").contains("区间离散"),
            "阈值抽象应标注为降级原因"
        );
        assert_eq!(status.analog_thresholds.len(), 2);
        for detail in &status.analog_thresholds {
            assert_eq!(detail.split_points, vec![0.0, 10.0, 50.0, 100.0]);
            assert_eq!(detail.total_intervals, 3);
            assert_eq!(detail.hit_intervals, 1);
        }
    }

    #[test]
    fn detects_conflict_for_overlapping_analog_thresholds() {
        let source = r#"
[topology]

device AI0: analog_input { range: 0..100 }

[constraints]

safety: AI0 > 50 conflicts_with AI0 > 60

[tasks]

task main:
    step s1:
        action: log "tick"
    step s2:
        action: log "tick"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let errors = verify_safety(&program, &constraints, &state_machine)
            .expect_err("重叠的模拟量阈值应触发冲突");

        assert!(
            errors
                .iter()
                .any(|error| error.constraint.contains("AI0 > 50")),
            "错误应包含模拟量阈值冲突描述"
        );
    }

    #[test]
    fn detects_cross_port_conflict_for_stepper_enable_and_pulse() {
        let source = r#"
[topology]

device axis_x: stepper_motor

[constraints]

safety: axis_x.enable.off conflicts_with axis_x.pulse.active

[tasks]

task main:
    step disable_axis:
        action: set axis_x.enable off
    step pulse_axis:
        action: set axis_x.pulse active
    step done:
        action: log "done"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let errors = verify_safety(&program, &constraints, &state_machine)
            .expect_err("enable.off 与 pulse.active 组合应触发冲突");

        assert!(
            errors
                .iter()
                .any(|error| error.constraint.contains("axis_x.enable.off")),
            "错误应包含端口化安全约束文本"
        );
        assert!(
            errors
                .iter()
                .any(|error| error.constraint.contains("axis_x.pulse.active")),
            "错误应包含 pulse 端口状态"
        );
    }

    #[test]
    fn models_cam_following_error_threshold_on_port_domain() {
        let source = r#"
[topology]

device encoder_main: analog_input { range: 0..360 }
device servo_cmd: analog_output { range: 0..360 }
device cam_xy: cam_coupling {
    master: encoder_main,
    slave: servo_cmd,
    table: cam_a,
}
cam_table cam_a: periodic [
    (0, 0),
    (180, 100),
    (360, 0),
]

[constraints]

safety: cam_xy.following_error > 2 conflicts_with cam_xy.in_sync.on

[tasks]

task main:
    step run:
        action: cam_engage cam_xy
        wait: cam_xy.in_sync == true
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");
        let report = verify_safety(&program, &constraints, &state_machine)
            .expect("cam following_error 阈值应可建模并返回 safety 结果");

        assert_eq!(report.rule_statuses.len(), 1);
        assert_eq!(report.rule_statuses[0].analog_thresholds.len(), 1);
        let detail = &report.rule_statuses[0].analog_thresholds[0];
        assert_eq!(detail.device, "cam_xy.following_error");
        assert!(
            detail.split_points.contains(&2.0),
            "阈值分割点应包含 following_error 阈值"
        );
    }

    #[test]
    fn validates_cam_fault_interlock_rule_on_cam_ports() {
        let source = r#"
[topology]

device encoder_main: analog_input { range: 0..360 }
device servo_cmd: analog_output { range: 0..360 }
device cam_xy: cam_coupling {
    master: encoder_main,
    slave: servo_cmd,
    table: cam_a,
}
cam_table cam_a: periodic [
    (0, 0),
    (180, 100),
    (360, 0),
]

[constraints]

safety: cam_xy.fault.on conflicts_with cam_xy.engage.off

[tasks]

task main:
    step force_fault:
        action: set cam_xy.fault on
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let report = verify_safety(&program, &constraints, &state_machine)
            .expect("cam fault 互锁规则应完成绑定并参与验证");
        assert_eq!(report.rule_statuses.len(), 1);
        assert!(
            report.rule_statuses[0].rule.contains("cam_xy.fault.on")
                && report.rule_statuses[0].rule.contains("cam_xy.engage.off"),
            "规则文本应包含 cam fault 互锁约束"
        );
    }

    #[test]
    fn maps_set_analog_to_region_state() {
        let source = r#"
[topology]

device AO0: analog_output { range: 0..10 }
device Y0: digital_output
device valve: solenoid_valve
relation { from: Y0.out, to: valve.coil, via: driven_by }

[constraints]

[tasks]

task main:
    step set:
        action: set_analog AO0 7.5
    step done:
        action: log "done"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");
        let model = SafetyModel::from_inputs(&program, &constraints, &state_machine);

        let device_id = *model
            .device_index
            .get(&("AO0".to_string(), "self".to_string()))
            .expect("AO0 应注册为设备");
        let target_state =
            analog_state_for_value(&model.devices, device_id, "7.5").expect("应找到区间状态");

        let has_effect = model
            .edges
            .iter()
            .any(|edge| edge.effects.get(&device_id).copied() == Some(target_state));

        assert!(has_effect, "set_analog 应映射到对应区间状态");
    }
}
