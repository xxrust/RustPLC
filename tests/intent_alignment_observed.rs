use std::path::PathBuf;

use rust_plc::intent_alignment::{
    ObservationCombination, ObservationSubject, RawObservedEvent, compile_expected_behavior_spec,
    extract_observed_behavior_sequence, parse_observed_trace_jsonl, read_intent_contract,
};

fn workspace_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn openplc_ready_spec() -> rust_plc::intent_alignment::ExpectedBehaviorSpec {
    let fixture =
        workspace_path("tests/fixtures/intent_alignment/contracts/cylinder_sequence_contract.json");
    let mut contract = read_intent_contract(&fixture).expect("fixture should load");

    for binding in &mut contract.observation_bindings {
        match &binding.subject {
            ObservationSubject::Milestone { milestone_id } if milestone_id == "cycle_started" => {
                binding.combination = ObservationCombination::AllOf;
                binding.evidence = vec![
                    observed("vars._state", "0"),
                    observed("vars.valve_a", "false"),
                    observed("vars.valve_b", "false"),
                ];
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
                binding.evidence = vec![
                    observed("vars._state", "40"),
                    observed("vars.valve_a", "false"),
                    observed("vars.valve_b", "false"),
                ];
            }
            ObservationSubject::Postcondition { postcondition_id }
                if postcondition_id == "cell_ready_for_next_cycle" =>
            {
                binding.combination = ObservationCombination::AllOf;
                binding.evidence = vec![
                    observed("vars._state", "40"),
                    observed("vars.valve_a", "false"),
                    observed("vars.valve_b", "false"),
                ];
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

fn trace_cycle_spec() -> rust_plc::intent_alignment::ExpectedBehaviorSpec {
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
    "contract_id": "trace-cycle-boundary",
    "title": "Trace cycle boundary",
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
        "milestone_id": "cycle_restartable",
        "business_milestone": { "label": "Cycle restartable", "description": "done" }
      }
    ],
    "required_edges": [
      { "predecessor": "cycle_started", "successor": "cycle_restartable" }
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
      "binding_id": "done",
      "subject": { "kind": "milestone", "milestone_id": "cycle_restartable" },
      "combination": "all_of",
      "evidence": [
        { "source": "trace_event", "key": "transition", "expected": "task=0;from=1;to=2;reason=action" }
      ]
    }
  ]
}
"#;

    let contract =
        rust_plc::intent_alignment::parse_intent_contract_str(json).expect("contract should parse");
    compile_expected_behavior_spec(&contract).expect("contract should compile")
}

#[test]
fn parses_openplc_variable_snapshot_trace_jsonl() {
    let raw = std::fs::read_to_string(workspace_path(
        "examples/openplc_trace_phase2/two_cylinder.sil.normalized.jsonl",
    ))
    .expect("read openplc normalized trace");

    let events = parse_observed_trace_jsonl(&raw).expect("trace should parse");

    assert_eq!(events.len(), 5);
    assert!(matches!(
        &events[0],
        RawObservedEvent::VariableSnapshot { tick: 0, .. }
    ));
}

#[test]
fn extract_observed_behavior_sequence_normalizes_repeated_snapshots_and_tracks_cycle_indices() {
    let spec = openplc_ready_spec();
    let raw = r#"
{"tick":0,"vars":{"_state":0,"valve_a":false,"valve_b":false}}
{"tick":1,"vars":{"_state":10,"valve_a":true,"valve_b":false}}
{"tick":2,"vars":{"_state":10,"valve_a":true,"valve_b":false}}
{"tick":3,"vars":{"_state":20,"valve_a":true,"valve_b":true}}
{"tick":4,"vars":{"_state":40,"valve_a":false,"valve_b":false}}
{"tick":5,"vars":{"_state":0,"valve_a":false,"valve_b":false}}
{"tick":6,"vars":{"_state":10,"valve_a":true,"valve_b":false}}
"#;

    let events = parse_observed_trace_jsonl(raw).expect("trace should parse");
    let sequence = extract_observed_behavior_sequence(&spec, &events)
        .expect("cycle-start evidence should be detected");

    assert_eq!(sequence.cycle_count, 2);
    assert_eq!(sequence.cycles.len(), 2);
    assert!(sequence.evidence.iter().all(|entry| entry.tick != 2));
    assert!(
        sequence
            .evidence
            .iter()
            .any(|entry| entry.tick == 5 && entry.cycle_index == 1)
    );
    assert_eq!(sequence.cycles[0].successful_cycle_end_tick, Some(4));
    assert_eq!(sequence.cycles[1].start_tick, 5);
    assert!(
        sequence
            .readiness
            .iter()
            .find(|readiness| {
                readiness.dimension
                    == rust_plc::intent_alignment::ObservedComparisonDimension::CrossCycle
            })
            .expect("cross-cycle readiness should exist")
            .ready
    );
}

#[test]
fn extractor_does_not_split_repeated_start_like_snapshot_before_cycle_end() {
    let spec = openplc_ready_spec();
    let raw = r#"
{"tick":0,"vars":{"_state":0,"valve_a":false,"valve_b":false}}
{"tick":1,"vars":{"_state":20,"valve_a":true,"valve_b":true}}
{"tick":2,"vars":{"_state":0,"valve_a":false,"valve_b":false}}
{"tick":3,"vars":{"_state":30,"valve_a":false,"valve_b":true}}
{"tick":4,"vars":{"_state":40,"valve_a":false,"valve_b":false}}
"#;

    let events = parse_observed_trace_jsonl(raw).expect("trace should parse");
    let sequence = extract_observed_behavior_sequence(&spec, &events)
        .expect("cycle-start evidence should be detected");

    assert_eq!(sequence.cycle_count, 1);
    assert!(
        sequence
            .evidence
            .iter()
            .any(|entry| entry.tick == 2 && entry.cycle_index == 0)
    );
    assert_eq!(sequence.cycles.len(), 1);
    assert_eq!(sequence.cycles[0].cycle_index, 0);
    assert_eq!(sequence.cycles[0].start_tick, 0);
    assert_eq!(sequence.cycles[0].successful_cycle_end_tick, Some(4));
}

#[test]
fn parse_observed_trace_jsonl_rejects_unknown_row_shape() {
    let raw = r#"{"tick":0,"milestone":"start_cycle"}"#;

    let error = parse_observed_trace_jsonl(raw).expect_err("unknown row should fail");

    match error {
        rust_plc::intent_alignment::ObservedTraceParseError::UnsupportedRow { line, detail } => {
            assert_eq!(line, 1);
            assert!(detail.contains("did not match any variant"));
        }
        other => panic!("expected unsupported-row error, got {other:?}"),
    }
}

#[test]
fn extractor_does_not_create_trailing_partial_cycle_from_post_complete_trace_noise() {
    let spec = trace_cycle_spec();
    let raw = r#"
{"tick":0,"task":0,"from_step":0,"to_step":1,"reason":"action"}
{"tick":1,"task":0,"from_step":1,"to_step":2,"reason":"action"}
{"tick":2,"task":9,"from_step":99,"to_step":100,"reason":"action"}
"#;

    let events = parse_observed_trace_jsonl(raw).expect("trace should parse");
    let sequence = extract_observed_behavior_sequence(&spec, &events)
        .expect("cycle-start evidence should be detected");

    assert_eq!(sequence.cycle_count, 1);
    assert_eq!(sequence.cycles.len(), 1);
    assert_eq!(sequence.cycles[0].successful_cycle_end_tick, Some(1));
}
