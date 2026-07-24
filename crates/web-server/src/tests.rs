use super::{
    build_app, build_collab_event, build_geometry_export_args, build_plc_diagnostics,
    build_plc_realtime_response, collab_comment_history,
    delivery::{current_evidence_digests, resolve_delivery_project_root},
    dsl_capabilities, generate_plc_from_flowchart, get_geometry, get_keypoints, get_project_source,
    get_trace, get_trace_range, internal_error, is_safe_collab_room, list_project_templates,
    new_run_id, normalize_topology_tags_in_place, parse_plc_topology, plc_language_snapshot,
    record_collab_comment, resolve_artifact_reference, resolve_workspace_input, save_scenario,
    save_topology, trigger_no_board, validate_bind_security, validate_scenario_limits, AppState,
    CollabClientEvent, FlowchartEditorStep, FlowchartEditorTransition, FlowchartGeneratePlcRequest,
    ParsePlcTopologyRequest, PlcLanguageRequest, PlcRealtimeRequest, RunArtifacts, RunRecord,
    RustPlcLauncher, TickRangeQuery, TriggerRunRequest, WebSecurityConfig,
    COLLAB_COMMENT_HISTORY_DIR, DEFAULT_RUN_TIMEOUT_SECS, MAX_ARTIFACT_BYTES,
    MAX_COLLAB_COMMENT_HISTORY, TAGS_SCHEMA_VERSION,
};
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, Request, StatusCode};
use axum::response::Json;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{RwLock, Semaphore};
use tower::ServiceExt;

use super::auth::UserRole;

async fn response_json(response: axum::response::Response) -> Value {
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should be readable");
    serde_json::from_slice(&body).expect("response body should contain JSON")
}

fn find_component<'a>(payload: &'a Value, id: &str) -> &'a Value {
    payload["components"]
        .as_array()
        .and_then(|components| {
            components
                .iter()
                .find(|component| component.get("id").and_then(Value::as_str) == Some(id))
        })
        .expect("component should exist")
}

fn has_detects_connection(payload: &Value, from: &str, to: &str, signal: &str) -> bool {
    payload["connections"]
        .as_array()
        .map(|connections| {
            connections.iter().any(|connection| {
                connection.get("relation").and_then(Value::as_str) == Some("detects")
                    && connection.get("from").and_then(Value::as_str) == Some(from)
                    && connection.get("to").and_then(Value::as_str) == Some(to)
                    && connection.get("signal").and_then(Value::as_str) == Some(signal)
                    && connection.get("from_port").and_then(Value::as_str) == Some(signal)
            })
        })
        .unwrap_or(false)
}

fn temp_workspace_root(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("rustplc-web-server-{label}-{unique}"));
    fs::create_dir_all(&root).expect("temp workspace root should be created");
    root
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should resolve")
}

fn test_state(
    workspace_root: PathBuf,
    runs: std::collections::BTreeMap<String, RunRecord>,
) -> Arc<AppState> {
    test_state_with_security(workspace_root, runs, None, 2)
}

fn test_state_with_security(
    workspace_root: PathBuf,
    runs: std::collections::BTreeMap<String, RunRecord>,
    auth_token: Option<&str>,
    max_concurrent_runs: usize,
) -> Arc<AppState> {
    Arc::new(AppState {
        auth: super::auth::AuthService::disabled(),
        signatures: super::signatures::SignatureStore::new(&workspace_root),
        physical_evidence: super::physical_evidence::PhysicalEvidenceStore::new(&workspace_root),
        workspace_root,
        runs: Arc::new(RwLock::new(runs)),
        collab_rooms: Arc::new(RwLock::new(std::collections::HashMap::new())),
        collab_comments: Arc::new(RwLock::new(std::collections::HashMap::new())),
        security: WebSecurityConfig {
            auth_token: auth_token.map(Arc::<str>::from),
            allowed_origins: Vec::new(),
        },
        run_semaphore: Arc::new(Semaphore::new(max_concurrent_runs)),
        run_timeout: Duration::from_secs(DEFAULT_RUN_TIMEOUT_SECS),
        rust_plc_launcher: RustPlcLauncher::Cargo,
    })
}

#[test]
fn remote_bind_requires_explicit_opt_in_auth_and_origins() {
    let remote = "0.0.0.0:8080".parse().expect("valid socket address");
    assert!(validate_bind_security(remote, false, true, true, true).is_err());
    assert!(validate_bind_security(remote, true, false, true, true).is_err());
    assert!(validate_bind_security(remote, true, true, false, true).is_err());
    assert!(validate_bind_security(remote, true, true, true, true).is_ok());

    let local = "127.0.0.1:8080"
        .parse()
        .expect("valid loopback socket address");
    assert!(validate_bind_security(local, false, false, false, false).is_ok());
}

#[tokio::test]
async fn mutating_routes_require_configured_bearer_token() {
    let workspace_root = temp_workspace_root("auth-route");
    fs::create_dir_all(workspace_root.join("examples")).expect("examples directory should exist");
    let state = test_state_with_security(
        workspace_root,
        std::collections::BTreeMap::new(),
        Some("test-secret"),
        2,
    );
    let app = build_app(state);
    let request = Request::builder()
        .method("PUT")
        .uri("/api/topology/demo")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .expect("request should build");
    let response = app.clone().oneshot(request).await.expect("route response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let request = Request::builder()
        .method("PUT")
        .uri("/api/topology/demo")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, "Bearer test-secret")
        .body(Body::from("{}"))
        .expect("request should build");
    let response = app.oneshot(request).await.expect("route response");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn artifact_routes_require_configured_bearer_token() {
    let workspace_root = temp_workspace_root("artifact-auth-route");
    let artifact = workspace_root.join("out/report.json");
    fs::create_dir_all(artifact.parent().expect("artifact parent"))
        .expect("artifact parent should exist");
    fs::write(&artifact, "{}").expect("artifact should be written");
    let state = test_state_with_security(
        workspace_root,
        std::collections::BTreeMap::new(),
        Some("test-secret"),
        2,
    );
    let app = build_app(state);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/artifacts/report.json")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("route response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/artifacts/report.json")
                .header(header::AUTHORIZATION, "Bearer test-secret")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("route response");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn human_hold_signing_requires_session_role_commit_and_current_evidence() {
    let workspace_root = temp_workspace_root("hold-signing");
    let project_root = workspace_root
        .join("delivery-projects")
        .join("station-demo");
    fs::create_dir_all(project_root.join("release"))
        .expect("delivery fixture directory should exist");
    fs::write(
        project_root.join("delivery-project.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "project_id": "station.demo",
            "delivery_layer": "station",
            "source_commit": "deadbeef",
            "artifact_roots": { "release": "release" },
            "fixtures": {
                "human_holds": { "fixture_ref": "release/human-holds.json" }
            }
        }))
        .expect("manifest should serialize"),
    )
    .expect("manifest should be written");
    fs::write(
        project_root.join("release/human-holds.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "holds": [{
                "hold_id": "wiring_review",
                "required_role": "electrical_engineer",
                "status": "human_action_required"
            }]
        }))
        .expect("hold contract should serialize"),
    )
    .expect("hold contract should be written");

    let mut state = test_state_with_security(
        workspace_root,
        std::collections::BTreeMap::new(),
        Some("automation-token"),
        2,
    );
    Arc::get_mut(&mut state)
        .expect("test state should be uniquely owned")
        .auth = super::auth::AuthService::for_test_user(
        "electrical",
        "correct",
        UserRole::ElectricalEngineer,
    );
    let app = build_app(state);

    let login = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "username": "electrical", "password": "correct" }).to_string(),
                ))
                .expect("login request should build"),
        )
        .await
        .expect("login route should respond");
    assert_eq!(login.status(), StatusCode::OK);
    let login = response_json(login).await;
    let session = login["token"]
        .as_str()
        .expect("login should return a session token");

    let signature_context = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/delivery-projects/station.demo/holds/signatures")
                .body(Body::empty())
                .expect("signature context request should build"),
        )
        .await
        .expect("signature context route should respond");
    assert_eq!(signature_context.status(), StatusCode::OK);
    let signature_context = response_json(signature_context).await;
    assert_eq!(
        signature_context["attestation_standard"],
        "internal_engineering_v1"
    );
    let sign_body = json!({
        "hold_type": "wiring_review",
        "attestation_standard": "internal_engineering_v1",
        "source_commit": "deadbeef",
        "evidence_digests": signature_context["current_evidence_digests"],
        "decision": "approve",
        "comment": "wiring plan reviewed"
    });

    let automation_attempt = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/delivery-projects/station.demo/holds/wiring_review/sign")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer automation-token")
                .body(Body::from(sign_body.to_string()))
                .expect("automation signature request should build"),
        )
        .await
        .expect("automation signature route should respond");
    assert_eq!(automation_attempt.status(), StatusCode::UNAUTHORIZED);

    let signed = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/delivery-projects/station.demo/holds/wiring_review/sign")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {session}"))
                .body(Body::from(sign_body.to_string()))
                .expect("human signature request should build"),
        )
        .await
        .expect("human signature route should respond");
    assert_eq!(signed.status(), StatusCode::CREATED);
    let signed = response_json(signed).await;
    assert_eq!(signed["user"]["role"], "electrical_engineer");
    assert_eq!(signed["decision"], "approve");

    fs::write(
        project_root.join("release/observed-change.txt"),
        "changed evidence",
    )
    .expect("changed evidence should be written");
    let signatures = app
        .oneshot(
            Request::builder()
                .uri("/api/delivery-projects/station.demo/holds/signatures")
                .body(Body::empty())
                .expect("signature list request should build"),
        )
        .await
        .expect("signature list route should respond");
    let signatures = response_json(signatures).await;
    assert_eq!(signatures["signatures"][0]["stale"], true);
}

