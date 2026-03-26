use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
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

fn write_fixture_plc(path: &Path) {
    let plc = r#"
[topology]
device plc_main: plc {
    purpose: "IO 快照诊断测试控制器",
    model_ref: openplc_softplc
}

[constraints]

[tasks]
task main:
    step wait_start:
        wait: X0 == true
        timeout: 20ms -> goto done
    step run:
        action: set Y0 on

task done:
    step halt:
        action: log "done"
"#;
    fs::write(path, plc).expect("write plc");
}

#[test]
fn sim_plc_can_emit_io_snapshot_and_trace_doctor_consumes_it() {
    let base = temp_dir("rust_plc_io_snapshot");
    let plc = base.join("fixture.plc");
    let scenario = base.join("scenario.yaml");
    let trace = base.join("trace.jsonl");
    let io_snapshot = base.join("io_snapshot.json");
    write_fixture_plc(&plc);

    fs::write(
        &scenario,
        r#"
tick_ms: 10
duration_ms: 50
inputs:
  - at_ms: 10
    set:
      digital_inputs:
        0: true
"#,
    )
    .expect("write scenario");

    let sim_out = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("sim-plc")
        .arg(&plc)
        .arg("--scenario")
        .arg(&scenario)
        .arg("--out")
        .arg(&trace)
        .arg("--io-snapshot-out")
        .arg(&io_snapshot)
        .output()
        .expect("run sim-plc with io snapshot");
    assert!(
        sim_out.status.success(),
        "sim-plc should succeed, stderr: {}",
        String::from_utf8_lossy(&sim_out.stderr)
    );
    assert!(trace.exists(), "trace should exist");
    assert!(io_snapshot.exists(), "io snapshot should exist");

    let snapshot_json: Value =
        serde_json::from_str(&fs::read_to_string(&io_snapshot).expect("read io snapshot"))
            .expect("io snapshot should be valid json");
    assert_eq!(
        snapshot_json.get("schema_version").and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        snapshot_json.get("tick_ms").and_then(Value::as_u64),
        Some(10)
    );
    let ticks = snapshot_json
        .get("ticks")
        .and_then(Value::as_array)
        .expect("ticks should be array");
    assert!(!ticks.is_empty(), "io snapshot ticks should not be empty");
    let first_tick = ticks.first().expect("first tick row");
    assert!(
        first_tick
            .get("digital_inputs")
            .and_then(Value::as_array)
            .is_some(),
        "digital_inputs should be present in snapshot rows"
    );
    assert!(
        first_tick
            .get("digital_outputs")
            .and_then(Value::as_array)
            .is_some(),
        "digital_outputs should be present in snapshot rows"
    );

    let doctor = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("trace-doctor")
        .arg(&plc)
        .arg("--scenario")
        .arg(&scenario)
        .arg("--trace")
        .arg(&trace)
        .arg("--io-snapshot")
        .arg(&io_snapshot)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run trace-doctor with io snapshot");
    assert!(
        doctor.status.success(),
        "trace-doctor should succeed, stderr: {}",
        String::from_utf8_lossy(&doctor.stderr)
    );

    let doctor_json: Value = serde_json::from_slice(&doctor.stdout).expect("trace-doctor json");
    let evidence_inputs = doctor_json
        .get("evidence_inputs")
        .and_then(Value::as_array)
        .expect("evidence_inputs should be array");
    assert!(
        evidence_inputs
            .iter()
            .any(|item| item.as_str() == Some("io_snapshot")),
        "trace-doctor should mark io_snapshot as consumed evidence"
    );
    assert!(
        doctor_json
            .get("artifacts")
            .and_then(Value::as_object)
            .and_then(|obj| obj.get("io_snapshot"))
            .and_then(Value::as_str)
            .map(|path| path.ends_with("io_snapshot.json"))
            .unwrap_or(false),
        "trace-doctor artifacts should include io_snapshot path"
    );
}
