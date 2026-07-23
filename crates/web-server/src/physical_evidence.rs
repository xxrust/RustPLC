use axum::{
    body::Bytes,
    extract::{Path as AxumPath, State},
    http::{header, HeaderMap, StatusCode},
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::auth::{bearer_token, AuthUser, UserRole};
use crate::delivery::{
    current_evidence_digests, delivery_deep_link, resolve_delivery_project_root,
};
use crate::signatures::SignatureDecision;
use crate::AppState;

const MAX_OBSERVATION_NOTE_LEN: usize = 4_096;
const MAX_MEASUREMENT_FIELD_LEN: usize = 256;
const MAX_UPLOAD_FILENAME_LEN: usize = 160;
const MAX_UPLOAD_BYTES: usize = 16 * 1024 * 1024;
const MAX_STORED_RECORDS: usize = 20_000;
const RELEASE_PREREQUISITES: [&str; 4] = [
    "wiring_review",
    "point_check_completion",
    "safety_review",
    "hil_review",
];

type ApiError = (StatusCode, Json<Value>);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Measurement {
    pub(crate) value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) instrument_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PointObservationStatus {
    Pass,
    Fail,
    Blocked,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RecordPointObservationRequest {
    pub(crate) status: PointObservationStatus,
    #[serde(default)]
    pub(crate) measurement: Option<Measurement>,
    #[serde(default)]
    pub(crate) photo_upload_id: Option<String>,
    #[serde(default)]
    pub(crate) trace_ref: Option<String>,
    #[serde(default)]
    pub(crate) note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PointObservation {
    pub(crate) schema_version: u32,
    pub(crate) observation_id: String,
    pub(crate) project_id: String,
    pub(crate) point_id: String,
    pub(crate) status: PointObservationStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) measurement: Option<Measurement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) photo_upload_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) trace_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) trace_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) note: Option<String>,
    pub(crate) user: AuthUser,
    pub(crate) source_commit: String,
    pub(crate) observed_at: String,
    pub(crate) observed_at_ms: u64,
    pub(crate) prior_evidence_digest_set_sha256: String,
    pub(crate) deep_link: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct EvidenceUpload {
    pub(crate) schema_version: u32,
    pub(crate) upload_id: String,
    pub(crate) project_id: String,
    pub(crate) original_filename: String,
    pub(crate) artifact_ref: String,
    pub(crate) media_type: String,
    pub(crate) evidence_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) semantic_object_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) semantic_object_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) note: Option<String>,
    pub(crate) size_bytes: usize,
    pub(crate) sha256: String,
    pub(crate) user: AuthUser,
    pub(crate) source_commit: String,
    pub(crate) uploaded_at: String,
    pub(crate) uploaded_at_ms: u64,
    pub(crate) deep_link: Value,
}

#[derive(Clone)]
pub(crate) struct PhysicalEvidenceStore {
    workspace_root: PathBuf,
    root: PathBuf,
    append_lock: Arc<Mutex<()>>,
}

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/delivery-projects/{id}/wiring/points/{point_id}/observations",
            get(list_point_observations).post(record_point_observation),
        )
        .route(
            "/delivery-projects/{id}/evidence/uploads/{filename}",
            post(upload_evidence),
        )
        .route(
            "/delivery-projects/{id}/physical-evidence",
            get(get_physical_evidence),
        )
        .route("/delivery-projects/{id}/holds", get(get_hold_projection))
        .route(
            "/delivery-projects/{id}/release",
            get(get_release_projection),
        )
}

impl PhysicalEvidenceStore {
    pub(crate) fn new(workspace_root: &Path) -> Self {
        Self {
            workspace_root: workspace_root.to_path_buf(),
            root: workspace_root.join("out").join("web-delivery-evidence"),
            append_lock: Arc::new(Mutex::new(())),
        }
    }

    pub(crate) fn project_root(&self, project_id: &str) -> Result<PathBuf, String> {
        validate_identifier(project_id, false)?;
        Ok(self.root.join(project_id))
    }

    pub(crate) fn observations(&self, project_id: &str) -> Result<Vec<PointObservation>, String> {
        let path = self
            .project_root(project_id)?
            .join("point-observations.jsonl");
        read_json_lines(&path)
    }

