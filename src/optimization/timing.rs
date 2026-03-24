use crate::ast::PlcProgram;
use crate::optimization::CandidateTimingSummary;

pub fn evaluate_candidate_timing(_program: &PlcProgram) -> Result<CandidateTimingSummary, Vec<String>> {
    Err(vec!["not implemented".to_string()])
}