#[tokio::test]
async fn cors_only_echoes_configured_origins() {
    let workspace_root = temp_workspace_root("cors-route");
    let mut state = test_state(workspace_root, std::collections::BTreeMap::new());
    Arc::get_mut(&mut state)
        .expect("test state should be uniquely owned")
        .security
        .allowed_origins = vec!["http://localhost:8080"
        .parse()
        .expect("origin header should parse")];
    let app = build_app(state);

    let denied = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/projects")
                .header(header::ORIGIN, "http://evil.example")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("route response");
    assert!(denied
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .is_none());

    let allowed = app
        .oneshot(
            Request::builder()
                .uri("/api/projects")
                .header(header::ORIGIN, "http://localhost:8080")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("route response");
    assert_eq!(
        allowed
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .and_then(|value| value.to_str().ok()),
        Some("http://localhost:8080")
    );
}

#[tokio::test]
async fn topology_and_scenario_ids_reject_traversal_forms() {
    let workspace_root = temp_workspace_root("resource-id-reject");
    let state = test_state(workspace_root, std::collections::BTreeMap::new());
    for id in [
        "../target/probe",
        "..%2ftarget%2fprobe",
        r"..\target\probe",
        "C:probe",
    ] {
        let topology_error =
            save_topology(State(state.clone()), Path(id.to_string()), Json(json!({})))
                .await
                .expect_err("unsafe topology id must fail");
        assert_eq!(topology_error.0, StatusCode::BAD_REQUEST);

        let scenario_error = save_scenario(
            State(state.clone()),
            Path(id.to_string()),
            Json(json!({"tick_ms": 10, "duration_ms": 100})),
        )
        .await
        .expect_err("unsafe scenario id must fail");
        assert_eq!(scenario_error.0, StatusCode::BAD_REQUEST);
    }
}

#[test]
fn workspace_inputs_reject_absolute_parent_encoded_and_backslash_paths() {
    let workspace_root = temp_workspace_root("workspace-input-reject");
    let examples = workspace_root.join("examples");
    fs::create_dir_all(&examples).expect("examples directory should exist");
    fs::write(examples.join("ok.plc"), "[topology]\n").expect("fixture should be written");
    assert!(resolve_workspace_input(&workspace_root, "examples/ok.plc").is_ok());
    for raw in [
        "../secret.plc",
        "examples/%2e%2e/secret.plc",
        r"examples\..\secret.plc",
        workspace_root
            .join("examples/ok.plc")
            .to_string_lossy()
            .as_ref(),
    ] {
        assert!(
            resolve_workspace_input(&workspace_root, raw).is_err(),
            "{raw}"
        );
    }
}

#[test]
fn scenario_front_door_limits_tick_duration_and_tick_count() {
    assert!(validate_scenario_limits(&json!({"tick_ms": 0, "duration_ms": 1})).is_err());
    assert!(validate_scenario_limits(&json!({"tick_ms": 1, "duration_ms": 86_400_001})).is_err());
    assert!(validate_scenario_limits(&json!({"tick_ms": 1, "duration_ms": 1_000_001})).is_err());
    assert!(validate_scenario_limits(&json!({"tick_ms": 10, "duration_ms": 1000})).is_ok());
}

#[tokio::test]
async fn run_trigger_rejects_when_concurrency_limit_is_full() {
    let workspace_root = temp_workspace_root("run-limit");
    let examples = workspace_root.join("examples");
    fs::create_dir_all(&examples).expect("examples directory should exist");
    fs::write(examples.join("demo.plc"), "[topology]\n").expect("PLC fixture should be written");
    fs::write(
        examples.join("demo.scenario.json"),
        r#"{"tick_ms":10,"duration_ms":100}"#,
    )
    .expect("scenario fixture should be written");
    let state =
        test_state_with_security(workspace_root, std::collections::BTreeMap::new(), None, 1);
    let _permit = state
        .run_semaphore
        .clone()
        .acquire_owned()
        .await
        .expect("semaphore should be open");
    let error = trigger_no_board(
        State(state),
        Json(TriggerRunRequest {
            plc_file: Some("examples/demo.plc".to_string()),
            scenario_file: Some("examples/demo.scenario.json".to_string()),
            topology_file: None,
            mode: None,
            triggered_by: None,
        }),
    )
    .await
    .expect_err("full semaphore should reject the run");
    assert_eq!(error.0, StatusCode::TOO_MANY_REQUESTS);
}

#[test]
fn generated_run_ids_are_collision_resistant() {
    let ids = (0..1000)
        .map(|_| new_run_id("run"))
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(ids.len(), 1000);
    assert!(ids.iter().all(|id| id.starts_with("run-")));
}

#[tokio::test]
async fn artifact_route_rejects_oversized_files() {
    let workspace_root = temp_workspace_root("artifact-limit");
    let artifact = workspace_root.join("out/big.bin");
    fs::create_dir_all(artifact.parent().expect("artifact parent"))
        .expect("artifact parent should exist");
    let file = fs::File::create(&artifact).expect("artifact should be created");
    file.set_len(MAX_ARTIFACT_BYTES + 1)
        .expect("sparse artifact should resize");
    let app = build_app(test_state(
        workspace_root,
        std::collections::BTreeMap::new(),
    ));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/artifacts/big.bin")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("route response");
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[test]
fn artifact_references_reject_escape_and_absolute_paths() {
    let workspace_root = temp_workspace_root("artifact-reference-reject");
    fs::create_dir_all(workspace_root.join("out")).expect("output directory should exist");
    assert!(resolve_artifact_reference(&workspace_root, "/artifacts/../secret").is_none());
    assert!(resolve_artifact_reference(&workspace_root, "/artifacts/%2e%2e/secret").is_none());
    assert!(resolve_artifact_reference(&workspace_root, r"C:\secret").is_none());
}

#[test]
fn artifact_references_allow_delivery_project_evidence_only() {
    let workspace_root = temp_workspace_root("artifact-reference-delivery-project");
    let evidence = workspace_root.join("delivery-projects/demo/runs/run-1/anomalies.json");
    fs::create_dir_all(evidence.parent().expect("evidence parent"))
        .expect("create evidence parent");
    fs::write(&evidence, "{}").expect("write evidence");
    let resolved = resolve_artifact_reference(
        &workspace_root,
        "/artifacts/delivery-projects/demo/runs/run-1/anomalies.json",
    )
    .expect("delivery evidence should resolve");
    assert_eq!(
        resolved,
        evidence.canonicalize().expect("canonical evidence")
    );

    fs::write(workspace_root.join("secret.txt"), "secret").expect("write workspace file");
    let unrelated = resolve_artifact_reference(&workspace_root, "/artifacts/secret.txt")
        .expect("output artifact path should remain lexical");
    assert!(unrelated.ends_with("out/secret.txt"));
    assert_ne!(
        unrelated,
        workspace_root
            .join("secret.txt")
            .canonicalize()
            .expect("workspace file")
    );
}

#[test]
fn internal_errors_do_not_echo_host_details() {
    let error = internal_error(r"failed at C:\Users\operator\private.txt");
    assert_eq!(error.0, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(error.1 .0["error"], json!("internal server error"));
}

#[test]
fn topology_tag_normalization_adds_schema_and_default_dimensions() {
    let mut payload = json!({
        "schema_version": 1,
        "component_library": { "schema_version": 1, "components": [] },
        "components": [
            {
                "id": "x0",
                "component_id": "sensor",
                "params": { "name": "x0" }
            }
        ],
        "connections": []
    });
    normalize_topology_tags_in_place(&mut payload);
    assert_eq!(
        payload.get("tags_schema_version").and_then(|v| v.as_u64()),
        Some(TAGS_SCHEMA_VERSION)
    );
    assert_eq!(
        payload["components"][0]["params"]["tags"],
        json!({
            "functional_group": [],
            "danger_level": [],
            "location_group": []
        })
    );
}

#[test]
fn topology_tag_normalization_filters_non_string_values() {
    let mut payload = json!({
        "components": [
            {
                "id": "x0",
                "component_id": "sensor",
                "params": {
                    "tags": {
                        "functional_group": ["press", 1, true],
                        "danger_level": ["high"],
                        "location_group": ["line_a/cell_2/station_7", null]
                    }
                }
            }
        ]
    });
    normalize_topology_tags_in_place(&mut payload);
    assert_eq!(
        payload["components"][0]["params"]["tags"],
        json!({
            "functional_group": ["press"],
            "danger_level": ["high"],
            "location_group": ["line_a/cell_2/station_7"]
        })
    );
}

#[tokio::test]
async fn get_project_source_reads_safe_example_plc_id() {
    let workspace_root = temp_workspace_root("project-source");
    let examples = workspace_root.join("examples");
    fs::create_dir_all(&examples).expect("examples dir should be created");
    fs::write(
        examples.join("demo.plc"),
        "[topology]\n\n[constraints]\n\n[tasks]\n",
    )
    .expect("fixture plc should be written");
    let state = test_state(workspace_root, std::collections::BTreeMap::new());

    let Json(payload) = get_project_source(State(state), Path("demo".to_string()))
        .await
        .expect("safe demo source should load");
    assert_eq!(payload["id"], json!("demo"));
    assert_eq!(
        payload["content"],
        json!("[topology]\n\n[constraints]\n\n[tasks]\n")
    );
}

#[tokio::test]
async fn list_project_templates_groups_available_examples() {
    let workspace_root = temp_workspace_root("project-templates");
    let examples = workspace_root.join("examples");
    fs::create_dir_all(examples.join("recovery_templates"))
        .expect("nested examples dir should be created");
    fs::write(examples.join("demo.plc"), "[topology]\n").expect("demo should exist");
    fs::write(
        examples.join("recovery_templates/estop_recovery.plc"),
        "[topology]\n",
    )
    .expect("nested recovery template should exist");

    let state = test_state(workspace_root, std::collections::BTreeMap::new());

    let Json(payload) = list_project_templates(State(state)).await;
    let categories = payload["categories"].as_array().expect("categories array");
    assert!(categories.iter().any(|category| {
        category["category"] == json!("01 Basics")
            && category["templates"]
                .as_array()
                .expect("basic templates")
                .iter()
                .any(|template| template["id"] == json!("demo"))
    }));
    assert!(categories.iter().any(|category| {
        category["category"] == json!("05 Safety, Recovery, And Diagnostics")
            && category["templates"]
                .as_array()
                .expect("recovery templates")
                .iter()
                .any(|template| {
                    template["id"] == json!("recovery_templates_estop_recovery")
                        && template["path"]
                            == json!("examples/recovery_templates/estop_recovery.plc")
                })
    }));
}

#[tokio::test]
async fn project_templates_prefer_examples_catalog_when_present() {
    let workspace_root = temp_workspace_root("project-templates-catalog");
    let examples = workspace_root.join("examples/recovery_templates");
    fs::create_dir_all(&examples).expect("nested examples dir should be created");
    fs::write(
        workspace_root.join("examples/demo.plc"),
        "[topology]\n// demo\n",
    )
    .expect("demo should be written");
    fs::write(
        examples.join("estop_recovery.plc"),
        "[topology]\n// estop\n",
    )
    .expect("nested template should be written");
    fs::write(
        workspace_root.join("examples/catalog.toml"),
        r#"schema_version = 1

[[categories]]
id = "05_safety_recovery_and_diagnostics"
name = "05 Safety, Recovery, And Diagnostics"

[[categories.examples]]
id = "estop_recovery"
title = "estop_recovery"
path = "examples/recovery_templates/estop_recovery.plc"
kind = "template"
purpose = "Emergency-stop recovery template."
"#,
    )
    .expect("catalog should be written");

    let state = test_state(workspace_root, std::collections::BTreeMap::new());

    let Json(payload) = list_project_templates(State(state.clone())).await;
    let categories = payload["categories"].as_array().expect("categories array");
    assert_eq!(categories.len(), 1);
    assert_eq!(
        categories[0]["category"],
        json!("05 Safety, Recovery, And Diagnostics")
    );
    assert_eq!(categories[0]["templates"][0]["id"], json!("estop_recovery"));
    assert_eq!(categories[0]["templates"][0]["type"], json!("template"));

    let Json(source) = get_project_source(State(state), Path("estop_recovery".to_string()))
        .await
        .expect("catalog template source should load by id");
    assert_eq!(
        source["path"],
        json!("examples/recovery_templates/estop_recovery.plc")
    );
}

#[tokio::test]
async fn get_project_source_reads_nested_template_slug() {
    let workspace_root = temp_workspace_root("project-source-template");
    let examples = workspace_root.join("examples/recovery_templates");
    fs::create_dir_all(&examples).expect("nested examples dir should be created");
    fs::write(
        examples.join("estop_recovery.plc"),
        "[topology]\n// estop\n",
    )
    .expect("nested template should be written");
    let state = test_state(workspace_root, std::collections::BTreeMap::new());

    let Json(payload) = get_project_source(
        State(state),
        Path("recovery_templates_estop_recovery".to_string()),
    )
    .await
    .expect("nested template source should load by slug");

    assert_eq!(
        payload["path"],
        json!("examples/recovery_templates/estop_recovery.plc")
    );
    assert_eq!(payload["content"], json!("[topology]\n// estop\n"));
}

#[tokio::test]
async fn get_project_source_rejects_path_traversal_id() {
    let state = test_state(
        temp_workspace_root("project-source-reject"),
        std::collections::BTreeMap::new(),
    );

    let err = get_project_source(State(state), Path("../secret".to_string()))
        .await
        .expect_err("unsafe id should be rejected");
    assert_eq!(err.0, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn parse_plc_topology_returns_relation_port_and_tag_metadata() {
    let plc = r#"
[topology]
device plc_main: plc {
    purpose: "fixture plc"
    model_ref: rp2040_softplc
}
device valve_A: solenoid_valve {
    purpose: "fixture valve"
    ports: [coil:digital:consumer, feedback:logical:producer]
    tags: {
        functional_group: [actuation]
        danger_level: [high]
        location_group: ["line_a/cell_2/station_7"]
    }
}
device sensor_A: sensor {
    purpose: "fixture sensor"
    ports: [sense:logical:consumer, out:digital:producer]
}

relation { from: plc_main.Y0, to: valve_A.coil, via: driven_by }
relation { from: valve_A.feedback, to: sensor_A.sense, via: detects }
relation { from: sensor_A.out, to: plc_main.X0, via: reports_to }

[constraints]

[tasks]
task main:
    step idle:
        action: log "ok"
"#;

    let response = parse_plc_topology(Json(ParsePlcTopologyRequest {
        content: plc.to_string(),
    }))
    .await
    .expect("parse-plc API should succeed")
    .0;
    assert!(response.get("semantic_gate").is_some());

    let valve = find_component(&response, "valve_A");
    assert_eq!(
        valve["params"]["ports"],
        json!([
            {"id": "coil", "type": "digital", "role": "consumer"},
            {"id": "feedback", "type": "logical", "role": "producer"}
        ])
    );
    assert_eq!(
        valve["params"]["tags"],
        json!({
            "functional_group": ["actuation"],
            "danger_level": ["high"],
            "location_group": ["line_a/cell_2/station_7"]
        })
    );
    let detects = response["connections"]
        .as_array()
        .and_then(|connections| {
            connections.iter().find(|connection| {
                connection.get("relation").and_then(Value::as_str) == Some("detects")
                    && connection.get("from").and_then(Value::as_str) == Some("valve_A")
                    && connection.get("to").and_then(Value::as_str) == Some("sensor_A")
            })
        })
        .expect("detects connection should exist");
    assert_eq!(detects["signal"], json!("feedback"));
    assert_eq!(detects["from_port"], json!("feedback"));
    assert_eq!(detects["to_port"], json!("sense"));
}

#[tokio::test]
async fn parse_plc_topology_keeps_extended_and_retracted_edges_distinct() {
    let plc = r#"
[topology]
device plc_main: plc {
    purpose: "topology parse fixture controller"
    model_ref: rp2040_softplc
}

device cyl_A: cylinder {
    purpose: "fixture actuator"
}

device cyl_B: cylinder {
    purpose: "fixture actuator"
}

device sensor_A_ext: sensor {
    purpose: "fixture sensor"
}

device sensor_A_ret: sensor {
    purpose: "fixture sensor"
}

device sensor_B_ext: sensor {
    purpose: "fixture sensor"
}

device sensor_B_ret: sensor {
    purpose: "fixture sensor"
}

relation { from: cyl_A.extended, to: sensor_A_ext.sense, via: detects }
relation { from: cyl_A.retracted, to: sensor_A_ret.sense, via: detects }
relation { from: cyl_B.extended, to: sensor_B_ext.sense, via: detects }
relation { from: cyl_B.retracted, to: sensor_B_ret.sense, via: detects }

[constraints]

[tasks]
task main:
    step idle:
        action: log "ok"
"#
    .to_string();

    let response = parse_plc_topology(Json(ParsePlcTopologyRequest { content: plc }))
        .await
        .expect("parse-plc API should parse fixture")
        .0;
    assert_eq!(
        response["semantic_gate"]["valid"],
        json!(true),
        "fixture should pass topology semantic gate"
    );

    assert!(
        has_detects_connection(&response, "cyl_A", "sensor_A_ext", "extended"),
        "should keep cyl_A.extended detects edge"
    );
    assert!(
        has_detects_connection(&response, "cyl_A", "sensor_A_ret", "retracted"),
        "should keep cyl_A.retracted detects edge"
    );
    assert!(
        has_detects_connection(&response, "cyl_B", "sensor_B_ext", "extended"),
        "should keep cyl_B.extended detects edge"
    );
    assert!(
        has_detects_connection(&response, "cyl_B", "sensor_B_ret", "retracted"),
        "should keep cyl_B.retracted detects edge"
    );
}

#[test]
fn plc_diagnostics_accepts_valid_program_and_reports_summary() {
    let plc = r#"
[topology]
device plc_main: plc {
    purpose: "diagnostics fixture controller"
    model_ref: rp2040_softplc
}
device sensor_A: sensor {
    purpose: "diagnostics fixture sensor"
}

[constraints]

[tasks]
task main:
    step idle:
        action: log "ok"
"#;

    let response = build_plc_diagnostics(&repo_root(), plc);
    assert!(
        response.valid,
        "valid PLC diagnostics should pass: {:?}",
        response.errors
    );
    assert_eq!(response.stage, "verification");
    assert_eq!(response.summary.topology_devices, 1);
    assert_eq!(response.summary.tasks, 1);
    assert!(response.summary.states >= 1);
}

#[test]
fn web_diagnostics_matches_shared_semantic_compile_service() {
    use rust_plc::device_library::DeviceLibrary;
    use rust_plc::parser::parse_plc;
    use rust_plc::semantic::compile_semantic_program_with_library;
    use rust_plc::verification::verify_all;

    let plc = r#"
[topology]
device plc_main: plc {
    purpose: "shared compile fixture controller"
    model_ref: rp2040_softplc
}

[constraints]

[tasks]
task main:
    step idle:
        delay: 10ms
        action: log "ok"
"#;

    let response = build_plc_diagnostics(&repo_root(), plc);
    let program = parse_plc(plc).expect("fixture should parse");
    let library =
        DeviceLibrary::load(&repo_root().join("devices")).expect("device library should load");
    let semantic =
        compile_semantic_program_with_library(&program, (!library.is_empty()).then_some(&library))
            .expect("shared semantic compile should succeed");
    let shared_valid = verify_all(
        &semantic.expanded_program,
        &semantic.topology,
        &semantic.constraints,
        &semantic.state_machine,
    )
    .is_ok();

    assert_eq!(response.valid, shared_valid);
    assert_eq!(response.summary.states, semantic.state_machine.states.len());
    assert_eq!(
        response.summary.topology_devices,
        semantic.topology.graph.node_count()
    );
}

#[test]
fn flowchart_generator_emits_diagnostic_checked_plc() {
    let request = FlowchartGeneratePlcRequest {
        project_id: Some("fixture".to_string()),
        task_name: "Main Cycle".to_string(),
        steps: vec![
            FlowchartEditorStep {
                id: "Start".to_string(),
                label: Some("Start conveyor".to_string()),
                action: Some("start conveyor".to_string()),
                delay_ms: None,
            },
            FlowchartEditorStep {
                id: "Done".to_string(),
                label: Some("Done".to_string()),
                action: Some("cycle complete".to_string()),
                delay_ms: None,
            },
        ],
        transitions: vec![FlowchartEditorTransition {
            from: "Start".to_string(),
            to: "Done".to_string(),
            guard: None,
        }],
    };

    let generated = generate_plc_from_flowchart(&request).expect("flowchart should generate");
    assert_eq!(generated.normalized_task_name, "main_cycle");
    assert!(generated.source.contains("task main_cycle:"));
    assert!(generated.source.contains("step start:"));
    assert!(generated.source.contains("goto main_cycle.done"));

    let diagnostics = build_plc_diagnostics(&repo_root(), &generated.source);
    assert!(
        diagnostics.valid,
        "generated PLC should pass diagnostics: {:?}",
        diagnostics.errors
    );
    assert_eq!(diagnostics.summary.tasks, 1);
}

#[test]
fn flowchart_generator_emits_guarded_branch_with_default_edge() {
    let request = FlowchartGeneratePlcRequest {
        project_id: Some("fixture".to_string()),
        task_name: "Branch Cycle".to_string(),
        steps: vec![
            FlowchartEditorStep {
                id: "check".to_string(),
                label: Some("Check condition".to_string()),
                action: Some("check branch".to_string()),
                delay_ms: None,
            },
            FlowchartEditorStep {
                id: "pass".to_string(),
                label: Some("Pass".to_string()),
                action: Some("pass path".to_string()),
                delay_ms: None,
            },
            FlowchartEditorStep {
                id: "fallback".to_string(),
                label: Some("Fallback".to_string()),
                action: Some("fallback path".to_string()),
                delay_ms: None,
            },
        ],
        transitions: vec![
            FlowchartEditorTransition {
                from: "check".to_string(),
                to: "pass".to_string(),
                guard: Some("true == true".to_string()),
            },
            FlowchartEditorTransition {
                from: "check".to_string(),
                to: "fallback".to_string(),
                guard: None,
            },
        ],
    };

    let generated = generate_plc_from_flowchart(&request).expect("guarded branch should generate");
    assert!(generated
        .source
        .contains("if: true == true goto branch_cycle.pass else: goto branch_cycle.fallback"));

    let diagnostics = build_plc_diagnostics(&repo_root(), &generated.source);
    assert!(
        diagnostics.valid,
        "generated guarded PLC should pass diagnostics: {:?}",
        diagnostics.errors
    );
}

#[test]
fn flowchart_generator_emits_step_delay() {
    let request = FlowchartGeneratePlcRequest {
        project_id: Some("fixture".to_string()),
        task_name: "Timed Cycle".to_string(),
        steps: vec![
            FlowchartEditorStep {
                id: "heat".to_string(),
                label: Some("Heat".to_string()),
                action: Some("heat station".to_string()),
                delay_ms: Some(75),
            },
            FlowchartEditorStep {
                id: "done".to_string(),
                label: Some("Done".to_string()),
                action: Some("cycle done".to_string()),
                delay_ms: None,
            },
        ],
        transitions: vec![FlowchartEditorTransition {
            from: "heat".to_string(),
            to: "done".to_string(),
            guard: None,
        }],
    };

    let generated = generate_plc_from_flowchart(&request).expect("timed flowchart should generate");
    assert!(generated.source.contains("        delay: 75ms\n"));

    let diagnostics = build_plc_diagnostics(&repo_root(), &generated.source);
    assert!(
        diagnostics.valid,
        "generated timed PLC should pass diagnostics: {:?}\n{}",
        diagnostics.errors, generated.source
    );
}

#[test]
fn flowchart_generator_lowers_multiple_guarded_branches_to_decision_steps() {
    let request = FlowchartGeneratePlcRequest {
        project_id: Some("fixture".to_string()),
        task_name: "Decision Cycle".to_string(),
        steps: vec![
            FlowchartEditorStep {
                id: "check".to_string(),
                label: Some("Check route".to_string()),
                action: Some("check route".to_string()),
                delay_ms: None,
            },
            FlowchartEditorStep {
                id: "route_a".to_string(),
                label: Some("Route A".to_string()),
                action: Some("route a".to_string()),
                delay_ms: None,
            },
            FlowchartEditorStep {
                id: "route_b".to_string(),
                label: Some("Route B".to_string()),
                action: Some("route b".to_string()),
                delay_ms: None,
            },
            FlowchartEditorStep {
                id: "fallback".to_string(),
                label: Some("Fallback".to_string()),
                action: Some("fallback path".to_string()),
                delay_ms: None,
            },
        ],
        transitions: vec![
            FlowchartEditorTransition {
                from: "check".to_string(),
                to: "route_a".to_string(),
                guard: Some("mode_a == true".to_string()),
            },
            FlowchartEditorTransition {
                from: "check".to_string(),
                to: "route_b".to_string(),
                guard: Some("mode_b == true".to_string()),
            },
            FlowchartEditorTransition {
                from: "check".to_string(),
                to: "fallback".to_string(),
                guard: None,
            },
        ],
    };

    let generated =
        generate_plc_from_flowchart(&request).expect("multi-guard branch should generate");
    assert!(generated.source.contains(
        "if: mode_a == true goto decision_cycle.route_a else: goto decision_cycle.check_branch_2"
    ));
    assert!(generated.source.contains("step check_branch_2:"));
    assert!(generated.source.contains(
        "if: mode_b == true goto decision_cycle.route_b else: goto decision_cycle.fallback"
    ));

    let diagnostics = build_plc_diagnostics(&repo_root(), &generated.source);
    assert!(
        diagnostics.valid,
        "generated multi-guard PLC should pass diagnostics: {:?}\n{}",
        diagnostics.errors, generated.source
    );
}

#[test]
fn flowchart_generator_rejects_guard_without_default_edge() {
    let request = FlowchartGeneratePlcRequest {
        project_id: None,
        task_name: "main".to_string(),
        steps: vec![
            FlowchartEditorStep {
                id: "a".to_string(),
                label: None,
                action: None,
                delay_ms: None,
            },
            FlowchartEditorStep {
                id: "b".to_string(),
                label: None,
                action: None,
                delay_ms: None,
            },
        ],
        transitions: vec![FlowchartEditorTransition {
            from: "a".to_string(),
            to: "b".to_string(),
            guard: Some("sensor_a == true".to_string()),
        }],
    };

    let err = generate_plc_from_flowchart(&request)
        .expect_err("guarded flowchart branch without default edge should be rejected");
    assert!(err.contains("requires one unguarded default transition"));
}

#[test]
fn flowchart_generator_rejects_empty_guard_expression() {
    let request = FlowchartGeneratePlcRequest {
        project_id: None,
        task_name: "main".to_string(),
        steps: vec![
            FlowchartEditorStep {
                id: "a".to_string(),
                label: None,
                action: None,
                delay_ms: None,
            },
            FlowchartEditorStep {
                id: "b".to_string(),
                label: None,
                action: None,
                delay_ms: None,
            },
            FlowchartEditorStep {
                id: "fallback".to_string(),
                label: None,
                action: None,
                delay_ms: None,
            },
        ],
        transitions: vec![
            FlowchartEditorTransition {
                from: "a".to_string(),
                to: "b".to_string(),
                guard: Some("   ".to_string()),
            },
            FlowchartEditorTransition {
                from: "a".to_string(),
                to: "fallback".to_string(),
                guard: None,
            },
        ],
    };

    let err = generate_plc_from_flowchart(&request)
        .expect_err("blank guarded flowchart branch should be rejected");
    assert!(err.contains("requires a non-empty guard expression"));
}

#[tokio::test]
async fn plc_language_snapshot_exposes_lsp_symbols_and_completions() {
    let plc = include_str!("../../../examples/demo.plc");

    let Json(response) = plc_language_snapshot(Json(PlcLanguageRequest {
        content: plc.to_string(),
    }))
    .await;

    assert!(
        response
            .symbols
            .iter()
            .any(|symbol| symbol.qualified_name == "cyl_a"),
        "language snapshot should include compiler-owned symbols"
    );
    assert!(
        response
            .completions
            .iter()
            .any(|completion| completion.label == "device block" && completion.snippet),
        "language snapshot should include LSP snippet completions"
    );
}

#[tokio::test]
async fn dsl_capabilities_endpoint_reports_supported_and_unsupported_contracts() {
    let Json(response) = dsl_capabilities().await;

    assert_eq!(response.schema_version, 1);
    assert!(response.supported_features.iter().any(|feature| {
        feature.id == "station_protocols" && feature.layer == "semantic_ir_verification"
    }));
    assert!(response
        .template_assets
        .iter()
        .any(|asset| { asset.id == "recovery_templates" && asset.status == "asset_template" }));
    assert!(response.supported_features.iter().any(|feature| {
        feature.id == "generic_task_templates" && feature.layer == "preprocess_semantic_ir"
    }));
}

#[test]
fn plc_realtime_response_combines_diagnostics_and_language_snapshot() {
    let plc = r#"
[topology]
device plc_main: plc {
    purpose: "realtime fixture controller"
    model_ref: rp2040_softplc
}
device sensor_A: sensor {
    purpose: "realtime fixture sensor"
}

[constraints]

[tasks]
task main:
    step idle:
        action: log "ok"
"#;

    let response = build_plc_realtime_response(
        &repo_root(),
        PlcRealtimeRequest {
            content: plc.to_string(),
            request_id: Some(7),
        },
    );

    assert_eq!(response.request_id, Some(7));
    assert!(
        response.diagnostics.valid,
        "demo fixture should be valid: {:?}",
        response.diagnostics.errors
    );
    assert!(
        !response.language.symbols.is_empty(),
        "realtime response should include LSP symbols"
    );
}

#[test]
fn collab_event_wraps_client_payload_with_room_metadata() {
    let event = build_collab_event(
        "demo",
        CollabClientEvent {
            kind: "edit".to_string(),
            client_id: "client-a".to_string(),
            user_name: Some("engineer".to_string()),
            content: Some("[topology]\n".to_string()),
            revision: Some(3),
            cursor_line: Some(1),
            cursor_column: Some(4),
            comment: None,
        },
    );

    assert_eq!(event.room, "demo");
    assert_eq!(event.kind, "edit");
    assert_eq!(event.client_id, "client-a");
    assert_eq!(event.user_name.as_deref(), Some("engineer"));
    assert_eq!(event.revision, Some(3));
    assert!(event.at_ms > 0);
}

#[tokio::test]
async fn collab_comment_history_replays_only_recent_comment_events() {
    let state = test_state(PathBuf::from("E:/workspace"), BTreeMap::new());

    let edit_event = build_collab_event(
        "review",
        CollabClientEvent {
            kind: "edit".to_string(),
            client_id: "client-a".to_string(),
            user_name: None,
            content: Some("[tasks]\n".to_string()),
            revision: Some(1),
            cursor_line: None,
            cursor_column: None,
            comment: None,
        },
    );
    record_collab_comment(&state, &edit_event).await;

    for index in 0..55 {
        let event = build_collab_event(
            "review",
            CollabClientEvent {
                kind: "comment".to_string(),
                client_id: format!("client-{index}"),
                user_name: Some("engineer".to_string()),
                content: None,
                revision: Some(index),
                cursor_line: Some(index as usize + 1),
                cursor_column: Some(1),
                comment: Some(format!("note-{index}")),
            },
        );
        record_collab_comment(&state, &event).await;
    }

    let history = collab_comment_history(&state, "review").await;
    assert_eq!(history.len(), MAX_COLLAB_COMMENT_HISTORY);
    assert!(history.iter().all(|event| event.kind == "comment"));
    assert_eq!(
        history.first().and_then(|event| event.comment.as_deref()),
        Some("note-5")
    );
    assert_eq!(
        history.last().and_then(|event| event.comment.as_deref()),
        Some("note-54")
    );
    assert!(collab_comment_history(&state, "other").await.is_empty());
}

#[tokio::test]
async fn collab_comment_history_reloads_from_workspace_storage() {
    let workspace_root = temp_workspace_root("collab-comments-persist");
    let state = test_state(workspace_root.clone(), BTreeMap::new());

    let event = build_collab_event(
        "review",
        CollabClientEvent {
            kind: "comment".to_string(),
            client_id: "client-a".to_string(),
            user_name: Some("engineer".to_string()),
            content: None,
            revision: Some(7),
            cursor_line: Some(12),
            cursor_column: Some(4),
            comment: Some("verify guard branch".to_string()),
        },
    );
    record_collab_comment(&state, &event).await;

    let fresh_state = test_state(workspace_root, BTreeMap::new());
    let history = collab_comment_history(&fresh_state, "review").await;

    assert_eq!(history.len(), 1);
    assert_eq!(
        history.first().and_then(|event| event.comment.as_deref()),
        Some("verify guard branch")
    );
    assert_eq!(
        history.first().and_then(|event| event.cursor_line),
        Some(12)
    );
}

#[tokio::test]
async fn collab_comment_storage_keeps_only_recent_comments() {
    let workspace_root = temp_workspace_root("collab-comments-trim");
    let state = test_state(workspace_root.clone(), BTreeMap::new());

    for index in 0..55 {
        let event = build_collab_event(
            "review",
            CollabClientEvent {
                kind: "comment".to_string(),
                client_id: format!("client-{index}"),
                user_name: None,
                content: None,
                revision: Some(index),
                cursor_line: Some(index as usize + 1),
                cursor_column: Some(1),
                comment: Some(format!("persisted-note-{index}")),
            },
        );
        record_collab_comment(&state, &event).await;
    }

    let path = workspace_root
        .join("out")
        .join(COLLAB_COMMENT_HISTORY_DIR)
        .join("review.json");
    let text = fs::read_to_string(path).expect("persisted comment history should exist");
    let persisted = serde_json::from_str::<Value>(&text).expect("history should be json");
    let comments = persisted.as_array().expect("history should be an array");

    assert_eq!(comments.len(), MAX_COLLAB_COMMENT_HISTORY);
    assert_eq!(comments[0]["comment"], json!("persisted-note-5"));
    assert_eq!(comments[49]["comment"], json!("persisted-note-54"));
}

#[test]
fn collab_room_id_rejects_path_like_names() {
    assert!(is_safe_collab_room("demo"));
    assert!(is_safe_collab_room("line_a.cell_2"));
    assert!(!is_safe_collab_room("../demo"));
    assert!(!is_safe_collab_room("line/a"));
    assert!(!is_safe_collab_room(""));
}

#[test]
fn plc_diagnostics_returns_topology_gate_issue_for_missing_purpose() {
    let plc = r#"
[topology]
device sensor_A: sensor {
    tags: { functional_group: [diagnostics] }
}

[constraints]

[tasks]
task main:
    step idle:
        action: log "ok"
"#;

    let response = build_plc_diagnostics(&repo_root(), plc);
    assert!(!response.valid);
    assert_eq!(response.stage, "topology_gate");
    assert!(response
        .issues
        .iter()
        .any(|issue| issue.code.as_deref() == Some("SEM-107")));
}

#[test]
fn build_geometry_export_args_includes_optional_overlay_paths() {
    let workspace_root = PathBuf::from("E:/workspace");
    let out_path = workspace_root.join("out/web_runs/run-1/geometry.json");
    let args = build_geometry_export_args(
        &workspace_root,
        &out_path,
        "examples/demo.plc",
        Some("/artifacts/web_runs/run-1/sil_trace.jsonl"),
        Some("/artifacts/web_runs/run-1/intent_alignment/report.json"),
    );

    assert_eq!(args[0], "geometry-export");
    assert!(
        args.contains(
            &workspace_root
                .join("examples/demo.plc")
                .display()
                .to_string()
        ),
        "expected absolute plc path in args: {args:?}"
    );
    assert!(args.contains(&"--trace".to_string()));
    assert!(args.iter().any(|arg| {
        PathBuf::from(arg).ends_with(PathBuf::from("out/web_runs/run-1/sil_trace.jsonl"))
    }));
    assert!(args.contains(&"--intent-report".to_string()));
    assert!(args.iter().any(|arg| {
        PathBuf::from(arg).ends_with(PathBuf::from(
            "out/web_runs/run-1/intent_alignment/report.json",
        ))
    }));
}

#[test]
fn resolve_artifact_reference_understands_geometry_artifact_href() {
    let workspace_root = PathBuf::from("E:/workspace");
    let resolved =
        resolve_artifact_reference(&workspace_root, "/artifacts/web_runs/run-1/geometry.json")
            .expect("geometry artifact href should resolve");
    assert_eq!(
        resolved,
        workspace_root.join("out/web_runs/run-1/geometry.json")
    );
}

#[tokio::test]
async fn get_geometry_returns_missing_payload_when_run_has_no_geometry_artifact() {
    let state = test_state(
        PathBuf::from("E:/workspace"),
        std::collections::BTreeMap::from([(
            "run-1".to_string(),
            RunRecord {
                run_id: "run-1".to_string(),
                status: "pass".to_string(),
                triggered_by: "test".to_string(),
                triggered_at: "0".to_string(),
                triggered_at_ms: 0,
                mode: "no_board_gate".to_string(),
                artifacts: RunArtifacts::default(),
                failure_summary: None,
                plc_file: Some("examples/demo.plc".to_string()),
                scenario_file: Some("scenarios/demo.yaml".to_string()),
                topology_file: None,
                tick_ms: Some(10),
            },
        )]),
    );

    let Json(payload) = get_geometry(State(state), Path("run-1".to_string()))
        .await
        .expect("geometry endpoint should return a payload");
    assert_eq!(payload["status"], json!("missing"));
    assert_eq!(payload["artifact_kind"], json!("semantic_twin_geometry"));
}

#[tokio::test]
async fn get_trace_returns_empty_ticks_when_run_has_no_trace_artifact() {
    let state = test_state(
        PathBuf::from("E:/workspace"),
        std::collections::BTreeMap::from([(
            "run-1".to_string(),
            RunRecord {
                run_id: "run-1".to_string(),
                status: "pass".to_string(),
                triggered_by: "test".to_string(),
                triggered_at: "0".to_string(),
                triggered_at_ms: 0,
                mode: "no_board_gate".to_string(),
                artifacts: RunArtifacts::default(),
                failure_summary: None,
                plc_file: Some("examples/demo.plc".to_string()),
                scenario_file: Some("examples/demo.scenario.json".to_string()),
                topology_file: None,
                tick_ms: Some(10),
            },
        )]),
    );

    let Json(payload) = get_trace(State(state), Path("run-1".to_string()))
        .await
        .expect("trace endpoint should return a payload");
    assert_eq!(payload["tick_ms"], json!(10));
    assert_eq!(payload["ticks"], json!([]));
}

#[tokio::test]
async fn get_trace_converts_component_trace_jsonl_into_replay_ticks() {
    let workspace_root = temp_workspace_root("component-trace");
    let trace_path = workspace_root.join("out/web_runs/run-1/component_trace.jsonl");
    fs::create_dir_all(trace_path.parent().expect("trace parent")).expect("trace dir");
    fs::write(
            &trace_path,
            concat!(
                "{\"tick\":0,\"components\":{\"s_start\":{\"state\":\"on\",\"component_type\":\"switch\"}}}\n",
                "{\"tick\":1,\"components\":{\"m1\":{\"state\":\"enabled\",\"component_type\":\"stepper_pd\",\"outputs\":{\"position_steps\":42}}}}\n"
            ),
        )
        .expect("component trace should be written");

    let state = test_state(
        workspace_root.clone(),
        std::collections::BTreeMap::from([(
            "run-1".to_string(),
            RunRecord {
                run_id: "run-1".to_string(),
                status: "pass".to_string(),
                triggered_by: "test".to_string(),
                triggered_at: "0".to_string(),
                triggered_at_ms: 0,
                mode: "component_sim".to_string(),
                artifacts: RunArtifacts {
                    trace: Some("/artifacts/web_runs/run-1/component_trace.jsonl".to_string()),
                    ..RunArtifacts::default()
                },
                failure_summary: None,
                plc_file: None,
                scenario_file: Some("examples/component_model/scenario_normal.json".to_string()),
                topology_file: Some("examples/component_model/topology.json".to_string()),
                tick_ms: Some(20),
            },
        )]),
    );

    let Json(payload) = get_trace(State(state), Path("run-1".to_string()))
        .await
        .expect("trace endpoint should parse component trace");

    assert_eq!(payload["tick_ms"], json!(20));
    assert_eq!(payload["ticks"][0]["tick"], json!(0));
    assert_eq!(
        payload["ticks"][0]["component_states"]["s_start"]["state"],
        json!("on")
    );
    assert_eq!(
        payload["ticks"][1]["component_states"]["m1"]["outputs"]["position_steps"],
        json!(42)
    );
}

#[tokio::test]
async fn get_trace_prefers_io_snapshot_sidecar_for_sil_trace_replay() {
    let workspace_root = temp_workspace_root("io-snapshot-replay");
    let trace_path = workspace_root.join("out/web_runs/run-1/sil_trace.jsonl");
    let io_snapshot_path = workspace_root.join("out/web_runs/run-1/io_snapshot.json");
    fs::create_dir_all(trace_path.parent().expect("trace parent")).expect("trace dir");
    fs::write(
        &trace_path,
        "{\"tick\":0,\"task\":0,\"from_step\":0,\"to_step\":1,\"reason\":\"action\"}\n",
    )
    .expect("sil trace should be written");
    fs::write(
        &io_snapshot_path,
        serde_json::to_string_pretty(&json!({
            "schema_version": 1,
            "tick_ms": 25,
            "ticks": [
                {
                    "tick": 0,
                    "digital_inputs": [false, true],
                    "analog_inputs": [],
                    "digital_outputs": [true],
                    "analog_outputs": []
                },
                {
                    "tick": 1,
                    "digital_inputs": [true, true],
                    "analog_inputs": [],
                    "digital_outputs": [true],
                    "analog_outputs": []
                }
            ]
        }))
        .expect("serialize io snapshot"),
    )
    .expect("io snapshot should be written");

    let state = test_state(
        workspace_root.clone(),
        std::collections::BTreeMap::from([(
            "run-1".to_string(),
            RunRecord {
                run_id: "run-1".to_string(),
                status: "pass".to_string(),
                triggered_by: "test".to_string(),
                triggered_at: "0".to_string(),
                triggered_at_ms: 0,
                mode: "no_board_gate".to_string(),
                artifacts: RunArtifacts {
                    trace: Some("/artifacts/web_runs/run-1/sil_trace.jsonl".to_string()),
                    ..RunArtifacts::default()
                },
                failure_summary: None,
                plc_file: Some("examples/demo.plc".to_string()),
                scenario_file: Some("examples/demo.scenario.json".to_string()),
                topology_file: None,
                tick_ms: Some(25),
            },
        )]),
    );

    let Json(payload) = get_trace(State(state), Path("run-1".to_string()))
        .await
        .expect("trace endpoint should prefer io snapshot");

    assert_eq!(payload["tick_ms"], json!(25));
    assert_eq!(payload["ticks"][0]["digital_inputs"][1], json!(true));
    assert_eq!(payload["ticks"][1]["tick"], json!(1));
}

#[tokio::test]
async fn get_trace_range_filters_ticks_in_jsonl_trace() {
    let workspace_root = temp_workspace_root("trace-range");
    let trace_path = workspace_root.join("out/web_runs/run-1/sil_trace.jsonl");
    fs::create_dir_all(trace_path.parent().expect("trace parent")).expect("trace dir");
    fs::write(
            &trace_path,
            concat!(
                "{\"tick\":0,\"digital_inputs\":[false],\"analog_inputs\":[],\"digital_outputs\":[false],\"analog_outputs\":[]}\n",
                "{\"tick\":1,\"digital_inputs\":[true],\"analog_inputs\":[],\"digital_outputs\":[true],\"analog_outputs\":[]}\n",
                "{\"tick\":2,\"digital_inputs\":[false],\"analog_inputs\":[],\"digital_outputs\":[false],\"analog_outputs\":[]}\n"
            ),
        )
        .expect("trace should be written");

    let state = test_state(
        workspace_root.clone(),
        std::collections::BTreeMap::from([(
            "run-1".to_string(),
            RunRecord {
                run_id: "run-1".to_string(),
                status: "pass".to_string(),
                triggered_by: "test".to_string(),
                triggered_at: "0".to_string(),
                triggered_at_ms: 0,
                mode: "no_board_gate".to_string(),
                artifacts: RunArtifacts {
                    trace: Some("/artifacts/web_runs/run-1/sil_trace.jsonl".to_string()),
                    ..RunArtifacts::default()
                },
                failure_summary: None,
                plc_file: Some("examples/demo.plc".to_string()),
                scenario_file: Some("examples/demo.scenario.json".to_string()),
                topology_file: None,
                tick_ms: Some(10),
            },
        )]),
    );

    let Json(payload) = get_trace_range(
        State(state),
        Path("run-1".to_string()),
        Query(TickRangeQuery {
            start: Some(1),
            end: Some(1),
        }),
    )
    .await
    .expect("trace range endpoint should filter trace");

    assert_eq!(
        payload["ticks"].as_array().map(|ticks| ticks.len()),
        Some(1)
    );
    assert_eq!(payload["ticks"][0]["tick"], json!(1));
}

#[tokio::test]
async fn get_keypoints_returns_empty_payload_when_artifact_is_missing() {
    let state = test_state(
        PathBuf::from("E:/workspace"),
        std::collections::BTreeMap::from([(
            "run-1".to_string(),
            RunRecord {
                run_id: "run-1".to_string(),
                status: "pass".to_string(),
                triggered_by: "test".to_string(),
                triggered_at: "0".to_string(),
                triggered_at_ms: 0,
                mode: "component_sim".to_string(),
                artifacts: RunArtifacts::default(),
                failure_summary: None,
                plc_file: None,
                scenario_file: Some("examples/component_model/scenario_normal.json".to_string()),
                topology_file: Some("examples/component_model/topology.json".to_string()),
                tick_ms: Some(10),
            },
        )]),
    );

    let Json(payload) = get_keypoints(State(state), Path("run-1".to_string()))
        .await
        .expect("keypoints endpoint should return fallback payload");
    assert_eq!(payload["tick_ms"], json!(10));
    assert_eq!(payload["keypoints"], json!([]));
}

fn write_delivery_project_fixture(workspace_root: &std::path::Path) {
    let project_root = workspace_root.join("delivery-projects/demo-line");
    fs::create_dir_all(project_root.join("plc")).expect("PLC directory should exist");
    fs::create_dir_all(project_root.join("out/agent-runs/run-1/compile"))
        .expect("run compile directory should exist");
    fs::create_dir_all(project_root.join("out/agent-runs/run-1/project-check"))
        .expect("project-check directory should exist");
    fs::create_dir_all(project_root.join("out/wiring")).expect("wiring directory should exist");
    fs::create_dir_all(project_root.join("release")).expect("release directory should exist");

    fs::write(
        project_root.join("delivery-project.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "project_id": "line.demo",
            "delivery_layer": "line",
            "source_commit": "0123456789abcdef",
            "source_entry": "plc/main.bundle.toml",
            "system_contract": "plc/main.system.md",
            "artifact_roots": {
                "agent_runs": "out/agent-runs",
                "verification": "out/agent-runs/run-1",
                "wiring": "out/wiring",
                "release": "release"
            },
            "fixtures": {
                "human_holds": { "fixture_ref": "release/human-holds.json" }
            }
        }))
        .expect("manifest should serialize"),
    )
    .expect("manifest should be written");
    fs::write(
        project_root.join("plc/main.bundle.toml"),
        "schema_version = 1\n",
    )
    .expect("source entry should be written");
    fs::write(
        project_root.join("plc/main.system.md"),
        "# Demo delivery system\n",
    )
    .expect("system contract should be written");
    fs::write(project_root.join("plc/trace.jsonl"), "{\"tick\":1}\n")
        .expect("trace fixture should be written");
    fs::write(
        project_root.join("out/agent-runs/run-1/input-manifest.json"),
        "{\"schema_version\":1}\n",
    )
    .expect("input manifest should be written");
    fs::write(
        project_root.join("out/agent-runs/run-1/result.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "run_id": "run-1",
            "harness_execution_id": "run-1",
            "artifact_root": "out/agent-runs/run-1",
            "git_head": "0123456789abcdef",
            "status": { "delivery": "pass" },
            "inputs": { "manifest": "out/agent-runs/run-1/input-manifest.json" },
            "digests": { "input_manifest_sha256": "0000000000000000000000000000000000000000000000000000000000000000" },
            "steps": [{
                "name": "compile_verify",
                "classification": "pass",
                "exit_code": 0,
                "elapsed_ms": 12,
                "artifacts": ["out/agent-runs/run-1/compile/verification_report.json"]
            }],
            "known_gaps": [{
                "id": "GAP-DEMO",
                "layer": "runtime",
                "classification": "code-gap",
                "evidence": "fixture blocker"
            }]
        }))
        .expect("result should serialize"),
    )
    .expect("result should be written");
    fs::write(
        project_root.join("out/agent-runs/run-1/compile/verification_report.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "verification": {
                "safety": {
                    "level": "pass",
                    "warnings": [{ "level": "warn", "message": "fixture warning" }],
                    "checked_rules": 1,
                    "skipped_rules": 0
                }
            }
        }))
        .expect("verification report should serialize"),
    )
    .expect("verification report should be written");
    fs::write(
        project_root.join("out/agent-runs/run-1/compile/geometry.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "artifact_kind": "semantic_twin_geometry",
            "source_path": "plc/main.bundle.toml",
            "summary": {
                "task_count": 1,
                "step_count": 1,
                "transition_count": 1,
                "device_count": 1,
                "resource_count": 0,
                "timing_rule_count": 0,
                "causality_chain_count": 0,
                "observed_transition_count": 0,
                "intent_mismatch_count": 0
            },
            "lanes": [{ "id": "task", "kind": "task", "label": "Task", "position": 0 }],
            "nodes": [{
                "id": "step:start",
                "kind": "step",
                "label": "start",
                "lane_id": "task",
                "views": ["constellation", "evidence"],
                "evidence_status": "verified"
            }],
            "edges": [{
                "id": "transition:start",
                "kind": "transition",
                "from": "step:start",
                "to": "step:start",
                "label": "complete",
                "views": ["constellation", "evidence"],
                "evidence_status": "derived"
            }],
            "overlays": {}
        }))
        .expect("geometry should serialize"),
    )
    .expect("geometry should be written");
    fs::write(
        project_root.join("out/agent-runs/run-1/project-check/project_check_report.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "status": "pass",
            "steps": [{
                "name": "intent_alignment",
                "status": "pass",
                "exit_code": 0
            }]
        }))
        .expect("project check should serialize"),
    )
    .expect("project check should be written");
    fs::write(
        project_root.join("out/wiring/wiring.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "rows": [{
                "point_id": "plc_main.X0",
                "alias": "start_cycle_cmd",
                "channel": "X0",
                "direction": "input",
                "device_terminal": "start_button.out",
                "signal_type": "digital",
                "safe_state": null,
                "status": "human_action_required"
            }],
            "diagnostics": [{
                "code": "WIR-004",
                "kind": "direction_mismatch",
                "point_id": "plc_main.X0",
                "severity": "error",
                "message": "Fixture wiring direction mismatch"
            }],
            "validation_summary": {
                "status": "fail",
                "error_count": 1,
                "checked_rules": ["direction_mismatch"]
            }
        }))
        .expect("wiring should serialize"),
    )
    .expect("wiring should be written");
    fs::write(
        project_root.join("release/human-holds.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "project_id": "line.demo",
            "source_commit": "0123456789abcdef",
            "holds": [
                { "hold_id": "wiring_review", "required_role": "electrical_engineer", "status": "human_action_required" },
                { "hold_id": "point_check_completion", "required_role": "commissioning_engineer", "status": "human_action_required" },
                { "hold_id": "safety_review", "required_role": "safety_reviewer", "status": "human_action_required" },
                { "hold_id": "hil_review", "required_role": "commissioning_engineer", "status": "human_action_required" },
                { "hold_id": "release_approval", "required_role": "release_approver", "status": "human_action_required" }
            ]
        }))
        .expect("hold contract should serialize"),
    )
    .expect("hold contract should be written");
}

