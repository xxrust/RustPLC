mod artifact_store;
mod auth;
mod collab;
mod config;
mod delivery;
mod physical_evidence;
mod run_service;
mod security;
mod signatures;

use axum::{
    body::Body,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        DefaultBodyLimit, Path, Query, State,
    },
    http::{header, StatusCode},
    middleware,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use futures_util::{SinkExt, StreamExt};
use rust_plc::ast::{
    DeviceDeclaration, DevicePort, DeviceType, PortRole, PortType, TopologyConnection,
    TopologyRelation,
};
use rust_plc::component_scenario::parse_component_scenario_value;
use rust_plc::component_topology::parse_component_topology_value;
use rust_plc::device_library::DeviceLibrary;
use rust_plc::dsl_capabilities::{build_dsl_capabilities_report, DslCapabilitiesReport};
use rust_plc::error::PlcError;
use rust_plc::lsp::{language_snapshot_for_source, LspLanguageSnapshot};
use rust_plc::parser::parse_plc;
use rust_plc::semantic::compile_semantic_program_with_library;
use rust_plc::topology_semantic_gate::{
    collect_topology_deprecation_warnings, validate_device_purpose_required,
    validate_removed_legacy_io_model, validate_topology_semantics, TopologySemanticGateError,
};
use rust_plc::verification::{verify_all, VerificationIssue, WarningLevel};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path as StdPath, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tokio::sync::{broadcast, RwLock, Semaphore};
use tracing::{error, info};
use uuid::Uuid;

use artifact_store::{
    artifact_href, artifact_href_any, is_safe_relative_path, read_text_file_limited,
    resolve_artifact_reference, resolve_workspace_input, resolve_workspace_output_path,
    workspace_output_root,
};
use collab::{
    build_collab_event, collab_comment_history, collab_room_sender, is_safe_collab_room,
    record_collab_comment, CollabClientEvent, CollabEvent,
};
#[cfg(test)]
use config::validate_bind_security;
use config::{RustPlcLauncher, WebConfig, WebSecurityConfig};
use run_service::{
    first_failure_message, public_command_error, public_command_failure, run_rust_plc,
};
use security::{authorize_mutations, cors_layer};

#[derive(Clone)]
struct AppState {
    workspace_root: PathBuf,
    auth: auth::AuthService,
    signatures: signatures::SignatureStore,
    physical_evidence: physical_evidence::PhysicalEvidenceStore,
    runs: Arc<RwLock<BTreeMap<String, RunRecord>>>,
    collab_rooms: Arc<RwLock<HashMap<String, broadcast::Sender<CollabEvent>>>>,
    collab_comments: Arc<RwLock<HashMap<String, Vec<CollabEvent>>>>,
    security: WebSecurityConfig,
    run_semaphore: Arc<Semaphore>,
    run_timeout: Duration,
    rust_plc_launcher: RustPlcLauncher,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RunArtifacts {
    trace: Option<String>,
    diff: Option<String>,
    timing: Option<String>,
    diagnosis: Option<String>,
    keypoints: Option<String>,
    fault_audit: Option<String>,
    geometry: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RunRecord {
    run_id: String,
    status: String,
    triggered_by: String,
    triggered_at: String,
    triggered_at_ms: u64,
    mode: String,
    artifacts: RunArtifacts,
    failure_summary: Option<String>,
    plc_file: Option<String>,
    scenario_file: Option<String>,
    topology_file: Option<String>,
    tick_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct TriggerRunRequest {
    plc_file: Option<String>,
    scenario_file: Option<String>,
    topology_file: Option<String>,
    mode: Option<String>,
    triggered_by: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeometryExportRequest {
    plc_file: String,
    trace: Option<String>,
    intent_report: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct TickRangeQuery {
    start: Option<u64>,
    end: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct AlarmQuery {
    severity: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct AckAlarmRequest {
    comment: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ParsePlcTopologyRequest {
    content: String,
}

#[derive(Debug, Deserialize)]
struct PlcDiagnosticsRequest {
    content: String,
}

#[derive(Debug, Deserialize)]
struct PlcLanguageRequest {
    content: String,
}

#[derive(Debug, Deserialize)]
struct FlowchartGeneratePlcRequest {
    project_id: Option<String>,
    task_name: String,
    steps: Vec<FlowchartEditorStep>,
    transitions: Vec<FlowchartEditorTransition>,
}

#[derive(Debug, Deserialize)]
struct FlowchartEditorStep {
    id: String,
    label: Option<String>,
    action: Option<String>,
    delay_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct FlowchartEditorTransition {
    from: String,
    to: String,
    guard: Option<String>,
}

#[derive(Debug, Serialize)]
struct FlowchartGeneratePlcResponse {
    source: String,
    valid: bool,
    diagnostics: PlcDiagnosticsResponse,
    normalized_task_name: String,
}

#[derive(Debug, Clone, Copy)]
struct ExampleTemplateDef {
    id: &'static str,
    name: &'static str,
    category: &'static str,
    path: &'static str,
    template_type: &'static str,
    summary: &'static str,
    scenario_path: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
struct ExampleTemplate {
    id: String,
    name: String,
    category: String,
    path: String,
    #[serde(rename = "type")]
    template_type: String,
    summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    scenario_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExampleCatalog {
    schema_version: u32,
    #[serde(default)]
    categories: Vec<ExampleCatalogCategory>,
}

#[derive(Debug, Deserialize)]
struct ExampleCatalogCategory {
    name: String,
    #[serde(default)]
    examples: Vec<ExampleCatalogEntry>,
}

#[derive(Debug, Deserialize)]
struct ExampleCatalogEntry {
    id: String,
    title: String,
    path: String,
    kind: String,
    purpose: String,
    #[serde(default)]
    scenario_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PlcRealtimeRequest {
    content: String,
    request_id: Option<u64>,
}

#[derive(Debug, Serialize)]
struct PlcRealtimeResponse {
    request_id: Option<u64>,
    diagnostics: PlcDiagnosticsResponse,
    language: LspLanguageSnapshot,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct PlcDiagnosticsResponse {
    valid: bool,
    stage: String,
    errors: Vec<String>,
    issues: Vec<PlcDiagnosticIssue>,
    summary: PlcDiagnosticsSummary,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct PlcDiagnosticIssue {
    severity: String,
    stage: String,
    message: String,
    line: usize,
    column: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    suggestion: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct PlcDiagnosticsSummary {
    topology_devices: usize,
    tasks: usize,
    states: usize,
    transitions: usize,
    constraints: usize,
    verification_warnings: usize,
}

const TAGS_SCHEMA_VERSION: u64 = 1;
const MAX_COLLAB_COMMENT_HISTORY: usize = 50;
const COLLAB_COMMENT_HISTORY_DIR: &str = "web_collab/comments";
const DEFAULT_MAX_CONCURRENT_RUNS: usize = 2;
const DEFAULT_RUN_TIMEOUT_SECS: u64 = 120;
const MAX_RUN_RECORDS: usize = 200;
const MAX_ARTIFACT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_INPUT_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_COMMAND_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
const MIN_SCENARIO_TICK_MS: u64 = 1;
const MAX_SCENARIO_TICK_MS: u64 = 60_000;
const MAX_SCENARIO_DURATION_MS: u64 = 24 * 60 * 60 * 1000;
const MAX_SCENARIO_TICKS: u64 = 1_000_000;

const EXAMPLE_TEMPLATES: &[ExampleTemplateDef] = &[
    ExampleTemplateDef {
        id: "demo",
        name: "demo",
        category: "01 Basics",
        path: "examples/demo.plc",
        template_type: "plc",
        summary: "Minimal language and device demonstration.",
        scenario_path: Some("examples/demo.scenario.json"),
    },
    ExampleTemplateDef {
        id: "process_device_demo",
        name: "process_device_demo",
        category: "01 Basics",
        path: "examples/process_device_demo.plc",
        template_type: "plc",
        summary: "Process-device topology and task flow example.",
        scenario_path: None,
    },
    ExampleTemplateDef {
        id: "quadratic_fit",
        name: "quadratic_fit",
        category: "01 Basics",
        path: "examples/quadratic_fit.plc",
        template_type: "plc",
        summary: "Compute and extern-style numeric workflow fixture.",
        scenario_path: None,
    },
    ExampleTemplateDef {
        id: "dual_axis_platform",
        name: "dual_axis_platform",
        category: "02 Motion Control",
        path: "examples/dual_axis_platform.plc",
        template_type: "plc",
        summary: "Canonical dual-axis motion example used by quickstart docs.",
        scenario_path: None,
    },
    ExampleTemplateDef {
        id: "rp2040_motion_minimal",
        name: "rp2040_motion_minimal",
        category: "02 Motion Control",
        path: "examples/rp2040_motion_minimal.plc",
        template_type: "plc",
        summary: "Board-oriented motion example paired with RP2040 scenarios and IO map.",
        scenario_path: None,
    },
    ExampleTemplateDef {
        id: "stepper_collision_guard",
        name: "stepper_collision_guard",
        category: "02 Motion Control",
        path: "examples/stepper_collision_guard.plc",
        template_type: "plc",
        summary: "Stepper safety and collision-guard scenario fixture.",
        scenario_path: None,
    },
    ExampleTemplateDef {
        id: "three_station_assembly",
        name: "three_station_assembly",
        category: "03 Process And Station Flow",
        path: "examples/three_station_assembly.plc",
        template_type: "plc",
        summary: "Multi-station assembly sequence.",
        scenario_path: None,
    },
    ExampleTemplateDef {
        id: "welding_station",
        name: "welding_station",
        category: "03 Process And Station Flow",
        path: "examples/welding_station.plc",
        template_type: "plc",
        summary: "Welding station sequence and constraints.",
        scenario_path: None,
    },
    ExampleTemplateDef {
        id: "load_unload_concurrent_tasks",
        name: "load_unload_concurrent_tasks",
        category: "03 Process And Station Flow",
        path: "examples/load_unload_concurrent_tasks.plc",
        template_type: "plc",
        summary: "Concurrent load/unload task fixture.",
        scenario_path: None,
    },
    ExampleTemplateDef {
        id: "realtime_stress_stress_case",
        name: "realtime_stress/stress_case",
        category: "03 Process And Station Flow",
        path: "examples/realtime_stress/stress_case.plc",
        template_type: "plc",
        summary: "No-board gate and realtime stress playbook fixture.",
        scenario_path: Some("examples/realtime_stress/scenarios/safe.yaml"),
    },
    ExampleTemplateDef {
        id: "project_scaffold_demo_main",
        name: "project_scaffold_demo/plc/main",
        category: "03 Process And Station Flow",
        path: "examples/project_scaffold_demo/plc/main.plc",
        template_type: "plc",
        summary: "Structured project scaffold reference used by scenario tools.",
        scenario_path: Some("examples/project_scaffold_demo/scenarios/nominal/normal.yaml"),
    },
    ExampleTemplateDef {
        id: "workpiece_phase1_transfer",
        name: "workpiece_phase1_transfer",
        category: "04 Workpiece And Material Flow",
        path: "examples/workpiece_phase1_transfer.plc",
        template_type: "plc",
        summary: "Phase 1 acquire/transfer/finish workpiece flow.",
        scenario_path: None,
    },
    ExampleTemplateDef {
        id: "workpiece_carrier_slot_transfer",
        name: "workpiece_carrier_slot_transfer",
        category: "04 Workpiece And Material Flow",
        path: "examples/workpiece_carrier_slot_transfer.plc",
        template_type: "plc",
        summary: "Carrier slot transfer fixture.",
        scenario_path: None,
    },
    ExampleTemplateDef {
        id: "workpiece_split_merge",
        name: "workpiece_split_merge",
        category: "04 Workpiece And Material Flow",
        path: "examples/workpiece_split_merge.plc",
        template_type: "plc",
        summary: "Split/merge lineage fixture.",
        scenario_path: None,
    },
    ExampleTemplateDef {
        id: "nuclear_coolant_isolation",
        name: "nuclear_coolant_isolation",
        category: "05 Safety, Recovery, And Diagnostics",
        path: "examples/nuclear_coolant_isolation.plc",
        template_type: "plc",
        summary: "High-criticality safety example.",
        scenario_path: None,
    },
    ExampleTemplateDef {
        id: "force_override_demo",
        name: "force_override_demo",
        category: "05 Safety, Recovery, And Diagnostics",
        path: "examples/force_override_demo.plc",
        template_type: "plc",
        summary: "Online force, retain, commissioning, and control-plane fixture.",
        scenario_path: None,
    },
    ExampleTemplateDef {
        id: "recovery_templates_estop_recovery",
        name: "recovery_templates/estop_recovery",
        category: "05 Safety, Recovery, And Diagnostics",
        path: "examples/recovery_templates/estop_recovery.plc",
        template_type: "plc",
        summary: "Emergency-stop recovery template.",
        scenario_path: None,
    },
    ExampleTemplateDef {
        id: "recovery_templates_power_loss_recovery",
        name: "recovery_templates/power_loss_recovery",
        category: "05 Safety, Recovery, And Diagnostics",
        path: "examples/recovery_templates/power_loss_recovery.plc",
        template_type: "plc",
        summary: "Power-loss recovery template.",
        scenario_path: None,
    },
    ExampleTemplateDef {
        id: "recovery_templates_sensor_stuck_recovery",
        name: "recovery_templates/sensor_stuck_recovery",
        category: "05 Safety, Recovery, And Diagnostics",
        path: "examples/recovery_templates/sensor_stuck_recovery.plc",
        template_type: "plc",
        summary: "Sensor-stuck recovery template.",
        scenario_path: None,
    },
    ExampleTemplateDef {
        id: "topology_perf_500",
        name: "topology_perf_500",
        category: "06 Performance And Deployment Fixtures",
        path: "examples/topology_perf_500.plc",
        template_type: "plc",
        summary: "Large topology performance fixture.",
        scenario_path: Some("examples/topology_perf_500.scenario.json"),
    },
    ExampleTemplateDef {
        id: "pil_baselines_case_timeout",
        name: "pil_baselines/case_timeout/case",
        category: "06 Performance And Deployment Fixtures",
        path: "examples/pil_baselines/case_timeout/case.plc",
        template_type: "plc",
        summary: "PIL/Renode timeout baseline.",
        scenario_path: Some("examples/pil_baselines/case_timeout/scenarios/base.yaml"),
    },
    ExampleTemplateDef {
        id: "component_model",
        name: "component_model",
        category: "07 Component Simulation",
        path: "examples/component_model/topology.json",
        template_type: "component_topology",
        summary: "Component simulation topology with normal and fault scenarios.",
        scenario_path: Some("examples/component_model/scenario_normal.json"),
    },
];

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("web_server=debug,tower_http=debug")
        .init();

    let workspace_root = find_workspace_root();
    let config = WebConfig::from_env().unwrap_or_else(|message| {
        eprintln!("rustplc-web configuration error: {message}");
        std::process::exit(2);
    });
    let auth = auth::AuthService::from_env(config.bind_addr.ip().is_loopback()).unwrap_or_else(
        |message| {
            eprintln!("rustplc-web authentication configuration error: {message}");
            std::process::exit(2);
        },
    );
    let state = Arc::new(AppState {
        workspace_root: workspace_root.clone(),
        auth,
        signatures: signatures::SignatureStore::new(&workspace_root),
        physical_evidence: physical_evidence::PhysicalEvidenceStore::new(&workspace_root),
        runs: Arc::new(RwLock::new(BTreeMap::new())),
        collab_rooms: Arc::new(RwLock::new(HashMap::new())),
        collab_comments: Arc::new(RwLock::new(HashMap::new())),
        security: config.security.clone(),
        run_semaphore: Arc::new(Semaphore::new(config.max_concurrent_runs)),
        run_timeout: config.run_timeout,
        rust_plc_launcher: config.rust_plc_launcher.clone(),
    });

    let app = build_app(state.clone());

    info!("RustPLC Web Server listening on {}", config.bind_addr);

    let listener = tokio::net::TcpListener::bind(config.bind_addr)
        .await
        .expect("bind web server");
    axum::serve(listener, app).await.expect("run web server");
}

fn build_app(state: Arc<AppState>) -> Router {
    let api_routes = Router::new()
        .merge(auth::routes())
        .merge(delivery::routes())
        .merge(physical_evidence::routes())
        .merge(signatures::routes())
        .route("/projects", get(list_projects))
        .route("/project-templates", get(list_project_templates))
        .route("/projects/{id}/source", get(get_project_source))
        .route("/plc/diagnostics", post(plc_diagnostics))
        .route("/plc/language", post(plc_language_snapshot))
        .route("/dsl/capabilities", get(dsl_capabilities))
        .route("/flowchart/generate-plc", post(flowchart_generate_plc))
        .route("/topology/parse-plc", post(parse_plc_topology))
        .route("/topology/{id}", get(get_topology).put(save_topology))
        .route("/topology/validate", post(validate_topology))
        .route("/scenario/{id}", get(get_scenario).put(save_scenario))
        .route("/scenario/validate", post(validate_scenario))
        .route("/run/no-board-gate", post(trigger_no_board))
        .route("/run/{id}/status", get(get_run_status))
        .route("/run/list", get(list_runs))
        .route("/geometry/export", post(export_geometry))
        .route("/geometry/{id}", get(get_geometry))
        .route("/trace/{id}", get(get_trace))
        .route("/trace/{id}/range", get(get_trace_range))
        .route("/trace/{id}/keypoints", get(get_keypoints))
        .route("/artifacts/{*path}", get(get_artifact))
        .route("/diagnosis/{id}", get(get_diagnosis))
        .route("/timing/{id}", get(get_timing))
        .route("/alarms", get(get_alarms))
        .route("/alarms/{id}/ack", post(ack_alarm));

    let static_dist = state.workspace_root.join("web-ui/dist");
    let cors = cors_layer(&state.security);
    Router::new()
        .route("/ws/plc", get(plc_realtime_ws))
        .route("/ws/collab/{room}", get(collab_ws))
        .nest("/api", api_routes)
        .route("/artifacts/{*path}", get(get_artifact))
        .fallback_service(tower_http::services::ServeDir::new(static_dist))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            authorize_mutations,
        ))
        .layer(DefaultBodyLimit::max(MAX_INPUT_FILE_BYTES as usize))
        .layer(cors)
        .with_state(state)
}

fn find_workspace_root() -> PathBuf {
    let start = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    start.join("../..").canonicalize().unwrap_or(start)
}

async fn list_projects(State(state): State<Arc<AppState>>) -> Json<Value> {
    let projects = collect_example_templates(&state.workspace_root);
    Json(serde_json::json!({ "projects": projects }))
}

async fn list_project_templates(State(state): State<Arc<AppState>>) -> Json<Value> {
    let templates = collect_example_templates(&state.workspace_root);
    let mut categories = BTreeMap::<String, Vec<ExampleTemplate>>::new();
    for template in templates {
        categories
            .entry(template.category.clone())
            .or_default()
            .push(template);
    }

    Json(serde_json::json!({
        "categories": categories
            .into_iter()
            .map(|(category, templates)| {
                serde_json::json!({
                    "category": category,
                    "templates": templates,
                })
            })
            .collect::<Vec<_>>()
    }))
}

async fn get_project_source(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if !is_safe_project_id(&id) {
        return Err(bad_request("invalid project id"));
    }

    let path = match resolve_example_template(&state.workspace_root, &id) {
        Some(template) if template.path.ends_with(".plc") => {
            resolve_workspace_input(&state.workspace_root, &template.path).map_err(bad_request)?
        }
        Some(_) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "project template is not a PLC source",
                    "id": id
                })),
            ));
        }
        None => workspace_path_for_id(&state.workspace_root, "examples", &id, ".plc")
            .map_err(bad_request)?,
    };
    let content = std::fs::read_to_string(&path).map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "project source not found",
                "id": id
            })),
        )
    })?;