    pub(crate) fn uploads(&self, project_id: &str) -> Result<Vec<EvidenceUpload>, String> {
        let path = self.project_root(project_id)?.join("uploads.jsonl");
        read_json_lines(&path)
    }

    async fn append_observation(&self, observation: &PointObservation) -> Result<(), String> {
        let path = self
            .project_root(&observation.project_id)?
            .join("point-observations.jsonl");
        self.append_json_line(&path, observation).await
    }

    async fn persist_upload(
        &self,
        project_id: &str,
        stored_name: &str,
        bytes: &[u8],
        upload: &EvidenceUpload,
    ) -> Result<(), String> {
        let project_root = self.project_root(project_id)?;
        let uploads_root = project_root.join("uploads");
        fs::create_dir_all(&uploads_root).map_err(|err| err.to_string())?;
        let target = uploads_root.join(stored_name);
        let _guard = self.append_lock.lock().await;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|err| err.to_string())?;
        file.write_all(bytes).map_err(|err| err.to_string())?;
        file.flush().map_err(|err| err.to_string())?;
        let metadata_path = project_root.join("uploads.jsonl");
        if let Err(err) = append_json_line_unlocked(&metadata_path, upload) {
            let _ = fs::remove_file(&target);
            return Err(err);
        }
        Ok(())
    }

    async fn append_json_line<T: Serialize>(&self, path: &Path, value: &T) -> Result<(), String> {
        let _guard = self.append_lock.lock().await;
        append_json_line_unlocked(path, value)
    }

    fn resolve_trace_ref(&self, project_id: &str, raw: &str) -> Result<PathBuf, String> {
        let relative = safe_relative_path(raw)?;
        let workspace = self
            .workspace_root
            .canonicalize()
            .map_err(|err| err.to_string())?;
        let path = workspace.join(relative);
        let canonical = path
            .canonicalize()
            .map_err(|_| "trace_ref does not exist".to_string())?;
        let allowed_roots = self.trace_allowed_roots(project_id)?;
        if !canonical.is_file()
            || !canonical.starts_with(&workspace)
            || !allowed_roots.iter().any(|root| canonical.starts_with(root))
        {
            return Err(
                "trace_ref must resolve inside the current project or its declared artifact roots"
                    .to_string(),
            );
        }
        Ok(canonical)
    }

    fn trace_allowed_roots(&self, project_id: &str) -> Result<Vec<PathBuf>, String> {
        let project_root = resolve_delivery_project_root(&self.workspace_root, project_id)?
            .canonicalize()
            .map_err(|err| err.to_string())?;
        let workspace = self
            .workspace_root
            .canonicalize()
            .map_err(|err| err.to_string())?;
        let mut roots = vec![project_root.clone()];
        let manifest = read_json(&project_root.join("delivery-project.json"))?;
        if let Some(artifact_roots) = manifest.get("artifact_roots").and_then(Value::as_object) {
            for raw in artifact_roots.values().filter_map(Value::as_str) {
                let Ok(relative) = safe_relative_path(raw) else {
                    continue;
                };
                for candidate in [project_root.join(&relative), workspace.join(&relative)] {
                    let Ok(canonical) = candidate.canonicalize() else {
                        continue;
                    };
                    if canonical.is_dir() && canonical.starts_with(&workspace) {
                        roots.push(canonical);
                    }
                }
            }
        }
        if let Ok(evidence_root) = self.project_root(project_id) {
            if let Ok(canonical) = evidence_root.canonicalize() {
                if canonical.starts_with(&workspace) {
                    roots.push(canonical);
                }
            }
        }
        roots.sort();
        roots.dedup();
        Ok(roots)
    }
}