fn write_historical_delivery_run_fixture(workspace_root: &std::path::Path) {
    let run_root = workspace_root.join("delivery-projects/demo-line/out/agent-runs/run-0-old");
    fs::create_dir_all(run_root.join("compile")).expect("old compile directory should exist");
    fs::create_dir_all(run_root.join("project-check"))
        .expect("old project-check directory should exist");
    fs::write(
        run_root.join("input-manifest.json"),
        "{\"schema_version\":1}\n",
    )
    .expect("old input manifest should be written");
    fs::write(
        run_root.join("result.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "run_id": "run-0-old",
            "harness_execution_id": "run-0-old",
            "artifact_root": "out/agent-runs/run-0-old",
            "git_head": "old-commit",
            "status": { "delivery": "fail" },
            "steps": [{
                "name": "old_compile_verify",
                "classification": "fail",
                "exit_code": 1,
                "note": "historical failure must not enter current tests"
            }],
            "known_gaps": [{
                "id": "GAP-OLD-RUN",
                "layer": "runtime",
                "classification": "code-gap",
                "evidence": "historical gap must not enter current problems"
            }]
        }))
        .expect("old result should serialize"),
    )
    .expect("old result should be written");
    fs::write(
        run_root.join("compile/verification_report.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "verification": {
                "safety": {
                    "level": "fail",
                    "warnings": [{
                        "code": "OLD-SAFETY-WARNING",
                        "level": "error",
                        "message": "historical safety result must not overwrite current safety"
                    }],
                    "checked_rules": 99,
                    "skipped_rules": 0
                }
            }
        }))
        .expect("old verification report should serialize"),
    )
    .expect("old verification report should be written");
    fs::write(
        run_root.join("project-check/project_check_report.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "status": "fail",
            "steps": [{
                "name": "old_project_check",
                "status": "fail",
                "exit_code": 1
            }]
        }))
        .expect("old project check should serialize"),
    )
    .expect("old project check should be written");
    fs::write(
        run_root.join("anomalies.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "records": [{
                "anomaly_id": "ANOM-OLD-RUN",
                "status": "blocked",
                "summary": "historical anomaly must not enter current problems"
            }]
        }))
        .expect("old anomalies should serialize"),
    )
    .expect("old anomalies should be written");
}

