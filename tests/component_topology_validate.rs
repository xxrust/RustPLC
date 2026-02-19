use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "{prefix}_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn component_topology_validate_passes_and_emits_json_contract() {
    let base = temp_dir("rust_plc_component_topology_pass");
    let topology = base.join("topology.json");
    let normalized = base.join("normalized_topology.json");
    fs::write(
        &topology,
        r#"{
  "schema_version": 1,
  "component_library": {
    "schema_version": 1,
    "components": [
      { "id": "switch", "name": "Start", "type": "switch", "params": {} },
      { "id": "sensor", "name": "Front", "type": "sensor", "params": {} },
      { "id": "cylinder", "name": "Lift", "type": "cylinder", "params": {} }
    ]
  },
  "components": [
    { "id": "s_start", "component_id": "switch", "params": {} },
    { "id": "x_front", "component_id": "sensor", "params": {} },
    { "id": "cyl_a", "component_id": "cylinder", "params": {} }
  ],
  "connections": [
    { "from": "s_start.state", "to": "cyl_a.cmd_extend" },
    { "from": "x_front.state", "to": "cyl_a.cmd_retract" }
  ]
}"#,
    )
    .expect("write topology json");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("component-topology-validate")
        .arg(&topology)
        .arg("--normalized-out")
        .arg(&normalized)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run component-topology-validate");

    assert!(
        output.status.success(),
        "component-topology-validate should pass, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value =
        serde_json::from_slice(&output.stdout).expect("component-topology json payload");
    assert_eq!(
        payload.get("command").and_then(Value::as_str),
        Some("component-topology-validate")
    );
    assert_eq!(payload.get("status").and_then(Value::as_str), Some("pass"));
    assert_eq!(
        payload.get("component_count").and_then(Value::as_u64),
        Some(3)
    );
    assert_eq!(
        payload.get("connection_count").and_then(Value::as_u64),
        Some(2)
    );
    assert!(
        normalized.exists(),
        "normalized topology output should exist"
    );
}

#[test]
fn component_topology_validate_fails_on_invalid_connection_direction() {
    let base = temp_dir("rust_plc_component_topology_fail");
    let topology = base.join("topology_invalid.json");
    fs::write(
        &topology,
        r#"{
  "schema_version": 1,
  "component_library": {
    "schema_version": 1,
    "components": [
      { "id": "sensor", "name": "Front", "type": "sensor", "params": {} },
      { "id": "cylinder", "name": "Lift", "type": "cylinder", "params": {} }
    ]
  },
  "components": [
    { "id": "x_front", "component_id": "sensor", "params": {} },
    { "id": "cyl_a", "component_id": "cylinder", "params": {} }
  ],
  "connections": [
    { "from": "x_front.state", "to": "cyl_a.sensor_extended" }
  ]
}"#,
    )
    .expect("write topology invalid json");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("component-topology-validate")
        .arg(&topology)
        .output()
        .expect("run component-topology-validate fail");

    assert!(
        !output.status.success(),
        "component-topology-validate should fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[CTOP-000]"),
        "stderr should contain CTOP prefix, got: {stderr}"
    );
    assert!(
        stderr.contains("CTOP-CONN-008"),
        "stderr should contain stable issue code, got: {stderr}"
    );
}