async fn upload_evidence(
    State(state): State<Arc<AppState>>,
    AxumPath((project_id, filename)): AxumPath<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<EvidenceUpload>), ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    resolve_delivery_project_root(&state.workspace_root, &project_id)
        .map_err(|message| api_error(StatusCode::NOT_FOUND, message))?;
    let filename = validate_filename(&filename)
        .map_err(|message| api_error(StatusCode::BAD_REQUEST, message))?;
    if body.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "upload body is empty"));
    }
    if body.len() > MAX_UPLOAD_BYTES {
        return Err(api_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("upload exceeds the {MAX_UPLOAD_BYTES} byte limit"),
        ));
    }
    let evidence_kind = required_header(&headers, "x-evidence-kind")?;
    if !matches!(
        evidence_kind.as_str(),
        "photo" | "trace" | "measurement" | "document" | "other"
    ) {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "x-evidence-kind must be photo, trace, measurement, document, or other",
        ));
    }
    let semantic_object_kind = optional_header(&headers, "x-semantic-object-kind")?;
    let semantic_object_id = optional_header(&headers, "x-semantic-object-id")?;
    if semantic_object_kind.is_some() != semantic_object_id.is_some() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "semantic object kind and id must be supplied together",
        ));
    }
    let note = optional_header(&headers, "x-evidence-note")?;
    validate_optional_text(note.as_deref(), MAX_OBSERVATION_NOTE_LEN, "x-evidence-note")?;
    let media_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let source_commit = project_source_commit(&state.workspace_root, &project_id)?;
    let upload_id = Uuid::new_v4().to_string();
    let stored_name = format!("{}-{}", upload_id, filename);
    let artifact_ref = format!("out/web-delivery-evidence/{project_id}/uploads/{stored_name}");
    let uploaded_at_ms = now_ms();
    let sha256 = sha256_bytes(&body);
    let upload = EvidenceUpload {
        schema_version: 1,
        upload_id,
        project_id: project_id.clone(),
        original_filename: filename,
        artifact_ref: artifact_ref.clone(),
        media_type,
        evidence_kind,
        semantic_object_kind: semantic_object_kind.clone(),
        semantic_object_id: semantic_object_id.clone(),
        note,
        size_bytes: body.len(),
        sha256,
        user,
        source_commit: source_commit.clone(),
        uploaded_at: timestamp(uploaded_at_ms),
        uploaded_at_ms,
        deep_link: delivery_deep_link(
            &project_id,
            None,
            Some(&artifact_ref),
            semantic_object_kind.as_deref(),
            semantic_object_id.as_deref(),
            Some(&source_commit),
        ),
    };
    state
        .physical_evidence
        .persist_upload(&project_id, &stored_name, &body, &upload)
        .await
        .map_err(|message| api_error(StatusCode::INTERNAL_SERVER_ERROR, message))?;
    Ok((StatusCode::CREATED, Json(upload)))
}

async fn record_point_observation(
    State(state): State<Arc<AppState>>,
    AxumPath((project_id, point_id)): AxumPath<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<RecordPointObservationRequest>,
) -> Result<(StatusCode, Json<PointObservation>), ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    if !matches!(
        user.role,
        UserRole::ElectricalEngineer | UserRole::CommissioningEngineer | UserRole::Admin
    ) {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "the authenticated role cannot record point-check evidence",
        ));
    }
    validate_identifier(&point_id, true)
        .map_err(|message| api_error(StatusCode::BAD_REQUEST, message))?;
    let points = authored_wiring_points(&state.workspace_root, &project_id)?;
    if !points
        .iter()
        .any(|point| point_id_of(point) == Some(point_id.as_str()))
    {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            format!("wiring point `{point_id}` is not declared by project `{project_id}`"),
        ));
    }
    validate_measurement(request.measurement.as_ref())?;
    validate_optional_text(request.note.as_deref(), MAX_OBSERVATION_NOTE_LEN, "note")?;
    if request.measurement.is_none()
        && request.photo_upload_id.is_none()
        && request.trace_ref.is_none()
        && request.note.as_deref().is_none_or(str::is_empty)
    {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "point observation requires measurement, photo_upload_id, trace_ref, or note",
        ));
    }
    if let Some(upload_id) = request.photo_upload_id.as_deref() {
        validate_identifier(upload_id, false)
            .map_err(|message| api_error(StatusCode::BAD_REQUEST, message))?;
        let upload_exists = state
            .physical_evidence
            .uploads(&project_id)
            .map_err(|message| api_error(StatusCode::INTERNAL_SERVER_ERROR, message))?
            .iter()
            .any(|upload| upload.upload_id == upload_id && upload.evidence_kind == "photo");
        if !upload_exists {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "photo_upload_id does not reference a current project photo upload",
            ));
        }
    }
    let trace_sha256 = if let Some(trace_ref) = request.trace_ref.as_deref() {
        let trace_path = state
            .physical_evidence
            .resolve_trace_ref(&project_id, trace_ref)
            .map_err(|message| api_error(StatusCode::BAD_REQUEST, message))?;
        Some(sha256_file(&trace_path).ok_or_else(|| {
            api_error(
                StatusCode::BAD_REQUEST,
                "trace_ref content cannot be hashed",
            )
        })?)
    } else {
        None
    };
    let source_commit = project_source_commit(&state.workspace_root, &project_id)?;
    let prior_digests = current_evidence_digests(&state.workspace_root, &project_id)
        .map_err(|message| api_error(StatusCode::BAD_REQUEST, message))?;
    let observed_at_ms = now_ms();
    let observation = PointObservation {
        schema_version: 1,
        observation_id: Uuid::new_v4().to_string(),
        project_id: project_id.clone(),
        point_id: point_id.clone(),
        status: request.status,
        measurement: request.measurement,
        photo_upload_id: request.photo_upload_id,
        trace_ref: request.trace_ref,
        trace_sha256,
        note: request.note.filter(|value| !value.trim().is_empty()),
        user,
        source_commit: source_commit.clone(),
        observed_at: timestamp(observed_at_ms),
        observed_at_ms,
        prior_evidence_digest_set_sha256: digest_set_sha256(&prior_digests),
        deep_link: delivery_deep_link(
            &project_id,
            None,
            None,
            Some("wiring_point"),
            Some(&point_id),
            Some(&source_commit),
        ),
    };
    state
        .physical_evidence
        .append_observation(&observation)
        .await
        .map_err(|message| api_error(StatusCode::INTERNAL_SERVER_ERROR, message))?;
    Ok((StatusCode::CREATED, Json(observation)))
}