fn normalized_fixture_sha256(path: &std::path::Path) -> String {
    let text = fs::read_to_string(path).expect("fixture should be readable");
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let digest = Sha256::digest(normalized.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn write_delivery_provenance_v2(
    workspace_root: &std::path::Path,
    attribution_kind: &str,
    include_agent_binding: bool,
    prove_source_authoring: bool,
) {
    let run_root = workspace_root.join("delivery-projects/demo-line/out/agent-runs/run-1");
    let input_manifest = run_root.join("input-manifest.json");
    let (attributed_artifact, attributed_path, task_id) = if prove_source_authoring {
        (
            workspace_root.join("delivery-projects/demo-line/plc/main.bundle.toml"),
            "delivery-projects/demo-line/plc/main.bundle.toml",
            "generate_source",
        )
    } else {
        (
            run_root.join("compile/verification_report.json"),
            "delivery-projects/demo-line/out/agent-runs/run-1/compile/verification_report.json",
            "compile_verify",
        )
    };
    let mut record = json!({
        "path": attributed_path,
        "before_sha256": if prove_source_authoring { json!("0".repeat(64)) } else { Value::Null },
        "after_sha256": normalized_fixture_sha256(&attributed_artifact),
        "attribution_kind": attribution_kind,
        "basis": "test-owned deterministic output"
    });
    if include_agent_binding {
        record["agent_id"] = json!("agent.materializer");
        record["task_id"] = json!(task_id);
        record["event_id"] = json!("EVT-001");
    } else {
        record["event_id"] = json!("");
    }
    fs::write(
        run_root.join("agent-events.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "records": [{ "event_id": "EVT-001", "task": task_id }]
        }))
        .expect("agent events should serialize"),
    )
    .expect("agent events should be written");
    fs::write(
        run_root.join("provenance.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 2,
            "project_id": "line.demo",
            "run_id": "run-1",
            "source_commit": "0123456789abcdef",
            "git_base_commit": "0123456789abcdef",
            "dirty_worktree_at_start": true,
            "started_at_utc": "2026-07-24T00:00:00Z",
            "completed_at_utc": "2026-07-24T00:00:01Z",
            "provenance_scope": if prove_source_authoring { "source_generation" } else { "delivery_fixture_materialization" },
            "execution_unattended_verdict": "proven",
            "source_authoring_verdict": if prove_source_authoring { "proven" } else { "not_proven" },
            "unattended_verdict": if prove_source_authoring { "proven" } else { "not_proven" },
            "event_stream": "delivery-projects/demo-line/out/agent-runs/run-1/agent-events.json",
            "input_manifest": {
                "artifact_ref": "delivery-projects/demo-line/out/agent-runs/run-1/input-manifest.json",
                "digest": {
                    "algorithm": "sha256",
                    "value": normalized_fixture_sha256(&input_manifest),
                    "normalization": "utf8_lf"
                },
                "source_commit": "0123456789abcdef",
                "freshness": { "status": "input_snapshot", "basis": "test snapshot" }
            },
            "models": [{ "role": "materializer", "model": "deterministic_harness" }],
            "agents": [{ "agent_id": "agent.materializer", "model": "deterministic_harness", "role": "materializer" }],
            "task_definitions": [{
                "task_id": task_id,
                "agent_id": "agent.materializer",
                "task_kind": if prove_source_authoring { "source_generation" } else { "validation" }
            }],
            "skills": [{ "name": "agent-harness-project-standard", "version": "v1" }],
            "tool_versions": [{ "name": "rust_plc", "version": "0.1.0" }],
            "file_attribution": {
                "policy_version": "rustplc-file-attribution-v1",
                "human_intervention_detected": attribution_kind == "human_intervention_detected",
                "records": [record],
                "evidence_envelopes": [
                    { "path": "delivery-projects/demo-line/out/agent-runs/run-1/provenance.json", "reason": "self envelope" },
                    { "path": "delivery-projects/demo-line/delivery-project.json", "reason": "external binding" }
                ]
            }
        }))
        .expect("provenance should serialize"),
    )
    .expect("provenance should be written");
}

