use crate::ast::{ActionStatement, PlcProgram, StepStatement};
use crate::ir::{
    ConstraintSet, StateMachine, TimingRelation, TimingScope, TopologyGraph, TransitionGuard,
};
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimingDiagnostic {
    pub line: usize,
    pub constraint: String,
    pub analysis: String,
    pub conclusion: String,
}

impl fmt::Display for TimingDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "ERROR [timing] 时序包络违反")?;
        writeln!(f, "  位置: <input>:{}:1", self.line)?;
        writeln!(f, "  约束: {}", self.constraint)?;
        writeln!(f, "  分析: {}", self.analysis)?;
        write!(f, "  结论: {}", self.conclusion)
    }
}

#[derive(Debug, Clone, Default)]
struct DeviceTimingProfile {
    response_ms: Option<u64>,
    stroke_ms: Option<u64>,
    retract_ms: Option<u64>,
    ramp_ms: Option<u64>,
}

#[derive(Debug, Clone, Default)]
struct StepTimingEstimate {
    action_max_ms: u64,
    delay_max_ms: u64,
    timeout_max_ms: u64,
    nominal_ms: u64,
    worst_case_ms: u64,
    action_details: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct TimingContext {
    profiles: HashMap<String, DeviceTimingProfile>,
    // Topology edges are producer -> consumer; store reverse adjacency for upstream response tracing.
    upstream_by_target: HashMap<String, Vec<String>>,
}

pub fn verify_timing(
    program: &PlcProgram,
    topology: &TopologyGraph,
    constraints: &ConstraintSet,
    state_machine: &StateMachine,
) -> Result<(), Vec<TimingDiagnostic>> {
    let context = TimingContext::from_inputs(program, topology);
    let step_estimates = build_step_estimates(program, &context);
    let task_nominal_case = build_task_nominal_case(program, &step_estimates);
    let task_worst_case = build_task_worst_case(program, &step_estimates);

    let mut diagnostics = Vec::new();

    for (index, rule) in constraints.timing.iter().enumerate() {
        let line = timing_constraint_line(program, index);
        let constraint_text = format_timing_constraint(rule);

        match rule.relation {
            TimingRelation::MustCompleteWithin => {
                let (observed_ms, analysis) = match &rule.scope {
                    TimingScope::Task { task } => {
                        let observed = task_nominal_case.get(task).copied().unwrap_or(0);
                        (
                            observed,
                            format!(
                                "task {task} 的动作关键路径时间 = {observed}ms（顺序 step 累加，忽略 timeout 上界）"
                            ),
                        )
                    }
                    TimingScope::Step { task, step } => {
                        let key = step_key(task, step);
                        let estimate = step_estimates.get(&key).cloned().unwrap_or_default();
                        let mut analysis = format!(
                            "step {task}.{step} 的动作关键路径时间 = {}ms（同 step 动作并行取最大值 {}ms，delay 固定等待最大值 {}ms，timeout 上界 {}ms 仅用于 worst_case 约束）",
                            estimate.nominal_ms,
                            estimate.action_max_ms,
                            estimate.delay_max_ms,
                            estimate.timeout_max_ms
                        );
                        if !estimate.action_details.is_empty() {
                            analysis.push_str("；动作明细: ");
                            analysis.push_str(&estimate.action_details.join("；"));
                        }
                        (estimate.nominal_ms, analysis)
                    }
                };

                if observed_ms > rule.duration_ms {
                    diagnostics.push(TimingDiagnostic {
                        line,
                        constraint: constraint_text,
                        analysis,
                        conclusion: format!(
                            "最坏情况下无法在 {}ms 内完成，当前关键路径为 {}ms",
                            rule.duration_ms, observed_ms
                        ),
                    });
                }
            }
            TimingRelation::MustCompleteWithinWorstCase => {
                let (observed_ms, analysis) = match &rule.scope {
                    TimingScope::Task { task } => {
                        let observed = task_worst_case.get(task).copied().unwrap_or(0);
                        (
                            observed,
                            format!(
                                "task {task} 的最坏关键路径时间 = {observed}ms（顺序 step 累加，包含 timeout 上界）"
                            ),
                        )
                    }
                    TimingScope::Step { task, step } => {
                        let key = step_key(task, step);
                        let estimate = step_estimates.get(&key).cloned().unwrap_or_default();
                        let mut analysis = format!(
                            "step {task}.{step} 的最坏关键路径时间 = {}ms（同 step 动作并行取最大值 {}ms，delay 固定等待最大值 {}ms，timeout 上界 {}ms）",
                            estimate.worst_case_ms,
                            estimate.action_max_ms,
                            estimate.delay_max_ms,
                            estimate.timeout_max_ms
                        );
                        if !estimate.action_details.is_empty() {
                            analysis.push_str("；动作明细: ");
                            analysis.push_str(&estimate.action_details.join("；"));
                        }
                        (estimate.worst_case_ms, analysis)
                    }
                };

                if observed_ms > rule.duration_ms {
                    diagnostics.push(TimingDiagnostic {
                        line,
                        constraint: constraint_text,
                        analysis,
                        conclusion: format!(
                            "最坏情况下无法在 {}ms 内完成，当前关键路径为 {}ms",
                            rule.duration_ms, observed_ms
                        ),
                    });
                }
            }
            TimingRelation::MustStartAfter => {
                let (min_interval_ms, predecessor_detail) =
                    shortest_predecessor_interval_ms(&rule.scope, program, state_machine);

                if min_interval_ms < rule.duration_ms {
                    diagnostics.push(TimingDiagnostic {
                        line,
                        constraint: constraint_text,
                        analysis: format!(
                            "前驱结束到当前开始的最短间隔 = {min_interval_ms}ms（{predecessor_detail}）"
                        ),
                        conclusion: format!(
                            "无法保证 {} 在 {}ms 后才开始，当前最短间隔为 {}ms",
                            format_scope(&rule.scope),
                            rule.duration_ms,
                            min_interval_ms
                        ),
                    });
                }
            }
        }
    }

    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

impl TimingContext {
    fn from_inputs(program: &PlcProgram, topology: &TopologyGraph) -> Self {
        let mut profiles = HashMap::new();
        for device in &program.topology.devices {
            profiles.insert(
                device.name.clone(),
                DeviceTimingProfile {
                    response_ms: device
                        .attributes
                        .response_time
                        .as_ref()
                        .map(duration_value_to_ms),
                    stroke_ms: device
                        .attributes
                        .stroke_time
                        .as_ref()
                        .map(duration_value_to_ms),
                    retract_ms: device
                        .attributes
                        .retract_time
                        .as_ref()
                        .map(duration_value_to_ms),
                    ramp_ms: device
                        .attributes
                        .ramp_time
                        .as_ref()
                        .map(duration_value_to_ms),
                },
            );
        }

        let mut upstream_by_target = HashMap::<String, Vec<String>>::new();
        for edge in topology.graph.edge_references() {
            let source = topology.graph[edge.source()].name.clone();
            let target = topology.graph[edge.target()].name.clone();
            upstream_by_target.entry(target).or_default().push(source);
        }

        Self {
            profiles,
            upstream_by_target,
        }
    }

    fn action_duration_ms(&self, action: &ActionStatement) -> Option<(String, u64)> {
        let (target, action_name, own_duration_ms) = match action {
            ActionStatement::Extend { target } => {
                let profile = self.profiles.get(target)?;
                let own = profile
                    .stroke_ms
                    .or(profile.response_ms)
                    .or(profile.ramp_ms)?;
                (target.as_str(), format!("extend {target}"), own)
            }
            ActionStatement::Retract { target } => {
                let profile = self.profiles.get(target)?;
                let own = profile
                    .retract_ms
                    .or(profile.response_ms)
                    .or(profile.ramp_ms)?;
                (target.as_str(), format!("retract {target}"), own)
            }
            ActionStatement::Set { target, value } => {
                let profile = self.profiles.get(target)?;
                let own = profile.ramp_ms.or(profile.response_ms)?;
                (
                    target.as_str(),
                    format!("set {target} {}", binary_value_text(value)),
                    own,
                )
            }
            ActionStatement::SetAnalog { target, value } => {
                let profile = self.profiles.get(target)?;
                let own = profile.ramp_ms.or(profile.response_ms)?;
                (target.as_str(), format!("set_analog {target} {value}"), own)
            }
            ActionStatement::Log { .. } => return None,
        };

        let upstream_response_ms = self.max_upstream_response_ms(target, &mut HashSet::new());
        let total_ms = own_duration_ms.saturating_add(upstream_response_ms);

        let detail = if upstream_response_ms > 0 {
            format!(
                "{action_name} = 动作本体 {own_duration_ms}ms + 上游 response_time {upstream_response_ms}ms = {total_ms}ms"
            )
        } else {
            format!("{action_name} = {total_ms}ms")
        };

        Some((detail, total_ms))
    }

    fn max_upstream_response_ms(&self, target: &str, visited: &mut HashSet<String>) -> u64 {
        if !visited.insert(target.to_string()) {
            return 0;
        }

        let mut best = 0;
        if let Some(upstream_nodes) = self.upstream_by_target.get(target) {
            for upstream in upstream_nodes {
                let own_response = self
                    .profiles
                    .get(upstream)
                    .and_then(|profile| profile.response_ms)
                    .unwrap_or(0);
                let chain_response = self.max_upstream_response_ms(upstream, visited);
                best = best.max(own_response.saturating_add(chain_response));
            }
        }

        visited.remove(target);
        best
    }
}

fn build_step_estimates(
    program: &PlcProgram,
    context: &TimingContext,
) -> HashMap<String, StepTimingEstimate> {
    let mut estimates = HashMap::new();

    for task in &program.tasks.tasks {
        for step in &task.steps {
            let mut actions = Vec::new();
            collect_actions(&step.statements, &mut actions);

            let mut action_max_ms = 0;
            let mut action_details = Vec::new();
            for action in &actions {
                if let Some((detail, duration_ms)) = context.action_duration_ms(action) {
                    action_max_ms = action_max_ms.max(duration_ms);
                    action_details.push(detail);
                }
            }

            let delay_max_ms = max_delay_ms(&step.statements);
            let timeout_max_ms = max_timeout_ms(&step.statements);
            let nominal_ms = action_max_ms.max(delay_max_ms);
            let worst_case_ms = action_max_ms.max(delay_max_ms).max(timeout_max_ms);

            estimates.insert(
                step_key(&task.name, &step.name),
                StepTimingEstimate {
                    action_max_ms,
                    delay_max_ms,
                    timeout_max_ms,
                    nominal_ms,
                    worst_case_ms,
                    action_details,
                },
            );
        }
    }

    estimates
}

fn build_task_nominal_case(
    program: &PlcProgram,
    step_estimates: &HashMap<String, StepTimingEstimate>,
) -> HashMap<String, u64> {
    let mut task_nominal_case = HashMap::new();

    for task in &program.tasks.tasks {
        let total = task.steps.iter().fold(0u64, |acc, step| {
            let step_nominal = step_estimates
                .get(&step_key(&task.name, &step.name))
                .map(|estimate| estimate.nominal_ms)
                .unwrap_or(0);
            acc.saturating_add(step_nominal)
        });
        task_nominal_case.insert(task.name.clone(), total);
    }

    task_nominal_case
}

fn build_task_worst_case(
    program: &PlcProgram,
    step_estimates: &HashMap<String, StepTimingEstimate>,
) -> HashMap<String, u64> {
    let mut task_worst_case = HashMap::new();

    for task in &program.tasks.tasks {
        let total = task.steps.iter().fold(0u64, |acc, step| {
            let step_worst_case = step_estimates
                .get(&step_key(&task.name, &step.name))
                .map(|estimate| estimate.worst_case_ms)
                .unwrap_or(0);
            acc.saturating_add(step_worst_case)
        });
        task_worst_case.insert(task.name.clone(), total);
    }

    task_worst_case
}

fn shortest_predecessor_interval_ms(
    scope: &TimingScope,
    program: &PlcProgram,
    state_machine: &StateMachine,
) -> (u64, String) {
    let (target_task, target_step) = match scope {
        TimingScope::Task { task } => {
            let Some(step) = initial_step_for_task(program, task) else {
                return (0, format!("未找到 task {task} 的初始 step，按 0ms 处理"));
            };
            (task.as_str(), step)
        }
        TimingScope::Step { task, step } => (task.as_str(), step.as_str()),
    };

    let mut best: Option<(u64, String)> = None;

    for transition in &state_machine.transitions {
        if transition.to.task_name != target_task || transition.to.step_name != target_step {
            continue;
        }

        let interval_ms = transition_guard_min_interval_ms(&transition.guard);
        let detail = format!(
            "{}.{}, guard={} -> {}.{}",
            transition.from.task_name,
            transition.from.step_name,
            transition_guard_name(&transition.guard),
            transition.to.task_name,
            transition.to.step_name
        );

        let replace = best
            .as_ref()
            .map(|(best_interval, _)| interval_ms < *best_interval)
            .unwrap_or(true);

        if replace {
            best = Some((interval_ms, detail));
        }
    }

    if let Some(result) = best {
        return result;
    }

    if state_machine.initial.task_name == target_task
        && state_machine.initial.step_name == target_step
    {
        return (0, "目标是状态机初始状态，无前驱延迟".to_string());
    }

    (0, "未找到前驱转移，按 0ms 处理".to_string())
}

fn transition_guard_min_interval_ms(guard: &TransitionGuard) -> u64 {
    match guard {
        TransitionGuard::Timeout { duration_ms } | TransitionGuard::Delay { duration_ms } => {
            *duration_ms
        }
        TransitionGuard::Always | TransitionGuard::Condition { .. } => 0,
    }
}

fn transition_guard_name(guard: &TransitionGuard) -> String {
    match guard {
        TransitionGuard::Always => "always".to_string(),
        TransitionGuard::Condition { expression } => format!("condition({expression})"),
        TransitionGuard::Timeout { duration_ms } => format!("timeout({duration_ms}ms)"),
        TransitionGuard::Delay { duration_ms } => format!("delay({duration_ms}ms)"),
    }
}

fn collect_actions(statements: &[StepStatement], actions: &mut Vec<ActionStatement>) {
    for statement in statements {
        match statement {
            StepStatement::Action(action) => actions.push(action.clone()),
            StepStatement::IfElse { .. } => {}
            StepStatement::Repeat { body, .. } => collect_actions(body, actions),
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    collect_actions(&branch.statements, actions);
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    collect_actions(&branch.statements, actions);
                }
            }
            StepStatement::Wait(_)
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
}

fn max_timeout_ms(statements: &[StepStatement]) -> u64 {
    let mut timeout_max_ms = 0;

    for statement in statements {
        match statement {
            StepStatement::Timeout(timeout) => {
                timeout_max_ms = timeout_max_ms.max(duration_value_to_ms(&timeout.duration));
            }
            StepStatement::IfElse { .. } => {}
            StepStatement::Repeat { body, .. } => {
                timeout_max_ms = timeout_max_ms.max(max_timeout_ms(body));
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    timeout_max_ms = timeout_max_ms.max(max_timeout_ms(&branch.statements));
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    timeout_max_ms = timeout_max_ms.max(max_timeout_ms(&branch.statements));
                }
            }
            StepStatement::Action(_)
            | StepStatement::Wait(_)
            | StepStatement::Delay { .. }
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }

    timeout_max_ms
}

fn max_delay_ms(statements: &[StepStatement]) -> u64 {
    let mut delay_max_ms = 0;

    for statement in statements {
        match statement {
            StepStatement::Delay { duration_ms } => {
                delay_max_ms = delay_max_ms.max(*duration_ms);
            }
            StepStatement::IfElse { .. } => {}
            StepStatement::Repeat { body, .. } => {
                delay_max_ms = delay_max_ms.max(max_delay_ms(body));
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    delay_max_ms = delay_max_ms.max(max_delay_ms(&branch.statements));
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    delay_max_ms = delay_max_ms.max(max_delay_ms(&branch.statements));
                }
            }
            StepStatement::Action(_)
            | StepStatement::Wait(_)
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }

    delay_max_ms
}

fn step_key(task: &str, step: &str) -> String {
    format!("{task}.{step}")
}

fn format_timing_constraint(rule: &crate::ir::TimingRule) -> String {
    format!(
        "{} {} {}ms",
        format_scope(&rule.scope),
        format_relation(&rule.relation),
        rule.duration_ms
    )
}

fn format_scope(scope: &TimingScope) -> String {
    match scope {
        TimingScope::Task { task } => format!("task.{task}"),
        TimingScope::Step { task, step } => format!("task.{task}.{step}"),
    }
}

fn format_relation(relation: &TimingRelation) -> &'static str {
    match relation {
        TimingRelation::MustCompleteWithin => "must_complete_within",
        TimingRelation::MustCompleteWithinWorstCase => "must_complete_within_worst_case",
        TimingRelation::MustStartAfter => "must_start_after",
    }
}

fn timing_constraint_line(program: &PlcProgram, index: usize) -> usize {
    program
        .constraints
        .timing
        .get(index)
        .map(|constraint| constraint.line.max(1))
        .unwrap_or(1)
}

fn duration_value_to_ms(duration: &crate::ast::DurationValue) -> u64 {
    match duration.unit {
        crate::ast::TimeUnit::Ms => duration.value,
        crate::ast::TimeUnit::S => duration.value.saturating_mul(1000),
    }
}

fn binary_value_text(value: &crate::ast::BinaryValue) -> &'static str {
    match value {
        crate::ast::BinaryValue::On => "on",
        crate::ast::BinaryValue::Off => "off",
    }
}

