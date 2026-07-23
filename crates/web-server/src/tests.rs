use super::{
    build_app, build_collab_event, build_geometry_export_args, build_plc_diagnostics,
    build_plc_realtime_response, collab_comment_history, dsl_capabilities,
    generate_plc_from_flowchart, get_geometry, get_keypoints, get_project_source, get_trace,
    get_trace_range, internal_error, is_safe_collab_room, list_project_templates, new_run_id,
    normalize_topology_tags_in_place, parse_plc_topology, plc_language_snapshot,
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
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{RwLock, Semaphore};
use tower::ServiceExt;

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
