use crate::ast::{
    ActionStatement, ComparisonOperator, ConditionExpression, Expression, LiteralValue,
    OnCompleteDirective, PlcProgram, StepStatement, WaitCondition, WaitStatement,
};
use crate::ir::{ActionKind, StateMachine, TransitionGuard};
use petgraph::algo::kosaraju_scc;
use petgraph::graph::DiGraph;
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LivenessDiagnostic {
    pub line: usize,
    pub reason: String,
    pub physical_analysis: String,
    pub suggestion: String,
}

impl fmt::Display for LivenessDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "ERROR [liveness] 潜在死锁")?;
        writeln!(f, "  位置: <input>:{}:1", self.line)?;
        writeln!(f, "  原因: {}", self.reason)?;
        writeln!(f, "  物理分析: {}", self.physical_analysis)?;
        write!(f, "  建议: {}", self.suggestion)
    }
}

#[derive(Debug, Clone, Default)]
struct StepLivenessFacts {
    waits: Vec<String>,
    has_timeout: bool,
    has_delay: bool,
    has_allow_indefinite_wait: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct FlowSummary {
    has_jump_path: bool,
    has_non_jump_path: bool,
}

impl FlowSummary {
    fn merge(&mut self, other: Self) {
        self.has_jump_path |= other.has_jump_path;
        self.has_non_jump_path |= other.has_non_jump_path;
    }

    fn guarantees_jump(&self) -> bool {
        self.has_jump_path && !self.has_non_jump_path
    }
}

#[derive(Debug, Clone, Copy)]
struct LivenessEdge {
    is_bounded_wait: bool,
    source_has_allow_wait: bool,
    source_wait_semantic: WaitSemantic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum WaitSemantic {
    #[default]
    None,
    WaitCondition,
    Delay,
    PendingAction,
}

#[derive(Debug, Clone, Default)]
struct StepWaitProfile {
    has_wait_condition: bool,
    has_delay: bool,
    has_pending_action: bool,
    has_timeout_escape: bool,
    has_allow_indefinite_wait: bool,
}

impl StepWaitProfile {
    fn wait_semantic(&self) -> WaitSemantic {
        if self.has_pending_action {
            WaitSemantic::PendingAction
        } else if self.has_delay {
            WaitSemantic::Delay
        } else if self.has_wait_condition {
            WaitSemantic::WaitCondition
        } else {
            WaitSemantic::None
        }
    }

    fn has_bounded_wait(&self) -> bool {
        self.has_delay || self.has_timeout_escape
    }

