use std::path::PathBuf;

use rust_plc::intent_alignment::{
    INTENT_ALIGNMENT_COMPARATOR_VERSION, IntentAlignmentEvidenceKind,
    IntentAlignmentVerdict, IntentMismatchKind, ObservationCombination, ObservationSubject,
    compare_intent_alignment, compare_trace_jsonl, compile_expected_behavior_spec,
    extract_observed_behavior_sequence, parse_observed_trace_jsonl, read_intent_contract,
    reduce_intent_alignment_report,
};

fn workspace_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn compare_ready_spec() -> rust_plc::intent_alignment::ExpectedBehaviorSpec {
    let fixture =
        workspace_path("tests/fixtures/intent_alignment/contracts/cylinder_sequence_contract.json");
    let mut contract = read_intent_contract(&fixture).expect("fixture should load");

    for binding in &mut contract.observation_bindings {
        match &binding.subject {
            ObservationSubject::Milestone { milestone_id } if milestone_id == "cycle_started" => {
                binding.combination = ObservationCombination::AllOf;
                binding.evidence = vec![observed("vars._state", "0")];
            }
            ObservationSubject::Milestone { milestone_id }
                if milestone_id == "grip_part_secured" =>
            {
                binding.combination = ObservationCombination::AllOf;
                binding.evidence = vec![observed("vars._state", "20")];
            }
            ObservationSubject::Milestone { milestone_id }
                if milestone_id == "transfer_lane_cleared" =>
            {
                binding.combination = ObservationCombination::AllOf;
                binding.evidence = vec![observed("vars._state", "30")];
            }
            ObservationSubject::Milestone { milestone_id }
                if milestone_id == "cycle_restartable" =>
            {
                binding.combination = ObservationCombination::AllOf;
                binding.evidence = vec![observed("vars._state", "40")];
            }
            ObservationSubject::Postcondition { postcondition_id }
                if postcondition_id == "cell_ready_for_next_cycle" =>
            {
                binding.combination = ObservationCombination::AllOf;
                binding.evidence = vec![observed("vars._state", "40")];
            }
            _ => {}
        }
    }

    compile_expected_behavior_spec(&contract).expect("contract should compile")
}

fn observed(key: &str, expected: &str) -> rust_plc::intent_alignment::ObservedEvidence {
    rust_plc::intent_alignment::ObservedEvidence {
        source: rust_plc::intent_alignment::MilestoneEvidenceSource::VariableState,
        key: key.to_string(),
        expected: expected.to_string(),
    }
}

#[test]
fn report_carries_stable_provenance_fields() {
    let spec = compare_ready_spec();
    let raw = r#"
{"tick":0,"vars":{"_state":0}}
{"tick":1,"vars":{"_state":20}}
{"tick":2,"vars":{"_state":30}}
{"tick":3,"vars":{"_state":40}}
{"tick":4,"vars":{"_state":0}}
{"tick":5,"vars":{"_state":20}}
{"tick":6,"vars":{"_state":30}}
{"tick":7,"vars":{"_state":40}}
"#;

    let report = compare_trace_jsonl(&spec, raw).expect("trace should compare");

    assert_eq!(report.verdict, IntentAlignmentVerdict::Aligned);
    assert_eq!(report.contract_identity.contract_id, spec.contract_id);
    assert_eq!(
        report.contract_identity.contract_version,
        spec.contract_version
    );
    assert_eq!(
        report.evidence_identity.kind,
        IntentAlignmentEvidenceKind::InlineTraceJsonl
    );
    assert_eq!(report.evidence_identity.label, "inline_trace_jsonl");
    assert_eq!(
        report.comparator_version,
        INTENT_ALIGNMENT_COMPARATOR_VERSION
    );
    assert_eq!(report.cycle_window.first_cycle_index, 0);
    assert_eq!(report.cycle_window.last_cycle_index, 1);
    assert_eq!(report.cycle_window.cycle_count, 2);
}

#[test]
fn pipeline_reducer_is_deterministic_and_preserves_mismatch_severity() {
    let spec = compare_ready_spec();
    let raw = r#"
{"tick":0,"vars":{"_state":0}}
{"tick":1,"vars":{"_state":30}}
{"tick":2,"vars":{"_state":20}}
{"tick":3,"vars":{"_state":40}}
"#;

    let report = compare_trace_jsonl(&spec, raw).expect("trace should compare");
    let first = reduce_intent_alignment_report(&report);
    let second = reduce_intent_alignment_report(&report);

    assert_eq!(first, second);
    assert_eq!(first.verdict, IntentAlignmentVerdict::Mismatch);
    assert_eq!(first.primary_mismatch_kind, Some(IntentMismatchKind::WrongOrder));
    assert_eq!(first.mismatch_count, report.mismatches.len());
    assert_eq!(first.blocker_kind, None);
}

#[test]
fn pipeline_reducer_keeps_blocked_verdict_and_matches_all_entry_points() {
    let spec = compare_ready_spec();
    let raw = r#"
{"tick":0,"vars":{"_state":0}}
"#;

    let direct = compare_trace_jsonl(&spec, raw).expect("trace should compare");
    let events = parse_observed_trace_jsonl(raw).expect("trace should parse");
    let observed = extract_observed_behavior_sequence(&spec, &events)
        .expect("trace should extract observed behavior");
    let via_sequence = compare_intent_alignment(&spec, &observed);

    let direct_summary = reduce_intent_alignment_report(&direct);
    let sequence_summary = reduce_intent_alignment_report(&via_sequence);

    assert_eq!(direct.verdict, IntentAlignmentVerdict::Blocked);
    assert_eq!(direct_summary.verdict, IntentAlignmentVerdict::Blocked);
    assert_eq!(direct_summary.blocker_kind, direct.blocker_kind);
    assert_eq!(direct_summary.verdict, sequence_summary.verdict);
    assert_eq!(
        direct_summary.primary_mismatch_kind,
        sequence_summary.primary_mismatch_kind
    );
    assert_eq!(direct_summary.blocker_kind, sequence_summary.blocker_kind);
    assert_eq!(
        direct_summary.comparator_version,
        sequence_summary.comparator_version
    );
}