async fn list_point_observations(
    State(state): State<Arc<AppState>>,
    AxumPath((project_id, point_id)): AxumPath<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    resolve_delivery_project_root(&state.workspace_root, &project_id)
        .map_err(|message| api_error(StatusCode::NOT_FOUND, message))?;
    validate_identifier(&point_id, true)
        .map_err(|message| api_error(StatusCode::BAD_REQUEST, message))?;
    let observations = state
        .physical_evidence
        .observations(&project_id)
        .map_err(|message| api_error(StatusCode::INTERNAL_SERVER_ERROR, message))?
        .into_iter()
        .filter(|observation| observation.point_id == point_id)
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "schema_version": 1,
        "project_id": project_id,
        "point_id": point_id,
        "observations": observations,
        "count": observations.len()
    })))
}

async fn get_physical_evidence(
    State(state): State<Arc<AppState>>,
    AxumPath(project_id): AxumPath<String>,
) -> Result<Json<Value>, ApiError> {
    resolve_delivery_project_root(&state.workspace_root, &project_id)
        .map_err(|message| api_error(StatusCode::NOT_FOUND, message))?;
    let observations = state
        .physical_evidence
        .observations(&project_id)
        .map_err(|message| api_error(StatusCode::INTERNAL_SERVER_ERROR, message))?;
    let uploads = state
        .physical_evidence
        .uploads(&project_id)
        .map_err(|message| api_error(StatusCode::INTERNAL_SERVER_ERROR, message))?;
    let point_checks = point_check_projection(&state.workspace_root, &project_id)?;
    Ok(Json(json!({
        "schema_version": 1,
        "project_id": project_id,
        "point_checks": point_checks,
        "observations": observations,
        "uploads": uploads,
        "provenance": {
            "observation_log": format!("out/web-delivery-evidence/{project_id}/point-observations.jsonl"),
            "upload_log": format!("out/web-delivery-evidence/{project_id}/uploads.jsonl")
        }
    })))
}

async fn get_hold_projection(
    State(state): State<Arc<AppState>>,
    AxumPath(project_id): AxumPath<String>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(hold_projection(&state, &project_id)?))
}

async fn get_release_projection(
    State(state): State<Arc<AppState>>,
    AxumPath(project_id): AxumPath<String>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(release_projection(&state, &project_id)?))
}

