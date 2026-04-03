use crate::ast::{
    Branch, ParallelBlock, PlcProgram, StepDeclaration, StepStatement, TimeoutDirective,
};
use crate::optimization::{
    CandidateRewrite, CandidateRewriteKind, OptimizationOpportunity, OptimizationOpportunityKind,
};

pub fn generate_candidate_rewrites(
    program: &PlcProgram,
    opportunities: &[OptimizationOpportunity],
) -> Vec<(CandidateRewrite, PlcProgram)> {
    let mut candidates = Vec::new();

    for opportunity in opportunities {
        let candidate = match opportunity.kind {
            OptimizationOpportunityKind::ReorderIndependentSteps => {
                swap_adjacent_steps(program, opportunity)
            }
            OptimizationOpportunityKind::ParallelizeIndependentSteps => {
                parallelize_adjacent_steps(program, opportunity)
            }
            OptimizationOpportunityKind::MergeRedundantWait => {
                remove_redundant_wait_step(program, opportunity)
            }
            OptimizationOpportunityKind::MergeAdjacentDelay => {
                merge_adjacent_delay_steps(program, opportunity)
            }
            OptimizationOpportunityKind::ReplaceRecoveryRoute => {
                replace_timeout_recovery_route(program, opportunity)
            }
        };

        if let Some(candidate) = candidate {
            candidates.push(candidate);
        }
    }

    candidates
}

fn swap_adjacent_steps(
    program: &PlcProgram,
    opportunity: &OptimizationOpportunity,
) -> Option<(CandidateRewrite, PlcProgram)> {
    let [first, second] = opportunity.steps.as_slice() else {
        return None;
    };
    let mut candidate = program.clone();
    let task = candidate
        .tasks
        .tasks
        .iter_mut()
        .find(|task| task.name == opportunity.task)?;
    let first_idx = task.steps.iter().position(|step| step.name == *first)?;
    let second_idx = task.steps.iter().position(|step| step.name == *second)?;
    if second_idx != first_idx + 1 {
        return None;
    }
    task.steps.swap(first_idx, second_idx);

    Some((
        CandidateRewrite {
            kind: CandidateRewriteKind::SwapAdjacentSteps,
            task: opportunity.task.clone(),
            summary: opportunity.summary.clone(),
            affected_steps: opportunity.steps.clone(),
        },
        candidate,
    ))
}

fn parallelize_adjacent_steps(
    program: &PlcProgram,
    opportunity: &OptimizationOpportunity,
) -> Option<(CandidateRewrite, PlcProgram)> {
    let [first, second] = opportunity.steps.as_slice() else {
        return None;
    };
    let mut candidate = program.clone();
    let task = candidate
        .tasks
        .tasks
        .iter_mut()
        .find(|task| task.name == opportunity.task)?;
    let first_idx = task.steps.iter().position(|step| step.name == *first)?;
    let second_idx = task.steps.iter().position(|step| step.name == *second)?;
    if second_idx != first_idx + 1 {
        return None;
    }

    let second_step = task.steps.remove(second_idx);
    let first_step = task.steps.get_mut(first_idx)?;
    let first_branch = Branch {
        statements: first_step.statements.clone(),
    };
    let second_branch = Branch {
        statements: second_step.statements,
    };
    first_step.statements = vec![StepStatement::Parallel(ParallelBlock {
        branches: vec![first_branch, second_branch],
    })];

    Some((
        CandidateRewrite {
            kind: CandidateRewriteKind::ParallelizeAdjacentSteps,
            task: opportunity.task.clone(),
            summary: opportunity.summary.clone(),
            affected_steps: opportunity.steps.clone(),
        },
        candidate,
    ))
}

fn remove_redundant_wait_step(
    program: &PlcProgram,
    opportunity: &OptimizationOpportunity,
) -> Option<(CandidateRewrite, PlcProgram)> {
    let [_, second] = opportunity.steps.as_slice() else {
        return None;
    };
    let mut candidate = program.clone();
    let task = candidate
        .tasks
        .tasks
        .iter_mut()
        .find(|task| task.name == opportunity.task)?;
    let second_idx = task.steps.iter().position(|step| step.name == *second)?;
    task.steps.remove(second_idx);

    Some((
        CandidateRewrite {
            kind: CandidateRewriteKind::RemoveRedundantWaitStep,
            task: opportunity.task.clone(),
            summary: opportunity.summary.clone(),
            affected_steps: opportunity.steps.clone(),
        },
        candidate,
    ))
}

