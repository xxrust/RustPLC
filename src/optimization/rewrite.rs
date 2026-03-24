use crate::ast::PlcProgram;
use crate::optimization::{CandidateRewrite, OptimizationOpportunity};

pub fn generate_candidate_rewrites(
    _program: &PlcProgram,
    _opportunities: &[OptimizationOpportunity],
) -> Vec<(CandidateRewrite, PlcProgram)> {
    Vec::new()
}