fn initial_step_for_task<'a>(program: &'a PlcProgram, task_name: &str) -> Option<&'a str> {
    program
        .tasks
        .tasks
        .iter()
        .find(|task| task.name == task_name)
        .and_then(|task| task.steps.first())
        .map(|step| step.name.as_str())
}

#[cfg(test)]
mod tests {
    use super::verify_timing;
    use crate::parser::parse_plc;
    use crate::semantic::{build_constraint_set, build_state_machine, build_topology_graph};

    #[test]
    fn passes_when_step_stroke_time_is_within_constraint() {
        let source = r#"
[topology]

device Y0: digital_output

device valve_A: solenoid_valve {
    driven_by: Y0,
    response_time: 20ms
}

device cyl_A: cylinder {
    driven_by: valve_A,
    stroke_time: 200ms,
    retract_time: 180ms
}

[constraints]

timing: task.init.step_extend_A must_complete_within 500ms

[tasks]

task init:
    step step_extend_A:
        action: extend cyl_A
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应构建成功");
        let constraints = build_constraint_set(&program).expect("约束应构建成功");
        let state_machine = build_state_machine(&program).expect("状态机应构建成功");

        verify_timing(&program, &topology, &constraints, &state_machine)
            .expect("200ms(动作) + 20ms(上游响应) < 500ms 时应通过");
    }

