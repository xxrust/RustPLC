use std::path::PathBuf;

use rust_plc::intent_alignment::{
    IntentAlignmentVerdict, IntentMismatchKind, ObservationCombination, ObservationSubject,
    compare_intent_alignment, compare_trace_jsonl, compile_expected_behavior_spec,
    extract_observed_behavior_sequence, parse_observed_trace_jsonl, read_intent_contract,
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

fn compare_postcondition_spec() -> rust_plc::intent_alignment::ExpectedBehaviorSpec {
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
                binding.evidence = vec![observed("vars.ready", "true")];
            }
            _ => {}
        }
    }

    compile_expected_behavior_spec(&contract).expect("contract should compile")
}

fn recovery_ready_spec() -> rust_plc::intent_alignment::ExpectedBehaviorSpec {
    let json = r#"
{
  "contract_version": "phase-2.v1",
  "source_ref": {
    "kind": "architecture_doc",
    "path": "docs/architecture/intent_alignment_verification.md",
    "description": "doc"
  },
  "source_digest": {
    "algorithm": "sha256",
    "value": "c1b32a71b9e47142e5b9ed142384e6f68568f635e71bdee7d35e661b7cb3d61e"
  },
  "metadata": {
    "contract_id": "recovery-sequence",
    "title": "Recovery sequence",
    "business_owner": "assembly-cell-owner",
    "authoritative_intent_source": {
      "kind": "architecture_doc",
      "path": "docs/architecture/intent_alignment_verification.md",
      "description": "doc"
    },
    "review_basis": [
      {
        "label": "Architecture review",
        "source": {
          "kind": "architecture_doc",
          "path": "docs/architecture/intent_alignment_verification.md",
          "description": "doc"
        }
      }
    ]
  },
  "contract_core": {
    "expected_milestones": [
      {
        "milestone_id": "fault_detected",
        "business_milestone": { "label": "Fault detected", "description": "fault" }
      },
      {
        "milestone_id": "safe_home_restored",
        "business_milestone": { "label": "Safe home restored", "description": "home" }
      },
      {
        "milestone_id": "cycle_restartable",
        "business_milestone": { "label": "Cycle restartable", "description": "restartable" }
      }
    ],
    "required_edges": [
      { "predecessor": "fault_detected", "successor": "safe_home_restored" },
      { "predecessor": "safe_home_restored", "successor": "cycle_restartable" }
    ],
    "postconditions": [
      { "postcondition_id": "ready_for_restart", "description": "ready is true" }
    ],
    "cycle_semantics": {
      "cycle_start_milestone": "fault_detected",
      "cycle_complete_milestone": "cycle_restartable",
      "restart_semantics": {
        "restartable_milestone": "cycle_restartable",
        "next_cycle_start_milestone": "fault_detected",
        "required_postconditions": ["ready_for_restart"]
      }
    }
  },
  "observation_bindings": [
    {
      "binding_id": "fault",
      "subject": { "kind": "milestone", "milestone_id": "fault_detected" },
      "combination": "all_of",
      "evidence": [{ "source": "variable_state", "key": "vars._state", "expected": "0" }]
    },
    {
      "binding_id": "home",
      "subject": { "kind": "milestone", "milestone_id": "safe_home_restored" },
      "combination": "all_of",
      "evidence": [{ "source": "variable_state", "key": "vars._state", "expected": "20" }]
    },
    {
      "binding_id": "restartable",
      "subject": { "kind": "milestone", "milestone_id": "cycle_restartable" },
      "combination": "all_of",
      "evidence": [{ "source": "variable_state", "key": "vars._state", "expected": "40" }]
    },
    {
      "binding_id": "ready",
      "subject": { "kind": "postcondition", "postcondition_id": "ready_for_restart" },
      "combination": "all_of",
      "evidence": [{ "source": "variable_state", "key": "vars.ready", "expected": "true" }]
    }
  ]
}
"#;

    let contract = rust_plc::intent_alignment::parse_intent_contract_str(json)
        .expect("recovery contract should parse");
    compile_expected_behavior_spec(&contract).expect("recovery contract should compile")
}