    fn is_unbounded_wait(&self) -> bool {
        matches!(
            self.wait_semantic(),
            WaitSemantic::WaitCondition | WaitSemantic::PendingAction
        ) && !self.has_bounded_wait()
            && !self.has_allow_indefinite_wait
    }
}

#[derive(Debug, Clone)]
struct UnboundedWaitRequirement {
    line: usize,
    signals: HashSet<String>,
}

pub fn verify_liveness(
    program: &PlcProgram,
    state_machine: &StateMachine,
) -> Result<(), Vec<LivenessDiagnostic>> {
    let mut diagnostics = Vec::new();

    let step_line_map = collect_step_line_map(program);
    let step_wait_profiles = collect_step_wait_profiles(program, state_machine);
    check_wait_timeout_or_allow(program, &mut diagnostics);
    check_concurrent_wait_deadlocks(program, &step_wait_profiles, &mut diagnostics);
    check_unreachable_on_complete(program, &mut diagnostics);
    check_non_terminal_zero_out_degree(program, state_machine, &step_line_map, &mut diagnostics);
    check_strongly_connected_components(
        program,
        state_machine,
        &step_line_map,
        &step_wait_profiles,
        &mut diagnostics,
    );

    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

fn check_wait_timeout_or_allow(program: &PlcProgram, diagnostics: &mut Vec<LivenessDiagnostic>) {
    for task in &program.tasks.tasks {
        for step in &task.steps {
            let mut facts = StepLivenessFacts::default();
            collect_step_liveness_facts(&step.statements, &mut facts);

            if facts.waits.is_empty()
                || facts.has_timeout
                || facts.has_delay
                || facts.has_allow_indefinite_wait
            {
                continue;
            }

            for wait in facts.waits {
                diagnostics.push(LivenessDiagnostic {
                    line: step.line.max(1),
                    reason: format!(
                        "task {}.{} 的 wait 条件 `{wait}` 缺少 timeout 分支，且未设置 allow_indefinite_wait",
                        task.name, step.name
                    ),
                    physical_analysis: "若传感器信号长期不满足（线路故障/执行器卡滞/设备离线），控制逻辑会永久停留在该等待点".to_string(),
                    suggestion: "请为该 step 添加 `timeout: <时长> -> goto <恢复 task>`，或在人工等待场景显式设置 `allow_indefinite_wait: true`".to_string(),
                });
            }
        }
    }
}

fn check_unreachable_on_complete(program: &PlcProgram, diagnostics: &mut Vec<LivenessDiagnostic>) {
    for task in &program.tasks.tasks {
        if !matches!(task.on_complete, Some(OnCompleteDirective::Unreachable)) {
            continue;
        }

        let Some(last_step) = task.steps.last() else {
            continue;
        };

        let summary = summarize_statements(&last_step.statements, false);
        if summary.guarantees_jump() {
            continue;
        }

        diagnostics.push(LivenessDiagnostic {
            line: task.on_complete_line.unwrap_or(task.line).max(1),
            reason: format!(
                "task {} 声明了 on_complete: unreachable，但最后一步 {} 仍存在非跳转执行路径",
                task.name, last_step.name
            ),
            physical_analysis:
                "该 task 仍可能在不执行 goto 的情况下到达完成点或停滞，`unreachable` 标记与真实控制流不一致"
                    .to_string(),
            suggestion:
                "请确保最后一步的所有路径都通过 goto/timeout->goto 离开该 task，或改为 `on_complete: goto <task>`"
                    .to_string(),
        });
    }
}

fn check_non_terminal_zero_out_degree(
    program: &PlcProgram,
    state_machine: &StateMachine,
    step_line_map: &HashMap<(String, String), usize>,
    diagnostics: &mut Vec<LivenessDiagnostic>,
) {
    let out_degree = out_degree_map(state_machine);

    for state in &state_machine.states {
        let key = state_key(&state.task_name, &state.step_name);
        if out_degree.get(&key).copied().unwrap_or(0) > 0 {
            continue;
        }

        if is_terminal_state(program, &state.task_name, &state.step_name) {
            continue;
        }

        let line = state_line(step_line_map, program, &state.task_name, &state.step_name);
        diagnostics.push(LivenessDiagnostic {
            line,
            reason: format!("状态 {}.{} 没有任何出边", state.task_name, state.step_name),
            physical_analysis:
                "该状态既不是显式终态，也不存在转移分支；运行到此处后控制流程将无法继续推进"
                    .to_string(),
            suggestion:
                "请补充 wait+timeout、goto 或 on_complete 跳转，确保该状态至少存在一条可执行出边"
                    .to_string(),
        });
    }
}

fn check_strongly_connected_components(
    program: &PlcProgram,
    state_machine: &StateMachine,
    step_line_map: &HashMap<(String, String), usize>,
    step_wait_profiles: &HashMap<(String, String), StepWaitProfile>,
    diagnostics: &mut Vec<LivenessDiagnostic>,
) {
    let mut graph = DiGraph::<(String, String), LivenessEdge>::new();
    let mut node_map = HashMap::<(String, String), petgraph::graph::NodeIndex>::new();

    for state in &state_machine.states {
        let key = state_key(&state.task_name, &state.step_name);
        let index = graph.add_node(key.clone());
        node_map.insert(key, index);
    }

    for transition in &state_machine.transitions {
        let from_key = state_key(&transition.from.task_name, &transition.from.step_name);
        let to_key = state_key(&transition.to.task_name, &transition.to.step_name);

        let Some(from_index) = node_map.get(&from_key).copied() else {
            continue;
        };
        let Some(to_index) = node_map.get(&to_key).copied() else {
            continue;
        };
        let step_profile = step_wait_profile_for_state(step_wait_profiles, &from_key);
        let source_wait_semantic = step_profile.wait_semantic();

        graph.add_edge(
            from_index,
            to_index,
            LivenessEdge {
                is_bounded_wait: step_profile.has_bounded_wait()
                    || matches!(
                        transition.guard,
                        TransitionGuard::Timeout { .. } | TransitionGuard::Delay { .. }
                    ),
                source_has_allow_wait: step_profile.has_allow_indefinite_wait,
                source_wait_semantic,
            },
        );
    }

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

        let component_set = component.iter().copied().collect::<HashSet<_>>();
        let has_exit_edge = component.iter().any(|node| {
            graph
                .edges(*node)
                .any(|edge| !component_set.contains(&edge.target()))
        });
        if has_exit_edge {
            continue;
        }

        let mut has_bounded_wait_or_allow = false;
        let mut has_wait_semantic = false;
        for node in &component {
            for edge in graph.edges(*node) {
                if edge.weight().is_bounded_wait || edge.weight().source_has_allow_wait {
                    has_bounded_wait_or_allow = true;
                    break;
                }
                if edge.weight().source_wait_semantic != WaitSemantic::None {
                    has_wait_semantic = true;
                }
            }
            if has_bounded_wait_or_allow {
                break;
            }
        }

        if has_bounded_wait_or_allow {
            continue;
        }

        let mut component_states = component
            .iter()
            .map(|node| {
                let (task, step) = &graph[*node];
                format!("{task}.{step}")
            })
            .collect::<Vec<_>>();
        component_states.sort();

        let line = component
            .iter()
            .map(|node| {
                let (task, step) = &graph[*node];
                state_line(step_line_map, program, task, step)
            })
            .min()
            .unwrap_or(1)
            .max(1);

        diagnostics.push(LivenessDiagnostic {
            line,
            reason: format!(
                "{}强连通分量 [{}] 不包含 timeout 或 allow_indefinite_wait 出边",
                if has_wait_semantic { "检测到死锁" } else { "检测到活锁" },
                component_states.join(", "),
            ),
            physical_analysis: if has_wait_semantic {
                "一旦并发 task 在该环内进入 wait/delay/pending 等待点，若彼此依赖条件长期不满足，将形成无界等待死锁"
                    .to_string()
            } else {
                "该环由非阻塞跳转组成且无退出边，流程会持续空转却无法完成有意义进展（活锁）".to_string()
            },
            suggestion: if has_wait_semantic {
                "请在该循环中添加 timeout 逃生分支，或在人工等待点显式声明 allow_indefinite_wait: true"
                    .to_string()
            } else {
                "请为该循环增加可达退出边（goto/timeout），或引入显式等待条件避免无界空转".to_string()
            },
        });
    }
}

fn check_concurrent_wait_deadlocks(
    program: &PlcProgram,
    step_wait_profiles: &HashMap<(String, String), StepWaitProfile>,
    diagnostics: &mut Vec<LivenessDiagnostic>,
) {
    let write_signals_by_task = collect_task_write_signals(program);
    let mut writers_by_signal = HashMap::<String, HashSet<String>>::new();
    for (task, signals) in &write_signals_by_task {
        for signal in signals {
            for key in signal_lookup_keys(signal) {
                writers_by_signal
                    .entry(key)
                    .or_default()
                    .insert(task.clone());
            }
        }
    }

    let wait_requirements = collect_unbounded_wait_requirements(program, step_wait_profiles);
    let mut dependency_graph = DiGraph::<String, ()>::new();
    let mut task_nodes = HashMap::<String, petgraph::graph::NodeIndex>::new();
    let mut blocking_lines_by_task = HashMap::<String, Vec<usize>>::new();

    for (task, requirements) in &wait_requirements {
        if requirements.is_empty() {
            continue;
        }

        let from_node = *task_nodes
            .entry(task.clone())
            .or_insert_with(|| dependency_graph.add_node(task.clone()));
        blocking_lines_by_task.insert(task.clone(), requirements.iter().map(|r| r.line).collect());

        for requirement in requirements {
            for signal in &requirement.signals {
                for key in signal_lookup_keys(signal) {
                    let Some(writers) = writers_by_signal.get(&key) else {
                        continue;
                    };
                    for writer in writers {
                        if writer == task {
                            continue;
                        }

                        let to_node = *task_nodes
                            .entry(writer.clone())
                            .or_insert_with(|| dependency_graph.add_node(writer.clone()));
                        dependency_graph.add_edge(from_node, to_node, ());
                    }
                }
            }
        }
    }

    for component in kosaraju_scc(&dependency_graph) {
        if component.len() < 2 {
            continue;
        }

        let component_set = component.iter().copied().collect::<HashSet<_>>();
        let mut component_tasks = component
            .iter()
            .map(|node| dependency_graph[*node].clone())
            .collect::<Vec<_>>();
        component_tasks.sort();

        let all_tasks_blocking = component_tasks.iter().all(|task| {
            blocking_lines_by_task
                .get(task)
                .is_some_and(|lines| !lines.is_empty())
        });
        if !all_tasks_blocking {
            continue;
        }

        let all_tasks_wait_on_component = component.iter().all(|node| {
            dependency_graph
                .edges(*node)
                .any(|edge| component_set.contains(&edge.target()))
        });
        if !all_tasks_wait_on_component {
            continue;
        }

        let line = component_tasks
            .iter()
            .flat_map(|task| blocking_lines_by_task.get(task).into_iter().flatten())
            .copied()
            .min()
            .unwrap_or(1)
            .max(1);

        diagnostics.push(LivenessDiagnostic {
            line,
            reason: format!(
                "检测到并发 deadlock：task [{}] 只等待彼此释放资源，且等待点未提供 timeout/allow_indefinite_wait",
                component_tasks.join(", ")
            ),
            physical_analysis:
                "这些 task 在无界等待条件上形成互相依赖闭环，任何一个 task 都无法先完成，系统会永久停滞"
                    .to_string(),
            suggestion:
                "请至少为一个等待点提供 timeout 逃生路径，或重构同步协议（信号握手/资源仲裁）以打破互等环"
                    .to_string(),
        });
    }
}

fn collect_step_wait_profiles(
    program: &PlcProgram,
    state_machine: &StateMachine,
) -> HashMap<(String, String), StepWaitProfile> {
    let mut profiles = HashMap::new();

    for task in &program.tasks.tasks {
        for step in &task.steps {
            let mut profile = StepWaitProfile::default();
            collect_step_wait_profile_from_statements(&step.statements, &mut profile);
            profiles.insert(state_key(&task.name, &step.name), profile);
        }
    }

    for task_ctx in &state_machine.task_contexts {
        for pending in &task_ctx.pending_actions {
            if !matches!(
                pending.action_kind,
                ActionKind::AxisMoveRelative | ActionKind::AxisMoveAbsolute
            ) {
                continue;
            }
            profiles
                .entry(state_key(
                    &pending.source_state.task_name,
                    &pending.source_state.step_name,
                ))
                .or_default()
                .has_pending_action = true;
        }
    }

    profiles
}

fn collect_step_wait_profile_from_statements(
    statements: &[StepStatement],
    profile: &mut StepWaitProfile,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(action) => match action {
                ActionStatement::AxisMoveRelative { timeout, .. }
                | ActionStatement::AxisMoveAbsolute { timeout, .. } => {
                    profile.has_pending_action = true;
                    if timeout.is_some() {
                        profile.has_timeout_escape = true;
                    }
                }
                ActionStatement::Extend { .. }
                | ActionStatement::Retract { .. }
                | ActionStatement::Set { .. }
                | ActionStatement::SetAnalog { .. }
                | ActionStatement::SetAnalogExpr { .. }
                | ActionStatement::Compute { .. }
                | ActionStatement::Call { .. }
                | ActionStatement::CamEngage { .. }
                | ActionStatement::CamDisengage { .. }
                | ActionStatement::CamSwitch { .. }
                | ActionStatement::CamPhase { .. }
                | ActionStatement::Log { .. } => {}
            },
            StepStatement::Wait(_) => profile.has_wait_condition = true,
            StepStatement::Timeout(_) => profile.has_timeout_escape = true,
            StepStatement::Delay { .. } => {
                profile.has_delay = true;
                profile.has_timeout_escape = true;
            }
            StepStatement::AllowIndefiniteWait(value) => {
                if *value {
                    profile.has_allow_indefinite_wait = true;
                }
            }
            StepStatement::Repeat { body, .. } => {
                collect_step_wait_profile_from_statements(body, profile)
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    collect_step_wait_profile_from_statements(&branch.statements, profile);
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    collect_step_wait_profile_from_statements(&branch.statements, profile);
                }
            }
            StepStatement::IfElse { .. } | StepStatement::Goto(_) | StepStatement::Effect(_) => {}
        }
    }
}

