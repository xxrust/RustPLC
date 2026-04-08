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
pub struct ObservedMilestoneEvidence {
    pub source: MilestoneEvidenceSource,
    pub key: String,
    pub expected: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentMilestone {
    pub milestone_id: String,
    pub business_milestone: BusinessMilestone,
    pub observed_as: Vec<ObservedMilestoneEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentContract {
    pub contract_version: String,
    pub source_ref: ContractSourceRef,
    pub source_digest: ContractSourceDigest,
    pub metadata: ContractMetadata,
    pub intent_sequence: Vec<IntentMilestone>,
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
    let source_path = workspace_root.as_ref().join(&contract.source_ref.path);
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
}
