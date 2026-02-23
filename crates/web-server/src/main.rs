use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use rust_plc::ast::{
    DeviceDeclaration, DevicePort, DeviceType, PortRole, PortType, TopologyConnection,
    TopologyRelation,
};
use rust_plc::component_scenario::parse_component_scenario_value;
use rust_plc::component_topology::parse_component_topology_value;
use rust_plc::parser::parse_plc;
use rust_plc::topology_semantic_gate::validate_topology_semantics;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path as StdPath, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::process::Command;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tracing::{error, info};

#[derive(Clone)]
struct AppState {
    workspace_root: PathBuf,
    runs: Arc<RwLock<BTreeMap<String, RunRecord>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RunArtifacts {
    trace: Option<String>,
    diff: Option<String>,
    timing: Option<String>,
    diagnosis: Option<String>,
    keypoints: Option<String>,
    fault_audit: Option<String>,
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

const TAGS_SCHEMA_VERSION: u64 = 1;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("web_server=debug,tower_http=debug")
        .init();

    let workspace_root = find_workspace_root();
    let state = Arc::new(AppState {
        workspace_root: workspace_root.clone(),
        runs: Arc::new(RwLock::new(BTreeMap::new())),
    });

    let api_routes = Router::new()
        .route("/projects", get(list_projects))
        .route("/topology/parse-plc", post(parse_plc_topology))
        .route("/topology/:id", get(get_topology).put(save_topology))
        .route("/topology/validate", post(validate_topology))
        .route("/scenario/:id", get(get_scenario).put(save_scenario))
        .route("/scenario/validate", post(validate_scenario))
        .route("/run/no-board-gate", post(trigger_no_board))
        .route("/run/:id/status", get(get_run_status))
        .route("/run/list", get(list_runs))
        .route("/trace/:id", get(get_trace))
        .route("/trace/:id/range", get(get_trace_range))
        .route("/trace/:id/keypoints", get(get_keypoints))
        .route("/diagnosis/:id", get(get_diagnosis))
        .route("/timing/:id", get(get_timing))
        .route("/alarms", get(get_alarms))
        .route("/alarms/:id/ack", post(ack_alarm))
        .with_state(state.clone());

    let artifacts_dir = workspace_root.join("out");
    let static_dist = workspace_root.join("web-ui/dist");
    let app = Router::new()
        .nest("/api", api_routes)
        .nest_service("/artifacts", ServeDir::new(artifacts_dir))
        .fallback_service(ServeDir::new(static_dist))
        .layer(CorsLayer::permissive());

    let addr = "0.0.0.0:8080";
    info!("RustPLC Web Server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind web server");
    axum::serve(listener, app).await.expect("run web server");
}

fn find_workspace_root() -> PathBuf {
    let start = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    start.join("../..").canonicalize().unwrap_or(start)
}

async fn list_projects(State(state): State<Arc<AppState>>) -> Json<Value> {
    let examples = state.workspace_root.join("examples");
    let mut projects = Vec::<Value>::new();

    if let Ok(entries) = std::fs::read_dir(&examples) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("plc") {
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                projects.push(serde_json::json!({
                    "id": stem,
                    "name": stem,
                    "path": display_rel(&state.workspace_root, &path),
                    "type": "plc"
                }));
            }
        }
    }

    let component_topology = examples.join("component_model/topology.json");
    if component_topology.exists() {
        projects.push(serde_json::json!({
            "id": "component_model",
            "name": "component_model",
            "path": display_rel(&state.workspace_root, &component_topology),
            "type": "component_topology"
        }));
    }

    projects.sort_by(|a, b| {
        a.get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .cmp(b.get("name").and_then(Value::as_str).unwrap_or(""))
    });

    Json(serde_json::json!({ "projects": projects }))
}