fn step_wait_profile_for_state(
    step_wait_profiles: &HashMap<(String, String), StepWaitProfile>,
    state: &(String, String),
) -> StepWaitProfile {
    if let Some(profile) = step_wait_profiles.get(state) {
        return profile.clone();
    }

    let normalized = state_key(&state.0, state.1.split("__").next().unwrap_or(&state.1));
    step_wait_profiles
        .get(&normalized)
        .cloned()
        .unwrap_or_default()
}

fn collect_unbounded_wait_requirements(
    program: &PlcProgram,
    step_wait_profiles: &HashMap<(String, String), StepWaitProfile>,
) -> HashMap<String, Vec<UnboundedWaitRequirement>> {
    let mut requirements = HashMap::<String, Vec<UnboundedWaitRequirement>>::new();

    for task in &program.tasks.tasks {
        for step in &task.steps {
            let step_key = state_key(&task.name, &step.name);
            let profile = step_wait_profiles
                .get(&step_key)
                .cloned()
                .unwrap_or_default();
            if !profile.is_unbounded_wait() {
                continue;
            }

            let mut signals = HashSet::new();
            collect_wait_signals(&step.statements, &mut signals);
            if signals.is_empty() {
                continue;
            }

            requirements
                .entry(task.name.clone())
                .or_default()
                .push(UnboundedWaitRequirement {
                    line: step.line.max(1),
                    signals,
                });
        }
    }

    requirements
}