fn delivery_state_with_user(workspace_root: PathBuf, role: UserRole) -> Arc<AppState> {
    let mut state = test_state(workspace_root, BTreeMap::new());
    Arc::get_mut(&mut state)
        .expect("test state should be uniquely owned")
        .auth = super::auth::AuthService::for_test_user("reviewer", "correct", role);
    state
}

async fn login_delivery_reviewer(app: &axum::Router) -> String {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "username": "reviewer", "password": "correct" }).to_string(),
                ))
                .expect("login request should build"),
        )
        .await
        .expect("login route should respond");
    assert_eq!(response.status(), StatusCode::OK);
    response_json(response).await["token"]
        .as_str()
        .expect("login response should contain a token")
        .to_string()
}

async fn sign_delivery_hold(app: &axum::Router, token: &str, hold_id: &str) -> Value {
    let context = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/delivery-projects/line.demo/holds/signatures")
                .body(Body::empty())
                .expect("signature context request should build"),
        )
        .await
        .expect("signature context should respond");
    assert_eq!(context.status(), StatusCode::OK);
    let context = response_json(context).await;
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/delivery-projects/line.demo/holds/{hold_id}/sign"
                ))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "hold_type": hold_id,
                        "attestation_standard": "internal_engineering_v1",
                        "source_commit": "0123456789abcdef",
                        "evidence_digests": context["current_evidence_digests"],
                        "decision": "approve",
                        "comment": format!("approved {hold_id}")
                    })
                    .to_string(),
                ))
                .expect("signature request should build"),
        )
        .await
        .expect("signature route should respond");
    assert_eq!(response.status(), StatusCode::CREATED, "hold {hold_id}");
    response_json(response).await
}