    Ok(Json(serde_json::json!({
        "id": id,
        "path": display_rel(&state.workspace_root, &path),
        "content": content
    })))
}

fn is_safe_project_id(id: &str) -> bool {
    is_safe_resource_id(id)
}

fn collect_example_templates(workspace_root: &StdPath) -> Vec<ExampleTemplate> {
    let catalog_templates = collect_example_templates_from_catalog(workspace_root);
    if !catalog_templates.is_empty() {
        return catalog_templates;
    }

    collect_example_templates_from_static_defs(workspace_root)
}

fn collect_example_templates_from_catalog(workspace_root: &StdPath) -> Vec<ExampleTemplate> {
    let catalog_path = workspace_root.join("examples/catalog.toml");
    let Ok(text) = std::fs::read_to_string(&catalog_path) else {
        return Vec::new();
    };
    let Ok(catalog) = toml::from_str::<ExampleCatalog>(&text) else {
        error!(
            path = %catalog_path.display(),
            "failed to parse examples catalog for web project templates"
        );
        return Vec::new();
    };
    if catalog.schema_version != 1 {
        error!(
            path = %catalog_path.display(),
            schema_version = catalog.schema_version,
            "unsupported examples catalog schema version"
        );
        return Vec::new();
    }

    catalog
        .categories
        .into_iter()
        .flat_map(|category| {
            category.examples.into_iter().filter_map(move |entry| {
                let path = resolve_workspace_input(workspace_root, &entry.path).ok()?;
                let scenario_path = entry.scenario_path.as_ref().and_then(|scenario| {
                    resolve_workspace_input(workspace_root, scenario)
                        .ok()
                        .map(|path| display_rel(workspace_root, &path))
                });

                Some(ExampleTemplate {
                    id: entry.id,
                    name: entry.title,
                    category: category.name.clone(),
                    path: display_rel(workspace_root, &path),
                    template_type: entry.kind,
                    summary: entry.purpose,
                    scenario_path,
                })
            })
        })
        .collect()
}

fn collect_example_templates_from_static_defs(workspace_root: &StdPath) -> Vec<ExampleTemplate> {
    EXAMPLE_TEMPLATES
        .iter()
        .filter_map(|template| {
            let path = resolve_workspace_input(workspace_root, template.path).ok()?;
            let scenario_path = template
                .scenario_path
                .and_then(|scenario| resolve_workspace_input(workspace_root, scenario).ok())
                .map(|scenario| display_rel(workspace_root, &scenario));

            Some(ExampleTemplate {
                id: template.id.to_string(),
                name: template.name.to_string(),
                category: template.category.to_string(),
                path: display_rel(workspace_root, &path),
                template_type: template.template_type.to_string(),
                summary: template.summary.to_string(),
                scenario_path,
            })
        })
        .collect()
}

fn resolve_example_template(workspace_root: &StdPath, id: &str) -> Option<ExampleTemplate> {
    collect_example_templates(workspace_root)
        .into_iter()
        .find(|template| template.id == id)
        .or_else(|| resolve_static_example_template(workspace_root, id))
}

fn resolve_static_example_template(workspace_root: &StdPath, id: &str) -> Option<ExampleTemplate> {
    EXAMPLE_TEMPLATES
        .iter()
        .find(|template| {
            template.id == id && resolve_workspace_input(workspace_root, template.path).is_ok()
        })
        .map(|template| {
            let path = resolve_workspace_input(workspace_root, template.path)
                .expect("static example path was validated");
            let scenario_path = template
                .scenario_path
                .and_then(|scenario| resolve_workspace_input(workspace_root, scenario).ok())
                .map(|scenario| display_rel(workspace_root, &scenario));
            ExampleTemplate {
                id: template.id.to_string(),
                name: template.name.to_string(),
                category: template.category.to_string(),
                path: display_rel(workspace_root, &path),
                template_type: template.template_type.to_string(),
                summary: template.summary.to_string(),
                scenario_path,
            }
        })
}

