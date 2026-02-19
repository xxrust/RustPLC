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
fn component_scenario_validate_passes_and_writes_normalized_file() {
    let base = temp_dir("rust_plc_component_scenario_pass");
    let scenario = base.join("scenario.json");
    let normalized = base.join("normalized_scenario.json");
    fs::write(
        &scenario,
        r#"{
  "schema_version": 1,
  "tick_ms": 10,
  "duration_ms": 100,
  "switch_events": [{ "at_ms": 0, "target": "s0", "value": true }],
  "sensor_events": [{ "at_ms": 20, "target": "x0", "value": true }],
  "component_faults": [{ "at_ms": 40, "target_component_id": "m0", "fault_kind": "stall" }]
}"#,
    )
    .expect("write scenario");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("component-scenario-validate")
        .arg(&scenario)
        .arg("--normalized-out")
        .arg(&normalized)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run component-scenario-validate");

    assert!(
        output.status.success(),
        "component-scenario-validate should pass, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("json payload");
    assert_eq!(
        payload.get("command").and_then(Value::as_str),
        Some("component-scenario-validate")
    );
    assert_eq!(payload.get("status").and_then(Value::as_str), Some("pass"));
    assert_eq!(
        payload.get("component_fault_count").and_then(Value::as_u64),
        Some(1)
    );
    assert!(normalized.exists(), "normalized output should exist");
}

#[test]
fn component_scenario_validate_rejects_legacy_faults_and_forces_with_migration_hint() {
    let base = temp_dir("rust_plc_component_scenario_migration");
    let scenario = base.join("legacy_scenario.json");
    fs::write(
        &scenario,
        r#"{
  "schema_version": 1,
  "tick_ms": 10,
  "duration_ms": 100,
  "faults": [{ "sensor_stuck": { "at_ms": 20, "target": 0, "value": true } }],
  "forces": []
}"#,
    )
    .expect("write legacy scenario");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("component-scenario-validate")
        .arg(&scenario)
        .output()
        .expect("run component-scenario-validate fail");

    assert!(!output.status.success(), "legacy scenario should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[CSCN-000]"),
        "stderr should contain CSCN prefix, got: {stderr}"
    );
    assert!(
        stderr.contains("CSCN-MIG-001"),
        "stderr should report legacy faults code, got: {stderr}"
    );
    assert!(
        stderr.contains("CSCN-MIG-002"),
        "stderr should report legacy forces code, got: {stderr}"
    );
    assert!(
        stderr.contains("Migration hint"),
        "stderr should include migration hint, got: {stderr}"
    );
}