fn merge_adjacent_delay_steps(
    program: &PlcProgram,
    opportunity: &OptimizationOpportunity,
) -> Option<(CandidateRewrite, PlcProgram)> {
    let [first, second] = opportunity.steps.as_slice() else {
        return None;
    };
    let mut candidate = program.clone();
    let task = candidate
        .tasks
        .tasks
        .iter_mut()
        .find(|task| task.name == opportunity.task)?;
    let first_idx = task.steps.iter().position(|step| step.name == *first)?;
    let second_idx = task.steps.iter().position(|step| step.name == *second)?;
    if second_idx != first_idx + 1 {
        return None;
    }
    let merged_delay = total_delay_ms(&task.steps[first_idx])
        .saturating_add(total_delay_ms(&task.steps[second_idx]));
    task.steps[first_idx].statements = vec![StepStatement::Delay {
        duration_ms: merged_delay,
    }];
    task.steps.remove(second_idx);

    Some((
        CandidateRewrite {
            kind: CandidateRewriteKind::MergeAdjacentDelaySteps,
            task: opportunity.task.clone(),
            summary: opportunity.summary.clone(),
            affected_steps: opportunity.steps.clone(),
        },
        candidate,
    ))
}

fn replace_timeout_recovery_route(
    program: &PlcProgram,
    opportunity: &OptimizationOpportunity,
) -> Option<(CandidateRewrite, PlcProgram)> {
    let step_name = opportunity.steps.first()?;
    let alternative = opportunity.replacement_task.clone()?;

    let mut candidate = program.clone();
    let task = candidate
        .tasks
        .tasks
        .iter_mut()
        .find(|task| task.name == opportunity.task)?;
    let step = task.steps.iter_mut().find(|step| step.name == *step_name)?;
    for statement in &mut step.statements {
        if let StepStatement::Timeout(TimeoutDirective { target, .. }) = statement {
            target.task = alternative.clone();
            target.step = None;
            return Some((
                CandidateRewrite {
                    kind: CandidateRewriteKind::ReplaceTimeoutRecoveryTarget,
                    task: opportunity.task.clone(),
                    summary: opportunity.summary.clone(),
                    affected_steps: opportunity.steps.clone(),
                },
                candidate,
            ));
        }
    }
    None
}

fn total_delay_ms(step: &StepDeclaration) -> u64 {
    step.statements
        .iter()
        .filter_map(|statement| match statement {
            StepStatement::Delay { duration_ms } => Some(*duration_ms),
            _ => None,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::generate_candidate_rewrites;
    use crate::ast::{StepStatement, TimeoutDirective};
    use crate::optimization::CandidateRewriteKind;
    use crate::optimization::analyzer::analyze_optimization_opportunities;
    use crate::parser::parse_plc;
    use crate::semantic::preprocess_program;

    #[test]
    fn generates_conservative_rewrites_from_phase1_opportunities() {
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
        let candidates = generate_candidate_rewrites(&expanded, &opportunities);

        assert!(candidates.iter().any(|(rewrite, candidate)| {
            rewrite.kind == CandidateRewriteKind::SwapAdjacentSteps
                && candidate.tasks.tasks[0].steps[0].name == "prep_b"
        }));
        assert!(candidates.iter().any(|(rewrite, candidate)| {
            rewrite.kind == CandidateRewriteKind::ParallelizeAdjacentSteps
                && matches!(
                    candidate.tasks.tasks[0].steps[0].statements.first(),
                    Some(StepStatement::Parallel(_))
                )
        }));
        assert!(candidates.iter().any(|(rewrite, candidate)| {
            rewrite.kind == CandidateRewriteKind::RemoveRedundantWaitStep
                && candidate.tasks.tasks[0]
                    .steps
                    .iter()
                    .all(|step| step.name != "wait_sensor_again")
        }));
        assert!(candidates.iter().any(|(rewrite, candidate)| {
            rewrite.kind == CandidateRewriteKind::MergeAdjacentDelaySteps
                && matches!(
                    candidate.tasks.tasks[0]
                        .steps
                        .iter()
                        .find(|step| step.name == "dwell_a")
                        .and_then(|step| step.statements.first()),
                    Some(StepStatement::Delay { duration_ms: 50 })
                )
        }));
        assert!(candidates.iter().any(|(rewrite, candidate)| {
            rewrite.kind == CandidateRewriteKind::ReplaceTimeoutRecoveryTarget
                && candidate.tasks.tasks[0]
                    .steps
                    .iter()
                    .find(|step| step.name == "wait_sensor")
                    .is_some_and(|step| {
                        step.statements.iter().any(|statement| {
                            matches!(
                                statement,
                                StepStatement::Timeout(TimeoutDirective { target, .. })
                                    if target.task == "fast_fault"
                            )
                        })
                    })
        }));
    }
}
