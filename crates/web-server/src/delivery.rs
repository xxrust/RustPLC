use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Component, Path as StdPath, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use crate::AppState;

const MAX_DISCOVERY_DEPTH: usize = 10;
const MAX_DISCOVERY_ENTRIES: usize = 50_000;
const MAX_PROJECT_ARTIFACTS: usize = 4_000;
const MAX_DOCUMENT_BYTES: u64 = 8 * 1024 * 1024;

type ApiError = (StatusCode, Json<Value>);

#[derive(Clone)]
struct DeliveryProject {
    workspace_root: PathBuf,
    project_root: PathBuf,
    manifest_path: PathBuf,
    manifest: Value,
    companion_manifest: Option<(PathBuf, Value)>,
    project_id: String,
    delivery_layer: String,
    source_entry: Option<String>,
    system_contract: Option<String>,
    source_commit: Option<String>,
}

#[derive(Default)]
struct DeliveryCatalog {
    projects: Vec<DeliveryProject>,
    registry_problems: Vec<Value>,
}

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/delivery-projects", get(list_delivery_projects))
        .route("/delivery-projects/{id}", get(get_delivery_project))
        .route("/delivery-projects/{id}/runs", get(list_delivery_runs))
        .route(
            "/delivery-projects/{id}/runs/{run_id}",
            get(get_delivery_run),
        )
        .route("/delivery-projects/{id}/wiring", get(get_delivery_wiring))
        .route(
            "/delivery-projects/{id}/verification",
            get(get_delivery_verification),
        )
        .route(
            "/delivery-projects/{id}/evidence",
            get(get_delivery_evidence),
        )
        .route("/workspace/problems", get(get_workspace_problems))
        .route("/workspace/tests", get(get_workspace_tests))
}

pub(crate) fn resolve_delivery_project_root(
    workspace_root: &StdPath,
    project_id: &str,
) -> Result<PathBuf, String> {
    if !is_safe_identifier(project_id) {
        return Err("invalid delivery project id".to_string());
    }
    let catalog = load_catalog(workspace_root);
    let project = catalog
        .projects
        .iter()
        .find(|project| project.project_id == project_id)
        .ok_or_else(|| format!("delivery project `{project_id}` not found"))?;
    let root = project
        .project_root
        .canonicalize()
        .map_err(|err| format!("delivery project root is unavailable: {err}"))?;
    if !root.starts_with(&project.workspace_root) {
        return Err("delivery project root escapes the workspace".to_string());
    }
    Ok(root)
}

pub(crate) fn current_evidence_digests(
    workspace_root: &StdPath,
    project_id: &str,
) -> Result<BTreeMap<String, String>, String> {
    if !is_safe_identifier(project_id) {
        return Err("invalid delivery project id".to_string());
    }
    let catalog = load_catalog(workspace_root);
    let project = catalog
        .projects
        .iter()
        .find(|project| project.project_id == project_id)
        .ok_or_else(|| format!("delivery project `{project_id}` not found"))?;
    let mut digests = BTreeMap::new();
    for path in collect_project_artifacts(project) {
        if let Some(digest) = sha256_file(&path) {
            digests.insert(workspace_rel(&project.workspace_root, &path), digest);
        }
    }
    Ok(digests)
}

pub(crate) fn delivery_deep_link(
    project_id: &str,
    run_id: Option<&str>,
    artifact: Option<&str>,
    semantic_object_kind: Option<&str>,
    semantic_object_id: Option<&str>,
    source_commit: Option<&str>,
) -> Value {
    let source = match (artifact, source_commit) {
        (None, None) => Value::Null,
        _ => json!({
            "artifact": artifact,
            "commit": source_commit,
        }),
    };
    let object = match (semantic_object_kind, semantic_object_id) {
        (Some(kind), Some(id)) => json!({ "kind": kind, "id": id }),
        _ => Value::Null,
    };
    json!({
        "schema_version": 1,
        "kind": "delivery_deep_link",
        "project_id": project_id,
        "run_id": run_id,
        "artifact": artifact,
        "source_commit": source_commit,
        "semantic_object": object,
        "source": source,
        "object": object,
    })
}

async fn list_delivery_projects(State(state): State<Arc<AppState>>) -> Json<Value> {
    let catalog = load_catalog(&state.workspace_root);
    let projects = catalog
        .projects
        .iter()
        .map(project_summary)
        .collect::<Vec<_>>();
    Json(json!({
        "schema_version": 1,
        "projects": projects,
        "registry_problems": catalog.registry_problems,
        "partial": !catalog.registry_problems.is_empty(),
        "provenance_policy": "workspace manifests and recorded harness artifacts only"
    }))
}

async fn get_delivery_project(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let catalog = load_catalog(&state.workspace_root);
    let project = find_project(&catalog, &id)?;
    let mut payload = project_summary(project);
    let object = payload
        .as_object_mut()
        .expect("project summary is always an object");
    object.insert("manifest_document".to_string(), project.manifest.clone());
    object.insert(
        "companion_manifest_document".to_string(),
        project
            .companion_manifest
            .as_ref()
            .map(|(_, value)| value.clone())
            .unwrap_or(Value::Null),
    );
    object.insert(
        "required_artifacts".to_string(),
        required_artifact_status(project),
    );
    let hold_document = project_human_hold_document(project);
    let dynamic_holds = crate::physical_evidence::hold_projection(&state, &id).ok();
    let dynamic_release = crate::physical_evidence::release_projection(&state, &id).ok();
    object.insert(
        "human_holds".to_string(),
        dynamic_holds
            .as_ref()
            .or(hold_document.as_ref())
            .map(normalized_human_holds)
            .unwrap_or_else(|| Value::Array(Vec::new())),
    );
    object.insert(
        "release_verdict".to_string(),
        dynamic_release
            .as_ref()
            .and_then(|document| document.get("status"))
            .cloned()
            .or_else(|| {
                hold_document
                    .as_ref()
                    .and_then(|document| document.get("release_status"))
                    .cloned()
            })
            .unwrap_or(Value::Null),
    );
    object.insert(
        "hold_projection".to_string(),
        dynamic_holds.unwrap_or(Value::Null),
    );
    object.insert(
        "release_projection".to_string(),
        dynamic_release.unwrap_or(Value::Null),
    );
    object.insert(
        "capabilities".to_string(),
        json!({
            "read_evidence": { "status": "available" },
            "hold_signing": {
                "status": "available",
                "authentication": "attributable_session",
                "signature_storage": "append_only_jsonl"
            }
        }),
    );
    Ok(Json(payload))
}

async fn list_delivery_runs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let catalog = load_catalog(&state.workspace_root);
    let project = find_project(&catalog, &id)?;
    let runs = collect_run_roots(project)
        .into_iter()
        .map(|root| run_summary(project, &root))
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "schema_version": 1,
        "project_id": project.project_id,
        "runs": runs,
        "provenance": manifest_provenance(project)
    })))
}