    #[test]
    fn fails_when_step_stroke_time_exceeds_constraint() {
        let source = r#"
[topology]

device Y0: digital_output

device valve_A: solenoid_valve {
    driven_by: Y0,
    response_time: 20ms
}

device cyl_A: cylinder {
    driven_by: valve_A,
    stroke_time: 200ms,
    retract_time: 180ms
}

[constraints]

timing: task.init.step_extend_A must_complete_within 100ms

[tasks]

task init:
    step step_extend_A:
        action: extend cyl_A
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应构建成功");
        let constraints = build_constraint_set(&program).expect("约束应构建成功");
        let state_machine = build_state_machine(&program).expect("状态机应构建成功");

        let errors = verify_timing(&program, &topology, &constraints, &state_machine)
            .expect_err("200ms+20ms 超过 100ms 时应报时序错误");

        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("ERROR [timing] 时序包络违反")),
            "错误信息应包含 timing 标题"
        );
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("无法在 100ms 内完成")),
            "错误结论应指出违反约束"
        );
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("220ms")),
            "错误分析应体现上游 response_time + stroke_time 的链路时间"
        );
    }

    #[test]
    fn fails_when_delay_exceeds_must_complete_within_constraint() {
        let source = r#"
[topology]

[constraints]

timing: task.cooldown must_complete_within 3000ms

[tasks]

task cooldown:
    step wait:
        delay: 5000ms
    step done:
        action: log "ok"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应构建成功");
        let constraints = build_constraint_set(&program).expect("约束应构建成功");
        let state_machine = build_state_machine(&program).expect("状态机应构建成功");

        let errors = verify_timing(&program, &topology, &constraints, &state_machine)
            .expect_err("delay 5000ms 超过 must_complete_within 3000ms 时应报时序错误");

        let joined = errors
            .iter()
            .map(|error| error.to_string())
            .collect::<Vec<_>>()
            .join("\n\n");
        assert!(
            joined.contains("无法在 3000ms 内完成"),
            "错误结论应指出 must_complete_within 违反"
        );
        assert!(joined.contains("5000ms"), "错误分析/结论应体现 delay 时长");
    }

    #[test]
    fn must_complete_within_uses_action_path_while_worst_case_uses_timeout_upper_bound() {
        let source = r#"
[topology]

device Y0: digital_output
device Y1: digital_output

device valve_A: solenoid_valve {
    driven_by: Y0,
    response_time: 50ms
}

device valve_B: solenoid_valve {
    driven_by: Y1,
    response_time: 50ms
}

device cyl_A: cylinder {
    driven_by: valve_A,
    stroke_time: 300ms,
    retract_time: 200ms
}

device cyl_B: cylinder {
    driven_by: valve_B,
    stroke_time: 300ms,
    retract_time: 200ms
}

[constraints]

timing: task.prod must_complete_within 1000ms
timing: task.prod must_complete_within_worst_case 1000ms

[tasks]

task prod:
    step a:
        action: extend cyl_A
        timeout: 600ms -> goto fault
    step b:
        action: extend cyl_B
        timeout: 700ms -> goto fault

task fault:
    step stop:
        action: log "fault"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应构建成功");
        let constraints = build_constraint_set(&program).expect("约束应构建成功");
        let state_machine = build_state_machine(&program).expect("状态机应构建成功");

        let errors = verify_timing(&program, &topology, &constraints, &state_machine)
            .expect_err("worst_case 应在 timeout 累加上界 1300ms 场景下报错");

        assert_eq!(errors.len(), 1, "仅 worst_case 约束应失败");
        let rendered = errors[0].to_string();
        assert!(
            rendered.contains("must_complete_within_worst_case 1000ms"),
            "失败约束应指向 must_complete_within_worst_case"
        );
        assert!(
            rendered.contains("1300ms"),
            "错误分析应体现 timeout 上界关键路径 1300ms"
        );
    }

    #[test]
    fn must_complete_and_worst_case_match_when_no_timeout_exists() {
        let source = r#"
[topology]

device Y0: digital_output

device valve_A: solenoid_valve {
    driven_by: Y0,
    response_time: 20ms
}

device cyl_A: cylinder {
    driven_by: valve_A,
    stroke_time: 200ms,
    retract_time: 180ms
}

[constraints]

timing: task.init must_complete_within 200ms
timing: task.init must_complete_within_worst_case 200ms

[tasks]

task init:
    step extend:
        action: extend cyl_A
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应构建成功");
        let constraints = build_constraint_set(&program).expect("约束应构建成功");
        let state_machine = build_state_machine(&program).expect("状态机应构建成功");

        let errors = verify_timing(&program, &topology, &constraints, &state_machine)
            .expect_err("无 timeout 时，两条约束都应基于相同动作关键路径失败");

        assert_eq!(errors.len(), 2, "两条约束都应失败");
        let joined = errors
            .iter()
            .map(|error| error.to_string())
            .collect::<Vec<_>>()
            .join("\n\n");
        assert!(joined.contains("must_complete_within 200ms"));
        assert!(joined.contains("must_complete_within_worst_case 200ms"));
        assert!(joined.contains("220ms"), "两条约束应共享相同关键路径 220ms");
    }

    #[test]
    fn passes_must_start_after_when_shortest_interval_is_sufficient() {
        let source = r#"
[topology]

[constraints]

timing: task.cooldown must_start_after 200ms

[tasks]

task pre:
    step hold:
        timeout: 300ms -> goto cooldown

task cooldown:
    step begin:
        action: log "ok"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应构建成功");
        let constraints = build_constraint_set(&program).expect("约束应构建成功");
        let state_machine = build_state_machine(&program).expect("状态机应构建成功");

        verify_timing(&program, &topology, &constraints, &state_machine)
            .expect("最短间隔 300ms 足以满足 must_start_after 200ms");
    }

    #[test]
    fn fails_must_start_after_when_shortest_interval_is_insufficient() {
        let source = r#"
[topology]

[constraints]

timing: task.cooldown must_start_after 200ms

[tasks]

task pre:
    step hold:
        timeout: 100ms -> goto cooldown

task cooldown:
    step begin:
        action: log "ok"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应构建成功");
        let constraints = build_constraint_set(&program).expect("约束应构建成功");
        let state_machine = build_state_machine(&program).expect("状态机应构建成功");

        let errors = verify_timing(&program, &topology, &constraints, &state_machine)
            .expect_err("最短间隔不足时应报错");

        assert!(
            errors.iter().any(|error| {
                error
                    .to_string()
                    .contains("无法保证 task.cooldown 在 200ms 后才开始，当前最短间隔为 100ms")
            }),
            "错误应包含 must_start_after 失败模板"
        );
    }
}
