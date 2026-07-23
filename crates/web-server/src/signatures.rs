use axum::{
    extract::{Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
    response::Json,
    routing::{get, post},
    Router,
};
use serde_json::{json, Value};

use crate::auth::{bearer_token, AuthUser};
use crate::delivery::{current_evidence_digests, resolve_delivery_project_root};
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SignatureDecision {
    Approve,
    Reject,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SignHoldRequest {
    pub(crate) hold_type: String,
    pub(crate) source_commit: String,
    pub(crate) evidence_digests: BTreeMap<String, String>,
    pub(crate) decision: SignatureDecision,
    #[serde(default)]
    pub(crate) comment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct HoldSignature {
    pub(crate) schema_version: u32,
    pub(crate) signature_id: String,
    pub(crate) project_id: String,
    pub(crate) hold_id: String,
    pub(crate) hold_type: String,
    pub(crate) user: AuthUser,
    pub(crate) source_commit: String,
    pub(crate) evidence_digests: BTreeMap<String, String>,
    pub(crate) decision: SignatureDecision,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) comment: Option<String>,
    pub(crate) signed_at: String,
    pub(crate) signed_at_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct HoldSignatureView {
    #[serde(flatten)]
    pub(crate) signature: HoldSignature,
    pub(crate) stale: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SignatureError {
    InvalidProjectId,
    InvalidHoldId,
    InvalidHoldType,
    MissingSourceCommit,
    MissingEvidence,
    EvidenceChanged,
    ForbiddenRole,
    Storage(String),
}

#[derive(Clone)]
pub(crate) struct SignatureStore {
    root: PathBuf,
    append_lock: Arc<Mutex<()>>,
}

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/delivery-projects/{id}/holds/signatures",
            get(list_signatures),
        )
        .route(
            "/delivery-projects/{id}/holds/{hold_id}/sign",
            post(sign_hold),
        )
}

impl SignatureStore {
    pub(crate) fn new(workspace_root: &Path) -> Self {
        Self {
            root: workspace_root.join("out").join("web-signatures"),
            append_lock: Arc::new(Mutex::new(())),
        }
    }

    pub(crate) async fn sign(
        &self,
        project_id: &str,
        hold_id: &str,
        user: AuthUser,
        request: SignHoldRequest,
        current_evidence_digests: &BTreeMap<String, String>,
    ) -> Result<HoldSignature, SignatureError> {
        validate_identifier(project_id).map_err(|_| SignatureError::InvalidProjectId)?;
        validate_identifier(hold_id).map_err(|_| SignatureError::InvalidHoldId)?;
        validate_hold_type(&request.hold_type)?;
        if request.source_commit.trim().is_empty() {
            return Err(SignatureError::MissingSourceCommit);
        }
        if request.evidence_digests.is_empty() {
            return Err(SignatureError::MissingEvidence);
        }
        if &request.evidence_digests != current_evidence_digests {
            return Err(SignatureError::EvidenceChanged);
        }
        if !user.role.can_sign(&request.hold_type) {
            return Err(SignatureError::ForbiddenRole);
        }

        let signed_at_ms = now_ms();
        let signed_at =
            OffsetDateTime::from_unix_timestamp_nanos(i128::from(signed_at_ms) * 1_000_000)
                .unwrap_or(OffsetDateTime::UNIX_EPOCH)
                .format(&Rfc3339)
                .unwrap_or_else(|_| signed_at_ms.to_string());
        let signature = HoldSignature {
            schema_version: 1,
            signature_id: Uuid::new_v4().to_string(),
            project_id: project_id.to_string(),
            hold_id: hold_id.to_string(),
            hold_type: request.hold_type,
            user,
            source_commit: request.source_commit,
            evidence_digests: request.evidence_digests,
            decision: request.decision,
            comment: request.comment.filter(|comment| !comment.trim().is_empty()),
            signed_at,
            signed_at_ms,
        };

        let _guard = self.append_lock.lock().await;
        let path = self.signature_path(project_id)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| SignatureError::Storage(err.to_string()))?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|err| SignatureError::Storage(err.to_string()))?;
        serde_json::to_writer(&mut file, &signature)
            .map_err(|err| SignatureError::Storage(err.to_string()))?;
        file.write_all(b"\n")
            .map_err(|err| SignatureError::Storage(err.to_string()))?;
        file.flush()
            .map_err(|err| SignatureError::Storage(err.to_string()))?;

        Ok(signature)
    }

    pub(crate) fn list(
        &self,
        project_id: &str,
        current_evidence_digests: &BTreeMap<String, String>,
    ) -> Result<Vec<HoldSignatureView>, SignatureError> {
        let path = self.signature_path(project_id)?;
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = fs::File::open(path).map_err(|err| SignatureError::Storage(err.to_string()))?;
        let mut signatures = Vec::new();
        for line in BufReader::new(file).lines() {
            let line = line.map_err(|err| SignatureError::Storage(err.to_string()))?;
            if line.trim().is_empty() {
                continue;
            }
            let signature: HoldSignature = serde_json::from_str(&line)
                .map_err(|err| SignatureError::Storage(err.to_string()))?;
            let stale = signature.evidence_digests != *current_evidence_digests;
            signatures.push(HoldSignatureView { signature, stale });
        }
        Ok(signatures)
    }

    fn signature_path(&self, project_id: &str) -> Result<PathBuf, SignatureError> {
        validate_identifier(project_id).map_err(|_| SignatureError::InvalidProjectId)?;
        Ok(self.root.join(project_id).join("signatures.jsonl"))
    }
}

