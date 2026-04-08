pub mod contract;
pub mod expected_behavior;

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

pub use expected_behavior::{
    ExpectedBehaviorCompileError, ExpectedBehaviorIrEdge, ExpectedBehaviorIrPrimitiveKind,
    ExpectedBehaviorIrView, ExpectedBehaviorSpec, ExpectedCycleHandoffIrView,
    ExpectedCycleSemantics, ExpectedMilestoneGraphView, ExpectedMilestoneIrNode,
    ExpectedMilestoneSemanticRole, ExpectedPostconditionIrView, ExpectedRestartability,
    compile_expected_behavior_spec,
};