async fn get_topology(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let json_path = topology_path_for_id(&state.workspace_root, &id).map_err(bad_request)?;
    if json_path.exists() {
        let mut value = read_json_value(&json_path)?;
        normalize_topology_tags_in_place(&mut value);
        return Ok(Json(value));
    }

    let plc_path = workspace_path_for_id(&state.workspace_root, "examples", &id, ".plc")
        .map_err(bad_request)?;
    if plc_path.exists() {
        let content = std::fs::read_to_string(&plc_path).map_err(internal_error)?;
        return Ok(Json(serde_json::json!({
            "id": id,
            "path": display_rel(&state.workspace_root, &plc_path),
            "content": content,
            "type": "plc"
        })));
    }

    let mut fallback = serde_json::json!({
        "schema_version": 1,
        "component_library": { "schema_version": 1, "components": [] },
        "components": [],
        "connections": []
    });
    normalize_topology_tags_in_place(&mut fallback);
    Ok(Json(fallback))
}

async fn save_topology(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(mut payload): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    normalize_topology_tags_in_place(&mut payload);
    let path = topology_path_for_id(&state.workspace_root, &id).map_err(bad_request)?;
    write_json_pretty(&path, &payload).map_err(internal_error)?;
    Ok(Json(serde_json::json!({
        "saved": true,
        "path": display_rel(&state.workspace_root, &path)
    })))
}

async fn validate_topology(Json(payload): Json<Value>) -> Json<Value> {
    match parse_component_topology_value(&payload) {
        Ok(_) => Json(serde_json::json!({ "valid": true, "errors": [], "issues": [] })),
        Err(err) => {
            let issues = err
                .issues
                .iter()
                .map(|issue| {
                    serde_json::json!({
                        "code": issue.code,
                        "path": issue.path,
                        "message": issue.message
                    })
                })
                .collect::<Vec<_>>();
            let errors = err
                .issues
                .iter()
                .map(|issue| format!("[{}] {}: {}", issue.code, issue.path, issue.message))
                .collect::<Vec<_>>();
            Json(serde_json::json!({ "valid": false, "errors": errors, "issues": issues }))
        }
    }
}

async fn plc_diagnostics(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<PlcDiagnosticsRequest>,
) -> Json<PlcDiagnosticsResponse> {
    let normalized = payload.content.trim_start_matches('\u{feff}');
    Json(build_plc_diagnostics(&state.workspace_root, normalized))
}

async fn plc_language_snapshot(
    Json(payload): Json<PlcLanguageRequest>,
) -> Json<LspLanguageSnapshot> {
    let normalized = payload.content.trim_start_matches('\u{feff}');
    Json(language_snapshot_for_source(normalized))
}

async fn dsl_capabilities() -> Json<DslCapabilitiesReport> {
    Json(build_dsl_capabilities_report("json"))
}

async fn flowchart_generate_plc(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<FlowchartGeneratePlcRequest>,
) -> Result<Json<FlowchartGeneratePlcResponse>, (StatusCode, Json<Value>)> {
    let generated = generate_plc_from_flowchart(&payload).map_err(bad_request)?;
    let diagnostics = build_plc_diagnostics(&state.workspace_root, &generated.source);

    Ok(Json(FlowchartGeneratePlcResponse {
        valid: diagnostics.valid,
        diagnostics,
        normalized_task_name: generated.normalized_task_name,
        source: generated.source,
    }))
}

async fn plc_realtime_ws(
    State(state): State<Arc<AppState>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| plc_realtime_socket(socket, state))
}

async fn plc_realtime_socket(mut socket: WebSocket, state: Arc<AppState>) {
    while let Some(Ok(message)) = socket.recv().await {
        match message {
            Message::Text(text) => {
                let payload = match serde_json::from_str::<PlcRealtimeRequest>(&text) {
                    Ok(request) => serde_json::to_string(&build_plc_realtime_response(
                        &state.workspace_root,
                        request,
                    )),
                    Err(err) => serde_json::to_string(&serde_json::json!({
                        "error": format!("invalid PLC realtime request: {err}")
                    })),
                };
                let Ok(payload) = payload else {
                    break;
                };
                if socket.send(Message::Text(payload.into())).await.is_err() {
                    break;
                }
            }
            Message::Ping(bytes) => {
                if socket.send(Message::Pong(bytes)).await.is_err() {
                    break;
                }
            }
            Message::Close(_) => break,
            Message::Binary(_) | Message::Pong(_) => {}
        }
    }
}

async fn collab_ws(
    State(state): State<Arc<AppState>>,
    Path(room): Path<String>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if !is_safe_collab_room(&room) {
        return bad_request("invalid collaboration room").into_response();
    }
    ws.on_upgrade(move |socket| collab_socket(socket, state, room))
        .into_response()
}