async fn list_signatures(
    State(state): State<Arc<AppState>>,
    AxumPath(project_id): AxumPath<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let contract = load_signature_contract(&state.workspace_root, &project_id)
        .map_err(|message| api_error(StatusCode::BAD_REQUEST, message))?;
    let digests = current_evidence_digests(&state.workspace_root, &project_id)
        .map_err(|message| api_error(StatusCode::BAD_REQUEST, message))?;
    let signatures = state
        .signatures
        .list(&project_id, &digests)
        .map_err(map_signature_error)?;
    Ok(Json(json!({
        "schema_version": 1,
        "project_id": project_id,
        "source_commit": contract.source_commit,
        "digest_algorithm": "sha256",
        "digest_normalization": "raw_bytes",
        "current_evidence_digests": digests,
        "signatures": signatures
    })))
}

async fn sign_hold(
    State(state): State<Arc<AppState>>,
    AxumPath((project_id, hold_id)): AxumPath<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<SignHoldRequest>,
) -> Result<(StatusCode, Json<HoldSignature>), (StatusCode, Json<Value>)> {
    let token = bearer_token(&headers)
        .ok_or_else(|| api_error(StatusCode::UNAUTHORIZED, "authentication required"))?;
    let user = state
        .auth
        .authenticate(token)
        .await
        .map_err(|_| api_error(StatusCode::UNAUTHORIZED, "session is not valid"))?;
    let contract = load_signature_contract(&state.workspace_root, &project_id)
        .map_err(|message| api_error(StatusCode::BAD_REQUEST, message))?;
    let Some(hold_status) = contract.holds.get(&hold_id) else {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            format!("hold `{hold_id}` is not declared by project `{project_id}`"),
        ));
    };
    if request.hold_type != hold_id {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "hold_type must match the hold_id in the route",
        ));
    }
    if request.source_commit != contract.source_commit {
        return Err(api_error(
            StatusCode::CONFLICT,
            "source commit changed; refresh the project before signing",
        ));
    }
    if hold_status == "blocked" && request.decision == SignatureDecision::Approve {
        return Err(api_error(
            StatusCode::CONFLICT,
            "blocked holds cannot be approved until their prerequisite evidence is current",
        ));
    }
    let digests = current_evidence_digests(&state.workspace_root, &project_id)
        .map_err(|message| api_error(StatusCode::BAD_REQUEST, message))?;
    if hold_id == "release_approval" && request.decision == SignatureDecision::Approve {
        if !delivery_status_allows_release(contract.delivery_status.as_deref()) {
            return Err(api_error(
                StatusCode::CONFLICT,
                "release approval requires delivery_status pass or current",
            ));
        }
        let existing = state
            .signatures
            .list(&project_id, &digests)
            .map_err(map_signature_error)?;
        let missing = missing_release_prerequisites(&contract, &existing);
        if !missing.is_empty() {
            return Err(api_error(
                StatusCode::CONFLICT,
                format!(
                    "release approval requires current approved signatures for: {}",
                    missing.join(", ")
                ),
            ));
        }
    }
    let signature = state
        .signatures
        .sign(&project_id, &hold_id, user, request, &digests)
        .await
        .map_err(map_signature_error)?;
    Ok((StatusCode::CREATED, Json(signature)))
}

struct SignatureContract {
    source_commit: String,
    delivery_status: Option<String>,
    holds: BTreeMap<String, String>,
}

