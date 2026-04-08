use std::path::PathBuf;

use rust_plc::intent_alignment::{
    IntentAlignmentVerdict, IntentMismatchKind, compare_trace_jsonl, compile_expected_behavior_spec,
    read_intent_contract,
};

fn workspace_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn compare_fixture(
    contract_relative: &str,
    evidence_relative: &str,
) -> rust_plc::intent_alignment::IntentAlignmentReport {
    let contract = read_intent_contract(workspace_path(contract_relative)).expect("contract");
    let spec = compile_expected_behavior_spec(&contract).expect("spec");
    let evidence = std::fs::read_to_string(workspace_path(evidence_relative)).expect("evidence");
    compare_trace_jsonl(&spec, &evidence).expect("compare")
}

#[test]
fn canonical_and_mutation_regressions_cover_frozen_phase2_semantics() {
    let aligned = compare_fixture(
        "tests/fixtures/intent_alignment/contracts/two_cylinder_openplc_contract.json",
        "examples/openplc_trace_phase2/two_cylinder.sil.normalized.jsonl",
    );
    assert_eq!(
        aligned.verdict,
        IntentAlignmentVerdict::Aligned,
        "FR-16 canonical double-cylinder sequence should stay aligned: {aligned:#?}"
    );

    let single_recovery = compare_fixture(
        "tests/fixtures/intent_alignment/contracts/recovery_single_contract.json",
        "tests/fixtures/intent_alignment/evidence/recovery_single_aligned.jsonl",
    );
    assert_eq!(
        single_recovery.verdict,
        IntentAlignmentVerdict::Aligned,
        "FR-16 canonical single-actuator recovery should stay aligned"
    );

    let multi_recovery = compare_fixture(
        "tests/fixtures/intent_alignment/contracts/recovery_multi_contract.json",
        "tests/fixtures/intent_alignment/evidence/recovery_multi_aligned.jsonl",
    );
    assert_eq!(
        multi_recovery.verdict,
        IntentAlignmentVerdict::Aligned,
        "FR-16 canonical multi-actuator recovery should stay aligned"
    );

    let missing_required = compare_fixture(
        "tests/fixtures/intent_alignment/contracts/two_cylinder_openplc_contract.json",
        "tests/fixtures/intent_alignment/evidence/two_cylinder_missing_required_step.jsonl",
    );
    assert_eq!(
        missing_required.primary_mismatch.as_ref().map(|mismatch| mismatch.kind),
        Some(IntentMismatchKind::MissingRequiredStep),
        "FR-8 missing_required_step should stay stable"
    );

    let wrong_order = compare_fixture(
        "tests/fixtures/intent_alignment/contracts/two_cylinder_openplc_contract.json",
        "tests/fixtures/intent_alignment/evidence/two_cylinder_wrong_order.jsonl",
    );
    assert_eq!(
        wrong_order.primary_mismatch.as_ref().map(|mismatch| mismatch.kind),
        Some(IntentMismatchKind::WrongOrder),
        "FR-8 wrong_order should stay stable"
    );

    let duplicated = compare_fixture(
        "tests/fixtures/intent_alignment/contracts/two_cylinder_openplc_contract.json",
        "tests/fixtures/intent_alignment/evidence/two_cylinder_duplicated_required_step.jsonl",
    );
    assert_eq!(
        duplicated.primary_mismatch.as_ref().map(|mismatch| mismatch.kind),
        Some(IntentMismatchKind::DuplicatedRequiredStep),
        "FR-8 duplicated_required_step should stay stable"
    );

    let premature = compare_fixture(
        "tests/fixtures/intent_alignment/contracts/recovery_single_contract.json",
        "tests/fixtures/intent_alignment/evidence/recovery_single_premature_readiness.jsonl",
    );
    assert!(
        premature
            .mismatches
            .iter()
            .any(|mismatch| mismatch.kind == IntentMismatchKind::PrematureReadiness),
        "FR-9 premature_readiness should remain present even when higher-priority coverage errors coexist"
    );

    let postcondition = compare_fixture(
        "tests/fixtures/intent_alignment/contracts/two_cylinder_openplc_contract.json",
        "tests/fixtures/intent_alignment/evidence/two_cylinder_postcondition_not_met.jsonl",
    );
    assert_eq!(
        postcondition.primary_mismatch.as_ref().map(|mismatch| mismatch.kind),
        Some(IntentMismatchKind::PostconditionNotMet),
        "FR-9 postcondition_not_met should stay stable"
    );

    let drift = compare_fixture(
        "tests/fixtures/intent_alignment/contracts/two_cylinder_openplc_contract.json",
        "tests/fixtures/intent_alignment/evidence/two_cylinder_cross_cycle_drift.jsonl",
    );
    assert_eq!(
        drift.primary_mismatch.as_ref().map(|mismatch| mismatch.kind),
        Some(IntentMismatchKind::CrossCycleDrift),
        "FR-10 cross_cycle_drift should stay stable: {drift:#?}"
    );
}

#[test]
fn real_two_cylinder_openplc_trace_is_a_golden_path() {
    let report = compare_fixture(
        "tests/fixtures/intent_alignment/contracts/two_cylinder_openplc_contract.json",
        "examples/openplc_trace_phase2/two_cylinder.sil.normalized.jsonl",
    );

    assert_eq!(
        report.verdict,
        IntentAlignmentVerdict::Aligned,
        "real example + real evidence should stay aligned"
    );
    assert_eq!(report.contract_identity.contract_id, "intent-alignment-two-cylinder-openplc");
    assert_eq!(report.cycle_window.cycle_count, 1);
}
