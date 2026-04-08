use std::path::PathBuf;

use rust_plc::intent_alignment::{
    IntentContractLoadError, MilestoneEvidenceSource, parse_intent_contract_str,
    read_intent_contract, verify_intent_contract_source_binding,
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
    assert_eq!(contract.intent_sequence.len(), 4);
}

#[test]
fn contract_fixture_keeps_business_milestones_distinct_from_observed_markers() {
    let fixture =
        workspace_path("tests/fixtures/intent_alignment/contracts/cylinder_sequence_contract.json");
    let contract = read_intent_contract(&fixture).expect("fixture should load");
    let secured = &contract.intent_sequence[1];

    assert_eq!(secured.milestone_id, "grip_part_secured");
    assert_eq!(
        secured.business_milestone.label,
        "Part secured before transfer"
    );
    assert!(secured.observed_as.iter().all(|evidence| {
        evidence.source == MilestoneEvidenceSource::TraceEvent
            && evidence.expected.starts_with("cyl_")
    }));
    assert!(
        secured
            .observed_as
            .iter()
            .all(|evidence| evidence.expected != secured.business_milestone.label)
    );
}

#[test]
fn contract_fixture_source_digest_matches_architecture_source() {
    let fixture =
        workspace_path("tests/fixtures/intent_alignment/contracts/cylinder_sequence_contract.json");
    let contract = read_intent_contract(&fixture).expect("fixture should load");

    verify_intent_contract_source_binding(&contract, workspace_path("."))
        .expect("source digest should match authoritative source");
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
        "review_basis": []
      },
      "intent_sequence": [],
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