pub(crate) fn release_projection(
    state: &Arc<AppState>,
    project_id: &str,
) -> Result<Value, ApiError> {
    let holds = hold_projection(state, project_id)?;
    let hold_items = holds
        .get("holds")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let manifest = load_project_manifest(&state.workspace_root, &project_id)?;
    let delivery_status = manifest
        .get("delivery_status")
        .and_then(Value::as_str)
        .unwrap_or("not_recorded");
    let release_hold = hold_items
        .iter()
        .find(|hold| hold.get("hold_id").and_then(Value::as_str) == Some("release_approval"));
    let prerequisites = RELEASE_PREREQUISITES
        .iter()
        .map(|hold_id| {
            hold_items
                .iter()
                .find(|hold| hold.get("hold_id").and_then(Value::as_str) == Some(*hold_id))
                .cloned()
                .unwrap_or_else(|| {
                    json!({
                        "hold_id": hold_id,
                        "status": "missing",
                        "reason": "required release prerequisite is absent from the hold contract"
                    })
                })
        })
        .collect::<Vec<_>>();
    let blocked_prerequisites = prerequisites
        .iter()
        .filter(|hold| hold.get("status").and_then(Value::as_str) != Some("human_confirmed"))
        .cloned()
        .collect::<Vec<_>>();
    let release_confirmed = release_hold
        .and_then(|hold| hold.get("status"))
        .and_then(Value::as_str)
        == Some("human_confirmed");
    let delivery_status_current = matches!(delivery_status, "pass" | "current");
    let status = if !delivery_status_current || !blocked_prerequisites.is_empty() {
        "blocked"
    } else if release_confirmed {
        "release_approved"
    } else {
        "human_action_required"
    };
    Ok(json!({
        "schema_version": 1,
        "project_id": project_id,
        "status": status,
        "delivery_status": delivery_status,
        "delivery_status_gate": {
            "status": if delivery_status_current { "current" } else { "blocked" },
            "allowed_statuses": ["pass", "current"],
            "error_code": (!delivery_status_current).then_some("DELIVERY_STATUS_NOT_RELEASABLE")
        },
        "holds": hold_items,
        "prerequisites": prerequisites,
        "blocked_prerequisites": blocked_prerequisites,
        "release_signature": release_hold.and_then(|hold| hold.get("signature")).cloned(),
        "provenance": holds.get("provenance").cloned(),
        "deep_link": delivery_deep_link(
            &project_id,
            None,
            None,
            Some("release"),
            Some("release_approval"),
            project_source_commit(&state.workspace_root, &project_id).ok().as_deref()
        )
    }))
}

pub(crate) fn point_check_projection(
    workspace_root: &Path,
    project_id: &str,
) -> Result<Value, ApiError> {
    let points = authored_wiring_points(workspace_root, project_id)?;
    let store = PhysicalEvidenceStore::new(workspace_root);
    let observations = store
        .observations(project_id)
        .map_err(|message| api_error(StatusCode::INTERNAL_SERVER_ERROR, message))?;
    let mut latest = HashMap::<String, PointObservation>::new();
    for observation in observations {
        let replace = latest
            .get(&observation.point_id)
            .is_none_or(|current| current.observed_at_ms <= observation.observed_at_ms);
        if replace {
            latest.insert(observation.point_id.clone(), observation);
        }
    }
    let source_commit = project_source_commit(workspace_root, project_id)?;
    let projected = points
        .into_iter()
        .filter_map(|point| {
            let point_id = point_id_of(&point)?.to_string();
            let observation = latest.get(&point_id).cloned();
            let authored_status = point
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("human_action_required");
            let status = match observation.as_ref().map(|value| value.status) {
                Some(PointObservationStatus::Pass) => "observed",
                Some(PointObservationStatus::Fail | PointObservationStatus::Blocked) => "blocked",
                None => authored_status,
            };
            Some(json!({
                "point_id": point_id,
                "authored": point,
                "status": status,
                "evidence_state": if observation.is_some() { "observed" } else { "authored" },
                "responsibility_state": if matches!(observation.as_ref().map(|value| value.status), Some(PointObservationStatus::Pass)) { "human_confirmed" } else { "human_action_required" },
                "latest_observation": observation,
                "deep_link": delivery_deep_link(project_id, None, None, Some("wiring_point"), Some(&point_id), Some(&source_commit))
            }))
        })
        .collect::<Vec<_>>();
    let observed = projected
        .iter()
        .filter(|point| point.get("status").and_then(Value::as_str) == Some("observed"))
        .count();
    let blocked = projected
        .iter()
        .filter(|point| point.get("status").and_then(Value::as_str) == Some("blocked"))
        .count();
    Ok(json!({
        "summary": {
            "declared_points": projected.len(),
            "observed_points": observed,
            "blocked_points": blocked,
            "remaining_points": projected.len().saturating_sub(observed)
        },
        "points": projected
    }))
}

