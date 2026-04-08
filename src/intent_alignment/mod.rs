pub mod contract;
pub mod compare;
pub mod expected_behavior;
pub mod observed;
pub mod pipeline;
pub mod report;

pub use contract::{
    BusinessMilestone, ContractCycleSemantics, ContractMetadata, ContractReviewInput,
    ContractSourceDigest, ContractSourceKind, ContractSourceRef, IntentContract,
    IntentContractCore, IntentContractDiagnostic, IntentContractDiagnosticCode,
    IntentContractLoadError, IntentContractValidationError, IntentMilestone, IntentPostcondition,
    MilestoneEvidenceSource, ObservationBinding, ObservationCombination, ObservationSubject,
    ObservedEvidence, ObservedMilestoneEvidence, RequiredMilestoneEdge, RestartSemantics,
    parse_intent_contract_str, read_intent_contract, validate_intent_contract,
    verify_intent_contract_source_binding,
};

pub use compare::{
    IntentAlignmentCompareInputError, compare_intent_alignment, compare_trace_jsonl,
};

pub use expected_behavior::{
    ExpectedBehaviorCompileError, ExpectedBehaviorIrEdge, ExpectedBehaviorIrPrimitiveKind,
    ExpectedBehaviorIrView, ExpectedBehaviorSpec, ExpectedCycleHandoffInvariant,
    ExpectedCycleHandoffIrView, ExpectedCycleSemantics, ExpectedMilestoneGraphView,
    ExpectedMilestoneIrNode, ExpectedMilestoneSemanticRole, ExpectedPostconditionIrView,
    ExpectedPostconditionPredicate, ExpectedRestartCondition, ExpectedRestartability,
    ObservedFact, PredicateExpr, compile_expected_behavior_spec,
};

pub use observed::{
    ObservedBehaviorSequence, ObservedComparisonDimension, ObservedCycleWindow,
    ObservedDimensionReadiness, ObservedEventSourceKind, ObservedEvidenceEntry,
    ObservedEvidenceGap, ObservedEvidenceGapCode, ObservedEvidenceThresholds,
    ObservedSnapshot, ObservedTraceParseError, RawObservedEvent,
    adapt_normalized_trace_events, extract_observed_behavior_sequence,
    parse_observed_trace_jsonl,
};

pub use pipeline::{IntentAlignmentPipelineSummary, reduce_intent_alignment_report};

pub use report::{
    INTENT_ALIGNMENT_COMPARATOR_VERSION, IntentAlignmentBlockerKind,
    IntentAlignmentContractIdentity, IntentAlignmentCycleWindow,
    IntentAlignmentEvidenceIdentity, IntentAlignmentEvidenceKind, IntentAlignmentReport,
    IntentAlignmentVerdict, IntentMismatch, IntentMismatchKind,
};
