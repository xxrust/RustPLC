pub mod analyzer;
pub mod emitter;
pub mod ranker;
pub mod rewrite;
pub mod timing;

use crate::ast::{PlcProgram, StepStatement};
use crate::error::PlcError;
use crate::parser::parse_plc;
use crate::semantic::{
    build_constraint_set, build_state_machine, build_topology_graph, preprocess_program,
};
use crate::verification::timing::ProgramTimingEstimate;
use crate::verification::{VerificationIssue, verify_all};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OptimizationOpportunityKind {
    ReorderIndependentSteps,
    ParallelizeIndependentSteps,
    MergeRedundantWait,
    MergeAdjacentDelay,
    ReplaceRecoveryRoute,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OptimizationOpportunity {
    pub kind: OptimizationOpportunityKind,
    pub task: String,
    pub steps: Vec<String>,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CandidateRewriteKind {
    SwapAdjacentSteps,
    ParallelizeAdjacentSteps,
    RemoveRedundantWaitStep,
    MergeAdjacentDelaySteps,
    ReplaceTimeoutRecoveryTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CandidateRewrite {
    pub kind: CandidateRewriteKind,
    pub task: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affected_steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CandidateTimingSummary {
    pub global_nominal_ms: u64,
    pub global_worst_case_ms: u64,
    pub sequential_nominal_ms: u64,
    pub sequential_worst_case_ms: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_tasks: Vec<(String, u64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CandidateLegality {
    pub is_legal: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OptimizationCandidate {
    pub id: String,
    pub rewrite: CandidateRewrite,
    pub timing: CandidateTimingSummary,
    pub legality: CandidateLegality,
    pub wait_points_after: usize,
    pub change_cost: usize,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct OptimizationContext {
    pub original_source: String,
    pub original_program: PlcProgram,
    pub expanded_program: PlcProgram,
}

impl OptimizationContext {
    pub fn from_source(source: &str) -> Result<Self, OptimizationBuildError> {
        let original_program = parse_plc(source)
            .map_err(|err| OptimizationBuildError::Parse(vec![err.to_string()]))?;
        let expanded_program = preprocess_program(&original_program)
            .map_err(|errors| OptimizationBuildError::Semantic(render_plc_errors(errors)))?;

        Ok(Self {
            original_source: source.to_string(),
            original_program,
            expanded_program,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptimizationBuildError {
    Parse(Vec<String>),
    Semantic(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptimizationLegalityError {
    Parse(Vec<String>),
    Semantic(Vec<String>),
    Verification(Vec<String>),
}

pub(crate) fn render_plc_errors(errors: Vec<PlcError>) -> Vec<String> {
    errors.into_iter().map(|error| error.to_string()).collect()
}

#[allow(dead_code)]
pub(crate) fn render_verification_errors(errors: Vec<VerificationIssue>) -> Vec<String> {
    errors.into_iter().map(|error| error.to_string()).collect()
}

#[allow(dead_code)]
pub(crate) fn summarize_program_timing(estimate: &ProgramTimingEstimate) -> CandidateTimingSummary {
    CandidateTimingSummary {
        global_nominal_ms: estimate.concurrent_summary.global_nominal_ms,
        global_worst_case_ms: estimate.concurrent_summary.global_worst_case_ms,
        sequential_nominal_ms: estimate.concurrent_summary.sequential_nominal_ms,
        sequential_worst_case_ms: estimate.concurrent_summary.sequential_worst_case_ms,
        active_tasks: estimate.concurrent_summary.active_nominal_by_task.clone(),
    }
}

#[allow(dead_code)]
pub(crate) fn recheck_source_legality(
    source: &str,
) -> Result<(PlcProgram, ProgramTimingEstimate), OptimizationLegalityError> {
    let program =
        parse_plc(source).map_err(|err| OptimizationLegalityError::Parse(vec![err.to_string()]))?;
    let expanded = preprocess_program(&program)
        .map_err(|errors| OptimizationLegalityError::Semantic(render_plc_errors(errors)))?;
    let topology = build_topology_graph(&expanded)
        .map_err(|errors| OptimizationLegalityError::Semantic(render_plc_errors(errors)))?;
    let constraints = build_constraint_set(&expanded)
        .map_err(|errors| OptimizationLegalityError::Semantic(render_plc_errors(errors)))?;
    let state_machine = build_state_machine(&expanded)
        .map_err(|errors| OptimizationLegalityError::Semantic(render_plc_errors(errors)))?;
    verify_all(&expanded, &topology, &constraints, &state_machine).map_err(|errors| {
        OptimizationLegalityError::Verification(render_verification_errors(errors))
    })?;
    let estimate =
        crate::verification::timing::estimate_program_timing(&expanded, &topology, &state_machine);

    Ok((expanded, estimate))
}

pub fn optimize_plc_source(
    source: &str,
) -> Result<Vec<OptimizationCandidate>, OptimizationBuildError> {
    let context = OptimizationContext::from_source(source)?;
    let opportunities = analyzer::analyze_optimization_opportunities(&context.expanded_program);
    let generated = rewrite::generate_candidate_rewrites(&context.expanded_program, &opportunities);
    let mut candidates = Vec::new();

    for (index, (rewrite, candidate_program)) in generated.into_iter().enumerate() {
        let timing = timing::evaluate_candidate_timing(&candidate_program)
            .map_err(OptimizationBuildError::Semantic)?;
        let legality = emitter::recheck_candidate_legality(&candidate_program);
        let source = emitter::emit_optimized_plc(&context.original_source, &candidate_program)
            .map_err(|error| OptimizationBuildError::Semantic(vec![error]))?;
        let change_cost = rewrite.affected_steps.len().max(1);
        candidates.push(OptimizationCandidate {
            id: format!("candidate_{:03}", index + 1),
            rewrite,
            timing,
            legality,
            wait_points_after: count_wait_points(&candidate_program),
            change_cost,
            source,
        });
    }

    ranker::rank_candidates(&mut candidates);
    Ok(candidates)
}

fn count_wait_points(program: &PlcProgram) -> usize {
    program
        .tasks
        .tasks
        .iter()
        .flat_map(|task| task.steps.iter())
        .map(|step| count_wait_points_in_statements(&step.statements))
        .sum()
}

fn count_wait_points_in_statements(statements: &[StepStatement]) -> usize {
    statements
        .iter()
        .map(|statement| match statement {
            StepStatement::Wait(_)
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::AllowIndefiniteWait(_) => 1,
            StepStatement::Action(_)
            | StepStatement::Effect(_)
            | StepStatement::Goto(_)
            | StepStatement::IfElse { .. } => 0,
            StepStatement::Repeat { body, .. } => count_wait_points_in_statements(body),
            StepStatement::Parallel(block) => block
                .branches
                .iter()
                .map(|branch| count_wait_points_in_statements(&branch.statements))
                .sum(),
            StepStatement::Race(block) => block
                .branches
                .iter()
                .map(|branch| count_wait_points_in_statements(&branch.statements))
                .sum(),
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::optimize_plc_source;

    #[test]
    fn optimize_plc_source_returns_ranked_parseable_candidates() {
        let source = r#"
[topology]

device Y0: digital_output
device Y1: digital_output

[constraints]

[tasks]

task main:
    step prep_a:
        action: set Y0 on
    step prep_b:
        action: set Y1 on
    step wait_sensor:
        wait: sensor_ready == true
        timeout: 200ms -> goto fault
    step wait_sensor_again:
        wait: sensor_ready == true
        timeout: 200ms -> goto fault

task fault:
    step safe_stop:
        delay: 20ms
"#;

        let candidates = optimize_plc_source(source).expect("optimize");
        assert!(!candidates.is_empty(), "expected optimization candidates");
        assert!(
            candidates
                .windows(2)
                .all(|pair| { pair[0].legality.is_legal >= pair[1].legality.is_legal })
        );
        assert!(
            candidates
                .iter()
                .all(|candidate| !candidate.source.is_empty())
        );
        assert!(
            candidates
                .iter()
                .all(|candidate| candidate.source.contains("[tasks]"))
        );
    }
}