fn trace_anchor_spec() -> rust_plc::intent_alignment::ExpectedBehaviorSpec {
    let json = r#"
{
  "contract_version": "phase-2.v1",
  "source_ref": {
    "kind": "architecture_doc",
    "path": "docs/architecture/intent_alignment_verification.md",
    "description": "doc"
  },
  "source_digest": {
    "algorithm": "sha256",
    "value": "c1b32a71b9e47142e5b9ed142384e6f68568f635e71bdee7d35e661b7cb3d61e"
  },
  "metadata": {
    "contract_id": "trace-anchor-sequence",
    "title": "Trace anchor sequence",
    "business_owner": "assembly-cell-owner",
    "authoritative_intent_source": {
      "kind": "architecture_doc",
      "path": "docs/architecture/intent_alignment_verification.md",
      "description": "doc"
    },
    "review_basis": [
      {
        "label": "Architecture review",
        "source": {
          "kind": "architecture_doc",
          "path": "docs/architecture/intent_alignment_verification.md",
          "description": "doc"
        }
      }
    ]
  },
  "contract_core": {
    "expected_milestones": [
      {
        "milestone_id": "cycle_started",
        "business_milestone": { "label": "Cycle started", "description": "start" }
      },
      {
        "milestone_id": "wafer_handed_off",
        "business_milestone": { "label": "Wafer handed off", "description": "handoff" }
      }
    ],
    "required_edges": [
      { "predecessor": "cycle_started", "successor": "wafer_handed_off" }
    ],
    "postconditions": [],
    "cycle_semantics": {
      "cycle_start_milestone": "cycle_started",
      "cycle_complete_milestone": "wafer_handed_off",
      "restart_semantics": {
        "restartable_milestone": "wafer_handed_off",
        "next_cycle_start_milestone": "cycle_started",
        "required_postconditions": []
      }
    }
  },
  "observation_bindings": [
    {
      "binding_id": "start",
      "subject": { "kind": "milestone", "milestone_id": "cycle_started" },
      "combination": "all_of",
      "evidence": [
        { "source": "trace_event", "key": "transition", "expected": "task=2;from=6;to=7;reason=action" }
      ]
    },
    {
      "binding_id": "handoff",
      "subject": { "kind": "milestone", "milestone_id": "wafer_handed_off" },
      "combination": "all_of",
      "evidence": [
        { "source": "trace_event", "key": "transition", "expected": "task=4;from=26;to=27;reason=action" }
      ]
    }
  ]
}
"#;

    let contract =
        rust_plc::intent_alignment::parse_intent_contract_str(json).expect("contract should parse");
    compile_expected_behavior_spec(&contract).expect("contract should compile")
}

