use crate::ast::{
    ActionStatement, ComparisonOperator, ConditionExpression, GotoDirective, PlcProgram,
    StepDeclaration, StepStatement, TaskDeclaration, TimeUnit, TimeoutDirective, WaitCondition,
    WaitStatement,
};
use crate::optimization::{OptimizationOpportunity, OptimizationOpportunityKind};
use std::collections::{BTreeSet, HashMap};

#[derive(Debug, Clone, Default)]
struct StepProfile {
    statements: usize,
    blocking: bool,
    has_complex_control_flow: bool,
    has_axis_motion: bool,
    action_targets: BTreeSet<String>,
    effect_targets: BTreeSet<String>,
    wait_signature: Option<String>,
    timeout_signature: Option<String>,
    total_delay_ms: u64,
}

pub fn analyze_optimization_opportunities(program: &PlcProgram) -> Vec<OptimizationOpportunity> {
    let task_timings = build_task_delay_footprint(program);
    let mut opportunities = Vec::new();

    for task in &program.tasks.tasks {
        for window in task.steps.windows(2) {
            let first = &window[0];
            let second = &window[1];
            let first_profile = profile_step(first);
            let second_profile = profile_step(second);

            if can_reorder_or_parallelize(&first_profile, &second_profile) {
                let shared_details = vec![
                    format!(
                        "{} 与 {} 都是非阻塞 immediate step",
                        first.name, second.name
                    ),
                    "两步没有 wait/delay/timeout/goto/parallel/race/axis.move".to_string(),
                    "两步动作/资源目标集合互不重叠".to_string(),
                ];
                opportunities.push(OptimizationOpportunity {
                    kind: OptimizationOpportunityKind::ReorderIndependentSteps,
                    task: task.name.clone(),
                    steps: vec![first.name.clone(), second.name.clone()],
                    summary: format!(
                        "task {} 的相邻 step {} / {} 可以调序",
                        task.name, first.name, second.name
                    ),
                    details: shared_details.clone(),
                });
                opportunities.push(OptimizationOpportunity {
                    kind: OptimizationOpportunityKind::ParallelizeIndependentSteps,
                    task: task.name.clone(),
                    steps: vec![first.name.clone(), second.name.clone()],
                    summary: format!(
                        "task {} 的相邻 step {} / {} 可以从串行改并行",
                        task.name, first.name, second.name
                    ),
                    details: shared_details,
                });
            }

            if let Some(wait_signature) = &first_profile.wait_signature {
                if second_profile.wait_signature.as_ref() == Some(wait_signature)
                    && first_profile.timeout_signature == second_profile.timeout_signature
                    && is_wait_only_profile(&first_profile)
                    && is_wait_only_profile(&second_profile)
                {
                    opportunities.push(OptimizationOpportunity {
                        kind: OptimizationOpportunityKind::MergeRedundantWait,
                        task: task.name.clone(),
                        steps: vec![first.name.clone(), second.name.clone()],
                        summary: format!(
                            "task {} 的连续等待 step {} / {} 可以合并",
                            task.name, first.name, second.name
                        ),
                        details: vec![
                            format!("两步 wait 条件完全一致：{wait_signature}"),
                            match &first_profile.timeout_signature {
                                Some(timeout) => format!("两步 timeout 路由也一致：{timeout}"),
                                None => "两步都没有 timeout".to_string(),
                            },
                        ],
                    });
                }
            }

            if is_delay_only_profile(&first_profile) && is_delay_only_profile(&second_profile) {
                opportunities.push(OptimizationOpportunity {
                    kind: OptimizationOpportunityKind::MergeAdjacentDelay,
                    task: task.name.clone(),
                    steps: vec![first.name.clone(), second.name.clone()],
                    summary: format!(
                        "task {} 的连续 delay step {} / {} 可以合并",
                        task.name, first.name, second.name
                    ),
                    details: vec![format!(
                        "delay 可从 {}ms + {}ms 收敛为 {}ms",
                        first_profile.total_delay_ms,
                        second_profile.total_delay_ms,
                        first_profile
                            .total_delay_ms
                            .saturating_add(second_profile.total_delay_ms)
                    )],
                });
            }
        }

        opportunities.extend(analyze_recovery_route_replacements(task, &task_timings));
    }

    opportunities
}

fn build_task_delay_footprint(program: &PlcProgram) -> HashMap<String, u64> {
    let mut totals = HashMap::new();
    for task in &program.tasks.tasks {
        let total = task
            .steps
            .iter()
            .map(profile_step)
            .map(|profile| profile.total_delay_ms)
            .sum();
        totals.insert(task.name.clone(), total);
    }
    totals
}