pub(crate) fn hold_projection(state: &Arc<AppState>, project_id: &str) -> Result<Value, ApiError> {
    let (manifest, hold_document, hold_path) =
        load_hold_document(&state.workspace_root, project_id)?;
    let source_commit = manifest
        .get("source_commit")
        .and_then(Value::as_str)
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "project source_commit is missing"))?;
    let digests = current_evidence_digests(&state.workspace_root, project_id)
        .map_err(|message| api_error(StatusCode::BAD_REQUEST, message))?;
    let signatures = state
        .signatures
        .list(project_id, &digests)
        .map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("signature storage failed: {error:?}"),
            )
        })?;
    let point_checks = point_check_projection(&state.workspace_root, project_id)?;
    let point_summary = point_checks
        .get("summary")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let holds = hold_document
        .get("holds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|hold| {
            let hold_id = hold.get("hold_id")?.as_str()?;
            let base_status = hold
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("human_action_required");
            let current_signature = signatures
                .iter()
                .rev()
                .find(|view| view.signature.hold_id == hold_id && !view.stale);
            let has_stale_signature = signatures
                .iter()
                .any(|view| view.signature.hold_id == hold_id && view.stale);
            let point_check_incomplete = hold_id == "point_check_completion"
                && point_summary
                    .get("remaining_points")
                    .and_then(Value::as_u64)
                    .unwrap_or(1)
                    > 0;
            let status = if base_status == "blocked" {
                "blocked"
            } else if point_check_incomplete {
                "human_action_required"
            } else if let Some(signature) = current_signature {
                match signature.signature.decision {
                    SignatureDecision::Approve => "human_confirmed",
                    SignatureDecision::Reject => "rejected",
                }
            } else if has_stale_signature {
                "stale"
            } else {
                base_status
            };
            Some(json!({
                "hold_id": hold_id,
                "required_role": hold.get("required_role").cloned(),
                "contract_status": base_status,
                "status": status,
                "reason": hold.get("reason").cloned(),
                "blocker_ids": hold.get("blocker_ids").cloned().unwrap_or_else(|| json!([])),
                "signature": current_signature,
                "stale_signature_present": has_stale_signature,
                "point_check_summary": (hold_id == "point_check_completion").then_some(point_summary.clone()),
                "deep_link": delivery_deep_link(project_id, None, None, Some("hold"), Some(hold_id), Some(source_commit))
            }))
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "schema_version": 1,
        "project_id": project_id,
        "source_commit": source_commit,
        "holds": holds,
        "current_evidence_digest_set_sha256": digest_set_sha256(&digests),
        "provenance": {
            "manifest": workspace_rel(&state.workspace_root, &resolve_delivery_project_root(&state.workspace_root, project_id).unwrap_or_default().join("delivery-project.json")),
            "hold_contract": workspace_rel(&state.workspace_root, &hold_path),
            "signature_store": format!("out/web-signatures/{project_id}/signatures.jsonl")
        }
    }))
}

fn authored_wiring_points(workspace_root: &Path, project_id: &str) -> Result<Vec<Value>, ApiError> {
    let project_root = resolve_delivery_project_root(workspace_root, project_id)
        .map_err(|message| api_error(StatusCode::NOT_FOUND, message))?;
    let manifest = load_project_manifest(workspace_root, project_id)?;
    let wiring_ref = manifest
        .pointer("/artifact_roots/wiring")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            api_error(
                StatusCode::CONFLICT,
                "project does not declare artifact_roots.wiring",
            )
        })?;
    let wiring_root =
        resolve_project_or_workspace_path(workspace_root, &project_root, wiring_ref, true)?;
    let mut files = Vec::new();
    collect_json_files(&wiring_root, 0, &mut files);
    let mut points = Vec::new();
    for file in files {
        let Ok(document) = read_json(&file) else {
            continue;
        };
        for field in ["points", "rows"] {
            if let Some(items) = document.get(field).and_then(Value::as_array) {
                points.extend(items.iter().cloned());
            }
        }
    }
    if points.is_empty() {
        return Err(api_error(
            StatusCode::CONFLICT,
            "wiring artifacts do not declare point-check rows",
        ));
    }
    Ok(points)
}