fn collect_task_write_signals(program: &PlcProgram) -> HashMap<String, HashSet<String>> {
    let mut writes = HashMap::<String, HashSet<String>>::new();

    for task in &program.tasks.tasks {
        let mut task_writes = HashSet::new();
        for step in &task.steps {
            collect_statement_write_signals(&step.statements, &mut task_writes);
        }
        if !task_writes.is_empty() {
            writes.insert(task.name.clone(), task_writes);
        }
    }

    writes
}

fn collect_statement_write_signals(statements: &[StepStatement], writes: &mut HashSet<String>) {
    for statement in statements {
        match statement {
            StepStatement::Action(action) => collect_action_write_signals(action, writes),
            StepStatement::Repeat { body, .. } => collect_statement_write_signals(body, writes),
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    collect_statement_write_signals(&branch.statements, writes);
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    collect_statement_write_signals(&branch.statements, writes);
                }
            }
            StepStatement::Wait(_)
            | StepStatement::Effect(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
}

fn collect_action_write_signals(action: &ActionStatement, writes: &mut HashSet<String>) {
    match action {
        ActionStatement::Set { target, .. }
        | ActionStatement::SetAnalog { target, .. }
        | ActionStatement::SetAnalogExpr { target, .. }
        | ActionStatement::Extend { target, .. }
        | ActionStatement::Retract { target, .. }
        | ActionStatement::AxisMoveRelative { target, .. }
        | ActionStatement::AxisMoveAbsolute { target, .. } => {
            writes.insert(target.device.clone());
            if target.port != "self" {
                writes.insert(format!("{}.{}", target.device, target.port));
            }
        }
        ActionStatement::Compute { target, .. } => {
            writes.insert(target.clone());
        }
        ActionStatement::CamEngage { target }
        | ActionStatement::CamDisengage { target }
        | ActionStatement::CamSwitch { target, .. }
        | ActionStatement::CamPhase { target, .. } => {
            writes.insert(target.clone());
        }
        ActionStatement::Call { .. } | ActionStatement::Log { .. } => {}
    }
}

fn collect_wait_signals(statements: &[StepStatement], signals: &mut HashSet<String>) {
    for statement in statements {
        match statement {
            StepStatement::Wait(wait) => collect_wait_signals_from_wait(wait, signals),
            StepStatement::Repeat { body, .. } => collect_wait_signals(body, signals),
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    collect_wait_signals(&branch.statements, signals);
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    collect_wait_signals(&branch.statements, signals);
                }
            }
            StepStatement::Action(_)
            | StepStatement::Effect(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
}

fn collect_wait_signals_from_wait(wait: &WaitStatement, signals: &mut HashSet<String>) {
    let conditions = match &wait.condition {
        WaitCondition::Single(cond) => vec![cond],
        WaitCondition::And(conds) | WaitCondition::Or(conds) => conds.iter().collect::<Vec<_>>(),
    };

    for condition in conditions {
        if let Some((left, right)) = condition.expression_pair() {
            collect_signals_from_expression(left, signals);
            collect_signals_from_expression(right, signals);
        } else {
            let left = condition.left.trim();
            if !left.is_empty() {
                signals.insert(left.to_string());
            }
        }
    }
}

fn collect_signals_from_expression(expr: &Expression, signals: &mut HashSet<String>) {
    match expr {
        Expression::Variable(name) => {
            signals.insert(name.clone());
        }
        Expression::UnaryNeg(inner) | Expression::UnaryNot(inner) => {
            collect_signals_from_expression(inner, signals);
        }
        Expression::BinaryOp { left, right, .. } => {
            collect_signals_from_expression(left, signals);
            collect_signals_from_expression(right, signals);
        }
        Expression::FunctionCall { args, .. } => {
            for arg in args {
                collect_signals_from_expression(arg, signals);
            }
        }
        Expression::Literal(_) | Expression::Boolean(_) => {}
    }
}

fn signal_lookup_keys(signal: &str) -> Vec<String> {
    let trimmed = signal.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut keys = vec![trimmed.to_string()];
    if let Some((base, _)) = trimmed.split_once('.') {
        if !base.is_empty() {
            keys.push(base.to_string());
        }
    }
    keys
}

fn collect_step_liveness_facts(statements: &[StepStatement], facts: &mut StepLivenessFacts) {
    for statement in statements {
        match statement {
            StepStatement::Wait(wait) => facts.waits.push(wait_to_text(wait)),
            StepStatement::Timeout(_) => facts.has_timeout = true,
            StepStatement::Delay { .. } => facts.has_delay = true,
            StepStatement::IfElse { .. } => {}
            StepStatement::Repeat { body, .. } => collect_step_liveness_facts(body, facts),
            StepStatement::AllowIndefiniteWait(value) => {
                if *value {
                    facts.has_allow_indefinite_wait = true;
                }
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    collect_step_liveness_facts(&branch.statements, facts);
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    collect_step_liveness_facts(&branch.statements, facts);
                }
            }
            StepStatement::Action(_) | StepStatement::Goto(_) | StepStatement::Effect(_) => {}
        }
    }
}

fn summarize_statements(statements: &[StepStatement], completion_is_jump: bool) -> FlowSummary {
    let mut summary = FlowSummary::default();
    let mut has_control_flow = false;

    for statement in statements {
        match statement {
            StepStatement::Goto(_) | StepStatement::Timeout(_) | StepStatement::IfElse { .. } => {
                has_control_flow = true;
                summary.has_jump_path = true;
            }
            StepStatement::Wait(_) => {
                has_control_flow = true;
                if completion_is_jump {
                    summary.has_jump_path = true;
                } else {
                    summary.has_non_jump_path = true;
                }
            }
            StepStatement::Repeat { body, .. } => {
                has_control_flow = true;
                summary.merge(summarize_statements(body, completion_is_jump));
            }
            StepStatement::Parallel(block) => {
                has_control_flow = true;
                for branch in &block.branches {
                    summary.merge(summarize_statements(&branch.statements, completion_is_jump));
                }
            }
            StepStatement::Race(block) => {
                has_control_flow = true;
                for branch in &block.branches {
                    let branch_completion_is_jump =
                        branch.then_goto.is_some() || completion_is_jump;
                    summary.merge(summarize_statements(
                        &branch.statements,
                        branch_completion_is_jump,
                    ));
                }
            }
            StepStatement::Action(_)
            | StepStatement::Effect(_)
            | StepStatement::Delay { .. }
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }

    if !has_control_flow {
        if completion_is_jump {
            summary.has_jump_path = true;
        } else {
            summary.has_non_jump_path = true;
        }
    }

    summary
}

fn collect_step_line_map(program: &PlcProgram) -> HashMap<(String, String), usize> {
    let mut map = HashMap::new();
    for task in &program.tasks.tasks {
        for step in &task.steps {
            map.insert(
                state_key(&task.name, &step.name),
                step.line.max(task.line).max(1),
            );
        }
    }
    map
}

fn out_degree_map(state_machine: &StateMachine) -> HashMap<(String, String), usize> {
    let mut out_degree = HashMap::new();

    for transition in &state_machine.transitions {
        let key = state_key(&transition.from.task_name, &transition.from.step_name);
        *out_degree.entry(key).or_insert(0) += 1;
    }

    out_degree
}

fn is_terminal_state(program: &PlcProgram, task_name: &str, step_name: &str) -> bool {
    let Some(task) = program
        .tasks
        .tasks
        .iter()
        .find(|task| task.name == task_name)
    else {
        return false;
    };

    if task.on_complete.is_some() {
        return false;
    }

    task.steps
        .last()
        .map(|step| step.name == step_name)
        .unwrap_or(false)
}

fn state_line(
    step_line_map: &HashMap<(String, String), usize>,
    program: &PlcProgram,
    task_name: &str,
    step_name: &str,
) -> usize {
    let direct_key = state_key(task_name, step_name);
    if let Some(line) = step_line_map.get(&direct_key) {
        return (*line).max(1);
    }

    let base_step = step_name
        .split("__")
        .next()
        .unwrap_or(step_name)
        .to_string();
    let normalized_key = state_key(task_name, &base_step);
    if let Some(line) = step_line_map.get(&normalized_key) {
        return (*line).max(1);
    }

    program
        .tasks
        .tasks
        .iter()
        .find(|task| task.name == task_name)
        .map(|task| task.line.max(1))
        .unwrap_or(1)
}

fn state_key(task_name: &str, step_name: &str) -> (String, String) {
    (task_name.to_string(), step_name.to_string())
}

fn wait_to_text(wait: &WaitStatement) -> String {
    match &wait.condition {
        WaitCondition::Single(condition) => condition_to_text(condition),
        WaitCondition::And(conditions) => conditions
            .iter()
            .map(condition_to_text)
            .collect::<Vec<_>>()
            .join(" AND "),
        WaitCondition::Or(conditions) => conditions
            .iter()
            .map(condition_to_text)
            .collect::<Vec<_>>()
            .join(" OR "),
    }
}

fn condition_to_text(condition: &ConditionExpression) -> String {
    if let Some((left, right)) = condition.expression_pair() {
        return format!(
            "{} {} {}",
            render_expression(left),
            comparison_operator_text(&condition.operator),
            render_expression(right)
        );
    }

    format!(
        "{} {} {}",
        condition.left,
        comparison_operator_text(&condition.operator),
        literal_to_text(&condition.right)
    )
}

fn comparison_operator_text(operator: &ComparisonOperator) -> &'static str {
    match operator {
        ComparisonOperator::Eq => "==",
        ComparisonOperator::Neq => "!=",
        ComparisonOperator::Gt => ">",
        ComparisonOperator::Lt => "<",
        ComparisonOperator::Gte => ">=",
        ComparisonOperator::Lte => "<=",
    }
}

fn literal_to_text(literal: &LiteralValue) -> String {
    match literal {
        LiteralValue::Boolean(value) => value.to_string(),
        LiteralValue::Number(value) => value.to_string(),
        LiteralValue::Measured(measured) => format!("{}{}", measured.value, measured.unit),
        LiteralValue::String(value) => format!("\"{value}\""),
        LiteralValue::State(state) => format!("{}.{}", state.device, state.state),
    }
}

fn render_expression(expr: &crate::ast::Expression) -> String {
    match expr {
        crate::ast::Expression::Literal(value) => value.to_string(),
        crate::ast::Expression::Boolean(value) => value.to_string(),
        crate::ast::Expression::Variable(name) => name.clone(),
        crate::ast::Expression::UnaryNeg(inner) => format!("-({})", render_expression(inner)),
        crate::ast::Expression::UnaryNot(inner) => format!("NOT({})", render_expression(inner)),
        crate::ast::Expression::BinaryOp { op, left, right } => format!(
            "({}{}{})",
            render_expression(left),
            match op {
                crate::ast::BinaryOperator::Add => "+",
                crate::ast::BinaryOperator::Sub => "-",
                crate::ast::BinaryOperator::Mul => "*",
                crate::ast::BinaryOperator::Div => "/",
                crate::ast::BinaryOperator::Mod => "%",
                crate::ast::BinaryOperator::Eq => "==",
                crate::ast::BinaryOperator::Neq => "!=",
                crate::ast::BinaryOperator::Gt => ">",
                crate::ast::BinaryOperator::Lt => "<",
                crate::ast::BinaryOperator::Gte => ">=",
                crate::ast::BinaryOperator::Lte => "<=",
                crate::ast::BinaryOperator::And => "AND",
                crate::ast::BinaryOperator::Or => "OR",
            },
            render_expression(right)
        ),
        crate::ast::Expression::FunctionCall { name, args } => format!(
            "{}({})",
            name,
            args.iter()
                .map(render_expression)
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::verify_liveness;
    use crate::parser::parse_plc;
    use crate::semantic::build_state_machine;

    #[test]
    fn passes_prd_5_5_1_to_5_5_3_liveness_examples() {
        let source = r#"
[topology]
device cyl_A: cylinder
device cyl_B: cylinder
device sensor_A_ext: sensor
device sensor_A_ret: sensor
device sensor_B_ext: sensor
device sensor_B_ret: sensor
device start_button: sensor
device alarm_light: digital_output

[constraints]

[tasks]

task init:
    step extend_A:
        action: extend cyl_A
        wait: sensor_A_ext == true
        timeout: 600ms -> goto fault_handler

    step retract_A:
        action: retract cyl_A
        wait: sensor_A_ret == true
        timeout: 500ms -> goto fault_handler

    step extend_B:
        action: extend cyl_B
        wait: sensor_B_ext == true
        timeout: 800ms -> goto fault_handler

    step retract_B:
        action: retract cyl_B
        wait: sensor_B_ret == true
        timeout: 700ms -> goto fault_handler

    on_complete: goto ready

task fault_handler:
    step safe_position:
        action: retract cyl_A
        action: retract cyl_B
    step alarm:
        action: set alarm_light on
        action: log "动作超时，已执行安全复位"
    on_complete: goto ready

task ready:
    step wait_start:
        wait: start_button == true
        allow_indefinite_wait: true
    on_complete: goto init
"#;

        let program = parse_plc(source).expect("PRD 5.5.1-5.5.3 示例应能解析");
        let state_machine = build_state_machine(&program).expect("状态机应构建成功");

        verify_liveness(&program, &state_machine)
            .expect("带 timeout 和 allow_indefinite_wait 的流程应通过活性检查");
    }

    #[test]
    fn fails_when_wait_has_no_timeout_and_no_allow_indefinite_wait() {
        let source = r#"
[topology]

[constraints]

[tasks]

task init:
    step wait_sensor:
        wait: sensor_A == true
    on_complete: goto ready

task ready:
    step idle:
        action: log "ready"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let state_machine = build_state_machine(&program).expect("状态机应构建成功");

        let errors = verify_liveness(&program, &state_machine)
            .expect_err("wait 无 timeout 且无 allow_indefinite_wait 时应报错");

        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("缺少 timeout 分支")),
            "错误应指出 wait 缺少 timeout"
        );
        assert!(
            errors.iter().all(|error| error.line > 0),
            "所有活性错误都应包含有效行号"
        );
    }

    #[test]
    fn fails_for_analog_wait_without_timeout() {
        let source = r#"
[topology]

device AI0: analog_input { range: 0..10 }

[constraints]

[tasks]

task main:
    step wait_pressure:
        wait: AI0 > 5
    step done:
        action: log "done"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let state_machine = build_state_machine(&program).expect("状态机应构建成功");

        let errors =
            verify_liveness(&program, &state_machine).expect_err("模拟量 wait 无 timeout 时应报错");

        assert!(
            errors.iter().any(|error| error.to_string().contains("AI0")
                && error.to_string().contains("缺少 timeout")),
            "错误应包含模拟量 wait 条件文本"
        );
    }

    #[test]
    fn accepts_analog_wait_with_timeout() {
        let source = r#"
[topology]

device AI0: analog_input { range: 0..10 }

[constraints]

[tasks]

task main:
    step wait_pressure:
        wait: AI0 > 5
        timeout: 100ms -> goto fault
    step done:
        action: log "done"

task fault:
    step alarm:
        action: log "fault"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let state_machine = build_state_machine(&program).expect("状态机应构建成功");

        verify_liveness(&program, &state_machine).expect("带 timeout 的模拟量 wait 应通过活性检查");
    }

    #[test]
    fn accepts_on_complete_goto_cycle_as_non_deadlock() {
        let source = r#"
[topology]

[constraints]

[tasks]

task init:
    step boot:
        action: log "boot"
    on_complete: goto ready

task ready:
    step wait_start:
        wait: start_button == true
        allow_indefinite_wait: true
    on_complete: goto init
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let state_machine = build_state_machine(&program).expect("状态机应构建成功");

        verify_liveness(&program, &state_machine)
            .expect("on_complete: goto 构成的循环不应被视为死锁");
    }

    #[test]
    fn accepts_terminal_parallel_step_with_on_complete_goto() {
        let source = r#"
[topology]
device start_button: digital_input

[constraints]

[tasks]

task cycle:
    step do_parallel:
        parallel:
            branch_A:
                delay: 10ms
            branch_B:
                delay: 20ms
    on_complete: goto ready

task ready:
    step idle:
        wait: start_button == true
        allow_indefinite_wait: true
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let state_machine = build_state_machine(&program).expect("状态机应构建成功");

        verify_liveness(&program, &state_machine)
            .expect("并行末尾 step + on_complete: goto 不应触发 join 无出边误报");
    }

    #[test]
    fn accepts_conditional_self_loop_when_scc_has_exit_edge() {
        let source = r#"
[topology]
device sensor_alarm: digital_input

[constraints]

[tasks]

task monitor:
    step check:
        if: sensor_alarm == true goto fault_handler else: goto monitor.check

task fault_handler:
    step alarm:
        action: log "fault"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let state_machine = build_state_machine(&program).expect("状态机应构建成功");

        verify_liveness(&program, &state_machine)
            .expect("条件轮询循环存在逃逸边时不应被误判为死锁");
    }

    #[test]
    fn rejects_unreachable_on_complete_when_last_step_has_non_jump_path() {
        let source = r#"
[topology]

[constraints]

[tasks]

task search:
    step detect:
        wait: sensor_A == true
        timeout: 800ms -> goto fault_handler
    on_complete: unreachable

task fault_handler:
    step alarm:
        action: log "fault"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let state_machine = build_state_machine(&program).expect("状态机应构建成功");

        let errors = verify_liveness(&program, &state_machine)
            .expect_err("unreachable 声明与可完成路径冲突时应报错");

        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("on_complete: unreachable")),
            "错误应明确指出 unreachable 标记无效"
        );
    }

    #[test]
    fn rejects_scc_without_timeout_or_allow_indefinite_wait_edges() {
        let source = r#"
[topology]

[constraints]

[tasks]

task init:
    step start:
        action: log "start"
    on_complete: goto loop

task loop:
    step spin:
        action: log "spin"
    on_complete: goto init
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let state_machine = build_state_machine(&program).expect("状态机应构建成功");

        let errors = verify_liveness(&program, &state_machine)
            .expect_err("无 timeout/allow_indefinite_wait 的循环 SCC 应报错");

        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("强连通分量")),
            "错误应包含 SCC 死锁风险说明"
        );
    }

    #[test]
    fn treats_delay_generated_edges_as_bounded_wait_in_scc_checks() {
        let source = r#"
[topology]

[constraints]

[tasks]

task loop:
    step spin:
        delay: 120ms
    on_complete: goto loop
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let state_machine = build_state_machine(&program).expect("状态机应构建成功");

        verify_liveness(&program, &state_machine)
            .expect("delay 生成的有界等待边不应被判定为死锁 SCC");
    }

    #[test]
    fn allows_unreachable_when_all_race_paths_jump_away() {
        let source = r#"
[topology]

[constraints]

[tasks]

task search:
    step detect:
        race:
            branch_A:
                wait: sensor_A == true
                then: goto process_A
            branch_B:
                wait: sensor_B == true
                then: goto process_B
        timeout: 800ms -> goto fault_handler
    on_complete: unreachable

task process_A:
    step done:
        action: log "A"

task process_B:
    step done:
        action: log "B"

task fault_handler:
    step done:
        action: log "fault"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let state_machine = build_state_machine(&program).expect("状态机应构建成功");

        verify_liveness(&program, &state_machine)
            .expect("race 每条路径都通过 goto/timeout 跳转时 unreachable 应合法");
    }

    #[test]
    fn accepts_axis_fault_recovery_path_with_bounded_wait() {
        let source = r#"
[topology]

device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default, motion_param_set: stepper_default_fast }
device sensor_fault: sensor