fn analyze_recovery_route_replacements(
    task: &TaskDeclaration,
    task_timings: &HashMap<String, u64>,
) -> Vec<OptimizationOpportunity> {
    let mut opportunities = Vec::new();
    for (step_name, timeout, current_target) in collect_timeout_recovery_targets(task) {
        let Some(current_total) = task_timings.get(&current_target).copied() else {
            continue;
        };
        let mut alternatives = task_timings
            .iter()
            .filter(|(candidate, _)| candidate.as_str() != current_target)
            .filter(|(_, total)| **total < current_total)
            .map(|(candidate, total)| (candidate.clone(), *total))
            .collect::<Vec<_>>();
        alternatives.sort_by_key(|(_, total)| *total);
        let Some((candidate, candidate_total)) = alternatives.first() else {
            continue;
        };
        opportunities.push(OptimizationOpportunity {
            kind: OptimizationOpportunityKind::ReplaceRecoveryRoute,
            task: task.name.clone(),
            steps: vec![step_name.clone()],
            summary: format!(
                "task {} 的 timeout 恢复路由 {} -> {} 可替换为更短路径",
                task.name, current_target, candidate
            ),
            details: vec![
                format!("step {step_name} 当前 timeout: {timeout}"),
                format!("当前恢复 task {current_target} 的 delay 足迹为 {current_total}ms"),
                format!("候选恢复 task {candidate} 的 delay 足迹为 {candidate_total}ms"),
            ],
        });
    }
    opportunities
}

fn collect_timeout_recovery_targets(task: &TaskDeclaration) -> Vec<(String, String, String)> {
    let mut targets = Vec::new();
    for step in &task.steps {
        for statement in &step.statements {
            if let StepStatement::Timeout(TimeoutDirective { duration, target }) = statement {
                targets.push((
                    step.name.clone(),
                    format_duration(duration.value, duration.unit.clone()),
                    render_goto(target),
                ));
            }
        }
    }
    targets
}

