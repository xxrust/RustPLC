use serde::{Deserialize, Serialize};

use super::report::{
    IntentAlignmentBlockerKind, IntentAlignmentReport, IntentAlignmentVerdict, IntentMismatchKind,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentAlignmentPipelineSummary {
    pub verdict: IntentAlignmentVerdict,
    pub primary_mismatch_kind: Option<IntentMismatchKind>,
    pub mismatch_count: usize,
    pub blocker_kind: Option<IntentAlignmentBlockerKind>,
    pub comparator_version: String,
}

pub fn reduce_intent_alignment_report(
    report: &IntentAlignmentReport,
) -> IntentAlignmentPipelineSummary {
    IntentAlignmentPipelineSummary {
        verdict: report.verdict,
        primary_mismatch_kind: report.primary_mismatch.as_ref().map(|m| m.kind),
        mismatch_count: report.mismatches.len(),
        blocker_kind: report.blocker_kind,
        comparator_version: report.comparator_version.clone(),
    }
}