async fn get_delivery_run(
    State(state): State<Arc<AppState>>,
    Path((id, run_id)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let catalog = load_catalog(&state.workspace_root);
    let project = find_project(&catalog, &id)?;
    if !is_safe_identifier(&run_id) {
        return Err(bad_request("invalid run id"));
    }
    let run_root = collect_run_roots(project)
        .into_iter()
        .find(|root| run_identifier(project, root) == run_id)
        .ok_or_else(|| not_found(format!("delivery run `{run_id}` not found")))?;
    let mut summary = run_summary(project, &run_root);
    let object = summary
        .as_object_mut()
        .expect("run summary is always an object");
    let documents = run_documents(project, &run_root);
    object.insert("documents".to_string(), Value::Object(documents));
    object.insert(
        "artifacts".to_string(),
        Value::Array(
            collect_files(&run_root, MAX_PROJECT_ARTIFACTS)
                .into_iter()
                .filter_map(|path| artifact_descriptor(project, &path, "derived"))
                .collect(),
        ),
    );
    Ok(Json(summary))
}

async fn get_delivery_wiring(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let catalog = load_catalog(&state.workspace_root);
    let project = find_project(&catalog, &id)?;
    let roots = declared_artifact_roots(project, "wiring");
    let mut documents = Vec::new();
    for root in roots {
        for path in collect_files(&root, 500) {
            let descriptor = artifact_descriptor(project, &path, "derived");
            let document = read_structured_document(&path).ok();
            documents.push(json!({
                "artifact": descriptor,
                "document": document
            }));
        }
    }
    let status = if documents.is_empty() {
        evidence_status(
            "blocked",
            &project.manifest_path,
            project,
            Some("WIRING_ARTIFACT_MISSING"),
        )
    } else {
        evidence_status(
            "derived",
            &project.manifest_path,
            project,
            Some("wiring artifact root declared by project manifest"),
        )
    };
    let authored_points = documents
        .iter()
        .filter_map(|entry| entry.get("document"))
        .filter_map(|document| {
            document
                .get("points")
                .and_then(Value::as_array)
                .or_else(|| document.get("rows").and_then(Value::as_array))
        })
        .flatten()
        .map(normalized_wiring_point)
        .collect::<Vec<_>>();
    let point_check_projection = crate::physical_evidence::point_check_projection(
        &state.workspace_root,
        &project.project_id,
    )
    .unwrap_or_else(|(status, Json(error))| {
        json!({
            "status": "blocked",
            "http_status": status.as_u16(),
            "error": error
        })
    });
    let points = point_check_projection
        .get("points")
        .and_then(Value::as_array)
        .map(|points| {
            points
                .iter()
                .filter_map(|projected| {
                    let mut authored = projected.get("authored")?.clone();
                    authored["status"] = projected
                        .get("status")
                        .cloned()
                        .unwrap_or_else(|| Value::String("human_action_required".to_string()));
                    let mut normalized = normalized_wiring_point(&authored);
                    if let Some(object) = normalized.as_object_mut() {
                        object.insert(
                            "latest_observation".to_string(),
                            projected
                                .get("latest_observation")
                                .cloned()
                                .unwrap_or(Value::Null),
                        );
                        object.insert(
                            "deep_link".to_string(),
                            projected.get("deep_link").cloned().unwrap_or(Value::Null),
                        );
                    }
                    Some(normalized)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or(authored_points);
    Ok(Json(json!({
        "schema_version": 1,
        "project_id": project.project_id,
        "status": status,
        "points": points,
        "point_check_projection": point_check_projection,
        "documents": documents,
        "boundary": "Wiring rows are returned from authored or generated wiring artifacts; the server does not derive PLC I/O semantics."
    })))
}

async fn get_delivery_verification(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let catalog = load_catalog(&state.workspace_root);
    let project = find_project(&catalog, &id)?;
    let mut reports = Vec::new();
    for run_root in collect_run_roots(project) {
        let run_id = run_identifier(project, &run_root);
        for path in verification_report_paths(&run_root) {
            let Ok(document) = read_structured_document(&path) else {
                continue;
            };
            reports.push(json!({
                "run_id": run_id,
                "artifact": artifact_descriptor(project, &path, "derived"),
                "reported_status": reported_status(&document),
                "stages": extract_verification_stages(&document),
                "document": document
            }));
        }
    }
    let state_name = if reports.is_empty() {
        "blocked"
    } else {
        "derived"
    };
    let mut normalized_stages = BTreeMap::<String, Value>::new();
    for report in &reports {
        let report_artifact = report.get("artifact");
        let run_id = report.get("run_id").cloned().unwrap_or(Value::Null);
        for stage in report
            .get("stages")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(stage_name) = stage.get("stage").and_then(Value::as_str) else {
                continue;
            };
            let Some(stage_key) = compiler_stage_key(stage_name) else {
                continue;
            };
            normalized_stages.insert(
                stage_key,
                normalized_verification_stage(project, &run_id, stage, report_artifact),
            );
        }
    }
    Ok(Json(json!({
        "schema_version": 1,
        "project_id": project.project_id,
        "status": evidence_status(
            state_name,
            &project.manifest_path,
            project,
            reports.is_empty().then_some("VERIFICATION_ARTIFACT_MISSING")
        ),
        "stages": normalized_stages.into_values().collect::<Vec<_>>(),
        "reports": reports,
        "boundary": "Stage verdicts are copied from compiler-owned reports without re-evaluating PLC semantics."
    })))
}

async fn get_delivery_evidence(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let catalog = load_catalog(&state.workspace_root);
    let project = find_project(&catalog, &id)?;
    let artifacts = collect_project_artifacts(project)
        .into_iter()
        .filter_map(|path| {
            let state = if path == project.manifest_path {
                "authored"
            } else {
                "derived"
            };
            artifact_descriptor(project, &path, state)
        })
        .collect::<Vec<_>>();
    let evidence = artifacts
        .iter()
        .filter_map(|artifact| normalized_evidence_record(project, artifact))
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "schema_version": 1,
        "project_id": project.project_id,
        "evidence": evidence,
        "artifacts": artifacts,
        "artifact_count": artifacts.len(),
        "provenance": manifest_provenance(project)
    })))
}

async fn get_workspace_problems(State(state): State<Arc<AppState>>) -> Json<Value> {
    let catalog = load_catalog(&state.workspace_root);
    let mut problems = catalog.registry_problems.clone();
    for project in &catalog.projects {
        problems.extend(project_problems(project));
    }
    for problem in &mut problems {
        attach_problem_deep_link(&catalog, problem);
    }
    Json(json!({
        "schema_version": 1,
        "problems": problems,
        "count": problems.len(),
        "partial": !catalog.registry_problems.is_empty()
    }))
}

async fn get_workspace_tests(State(state): State<Arc<AppState>>) -> Json<Value> {
    let catalog = load_catalog(&state.workspace_root);
    let mut tests = Vec::new();
    let mut sources = Vec::new();
    let mut seen = HashSet::new();
    for project in &catalog.projects {
        let local_runs = collect_run_roots(project);
        let local_start = tests.len();
        let mut local_freshness = Vec::new();
        for run_root in &local_runs {
            let run_id = run_identifier(project, &run_root);
            let documents = run_documents(project, &run_root);
            let freshness = run_freshness(project, &run_root, &documents);
            local_freshness.push(json!({
                "run_id": run_id,
                "freshness": freshness,
            }));
            for (source, document) in documents {
                let source_artifact = run_document_path(&run_root, &source);
                collect_test_records(
                    project,
                    &run_id,
                    &run_root,
                    &source,
                    &document,
                    "local",
                    &freshness,
                    source_artifact.as_deref(),
                    &mut seen,
                    &mut tests,
                );
            }
        }
        sources.push(json!({
            "project_id": project.project_id,
            "execution_source": "local",
            "status": if local_runs.is_empty() { "unavailable" } else { "available" },
            "test_count": tests.len().saturating_sub(local_start),
            "freshness": if local_runs.is_empty() {
                json!({
                    "state": "unavailable",
                    "error_code": "LOCAL_TEST_EVIDENCE_UNAVAILABLE",
                    "reason": "no local run artifacts were discovered"
                })
            } else {
                json!({ "state": aggregate_freshness_state(&local_freshness), "runs": local_freshness })
            }
        }));

        let ci_roots = declared_artifact_roots(project, "ci");
        let ci_start = tests.len();
        let mut ci_documents = 0usize;
        let mut ci_freshness = Vec::new();
        for ci_root in &ci_roots {
            for path in collect_files(ci_root, MAX_PROJECT_ARTIFACTS) {
                if path.extension().and_then(|value| value.to_str()) != Some("json") {
                    continue;
                }
                let Ok(document) = read_structured_document(&path) else {
                    continue;
                };
                if document.get("steps").and_then(Value::as_array).is_none()
                    && document.get("stages").and_then(Value::as_array).is_none()
                {
                    continue;
                }
                ci_documents = ci_documents.saturating_add(1);
                let run_id = string_at(&document, &["run_id"])
                    .or_else(|| string_at(&document, &["harness_execution_id"]))
                    .unwrap_or_else(|| {
                        path.parent()
                            .and_then(StdPath::file_name)
                            .and_then(|value| value.to_str())
                            .unwrap_or("ci")
                            .to_string()
                    });
                let source = workspace_rel(&project.workspace_root, &path);
                let freshness = source_document_freshness(project, &document, &path);
                ci_freshness.push(json!({
                    "run_id": run_id,
                    "freshness": freshness,
                }));
                collect_test_records(
                    project,
                    &run_id,
                    ci_root,
                    &source,
                    &document,
                    "ci",
                    &freshness,
                    Some(&path),
                    &mut seen,
                    &mut tests,
                );
            }
        }
        let ci_available = !ci_roots.is_empty() && ci_documents > 0;
        sources.push(json!({
            "project_id": project.project_id,
            "execution_source": "ci",
            "status": if ci_available { "available" } else { "unavailable" },
            "test_count": tests.len().saturating_sub(ci_start),
            "freshness": if ci_available {
                json!({
                    "state": aggregate_freshness_state(&ci_freshness),
                    "document_count": ci_documents,
                    "runs": ci_freshness
                })
            } else {
                json!({
                    "state": "unavailable",
                    "error_code": "CI_TEST_EVIDENCE_UNAVAILABLE",
                    "reason": if ci_roots.is_empty() {
                        "artifact_roots.ci is not declared"
                    } else {
                        "artifact_roots.ci contains no shared-schema test result"
                    }
                })
            }
        }));
    }
    Json(json!({
        "schema_version": 1,
        "tests": tests,
        "count": tests.len(),
        "sources": sources,
        "partial": !catalog.registry_problems.is_empty()
        ,"boundary": "Reported local and CI test states are copied from recorded artifacts without re-evaluating compiler semantics."
    }))
}

