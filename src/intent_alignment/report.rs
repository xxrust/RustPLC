use serde::{Deserialize, Serialize};

pub const INTENT_ALIGNMENT_COMPARATOR_VERSION: &str = "phase-2.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentMismatchKind {
    MissingRequiredStep,
    WrongOrder,
    DuplicatedRequiredStep,
    UnexpectedObservedStep,
    PrematureReadiness,
    PostconditionNotMet,
    CrossCycleDrift,
}

impl IntentMismatchKind {
    pub fn priority(self) -> usize {
        match self {
            Self::MissingRequiredStep => 0,
            Self::WrongOrder => 1,
            Self::DuplicatedRequiredStep => 2,
            Self::UnexpectedObservedStep => 3,
            Self::PrematureReadiness => 4,
            Self::PostconditionNotMet => 5,
            Self::CrossCycleDrift => 6,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentMismatch {
    pub kind: IntentMismatchKind,
    pub subject: String,
    pub detail: String,
    pub cycle_index: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentAlignmentVerdict {
    Aligned,
    Mismatch,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentAlignmentBlockerKind {
    MissingEvidence,
    MissingComparator,
    ToolchainLimitation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentAlignmentEvidenceKind {
    InlineTraceJsonl,
    ObservedSequence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentAlignmentContractIdentity {
    pub contract_id: String,
    pub contract_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentAlignmentEvidenceIdentity {
    pub kind: IntentAlignmentEvidenceKind,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentAlignmentCycleWindow {
    pub first_cycle_index: usize,
    pub last_cycle_index: usize,
    pub cycle_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentAlignmentReport {
    pub contract_identity: IntentAlignmentContractIdentity,
    pub evidence_identity: IntentAlignmentEvidenceIdentity,
    pub comparator_version: String,
    pub cycle_window: IntentAlignmentCycleWindow,
    pub verdict: IntentAlignmentVerdict,
    pub primary_mismatch: Option<IntentMismatch>,
    pub mismatches: Vec<IntentMismatch>,
    pub blocked_reason: Option<String>,
    pub blocker_kind: Option<IntentAlignmentBlockerKind>,
    pub warnings: Vec<String>,
}