async fn collab_socket(socket: WebSocket, state: Arc<AppState>, room: String) {
    let sender = collab_room_sender(&state, &room).await;
    let mut receiver = sender.subscribe();
    let (mut outbound, mut inbound) = socket.split();

    for event in collab_comment_history(&state, &room).await {
        let Ok(payload) = serde_json::to_string(&event) else {
            continue;
        };
        if outbound.send(Message::Text(payload.into())).await.is_err() {
            return;
        }
    }

    loop {
        tokio::select! {
            inbound_message = inbound.next() => {
                match inbound_message {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<CollabClientEvent>(&text) {
                            Ok(request) => {
                                let event = build_collab_event(&room, request);
                                record_collab_comment(&state, &event).await;
                                let _ = sender.send(event);
                            }
                            Err(err) => {
                                let payload = serde_json::json!({
                                    "room": room,
                                    "kind": "error",
                                    "message": format!("invalid collaboration event: {err}"),
                                    "at_ms": now_ms(),
                                });
                                if outbound
                                    .send(Message::Text(payload.to_string().into()))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Ping(bytes))) => {
                        if outbound.send(Message::Pong(bytes)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Binary(_) | Message::Pong(_))) => {}
                    Some(Err(_)) => break,
                }
            }
            broadcast_event = receiver.recv() => {
                match broadcast_event {
                    Ok(event) => {
                        let Ok(payload) = serde_json::to_string(&event) else {
                            continue;
                        };
                        if outbound.send(Message::Text(payload.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

fn build_plc_realtime_response(
    workspace_root: &StdPath,
    request: PlcRealtimeRequest,
) -> PlcRealtimeResponse {
    let normalized = request.content.trim_start_matches('\u{feff}');
    PlcRealtimeResponse {
        request_id: request.request_id,
        diagnostics: build_plc_diagnostics(workspace_root, normalized),
        language: language_snapshot_for_source(normalized),
    }
}

#[derive(Debug)]
struct GeneratedFlowchartPlc {
    source: String,
    normalized_task_name: String,
}

fn generate_plc_from_flowchart(
    request: &FlowchartGeneratePlcRequest,
) -> Result<GeneratedFlowchartPlc, String> {
    if request.steps.is_empty() {
        return Err("flowchart must contain at least one step".to_string());
    }
    if request.steps.len() > 128 {
        return Err("flowchart step limit is 128".to_string());
    }
    if request.transitions.len() > 256 {
        return Err("flowchart transition limit is 256".to_string());
    }

    let normalized_task_name = sanitize_plc_identifier(&request.task_name, "main");
    let mut used_step_names = HashSet::<String>::new();
    let mut step_names = HashMap::<String, String>::new();
    let mut ordered_steps = Vec::<(&FlowchartEditorStep, String)>::new();

    for (index, step) in request.steps.iter().enumerate() {
        let raw_name = if step.id.trim().is_empty() {
            step.label.as_deref().unwrap_or_default()
        } else {
            step.id.as_str()
        };
        let base_name = sanitize_plc_identifier(raw_name, &format!("step_{}", index + 1));
        let unique_name = make_unique_identifier(&base_name, &mut used_step_names);
        step_names.insert(step.id.clone(), unique_name.clone());
        if let Some(label) = step.label.as_ref() {
            if !label.trim().is_empty() {
                step_names
                    .entry(label.clone())
                    .or_insert(unique_name.clone());
            }
        }
        ordered_steps.push((step, unique_name));
    }

    let mut outgoing = HashMap::<String, Vec<ResolvedFlowchartTransition>>::new();
    for transition in &request.transitions {
        let Some(from) = step_names.get(&transition.from) else {
            return Err(format!(
                "transition source `{}` does not match a step",
                transition.from
            ));
        };
        let Some(to) = step_names.get(&transition.to) else {
            return Err(format!(
                "transition target `{}` does not match a step",
                transition.to
            ));
        };
        let guard = transition.guard.as_deref().map(str::trim);
        if transition.guard.is_some() && guard.unwrap_or_default().is_empty() {
            return Err(format!(
                "guarded transition from step `{}` to `{}` requires a non-empty guard expression",
                transition.from, transition.to
            ));
        }
        if let Some(guard) = guard {
            validate_flowchart_guard(guard)?;
        }
        outgoing
            .entry(from.clone())
            .or_default()
            .push(ResolvedFlowchartTransition {
                target: to.clone(),
                guard: guard.map(str::to_string),
            });
    }

    let project_label = request
        .project_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("flowchart_editor");
    let mut source = String::new();
    source.push_str("[topology]\n");
    source.push_str("device plc_main: plc {\n");
    source.push_str("    purpose: \"Generated from Web IDE flowchart editor");
    source.push_str(" for ");
    source.push_str(&escape_plc_string(project_label));
    source.push_str("\"\n");
    source.push_str("    model_ref: openplc_softplc\n");
    source.push_str("}\n\n");
    source.push_str("[constraints]\n\n");
    source.push_str("[tasks]\n");
    source.push_str("task ");
    source.push_str(&normalized_task_name);
    source.push_str(":\n");

    for (step, step_name) in ordered_steps {
        let label = step
            .label
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(step.id.as_str());
        let action = step
            .action
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(label);
        source.push_str("    step ");
        source.push_str(&step_name);
        source.push_str(":\n");
        source.push_str("        action: log \"");
        source.push_str(&escape_plc_string(action));
        source.push_str("\"\n");
        if let Some(delay_ms) = step.delay_ms {
            if delay_ms > 3_600_000 {
                return Err(format!(
                    "step `{}` delay_ms exceeds the 3600000ms limit",
                    step.id
                ));
            }
            if delay_ms > 0 {
                source.push_str("        delay: ");
                source.push_str(&delay_ms.to_string());
                source.push_str("ms\n");
            }
        }
        if let Some(transitions) = outgoing.get(&step_name) {
            write_flowchart_transitions(
                &mut source,
                &normalized_task_name,
                &step_name,
                transitions,
                &mut used_step_names,
            )?;
        }
        source.push('\n');
    }

    Ok(GeneratedFlowchartPlc {
        source,
        normalized_task_name,
    })
}

#[derive(Debug, Clone)]
struct ResolvedFlowchartTransition {
    target: String,
    guard: Option<String>,
}

fn write_flowchart_transitions(
    source: &mut String,
    task_name: &str,
    source_step_name: &str,
    transitions: &[ResolvedFlowchartTransition],
    used_step_names: &mut HashSet<String>,
) -> Result<(), String> {
    let default_edges = transitions
        .iter()
        .filter(|transition| transition.guard.is_none())
        .collect::<Vec<_>>();
    let guarded_edges = transitions
        .iter()
        .filter(|transition| transition.guard.is_some())
        .collect::<Vec<_>>();

    match (default_edges.as_slice(), guarded_edges.as_slice()) {
        ([], []) => Ok(()),
        ([default_edge], []) => {
            write_goto(source, task_name, &default_edge.target);
            Ok(())
        }
        ([default_edge], [guarded_edge]) => {
            write_if_else(
                source,
                task_name,
                guarded_edge.guard.as_deref().unwrap_or_default(),
                &guarded_edge.target,
                &default_edge.target,
            );
            Ok(())
        }
        ([default_edge], guarded_edges) => {
            if guarded_edges.is_empty() {
                return Ok(());
            }

            let mut decision_steps = Vec::<String>::new();
            for index in 1..guarded_edges.len() {
                let base = format!("{source_step_name}_branch_{}", index + 1);
                decision_steps.push(make_unique_identifier(&base, used_step_names));
            }

            let first_else = decision_steps
                .first()
                .map(String::as_str)
                .unwrap_or(default_edge.target.as_str());
            write_if_else(
                source,
                task_name,
                guarded_edges[0].guard.as_deref().unwrap_or_default(),
                &guarded_edges[0].target,
                first_else,
            );

            for (index, guarded_edge) in guarded_edges.iter().enumerate().skip(1) {
                let decision_step = &decision_steps[index - 1];
                let else_target = decision_steps
                    .get(index)
                    .map(String::as_str)
                    .unwrap_or(default_edge.target.as_str());
                source.push_str("    step ");
                source.push_str(decision_step);
                source.push_str(":\n");
                write_if_else(
                    source,
                    task_name,
                    guarded_edge.guard.as_deref().unwrap_or_default(),
                    &guarded_edge.target,
                    else_target,
                );
            }

            Ok(())
        }
        ([], [_]) => Err(format!(
            "guarded transition from step `{source_step_name}` requires one unguarded default transition"
        )),
        ([], _) => Err(format!(
            "guarded transitions from step `{source_step_name}` require one unguarded default transition"
        )),
        _ => Err(format!(
            "step `{source_step_name}` has unsupported branching; use exactly one unguarded default transition"
        )),
    }
}

fn write_if_else(
    source: &mut String,
    task_name: &str,
    guard: &str,
    then_target: &str,
    else_target: &str,
) {
    source.push_str("        if: ");
    source.push_str(guard);
    source.push_str(" goto ");
    source.push_str(task_name);
    source.push('.');
    source.push_str(then_target);
    source.push_str(" else: goto ");
    source.push_str(task_name);
    source.push('.');
    source.push_str(else_target);
    source.push('\n');
}

fn write_goto(source: &mut String, task_name: &str, target: &str) {
    source.push_str("        goto ");
    source.push_str(task_name);
    source.push('.');
    source.push_str(target);
    source.push('\n');
}

fn validate_flowchart_guard(guard: &str) -> Result<(), String> {
    if guard.len() > 160 {
        return Err("flowchart guard expression limit is 160 characters".to_string());
    }
    if guard.contains('\n') || guard.contains('\r') {
        return Err("flowchart guard expression must stay on one line".to_string());
    }
    if guard
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || " _().=!<>+-*/&|".contains(ch))
    {
        Ok(())
    } else {
        Err("flowchart guard expression contains unsupported characters".to_string())
    }
}

fn sanitize_plc_identifier(raw: &str, fallback: &str) -> String {
    let mut sanitized = String::new();
    let mut previous_was_separator = false;

    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            sanitized.push(ch.to_ascii_lowercase());
            previous_was_separator = false;
        } else if !previous_was_separator {
            sanitized.push('_');
            previous_was_separator = true;
        }
    }

    let sanitized = sanitized.trim_matches('_').to_string();
    let mut identifier = if sanitized.is_empty() {
        fallback.to_string()
    } else {
        sanitized
    };
    if identifier
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_digit())
    {
        identifier = format!("{fallback}_{identifier}");
    }
    identifier
}

fn make_unique_identifier(base: &str, used: &mut HashSet<String>) -> String {
    if used.insert(base.to_string()) {
        return base.to_string();
    }
    for index in 2.. {
        let candidate = format!("{base}_{index}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!("identifier uniquifier should always find a suffix")
}

fn escape_plc_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\r', " ")
        .replace('\n', " ")
        .replace('\t', " ")
}

fn build_plc_diagnostics(workspace_root: &StdPath, source: &str) -> PlcDiagnosticsResponse {
    let mut issues = Vec::<PlcDiagnosticIssue>::new();

    let program = match parse_plc(source) {
        Ok(program) => program,
        Err(error) => {
            let issue = plc_error_to_issue("parse", error);
            return diagnostics_failure("parse", vec![issue]);
        }
    };

    for warning in collect_topology_deprecation_warnings(&program.topology) {
        issues.push(PlcDiagnosticIssue {
            severity: "warning".to_string(),
            stage: "topology_gate".to_string(),
            message: warning,
            line: 1,
            column: 1,
            code: Some("TOPOLOGY-DEPRECATION".to_string()),
            suggestion: None,
        });
    }

    if let Err(error) = validate_removed_legacy_io_model(&program.topology)
        .and_then(|_| validate_device_purpose_required(&program.topology))
        .and_then(|_| validate_topology_semantics(&program.topology))
    {
        issues.extend(topology_gate_to_issues("topology_gate", error));
        return diagnostics_failure("topology_gate", issues);
    }

    let device_library = match load_web_device_library(workspace_root) {
        Ok(library) => library,
        Err(errors) => {
            issues.extend(
                errors
                    .into_iter()
                    .map(|error| plc_error_to_issue("preprocess", error)),
            );
            return diagnostics_failure("preprocess", issues);
        }
    };

    let semantic = match compile_semantic_program_with_library(
        &program,
        if device_library.is_empty() {
            None
        } else {
            Some(&device_library)
        },
    ) {
        Ok(semantic) => semantic,
        Err(errors) => {
            issues.extend(
                errors
                    .into_iter()
                    .map(|error| plc_error_to_issue("semantic", error)),
            );
            return diagnostics_failure("semantic", issues);
        }
    };

    let expanded = semantic.expanded_program;
    let topology = semantic.topology;
    let state_machine = semantic.state_machine;
    let constraints = semantic.constraints;

    let mut summary = PlcDiagnosticsSummary {
        topology_devices: topology.graph.node_count(),
        tasks: expanded.tasks.tasks.len(),
        states: state_machine.states.len(),
        transitions: state_machine.transitions.len(),
        constraints: constraint_count(&constraints),
        verification_warnings: 0,
    };

    match verify_all(&expanded, &topology, &constraints, &state_machine) {
        Ok(verification) => {
            let warning_issues = verification_warning_issues(&verification);
            summary.verification_warnings = warning_issues.len();
            issues.extend(warning_issues);
            PlcDiagnosticsResponse {
                valid: !issues.iter().any(|issue| issue.severity == "error"),
                stage: "verification".to_string(),
                errors: Vec::new(),
                issues,
                summary,
            }
        }
        Err(verification_issues) => {
            issues.extend(
                verification_issues
                    .into_iter()
                    .map(verification_issue_to_diagnostic),
            );
            PlcDiagnosticsResponse {
                valid: false,
                stage: "verification".to_string(),
                errors: error_messages(&issues),
                issues,
                summary,
            }
        }
    }
}

fn load_web_device_library(workspace_root: &StdPath) -> Result<DeviceLibrary, Vec<PlcError>> {
    static DEVICE_LIBRARY: OnceLock<Result<DeviceLibrary, Vec<PlcError>>> = OnceLock::new();
    DEVICE_LIBRARY
        .get_or_init(|| DeviceLibrary::load(&workspace_root.join("devices")))
        .clone()
}

fn diagnostics_failure(stage: &str, issues: Vec<PlcDiagnosticIssue>) -> PlcDiagnosticsResponse {
    PlcDiagnosticsResponse {
        valid: false,
        stage: stage.to_string(),
        errors: error_messages(&issues),
        issues,
        summary: PlcDiagnosticsSummary::default(),
    }
}

fn error_messages(issues: &[PlcDiagnosticIssue]) -> Vec<String> {
    issues
        .iter()
        .filter(|issue| issue.severity == "error")
        .map(|issue| issue.message.clone())
        .collect()
}

fn topology_gate_to_issues(
    stage: &str,
    error: TopologySemanticGateError,
) -> Vec<PlcDiagnosticIssue> {
    error
        .issues
        .into_iter()
        .map(|issue| PlcDiagnosticIssue {
            severity: "error".to_string(),
            stage: stage.to_string(),
            message: issue.message,
            line: issue.line.max(1),
            column: 1,
            code: Some(issue.code.as_str().to_string()),
            suggestion: Some(issue.suggestion),
        })
        .collect()
}

fn plc_error_to_issue(stage: &str, error: PlcError) -> PlcDiagnosticIssue {
    let code = match &error {
        PlcError::ParseError { .. } => "parse",
        PlcError::SemanticError { .. } => "semantic",
        PlcError::UndefinedReference { .. } => "undefined_reference",
        PlcError::TypeMismatch { .. } => "type_mismatch",
        PlcError::DuplicateDefinition { .. } => "duplicate_definition",
    };
    PlcDiagnosticIssue {
        severity: "error".to_string(),
        stage: stage.to_string(),
        message: error.to_string(),
        line: error.line().max(1),
        column: error.column().max(1),
        code: Some(code.to_string()),
        suggestion: None,
    }
}

fn verification_issue_to_diagnostic(issue: VerificationIssue) -> PlcDiagnosticIssue {
    let mut message = issue.reason;
    if !issue.details.is_empty() {
        message.push_str(": ");
        message.push_str(&issue.details.join("; "));
    }
    PlcDiagnosticIssue {
        severity: "error".to_string(),
        stage: "verification".to_string(),
        message,
        line: issue.line.max(1),
        column: 1,
        code: Some(issue.checker),
        suggestion: Some(issue.suggestion),
    }
}

fn verification_warning_issues(
    summary: &rust_plc::verification::VerificationSummary,
) -> Vec<PlcDiagnosticIssue> {
    let mut issues = Vec::new();
    collect_checker_warnings("safety", &summary.safety.warnings, &mut issues);
    collect_checker_warnings("liveness", &summary.liveness.warnings, &mut issues);
    collect_checker_warnings("timing", &summary.timing.warnings, &mut issues);
    collect_checker_warnings("causality", &summary.causality.warnings, &mut issues);
    issues
}

fn collect_checker_warnings(
    stage: &str,
    warnings: &[rust_plc::verification::WarningEntry],
    issues: &mut Vec<PlcDiagnosticIssue>,
) {
    for warning in warnings {
        let severity = match &warning.level {
            WarningLevel::Error => "error",
            WarningLevel::Warn => "warning",
            WarningLevel::Info => "info",
        };
        issues.push(PlcDiagnosticIssue {
            severity: severity.to_string(),
            stage: format!("verification.{stage}"),
            message: warning.message.clone(),
            line: 1,
            column: 1,
            code: warning.code.clone(),
            suggestion: None,
        });
    }
}

fn constraint_count(constraints: &rust_plc::ir::ConstraintSet) -> usize {
    constraints.safety.len()
        + constraints.timing.len()
        + constraints.causality.len()
        + constraints.semantic_resources.len()
        + constraints.resource_claims.len()
        + constraints.workpiece_types.len()
        + constraints.workpiece_sites.len()
        + constraints.workpiece_holders.len()
        + constraints.workpiece_carriers.len()
}

async fn parse_plc_topology(
    Json(payload): Json<ParsePlcTopologyRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let normalized = payload.content.trim_start_matches('\u{feff}');
    let preview_plc =
        build_topology_preview_plc(normalized).unwrap_or_else(|| normalized.to_string());
    let program = parse_plc(&preview_plc)
        .map_err(|err| bad_request(format!("failed to parse PLC: {err}")))?;
    let semantic_gate = match validate_removed_legacy_io_model(&program.topology)
        .and_then(|_| validate_device_purpose_required(&program.topology))
        .and_then(|_| validate_topology_semantics(&program.topology))
    {
        Ok(()) => serde_json::json!({
            "valid": true,
            "code": serde_json::Value::Null,
            "issues": []
        }),
        Err(err) => serde_json::json!({
            "valid": false,
            "code": err.code,
            "issues": err.issues
        }),
    };
    let compat_warnings = collect_topology_deprecation_warnings(&program.topology);

    let devices = &program.topology.devices;
    let mut name_to_index = HashMap::<String, usize>::new();
    for (idx, device) in devices.iter().enumerate() {
        name_to_index.insert(device.name.clone(), idx);
    }

    let resolved_ports_by_name = devices
        .iter()
        .map(|device| (device.name.clone(), resolved_device_ports(device)))
        .collect::<HashMap<_, _>>();

    let components = devices
        .iter()
        .enumerate()
        .map(|(idx, device)| {
            let col = idx % 4;
            let row = idx / 4;
            let ports = resolved_ports_by_name
                .get(&device.name)
                .cloned()
                .unwrap_or_default();
            serde_json::json!({
                "id": device.name,
                "component_id": map_plc_device_to_component_id(&device.device_type),
                "params": {
                    "name": device.name,
                    "device_type": plc_device_type_name(&device.device_type),
                    "endpoint_kind": endpoint_kind_for_device_type(&device.device_type),
                    "purpose": device.attributes.purpose.clone(),
                    "driven_by": device.attributes.driven_by.clone(),
                    "reports_to": device.attributes.reports_to.clone(),
                    "ports": ports,
                    "tags": device.attributes.tags.clone(),
                    "detects": device.attributes.detects.as_ref().map(|d| format!("{}.{}", d.device, d.state)),
                    "detects_device": device.attributes.detects.as_ref().map(|d| d.device.clone()),
                    "detects_state": device.attributes.detects.as_ref().map(|d| d.state.clone()),
                },
                "position": {
                    "x": 160 + col as i64 * 220,
                    "y": 120 + row as i64 * 180,
                }
            })
        })
        .collect::<Vec<_>>();

    let connections = resolve_topology_connections(devices, &program.topology.connections)
        .into_iter()
        .filter(|conn| {
            name_to_index.contains_key(&conn.from) && name_to_index.contains_key(&conn.to)
        })
        .map(|conn| infer_connection_ports(conn, &resolved_ports_by_name))
        .map(|conn| {
            serde_json::json!({
                "from": conn.from,
                "to": conn.to,
                "relation": conn.relation,
                "signal": conn.signal,
                "from_port": conn.from_port,
                "to_port": conn.to_port,
            })
        })
        .collect::<Vec<_>>();

    let mut response = serde_json::json!({
        "schema_version": 1,
        "component_library": {
            "schema_version": 1,
            "components": []
        },
        "components": components,
        "connections": connections,
        "semantic_gate": semantic_gate,
        "compat_warnings": compat_warnings
    });
    normalize_topology_tags_in_place(&mut response);
    Ok(Json(response))
}

fn resolve_topology_connections(
    devices: &[DeviceDeclaration],
    explicit_connections: &[TopologyConnection],
) -> Vec<TopologyConnection> {
    if !explicit_connections.is_empty() {
        return explicit_connections.to_vec();
    }

    let mut connections = Vec::new();
    for device in devices {
        if let Some(upstream) = device.attributes.driven_by.as_ref() {
            connections.push(TopologyConnection {
                from: upstream.clone(),
                to: device.name.clone(),
                relation: TopologyRelation::DrivenBy,
                from_port: None,
                to_port: None,
                signal: None,
            });
        }
        if let Some(target) = device.attributes.reports_to.as_ref() {
            connections.push(TopologyConnection {
                from: device.name.clone(),
                to: target.clone(),
                relation: TopologyRelation::ReportsTo,
                from_port: None,
                to_port: None,
                signal: None,
            });
        }
        if let Some(detects) = device.attributes.detects.as_ref() {
            connections.push(TopologyConnection {
                from: detects.device.clone(),
                to: device.name.clone(),
                relation: TopologyRelation::Detects,
                from_port: Some(detects.state.clone()),
                to_port: None,
                signal: Some(detects.state.clone()),
            });
        }
    }
    connections
}

fn resolved_device_ports(device: &DeviceDeclaration) -> Vec<DevicePort> {
    if !device.attributes.ports.is_empty() {
        return device.attributes.ports.clone();
    }
    implicit_ports_for_device_type(&device.device_type)
}

fn implicit_ports_for_device_type(device_type: &DeviceType) -> Vec<DevicePort> {
    match device_type {
        DeviceType::DigitalOutput => {
            vec![device_port("out", PortType::Digital, PortRole::Producer)]
        }
        DeviceType::DigitalInput => vec![device_port("in", PortType::Digital, PortRole::Consumer)],
        DeviceType::Plc => Vec::new(),
        DeviceType::SolenoidValve => vec![
            device_port("coil", PortType::Digital, PortRole::Consumer),
            device_port("out", PortType::Pneumatic, PortRole::Producer),
        ],
        DeviceType::Cylinder => vec![
            device_port("cmd", PortType::Pneumatic, PortRole::Consumer),
            device_port("extended", PortType::Logical, PortRole::Producer),
            device_port("retracted", PortType::Logical, PortRole::Producer),
        ],
        DeviceType::Sensor => vec![
            device_port("sense", PortType::Logical, PortRole::Consumer),
            device_port("out", PortType::Digital, PortRole::Producer),
        ],
        DeviceType::Motor => vec![
            device_port("cmd", PortType::Digital, PortRole::Consumer),
            device_port("on", PortType::Logical, PortRole::Producer),
        ],
        DeviceType::StepperMotor => vec![
            device_port("enable", PortType::Digital, PortRole::Consumer),
            device_port("direction", PortType::Digital, PortRole::Consumer),
            device_port("pulse", PortType::Digital, PortRole::Consumer),
            device_port("fault", PortType::Logical, PortRole::Producer),
        ],
        DeviceType::Vfd => vec![
            device_port("run", PortType::Digital, PortRole::Consumer),
            device_port("direction", PortType::Digital, PortRole::Consumer),
            device_port("running", PortType::Logical, PortRole::Producer),
            device_port("fault", PortType::Logical, PortRole::Producer),
            device_port("freq_arrive", PortType::Logical, PortRole::Producer),
        ],
        DeviceType::ServoDrive => vec![
            device_port("enable", PortType::Digital, PortRole::Consumer),
            device_port("direction", PortType::Digital, PortRole::Consumer),
            device_port("pulse", PortType::Digital, PortRole::Consumer),
            device_port("clear_fault", PortType::Digital, PortRole::Consumer),
            device_port("ready", PortType::Logical, PortRole::Producer),
            device_port("in_position", PortType::Logical, PortRole::Producer),
            device_port("fault", PortType::Logical, PortRole::Producer),
            device_port("zero_speed", PortType::Logical, PortRole::Producer),
        ],
        DeviceType::CamCoupling => vec![
            device_port("engage", PortType::Digital, PortRole::Consumer),
            device_port("in_sync", PortType::Logical, PortRole::Producer),
            device_port("fault", PortType::Logical, PortRole::Producer),
            device_port("following_error", PortType::Analog, PortRole::Producer),
            device_port("master_pos", PortType::Analog, PortRole::Producer),
            device_port("slave_cmd", PortType::Analog, PortRole::Producer),
        ],
        DeviceType::AnalogInput => vec![device_port("in", PortType::Analog, PortRole::Consumer)],
        DeviceType::AnalogOutput => vec![device_port("out", PortType::Analog, PortRole::Producer)],
        DeviceType::Pid => vec![
            device_port("in", PortType::Analog, PortRole::Consumer),
            device_port("out", PortType::Analog, PortRole::Producer),
        ],
        DeviceType::ProportionalValve => vec![
            device_port("cmd", PortType::Analog, PortRole::Consumer),
            device_port("feedback", PortType::Analog, PortRole::Producer),
            device_port("fault", PortType::Logical, PortRole::Producer),
        ],
        DeviceType::Gripper => vec![
            device_port("cmd", PortType::Digital, PortRole::Consumer),
            device_port("gripped", PortType::Logical, PortRole::Producer),
            device_port("released", PortType::Logical, PortRole::Producer),
            device_port("part_present", PortType::Logical, PortRole::Producer),
            device_port("fault", PortType::Logical, PortRole::Producer),
        ],
        DeviceType::Conveyor => vec![
            device_port("drive", PortType::Digital, PortRole::Consumer),
            device_port("running", PortType::Logical, PortRole::Producer),
            device_port("jam", PortType::Logical, PortRole::Producer),
            device_port("fault", PortType::Logical, PortRole::Producer),
        ],
        DeviceType::Pump => vec![
            device_port("drive", PortType::Digital, PortRole::Consumer),
            device_port("running", PortType::Logical, PortRole::Producer),
            device_port("pressure", PortType::Analog, PortRole::Producer),
            device_port("flow", PortType::Analog, PortRole::Producer),
            device_port("fault", PortType::Logical, PortRole::Producer),
        ],
        DeviceType::Heater => vec![
            device_port("power", PortType::Digital, PortRole::Consumer),
            device_port("temperature", PortType::Analog, PortRole::Producer),
            device_port("fault", PortType::Logical, PortRole::Producer),
        ],
        DeviceType::VisionSensor => vec![
            device_port("trigger", PortType::Digital, PortRole::Consumer),
            device_port("ready", PortType::Logical, PortRole::Producer),
            device_port("busy", PortType::Logical, PortRole::Producer),
            device_port("pass", PortType::Logical, PortRole::Producer),
            device_port("fail", PortType::Logical, PortRole::Producer),
            device_port("fault", PortType::Logical, PortRole::Producer),
        ],
    }
}

fn device_port(id: &str, port_type: PortType, role: PortRole) -> DevicePort {
    DevicePort {
        id: id.to_string(),
        port_type,
        role,
        states: Vec::new(),
        default_state: String::new(),
    }
}

fn infer_connection_ports(
    mut connection: TopologyConnection,
    ports_by_name: &HashMap<String, Vec<DevicePort>>,
) -> TopologyConnection {
    if connection.from_port.is_none() {
        connection.from_port = infer_port_for_side(&connection, ports_by_name, true);
    }
    if connection.to_port.is_none() {
        connection.to_port = infer_port_for_side(&connection, ports_by_name, false);
    }
    if connection.signal.is_none() {
        connection.signal = connection
            .from_port
            .clone()
            .or_else(|| connection.to_port.clone());
    }
    connection
}

fn infer_port_for_side(
    connection: &TopologyConnection,
    ports_by_name: &HashMap<String, Vec<DevicePort>>,
    source_side: bool,
) -> Option<String> {
    let node_name = if source_side {
        &connection.from
    } else {
        &connection.to
    };
    let ports = ports_by_name.get(node_name)?;
    let candidates = ports
        .iter()
        .filter(|port| match (source_side, &port.role) {
            (true, PortRole::Producer | PortRole::Bidirectional) => true,
            (false, PortRole::Consumer | PortRole::Bidirectional) => true,
            _ => false,
        })
        .map(|port| port.id.as_str())
        .collect::<Vec<_>>();

    if candidates.is_empty() {
        return None;
    }

    let preferred = if source_side {
        preferred_source_port_id(connection)
    } else {
        preferred_target_port_id(connection)
    };
    if let Some(preferred_id) = preferred {
        if candidates
            .iter()
            .any(|candidate| *candidate == preferred_id)
        {
            return Some(preferred_id.to_string());
        }
    }

    if candidates.len() == 1 {
        return Some(candidates[0].to_string());
    }

    // Stable fallback for multi-port nodes: choose the first candidate in declaration order.
    Some(candidates[0].to_string())
}

fn preferred_source_port_id(connection: &TopologyConnection) -> Option<&str> {
    if matches!(connection.relation, TopologyRelation::Detects) {
        if let Some(signal) = connection.signal.as_deref() {
            return Some(signal);
        }
        if let Some(from_port) = connection.from_port.as_deref() {
            return Some(from_port);
        }
    }

    match connection.relation {
        TopologyRelation::DrivenBy | TopologyRelation::ReportsTo => Some("out"),
        TopologyRelation::Detects => Some("state"),
    }
}

fn preferred_target_port_id(connection: &TopologyConnection) -> Option<&str> {
    match connection.relation {
        TopologyRelation::DrivenBy => Some("cmd"),
        TopologyRelation::ReportsTo => Some("in"),
        TopologyRelation::Detects => Some("sense"),
    }
}

fn build_topology_preview_plc(input: &str) -> Option<String> {
    let topology = extract_section(input, "topology")?;
    Some(format!(
        "{topology}\n\n[constraints]\n\n[tasks]\n\ntask __topology_preview__:\n    step halt:\n"
    ))
}

fn extract_section(input: &str, name: &str) -> Option<String> {
    let mut offset = 0usize;
    let mut start: Option<usize> = None;
    let mut end: Option<usize> = None;

    for chunk in input.split_inclusive('\n') {
        if let Some(section) = parse_section_header(chunk) {
            if start.is_none() && section.eq_ignore_ascii_case(name) {
                start = Some(offset);
            } else if start.is_some() {
                end = Some(offset);
                break;
            }
        }
        offset += chunk.len();
    }

    if let Some(begin) = start {
        let finish = end.unwrap_or(input.len());
        Some(input[begin..finish].trim_end().to_string())
    } else {
        None
    }
}

fn parse_section_header(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if !(trimmed.starts_with('[') && trimmed.ends_with(']')) {
        return None;
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    let section = inner.trim();
    if section.is_empty() {
        None
    } else {
        Some(section)
    }
}

async fn get_scenario(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let json_path = scenario_path_for_id(&state.workspace_root, &id).map_err(bad_request)?;
    if json_path.exists() {
        return read_json_file(&json_path);
    }

    let legacy_yaml =
        workspace_path_for_id(&state.workspace_root, "examples", &id, "_scenario.yaml")
            .map_err(bad_request)?;
    if legacy_yaml.exists() {
        let content = std::fs::read_to_string(&legacy_yaml).map_err(internal_error)?;
        return Ok(Json(serde_json::json!({
            "id": id,
            "path": display_rel(&state.workspace_root, &legacy_yaml),
            "content": content,
            "type": "yaml"
        })));
    }

    Ok(Json(serde_json::json!({
        "schema_version": 1,
        "tick_ms": 10,
        "duration_ms": 1000,
        "switch_events": [],
        "sensor_events": [],
        "component_faults": []
    })))
}

async fn save_scenario(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    validate_scenario_limits(&payload).map_err(bad_request)?;
    let path = scenario_path_for_id(&state.workspace_root, &id).map_err(bad_request)?;
    write_json_pretty(&path, &payload).map_err(internal_error)?;
    Ok(Json(serde_json::json!({
        "saved": true,
        "path": display_rel(&state.workspace_root, &path)
    })))
}

async fn validate_scenario(Json(payload): Json<Value>) -> Json<Value> {
    if let Err(message) = validate_scenario_limits(&payload) {
        return Json(serde_json::json!({
            "valid": false,
            "errors": [message],
            "issues": [{
                "code": "WEB-SCENARIO-LIMIT",
                "path": "$",
                "message": message
            }]
        }));
    }
    match parse_component_scenario_value(&payload) {
        Ok(_) => Json(serde_json::json!({ "valid": true, "errors": [], "issues": [] })),
        Err(err) => {
            let issues = err
                .issues
                .iter()
                .map(|issue| {
                    serde_json::json!({
                        "code": issue.code,
                        "path": issue.path,
                        "message": issue.message
                    })
                })
                .collect::<Vec<_>>();
            let errors = err
                .issues
                .iter()
                .map(|issue| format!("[{}] {}: {}", issue.code, issue.path, issue.message))
                .collect::<Vec<_>>();
            Json(serde_json::json!({ "valid": false, "errors": errors, "issues": issues }))
        }
    }
}

async fn trigger_no_board(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<TriggerRunRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    validate_run_request(&state.workspace_root, &payload)?;
    let permit = state
        .run_semaphore
        .clone()
        .try_acquire_owned()
        .map_err(|_| too_many_requests("run concurrency limit reached"))?;
    let run_id = new_run_id("run");
    let triggered_at_ms = now_ms();
    let triggered_by = payload
        .triggered_by
        .clone()
        .unwrap_or_else(|| "web-user".to_string());
    let mode = if payload.topology_file.is_some()
        || payload.mode.as_deref() == Some("component")
        || payload.mode.as_deref() == Some("component_sim")
    {
        "component_sim"
    } else {
        "no_board_gate"
    }
    .to_string();

    {
        let mut runs = state.runs.write().await;
        prune_run_records(&mut runs);
        if runs.len() >= MAX_RUN_RECORDS {
            return Err(too_many_requests("run record limit reached"));
        }
        runs.insert(
            run_id.clone(),
            RunRecord {
                run_id: run_id.clone(),
                status: "running".to_string(),
                triggered_by,
                triggered_at: iso_like_timestamp(triggered_at_ms),
                triggered_at_ms,
                mode,
                artifacts: RunArtifacts::default(),
                failure_summary: None,
                plc_file: payload.plc_file.clone(),
                scenario_file: payload.scenario_file.clone(),
                topology_file: payload.topology_file.clone(),
                tick_ms: None,
            },
        );
    }

    let task_state = state.clone();
    let task_payload = payload.clone();
    let task_run_id = run_id.clone();
    tokio::spawn(async move {
        let _permit = permit;
        if let Err(err) = execute_run(task_state.clone(), task_run_id.clone(), task_payload).await {
            error!("run {} failed: {}", task_run_id, err);
            let mut runs = task_state.runs.write().await;
            if let Some(run) = runs.get_mut(&task_run_id) {
                run.status = "fail".to_string();
                run.failure_summary = Some(public_command_error(&task_state.workspace_root, &err));
            }
        }
    });

    Ok(Json(serde_json::json!({ "run_id": run_id })))
}

async fn execute_run(
    state: Arc<AppState>,
    run_id: String,
    payload: TriggerRunRequest,
) -> Result<(), String> {
    let out_dir = workspace_output_root(&state.workspace_root)?
        .join("web_runs")
        .join(&run_id);
    std::fs::create_dir_all(&out_dir)
        .map_err(|err| format!("failed to create run output directory: {err}"))?;

    let scenario_file = payload
        .scenario_file
        .as_deref()
        .ok_or_else(|| "scenario_file is required".to_string())
        .and_then(|path| resolve_workspace_input(&state.workspace_root, path))?
        .display()
        .to_string();

    let mut record_updates = RunArtifacts::default();
    let mut status = "fail".to_string();
    let mut failure_summary: Option<String> = None;
    let parsed_tick_ms = parse_tick_ms_from_scenario(&state.workspace_root, &payload.scenario_file);

    if let Some(topology_file) = payload
        .topology_file
        .as_deref()
        .map(|path| resolve_workspace_input(&state.workspace_root, path))
        .transpose()?
        .map(|path| path.display().to_string())
    {
        let trace_path = out_dir.join("component_trace.jsonl");
        let fault_audit_path = out_dir.join("fault_audit.jsonl");
        let diagnosis_path = out_dir.join("component_diagnosis.json");
        let keypoints_path = out_dir.join("component_keypoints.json");

        let args = vec![
            "component-sim".to_string(),
            topology_file,
            "--scenario".to_string(),
            scenario_file,
            "--out".to_string(),
            trace_path.display().to_string(),
            "--fault-audit-out".to_string(),
            fault_audit_path.display().to_string(),
            "--diagnosis-out".to_string(),
            diagnosis_path.display().to_string(),
            "--keypoints-out".to_string(),
            keypoints_path.display().to_string(),
            "--output".to_string(),
            "json".to_string(),
        ];

        let command = run_rust_plc(&state, &args).await?;
        let output_json = serde_json::from_str::<Value>(&command.stdout).ok();

        if command.success
            && output_json
                .as_ref()
                .and_then(|v| v.get("status"))
                .and_then(Value::as_str)
                == Some("pass")
        {
            status = "pass".to_string();
        } else {
            failure_summary = Some(public_command_failure(&state.workspace_root, &command));
        }

        record_updates.trace = Some(artifact_href(&state.workspace_root, &trace_path));
        record_updates.fault_audit = Some(artifact_href(&state.workspace_root, &fault_audit_path));
        record_updates.diagnosis = Some(artifact_href(&state.workspace_root, &diagnosis_path));
        record_updates.keypoints = Some(artifact_href(&state.workspace_root, &keypoints_path));

        if let Some(tick_ms) =
            parse_tick_ms_from_scenario(&state.workspace_root, &payload.scenario_file)
        {
            let mut runs = state.runs.write().await;
            if let Some(run) = runs.get_mut(&run_id) {
                run.tick_ms = Some(tick_ms);
            }
        }
    } else {
        let plc_file = payload
            .plc_file
            .as_deref()
            .ok_or_else(|| "plc_file is required for no-board-gate mode".to_string())
            .and_then(|path| resolve_workspace_input(&state.workspace_root, path))?
            .display()
            .to_string();

        let args = vec![
            "no-board-gate".to_string(),
            plc_file.clone(),
            "--scenario".to_string(),
            scenario_file.clone(),
            "--out-dir".to_string(),
            out_dir.display().to_string(),
            "--output".to_string(),
            "json".to_string(),
        ];

        let command = run_rust_plc(&state, &args).await?;
        let output_json = serde_json::from_str::<Value>(&command.stdout).ok();

        if command.success
            && output_json
                .as_ref()
                .and_then(|v| v.get("status"))
                .and_then(Value::as_str)
                == Some("pass")
        {
            status = "pass".to_string();
        } else {
            failure_summary = Some(public_command_failure(&state.workspace_root, &command));
        }

        if let Some(v) = output_json {
            record_updates.trace = v
                .get("sil_trace")
                .and_then(Value::as_str)
                .and_then(|path| artifact_href_any(&state.workspace_root, path));
            record_updates.diff = v
                .get("diff_report")
                .and_then(Value::as_str)
                .and_then(|path| artifact_href_any(&state.workspace_root, path));
            record_updates.timing = v
                .get("timing_report")
                .and_then(Value::as_str)
                .and_then(|path| artifact_href_any(&state.workspace_root, path));
            record_updates.diagnosis = v
                .get("diagnosis_report")
                .and_then(Value::as_str)
                .and_then(|path| artifact_href_any(&state.workspace_root, path));
        }

        // Replay needs per-tick IO snapshots, while no-board-gate primarily exports event traces.
        // Generate the snapshot sidecar without disturbing the main gate artifacts.
        let io_snapshot_path = out_dir.join("io_snapshot.json");
        let replay_trace_path = out_dir.join("replay_support_trace.jsonl");
        let replay_args = vec![
            "sim-plc".to_string(),
            plc_file.clone(),
            "--scenario".to_string(),
            scenario_file.clone(),
            "--out".to_string(),
            replay_trace_path.display().to_string(),
            "--io-snapshot-out".to_string(),
            io_snapshot_path.display().to_string(),
        ];
        let replay_command = run_rust_plc(&state, &replay_args).await?;
        if !replay_command.success {
            info!(
                "run {} replay snapshot generation failed: {}",
                run_id,
                first_failure_message(&replay_command.stderr, &replay_command.stdout)
            );
        }

        if let Some(geometry_href) = generate_geometry_for_run(
            &state,
            &out_dir,
            &payload.plc_file,
            record_updates.trace.as_deref(),
            None,
        )
        .await?
        {
            record_updates.geometry = Some(geometry_href);
        }
    }

    let mut runs = state.runs.write().await;
    if let Some(run) = runs.get_mut(&run_id) {
        run.status = status;
        run.failure_summary = failure_summary;
        run.artifacts = record_updates;
        run.tick_ms = parsed_tick_ms;
    }
    Ok(())
}

async fn get_run_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let runs = state.runs.read().await;
    let Some(run) = runs.get(&id) else {
        return Err(not_found(format!("run `{id}` not found")));
    };
    Ok(Json(
        serde_json::to_value(run).unwrap_or_else(|_| serde_json::json!({})),
    ))
}

async fn list_runs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListQuery>,
) -> Json<Vec<Value>> {
    let limit = params.limit.unwrap_or(20).max(1);
    let runs = state.runs.read().await;
    let mut values = runs.values().cloned().collect::<Vec<_>>();
    values.sort_by(|a, b| b.triggered_at_ms.cmp(&a.triggered_at_ms));
    Json(
        values
            .into_iter()
            .take(limit)
            .map(|run| serde_json::to_value(run).unwrap_or_else(|_| serde_json::json!({})))
            .collect(),
    )
}

async fn get_trace(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    build_trace_payload(&state, &id, None, None).await
}

async fn export_geometry(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<GeometryExportRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    resolve_workspace_input(&state.workspace_root, &payload.plc_file).map_err(bad_request)?;
    for reference in [payload.trace.as_deref(), payload.intent_report.as_deref()]
        .into_iter()
        .flatten()
    {
        resolve_artifact_reference(&state.workspace_root, reference)
            .ok_or_else(|| bad_request("invalid artifact reference"))?;
    }
    let _permit = state
        .run_semaphore
        .clone()
        .try_acquire_owned()
        .map_err(|_| too_many_requests("run concurrency limit reached"))?;
    let export_id = new_run_id("geometry");
    let out_dir = workspace_output_root(&state.workspace_root)
        .map_err(internal_error)?
        .join("web_geometry");
    std::fs::create_dir_all(&out_dir).map_err(internal_error)?;
    let out_path = out_dir.join(format!("{export_id}.json"));

    let geometry_href = generate_geometry_artifact(
        &state,
        &out_path,
        &payload.plc_file,
        payload.trace.as_deref(),
        payload.intent_report.as_deref(),
    )
    .await
    .map_err(internal_error)?
    .ok_or_else(|| internal_error("geometry export did not produce an artifact"))?;

    let geometry_path = resolve_artifact_reference(&state.workspace_root, &geometry_href)
        .ok_or_else(|| internal_error("geometry artifact path is invalid"))?;
    let value = read_json_value(&geometry_path)?;

    Ok(Json(serde_json::json!({
        "geometry_ref": geometry_href,
        "artifact": value
    })))
}

async fn get_geometry(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let runs = state.runs.read().await;
    let run = runs
        .get(&id)
        .cloned()
        .ok_or_else(|| not_found(format!("run `{id}` not found")))?;
    drop(runs);

    let Some(geometry_ref) = run.artifacts.geometry.clone() else {
        return Ok(Json(serde_json::json!({
            "schema_version": 1,
            "artifact_kind": "semantic_twin_geometry",
            "status": "missing"
        })));
    };

    let Some(path) = resolve_artifact_reference(&state.workspace_root, &geometry_ref) else {
        return Err(not_found("geometry artifact path is invalid".to_string()));
    };
    read_json_file(&path)
}

async fn get_trace_range(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(range): Query<TickRangeQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    build_trace_payload(&state, &id, range.start, range.end).await
}

async fn build_trace_payload(
    state: &Arc<AppState>,
    run_id: &str,
    start: Option<u64>,
    end: Option<u64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let runs = state.runs.read().await;
    let run = runs
        .get(run_id)
        .cloned()
        .ok_or_else(|| not_found(format!("run `{run_id}` not found")))?;
    drop(runs);

    let Some(trace_ref) = run.artifacts.trace.clone() else {
        return Ok(Json(serde_json::json!({
            "schema_version": 1,
            "tick_ms": run.tick_ms.unwrap_or(10),
            "ticks": []
        })));
    };

    let trace_path = resolve_artifact_reference(&state.workspace_root, &trace_ref)
        .ok_or_else(|| not_found("trace artifact path is invalid".to_string()))?;
    let replay_trace_path = preferred_replay_trace_path(&trace_path);

    if replay_trace_path.extension().and_then(|s| s.to_str()) == Some("json") {
        let value = read_json_file(&replay_trace_path)?.0;
        return Ok(Json(value));
    }

    let text = std::fs::read_to_string(&replay_trace_path).map_err(internal_error)?;
    let mut ticks = Vec::<Value>::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(row) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(tick) = row.get("tick").and_then(Value::as_u64) else {
            continue;
        };
        if let Some(s) = start {
            if tick < s {
                continue;
            }
        }
        if let Some(e) = end {
            if tick > e {
                continue;
            }
        }

        if row.get("components").is_some() {
            ticks.push(serde_json::json!({
                "tick": tick,
                "digital_inputs": [],
                "analog_inputs": [],
                "digital_outputs": [],
                "analog_outputs": [],
                "component_states": row.get("components").cloned().unwrap_or_else(|| serde_json::json!({}))
            }));
        } else {
            ticks.push(row);
        }
    }

    Ok(Json(serde_json::json!({
        "schema_version": 1,
        "tick_ms": run.tick_ms.unwrap_or(10),
        "ticks": ticks
    })))
}

fn preferred_replay_trace_path(trace_path: &StdPath) -> PathBuf {
    let Some(file_name) = trace_path.file_name().and_then(|value| value.to_str()) else {
        return trace_path.to_path_buf();
    };

    if file_name == "sil_trace.jsonl" {
        let io_snapshot_path = trace_path.with_file_name("io_snapshot.json");
        if io_snapshot_path.exists() {
            return io_snapshot_path;
        }
    }

    trace_path.to_path_buf()
}

async fn generate_geometry_for_run(
    state: &Arc<AppState>,
    out_dir: &StdPath,
    plc_file: &Option<String>,
    trace_href: Option<&str>,
    intent_report_href: Option<&str>,
) -> Result<Option<String>, String> {
    let Some(plc_file) = plc_file.as_deref() else {
        return Ok(None);
    };

    let out_path = out_dir.join("geometry.json");
    generate_geometry_artifact(state, &out_path, plc_file, trace_href, intent_report_href).await
}

async fn generate_geometry_artifact(
    state: &Arc<AppState>,
    out_path: &StdPath,
    plc_file: &str,
    trace_href: Option<&str>,
    intent_report_href: Option<&str>,
) -> Result<Option<String>, String> {
    let args = build_geometry_export_args(
        &state.workspace_root,
        out_path,
        plc_file,
        trace_href,
        intent_report_href,
    );
    let command = run_rust_plc(state, &args).await?;
    if !command.success {
        return Err(public_command_failure(&state.workspace_root, &command));
    }
    if !out_path.exists() {
        return Ok(None);
    }
    Ok(Some(artifact_href(&state.workspace_root, out_path)))
}

fn build_geometry_export_args(
    workspace_root: &StdPath,
    out_path: &StdPath,
    plc_file: &str,
    trace_href: Option<&str>,
    intent_report_href: Option<&str>,
) -> Vec<String> {
    let mut args = vec![
        "geometry-export".to_string(),
        (if is_safe_relative_path(plc_file) {
            workspace_root.join(plc_file)
        } else {
            workspace_root.join("__invalid_web_input__")
        })
        .display()
        .to_string(),
        "--out".to_string(),
        out_path.display().to_string(),
        "--output".to_string(),
        "json".to_string(),
    ];

    if let Some(trace_href) = trace_href {
        if let Some(trace_path) = resolve_artifact_reference(workspace_root, trace_href) {
            args.push("--trace".to_string());
            args.push(trace_path.display().to_string());
        }
    }
    if let Some(intent_report_href) = intent_report_href {
        if let Some(intent_path) = resolve_artifact_reference(workspace_root, intent_report_href) {
            args.push("--intent-report".to_string());
            args.push(intent_path.display().to_string());
        }
    }

    args
}

async fn get_keypoints(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let runs = state.runs.read().await;
    let run = runs
        .get(&id)
        .cloned()
        .ok_or_else(|| not_found(format!("run `{id}` not found")))?;
    drop(runs);

    let Some(keypoints_ref) = run.artifacts.keypoints.clone() else {
        return Ok(Json(serde_json::json!({
            "schema_version": 1,
            "tick_ms": run.tick_ms.unwrap_or(10),
            "keypoints": []
        })));
    };
    let Some(path) = resolve_artifact_reference(&state.workspace_root, &keypoints_ref) else {
        return Ok(Json(serde_json::json!({
            "schema_version": 1,
            "tick_ms": run.tick_ms.unwrap_or(10),
            "keypoints": []
        })));
    };
    read_json_file(&path)
}

async fn get_diagnosis(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let runs = state.runs.read().await;
    let run = runs
        .get(&id)
        .cloned()
        .ok_or_else(|| not_found(format!("run `{id}` not found")))?;
    drop(runs);

    let Some(diag_ref) = run.artifacts.diagnosis.clone() else {
        return Ok(Json(serde_json::json!({
            "schema_version": 1,
            "candidates": []
        })));
    };
    let Some(path) = resolve_artifact_reference(&state.workspace_root, &diag_ref) else {
        return Ok(Json(serde_json::json!({
            "schema_version": 1,
            "candidates": []
        })));
    };
    read_json_file(&path)
}

async fn get_timing(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let runs = state.runs.read().await;
    let run = runs
        .get(&id)
        .cloned()
        .ok_or_else(|| not_found(format!("run `{id}` not found")))?;
    drop(runs);

    let Some(timing_ref) = run.artifacts.timing.clone() else {
        return Ok(Json(serde_json::json!({
            "schema_version": 1,
            "tick_ms": run.tick_ms.unwrap_or(10),
            "total_ticks": 0,
            "statistics": {
                "p50_exec_us": 0,
                "p95_exec_us": 0,
                "p99_exec_us": 0,
                "max_exec_us": 0,
                "overrun_count": 0
            }
        })));
    };
    let Some(path) = resolve_artifact_reference(&state.workspace_root, &timing_ref) else {
        return Ok(Json(serde_json::json!({
            "schema_version": 1,
            "tick_ms": run.tick_ms.unwrap_or(10),
            "total_ticks": 0,
            "statistics": {
                "p50_exec_us": 0,
                "p95_exec_us": 0,
                "p99_exec_us": 0,
                "max_exec_us": 0,
                "overrun_count": 0
            }
        })));
    };
    read_json_file(&path)
}

async fn get_alarms(
    State(state): State<Arc<AppState>>,
    Query(params): Query<AlarmQuery>,
) -> Json<Vec<Value>> {
    let runs = state.runs.read().await;
    let mut alarms = runs
        .values()
        .filter(|run| run.status == "fail")
        .map(|run| {
            serde_json::json!({
                "alarm_id": format!("alarm-{}", run.run_id),
                "severity": infer_alarm_severity(run),
                "first_seen_ms": run.triggered_at_ms,
                "top_candidates": [],
                "evidence_ref": run.artifacts.diagnosis,
                "evidence_source": if run.mode == "component_sim" { "no_board" } else { "mixed" },
                "scenario_or_recipe_id": run.scenario_file.clone().unwrap_or_else(|| "unknown".to_string())
            })
        })
        .collect::<Vec<_>>();

    if let Some(severity) = params.severity {
        alarms.retain(|a| a.get("severity").and_then(Value::as_str) == Some(severity.as_str()));
    }
    alarms.sort_by(|a, b| {
        b.get("first_seen_ms")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .cmp(&a.get("first_seen_ms").and_then(Value::as_u64).unwrap_or(0))
    });
    if let Some(limit) = params.limit {
        alarms.truncate(limit);
    }
    Json(alarms)
}

async fn ack_alarm(Path(id): Path<String>, Json(payload): Json<AckAlarmRequest>) -> Json<Value> {
    Json(serde_json::json!({
        "acknowledged": true,
        "alarm_id": id,
        "comment": payload.comment.unwrap_or_else(|| "".to_string())
    }))
}

fn parse_tick_ms_from_scenario(workspace_root: &StdPath, scenario: &Option<String>) -> Option<u64> {
    let raw = scenario.as_ref()?;
    let path = resolve_workspace_input(workspace_root, raw).ok()?;
    let value = read_scenario_value(&path).ok()?;
    value.get("tick_ms").and_then(Value::as_u64)
}

fn topology_path_for_id(workspace_root: &StdPath, id: &str) -> Result<PathBuf, String> {
    if id == "component_model" {
        resolve_workspace_output_path(workspace_root, "examples/component_model/topology.json")
    } else {
        workspace_path_for_id(workspace_root, "examples", id, ".topology.json")
    }
}

fn scenario_path_for_id(workspace_root: &StdPath, id: &str) -> Result<PathBuf, String> {
    if id == "component_model" {
        resolve_workspace_output_path(
            workspace_root,
            "examples/component_model/scenario_normal.json",
        )
    } else {
        workspace_path_for_id(workspace_root, "examples", id, ".scenario.json")
    }
}

fn workspace_path_for_id(
    workspace_root: &StdPath,
    directory: &str,
    id: &str,
    suffix: &str,
) -> Result<PathBuf, String> {
    if !is_safe_resource_id(id) {
        return Err("invalid resource id".to_string());
    }
    resolve_workspace_output_path(workspace_root, &format!("{directory}/{id}{suffix}"))
}

fn is_safe_resource_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
}

fn validate_run_request(
    workspace_root: &StdPath,
    payload: &TriggerRunRequest,
) -> Result<(), (StatusCode, Json<Value>)> {
    let scenario = payload
        .scenario_file
        .as_deref()
        .ok_or_else(|| bad_request("scenario_file is required"))?;
    let scenario_path = resolve_workspace_input(workspace_root, scenario).map_err(bad_request)?;
    let scenario_value = read_scenario_value(&scenario_path).map_err(bad_request)?;
    validate_scenario_limits(&scenario_value).map_err(bad_request)?;

    if let Some(plc_file) = payload.plc_file.as_deref() {
        resolve_workspace_input(workspace_root, plc_file).map_err(bad_request)?;
    }
    if let Some(topology_file) = payload.topology_file.as_deref() {
        resolve_workspace_input(workspace_root, topology_file).map_err(bad_request)?;
    }
    if payload.topology_file.is_none() && payload.plc_file.is_none() {
        return Err(bad_request("plc_file or topology_file is required"));
    }
    Ok(())
}

fn read_scenario_value(path: &StdPath) -> Result<Value, String> {
    let text = read_text_file_limited(path, MAX_INPUT_FILE_BYTES)
        .map_err(|_| "scenario file could not be read".to_string())?;
    serde_json::from_str::<Value>(&text)
        .or_else(|_| serde_yaml::from_str::<Value>(&text))
        .map_err(|_| "scenario file must contain valid JSON or YAML".to_string())
}

fn validate_scenario_limits(payload: &Value) -> Result<(), String> {
    let tick_ms = payload.get("tick_ms").and_then(Value::as_u64);
    let duration_ms = payload.get("duration_ms").and_then(Value::as_u64);
    if let Some(tick_ms) = tick_ms {
        if !(MIN_SCENARIO_TICK_MS..=MAX_SCENARIO_TICK_MS).contains(&tick_ms) {
            return Err(format!(
                "tick_ms must be between {MIN_SCENARIO_TICK_MS} and {MAX_SCENARIO_TICK_MS}"
            ));
        }
    }
    if let Some(duration_ms) = duration_ms {
        if duration_ms > MAX_SCENARIO_DURATION_MS {
            return Err(format!(
                "duration_ms must not exceed {MAX_SCENARIO_DURATION_MS}"
            ));
        }
    }
    if let (Some(tick_ms), Some(duration_ms)) = (tick_ms, duration_ms) {
        let ticks = duration_ms.saturating_add(tick_ms - 1) / tick_ms;
        if ticks > MAX_SCENARIO_TICKS {
            return Err(format!(
                "scenario must not exceed {MAX_SCENARIO_TICKS} ticks"
            ));
        }
    }
    Ok(())
}

fn prune_run_records(runs: &mut BTreeMap<String, RunRecord>) {
    while runs.len() >= MAX_RUN_RECORDS {
        let Some(oldest_id) = runs
            .values()
            .filter(|run| run.status != "running")
            .min_by_key(|run| run.triggered_at_ms)
            .map(|run| run.run_id.clone())
        else {
            break;
        };
        runs.remove(&oldest_id);
    }
}

fn display_rel(workspace_root: &StdPath, path: &StdPath) -> String {
    let root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    path.strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"))
}

fn write_json_pretty(path: &StdPath, payload: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create parent directory: {err}"))?;
    }
    let mut body = serde_json::to_string_pretty(payload)
        .map_err(|err| format!("failed to serialize JSON: {err}"))?;
    body.push('\n');
    std::fs::write(path, body).map_err(|err| format!("failed to write file: {err}"))
}

fn read_json_file(path: &StdPath) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let value = read_json_value(path)?;
    Ok(Json(value))
}