fn run_git(workspace_root: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace_root)
        .output()
        .expect("git should start");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn find_hold<'a>(payload: &'a Value, hold_id: &str) -> &'a Value {
    payload["holds"]
        .as_array()
        .and_then(|holds| {
            holds
                .iter()
                .find(|hold| hold.get("hold_id").and_then(Value::as_str) == Some(hold_id))
        })
        .expect("hold should exist")
}

#[tokio::test]
async fn delivery_routes_aggregate_manifest_runs_and_evidence() {
    let workspace_root = temp_workspace_root("delivery-routes");
    write_delivery_project_fixture(&workspace_root);
    let state = test_state(workspace_root.clone(), BTreeMap::new());
    let app = build_app(state);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/delivery-projects")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("list response");
    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["projects"][0]["project_id"], json!("line.demo"));
    assert_eq!(
        payload["projects"][0]["evidence_status"]["state"],
        json!("stale")
    );

    let endpoints = [
        "/api/delivery-projects/line.demo",
        "/api/delivery-projects/line.demo/runs",
        "/api/delivery-projects/line.demo/runs/run-1",
        "/api/delivery-projects/line.demo/wiring",
        "/api/delivery-projects/line.demo/verification",
        "/api/delivery-projects/line.demo/evidence",
        "/api/delivery-projects/line.demo/geometry",
        "/api/workspace/problems",
        "/api/workspace/tests",
    ];
    for endpoint in endpoints {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(endpoint)
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("delivery route response");
        assert_eq!(response.status(), StatusCode::OK, "endpoint {endpoint}");
        let payload = response_json(response).await;
        assert_eq!(payload["schema_version"], json!(1), "endpoint {endpoint}");
    }

    let geometry_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/delivery-projects/line.demo/geometry")
                .body(Body::empty())
                .expect("geometry request should build"),
        )
        .await
        .expect("geometry response");
    assert_eq!(geometry_response.status(), StatusCode::OK);
    let geometry = response_json(geometry_response).await;
    assert_eq!(geometry["artifact_kind"], "semantic_twin_geometry");
    assert_eq!(geometry["nodes"][0]["evidence_status"], "verified");
    assert_eq!(geometry["edges"][0]["evidence_status"], "derived");
    assert_eq!(geometry["project_id"], "line.demo");

    let wiring_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/delivery-projects/line.demo/wiring")
                .body(Body::empty())
                .expect("wiring request should build"),
        )
        .await
        .expect("wiring response");
    assert_eq!(wiring_response.status(), StatusCode::OK);
    let wiring = response_json(wiring_response).await;
    assert_eq!(wiring["diagnostics"][0]["code"], "WIR-004");
    assert_eq!(wiring["validation_summaries"][0]["error_count"], 1);

    let resolved = resolve_delivery_project_root(&workspace_root, "line.demo")
        .expect("project root should resolve");
    assert_eq!(
        resolved,
        workspace_root
            .join("delivery-projects/demo-line")
            .canonicalize()
            .expect("fixture project root should canonicalize")
    );
    let digests = current_evidence_digests(&workspace_root, "line.demo")
        .expect("current evidence digests should resolve");
    assert!(digests
        .keys()
        .any(|path| path.ends_with("delivery-project.json")));
    assert!(digests
        .values()
        .all(|digest| digest.len() == 64 && digest.chars().all(|ch| ch.is_ascii_hexdigit())));
}