fn load_catalog(workspace_root: &StdPath) -> DeliveryCatalog {
    let workspace_root = match workspace_root.canonicalize() {
        Ok(root) => root,
        Err(err) => {
            return DeliveryCatalog {
                projects: Vec::new(),
                registry_problems: vec![json!({
                    "code": "WORKSPACE_ROOT_UNAVAILABLE",
                    "severity": "blocked",
                    "message": err.to_string()
                })],
            }
        }
    };
    let names = [
        "delivery-project.json",
        "delivery-project.manifest.json",
        "artifact-manifest.json",
        "delivery-project.toml",
        "delivery-project.manifest.toml",
    ];
    let mut manifest_paths = Vec::new();
    let mut visited = 0usize;
    for relative in ["delivery-projects", "projects", "out"] {
        let root = workspace_root.join(relative);
        if root.is_dir() {
            discover_named_files(&root, 0, &names, &mut visited, &mut manifest_paths);
        }
    }
    manifest_paths.sort();
    manifest_paths.dedup();

    let mut catalog = DeliveryCatalog::default();
    let mut project_ids = BTreeSet::new();
    for manifest_path in manifest_paths {
        let manifest = match read_structured_document(&manifest_path) {
            Ok(value) => value,
            Err(message) => {
                catalog.registry_problems.push(json!({
                    "code": "DELIVERY_MANIFEST_INVALID",
                    "severity": "blocked",
                    "message": message,
                    "artifact": workspace_rel(&workspace_root, &manifest_path)
                }));
                continue;
            }
        };
        let Some(project_id) = string_at(&manifest, &["project_id"]) else {
            continue;
        };
        if !is_safe_identifier(&project_id) {
            catalog.registry_problems.push(json!({
                "code": "DELIVERY_PROJECT_ID_INVALID",
                "severity": "blocked",
                "message": format!("manifest contains invalid project_id `{project_id}`"),
                "artifact": workspace_rel(&workspace_root, &manifest_path)
            }));
            continue;
        }
        if !project_ids.insert(project_id.clone()) {
            catalog.registry_problems.push(json!({
                "code": "DELIVERY_PROJECT_ID_DUPLICATE",
                "severity": "blocked",
                "project_id": project_id,
                "artifact": workspace_rel(&workspace_root, &manifest_path)
            }));
            continue;
        }

        let project_root = infer_project_root(&workspace_root, &manifest_path, &manifest);
        let companion_path = project_root.join("manifest.json");
        let companion_manifest = if companion_path != manifest_path && companion_path.is_file() {
            read_structured_document(&companion_path)
                .ok()
                .map(|value| (companion_path, value))
        } else {
            None
        };
        let delivery_layer =
            string_at(&manifest, &["delivery_layer"]).unwrap_or_else(|| "unspecified".to_string());
        let source_entry = string_at(&manifest, &["source_entry"]);
        let system_contract = string_at(&manifest, &["system_contract"])
            .or_else(|| string_at(&manifest, &["authoritative_system_contract"]));
        let source_commit = string_at(&manifest, &["source_commit"]).or_else(|| {
            companion_manifest
                .as_ref()
                .and_then(|(_, value)| string_at(value, &["git_head"]))
        });
        catalog.projects.push(DeliveryProject {
            workspace_root: workspace_root.clone(),
            project_root,
            manifest_path,
            manifest,
            companion_manifest,
            project_id,
            delivery_layer,
            source_entry,
            system_contract,
            source_commit,
        });
    }
    catalog.projects.sort_by(|left, right| {
        manifest_priority(left)
            .cmp(&manifest_priority(right))
            .then_with(|| left.project_id.cmp(&right.project_id))
    });
    catalog
}

fn manifest_priority(project: &DeliveryProject) -> u8 {
    match project
        .manifest_path
        .file_name()
        .and_then(|value| value.to_str())
    {
        Some("delivery-project.json") => 0,
        _ => 1,
    }
}

fn find_project<'a>(
    catalog: &'a DeliveryCatalog,
    id: &str,
) -> Result<&'a DeliveryProject, ApiError> {
    if !is_safe_identifier(id) {
        return Err(bad_request("invalid delivery project id"));
    }
    catalog
        .projects
        .iter()
        .find(|project| project.project_id == id)
        .ok_or_else(|| not_found(format!("delivery project `{id}` not found")))
}