fn read_json_value(path: &StdPath) -> Result<Value, (StatusCode, Json<Value>)> {
    let text = read_text_file_limited(path, MAX_ARTIFACT_BYTES).map_err(internal_error)?;
    serde_json::from_str::<Value>(&text).map_err(|_| bad_request("file contains invalid JSON"))
}

fn normalize_topology_tags_in_place(value: &mut Value) {
    let Some(root) = value.as_object_mut() else {
        return;
    };
    root.insert(
        "tags_schema_version".to_string(),
        Value::from(TAGS_SCHEMA_VERSION),
    );
    let Some(components) = root.get_mut("components").and_then(Value::as_array_mut) else {
        return;
    };
    for component in components {
        let Some(component_obj) = component.as_object_mut() else {
            continue;
        };
        let params = component_obj
            .entry("params")
            .or_insert_with(|| Value::Object(Map::new()));
        if !params.is_object() {
            *params = Value::Object(Map::new());
        }
        let Some(params_obj) = params.as_object_mut() else {
            continue;
        };
        let normalized_tags = normalize_tags_value(params_obj.get("tags"));
        params_obj.insert("tags".to_string(), normalized_tags);
    }
}

fn normalize_tags_value(raw: Option<&Value>) -> Value {
    let source = raw.and_then(Value::as_object);
    let mut out = Map::new();
    out.insert(
        "functional_group".to_string(),
        Value::Array(normalize_tag_dimension(source, "functional_group")),
    );
    out.insert(
        "danger_level".to_string(),
        Value::Array(normalize_tag_dimension(source, "danger_level")),
    );
    out.insert(
        "location_group".to_string(),
        Value::Array(normalize_tag_dimension(source, "location_group")),
    );
    Value::Object(out)
}