#[tokio::test]
async fn delivery_project_projections_ignore_historical_runs() {
    let workspace_root = temp_workspace_root("delivery-current-run-projection");
    write_delivery_project_fixture(&workspace_root);
    write_historical_delivery_run_fixture(&workspace_root);
    let app = build_app(test_state(workspace_root.clone(), BTreeMap::new()));

    let runs = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/delivery-projects/line.demo/runs")
                .body(Body::empty())
                .expect("runs request should build"),
        )
        .await
        .expect("runs route should respond");
    assert_eq!(runs.status(), StatusCode::OK);
    let runs = response_json(runs).await;
    assert_eq!(runs["runs"].as_array().map(Vec::len), Some(2));
    assert!(runs["runs"].as_array().is_some_and(|runs| {
        runs.iter().any(|run| run["run_id"] == "run-1")
            && runs.iter().any(|run| run["run_id"] == "run-0-old")
    }));

    let projects = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/delivery-projects")
                .body(Body::empty())
                .expect("projects request should build"),
        )
        .await
        .expect("projects route should respond");
    let projects = response_json(projects).await;
    assert_eq!(projects["projects"][0]["run_count"], 2);
    assert_eq!(projects["projects"][0]["latest_run"]["run_id"], "run-1");

    let verification = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/delivery-projects/line.demo/verification")
                .body(Body::empty())
                .expect("verification request should build"),
        )
        .await
        .expect("verification route should respond");
    let verification = response_json(verification).await;
    assert!(verification["reports"].as_array().is_some_and(|reports| {
        !reports.is_empty() && reports.iter().all(|report| report["run_id"] == "run-1")
    }));
    let safety = verification["stages"]
        .as_array()
        .and_then(|stages| stages.iter().find(|stage| stage["stage"] == "safety"))
        .expect("current safety stage should be projected");
    assert_eq!(safety["reported_status"], "pass");
    assert!(!verification.to_string().contains("OLD-SAFETY-WARNING"));

    let problems = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/workspace/problems")
                .body(Body::empty())
                .expect("problems request should build"),
        )
        .await
        .expect("problems route should respond");
    let problems = response_json(problems).await;
    assert!(problems["problems"].as_array().is_some_and(|records| {
        records.iter().any(|record| record["code"] == "GAP-DEMO")
            && records
                .iter()
                .filter(|record| record["project_id"] == "line.demo")
                .all(|record| record["run_id"].is_null() || record["run_id"] == "run-1")
    }));
    assert!(!problems.to_string().contains("GAP-OLD-RUN"));
    assert!(!problems.to_string().contains("ANOM-OLD-RUN"));

    let tests = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/workspace/tests")
                .body(Body::empty())
                .expect("tests request should build"),
        )
        .await
        .expect("tests route should respond");
    let tests = response_json(tests).await;
    assert!(tests["tests"].as_array().is_some_and(|records| {
        !records.is_empty()
            && records
                .iter()
                .filter(|record| record["project_id"] == "line.demo")
                .all(|record| record["run_id"] == "run-1")
    }));
    assert!(!tests.to_string().contains("old_compile_verify"));
    assert!(!tests.to_string().contains("old_project_check"));

    let evidence = app
        .oneshot(
            Request::builder()
                .uri("/api/delivery-projects/line.demo/evidence")
                .body(Body::empty())
                .expect("evidence request should build"),
        )
        .await
        .expect("evidence route should respond");
    let evidence = response_json(evidence).await;
    assert!(evidence.to_string().contains("out/agent-runs/run-1"));
    assert!(!evidence.to_string().contains("out/agent-runs/run-0-old"));

    let digests = current_evidence_digests(&workspace_root, "line.demo")
        .expect("current evidence digests should resolve");
    assert!(digests.keys().any(|path| path.contains("run-1")));
    assert!(!digests.keys().any(|path| path.contains("run-0-old")));
}

#[tokio::test]
async fn delivery_run_recomputes_unattended_attribution_from_file_evidence() {
    let workspace_root = temp_workspace_root("delivery-attribution-proven");
    write_delivery_project_fixture(&workspace_root);
    write_delivery_provenance_v2(&workspace_root, "agent_generated", true, false);
    let app = build_app(test_state(workspace_root.clone(), BTreeMap::new()));

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/delivery-projects/line.demo/runs/run-1")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("run route should respond");
    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["unattended_verdict"], "not_proven");
    assert_eq!(
        payload["attribution"]["execution_unattended_verdict"],
        "proven"
    );
    assert_eq!(
        payload["attribution"]["source_authoring_verdict"],
        "not_proven"
    );
    assert!(payload["attribution"]["validation_issues"]
        .as_array()
        .is_some_and(|issues| issues
            .iter()
            .any(|issue| issue == "source_authoring_provenance_missing")));
    assert_eq!(
        payload["attribution"]["records"][0]["attribution_kind"],
        "agent_generated"
    );
    assert_eq!(payload["attribution"]["evidence"][0]["kind"], "provenance");
    assert_eq!(payload["git"]["dirty_worktree"], true);
    assert!(!payload["attribution"]["validation_issues"]
        .as_array()
        .is_some_and(|issues| issues.iter().any(|issue| issue
            .as_str()
            .is_some_and(|issue| issue.ends_with("event_unknown")))));

    fs::write(
        workspace_root.join(
            "delivery-projects/demo-line/out/agent-runs/run-1/compile/verification_report.json",
        ),
        "{\"changed_after_run\":true}\n",
    )
    .expect("post-run edit should be written");
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/delivery-projects/line.demo/runs/run-1")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("run route should respond after post-run edit");
    let payload = response_json(response).await;
    assert_eq!(payload["unattended_verdict"], "not_proven");
    assert_eq!(
        payload["attribution"]["execution_unattended_verdict"],
        "proven"
    );
    assert_eq!(
        payload["attribution"]["records"][0]["attribution_kind"],
        "post_run_human_change"
    );
    assert_eq!(payload["attribution"]["post_run_human_change_count"], 1);
}

#[tokio::test]
async fn delivery_run_requires_agent_bound_source_authoring_for_overall_proven_verdict() {
    let workspace_root = temp_workspace_root("delivery-source-authoring-proven");
    write_delivery_project_fixture(&workspace_root);
    write_delivery_provenance_v2(&workspace_root, "agent_modified", true, true);
    let app = build_app(test_state(workspace_root, BTreeMap::new()));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/delivery-projects/line.demo/runs/run-1")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("run route should respond");
    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["unattended_verdict"], "proven");
    assert_eq!(
        payload["attribution"]["execution_unattended_verdict"],
        "proven"
    );
    assert_eq!(payload["attribution"]["source_authoring_verdict"], "proven");
    assert_eq!(payload["attribution"]["source_authoring_record_count"], 1);
    assert_eq!(payload["attribution"]["validation_issues"], json!([]));
}

#[tokio::test]
async fn delivery_run_rejects_missing_agent_binding_and_reports_human_intervention() {
    let workspace_root = temp_workspace_root("delivery-attribution-falsifiable");
    write_delivery_project_fixture(&workspace_root);
    write_delivery_provenance_v2(&workspace_root, "agent_generated", false, false);
    let app = build_app(test_state(workspace_root.clone(), BTreeMap::new()));

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/delivery-projects/line.demo/runs/run-1")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("run route should respond");
    let payload = response_json(response).await;
    assert_eq!(payload["unattended_verdict"], "not_proven");
    assert!(payload["attribution"]["validation_issues"]
        .as_array()
        .is_some_and(|issues| issues
            .iter()
            .any(|issue| issue == "file_attribution_0_agent_binding_missing")));
    assert!(!payload["attribution"]["validation_issues"]
        .as_array()
        .is_some_and(|issues| issues
            .iter()
            .any(|issue| issue == "file_attribution_0_event_unknown")));

    write_delivery_provenance_v2(&workspace_root, "human_intervention_detected", false, false);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/delivery-projects/line.demo/runs/run-1")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("run route should respond after human intervention record");
    let payload = response_json(response).await;
    assert_eq!(payload["unattended_verdict"], "human_intervention_detected");
    assert_eq!(payload["attribution"]["human_intervention_detected"], true);
}

