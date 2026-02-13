use crate::ast::{
    ComparisonOperator, ConditionExpression, LiteralValue, OnCompleteDirective, PlcProgram,
    StepStatement, WaitCondition, WaitStatement,
};
use crate::ir::{StateMachine, TransitionGuard};
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
}

pub fn verify_liveness(
    program: &PlcProgram,
    state_machine: &StateMachine,
) -> Result<(), Vec<LivenessDiagnostic>> {
    let mut diagnostics = Vec::new();

    let step_line_map = collect_step_line_map(program);
    check_wait_timeout_or_allow(program, &mut diagnostics);
    check_unreachable_on_complete(program, &mut diagnostics);
    check_non_terminal_zero_out_degree(program, state_machine, &step_line_map, &mut diagnostics);
    check_strongly_connected_components(program, state_machine, &step_line_map, &mut diagnostics);

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
    diagnostics: &mut Vec<LivenessDiagnostic>,
) {
    let allow_wait_states = collect_allow_wait_states(program);
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

        graph.add_edge(
            from_index,
            to_index,
            LivenessEdge {
                is_bounded_wait: matches!(
                    transition.guard,
                    TransitionGuard::Timeout { .. } | TransitionGuard::Delay { .. }
                ),
                source_has_allow_wait: allow_wait_states.contains(&from_key),
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

        let mut has_bounded_wait_or_allow = false;
        for node in &component {
            for edge in graph.edges(*node) {
                if edge.weight().is_bounded_wait || edge.weight().source_has_allow_wait {
                    has_bounded_wait_or_allow = true;
                    break;
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
                "检测到强连通分量 [{}] 不包含 timeout 或 allow_indefinite_wait 出边",
                component_states.join(", ")
            ),
            physical_analysis:
                "一旦进入该循环，若条件长期不满足，流程会在环内反复执行且没有超时/人工等待豁免出口"
                    .to_string(),
            suggestion:
                "请在该循环中添加 timeout 逃生分支，或在人工等待点显式声明 allow_indefinite_wait: true"
                    .to_string(),
        });
    }
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
            StepStatement::Action(_) | StepStatement::Goto(_) => {}
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

fn collect_allow_wait_states(program: &PlcProgram) -> HashSet<(String, String)> {
    let mut states = HashSet::new();

    for task in &program.tasks.tasks {
        for step in &task.steps {
            if step
                .statements
                .iter()
                .any(|statement| matches!(statement, StepStatement::AllowIndefiniteWait(true)))
            {
                states.insert(state_key(&task.name, &step.name));
            }
        }
    }

    states
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
        LiteralValue::String(value) => format!("\"{value}\""),
        LiteralValue::State(state) => format!("{}.{}", state.device, state.state),
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
}