fn overlapping_trace_anchor_spec() -> rust_plc::intent_alignment::ExpectedBehaviorSpec {
    let json = r#"
{
  "contract_version": "phase-2.v1",
  "source_ref": {
    "kind": "architecture_doc",
    "path": "docs/architecture/intent_alignment_verification.md",
    "description": "doc"
  },
  "source_digest": {
    "algorithm": "sha256",
    "value": "c1b32a71b9e47142e5b9ed142384e6f68568f635e71bdee7d35e661b7cb3d61e"
  },
  "metadata": {
    "contract_id": "overlapping-trace-anchor-sequence",
    "title": "Overlapping trace anchor sequence",
    "business_owner": "assembly-cell-owner",
    "authoritative_intent_source": {
      "kind": "architecture_doc",
      "path": "docs/architecture/intent_alignment_verification.md",
      "description": "doc"
    },
    "review_basis": [
      {
        "label": "Architecture review",
        "source": {
          "kind": "architecture_doc",
          "path": "docs/architecture/intent_alignment_verification.md",
          "description": "doc"
        }
      }
    ]
  },
  "contract_core": {
    "expected_milestones": [
      {
        "milestone_id": "cycle_started",
        "business_milestone": { "label": "Cycle started", "description": "start" }
      },
      {
        "milestone_id": "midpoint",
        "business_milestone": { "label": "Midpoint", "description": "mid" }
      },
      {
        "milestone_id": "cycle_restartable",
        "business_milestone": { "label": "Cycle restartable", "description": "done" }
      }
    ],
    "required_edges": [
      { "predecessor": "cycle_started", "successor": "midpoint" },
      { "predecessor": "midpoint", "successor": "cycle_restartable" }
    ],
    "postconditions": [],
    "cycle_semantics": {
      "cycle_start_milestone": "cycle_started",
      "cycle_complete_milestone": "cycle_restartable",
      "restart_semantics": {
        "restartable_milestone": "cycle_restartable",
        "next_cycle_start_milestone": "cycle_started",
        "required_postconditions": []
      }
    }
  },
  "observation_bindings": [
    {
      "binding_id": "start",
      "subject": { "kind": "milestone", "milestone_id": "cycle_started" },
      "combination": "all_of",
      "evidence": [
        { "source": "trace_event", "key": "transition", "expected": "task=0;from=0;to=1;reason=action" }
      ]
    },
    {
      "binding_id": "mid",
      "subject": { "kind": "milestone", "milestone_id": "midpoint" },
      "combination": "all_of",
      "evidence": [
        { "source": "trace_event", "key": "transition", "expected": "task=0;from=1;to=2;reason=action" }
      ]
    },
    {
      "binding_id": "restartable",
      "subject": { "kind": "milestone", "milestone_id": "cycle_restartable" },
      "combination": "all_of",
      "evidence": [
        { "source": "trace_event", "key": "transition", "expected": "task=0;from=2;to=3;reason=action" }
      ]
    }
  ]
}
"#;

    let contract =
        rust_plc::intent_alignment::parse_intent_contract_str(json).expect("contract should parse");
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
fn compare_accepts_legal_reentry_across_two_cycles() {
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
    assert!(report.mismatches.is_empty());
}

#[test]
fn compare_accepts_overlapping_transition_anchored_cycles_by_occurrence_order() {
    let spec = overlapping_trace_anchor_spec();
    let raw = r#"
{"tick":0,"task":0,"from_step":0,"to_step":1,"reason":"action"}
{"tick":1,"task":0,"from_step":1,"to_step":2,"reason":"action"}
{"tick":2,"task":0,"from_step":0,"to_step":1,"reason":"action"}
{"tick":3,"task":0,"from_step":2,"to_step":3,"reason":"action"}
{"tick":4,"task":0,"from_step":1,"to_step":2,"reason":"action"}
{"tick":5,"task":0,"from_step":2,"to_step":3,"reason":"action"}
"#;

    let report = compare_trace_jsonl(&spec, raw).expect("trace should compare");

    assert_eq!(report.verdict, IntentAlignmentVerdict::Aligned);
    assert!(report.mismatches.is_empty());
    assert_eq!(report.cycle_window.cycle_count, 2);
}

#[test]
fn compare_rejects_unexpected_detour_without_silence() {
    let spec = compare_ready_spec();
    let raw = r#"
{"tick":0,"vars":{"_state":0}}
{"tick":1,"vars":{"_state":20}}
{"tick":2,"vars":{"_state":15}}
{"tick":3,"vars":{"_state":30}}
{"tick":4,"vars":{"_state":40}}
"#;

    let report = compare_trace_jsonl(&spec, raw).expect("trace should compare");

    assert_eq!(report.verdict, IntentAlignmentVerdict::Mismatch);
    assert_eq!(
        report
            .primary_mismatch
            .as_ref()
            .map(|mismatch| mismatch.kind),
        Some(IntentMismatchKind::UnexpectedObservedStep)
    );
}

#[test]
fn compare_emits_stable_primary_diagnosis_across_entry_points() {
    let spec = compare_ready_spec();
    let raw = r#"
{"tick":0,"vars":{"_state":0}}
{"tick":1,"vars":{"_state":30}}
{"tick":2,"vars":{"_state":20}}
{"tick":3,"vars":{"_state":40}}
"#;

    let direct = compare_trace_jsonl(&spec, raw).expect("trace should compare");

    let events = parse_observed_trace_jsonl(raw).expect("trace should parse");
    let observed = extract_observed_behavior_sequence(&spec, &events)
        .expect("trace should extract observed behavior");
    let via_sequence = compare_intent_alignment(&spec, &observed);

    assert_eq!(direct.verdict, IntentAlignmentVerdict::Mismatch);
    assert_eq!(
        direct
            .primary_mismatch
            .as_ref()
            .map(|mismatch| mismatch.kind),
        Some(IntentMismatchKind::WrongOrder)
    );
    assert_eq!(
        direct
            .primary_mismatch
            .as_ref()
            .map(|mismatch| mismatch.kind),
        via_sequence
            .primary_mismatch
            .as_ref()
            .map(|mismatch| mismatch.kind)
    );
}

#[test]
fn compare_reports_postcondition_not_met_when_terminal_snapshot_lacks_required_fact() {
    let spec = compare_postcondition_spec();
    let raw = r#"
{"tick":0,"vars":{"_state":0}}
{"tick":1,"vars":{"_state":20}}
{"tick":2,"vars":{"_state":30}}
{"tick":3,"vars":{"_state":40,"ready":false}}
"#;

    let report = compare_trace_jsonl(&spec, raw).expect("trace should compare");

    assert_eq!(report.verdict, IntentAlignmentVerdict::Mismatch);
    assert!(
        report
            .mismatches
            .iter()
            .any(|mismatch| mismatch.kind == IntentMismatchKind::PostconditionNotMet)
    );
}

#[test]
fn compare_accepts_postcondition_when_required_fact_is_present() {
    let spec = compare_postcondition_spec();
    let raw = r#"
{"tick":0,"vars":{"_state":0}}
{"tick":1,"vars":{"_state":20}}
{"tick":2,"vars":{"_state":30}}
{"tick":3,"vars":{"_state":40,"ready":true}}
"#;

    let report = compare_trace_jsonl(&spec, raw).expect("trace should compare");

    assert_eq!(report.verdict, IntentAlignmentVerdict::Aligned);
}

#[test]
fn compare_reports_premature_readiness_when_restartable_arrives_before_recovery_history_closes() {
    let spec = recovery_ready_spec();
    let raw = r#"
{"tick":0,"vars":{"_state":0}}
{"tick":1,"vars":{"_state":40,"ready":true}}
"#;

    let report = compare_trace_jsonl(&spec, raw).expect("trace should compare");

    assert_eq!(report.verdict, IntentAlignmentVerdict::Mismatch);
    assert!(
        report
            .mismatches
            .iter()
            .any(|mismatch| mismatch.kind == IntentMismatchKind::PrematureReadiness)
    );
}

#[test]
fn compare_reports_cross_cycle_drift_when_next_cycle_starts_without_handoff_gap() {
    let spec = compare_ready_spec();
    let raw = r#"
{"tick":0,"vars":{"_state":0}}
{"tick":1,"vars":{"_state":20}}
{"tick":2,"vars":{"_state":30}}
{"tick":3,"vars":{"_state":40}}
{"tick":3,"vars":{"_state":0}}
{"tick":4,"vars":{"_state":20}}
{"tick":5,"vars":{"_state":30}}
{"tick":6,"vars":{"_state":40}}
"#;

    let report = compare_trace_jsonl(&spec, raw).expect("trace should compare");

    assert_eq!(report.verdict, IntentAlignmentVerdict::Mismatch);
    assert_eq!(
        report
            .primary_mismatch
            .as_ref()
            .map(|mismatch| mismatch.kind),
        Some(IntentMismatchKind::CrossCycleDrift)
    );
    assert!(report.mismatches.iter().any(|mismatch| {
        mismatch.kind == IntentMismatchKind::CrossCycleDrift
            && mismatch.detail.contains("before handoff window advanced")
    }));
}

#[test]
fn compare_allows_sparse_trace_anchor_contracts_with_background_transitions() {
    let spec = trace_anchor_spec();
    let raw = r#"
{"tick":0,"task":0,"from_step":0,"to_step":1,"reason":"action"}
{"tick":0,"task":2,"from_step":6,"to_step":7,"reason":"action"}
{"tick":1,"task":1,"from_step":0,"to_step":1,"reason":"action"}
{"tick":2,"task":4,"from_step":26,"to_step":27,"reason":"action"}
{"tick":3,"task":9,"from_step":99,"to_step":100,"reason":"goto"}
"#;

    let report = compare_trace_jsonl(&spec, raw).expect("trace should compare");

    assert_eq!(report.verdict, IntentAlignmentVerdict::Aligned);
    assert!(report.mismatches.is_empty(), "{report:#?}");
}