#[tokio::test]
async fn physical_evidence_is_attributable_append_only_and_gates_release() {
    let workspace_root = temp_workspace_root("physical-evidence");
    write_delivery_project_fixture(&workspace_root);

    let engineer_app = build_app(delivery_state_with_user(
        workspace_root.clone(),
        UserRole::Engineer,
    ));
    let engineer_token = login_delivery_reviewer(&engineer_app).await;
    let forbidden = engineer_app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/delivery-projects/line.demo/wiring/points/plc_main.X0/observations")
                .header(header::AUTHORIZATION, format!("Bearer {engineer_token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "status": "pass", "note": "not authorized" }).to_string(),
                ))
                .expect("observation request should build"),
        )
        .await
        .expect("observation route should respond");
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

    let app = build_app(delivery_state_with_user(
        workspace_root.clone(),
        UserRole::Admin,
    ));
    let unauthenticated_upload = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/delivery-projects/line.demo/evidence/uploads/point.png")
                .header("x-evidence-kind", "photo")
                .body(Body::from("image"))
                .expect("upload request should build"),
        )
        .await
        .expect("upload route should respond");
    assert_eq!(unauthenticated_upload.status(), StatusCode::UNAUTHORIZED);

    let token = login_delivery_reviewer(&app).await;
    let upload = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/delivery-projects/line.demo/evidence/uploads/point.png")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "image/png")
                .header("x-evidence-kind", "photo")
                .header("x-semantic-object-kind", "wiring_point")
                .header("x-semantic-object-id", "plc_main.X0")
                .body(Body::from("binary-photo-content"))
                .expect("upload request should build"),
        )
        .await
        .expect("upload route should respond");
    assert_eq!(upload.status(), StatusCode::CREATED);
    let upload = response_json(upload).await;
    assert_eq!(upload["size_bytes"], json!(20));
    assert_eq!(upload["deep_link"]["kind"], "delivery_deep_link");
    assert_eq!(
        upload["deep_link"]["source"]["artifact"],
        upload["artifact_ref"]
    );
    assert_eq!(upload["deep_link"]["object"]["id"], "plc_main.X0");

    sign_delivery_hold(&app, &token, "wiring_review").await;

    let unsafe_trace = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/delivery-projects/line.demo/wiring/points/plc_main.X0/observations")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "status": "pass", "trace_ref": "../outside.jsonl" }).to_string(),
                ))
                .expect("unsafe trace request should build"),
        )
        .await
        .expect("unsafe trace route should respond");
    assert_eq!(unsafe_trace.status(), StatusCode::BAD_REQUEST);

    let observation = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/delivery-projects/line.demo/wiring/points/plc_main.X0/observations")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "status": "pass",
                        "measurement": {
                            "value": "24.1",
                            "unit": "VDC",
                            "instrument_id": "DMM-01"
                        },
                        "photo_upload_id": upload["upload_id"],
                        "trace_ref": "delivery-projects/demo-line/plc/trace.jsonl",
                        "note": "input toggled and returned to safe state"
                    })
                    .to_string(),
                ))
                .expect("observation request should build"),
        )
        .await
        .expect("observation route should respond");
    assert_eq!(observation.status(), StatusCode::CREATED);
    let observation = response_json(observation).await;
    assert_eq!(observation["measurement"]["value"], "24.1");
    assert_eq!(observation["photo_upload_id"], upload["upload_id"]);
    assert_eq!(observation["user"]["role"], "admin");
    assert_eq!(observation["trace_sha256"].as_str().map(str::len), Some(64));

    fs::create_dir_all(workspace_root.join("delivery-projects/other"))
        .expect("other project directory should be created");
    fs::write(
        workspace_root.join("delivery-projects/other/trace.jsonl"),
        "{\"tick\":2}\n",
    )
    .expect("other project trace should be written");
    let cross_project_trace = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/delivery-projects/line.demo/wiring/points/plc_main.X0/observations")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "status": "pass",
                        "trace_ref": "delivery-projects/other/trace.jsonl"
                    })
                    .to_string(),
                ))
                .expect("cross-project trace request should build"),
        )
        .await
        .expect("cross-project trace route should respond");
    assert_eq!(cross_project_trace.status(), StatusCode::BAD_REQUEST);

    let physical = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/delivery-projects/line.demo/physical-evidence")
                .body(Body::empty())
                .expect("physical evidence request should build"),
        )
        .await
        .expect("physical evidence route should respond");
    let physical = response_json(physical).await;
    assert_eq!(physical["observations"].as_array().map(Vec::len), Some(1));
    assert_eq!(physical["uploads"].as_array().map(Vec::len), Some(1));
    assert_eq!(physical["point_checks"]["summary"]["observed_points"], 1);
    assert_eq!(physical["point_checks"]["points"][0]["status"], "observed");

    let ui_wiring = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/delivery-projects/line.demo/wiring")
                .body(Body::empty())
                .expect("UI wiring request should build"),
        )
        .await
        .expect("UI wiring route should respond");
    let ui_wiring = response_json(ui_wiring).await;
    assert_eq!(ui_wiring["points"][0]["point_check_status"], "observed");
    assert_eq!(
        ui_wiring["points"][0]["deep_link"]["object"]["kind"],
        "wiring_point"
    );

    let ui_project = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/delivery-projects/line.demo")
                .body(Body::empty())
                .expect("UI project request should build"),
        )
        .await
        .expect("UI project route should respond");
    let ui_project = response_json(ui_project).await;
    let ui_wiring_hold = ui_project["human_holds"]
        .as_array()
        .and_then(|holds| holds.iter().find(|hold| hold["hold_id"] == "wiring_review"))
        .expect("UI wiring hold should exist");
    assert_eq!(ui_wiring_hold["status"], "stale");
    assert_eq!(ui_project["release_verdict"], "blocked");

    let holds = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/delivery-projects/line.demo/holds")
                .body(Body::empty())
                .expect("holds request should build"),
        )
        .await
        .expect("holds route should respond");
    let holds = response_json(holds).await;
    assert_eq!(find_hold(&holds, "wiring_review")["status"], "stale");
    assert_eq!(
        find_hold(&holds, "point_check_completion")["point_check_summary"]["remaining_points"],
        0
    );

    for hold_id in [
        "wiring_review",
        "point_check_completion",
        "safety_review",
        "hil_review",
    ] {
        sign_delivery_hold(&app, &token, hold_id).await;
    }
    let unreleasable_status = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/delivery-projects/line.demo/release")
                .body(Body::empty())
                .expect("unreleasable status request should build"),
        )
        .await
        .expect("unreleasable status route should respond");
    let unreleasable_status = response_json(unreleasable_status).await;
    assert_eq!(unreleasable_status["status"], "blocked");
    assert_eq!(
        unreleasable_status["delivery_status_gate"]["error_code"],
        "DELIVERY_STATUS_NOT_RELEASABLE"
    );

    let manifest_path = workspace_root.join("delivery-projects/demo-line/delivery-project.json");
    let mut manifest: Value = serde_json::from_slice(
        &fs::read(&manifest_path).expect("delivery manifest should be readable"),
    )
    .expect("delivery manifest should parse");
    manifest["delivery_status"] = json!("fail");
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).expect("failed delivery manifest should serialize"),
    )
    .expect("failed delivery manifest should update");
    for hold_id in [
        "wiring_review",
        "point_check_completion",
        "safety_review",
        "hil_review",
    ] {
        sign_delivery_hold(&app, &token, hold_id).await;
    }
    let failed_delivery = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/delivery-projects/line.demo/release")
                .body(Body::empty())
                .expect("failed delivery request should build"),
        )
        .await
        .expect("failed delivery route should respond");
    let failed_delivery = response_json(failed_delivery).await;
    assert_eq!(failed_delivery["status"], "blocked");
    assert_eq!(failed_delivery["delivery_status"], "fail");

    manifest["delivery_status"] = json!("current");
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).expect("delivery manifest should serialize"),
    )
    .expect("delivery manifest should update");
    for hold_id in [
        "wiring_review",
        "point_check_completion",
        "safety_review",
        "hil_review",
    ] {
        sign_delivery_hold(&app, &token, hold_id).await;
    }
    let release_ready = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/delivery-projects/line.demo/release")
                .body(Body::empty())
                .expect("release request should build"),
        )
        .await
        .expect("release route should respond");
    let release_ready = response_json(release_ready).await;
    assert_eq!(release_ready["status"], "human_action_required");
    assert_eq!(
        release_ready["blocked_prerequisites"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );

    sign_delivery_hold(&app, &token, "release_approval").await;
    let approved = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/delivery-projects/line.demo/release")
                .body(Body::empty())
                .expect("approved release request should build"),
        )
        .await
        .expect("approved release route should respond");
    assert_eq!(response_json(approved).await["status"], "release_approved");

    fs::write(
        workspace_root.join("delivery-projects/demo-line/plc/trace.jsonl"),
        "{\"tick\":1,\"changed\":true}\n",
    )
    .expect("referenced trace should change");

    let blocked = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/delivery-projects/line.demo/release")
                .body(Body::empty())
                .expect("blocked release request should build"),
        )
        .await
        .expect("blocked release route should respond");
    let blocked = response_json(blocked).await;
    assert_eq!(blocked["status"], "blocked");
    assert_eq!(find_hold(&blocked, "release_approval")["status"], "stale");

    let traversal_upload = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/delivery-projects/line.demo/evidence/uploads/%2E%2E%5Cescape.bin")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header("x-evidence-kind", "other")
                .body(Body::from("escape"))
                .expect("traversal upload request should build"),
        )
        .await
        .expect("traversal upload route should respond");
    assert_eq!(traversal_upload.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn workspace_tests_share_local_ci_schema_and_report_git_dirty_state() {
    let workspace_root = temp_workspace_root("delivery-git-ci");
    write_delivery_project_fixture(&workspace_root);
    run_git(&workspace_root, &["init"]);
    run_git(
        &workspace_root,
        &["config", "user.email", "codex@example.invalid"],
    );
    run_git(&workspace_root, &["config", "user.name", "Codex Test"]);
    run_git(&workspace_root, &["add", "."]);
    run_git(&workspace_root, &["commit", "-m", "fixture"]);
    fs::write(workspace_root.join("dirty-marker.txt"), "dirty\n")
        .expect("dirty marker should be written");

    let app = build_app(test_state(workspace_root.clone(), BTreeMap::new()));
    let projects = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/delivery-projects")
                .body(Body::empty())
                .expect("projects request should build"),
        )
        .await
        .expect("projects route should respond");
    let projects = response_json(projects).await;
    assert_eq!(
        projects["projects"][0]["workspace_git"]["status"],
        "current"
    );
    assert_eq!(projects["projects"][0]["workspace_git"]["dirty"], true);
    assert!(projects["projects"][0]["workspace_git"]["changed_paths"]
        .as_array()
        .is_some_and(|paths| paths.iter().any(|path| path == "dirty-marker.txt")));

    let tests = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/workspace/tests")
                .body(Body::empty())
                .expect("tests request should build"),
        )
        .await
        .expect("tests route should respond");
    let tests = response_json(tests).await;
    assert!(tests["tests"].as_array().is_some_and(|records| {
        records.iter().any(|record| {
            record["execution_source"] == "local"
                && record["deep_link"]["object"]["kind"] == "test"
                && record["artifact_ref"].as_str().is_some()
                && record.get("freshness").is_some()
        })
    }));
    let ci_source = tests["sources"]
        .as_array()
        .and_then(|sources| {
            sources.iter().find(|source| {
                source["project_id"] == "line.demo" && source["execution_source"] == "ci"
            })
        })
        .expect("CI source status should be present");
    assert_eq!(ci_source["status"], "unavailable");
    assert_eq!(
        ci_source["freshness"]["error_code"],
        "CI_TEST_EVIDENCE_UNAVAILABLE"
    );

    let problems = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/workspace/problems")
                .body(Body::empty())
                .expect("problems request should build"),
        )
        .await
        .expect("problems route should respond");
    let problems = response_json(problems).await;
    assert!(problems["problems"].as_array().is_some_and(|records| {
        records.iter().any(|record| {
            record["project_id"] == "line.demo"
                && record["deep_link"]["object"]["kind"] == "delivery_gap"
                && record["source_ref"].as_str().is_some()
                && record["artifact_ref"].as_str().is_some()
        })
    }));
    assert!(problems["problems"].as_array().is_some_and(|records| {
        records.iter().all(|record| {
            !matches!(record["severity"].as_str(), Some("warning" | "blocked"))
                || (record["source_ref"]
                    .as_str()
                    .is_some_and(|value| !value.is_empty())
                    && record["artifact_ref"]
                        .as_str()
                        .is_some_and(|value| !value.is_empty())
                    && record["semantic_object"]["kind"]
                        .as_str()
                        .is_some_and(|value| !value.is_empty())
                    && record["semantic_object"]["id"]
                        .as_str()
                        .is_some_and(|value| !value.is_empty()))
        })
    }));

    let project_root = workspace_root.join("delivery-projects/demo-line");
    fs::create_dir_all(project_root.join("out/ci")).expect("CI artifact root should be created");
    fs::write(
        project_root.join("out/ci/tests.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "run_id": "ci-1",
            "source_commit": "0123456789abcdef",
            "steps": [{
                "name": "cargo_test_web_server",
                "classification": "pass",
                "exit_code": 0,
                "elapsed_ms": 42,
                "artifacts": []
            }]
        }))
        .expect("CI test result should serialize"),
    )
    .expect("CI test result should be written");
    let manifest_path = project_root.join("delivery-project.json");
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(&manifest_path).expect("manifest should be readable"))
            .expect("manifest should parse");
    manifest["artifact_roots"]["ci"] = json!("out/ci");
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).expect("manifest should serialize"),
    )
    .expect("manifest should update");

    let ci_tests = app
        .oneshot(
            Request::builder()
                .uri("/api/workspace/tests")
                .body(Body::empty())
                .expect("CI tests request should build"),
        )
        .await
        .expect("CI tests route should respond");
    let ci_tests = response_json(ci_tests).await;
    assert!(ci_tests["tests"].as_array().is_some_and(|records| {
        records.iter().any(|record| {
            record["execution_source"] == "ci"
                && record["name"] == "cargo_test_web_server"
                && record["freshness"]["state"] == "current"
                && record["deep_link"]["source"]["artifact"]
                    == "delivery-projects/demo-line/out/ci/tests.json"
        })
    }));
    assert!(ci_tests["sources"].as_array().is_some_and(|sources| {
        sources.iter().any(|source| {
            source["project_id"] == "line.demo"
                && source["execution_source"] == "ci"
                && source["status"] == "available"
        })
    }));
}

#[test]
fn delivery_project_root_rejects_unsafe_ids() {
    let workspace_root = temp_workspace_root("delivery-id-reject");
    write_delivery_project_fixture(&workspace_root);
    assert!(resolve_delivery_project_root(&workspace_root, "../line.demo").is_err());
    assert!(current_evidence_digests(&workspace_root, "line/demo").is_err());
}
