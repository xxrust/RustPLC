use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractSourceKind {
    ArchitectureDoc,
    CanonicalExample,
    AuthoredAsset,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MilestoneEvidenceSource {
    TraceEvent,
    DeviceState,
    VariableState,
    RuntimeTaskState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationCombination {
    AllOf,
    AnyOf,
    OrderedAllOf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractSourceRef {
    pub kind: ContractSourceKind,
    pub path: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractSourceDigest {
    pub algorithm: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractReviewInput {
    pub label: String,
    pub source: ContractSourceRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractMetadata {
    pub contract_id: String,
    pub title: String,
    pub business_owner: String,
    pub authoritative_intent_source: ContractSourceRef,
    pub review_basis: Vec<ContractReviewInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusinessMilestone {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentMilestone {
    pub milestone_id: String,
    pub business_milestone: BusinessMilestone,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequiredMilestoneEdge {
    pub predecessor: String,
    pub successor: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentPostcondition {
    pub postcondition_id: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestartSemantics {
    pub restartable_milestone: String,
    pub next_cycle_start_milestone: String,
    pub required_postconditions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractCycleSemantics {
    pub cycle_start_milestone: String,
    pub cycle_complete_milestone: String,
    pub restart_semantics: RestartSemantics,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentContractCore {
    pub expected_milestones: Vec<IntentMilestone>,
    pub required_edges: Vec<RequiredMilestoneEdge>,
    pub postconditions: Vec<IntentPostcondition>,
    pub cycle_semantics: ContractCycleSemantics,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ObservationSubject {
    Milestone { milestone_id: String },
    Postcondition { postcondition_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedEvidence {
    pub source: MilestoneEvidenceSource,
    pub key: String,
    pub expected: String,
}

pub type ObservedMilestoneEvidence = ObservedEvidence;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationBinding {
    pub binding_id: String,
    pub subject: ObservationSubject,
    pub combination: ObservationCombination,
    pub evidence: Vec<ObservedEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentContract {
    pub contract_version: String,
    pub source_ref: ContractSourceRef,
    pub source_digest: ContractSourceDigest,
    pub metadata: ContractMetadata,
    pub contract_core: IntentContractCore,
    pub observation_bindings: Vec<ObservationBinding>,
}

impl IntentContract {
    pub fn observation_binding_for_subject(
        &self,
        subject: &ObservationSubject,
    ) -> Option<&ObservationBinding> {
        self.observation_bindings
            .iter()
            .find(|binding| &binding.subject == subject)
    }
}

#[derive(Debug, Error)]
pub enum IntentContractLoadError {
    #[error("failed to read intent contract from {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "unsupported intent contract format for {path}; phase-2 v1 only supports .json fixtures"
    )]
    UnsupportedFormat { path: String },
    #[error("failed to parse intent contract JSON from {path}: {source}")]
    Json {
        path: String,
        #[source]
        source: serde_json::Error,
    },
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IntentContractSourceBindingError {
    #[error("unsupported source digest algorithm `{algorithm}`; phase-2 v1 expects sha256")]
    UnsupportedAlgorithm { algorithm: String },
    #[error("contract source `{path}` does not exist")]
    MissingSource { path: String },
    #[error("source digest mismatch for `{path}`: expected {expected}, actual {actual}")]
    DigestMismatch {
        path: String,
        expected: String,
        actual: String,
    },
    #[error(
        "authoritative intent source must match top-level source_ref: contract declares `{contract_path}` but metadata declares `{authoritative_path}`"
    )]
    AuthoritativeSourceConflict {
        contract_path: String,
        authoritative_path: String,
    },
    #[error("intent contract metadata.review_basis must not be empty")]
    MissingReviewBasis,
    #[error("review basis `{label}` references missing source `{path}`")]
    MissingReviewBasisSource { label: String, path: String },
}

pub fn parse_intent_contract_str(json: &str) -> Result<IntentContract, IntentContractLoadError> {
    serde_json::from_str(json).map_err(|source| IntentContractLoadError::Json {
        path: "<inline>".to_string(),
        source,
    })
}

pub fn read_intent_contract(
    path: impl AsRef<Path>,
) -> Result<IntentContract, IntentContractLoadError> {
    let path = path.as_ref();
    if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
        return Err(IntentContractLoadError::UnsupportedFormat {
            path: path.display().to_string(),
        });
    }

    let body = fs::read_to_string(path).map_err(|source| IntentContractLoadError::Io {
        path: path.display().to_string(),
        source,
    })?;
    serde_json::from_str(&body).map_err(|source| IntentContractLoadError::Json {
        path: path.display().to_string(),
        source,
    })
}

pub fn verify_intent_contract_source_binding(
    contract: &IntentContract,
    workspace_root: impl AsRef<Path>,
) -> Result<(), IntentContractSourceBindingError> {
    verify_contract_source_digest(contract, workspace_root.as_ref())?;

    if !same_source(
        &contract.source_ref,
        &contract.metadata.authoritative_intent_source,
    ) {
        return Err(
            IntentContractSourceBindingError::AuthoritativeSourceConflict {
                contract_path: contract.source_ref.path.clone(),
                authoritative_path: contract.metadata.authoritative_intent_source.path.clone(),
            },
        );
    }

    if contract.metadata.review_basis.is_empty() {
        return Err(IntentContractSourceBindingError::MissingReviewBasis);
    }

    for review_input in &contract.metadata.review_basis {
        let review_source_path = workspace_root.as_ref().join(&review_input.source.path);
        if !review_source_path.is_file() {
            return Err(IntentContractSourceBindingError::MissingReviewBasisSource {
                label: review_input.label.clone(),
                path: review_input.source.path.clone(),
            });
        }
    }

    Ok(())
}

fn verify_contract_source_digest(
    contract: &IntentContract,
    workspace_root: &Path,
) -> Result<(), IntentContractSourceBindingError> {
    let source_path = workspace_root.join(&contract.source_ref.path);
    if !source_path.is_file() {
        return Err(IntentContractSourceBindingError::MissingSource {
            path: contract.source_ref.path.clone(),
        });
    }

    let algorithm = contract.source_digest.algorithm.to_ascii_lowercase();
    if algorithm != "sha256" {
        return Err(IntentContractSourceBindingError::UnsupportedAlgorithm {
            algorithm: contract.source_digest.algorithm.clone(),
        });
    }

    let actual =
        sha256_hex(&source_path).map_err(|_| IntentContractSourceBindingError::MissingSource {
            path: contract.source_ref.path.clone(),
        })?;
    if actual != contract.source_digest.value {
        return Err(IntentContractSourceBindingError::DigestMismatch {
            path: contract.source_ref.path.clone(),
            expected: contract.source_digest.value.clone(),
            actual,
        });
    }

    Ok(())
}

fn same_source(left: &ContractSourceRef, right: &ContractSourceRef) -> bool {
    left.kind == right.kind && left.path == right.path
}

fn sha256_hex(path: &Path) -> Result<String, std::io::Error> {
    let bytes = fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_path(relative: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
    }

    #[test]
    fn source_binding_verification_detects_digest_mismatch() {
        let mut contract = read_intent_contract(fixture_path(
            "tests/fixtures/intent_alignment/contracts/cylinder_sequence_contract.json",
        ))
        .expect("fixture should load");
        contract.source_digest.value = "deadbeef".to_string();

        let error = verify_intent_contract_source_binding(&contract, fixture_path("."))
            .expect_err("digest mismatch should fail");

        assert_eq!(
            error,
            IntentContractSourceBindingError::DigestMismatch {
                path: "docs/architecture/intent_alignment_verification.md".to_string(),
                expected: "deadbeef".to_string(),
                actual: "c1b32a71b9e47142e5b9ed142384e6f68568f635e71bdee7d35e661b7cb3d61e"
                    .to_string(),
            }
        );
    }

    #[test]
    fn source_binding_verification_rejects_authoritative_source_conflicts() {
        let mut contract = read_intent_contract(fixture_path(
            "tests/fixtures/intent_alignment/contracts/cylinder_sequence_contract.json",
        ))
        .expect("fixture should load");
        contract.metadata.authoritative_intent_source.path =
            "examples/two_cylinder.plc".to_string();

        let error = verify_intent_contract_source_binding(&contract, fixture_path("."))
            .expect_err("conflicting authoritative source should fail");

        assert_eq!(
            error,
            IntentContractSourceBindingError::AuthoritativeSourceConflict {
                contract_path: "docs/architecture/intent_alignment_verification.md".to_string(),
                authoritative_path: "examples/two_cylinder.plc".to_string(),
            }
        );
    }

    #[test]
    fn source_binding_verification_requires_review_basis_sources_to_exist() {
        let mut contract = read_intent_contract(fixture_path(
            "tests/fixtures/intent_alignment/contracts/cylinder_sequence_contract.json",
        ))
        .expect("fixture should load");
        contract.metadata.review_basis[0].source.path = "docs/architecture/missing.md".to_string();

        let error = verify_intent_contract_source_binding(&contract, fixture_path("."))
            .expect_err("missing review basis source should fail");

        assert_eq!(
            error,
            IntentContractSourceBindingError::MissingReviewBasisSource {
                label: "Architecture semantics review".to_string(),
                path: "docs/architecture/missing.md".to_string(),
            }
        );
    }
}
