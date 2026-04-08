use std::path::PathBuf;

use rust_plc::intent_alignment::{
    IntentContractLoadError, MilestoneEvidenceSource, ObservationCombination, ObservationSubject,
    parse_intent_contract_str, read_intent_contract, verify_intent_contract_source_binding,
};

fn workspace_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

#[test]
fn loads_phase2_contract_fixture_from_independent_json_file() {
    let fixture =
        workspace_path("tests/fixtures/intent_alignment/contracts/cylinder_sequence_contract.json");
    let contract = read_intent_contract(&fixture).expect("fixture should load");

    assert_eq!(contract.contract_version, "phase-2.v1");
    assert_eq!(
        contract.source_ref.path,
        "docs/architecture/intent_alignment_verification.md"
    );
    assert_eq!(contract.metadata.business_owner, "assembly-cell-owner");
    assert_eq!(contract.metadata.review_basis.len(), 2);

    assert_eq!(contract.contract_core.expected_milestones.len(), 4);
    assert_eq!(contract.contract_core.required_edges.len(), 3);
    assert_eq!(contract.contract_core.postconditions.len(), 1);
    assert_eq!(
        contract
            .contract_core
            .cycle_semantics
            .restart_semantics
            .restartable_milestone,
        "cycle_restartable"
    );
    assert_eq!(contract.observation_bindings.len(), 5);
}

#[test]
fn contract_fixture_models_business_core_separately_from_observation_bindings() {
    let fixture =
        workspace_path("tests/fixtures/intent_alignment/contracts/cylinder_sequence_contract.json");
    let contract = read_intent_contract(&fixture).expect("fixture should load");
    let secured = &contract.contract_core.expected_milestones[1];
    let secured_binding = contract
        .observation_binding_for_subject(&ObservationSubject::Milestone {
            milestone_id: secured.milestone_id.clone(),
        })
        .expect("milestone should have observation binding");

    assert_eq!(secured.milestone_id, "grip_part_secured");
    assert_eq!(
        secured.business_milestone.label,
        "Part secured before transfer"
    );
    assert_eq!(
        secured_binding.combination,
        ObservationCombination::OrderedAllOf
    );
    assert!(secured_binding.evidence.iter().all(|evidence| {
        evidence.source == MilestoneEvidenceSource::TraceEvent
            && evidence.expected.starts_with("cyl_")
    }));
    assert!(
        secured_binding
            .evidence
            .iter()
            .all(|evidence| evidence.expected != secured.business_milestone.label)
    );
}

#[test]
fn contract_fixture_explicitly_models_required_edges_postconditions_and_restart_semantics() {
    let fixture =
        workspace_path("tests/fixtures/intent_alignment/contracts/cylinder_sequence_contract.json");
    let contract = read_intent_contract(&fixture).expect("fixture should load");

    assert_eq!(
        contract.contract_core.required_edges[0].predecessor,
        "cycle_started"
    );
    assert_eq!(
        contract.contract_core.required_edges[0].successor,
        "grip_part_secured"
    );
    assert_eq!(
        contract.contract_core.postconditions[0].postcondition_id,
        "cell_ready_for_next_cycle"
    );
    assert_eq!(
        contract.contract_core.cycle_semantics.cycle_start_milestone,
        "cycle_started"
    );
    assert_eq!(
        contract
            .contract_core
            .cycle_semantics
            .restart_semantics
            .required_postconditions,
        vec!["cell_ready_for_next_cycle".to_string()]
    );

    let postcondition_binding = contract
        .observation_binding_for_subject(&ObservationSubject::Postcondition {
            postcondition_id: "cell_ready_for_next_cycle".to_string(),
        })
        .expect("postcondition should have observation binding");
    assert_eq!(
        postcondition_binding.combination,
        ObservationCombination::AllOf
    );
}

#[test]
fn contract_fixture_source_digest_matches_architecture_source() {
    let fixture =
        workspace_path("tests/fixtures/intent_alignment/contracts/cylinder_sequence_contract.json");
    let contract = read_intent_contract(&fixture).expect("fixture should load");

    verify_intent_contract_source_binding(&contract, workspace_path("."))
        .expect("source digest and governance bindings should match");
}

#[test]
fn strict_schema_rejects_unknown_fields() {
    let payload = r#"
    {
      "contract_version": "phase-2.v1",
      "source_ref": {
        "kind": "architecture_doc",
        "path": "docs/architecture/intent_alignment_verification.md",
        "description": "doc"
      },
      "source_digest": {
        "algorithm": "sha256",
        "value": "abc"
      },
      "metadata": {
        "contract_id": "contract",
        "title": "Intent",
        "business_owner": "owner",
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
        "expected_milestones": [],
        "required_edges": [],
        "postconditions": [],
        "cycle_semantics": {
          "cycle_start_milestone": "start",
          "cycle_complete_milestone": "done",
          "restart_semantics": {
            "restartable_milestone": "done",
            "next_cycle_start_milestone": "start",
            "required_postconditions": []
          }
        }
      },
      "observation_bindings": [],
      "unexpected_field": true
    }
    "#;

    let error = parse_intent_contract_str(payload).expect_err("unknown field should fail");
    match error {
        IntentContractLoadError::Json { source, .. } => {
            assert!(
                source
                    .to_string()
                    .contains("unknown field `unexpected_field`")
            );
        }
        other => panic!("expected JSON parse error, got {other:?}"),
    }
}