fn project_summary(project: &DeliveryProject) -> Value {
    let runs = collect_run_roots(project);
    let latest = runs.first().map(|root| run_summary(project, root));
    let explicit_status = string_at(&project.manifest, &["delivery_status"])
        .or_else(|| {
            project
                .companion_manifest
                .as_ref()
                .and_then(|(_, value)| string_at(value, &["status"]))
        })
        .or_else(|| {
            latest
                .as_ref()
                .and_then(|run| run.pointer("/evidence_status/state"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "authored".to_string());
    let missing = required_artifact_status(project)
        .get("missing")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let state = if missing > 0 && explicit_status != "blocked" {
        "blocked".to_string()
    } else {
        explicit_status
    };
    let title = string_at(&project.manifest, &["title"])
        .or_else(|| string_at(&project.manifest, &["name"]))
        .unwrap_or_else(|| project.project_id.clone());
    let stale = latest
        .as_ref()
        .and_then(|run| run.pointer("/freshness/state"))
        .and_then(Value::as_str)
        == Some("stale");
    let reported_blockers = project
        .manifest
        .pointer("/evidence_summary/acceptance_blocked")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    json!({
        "schema_version": 1,
        "project_id": project.project_id,
        "name": title,
        "delivery_layer": project.delivery_layer,
        "source_commit": project.source_commit,
        "source_entry": project.source_entry,
        "system_contract": project.system_contract,
        "project_root": workspace_rel(&project.workspace_root, &project.project_root),
        "manifest": workspace_rel(&project.workspace_root, &project.manifest_path),
        "evidence_status": evidence_status(
            &state,
            &project.manifest_path,
            project,
            (missing > 0).then_some("required artifact missing")
        ),
        "status": if stale { "stale" } else { state.as_str() },
        "responsibility_state": if state == "blocked" { "human_action_required" } else { "agent_complete" },
        "stale": stale,
        "blocker_count": reported_blockers.saturating_add(missing as u64),
        "latest_run": latest,
        "run_count": runs.len(),
        "missing_required_artifact_count": missing,
        "workspace_git": workspace_git_status(&project.workspace_root),
        "provenance": manifest_provenance(project)
    })
}

fn required_artifact_status(project: &DeliveryProject) -> Value {
    let definition_root = project
        .manifest_path
        .parent()
        .unwrap_or(&project.project_root);
    let implementation_root = project
        .source_entry
        .as_deref()
        .and_then(|source| resolve_declared_path(project, source))
        .and_then(|path| path.parent().map(StdPath::to_path_buf))
        .unwrap_or_else(|| project.project_root.clone());
    let mut present = Vec::new();
    let mut missing = Vec::new();
    for (field, raw) in [
        ("source_entry", project.source_entry.as_deref()),
        ("system_contract", project.system_contract.as_deref()),
    ] {
        let exists = raw
            .and_then(|value| resolve_declared_path(project, value))
            .is_some();
        let record = json!({ "field": field, "path": raw });
        if exists {
            present.push(record);
        } else {
            missing.push(record);
        }
    }
    for (field, base) in [
        ("required_definition_files", definition_root),
        (
            "required_implementation_files",
            implementation_root.as_path(),
        ),
    ] {
        for raw in string_array_at(&project.manifest, field) {
            let record = json!({ "field": field, "path": raw });
            let exists = resolve_from_base(project, base, &raw).is_some();
            if exists {
                present.push(record);
            } else {
                missing.push(record);
            }
        }
    }
    if let Some(roots) = project
        .manifest
        .get("artifact_roots")
        .and_then(Value::as_object)
    {
        for (key, value) in roots {
            let raw = value.as_str();
            let exists = raw
                .and_then(|value| resolve_declared_dir(project, value))
                .is_some();
            let record = json!({
                "field": format!("artifact_roots.{key}"),
                "path": raw
            });
            if exists {
                present.push(record);
            } else {
                missing.push(record);
            }
        }
    }
    json!({
        "present": present,
        "missing": missing,
        "status": if missing.is_empty() { "complete" } else { "blocked" }
    })
}

fn collect_run_roots(project: &DeliveryProject) -> Vec<PathBuf> {
    let mut roots = BTreeSet::new();
    for root in declared_artifact_roots(project, "agent_runs") {
        add_run_directories(&root, &mut roots);
    }
    let conventional = project.project_root.join("harness-runs");
    if conventional.is_dir() && is_within_workspace(project, &conventional) {
        add_run_directories(&conventional, &mut roots);
    }
    if let Some((_, companion)) = &project.companion_manifest {
        if let Some(result_raw) = companion
            .pointer("/artifacts/result")
            .and_then(Value::as_str)
        {
            if let Some(result_path) = resolve_declared_path(project, result_raw) {
                if let Ok(result) = read_structured_document(&result_path) {
                    if let Some(root_raw) = result.get("artifact_root").and_then(Value::as_str) {
                        if let Some(root) = resolve_declared_dir(project, root_raw) {
                            roots.insert(root);
                        }
                    }
                }
            }
        }
    }
    let mut roots = roots.into_iter().collect::<Vec<_>>();
    roots.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
    roots
}

fn add_run_directories(root: &StdPath, output: &mut BTreeSet<PathBuf>) {
    if root.join("input-manifest.json").is_file()
        || root
            .join("project-check/project_check_report.json")
            .is_file()
        || root.join("compile/verification_report.json").is_file()
    {
        output.insert(root.to_path_buf());
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() && !file_type.is_symlink() {
            output.insert(entry.path());
        }
    }
}

fn run_summary(project: &DeliveryProject, run_root: &StdPath) -> Value {
    let documents = run_documents(project, run_root);
    let result = documents.get("result");
    let project_check = documents.get("project_check");
    let provenance = documents.get("provenance");
    let input_manifest = documents.get("input_manifest");
    let (reported, status_source) = result
        .and_then(|value| {
            value
                .pointer("/status/delivery")
                .and_then(Value::as_str)
                .or_else(|| {
                    value
                        .pointer("/status/harness_execution")
                        .and_then(Value::as_str)
                })
                .or_else(|| value.get("status").and_then(Value::as_str))
                .map(|status| (status.to_string(), "result"))
        })
        .or_else(|| {
            project_check
                .and_then(|value| value.get("status").and_then(Value::as_str))
                .map(|status| (status.to_string(), "project_check"))
        })
        .or_else(|| {
            provenance
                .and_then(|value| value.get("unattended_verdict").and_then(Value::as_str))
                .map(|status| (status.to_string(), "provenance"))
        })
        .unwrap_or_else(|| ("not_recorded".to_string(), "run_directory"));
    let freshness = run_freshness(project, run_root, &documents);
    let mut evidence_state = map_reported_status(&reported).to_string();
    if freshness.get("state").and_then(Value::as_str) == Some("stale") {
        evidence_state = "stale".to_string();
    }
    let status_path = match status_source {
        "result" => run_result_path(project, run_root),
        "project_check" => Some(run_root.join("project-check/project_check_report.json")),
        "provenance" => Some(run_root.join("provenance.json")),
        _ => None,
    };
    json!({
        "schema_version": 1,
        "run_id": run_identifier(project, run_root),
        "run_root": workspace_rel(&project.workspace_root, run_root),
        "reported_status": reported,
        "evidence_status": status_path
            .as_deref()
            .map(|path| evidence_status(&evidence_state, path, project, Some(status_source)))
            .unwrap_or_else(|| evidence_status("blocked", &project.manifest_path, project, Some("RUN_STATUS_NOT_RECORDED"))),
        "freshness": freshness,
        "started_at": result.or(provenance).and_then(|value| value.get("started_at_utc")).cloned(),
        "completed_at": result.or(provenance).and_then(|value| value.get("completed_at_utc")).cloned(),
        "elapsed_ms": result.or(provenance).and_then(|value| value.get("elapsed_ms")).cloned(),
        "source_commit": result
            .and_then(|value| value.get("git_head"))
            .cloned()
            .or_else(|| project.source_commit.clone().map(Value::String)),
        "model": provenance
            .and_then(|value| value.pointer("/models/0/model"))
            .cloned(),
        "unattended_verdict": provenance
            .and_then(|value| value.get("unattended_verdict"))
            .cloned(),
        "unattended_reason": provenance
            .and_then(|value| value.get("unattended_reason"))
            .cloned(),
        "input_manifest_digest": input_manifest
            .and_then(|value| value.pointer("/digest/value"))
            .cloned(),
        "git": run_git_status(project, result, provenance),
        "artifact_count": collect_files(run_root, MAX_PROJECT_ARTIFACTS).len()
    })
}

fn run_identifier(project: &DeliveryProject, run_root: &StdPath) -> String {
    run_result_path(project, run_root)
        .and_then(|path| read_structured_document(&path).ok())
        .and_then(|value| {
            string_at(&value, &["harness_execution_id"]).or_else(|| string_at(&value, &["run_id"]))
        })
        .unwrap_or_else(|| {
            run_root
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("unknown")
                .to_string()
        })
}

fn run_documents(project: &DeliveryProject, run_root: &StdPath) -> Map<String, Value> {
    let mut documents = Map::new();
    let candidates = [
        ("input_manifest", run_root.join("input-manifest.json")),
        ("provenance", run_root.join("provenance.json")),
        ("anomalies", run_root.join("anomalies.json")),
        ("corrections", run_root.join("corrections.json")),
        ("compiler_stages", run_root.join("compiler-stages.json")),
        (
            "project_check",
            run_root.join("project-check/project_check_report.json"),
        ),
        (
            "verification",
            run_root.join("compile/verification_report.json"),
        ),
    ];
    for (name, path) in candidates {
        if let Ok(value) = read_structured_document(&path) {
            documents.insert(name.to_string(), value);
        }
    }
    if let Some(result_path) = run_result_path(project, run_root) {
        if let Ok(value) = read_structured_document(&result_path) {
            documents.insert("result".to_string(), value);
        }
    }
    documents
}

fn run_result_path(project: &DeliveryProject, run_root: &StdPath) -> Option<PathBuf> {
    let local = run_root.join("result.json");
    if local.is_file() {
        return Some(local);
    }
    let root_result = project.project_root.join("result.json");
    let result = read_structured_document(&root_result).ok()?;
    let artifact_root = result.get("artifact_root")?.as_str()?;
    let resolved = resolve_declared_dir(project, artifact_root)?;
    same_path(&resolved, run_root).then_some(root_result)
}

fn run_freshness(
    project: &DeliveryProject,
    run_root: &StdPath,
    documents: &Map<String, Value>,
) -> Value {
    let Some(result) = documents.get("result") else {
        if let Some(provenance) = documents.get("provenance") {
            let reported = provenance
                .pointer("/freshness/status")
                .and_then(Value::as_str)
                .unwrap_or("not_recorded");
            let state = match reported {
                "same_run" | "input_snapshot" | "current" => "current",
                "stale" => "stale",
                "missing" | "blocked" => "blocked",
                _ => "not_recorded",
            };
            return json!({
                "state": state,
                "bindings": [{
                    "name": "provenance_freshness",
                    "reported_status": reported,
                    "artifact": workspace_rel(&project.workspace_root, &run_root.join("provenance.json"))
                }]
            });
        }
        return json!({ "state": "not_recorded", "bindings": [] });
    };
    let mut bindings = Vec::new();
    if let (Some(expected), Some(path_raw)) = (
        result
            .pointer("/digests/input_manifest_sha256")
            .and_then(Value::as_str),
        result.pointer("/inputs/manifest").and_then(Value::as_str),
    ) {
        let path = resolve_declared_path(project, path_raw);
        let actual = path.as_deref().and_then(sha256_file);
        let state = match actual.as_deref() {
            Some(value) if value.eq_ignore_ascii_case(expected) => "current",
            Some(_) => "stale",
            None => "missing",
        };
        bindings.push(json!({
            "name": "input_manifest_sha256",
            "state": state,
            "expected_sha256": expected,
            "actual_sha256": actual,
            "artifact": path.map(|value| workspace_rel(&project.workspace_root, &value))
        }));
    }
    let state = if bindings
        .iter()
        .any(|binding| binding.get("state").and_then(Value::as_str) == Some("stale"))
    {
        "stale"
    } else if bindings
        .iter()
        .any(|binding| binding.get("state").and_then(Value::as_str) == Some("missing"))
    {
        "blocked"
    } else if bindings.is_empty() {
        "not_recorded"
    } else {
        "current"
    };
    json!({ "state": state, "bindings": bindings })
}

fn declared_artifact_roots(project: &DeliveryProject, key: &str) -> Vec<PathBuf> {
    let mut roots = BTreeSet::new();
    for document in std::iter::once(&project.manifest).chain(
        project
            .companion_manifest
            .as_ref()
            .map(|(_, value)| value)
            .into_iter(),
    ) {
        if let Some(raw) = document
            .pointer(&format!("/artifact_roots/{key}"))
            .and_then(Value::as_str)
        {
            if let Some(path) = resolve_declared_dir(project, raw) {
                roots.insert(path);
            }
        }
    }
    roots.into_iter().collect()
}

fn verification_report_paths(run_root: &StdPath) -> Vec<PathBuf> {
    let candidates = [
        run_root.join("compile/verification_report.json"),
        run_root.join("project-check/project_check_report.json"),
        run_root.join("project-check/compile_verify/verification_report.json"),
        run_root.join("project-check/intent_alignment/report.json"),
        run_root.join("compiler-stages.json"),
    ];
    candidates
        .into_iter()
        .filter(|path| path.is_file())
        .collect()
}

fn extract_verification_stages(document: &Value) -> Vec<Value> {
    if let Some(stages) = document.get("verification").and_then(Value::as_object) {
        return stages
            .iter()
            .map(|(name, value)| {
                json!({
                    "stage": name,
                    "reported_status": value.get("level").cloned(),
                    "warnings": value.get("warnings").cloned().unwrap_or_else(|| json!([])),
                    "checked_rules": value.get("checked_rules").cloned(),
                    "skipped_rules": value.get("skipped_rules").cloned()
                })
            })
            .collect();
    }
    if let Some(stages) = document.get("stages").and_then(Value::as_array) {
        return stages.clone();
    }
    if let Some(steps) = document.get("steps").and_then(Value::as_array) {
        return steps
            .iter()
            .map(|step| {
                json!({
                    "stage": step.get("name").cloned(),
                    "reported_status": step.get("status").cloned(),
                    "exit_code": step.get("exit_code").cloned(),
                    "report_json": step.get("report_json").cloned()
                })
            })
            .collect();
    }
    if document.get("verdict").is_some() {
        return vec![json!({
            "stage": "intent_alignment",
            "reported_status": document.get("verdict").cloned(),
            "blocker_kind": document.get("blocker_kind").cloned(),
            "warnings": document.get("warnings").cloned().unwrap_or_else(|| json!([]))
        })];
    }
    Vec::new()
}

fn project_human_hold_document(project: &DeliveryProject) -> Option<Value> {
    let declared = project
        .manifest
        .pointer("/fixtures/human_holds/fixture_ref")
        .and_then(Value::as_str)
        .and_then(|raw| resolve_declared_path(project, raw));
    let conventional = project.project_root.join("release/human-holds.json");
    declared
        .or_else(|| conventional.is_file().then_some(conventional))
        .and_then(|path| read_structured_document(&path).ok())
}

fn normalized_human_holds(document: &Value) -> Value {
    Value::Array(
        document
            .get("holds")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|hold| {
                let hold_id = hold.get("hold_id")?.as_str()?;
                let raw_status = hold
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("pending");
                let status = match raw_status {
                    "human_confirmed" | "confirmed" | "release_approved" => "confirmed",
                    "rejected" => "rejected",
                    "stale" => "stale",
                    "blocked" => "blocked",
                    _ => "pending",
                };
                Some(json!({
                    "hold_id": hold_id,
                    "label": humanize_identifier(hold_id),
                    "role": hold.get("required_role").cloned().unwrap_or(Value::Null),
                    "status": status,
                    "reason": hold.get("reason").cloned().unwrap_or(Value::Null),
                    "blocker_ids": hold.get("blocker_ids").cloned().unwrap_or_else(|| json!([])),
                    "signed_by": hold.pointer("/signature/user/name").cloned().unwrap_or(Value::Null),
                    "signed_at": hold.pointer("/signature/signed_at").cloned().unwrap_or(Value::Null)
                }))
            })
            .collect(),
    )
}

fn normalized_wiring_point(point: &Value) -> Value {
    let point_id = point
        .get("point_id")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let (controller, channel) = point_id
        .split_once('.')
        .map(|(controller, channel)| (controller, channel))
        .unwrap_or(("unknown", point_id));
    let point_status = point
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("human_action_required");
    json!({
        "point_id": point_id,
        "controller": controller,
        "channel": channel,
        "alias": point.get("alias").cloned().unwrap_or(Value::Null),
        "direction": point.get("direction").cloned().unwrap_or(Value::Null),
        "device_terminal": point.get("device_terminal").cloned().unwrap_or(Value::Null),
        "signal_type": point.get("signal_type").cloned().unwrap_or(Value::Null),
        "safe_state": point.get("safe_state").cloned().unwrap_or(Value::Null),
        "wire_id": point.get("wire_id").cloned().unwrap_or(Value::Null),
        "evidence_source": point.get("evidence_source").cloned().unwrap_or(Value::Null),
        "compiler_status": "derived",
        "point_check_status": match point_status {
            "verified" | "observed" | "confirmed" => "observed",
            "blocked" => "blocked",
            "stale" => "stale",
            _ => "pending",
        },
        "note": point.get("note").cloned().unwrap_or(Value::Null)
    })
}

fn normalized_verification_stage(
    project: &DeliveryProject,
    run_id: &Value,
    stage: &Value,
    report_artifact: Option<&Value>,
) -> Value {
    let stage_name = stage
        .get("stage")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let raw_status = stage
        .get("status")
        .or_else(|| stage.get("reported_status"))
        .and_then(Value::as_str)
        .unwrap_or("derived");
    let status = normalized_evidence_state(raw_status);
    let diagnostics = stage
        .get("diagnostics")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let diagnostic_code = diagnostics.first().cloned().unwrap_or(Value::Null);
    let message = if diagnostics.is_empty() {
        Value::Null
    } else {
        Value::String(
            diagnostics
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| value.to_string())
                })
                .collect::<Vec<_>>()
                .join(", "),
        )
    };
    let artifact_ref = stage
        .pointer("/evidence/artifact_ref")
        .cloned()
        .or_else(|| report_artifact.and_then(|artifact| artifact.get("href").cloned()))
        .or_else(|| report_artifact.and_then(|artifact| artifact.get("path").cloned()))
        .unwrap_or(Value::Null);
    json!({
        "stage": stage_name,
        "status": status,
        "reported_status": raw_status,
        "producer": "rust_plc compiler evidence",
        "run_id": run_id,
        "source_commit": stage.pointer("/evidence/source_commit").cloned().unwrap_or_else(|| project.source_commit.clone().map(Value::String).unwrap_or(Value::Null)),
        "artifact_ref": artifact_ref,
        "diagnostic_code": diagnostic_code,
        "message": message
    })
}