fn profile_step(step: &StepDeclaration) -> StepProfile {
    let mut profile = StepProfile {
        statements: step.statements.len(),
        ..StepProfile::default()
    };
    for statement in &step.statements {
        match statement {
            StepStatement::Action(action) => {
                collect_action_target(action, &mut profile.action_targets);
                if matches!(
                    action,
                    ActionStatement::AxisMoveRelative { .. }
                        | ActionStatement::AxisMoveAbsolute { .. }
                ) {
                    profile.blocking = true;
                    profile.has_axis_motion = true;
                }
            }
            StepStatement::Effect(effect) => {
                profile.effect_targets.insert(format!("{effect:?}"));
            }
            StepStatement::Wait(wait) => {
                profile.blocking = true;
                profile.wait_signature = Some(render_wait(wait));
            }
            StepStatement::Delay { duration_ms } => {
                profile.blocking = true;
                profile.total_delay_ms = profile.total_delay_ms.saturating_add(*duration_ms);
            }
            StepStatement::Timeout(timeout) => {
                profile.blocking = true;
                profile.timeout_signature = Some(render_timeout(timeout));
            }
            StepStatement::Goto(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Parallel(_)
            | StepStatement::Race(_)
            | StepStatement::Repeat { .. } => {
                profile.has_complex_control_flow = true;
            }
            StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
    profile
}

fn collect_action_target(action: &ActionStatement, targets: &mut BTreeSet<String>) {
    match action {
        ActionStatement::Extend { target, .. }
        | ActionStatement::Retract { target, .. }
        | ActionStatement::Set { target, .. }
        | ActionStatement::SetAnalog { target, .. }
        | ActionStatement::SetAnalogExpr { target, .. }
        | ActionStatement::AxisMoveRelative { target, .. }
        | ActionStatement::AxisMoveAbsolute { target, .. } => {
            targets.insert(target.to_string());
        }
        ActionStatement::Compute { target, .. } => {
            targets.insert(format!("var:{target}"));
        }
        ActionStatement::Call {
            function, binding, ..
        } => {
            targets.insert(format!("extern:{function}"));
            targets.insert(format!("binding:{binding:?}"));
        }
        ActionStatement::CamEngage { target }
        | ActionStatement::CamDisengage { target }
        | ActionStatement::CamSwitch { target, .. }
        | ActionStatement::CamPhase { target, .. } => {
            targets.insert(format!("cam:{target}"));
        }
        ActionStatement::Log { message } => {
            targets.insert(format!("log:{message}"));
        }
    }
}

fn can_reorder_or_parallelize(first: &StepProfile, second: &StepProfile) -> bool {
    is_simple_immediate_profile(first)
        && is_simple_immediate_profile(second)
        && first.action_targets.is_disjoint(&second.action_targets)
        && first.effect_targets.is_disjoint(&second.effect_targets)
}

fn is_simple_immediate_profile(profile: &StepProfile) -> bool {
    profile.statements > 0
        && !profile.blocking
        && !profile.has_complex_control_flow
        && !profile.has_axis_motion
        && profile.wait_signature.is_none()
        && profile.timeout_signature.is_none()
        && profile.total_delay_ms == 0
}

fn is_wait_only_profile(profile: &StepProfile) -> bool {
    profile.wait_signature.is_some()
        && !profile.has_complex_control_flow
        && profile.action_targets.is_empty()
        && profile.effect_targets.is_empty()
        && profile.total_delay_ms == 0
}

fn is_delay_only_profile(profile: &StepProfile) -> bool {
    profile.total_delay_ms > 0
        && !profile.has_complex_control_flow
        && profile.action_targets.is_empty()
        && profile.effect_targets.is_empty()
        && profile.wait_signature.is_none()
}

fn render_wait(wait: &WaitStatement) -> String {
    match &wait.condition {
        WaitCondition::Single(condition) => render_condition(condition),
        WaitCondition::And(conditions) => conditions
            .iter()
            .map(render_condition)
            .collect::<Vec<_>>()
            .join(" AND "),
        WaitCondition::Or(conditions) => conditions
            .iter()
            .map(render_condition)
            .collect::<Vec<_>>()
            .join(" OR "),
    }
}

fn render_condition(condition: &ConditionExpression) -> String {
    if let Some((left, right)) = condition.expression_pair() {
        return format!(
            "{left:?} {} {right:?}",
            render_operator(condition.operator.clone())
        );
    }
    format!(
        "{} {} {:?}",
        condition.left,
        render_operator(condition.operator.clone()),
        condition.right
    )
}

fn render_operator(operator: ComparisonOperator) -> &'static str {
    match operator {
        ComparisonOperator::Eq => "==",
        ComparisonOperator::Neq => "!=",
        ComparisonOperator::Gt => ">",
        ComparisonOperator::Lt => "<",
        ComparisonOperator::Gte => ">=",
        ComparisonOperator::Lte => "<=",
    }
}

fn render_timeout(timeout: &TimeoutDirective) -> String {
    format!(
        "{} -> {}",
        format_duration(timeout.duration.value, timeout.duration.unit.clone()),
        render_goto(&timeout.target)
    )
}

fn format_duration(value: u64, unit: TimeUnit) -> String {
    match unit {
        TimeUnit::Ms => format!("{value}ms"),
        TimeUnit::S => format!("{value}s"),
    }
}

fn render_goto(target: &GotoDirective) -> String {
    match &target.step {
        Some(step) => format!("{}.{}", target.task, step),
        None => target.task.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::analyze_optimization_opportunities;
    use crate::optimization::OptimizationOpportunityKind;
    use crate::parser::parse_plc;
    use crate::semantic::preprocess_program;

    #[test]
    fn finds_reorder_parallel_wait_delay_and_recovery_opportunities() {
        let source = r#"
[topology]

[constraints]

[tasks]

task main:
    step prep_a:
        action: set Y0 on
    step prep_b:
        action: set Y1 on
    step wait_sensor:
        wait: sensor_ready == true
        timeout: 200ms -> goto slow_fault
    step wait_sensor_again:
        wait: sensor_ready == true
        timeout: 200ms -> goto slow_fault
    step dwell_a:
        delay: 20ms
    step dwell_b:
        delay: 30ms

task slow_fault:
    step cool_down:
        delay: 200ms

task fast_fault:
    step safe_stop:
        delay: 20ms
"#;

        let program = parse_plc(source).expect("parse");
        let expanded = preprocess_program(&program).expect("preprocess");
        let opportunities = analyze_optimization_opportunities(&expanded);

        assert!(opportunities.iter().any(|item| {
            item.summary.contains("prep_a / prep_b")
                && item.kind == OptimizationOpportunityKind::ReorderIndependentSteps
        }));
        assert!(opportunities.iter().any(|item| {
            item.summary.contains("prep_a / prep_b")
                && item.kind == OptimizationOpportunityKind::ParallelizeIndependentSteps
        }));
        assert!(
            opportunities
                .iter()
                .any(|item| item.kind == OptimizationOpportunityKind::MergeRedundantWait)
        );
        assert!(
            opportunities
                .iter()
                .any(|item| item.kind == OptimizationOpportunityKind::MergeAdjacentDelay)
        );
        assert!(opportunities.iter().any(|item| {
            item.kind == OptimizationOpportunityKind::ReplaceRecoveryRoute
                && item.summary.contains("slow_fault -> fast_fault")
        }));
    }
}
