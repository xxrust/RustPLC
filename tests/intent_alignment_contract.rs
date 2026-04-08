use std::path::PathBuf;

use rust_plc::intent_alignment::{
    BusinessMilestone, ExpectedBehaviorCompileError, ExpectedBehaviorIrPrimitiveKind,
    ExpectedMilestoneSemanticRole, IntentContractDiagnostic, IntentContractDiagnosticCode,
    IntentContractLoadError, IntentMilestone, MilestoneEvidenceSource, ObservationCombination,
    ObservationSubject, compile_expected_behavior_spec, parse_intent_contract_str,
    read_intent_contract, validate_intent_contract, verify_intent_contract_source_binding,
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

#[test]
fn semantic_validation_accepts_canonical_contract_fixture() {
    let fixture =
        workspace_path("tests/fixtures/intent_alignment/contracts/cylinder_sequence_contract.json");
    let contract = read_intent_contract(&fixture).expect("fixture should load");

    validate_intent_contract(&contract).expect("fixture should be semantically valid");
}

#[test]
fn semantic_validation_rejects_conflicting_required_edges_with_stable_diagnostic() {
    let fixture =
        workspace_path("tests/fixtures/intent_alignment/contracts/cylinder_sequence_contract.json");
    let mut contract = read_intent_contract(&fixture).expect("fixture should load");
    contract
        .contract_core
        .required_edges
        .push(rust_plc::intent_alignment::RequiredMilestoneEdge {
            predecessor: "grip_part_secured".to_string(),
            successor: "cycle_started".to_string(),
        });

    let error =
        validate_intent_contract(&contract).expect_err("conflicting required edges should fail");

    assert_eq!(
        error.diagnostics,
        vec![IntentContractDiagnostic {
            code: IntentContractDiagnosticCode::ConflictingRequiredEdges,
            subject: "grip_part_secured -> cycle_started".to_string(),
            detail: "required edges create a cycle, so milestone ordering is contradictory"
                .to_string(),
        }]
    );
    assert_eq!(error.diagnostics[0].stable_code(), "IAC-VAL-001");
}

#[test]
fn semantic_validation_rejects_unreachable_milestones_with_stable_diagnostic() {
    let fixture =
        workspace_path("tests/fixtures/intent_alignment/contracts/cylinder_sequence_contract.json");
    let mut contract = read_intent_contract(&fixture).expect("fixture should load");
    contract
        .contract_core
        .expected_milestones
        .push(IntentMilestone {
            milestone_id: "quality_checked".to_string(),
            business_milestone: BusinessMilestone {
                label: "Quality check completed".to_string(),
                description: "Detached milestone that should be rejected by validation."
                    .to_string(),
            },
        });

    let error = validate_intent_contract(&contract)
        .expect_err("unreachable milestones should fail semantic validation");

    assert_eq!(
        error.diagnostics,
        vec![IntentContractDiagnostic {
            code: IntentContractDiagnosticCode::UnreachableMilestone,
            subject: "quality_checked".to_string(),
            detail:
                "is not reachable from cycle_start_milestone `cycle_started` through required_edges"
                    .to_string(),
        }]
    );
    assert_eq!(error.diagnostics[0].stable_code(), "IAC-VAL-002");
}

#[test]
fn semantic_validation_rejects_contradictory_cycle_restart_constraints() {
    let fixture =
        workspace_path("tests/fixtures/intent_alignment/contracts/cylinder_sequence_contract.json");
    let mut contract = read_intent_contract(&fixture).expect("fixture should load");
    contract
        .contract_core
        .cycle_semantics
        .restart_semantics
        .next_cycle_start_milestone = "grip_part_secured".to_string();

    let error = validate_intent_contract(&contract)
        .expect_err("contradictory restart semantics should fail semantic validation");

    assert_eq!(
        error.diagnostics,
        vec![IntentContractDiagnostic {
            code: IntentContractDiagnosticCode::ContradictoryCycleSemantics,
            subject: "cycle_semantics.restart_semantics.next_cycle_start_milestone".to_string(),
            detail: "must match cycle_start_milestone `cycle_started`".to_string(),
        }]
    );
    assert_eq!(error.diagnostics[0].stable_code(), "IAC-VAL-003");
}

#[test]
fn compile_expected_behavior_spec_preserves_contract_core_and_observation_bindings() {
    let fixture =
        workspace_path("tests/fixtures/intent_alignment/contracts/cylinder_sequence_contract.json");
    let contract = read_intent_contract(&fixture).expect("fixture should load");

    let spec = compile_expected_behavior_spec(&contract).expect("valid contract should compile");

    assert_eq!(spec.contract_id, contract.metadata.contract_id);
    assert_eq!(spec.contract_version, contract.contract_version);
    assert_eq!(
        spec.expected_milestones,
        contract.contract_core.expected_milestones
    );
    assert_eq!(spec.required_edges, contract.contract_core.required_edges);
    assert_eq!(spec.postconditions, contract.contract_core.postconditions);
    assert_eq!(spec.observation_bindings, contract.observation_bindings);
    assert_eq!(
        spec.cycle_semantics.cycle_start_milestone,
        contract.contract_core.cycle_semantics.cycle_start_milestone
    );
    assert_eq!(
        spec.cycle_semantics.cycle_complete_milestone,
        contract
            .contract_core
            .cycle_semantics
            .cycle_complete_milestone
    );
    assert_eq!(
        spec.cycle_semantics.restartability.restartable_milestone,
        contract
            .contract_core
            .cycle_semantics
            .restart_semantics
            .restartable_milestone
    );
    assert_eq!(
        spec.cycle_semantics
            .restartability
            .next_cycle_start_milestone,
        contract
            .contract_core
            .cycle_semantics
            .restart_semantics
            .next_cycle_start_milestone
    );
    assert_eq!(
        spec.cycle_semantics.restartability.required_postconditions,
        contract
            .contract_core
            .cycle_semantics
            .restart_semantics
            .required_postconditions
    );
}

#[test]
fn compile_expected_behavior_spec_emits_stable_ir_semantic_view() {
    let fixture =
        workspace_path("tests/fixtures/intent_alignment/contracts/cylinder_sequence_contract.json");
    let contract = read_intent_contract(&fixture).expect("fixture should load");

    let spec = compile_expected_behavior_spec(&contract).expect("valid contract should compile");

    assert_eq!(
        spec.ir_view.milestone_graph.edges.len(),
        contract.contract_core.required_edges.len()
    );
    assert!(
        spec.ir_view.milestone_graph.edges.iter().all(|edge| {
            edge.primitive == ExpectedBehaviorIrPrimitiveKind::StateMachineOrdering
        })
    );
    assert_eq!(
        spec.ir_view.postcondition_obligations.len(),
        contract.contract_core.postconditions.len()
    );
    assert!(
        spec.ir_view
            .postcondition_obligations
            .iter()
            .all(|postcondition| {
                postcondition.primitive == ExpectedBehaviorIrPrimitiveKind::ConstraintPostcondition
            })
    );
    assert_eq!(
        spec.ir_view.cycle_handoff.cycle_boundary_primitive,
        ExpectedBehaviorIrPrimitiveKind::CycleBoundary
    );
    assert_eq!(
        spec.ir_view.cycle_handoff.restartability_primitive,
        ExpectedBehaviorIrPrimitiveKind::Restartability
    );

    let start_node = spec
        .ir_view
        .milestone_graph
        .nodes
        .iter()
        .find(|node| node.milestone_id == "cycle_started")
        .expect("cycle start milestone should appear in IR view");
    assert!(
        start_node
            .semantic_roles
            .contains(&ExpectedMilestoneSemanticRole::CycleStart)
    );
    assert!(
        start_node
            .semantic_roles
            .contains(&ExpectedMilestoneSemanticRole::RequiredStep)
    );

    let restartable_node = spec
        .ir_view
        .milestone_graph
        .nodes
        .iter()
        .find(|node| node.milestone_id == "cycle_restartable")
        .expect("restartable milestone should appear in IR view");
    assert!(
        restartable_node
            .semantic_roles
            .contains(&ExpectedMilestoneSemanticRole::CycleComplete)
    );
    assert!(
        restartable_node
            .semantic_roles
            .contains(&ExpectedMilestoneSemanticRole::Restartable)
    );
}

#[test]
fn compile_expected_behavior_spec_reuses_contract_validation_failures() {
    let fixture =
        workspace_path("tests/fixtures/intent_alignment/contracts/cylinder_sequence_contract.json");
    let mut contract = read_intent_contract(&fixture).expect("fixture should load");
    contract
        .contract_core
        .cycle_semantics
        .restart_semantics
        .next_cycle_start_milestone = "grip_part_secured".to_string();

    let error = compile_expected_behavior_spec(&contract)
        .expect_err("invalid contract should fail before IR-view compilation");

    match error {
        ExpectedBehaviorCompileError::InvalidContract(validation_error) => {
            assert_eq!(
                validation_error.diagnostics,
                vec![IntentContractDiagnostic {
                    code: IntentContractDiagnosticCode::ContradictoryCycleSemantics,
                    subject: "cycle_semantics.restart_semantics.next_cycle_start_milestone"
                        .to_string(),
                    detail: "must match cycle_start_milestone `cycle_started`".to_string(),
                }]
            );
        }
    }
}
