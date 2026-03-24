use crate::ast::PlcProgram;

pub fn emit_optimized_plc(_original_source: &str, _program: &PlcProgram) -> Result<String, String> {
    Err("not implemented".to_string())
}

pub fn recheck_candidate_legality(_source: &str) -> Result<crate::optimization::CandidateTimingSummary, Vec<String>> {
    Err(vec!["not implemented".to_string()])
}