fn normalize_tag_dimension(source: Option<&Map<String, Value>>, key: &str) -> Vec<Value> {
    source
        .and_then(|obj| obj.get(key))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(|value| Value::String(value.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

async fn get_artifact(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let reference = format!("/artifacts/{path}");
    let artifact = resolve_artifact_reference(&state.workspace_root, &reference)
        .ok_or_else(|| not_found("artifact not found"))?;
    let metadata = std::fs::metadata(&artifact).map_err(|_| not_found("artifact not found"))?;
    if !metadata.is_file() {
        return Err(not_found("artifact not found"));
    }
    if metadata.len() > MAX_ARTIFACT_BYTES {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(serde_json::json!({ "error": "artifact exceeds download limit" })),
        ));
    }
    let body = std::fs::read(&artifact).map_err(internal_error)?;
    let content_type = match artifact.extension().and_then(|value| value.to_str()) {
        Some("json") | Some("jsonl") => "application/json",
        Some("txt") | Some("log") => "text/plain; charset=utf-8",
        Some("csv") => "text/csv; charset=utf-8",
        _ => "application/octet-stream",
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(Body::from(body))
        .map_err(internal_error)
}

fn infer_alarm_severity(run: &RunRecord) -> &'static str {
    if run.failure_summary.is_some() {
        "critical"
    } else {
        "warning"
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn new_run_id(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4())
}

fn iso_like_timestamp(ms: u64) -> String {
    OffsetDateTime::from_unix_timestamp_nanos((ms as i128) * 1_000_000)
        .ok()
        .and_then(|dt| dt.format(&Rfc3339).ok())
        .unwrap_or_else(|| ms.to_string())
}

fn map_plc_device_to_component_id(kind: &DeviceType) -> &'static str {
    match kind {
        DeviceType::DigitalOutput => "switch",
        DeviceType::DigitalInput => "sensor",
        DeviceType::SolenoidValve => "switch",
        DeviceType::Cylinder => "cylinder",
        DeviceType::Sensor => "sensor",
        DeviceType::Motor => "stepper_pd",
        DeviceType::StepperMotor => "stepper_pd",
        DeviceType::Vfd => "stepper_pd",
        DeviceType::ServoDrive => "stepper_pd",
        DeviceType::AnalogInput => "sensor",
        DeviceType::AnalogOutput => "stepper_pd",
        DeviceType::Pid => "generic",
        DeviceType::Plc => "generic",
        DeviceType::CamCoupling => "generic",
        DeviceType::ProportionalValve => "generic",
        DeviceType::Gripper => "generic",
        DeviceType::Conveyor => "generic",
        DeviceType::Pump => "generic",
        DeviceType::Heater => "generic",
        DeviceType::VisionSensor => "generic",
    }
}

fn plc_device_type_name(kind: &DeviceType) -> &'static str {
    match kind {
        DeviceType::DigitalOutput => "digital_output",
        DeviceType::DigitalInput => "digital_input",
        DeviceType::SolenoidValve => "solenoid_valve",
        DeviceType::Cylinder => "cylinder",
        DeviceType::Sensor => "sensor",
        DeviceType::Motor => "motor",
        DeviceType::StepperMotor => "stepper_motor",
        DeviceType::Vfd => "vfd",
        DeviceType::ServoDrive => "servo_drive",
        DeviceType::AnalogInput => "analog_input",
        DeviceType::AnalogOutput => "analog_output",
        DeviceType::Pid => "pid",
        DeviceType::Plc => "plc",
        DeviceType::CamCoupling => "cam_coupling",
        DeviceType::ProportionalValve => "proportional_valve",
        DeviceType::Gripper => "gripper",
        DeviceType::Conveyor => "conveyor",
        DeviceType::Pump => "pump",
        DeviceType::Heater => "heater",
        DeviceType::VisionSensor => "vision_sensor",
    }
}