fn load_hold_document(
    workspace_root: &Path,
    project_id: &str,
) -> Result<(Value, Value, PathBuf), ApiError> {
    let project_root = resolve_delivery_project_root(workspace_root, project_id)
        .map_err(|message| api_error(StatusCode::NOT_FOUND, message))?;
    let manifest = load_project_manifest(workspace_root, project_id)?;
    let hold_ref = manifest
        .pointer("/fixtures/human_holds/fixture_ref")
        .and_then(Value::as_str)
        .unwrap_or("release/human-holds.json");
    let hold_path =
        resolve_project_or_workspace_path(workspace_root, &project_root, hold_ref, false)?;
    let hold_document =
        read_json(&hold_path).map_err(|message| api_error(StatusCode::BAD_REQUEST, message))?;
    Ok((manifest, hold_document, hold_path))
}

fn load_project_manifest(workspace_root: &Path, project_id: &str) -> Result<Value, ApiError> {
    let project_root = resolve_delivery_project_root(workspace_root, project_id)
        .map_err(|message| api_error(StatusCode::NOT_FOUND, message))?;
    read_json(&project_root.join("delivery-project.json"))
        .map_err(|message| api_error(StatusCode::BAD_REQUEST, message))
}

fn project_source_commit(workspace_root: &Path, project_id: &str) -> Result<String, ApiError> {
    load_project_manifest(workspace_root, project_id)?
        .get("source_commit")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| api_error(StatusCode::CONFLICT, "project source_commit is missing"))
}

async fn authenticated_user(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<AuthUser, ApiError> {
    let token = bearer_token(headers)
        .ok_or_else(|| api_error(StatusCode::UNAUTHORIZED, "authentication required"))?;
    state
        .auth
        .authenticate(token)
        .await
        .map_err(|_| api_error(StatusCode::UNAUTHORIZED, "session is not valid"))
}

fn resolve_project_or_workspace_path(
    workspace_root: &Path,
    project_root: &Path,
    raw: &str,
    require_dir: bool,
) -> Result<PathBuf, ApiError> {
    let relative =
        safe_relative_path(raw).map_err(|message| api_error(StatusCode::BAD_REQUEST, message))?;
    let workspace = workspace_root
        .canonicalize()
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    for candidate in [project_root.join(&relative), workspace.join(&relative)] {
        let Ok(canonical) = candidate.canonicalize() else {
            continue;
        };
        let kind_matches = if require_dir {
            canonical.is_dir()
        } else {
            canonical.is_file()
        };
        if canonical.starts_with(&workspace) && kind_matches {
            return Ok(canonical);
        }
    }
    Err(api_error(
        StatusCode::NOT_FOUND,
        format!("declared artifact `{raw}` does not exist"),
    ))
}

fn collect_json_files(root: &Path, depth: usize, output: &mut Vec<PathBuf>) {
    if depth > 4 || output.len() >= 256 {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            collect_json_files(&entry.path(), depth + 1, output);
        } else if kind.is_file()
            && entry.path().extension().and_then(|value| value.to_str()) == Some("json")
        {
            output.push(entry.path());
        }
    }
}

fn point_id_of(point: &Value) -> Option<&str> {
    point
        .get("point_id")
        .and_then(Value::as_str)
        .or_else(|| point.get("id").and_then(Value::as_str))
}

fn append_json_line_unlocked<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| err.to_string())?;
    serde_json::to_writer(&mut file, value).map_err(|err| err.to_string())?;
    file.write_all(b"\n").map_err(|err| err.to_string())?;
    file.flush().map_err(|err| err.to_string())
}