[constraints]

[tasks]

task main:
    step move:
        action: axis.move_relative(axis_x, distance: 5, speed: 10)
            timeout: 100ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
    on_complete: goto ready

task fault:
    step timeout:
        wait: sensor_fault == true
        timeout: 200ms -> goto ready
    step reject:
        action: log "reject"
    step motion_fault:
        action: log "motion"
    step safety_fault:
        action: log "safety"
    on_complete: goto ready

task ready:
    step idle:
        action: log "idle"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let state_machine = build_state_machine(&program).expect("状态机应构建成功");

        verify_liveness(&program, &state_machine)
            .expect("axis 故障恢复路径有界等待且可完成时应通过活性检查");
    }

    #[test]
    fn rejects_axis_fault_recovery_wait_without_timeout() {
        let source = r#"
[topology]

device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default, motion_param_set: stepper_default_fast }
device sensor_fault: sensor

[constraints]

[tasks]

task main:
    step move:
        action: axis.move_relative(axis_x, distance: 5, speed: 10)
            timeout: 100ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault

task fault:
    step timeout:
        wait: sensor_fault == true
    step reject:
        action: log "reject"
    step motion_fault:
        action: log "motion"
    step safety_fault:
        action: log "safety"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let state_machine = build_state_machine(&program).expect("状态机应构建成功");

        let errors = verify_liveness(&program, &state_machine)
            .expect_err("axis 故障恢复 wait 缺少 timeout 时应报错");

        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("fault.timeout")
                    && error.to_string().contains("缺少 timeout")),
            "错误应指出 fault.timeout 恢复等待缺少 timeout"
        );
    }

    #[test]
    fn accepts_concurrent_manual_waits_with_allow_indefinite_wait() {
        let source = r#"
[topology]

[constraints]

[tasks]

task loader:
    step wait_operator:
        wait: start_loader == true
        allow_indefinite_wait: true
    on_complete: goto loader

task unloader:
    step wait_operator:
        wait: start_unloader == true
        allow_indefinite_wait: true
    on_complete: goto unloader
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let state_machine = build_state_machine(&program).expect("状态机应构建成功");

        verify_liveness(&program, &state_machine)
            .expect("allow_indefinite_wait 的并发人工等待应被视为合法等待");
    }

    #[test]
    fn reports_deadlock_when_two_tasks_only_wait_each_other_resource_release() {
        let source = r#"
[topology]

[constraints]

[tasks]

task feeder:
    step hold_fixture:
        action: set fixture_a on
    step wait_peer_release:
        wait: fixture_b == false
    on_complete: goto feeder

task picker:
    step hold_fixture:
        action: set fixture_b on
    step wait_peer_release:
        wait: fixture_a == false
    on_complete: goto picker
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let state_machine = build_state_machine(&program).expect("状态机应构建成功");

        let errors = verify_liveness(&program, &state_machine)
            .expect_err("互相等待资源释放的并发 task 应触发 deadlock");

        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("并发 deadlock")),
            "错误应包含并发 deadlock 诊断"
        );
    }

    #[test]
    fn reports_livelock_for_non_blocking_cycle_without_exit() {
        let source = r#"
[topology]

[constraints]

[tasks]

task spin_a:
    step loop:
        action: log "spin_a"
    on_complete: goto spin_b

task spin_b:
    step loop:
        action: log "spin_b"
    on_complete: goto spin_a
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let state_machine = build_state_machine(&program).expect("状态机应构建成功");

        let errors = verify_liveness(&program, &state_machine)
            .expect_err("无等待条件且无出口的循环应触发活锁诊断");

        assert!(
            errors.iter().any(|error| {
                let text = error.to_string();
                text.contains("活锁") && text.contains("强连通分量")
            }),
            "错误应明确区分为活锁风险"
        );
    }

    #[test]
    fn treats_pending_axis_motion_cycle_with_timeout_as_bounded_wait() {
        let source = r#"
[topology]

device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default, motion_param_set: stepper_default_fast }

[constraints]

[tasks]

task move_loop:
    step move:
        action: axis.move_relative(axis_x, distance: 5, speed: 10)
            timeout: 100ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
    on_complete: goto move_loop

task fault:
    step timeout:
        action: log "timeout"
    step reject:
        action: log "reject"
    step motion_fault:
        action: log "motion_fault"
    step safety_fault:
        action: log "safety_fault"
    on_complete: goto move_loop
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let state_machine = build_state_machine(&program).expect("状态机应构建成功");

        verify_liveness(&program, &state_machine)
            .expect("Pending 轴动作带 timeout/fault 路由时应被视为有界等待");
    }
}
