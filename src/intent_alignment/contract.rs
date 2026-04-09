use std::fs;
use std::path::Path;
use std::{collections::BTreeSet, fmt};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentContractDiagnosticCode {
    ConflictingRequiredEdges,
    UnreachableMilestone,
    ContradictoryCycleSemantics,
}

impl IntentContractDiagnosticCode {
    pub fn stable_code(self) -> &'static str {
        match self {
            Self::ConflictingRequiredEdges => "IAC-VAL-001",
            Self::UnreachableMilestone => "IAC-VAL-002",
            Self::ContradictoryCycleSemantics => "IAC-VAL-003",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentContractDiagnostic {
    pub code: IntentContractDiagnosticCode,
    pub subject: String,
    pub detail: String,
}

impl IntentContractDiagnostic {
    fn new(
        code: IntentContractDiagnosticCode,
        subject: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            code,
            subject: subject.into(),
            detail: detail.into(),
        }
    }

    pub fn stable_code(&self) -> &'static str {
        self.code.stable_code()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntentContractValidationError {
    pub diagnostics: Vec<IntentContractDiagnostic>,
}

impl fmt::Display for IntentContractValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "intent contract validation failed with {} diagnostic(s)",
            self.diagnostics.len()
        )?;
        for diagnostic in &self.diagnostics {
            writeln!(
                f,
                "- {} {}: {}",
                diagnostic.stable_code(),
                diagnostic.subject,
                diagnostic.detail
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for IntentContractValidationError {}

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

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IntentContractDeliveryReadinessError {
    #[error(
        "intent contract still uses the scaffold placeholder source digest `{value}`; replace it with the authored source sha256 before calling the delivery validated"
    )]
    PlaceholderSourceDigest { value: String },
    #[error(
        "intent contract binding `{binding_id}` still uses the scaffold placeholder binding id; replace it with a real authored binding id before validation"
    )]
    PlaceholderBindingId { binding_id: String },
    #[error(
        "intent contract binding `{binding_id}` still uses the scaffold placeholder evidence `{expected}`; freeze a real comparator-supported anchor before validation"
    )]
    PlaceholderEvidence {
        binding_id: String,
        expected: String,
    },
}

const PLACEHOLDER_SOURCE_DIGEST: &str = "replace_me_after_authoring";
const PLACEHOLDER_BINDING_ID: &str = "replace_with_real_anchor";
const PLACEHOLDER_TRACE_EXPECTED: &str = "replace_after_intent_doctor";

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