fn normalized_evidence_record(project: &DeliveryProject, artifact: &Value) -> Option<Value> {
    let path = artifact.get("path")?.as_str()?;
    let label = StdPath::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(path);
    Some(json!({
        "evidence_id": path,
        "label": label,
        "evidence_state": artifact.get("evidence_state").cloned().unwrap_or_else(|| Value::String("derived".to_string())),
        "producer": "delivery project registry",
        "source_commit": project.source_commit,
        "artifact_ref": artifact.get("href").filter(|value| !value.is_null()).cloned().unwrap_or_else(|| Value::String(path.to_string())),
        "digest": artifact.get("sha256").cloned().unwrap_or(Value::Null),
        "digest_algorithm": "sha256",
        "digest_normalization": "raw_bytes",
        "stale": false
    }))
}

fn normalized_evidence_state(status: &str) -> &'static str {
    let status = status.to_ascii_lowercase();
    if status.contains("not_exercised") || status.contains("blocked") {
        "blocked"
    } else if status.contains("blocker") || status.contains("warning") {
        "warning"
    } else if status.contains("observed") {
        "observed"
    } else {
        map_reported_status(&status)
    }
}

fn compiler_stage_key(stage: &str) -> Option<String> {
    let key = stage
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    matches!(
        key.as_str(),
        "parser"
            | "ast"
            | "semantic"
            | "ir"
            | "safety"
            | "liveness"
            | "timing"
            | "causality"
            | "runtimebridgesimulation"
            | "processmodelcheck"
            | "intentalignment"
            | "codegen"
    )
    .then_some(key)
}

