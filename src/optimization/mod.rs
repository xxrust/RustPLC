pub mod analyzer;
pub mod emitter;
pub mod ranker;
pub mod rewrite;
pub mod timing;

use crate::ast::PlcProgram;
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
        let original_program =
            parse_plc(source).map_err(|err| OptimizationBuildError::Parse(vec![err.to_string()]))?;
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
    verify_all(&expanded, &topology, &constraints, &state_machine)
        .map_err(|errors| OptimizationLegalityError::Verification(render_verification_errors(errors)))?;
    let estimate =
        crate::verification::timing::estimate_program_timing(&expanded, &topology, &state_machine);

    Ok((expanded, estimate))
}