pub fn validate_intent_contract(
    contract: &IntentContract,
) -> Result<(), IntentContractValidationError> {
    let milestone_ids: BTreeSet<&str> = contract
        .contract_core
        .expected_milestones
        .iter()
        .map(|milestone| milestone.milestone_id.as_str())
        .collect();
    let postcondition_ids: BTreeSet<&str> = contract
        .contract_core
        .postconditions
        .iter()
        .map(|postcondition| postcondition.postcondition_id.as_str())
        .collect();
    let mut diagnostics = Vec::new();

    validate_cycle_semantics(
        contract,
        &milestone_ids,
        &postcondition_ids,
        &mut diagnostics,
    );
    validate_required_edges(contract, &milestone_ids, &mut diagnostics);
    validate_milestone_reachability(contract, &milestone_ids, &mut diagnostics);

    if diagnostics.is_empty() {
        return Ok(());
    }

    diagnostics.sort_by(|left, right| {
        left.code
            .cmp(&right.code)
            .then_with(|| left.subject.cmp(&right.subject))
            .then_with(|| left.detail.cmp(&right.detail))
    });
    diagnostics.dedup();
    Err(IntentContractValidationError { diagnostics })
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

pub fn verify_intent_contract_delivery_readiness(
    contract: &IntentContract,
) -> Result<(), IntentContractDeliveryReadinessError> {
    if contract.source_digest.value == PLACEHOLDER_SOURCE_DIGEST {
        return Err(
            IntentContractDeliveryReadinessError::PlaceholderSourceDigest {
                value: contract.source_digest.value.clone(),
            },
        );
    }

    for binding in &contract.observation_bindings {
        if binding.binding_id == PLACEHOLDER_BINDING_ID {
            return Err(IntentContractDeliveryReadinessError::PlaceholderBindingId {
                binding_id: binding.binding_id.clone(),
            });
        }

        if let Some(evidence) = binding
            .evidence
            .iter()
            .find(|evidence| evidence.expected == PLACEHOLDER_TRACE_EXPECTED)
        {
            return Err(IntentContractDeliveryReadinessError::PlaceholderEvidence {
                binding_id: binding.binding_id.clone(),
                expected: evidence.expected.clone(),
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

fn validate_cycle_semantics(
    contract: &IntentContract,
    milestone_ids: &BTreeSet<&str>,
    postcondition_ids: &BTreeSet<&str>,
    diagnostics: &mut Vec<IntentContractDiagnostic>,
) {
    let cycle = &contract.contract_core.cycle_semantics;
    let restart = &cycle.restart_semantics;

    if !milestone_ids.contains(cycle.cycle_start_milestone.as_str()) {
        diagnostics.push(IntentContractDiagnostic::new(
            IntentContractDiagnosticCode::ContradictoryCycleSemantics,
            "cycle_semantics.cycle_start_milestone",
            format!(
                "references unknown milestone `{}`",
                cycle.cycle_start_milestone
            ),
        ));
    }

    if !milestone_ids.contains(cycle.cycle_complete_milestone.as_str()) {
        diagnostics.push(IntentContractDiagnostic::new(
            IntentContractDiagnosticCode::ContradictoryCycleSemantics,
            "cycle_semantics.cycle_complete_milestone",
            format!(
                "references unknown milestone `{}`",
                cycle.cycle_complete_milestone
            ),
        ));
    }

    if !milestone_ids.contains(restart.restartable_milestone.as_str()) {
        diagnostics.push(IntentContractDiagnostic::new(
            IntentContractDiagnosticCode::ContradictoryCycleSemantics,
            "cycle_semantics.restart_semantics.restartable_milestone",
            format!(
                "references unknown milestone `{}`",
                restart.restartable_milestone
            ),
        ));
    }

    if !milestone_ids.contains(restart.next_cycle_start_milestone.as_str()) {
        diagnostics.push(IntentContractDiagnostic::new(
            IntentContractDiagnosticCode::ContradictoryCycleSemantics,
            "cycle_semantics.restart_semantics.next_cycle_start_milestone",
            format!(
                "references unknown milestone `{}`",
                restart.next_cycle_start_milestone
            ),
        ));
    }

    if restart.restartable_milestone != cycle.cycle_complete_milestone {
        diagnostics.push(IntentContractDiagnostic::new(
            IntentContractDiagnosticCode::ContradictoryCycleSemantics,
            "cycle_semantics.restart_semantics.restartable_milestone",
            format!(
                "must match cycle_complete_milestone `{}`",
                cycle.cycle_complete_milestone
            ),
        ));
    }

    if restart.next_cycle_start_milestone != cycle.cycle_start_milestone {
        diagnostics.push(IntentContractDiagnostic::new(
            IntentContractDiagnosticCode::ContradictoryCycleSemantics,
            "cycle_semantics.restart_semantics.next_cycle_start_milestone",
            format!(
                "must match cycle_start_milestone `{}`",
                cycle.cycle_start_milestone
            ),
        ));
    }

    for postcondition_id in &restart.required_postconditions {
        if !postcondition_ids.contains(postcondition_id.as_str()) {
            diagnostics.push(IntentContractDiagnostic::new(
                IntentContractDiagnosticCode::ContradictoryCycleSemantics,
                "cycle_semantics.restart_semantics.required_postconditions",
                format!("references unknown postcondition `{}`", postcondition_id),
            ));
        }
    }
}

fn validate_required_edges(
    contract: &IntentContract,
    milestone_ids: &BTreeSet<&str>,
    diagnostics: &mut Vec<IntentContractDiagnostic>,
) {
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();

    for milestone_id in milestone_ids {
        detect_required_edge_cycle(
            contract,
            milestone_id,
            milestone_ids,
            &mut visiting,
            &mut visited,
            diagnostics,
        );
    }
}

fn detect_required_edge_cycle<'a>(
    contract: &'a IntentContract,
    current: &'a str,
    milestone_ids: &BTreeSet<&'a str>,
    visiting: &mut BTreeSet<&'a str>,
    visited: &mut BTreeSet<&'a str>,
    diagnostics: &mut Vec<IntentContractDiagnostic>,
) {
    if visited.contains(current) {
        return;
    }

    visiting.insert(current);
    for edge in contract
        .contract_core
        .required_edges
        .iter()
        .filter(|edge| edge.predecessor == current)
    {
        let successor = edge.successor.as_str();
        if !milestone_ids.contains(successor) {
            continue;
        }

        if visiting.contains(successor) {
            diagnostics.push(IntentContractDiagnostic::new(
                IntentContractDiagnosticCode::ConflictingRequiredEdges,
                format!("{} -> {}", edge.predecessor, edge.successor),
                "required edges create a cycle, so milestone ordering is contradictory",
            ));
            continue;
        }

        detect_required_edge_cycle(
            contract,
            successor,
            milestone_ids,
            visiting,
            visited,
            diagnostics,
        );
    }

    visiting.remove(current);
    visited.insert(current);
}

fn validate_milestone_reachability(
    contract: &IntentContract,
    milestone_ids: &BTreeSet<&str>,
    diagnostics: &mut Vec<IntentContractDiagnostic>,
) {
    let cycle = &contract.contract_core.cycle_semantics;
    let cycle_start = cycle.cycle_start_milestone.as_str();
    let cycle_complete = cycle.cycle_complete_milestone.as_str();
    if !milestone_ids.contains(cycle_start) || !milestone_ids.contains(cycle_complete) {
        return;
    }

    let reachable_from_start = collect_reachable_milestones(
        contract,
        milestone_ids,
        cycle_start,
        |edge, current| edge.predecessor == current,
        |edge| edge.successor.as_str(),
    );
    let reachable_to_complete = collect_reachable_milestones(
        contract,
        milestone_ids,
        cycle_complete,
        |edge, current| edge.successor == current,
        |edge| edge.predecessor.as_str(),
    );

    for milestone_id in milestone_ids {
        if !reachable_from_start.contains(milestone_id) {
            diagnostics.push(IntentContractDiagnostic::new(
                IntentContractDiagnosticCode::UnreachableMilestone,
                *milestone_id,
                format!(
                    "is not reachable from cycle_start_milestone `{}` through required_edges",
                    cycle_start
                ),
            ));
        } else if !reachable_to_complete.contains(milestone_id) {
            diagnostics.push(IntentContractDiagnostic::new(
                IntentContractDiagnosticCode::UnreachableMilestone,
                *milestone_id,
                format!(
                    "cannot reach cycle_complete_milestone `{}` through required_edges",
                    cycle_complete
                ),
            ));
        }
    }
}

fn collect_reachable_milestones<'a, Filter, Next>(
    contract: &'a IntentContract,
    milestone_ids: &BTreeSet<&'a str>,
    start: &'a str,
    edge_matches: Filter,
    next_milestone: Next,
) -> BTreeSet<&'a str>
where
    Filter: Fn(&RequiredMilestoneEdge, &str) -> bool,
    Next: Fn(&'a RequiredMilestoneEdge) -> &'a str,
{
    let mut frontier = vec![start];
    let mut visited = BTreeSet::new();

    while let Some(current) = frontier.pop() {
        if !visited.insert(current) {
            continue;
        }

        for edge in &contract.contract_core.required_edges {
            if !edge_matches(edge, current) {
                continue;
            }
            let next = next_milestone(edge);
            if milestone_ids.contains(next) && !visited.contains(next) {
                frontier.push(next);
            }
        }
    }

    visited
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
                actual: "10b8ce179f80c7862ff82a3b363b8792d6ff106895f68609d09558f1b8deb83c"
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