fn read_json_lines<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Vec<T>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(path).map_err(|err| err.to_string())?;
    let mut records = Vec::new();
    for line in BufReader::new(file).lines().take(MAX_STORED_RECORDS) {
        let line = line.map_err(|err| err.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        records.push(serde_json::from_str(&line).map_err(|err| err.to_string())?);
    }
    Ok(records)
}

fn read_json(path: &Path) -> Result<Value, String> {
    serde_json::from_slice(&fs::read(path).map_err(|err| err.to_string())?)
        .map_err(|err| err.to_string())
}

fn validate_measurement(measurement: Option<&Measurement>) -> Result<(), ApiError> {
    let Some(measurement) = measurement else {
        return Ok(());
    };
    if measurement.value.trim().is_empty() || measurement.value.len() > MAX_MEASUREMENT_FIELD_LEN {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "measurement.value is empty or too long",
        ));
    }
    for (name, value) in [
        ("measurement.unit", measurement.unit.as_deref()),
        (
            "measurement.instrument_id",
            measurement.instrument_id.as_deref(),
        ),
    ] {
        validate_optional_text(value, MAX_MEASUREMENT_FIELD_LEN, name)?;
    }
    Ok(())
}

fn validate_optional_text(value: Option<&str>, max_len: usize, name: &str) -> Result<(), ApiError> {
    if value.is_some_and(|value| value.len() > max_len || value.contains('\0')) {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            format!("{name} exceeds its limit or contains invalid characters"),
        ));
    }
    Ok(())
}

fn validate_identifier(value: &str, allow_point_punctuation: bool) -> Result<(), String> {
    let valid = !value.is_empty()
        && value.len() <= 160
        && value.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(ch, '_' | '-' | '.')
                || (allow_point_punctuation && matches!(ch, '[' | ']' | ':' | ','))
        });
    valid
        .then_some(())
        .ok_or_else(|| "identifier contains unsupported characters".to_string())
}

fn validate_filename(value: &str) -> Result<String, String> {
    if value.is_empty()
        || value.len() > MAX_UPLOAD_FILENAME_LEN
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
        || value.starts_with('.')
    {
        return Err("filename contains unsupported characters".to_string());
    }
    Ok(value.to_string())
}

fn safe_relative_path(raw: &str) -> Result<PathBuf, String> {
    if raw.is_empty() || raw.len() > 4096 || raw.contains(['%', '\0', '\\']) || raw.starts_with('/')
    {
        return Err("path must be normalized and workspace-relative".to_string());
    }
    let path = PathBuf::from(raw);
    if path.is_absolute()
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err("path must be normalized and workspace-relative".to_string());
    }
    Ok(path)
}

fn required_header(headers: &HeaderMap, name: &str) -> Result<String, ApiError> {
    optional_header(headers, name)?.ok_or_else(|| {
        api_error(
            StatusCode::BAD_REQUEST,
            format!("required header `{name}` is missing"),
        )
    })
}

fn optional_header(headers: &HeaderMap, name: &str) -> Result<Option<String>, ApiError> {
    headers
        .get(name)
        .map(|value| {
            value.to_str().map(str::to_string).map_err(|_| {
                api_error(
                    StatusCode::BAD_REQUEST,
                    format!("header `{name}` is invalid"),
                )
            })
        })
        .transpose()
}

fn digest_set_sha256(digests: &BTreeMap<String, String>) -> String {
    let mut hasher = Sha256::new();
    for (path, digest) in digests {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(digest.as_bytes());
        hasher.update([b'\n']);
    }
    hex_digest(hasher.finalize())
}

fn sha256_bytes(bytes: &[u8]) -> String {
    hex_digest(Sha256::digest(bytes))
}

fn sha256_file(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Some(hex_digest(hasher.finalize()))
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn workspace_rel(workspace_root: &Path, path: &Path) -> String {
    let root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    path.strip_prefix(root)
        .unwrap_or(&path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn timestamp(ms: u64) -> String {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH)
        .format(&Rfc3339)
        .unwrap_or_else(|_| ms.to_string())
}

fn api_error(status: StatusCode, message: impl Into<String>) -> ApiError {
    let message = message.into();
    (
        status,
        Json(json!({
            "error": message,
            "message": message
        })),
    )
}