fn endpoint_kind_for_device_type(kind: &DeviceType) -> &'static str {
    match kind {
        DeviceType::DigitalInput
        | DeviceType::DigitalOutput
        | DeviceType::AnalogInput
        | DeviceType::AnalogOutput => "controller_port",
        DeviceType::Plc => "controller_device",
        DeviceType::SolenoidValve
        | DeviceType::Cylinder
        | DeviceType::Sensor
        | DeviceType::Motor
        | DeviceType::StepperMotor
        | DeviceType::Vfd
        | DeviceType::ServoDrive
        | DeviceType::Pid
        | DeviceType::CamCoupling
        | DeviceType::ProportionalValve
        | DeviceType::Gripper
        | DeviceType::Conveyor
        | DeviceType::Pump
        | DeviceType::Heater
        | DeviceType::VisionSensor => "process_device",
    }
}

fn bad_request(message: impl Into<String>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({
            "error": message.into()
        })),
    )
}

fn not_found(message: impl Into<String>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "error": message.into()
        })),
    )
}

fn too_many_requests(message: impl Into<String>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::TOO_MANY_REQUESTS,
        Json(serde_json::json!({
            "error": message.into()
        })),
    )
}

fn internal_error<E: std::fmt::Display>(err: E) -> (StatusCode, Json<Value>) {
    error!("internal web server error: {}", err);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({
            "error": "internal server error"
        })),
    )
}

#[cfg(test)]
mod tests;