async fn get_topology(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let json_path = topology_path_for_id(&state.workspace_root, &id);
    if json_path.exists() {
        let mut value = read_json_value(&json_path)?;
        normalize_topology_tags_in_place(&mut value);
        return Ok(Json(value));
    }

    let plc_path = state.workspace_root.join(format!("examples/{id}.plc"));
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
    let path = topology_path_for_id(&state.workspace_root, &id);
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

async fn parse_plc_topology(
    Json(payload): Json<ParsePlcTopologyRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let normalized = payload.content.trim_start_matches('\u{feff}');
    let preview_plc =
        build_topology_preview_plc(normalized).unwrap_or_else(|| normalized.to_string());
    let program = parse_plc(&preview_plc)
        .map_err(|err| bad_request(format!("failed to parse PLC: {err}")))?;
    let semantic_gate = match validate_topology_semantics(&program.topology) {
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
        "semantic_gate": semantic_gate
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
        DeviceType::DigitalOutput => vec![device_port("out", PortType::Digital, PortRole::Producer)],
        DeviceType::DigitalInput => vec![device_port("in", PortType::Digital, PortRole::Consumer)],
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
        DeviceType::AnalogInput => vec![device_port("in", PortType::Analog, PortRole::Consumer)],
        DeviceType::AnalogOutput => vec![device_port("out", PortType::Analog, PortRole::Producer)],
        DeviceType::Pid => vec![
            device_port("in", PortType::Analog, PortRole::Consumer),
            device_port("out", PortType::Analog, PortRole::Producer),
        ],
    }
}

fn device_port(id: &str, port_type: PortType, role: PortRole) -> DevicePort {
    DevicePort {
        id: id.to_string(),
        port_type,
        role,
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
        if candidates.iter().any(|candidate| *candidate == preferred_id) {
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
    let json_path = scenario_path_for_id(&state.workspace_root, &id);
    if json_path.exists() {
        return read_json_file(&json_path);
    }

    let legacy_yaml = state
        .workspace_root
        .join(format!("examples/{id}_scenario.yaml"));
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
    let path = scenario_path_for_id(&state.workspace_root, &id);
    write_json_pretty(&path, &payload).map_err(internal_error)?;
    Ok(Json(serde_json::json!({
        "saved": true,
        "path": display_rel(&state.workspace_root, &path)
    })))
}

async fn validate_scenario(Json(payload): Json<Value>) -> Json<Value> {
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
    let run_id = format!("run-{}", now_ms());
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
        if let Err(err) = execute_run(task_state.clone(), task_run_id.clone(), task_payload).await {
            error!("run {} failed: {}", task_run_id, err);
            let mut runs = task_state.runs.write().await;
            if let Some(run) = runs.get_mut(&task_run_id) {
                run.status = "fail".to_string();
                run.failure_summary = Some(err);
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
    let out_dir = state.workspace_root.join("out/web_runs").join(&run_id);
    std::fs::create_dir_all(&out_dir)
        .map_err(|err| format!("failed to create run output directory: {err}"))?;

    let scenario_file = payload
        .scenario_file
        .clone()
        .ok_or_else(|| "scenario_file is required".to_string())?;

    let mut record_updates = RunArtifacts::default();
    let mut status = "fail".to_string();
    let mut failure_summary: Option<String> = None;

    if let Some(topology_file) = payload.topology_file.clone() {
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

        let command = run_rust_plc(&state.workspace_root, &args).await?;
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
            failure_summary = Some(first_failure_message(&command.stderr, &command.stdout));
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
            .clone()
            .ok_or_else(|| "plc_file is required for no-board-gate mode".to_string())?;

        let args = vec![
            "no-board-gate".to_string(),
            plc_file,
            "--scenario".to_string(),
            scenario_file,
            "--out-dir".to_string(),
            out_dir.display().to_string(),
            "--output".to_string(),
            "json".to_string(),
        ];

        let command = run_rust_plc(&state.workspace_root, &args).await?;
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
            failure_summary = Some(first_failure_message(&command.stderr, &command.stdout));
        }

        if let Some(v) = output_json {
            record_updates.trace = v
                .get("sil_trace")
                .and_then(Value::as_str)
                .map(|path| artifact_href_any(&state.workspace_root, path));
            record_updates.diff = v
                .get("diff_report")
                .and_then(Value::as_str)
                .map(|path| artifact_href_any(&state.workspace_root, path));
            record_updates.timing = v
                .get("timing_report")
                .and_then(Value::as_str)
                .map(|path| artifact_href_any(&state.workspace_root, path));
            record_updates.diagnosis = v
                .get("diagnosis_report")
                .and_then(Value::as_str)
                .map(|path| artifact_href_any(&state.workspace_root, path));
        }
    }

    let mut runs = state.runs.write().await;
    if let Some(run) = runs.get_mut(&run_id) {
        run.status = status;
        run.failure_summary = failure_summary;
        run.artifacts = record_updates;
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

    if trace_path.extension().and_then(|s| s.to_str()) == Some("json") {
        let value = read_json_file(&trace_path)?.0;
        return Ok(Json(value));
    }

    let text = std::fs::read_to_string(&trace_path).map_err(internal_error)?;
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

async fn run_rust_plc(workspace_root: &StdPath, args: &[String]) -> Result<CommandOutput, String> {
    let bin_name = if cfg!(windows) {
        "rust_plc.exe"
    } else {
        "rust_plc"
    };
    let bin_path = workspace_root.join("target/debug").join(bin_name);

    let output = if bin_path.exists() {
        info!("run {:?} {:?}", bin_path, args);
        Command::new(&bin_path)
            .args(args)
            .current_dir(workspace_root)
            .output()
            .await
            .map_err(|err| format!("failed to run rust_plc binary: {err}"))?
    } else {
        info!("run cargo {:?}", args);
        let mut cargo_args = vec!["run".to_string(), "--quiet".to_string(), "--".to_string()];
        cargo_args.extend(args.iter().cloned());
        Command::new("cargo")
            .args(cargo_args)
            .current_dir(workspace_root)
            .output()
            .await
            .map_err(|err| format!("failed to run cargo command: {err}"))?
    };

    Ok(CommandOutput {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

#[derive(Debug)]
struct CommandOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

fn first_failure_message(stderr: &str, stdout: &str) -> String {
    if let Some(line) = stderr.lines().find(|line| !line.trim().is_empty()) {
        return line.trim().to_string();
    }
    if let Some(line) = stdout.lines().find(|line| !line.trim().is_empty()) {
        return line.trim().to_string();
    }
    "command failed without details".to_string()
}

fn parse_tick_ms_from_scenario(workspace_root: &StdPath, scenario: &Option<String>) -> Option<u64> {
    let raw = scenario.as_ref()?;
    let path = resolve_relative_or_absolute(workspace_root, raw);
    let text = std::fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<Value>(&text).ok()?;
    value.get("tick_ms").and_then(Value::as_u64)
}

fn topology_path_for_id(workspace_root: &StdPath, id: &str) -> PathBuf {
    if id == "component_model" {
        workspace_root.join("examples/component_model/topology.json")
    } else {
        workspace_root.join(format!("examples/{id}.topology.json"))
    }
}

fn scenario_path_for_id(workspace_root: &StdPath, id: &str) -> PathBuf {
    if id == "component_model" {
        workspace_root.join("examples/component_model/scenario_normal.json")
    } else {
        workspace_root.join(format!("examples/{id}.scenario.json"))
    }
}

fn display_rel(workspace_root: &StdPath, path: &StdPath) -> String {
    path.strip_prefix(workspace_root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string_lossy().to_string())
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
    let text = std::fs::read_to_string(path).map_err(internal_error)?;
    serde_json::from_str::<Value>(&text)
        .map_err(|err| bad_request(format!("invalid JSON {}: {err}", path.display())))
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

fn artifact_href(workspace_root: &StdPath, path: &StdPath) -> String {
    let rel = path
        .strip_prefix(workspace_root.join("out"))
        .map(|p| p.to_string_lossy().replace('\\', "/"));
    match rel {
        Ok(rel) => format!("/artifacts/{rel}"),
        Err(_) => path.to_string_lossy().to_string(),
    }
}

fn artifact_href_any(workspace_root: &StdPath, raw_path: &str) -> String {
    let path = resolve_relative_or_absolute(workspace_root, raw_path);
    artifact_href(workspace_root, &path)
}

fn resolve_artifact_reference(workspace_root: &StdPath, reference: &str) -> Option<PathBuf> {
    if let Some(rel) = reference.strip_prefix("/artifacts/") {
        return Some(workspace_root.join("out").join(rel));
    }
    if reference.starts_with('/') || reference.contains(":\\") {
        return Some(PathBuf::from(reference));
    }
    Some(workspace_root.join(reference))
}

fn resolve_relative_or_absolute(workspace_root: &StdPath, raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        workspace_root.join(path)
    }
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

fn iso_like_timestamp(ms: u64) -> String {
    // Keep this dependency-free and parseable by `new Date(...)` in UI.
    format!("{}", ms)
}

fn map_plc_device_to_component_id(kind: &DeviceType) -> &'static str {
    match kind {
        DeviceType::DigitalOutput => "switch",
        DeviceType::DigitalInput => "sensor",
        DeviceType::SolenoidValve => "switch",
        DeviceType::Cylinder => "cylinder",
        DeviceType::Sensor => "sensor",
        DeviceType::Motor => "stepper_pd",
        DeviceType::AnalogInput => "sensor",
        DeviceType::AnalogOutput => "stepper_pd",
        DeviceType::Pid => "generic",
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
        DeviceType::AnalogInput => "analog_input",
        DeviceType::AnalogOutput => "analog_output",
        DeviceType::Pid => "pid",
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

fn internal_error<E: std::fmt::Display>(err: E) -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({
            "error": err.to_string()
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        find_workspace_root, normalize_topology_tags_in_place, parse_plc_topology,
        ParsePlcTopologyRequest, TAGS_SCHEMA_VERSION,
    };
    use axum::response::Json;
    use serde_json::{json, Value};

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
    async fn parse_plc_topology_returns_relation_port_and_tag_metadata() {
        let plc = r#"
[topology]
device Y0: digital_output {
    ports: [out:digital:producer]
}
device X0: digital_input {
    ports: [in:digital:consumer]
}
device valve_A: solenoid_valve {
    ports: [coil:digital:consumer, feedback:logical:producer],
    tags: {
        functional_group: [actuation],
        danger_level: [high],
        location_group: ["line_a/cell_2/station_7"]
    }
}
device sensor_A: sensor {
    ports: [sense:logical:consumer, out:digital:producer]
}

relation { from: Y0, to: valve_A.coil, via: driven_by }
relation { from: valve_A.feedback, to: sensor_A.sense, via: detects }
relation { from: sensor_A.out, to: X0, via: reports_to }

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
        assert_eq!(response["semantic_gate"]["valid"], json!(true));

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
    async fn parse_plc_topology_two_cylinder_keeps_extended_and_retracted_edges_distinct() {
        let root = find_workspace_root();
        let plc = std::fs::read_to_string(root.join("examples/two_cylinder.plc"))
            .expect("two_cylinder example should exist");

        let response = parse_plc_topology(Json(ParsePlcTopologyRequest { content: plc }))
            .await
            .expect("parse-plc API should parse two_cylinder")
            .0;
        assert_eq!(
            response["semantic_gate"]["valid"],
            json!(true),
            "two_cylinder should pass topology semantic gate"
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
}