fn humanize_identifier(value: &str) -> String {
    value
        .split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn reported_status(document: &Value) -> Value {
    document
        .get("status")
        .cloned()
        .or_else(|| document.get("verdict").cloned())
        .unwrap_or(Value::Null)
}

fn collect_project_artifacts(project: &DeliveryProject) -> Vec<PathBuf> {
    let mut paths = BTreeSet::new();
    paths.insert(project.manifest_path.clone());
    if let Some((path, _)) = &project.companion_manifest {
        paths.insert(path.clone());
    }
    for raw in project
        .source_entry
        .iter()
        .chain(project.system_contract.iter())
    {
        if let Some(path) = resolve_declared_path(project, raw) {
            paths.insert(path);
        }
    }
    let mut declared_refs = Vec::new();
    collect_declared_references(&project.manifest, &mut declared_refs);
    for raw in declared_refs {
        if let Some(path) = resolve_declared_path(project, &raw) {
            paths.insert(path);
        }
    }
    for key in [
        "agent_runs",
        "verification",
        "wiring",
        "execution",
        "release",
    ] {
        for root in declared_artifact_roots(project, key) {
            paths.extend(collect_files(&root, MAX_PROJECT_ARTIFACTS));
        }
    }
    let conventional_runs = project.project_root.join("harness-runs");
    if conventional_runs.is_dir() {
        paths.extend(collect_files(&conventional_runs, MAX_PROJECT_ARTIFACTS));
    }
    let physical_evidence = project
        .workspace_root
        .join("out")
        .join("web-delivery-evidence")
        .join(&project.project_id);
    if physical_evidence.is_dir() && is_within_workspace(project, &physical_evidence) {
        paths.extend(collect_files(&physical_evidence, MAX_PROJECT_ARTIFACTS));
        let observation_log = physical_evidence.join("point-observations.jsonl");
        paths.extend(observation_trace_artifacts(project, &observation_log));
    }
    for name in ["result.json", "selftest-report.md", "selftest-report.html"] {
        let path = project.project_root.join(name);
        if path.is_file() {
            paths.insert(path);
        }
    }
    paths.into_iter().take(MAX_PROJECT_ARTIFACTS).collect()
}

fn observation_trace_artifacts(
    project: &DeliveryProject,
    observation_log: &StdPath,
) -> Vec<PathBuf> {
    let Ok(raw) = fs::read_to_string(observation_log) else {
        return Vec::new();
    };
    let mut allowed_roots = vec![project.project_root.clone()];
    for key in [
        "agent_runs",
        "verification",
        "wiring",
        "execution",
        "release",
        "ci",
    ] {
        allowed_roots.extend(declared_artifact_roots(project, key));
    }
    raw.lines()
        .take(20_000)
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|observation| {
            let raw = observation.get("trace_ref")?.as_str()?;
            let path = resolve_declared_path(project, raw)?;
            allowed_roots
                .iter()
                .any(|root| path.starts_with(root))
                .then_some(path)
        })
        .collect()
}

fn collect_declared_references(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if matches!(key.as_str(), "artifact_ref" | "fixture_ref" | "source_ref") {
                    if let Some(raw) = value.as_str() {
                        output.push(raw.to_string());
                    }
                }
                collect_declared_references(value, output);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_declared_references(item, output);
            }
        }
        _ => {}
    }
}

fn project_problems(project: &DeliveryProject) -> Vec<Value> {
    let mut problems = Vec::new();
    let required = required_artifact_status(project);
    if let Some(missing) = required.get("missing").and_then(Value::as_array) {
        for item in missing {
            problems.push(json!({
                "project_id": project.project_id,
                "run_id": Value::Null,
                "stage": "project_registry",
                "code": "REQUIRED_ARTIFACT_MISSING",
                "severity": "blocked",
                "message": format!("required delivery artifact is missing: {}", item.get("path").and_then(Value::as_str).unwrap_or("unknown")),
                "artifact": manifest_provenance(project)
            }));
        }
    }
    for run_root in collect_run_roots(project) {
        let run_id = run_identifier(project, &run_root);
        let documents = run_documents(project, &run_root);
        if let Some(result) = documents.get("result") {
            if let Some(gaps) = result.get("known_gaps").and_then(Value::as_array) {
                for gap in gaps {
                    problems.push(json!({
                        "project_id": project.project_id,
                        "run_id": run_id,
                        "stage": gap.get("layer").cloned(),
                        "code": gap.get("id").cloned(),
                        "severity": "blocked",
                        "message": gap.get("evidence").cloned(),
                        "classification": gap.get("classification").cloned(),
                        "artifact": run_result_path(project, &run_root).and_then(|path| artifact_descriptor(project, &path, "derived"))
                    }));
                }
            }
            if let Some(steps) = result.get("steps").and_then(Value::as_array) {
                for step in steps {
                    let classification = step
                        .get("classification")
                        .and_then(Value::as_str)
                        .unwrap_or("not_recorded");
                    if !matches!(classification, "pass" | "ok" | "success") {
                        problems.push(json!({
                            "project_id": project.project_id,
                            "run_id": run_id,
                            "stage": step.get("name").cloned(),
                            "code": format!("STEP_{}", classification.to_ascii_uppercase()),
                            "severity": if classification == "known_gap" { "blocked" } else { "warning" },
                            "message": step.get("note").cloned().or_else(|| step.get("expectation").cloned()),
                            "artifact": run_result_path(project, &run_root).and_then(|path| artifact_descriptor(project, &path, "derived"))
                        }));
                    }
                }
            }
        }
        if let Some(verification) = documents.get("verification") {
            if let Some(stages) = verification.get("verification").and_then(Value::as_object) {
                for (stage, value) in stages {
                    if let Some(warnings) = value.get("warnings").and_then(Value::as_array) {
                        for warning in warnings {
                            problems.push(json!({
                                "project_id": project.project_id,
                                "run_id": run_id,
                                "stage": stage,
                                "code": warning.get("code").cloned().unwrap_or_else(|| Value::String("VERIFICATION_WARNING".to_string())),
                                "severity": warning.get("level").cloned().unwrap_or_else(|| Value::String("warning".to_string())),
                                "message": warning.get("message").cloned().unwrap_or_else(|| warning.clone()),
                                "artifact": artifact_descriptor(project, &run_root.join("compile/verification_report.json"), "derived")
                            }));
                        }
                    }
                }
            }
        }
        if let Some(records) = documents
            .get("anomalies")
            .and_then(|value| value.get("records"))
            .and_then(Value::as_array)
        {
            for record in records {
                let status = record
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("not_recorded");
                if status == "corrected" {
                    continue;
                }
                problems.push(json!({
                    "project_id": project.project_id,
                    "run_id": run_id,
                    "stage": "agent_run",
                    "code": record.get("gap_id").cloned().or_else(|| record.get("anomaly_id").cloned()),
                    "severity": if status == "blocked" { "blocked" } else { "warning" },
                    "message": record.get("summary").cloned(),
                    "classification": record.get("classification").cloned(),
                    "artifact": artifact_descriptor(project, &run_root.join("anomalies.json"), "derived")
                }));
            }
        }
    }
    problems
}