fn load_signature_contract(
    workspace_root: &Path,
    project_id: &str,
) -> Result<SignatureContract, String> {
    let project_root = resolve_delivery_project_root(workspace_root, project_id)?;
    let manifest_path = project_root.join("delivery-project.json");
    let manifest: Value = serde_json::from_slice(
        &fs::read(&manifest_path)
            .map_err(|err| format!("failed to read delivery project manifest: {err}"))?,
    )
    .map_err(|err| format!("delivery project manifest is invalid: {err}"))?;
    let source_commit = manifest
        .get("source_commit")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "delivery project does not declare source_commit".to_string())?
        .to_string();
    let delivery_status = manifest
        .get("delivery_status")
        .and_then(Value::as_str)
        .map(str::to_string);
    let hold_ref = manifest
        .pointer("/fixtures/human_holds/fixture_ref")
        .and_then(Value::as_str)
        .unwrap_or("release/human-holds.json");
    let hold_path = project_root.join(hold_ref);
    let canonical_hold_path = hold_path
        .canonicalize()
        .map_err(|err| format!("failed to resolve human hold contract: {err}"))?;
    if !canonical_hold_path.starts_with(&project_root) {
        return Err("human hold contract escapes the delivery project".to_string());
    }
    let hold_document: Value = serde_json::from_slice(
        &fs::read(&canonical_hold_path)
            .map_err(|err| format!("failed to read human hold contract: {err}"))?,
    )
    .map_err(|err| format!("human hold contract is invalid: {err}"))?;
    let holds = hold_document
        .get("holds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|hold| {
            Some((
                hold.get("hold_id")?.as_str()?.to_string(),
                hold.get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("human_action_required")
                    .to_string(),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    if holds.is_empty() {
        return Err("human hold contract does not declare any holds".to_string());
    }
    Ok(SignatureContract {
        source_commit,
        delivery_status,
        holds,
    })
}

fn delivery_status_allows_release(status: Option<&str>) -> bool {
    matches!(status, Some("pass" | "current"))
}

fn missing_release_prerequisites(
    contract: &SignatureContract,
    signatures: &[HoldSignatureView],
) -> Vec<String> {
    contract
        .holds
        .keys()
        .filter(|hold_id| hold_id.as_str() != "release_approval")
        .filter(|hold_id| {
            signatures
                .iter()
                .rev()
                .find(|view| view.signature.hold_id == hold_id.as_str() && !view.stale)
                .is_none_or(|view| view.signature.decision != SignatureDecision::Approve)
        })
        .cloned()
        .collect()
}

fn map_signature_error(error: SignatureError) -> (StatusCode, Json<Value>) {
    match error {
        SignatureError::ForbiddenRole => api_error(
            StatusCode::FORBIDDEN,
            "the authenticated role cannot sign this hold",
        ),
        SignatureError::EvidenceChanged => api_error(
            StatusCode::CONFLICT,
            "evidence changed; refresh the digest set before signing",
        ),
        SignatureError::Storage(message) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("signature storage failed: {message}"),
        ),
        SignatureError::InvalidProjectId
        | SignatureError::InvalidHoldId
        | SignatureError::InvalidHoldType
        | SignatureError::MissingSourceCommit
        | SignatureError::MissingEvidence => api_error(
            StatusCode::BAD_REQUEST,
            format!("invalid signature request: {error:?}"),
        ),
    }
}

fn api_error(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<Value>) {
    let message = message.into();
    (
        status,
        Json(json!({ "error": message, "message": message })),
    )
}

fn validate_identifier(value: &str) -> Result<(), ()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(());
    }
    Ok(())
}

