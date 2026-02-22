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

fn array_has_string(value: &Value, needle: &str) -> bool {
    value
        .as_array()
        .map(|items| items.iter().any(|item| item.as_str() == Some(needle)))
        .unwrap_or(false)
}

#[test]
fn component_topology_diff_reports_match_for_identical_inputs() {
    let base = temp_dir("rust_plc_component_topology_diff_match");
    let before = base.join("before.json");
    let after = base.join("after.json");
    let report = base.join("semantic_diff.json");
    let topology = r#"{
  "schema_version": 1,
  "component_library": {
    "schema_version": 1,
    "components": [
      { "id": "switch", "name": "Switch", "type": "switch", "params": {} },
      { "id": "sensor", "name": "Sensor", "type": "sensor", "params": {} },
      { "id": "cylinder", "name": "Cylinder", "type": "cylinder", "params": {} }
    ]
  },
  "components": [
    { "id": "s0", "component_id": "switch", "params": {} },
    { "id": "x0", "component_id": "sensor", "params": {} },
    { "id": "c0", "component_id": "cylinder", "params": {} }
  ],
  "connections": [
    { "from": "s0.state", "to": "c0.cmd_extend" },
    { "from": "x0.state", "to": "c0.cmd_retract" }
  ]
}"#;
    fs::write(&before, topology).expect("write before topology");
    fs::write(&after, topology).expect("write after topology");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("component-topology-diff")
        .arg(&before)
        .arg(&after)
        .arg("--out")
        .arg(&report)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run component-topology-diff");

    assert!(
        output.status.success(),
        "component-topology-diff should pass, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(report.exists(), "semantic diff report should be written");
    let payload: Value = serde_json::from_slice(&output.stdout).expect("json output payload");
    assert_eq!(
        payload.get("command").and_then(Value::as_str),
        Some("component-topology-diff")
    );
    assert_eq!(
        payload.get("changes_detected").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(payload.get("node_changes").and_then(Value::as_u64), Some(0));
    assert_eq!(payload.get("port_changes").and_then(Value::as_u64), Some(0));
    assert_eq!(
        payload.get("relation_changes").and_then(Value::as_u64),
        Some(0)
    );
    assert_eq!(payload.get("tag_changes").and_then(Value::as_u64), Some(0));

    let report_json: Value =
        serde_json::from_str(&fs::read_to_string(&report).expect("read semantic diff report"))
            .expect("parse semantic diff report");
    assert_eq!(
        report_json.get("is_match").and_then(Value::as_bool),
        Some(true)
    );
}

#[test]
fn component_topology_diff_reports_semantic_changes_and_impact_analysis() {
    let base = temp_dir("rust_plc_component_topology_diff_changes");
    let before = base.join("before.json");
    let after = base.join("after.json");
    let report = base.join("semantic_diff.json");
    fs::write(
        &before,
        r#"{
  "schema_version": 1,
  "component_library": {
    "schema_version": 1,
    "components": [
      { "id": "switch", "name": "Switch", "type": "switch", "params": {} },
      { "id": "sensor", "name": "Sensor", "type": "sensor", "params": {} },
      { "id": "cylinder", "name": "Cylinder", "type": "cylinder", "params": {} },
      { "id": "stepper", "name": "Stepper", "type": "stepper_pd", "params": {} }
    ]
  },
  "components": [
    { "id": "s0", "component_id": "switch", "params": {} },
    { "id": "x0", "component_id": "sensor", "params": {} },
    {
      "id": "c0",
      "component_id": "cylinder",
      "params": {
        "tags": {
          "functional_group": ["actuation"],
          "danger_level": ["low"],
          "location_group": ["line_a/cell_1"]
        }
      }
    }
  ],
  "connections": [
    { "from": "s0.state", "to": "c0.cmd_extend" },
    { "from": "x0.state", "to": "c0.cmd_retract" }
  ]
}"#,
    )
    .expect("write before topology");
    fs::write(
        &after,
        r#"{
  "schema_version": 1,
  "component_library": {
    "schema_version": 1,
    "components": [
      { "id": "switch", "name": "Switch", "type": "switch", "params": {} },
      { "id": "sensor", "name": "Sensor", "type": "sensor", "params": {} },
      { "id": "cylinder", "name": "Cylinder", "type": "cylinder", "params": {} },
      { "id": "stepper", "name": "Stepper", "type": "stepper_pd", "params": {} }
    ]
  },
  "components": [
    { "id": "s0", "component_id": "switch", "params": {} },
    { "id": "x0", "component_id": "sensor", "params": {} },
    {
      "id": "c0",
      "component_id": "stepper",
      "params": {
        "tags": {
          "functional_group": ["motion"],
          "danger_level": ["high"],
          "location_group": ["line_b/cell_1"]
        }
      }
    },
    { "id": "m1", "component_id": "cylinder", "params": {} }
  ],
  "connections": [
    { "from": "s0.state", "to": "c0.pulse" },
    { "from": "x0.state", "to": "c0.direction" },
    { "from": "c0.position_steps", "to": "m1.cmd_extend" },
    { "from": "s0.state", "to": "m1.cmd_retract" }
  ]
}"#,
    )
    .expect("write after topology");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("component-topology-diff")
        .arg(&before)
        .arg(&after)
        .arg("--out")
        .arg(&report)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run component-topology-diff");

    assert!(
        output.status.success(),
        "component-topology-diff should pass, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("json output payload");
    assert_eq!(
        payload.get("changes_detected").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(payload.get("node_changes").and_then(Value::as_u64), Some(2));
    assert_eq!(payload.get("port_changes").and_then(Value::as_u64), Some(2));
    assert_eq!(payload.get("tag_changes").and_then(Value::as_u64), Some(1));

    let report_json: Value =
        serde_json::from_str(&fs::read_to_string(&report).expect("read semantic diff report"))
            .expect("parse semantic diff report");
    assert_eq!(
        report_json.get("is_match").and_then(Value::as_bool),
        Some(false)
    );
    assert!(
        report_json
            .get("nodes")
            .and_then(|nodes| nodes.get("added"))
            .and_then(Value::as_array)
            .map(|added| {
                added
                    .iter()
                    .any(|entry| entry.get("node_id").and_then(Value::as_str) == Some("m1"))
            })
            .unwrap_or(false),
        "semantic diff should include added node m1"
    );
    assert!(
        report_json
            .get("relations")
            .and_then(|relations| relations.get("added"))
            .and_then(Value::as_array)
            .map(|added| !added.is_empty())
            .unwrap_or(false),
        "semantic diff should include added relations"
    );
    assert!(
        array_has_string(
            report_json
                .get("impact")
                .and_then(|impact| impact.get("tag_change_nodes"))
                .unwrap_or(&Value::Null),
            "c0"
        ),
        "impact analysis should include c0 tag changes"
    );
    assert!(
        array_has_string(
            report_json
                .get("impact")
                .and_then(|impact| impact.get("high_risk_nodes"))
                .unwrap_or(&Value::Null),
            "c0"
        ),
        "danger_level updates should mark c0 as high risk"
    );
}