fn attach_problem_deep_link(catalog: &DeliveryCatalog, problem: &mut Value) {
    let Some(project_id) = problem
        .get("project_id")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return;
    };
    let Some(project) = catalog
        .projects
        .iter()
        .find(|project| project.project_id == project_id)
    else {
        return;
    };
    let source_commit = project.source_commit.clone();
    let run_id = problem
        .get("run_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let artifact = problem
        .get("artifact")
        .and_then(|value| {
            value
                .get("path")
                .and_then(Value::as_str)
                .or_else(|| value.get("artifact").and_then(Value::as_str))
        })
        .map(str::to_string);
    let object_id = problem
        .get("code")
        .and_then(Value::as_str)
        .or_else(|| problem.get("stage").and_then(Value::as_str))
        .map(str::to_string);
    if let Some(object) = problem.as_object_mut() {
        if let Some(artifact) = artifact.as_ref() {
            object
                .entry("source_ref".to_string())
                .or_insert_with(|| Value::String(artifact.clone()));
        }
        object.insert(
            "deep_link".to_string(),
            delivery_deep_link(
                &project_id,
                run_id.as_deref(),
                artifact.as_deref(),
                Some("problem"),
                object_id.as_deref(),
                source_commit.as_deref(),
            ),
        );
    }
}

fn aggregate_freshness_state(runs: &[Value]) -> &'static str {
    let states = runs
        .iter()
        .filter_map(|run| run.pointer("/freshness/state").and_then(Value::as_str));
    let mut current = false;
    let mut not_recorded = false;
    for state in states {
        match state {
            "stale" => return "stale",
            "blocked" | "missing" => return "blocked",
            "current" => current = true,
            _ => not_recorded = true,
        }
    }
    if not_recorded {
        "not_recorded"
    } else if current {
        "current"
    } else {
        "unavailable"
    }
}

fn source_document_freshness(project: &DeliveryProject, document: &Value, path: &StdPath) -> Value {
    let recorded_commit = document
        .get("source_commit")
        .and_then(Value::as_str)
        .or_else(|| document.get("git_head").and_then(Value::as_str));
    let state = match (recorded_commit, project.source_commit.as_deref()) {
        (Some(recorded), Some(expected)) if recorded == expected => "current",
        (Some(_), Some(_)) => "stale",
        _ => "not_recorded",
    };
    json!({
        "state": state,
        "source_commit": recorded_commit,
        "expected_source_commit": project.source_commit,
        "artifact": workspace_rel(&project.workspace_root, path),
        "artifact_sha256": sha256_file(path),
    })
}

fn collect_test_records(
    project: &DeliveryProject,
    run_id: &str,
    run_root: &StdPath,
    source: &str,
    document: &Value,
    execution_source: &str,
    freshness: &Value,
    source_artifact: Option<&StdPath>,
    seen: &mut HashSet<String>,
    tests: &mut Vec<Value>,
) {
    let Some(records) = document
        .get("steps")
        .and_then(Value::as_array)
        .or_else(|| document.get("stages").and_then(Value::as_array))
    else {
        return;
    };
    for record in records {
        let name = record
            .get("name")
            .and_then(Value::as_str)
            .or_else(|| record.get("stage").and_then(Value::as_str))
            .unwrap_or("unnamed");
        let key = format!(
            "{}:{execution_source}:{run_id}:{source}:{name}",
            project.project_id
        );
        if !seen.insert(key) {
            continue;
        }
        let artifact_ref = source_artifact.map(|path| workspace_rel(&project.workspace_root, path));
        let semantic_object_id = format!("{source}:{name}");
        tests.push(json!({
            "project_id": project.project_id,
            "run_id": run_id,
            "execution_source": execution_source,
            "suite": source,
            "name": name,
            "reported_status": record.get("classification").cloned().or_else(|| record.get("status").cloned()),
            "exit_code": record.get("exit_code").cloned(),
            "elapsed_ms": record.get("elapsed_ms").cloned(),
            "artifact_paths": record.get("artifacts").cloned().unwrap_or_else(|| json!([])),
            "artifact_ref": artifact_ref,
            "diagnostics": record.get("diagnostics").cloned().unwrap_or_else(|| json!([])),
            "stdout_log": record.get("stdout_log").cloned(),
            "stderr_log": record.get("stderr_log").cloned(),
            "freshness": freshness,
            "deep_link": delivery_deep_link(
                &project.project_id,
                Some(run_id),
                artifact_ref.as_deref(),
                Some("test"),
                Some(&semantic_object_id),
                project.source_commit.as_deref()
            ),
            "provenance": {
                "execution_source": execution_source,
                "source_document": source,
                "run_root": workspace_rel(&project.workspace_root, run_root),
                "artifact": source_artifact
                    .and_then(|path| artifact_descriptor(project, path, "derived"))
            }
        }));
    }
}

fn run_document_path(run_root: &StdPath, source: &str) -> Option<PathBuf> {
    let relative = match source {
        "result" => "result.json",
        "input_manifest" => "input-manifest.json",
        "provenance" => "provenance.json",
        "anomalies" => "anomalies.json",
        "corrections" => "corrections.json",
        "compiler_stages" => "compiler-stages.json",
        "project_check" => "project-check/project_check_report.json",
        "verification" => "compile/verification_report.json",
        _ => return None,
    };
    let path = run_root.join(relative);
    path.is_file().then_some(path)
}

fn artifact_descriptor(
    project: &DeliveryProject,
    path: &StdPath,
    evidence_state: &str,
) -> Option<Value> {
    let canonical = path.canonicalize().ok()?;
    if !canonical.starts_with(&project.workspace_root) {
        return None;
    }
    let metadata = fs::metadata(&canonical).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_DOCUMENT_BYTES {
        return None;
    }
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64);
    let workspace_path = workspace_rel(&project.workspace_root, &canonical);
    let out_root = project.workspace_root.join("out");
    let href = canonical.strip_prefix(&out_root).ok().map(|relative| {
        format!(
            "/artifacts/{}",
            relative.to_string_lossy().replace('\\', "/")
        )
    });
    Some(json!({
        "path": workspace_path,
        "href": href,
        "size_bytes": metadata.len(),
        "modified_ms": modified_ms,
        "sha256": sha256_file(&canonical),
        "evidence_state": evidence_state,
        "deep_link": delivery_deep_link(
            &project.project_id,
            None,
            Some(&workspace_path),
            None,
            None,
            project.source_commit.as_deref()
        ),
        "provenance": {
            "project_id": project.project_id,
            "manifest": workspace_rel(&project.workspace_root, &project.manifest_path)
        }
    }))
}

fn evidence_status(
    state: &str,
    artifact: &StdPath,
    project: &DeliveryProject,
    reason: Option<&str>,
) -> Value {
    json!({
        "state": state,
        "reason": reason,
        "artifact": workspace_rel(&project.workspace_root, artifact),
        "artifact_sha256": sha256_file(artifact),
        "source_commit": project.source_commit
    })
}

fn manifest_provenance(project: &DeliveryProject) -> Value {
    json!({
        "artifact": workspace_rel(&project.workspace_root, &project.manifest_path),
        "sha256": sha256_file(&project.manifest_path),
        "source_commit": project.source_commit,
        "project_root": workspace_rel(&project.workspace_root, &project.project_root)
    })
}

fn infer_project_root(
    workspace_root: &StdPath,
    manifest_path: &StdPath,
    manifest: &Value,
) -> PathBuf {
    if let Some(source) = string_at(manifest, &["source_entry"]) {
        if let Some(relative) = normalized_recorded_path(&source) {
            for ancestor in manifest_path
                .parent()
                .into_iter()
                .flat_map(StdPath::ancestors)
            {
                if !ancestor.starts_with(workspace_root) {
                    break;
                }
                if ancestor.join(&relative).is_file() {
                    return ancestor.to_path_buf();
                }
                if ancestor == workspace_root {
                    break;
                }
            }
        }
    }
    manifest_path
        .parent()
        .unwrap_or(workspace_root)
        .to_path_buf()
}