fn validate_hold_type(value: &str) -> Result<(), SignatureError> {
    if matches!(
        value,
        "wiring_review"
            | "point_check_completion"
            | "safety_review"
            | "hil_review"
            | "release_approval"
    ) {
        Ok(())
    } else {
        Err(SignatureError::InvalidHoldType)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::UserRole;

    fn temp_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be valid")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rustplc-signatures-{label}-{unique}"));
        fs::create_dir_all(&root).expect("temp root should be created");
        root
    }

    fn user(role: UserRole) -> AuthUser {
        AuthUser {
            id: "reviewer".to_string(),
            name: "Reviewer".to_string(),
            role,
        }
    }

    fn digests(value: &str) -> BTreeMap<String, String> {
        BTreeMap::from([("evidence/report.json".to_string(), value.to_string())])
    }

    fn signature_view(
        hold_id: &str,
        decision: SignatureDecision,
        stale: bool,
    ) -> HoldSignatureView {
        HoldSignatureView {
            signature: HoldSignature {
                schema_version: 1,
                signature_id: format!("signature-{hold_id}"),
                project_id: "station.demo".to_string(),
                hold_id: hold_id.to_string(),
                hold_type: hold_id.to_string(),
                user: user(UserRole::Admin),
                source_commit: "deadbeef".to_string(),
                evidence_digests: digests("abc"),
                decision,
                comment: None,
                signed_at: "2026-07-24T00:00:00Z".to_string(),
                signed_at_ms: 1,
            },
            stale,
        }
    }

    #[tokio::test]
    async fn signature_is_append_only_and_attributable() {
        let root = temp_root("append");
        let store = SignatureStore::new(&root);
        let current = digests("abc");
        let signature = store
            .sign(
                "station.demo",
                "safety-gate",
                user(UserRole::SafetyReviewer),
                SignHoldRequest {
                    hold_type: "safety_review".to_string(),
                    source_commit: "deadbeef".to_string(),
                    evidence_digests: current.clone(),
                    decision: SignatureDecision::Approve,
                    comment: Some("reviewed".to_string()),
                },
                &current,
            )
            .await
            .expect("signature should be written");

        let listed = store
            .list("station.demo", &current)
            .expect("list should load");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].signature, signature);
        assert!(!listed[0].stale);
    }

    #[tokio::test]
    async fn changed_evidence_rejects_new_signature_and_stales_old_signature() {
        let root = temp_root("stale");
        let store = SignatureStore::new(&root);
        let original = digests("abc");
        store
            .sign(
                "station.demo",
                "release-gate",
                user(UserRole::ReleaseApprover),
                SignHoldRequest {
                    hold_type: "release_approval".to_string(),
                    source_commit: "deadbeef".to_string(),
                    evidence_digests: original.clone(),
                    decision: SignatureDecision::Approve,
                    comment: None,
                },
                &original,
            )
            .await
            .expect("initial signature should be written");

        let changed = digests("def");
        assert_eq!(
            store
                .sign(
                    "station.demo",
                    "release-gate",
                    user(UserRole::ReleaseApprover),
                    SignHoldRequest {
                        hold_type: "release_approval".to_string(),
                        source_commit: "deadbeef".to_string(),
                        evidence_digests: original,
                        decision: SignatureDecision::Approve,
                        comment: None,
                    },
                    &changed,
                )
                .await,
            Err(SignatureError::EvidenceChanged)
        );
        assert!(
            store
                .list("station.demo", &changed)
                .expect("list should load")[0]
                .stale
        );
    }

    #[tokio::test]
    async fn role_cannot_sign_unowned_hold() {
        let root = temp_root("role");
        let store = SignatureStore::new(&root);
        let current = digests("abc");
        let error = store
            .sign(
                "station.demo",
                "release-gate",
                user(UserRole::Engineer),
                SignHoldRequest {
                    hold_type: "release_approval".to_string(),
                    source_commit: "deadbeef".to_string(),
                    evidence_digests: current.clone(),
                    decision: SignatureDecision::Approve,
                    comment: None,
                },
                &current,
            )
            .await
            .expect_err("role must not sign release approval");
        assert_eq!(error, SignatureError::ForbiddenRole);
    }

    #[test]
    fn release_requires_current_approved_prerequisite_signatures() {
        let contract = SignatureContract {
            source_commit: "deadbeef".to_string(),
            delivery_status: Some("current".to_string()),
            holds: BTreeMap::from([
                (
                    "wiring_review".to_string(),
                    "human_action_required".to_string(),
                ),
                (
                    "safety_review".to_string(),
                    "human_action_required".to_string(),
                ),
                (
                    "release_approval".to_string(),
                    "human_action_required".to_string(),
                ),
            ]),
        };
        let wiring = signature_view("wiring_review", SignatureDecision::Approve, false);
        assert_eq!(
            missing_release_prerequisites(&contract, &[wiring.clone()]),
            vec!["safety_review".to_string()]
        );
        let stale_safety = signature_view("safety_review", SignatureDecision::Approve, true);
        assert_eq!(
            missing_release_prerequisites(&contract, &[wiring.clone(), stale_safety]),
            vec!["safety_review".to_string()]
        );
        let rejected_safety = signature_view("safety_review", SignatureDecision::Reject, false);
        assert_eq!(
            missing_release_prerequisites(&contract, &[wiring.clone(), rejected_safety]),
            vec!["safety_review".to_string()]
        );
        let approved_safety = signature_view("safety_review", SignatureDecision::Approve, false);
        assert!(missing_release_prerequisites(&contract, &[wiring, approved_safety]).is_empty());
    }

    #[test]
    fn release_delivery_status_requires_current_or_pass() {
        assert!(delivery_status_allows_release(Some("current")));
        assert!(delivery_status_allows_release(Some("pass")));
        for status in [
            None,
            Some("fail"),
            Some("blocked"),
            Some("stale"),
            Some("missing"),
        ] {
            assert!(!delivery_status_allows_release(status));
        }
    }
}
