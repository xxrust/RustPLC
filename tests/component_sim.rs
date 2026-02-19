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

fn write_topology(path: &PathBuf) {
    fs::write(
        path,
        r#"{
  "schema_version": 1,
  "component_library": {
    "schema_version": 1,
    "components": [
      { "id": "switch", "name": "Start", "type": "switch", "params": {} },
      { "id": "sensor", "name": "Front", "type": "sensor", "params": {} },
      { "id": "cylinder", "name": "Lift", "type": "cylinder", "params": { "stroke_ticks": 3 } },
      { "id": "stepper", "name": "Axis", "type": "stepper_pd", "params": {} }
    ]
  },
  "components": [
    { "id": "s0", "component_id": "switch", "params": {} },
    { "id": "x0", "component_id": "sensor", "params": {} },
    { "id": "c0", "component_id": "cylinder", "params": {} },
    { "id": "m0", "component_id": "stepper", "params": {} }
  ],
  "connections": [
    { "from": "s0.state", "to": "c0.cmd_extend" },
    { "from": "x0.state", "to": "c0.cmd_retract" },
    { "from": "s0.state", "to": "m0.pulse" },
    { "from": "x0.state", "to": "m0.direction" },
    { "from": "s0.state", "to": "m0.enable" }
  ]
}"#,
    )
    .expect("write topology");
}

#[test]
fn component_sim_writes_trace_audit_and_diagnosis_artifacts() {
    let base = temp_dir("rust_plc_component_sim_artifacts");
    let topology = base.join("topology.json");
    let scenario = base.join("scenario.json");
    let trace_out = base.join("trace.jsonl");
    let audit_out = base.join("fault_audit.jsonl");
    let diagnosis_out = base.join("diagnosis.json");
    let keypoints_out = base.join("keypoints.json");

    write_topology(&topology);
    fs::write(
        &scenario,
        r#"{
  "schema_version": 1,
  "tick_ms": 10,
  "duration_ms": 90,
  "switch_events": [
    { "at_ms": 0, "target": "s0", "value": true },
    { "at_ms": 20, "target": "s0", "value": false },
    { "at_ms": 40, "target": "s0", "value": true },
    { "at_ms": 60, "target": "s0", "value": false }
  ],
  "sensor_events": [{ "at_ms": 0, "target": "x0", "value": true }],
  "component_faults": [
    { "at_ms": 20, "duration_ms": 20, "target_component_id": "m0", "fault_kind": "stall" },
    { "at_ms": 50, "duration_ms": 20, "target_component_id": "x0", "fault_kind": "stuck_off" }
  ]
}"#,
    )
    .expect("write scenario");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("component-sim")
        .arg(&topology)
        .arg("--scenario")
        .arg(&scenario)
        .arg("--out")
        .arg(&trace_out)
        .arg("--fault-audit-out")
        .arg(&audit_out)
        .arg("--diagnosis-out")
        .arg(&diagnosis_out)
        .arg("--keypoints-out")
        .arg(&keypoints_out)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run component-sim");

    assert!(
        output.status.success(),
        "component-sim should pass, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("json payload");
    assert_eq!(payload.get("status").and_then(Value::as_str), Some("pass"));
    assert!(
        payload
            .get("fault_audit_count")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            >= 2
    );
    assert!(
        payload
            .get("diagnosis_count")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            >= 1
    );
    assert!(
        payload
            .get("keypoint_count")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            >= 1
    );
    assert!(trace_out.exists(), "trace output should exist");
    assert!(audit_out.exists(), "fault audit output should exist");
    assert!(diagnosis_out.exists(), "diagnosis output should exist");
    assert!(keypoints_out.exists(), "keypoints output should exist");

    let diagnosis_text = fs::read_to_string(&diagnosis_out).expect("read diagnosis");
    let diagnosis_json: Value = serde_json::from_str(&diagnosis_text).expect("parse diagnosis");
    let first = diagnosis_json
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .expect("first candidate");
    let evidence = first
        .get("evidence")
        .and_then(Value::as_array)
        .expect("evidence array");
    assert!(
        evidence.iter().any(|entry| {
            entry
                .get("source")
                .and_then(Value::as_str)
                .is_some_and(|source| source == "fault_injection")
        }),
        "should include fault injection evidence"
    );
    assert!(
        evidence.iter().any(|entry| {
            entry
                .get("source")
                .and_then(Value::as_str)
                .is_some_and(|source| source == "program_behavior")
        }),
        "should include program behavior evidence"
    );

    let keypoints_text = fs::read_to_string(&keypoints_out).expect("read keypoints");
    let keypoints_json: Value = serde_json::from_str(&keypoints_text).expect("parse keypoints");
    let keypoints = keypoints_json
        .get("keypoints")
        .and_then(Value::as_array)
        .expect("keypoints array");
    assert!(
        keypoints.iter().any(|item| {
            item.get("category")
                .and_then(Value::as_str)
                .is_some_and(|c| c == "fault_activated")
        }),
        "keypoints should include fault lifecycle markers"
    );
}

#[test]
fn component_sim_rejects_fault_target_type_mismatch() {
    let base = temp_dir("rust_plc_component_sim_target_mismatch");
    let topology = base.join("topology.json");
    let scenario = base.join("scenario_bad.json");

    write_topology(&topology);
    fs::write(
        &scenario,
        r#"{
  "schema_version": 1,
  "tick_ms": 10,
  "duration_ms": 50,
  "component_faults": [
    { "at_ms": 10, "target_component_id": "s0", "fault_kind": "jammed" }
  ]
}"#,
    )
    .expect("write scenario");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("component-sim")
        .arg(&topology)
        .arg("--scenario")
        .arg(&scenario)
        .output()
        .expect("run component-sim fail");

    assert!(!output.status.success(), "component-sim should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("CSIM-TGT-006"),
        "stderr should include target mismatch code, got: {stderr}"
    );
}