fn resolve_declared_path(project: &DeliveryProject, raw: &str) -> Option<PathBuf> {
    let relative = normalized_recorded_path(raw)?;
    let manifest_parent = project.manifest_path.parent()?;
    for candidate in [
        project.project_root.join(&relative),
        manifest_parent.join(&relative),
        project.workspace_root.join(&relative),
    ] {
        if let Some(path) = canonical_workspace_file(project, &candidate) {
            return Some(path);
        }
    }
    None
}

fn resolve_declared_dir(project: &DeliveryProject, raw: &str) -> Option<PathBuf> {
    let relative = normalized_recorded_path(raw)?;
    let manifest_parent = project.manifest_path.parent()?;
    for candidate in [
        project.project_root.join(&relative),
        manifest_parent.join(&relative),
        project.workspace_root.join(&relative),
    ] {
        let Ok(canonical) = candidate.canonicalize() else {
            continue;
        };
        if canonical.starts_with(&project.workspace_root) && canonical.is_dir() {
            return Some(canonical);
        }
    }
    None
}

fn resolve_from_base(project: &DeliveryProject, base: &StdPath, raw: &str) -> Option<PathBuf> {
    let relative = normalized_recorded_path(raw)?;
    canonical_workspace_file(project, &base.join(relative))
}

fn canonical_workspace_file(project: &DeliveryProject, candidate: &StdPath) -> Option<PathBuf> {
    let canonical = candidate.canonicalize().ok()?;
    (canonical.starts_with(&project.workspace_root) && canonical.is_file()).then_some(canonical)
}

fn normalized_recorded_path(raw: &str) -> Option<PathBuf> {
    if raw.is_empty() || raw.len() > 4096 || raw.contains(['%', '\0']) {
        return None;
    }
    let normalized = raw.replace('\\', "/");
    if normalized.starts_with('/') || normalized.contains(':') {
        return None;
    }
    let path = PathBuf::from(normalized);
    if path.is_absolute()
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return None;
    }
    Some(path)
}

fn is_within_workspace(project: &DeliveryProject, path: &StdPath) -> bool {
    path.canonicalize()
        .map(|canonical| canonical.starts_with(&project.workspace_root))
        .unwrap_or(false)
}

fn discover_named_files(
    root: &StdPath,
    depth: usize,
    names: &[&str],
    visited: &mut usize,
    output: &mut Vec<PathBuf>,
) {
    if depth > MAX_DISCOVERY_DEPTH || *visited >= MAX_DISCOVERY_ENTRIES {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        *visited += 1;
        if *visited >= MAX_DISCOVERY_ENTRIES {
            return;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            discover_named_files(&entry.path(), depth + 1, names, visited, output);
        } else if file_type.is_file()
            && entry
                .file_name()
                .to_str()
                .is_some_and(|name| names.contains(&name))
        {
            output.push(entry.path());
        }
    }
}

fn collect_files(root: &StdPath, limit: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_files_inner(root, 0, limit, &mut files);
    files.sort();
    files
}

fn collect_files_inner(root: &StdPath, depth: usize, limit: usize, files: &mut Vec<PathBuf>) {
    if depth > MAX_DISCOVERY_DEPTH || files.len() >= limit {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if files.len() >= limit {
            return;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect_files_inner(&entry.path(), depth + 1, limit, files);
        } else if file_type.is_file() {
            files.push(entry.path());
        }
    }
}

fn read_structured_document(path: &StdPath) -> Result<Value, String> {
    let metadata = fs::metadata(path).map_err(|err| err.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_DOCUMENT_BYTES {
        return Err("document is unavailable or exceeds the delivery API limit".to_string());
    }
    let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
    match path.extension().and_then(|value| value.to_str()) {
        Some("toml") => toml::from_str::<toml::Value>(&text)
            .map_err(|err| err.to_string())
            .and_then(|value| serde_json::to_value(value).map_err(|err| err.to_string())),
        Some("yaml") | Some("yml") => {
            serde_yaml::from_str::<Value>(&text).map_err(|err| err.to_string())
        }
        _ => serde_json::from_str::<Value>(&text).map_err(|err| err.to_string()),
    }
}

fn sha256_file(path: &StdPath) -> Option<String> {
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
    let digest = hasher.finalize();
    Some(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn string_at(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    current.as_str().map(str::to_string)
}

fn string_array_at(value: &Value, field: &str) -> Vec<String> {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn map_reported_status(status: &str) -> &'static str {
    match status.to_ascii_lowercase().as_str() {
        "pass" | "passed" | "ok" | "success" | "verified" | "aligned" => "verified",
        "warning" | "warn" | "partial" => "warning",
        "stale" => "stale",
        "blocked" | "fail" | "failed" | "error" => "blocked",
        "authored" => "authored",
        _ => "derived",
    }
}

fn workspace_git_status(workspace_root: &StdPath) -> Value {
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(workspace_root)
        .output();
    let status = Command::new("git")
        .args(["status", "--porcelain=v1", "--untracked-files=normal"])
        .current_dir(workspace_root)
        .output();
    match (head, status) {
        (Ok(head), Ok(status)) if head.status.success() && status.status.success() => {
            let head = String::from_utf8_lossy(&head.stdout).trim().to_string();
            let changed_paths = String::from_utf8_lossy(&status.stdout)
                .lines()
                .take(500)
                .map(|line| line.get(3..).unwrap_or(line).trim().replace('\\', "/"))
                .collect::<Vec<_>>();
            json!({
                "status": "current",
                "head": head,
                "dirty": !changed_paths.is_empty(),
                "changed_paths": changed_paths,
                "provenance": {
                    "kind": "git_worktree",
                    "commands": ["git rev-parse HEAD", "git status --porcelain=v1 --untracked-files=normal"]
                }
            })
        }
        (head, status) => json!({
            "status": "unavailable",
            "head": Value::Null,
            "dirty": Value::Null,
            "changed_paths": [],
            "error_code": "GIT_STATUS_UNAVAILABLE",
            "provenance": {
                "kind": "git_worktree",
                "rev_parse_started": head.is_ok(),
                "status_started": status.is_ok()
            }
        }),
    }
}

fn run_git_status(
    project: &DeliveryProject,
    result: Option<&Value>,
    provenance: Option<&Value>,
) -> Value {
    let recorded_commit = result
        .and_then(|value| value.get("git_head").and_then(Value::as_str))
        .or_else(|| provenance.and_then(|value| value.get("source_commit").and_then(Value::as_str)))
        .or(project.source_commit.as_deref());
    let recorded_dirty = result
        .and_then(|value| value.get("dirty_worktree").and_then(Value::as_bool))
        .or_else(|| result.and_then(|value| value.pointer("/git/dirty").and_then(Value::as_bool)))
        .or_else(|| {
            result.and_then(|value| {
                value
                    .pointer("/workspace/dirty_worktree")
                    .and_then(Value::as_bool)
            })
        })
        .or_else(|| {
            provenance.and_then(|value| value.get("dirty_worktree").and_then(Value::as_bool))
        });
    let state = if recorded_commit.is_none() || recorded_dirty.is_none() {
        "not_recorded"
    } else {
        "recorded"
    };
    json!({
        "status": state,
        "source_commit": recorded_commit,
        "dirty_worktree": recorded_dirty,
        "error_code": (state == "not_recorded").then_some("RUN_GIT_STATE_NOT_RECORDED"),
        "provenance": {
            "result_fields": ["git_head", "dirty_worktree", "git.dirty", "workspace.dirty_worktree"]
        }
    })
}

fn is_safe_identifier(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
}

fn workspace_rel(workspace_root: &StdPath, path: &StdPath) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    canonical
        .strip_prefix(workspace_root)
        .unwrap_or(&canonical)
        .to_string_lossy()
        .replace('\\', "/")
}

fn same_path(left: &StdPath, right: &StdPath) -> bool {
    left.canonicalize().ok() == right.canonicalize().ok()
}

fn bad_request(message: impl Into<String>) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": message.into() })),
    )
}

fn not_found(message: impl Into<String>) -> ApiError {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": message.into() })),
    )
}
