use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_path(p: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(p)
}

fn temp_dir(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "{prefix}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn write_no_board_gate_fixture_plc(path: &Path) {
    let plc = r#"
[topology]
device X0: digital_input
device Y0: digital_output

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
    fs::write(path, plc).expect("write fixture plc");
}

#[test]
fn scenario_validate_json_output_includes_stable_error_code() {
    let base = temp_dir("rust_plc_m6_validate_json");
    let scenario = base.join("bad_input.yaml");
    fs::write(
        &scenario,
        r#"
tick_ms: 10
duration_ms: 100
inputs:
  - at_ms: 0
    set:
      digital_inputs:
        999: true
"#,
    )
    .expect("write scenario");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("scenario-validate")
        .arg(repo_path("examples/assembly_station.plc"))
        .arg("--scenario")
        .arg(&scenario)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run scenario-validate");

    assert!(
        !output.status.success(),
        "scenario-validate should fail for invalid input"
    );

    let report: Value =
        serde_json::from_slice(&output.stdout).expect("scenario-validate should print JSON report");
    assert_eq!(
        report.get("command").and_then(Value::as_str),
        Some("scenario-validate")
    );
    assert_eq!(report.get("status").and_then(Value::as_str), Some("fail"));

    let issues = report
        .get("issues")
        .and_then(Value::as_array)
        .expect("issues array");
    assert!(
        issues.iter().any(|i| {
            i.get("code")
                .and_then(Value::as_str)
                .map(|s| s.starts_with("SCN-"))
                .unwrap_or(false)
        }),
        "at least one issue should carry SCN-* error code"
    );
}

#[test]
fn scenario_doctor_fix_preview_reports_same_tick_risk() {
    let base = temp_dir("rust_plc_m6_doctor");
    let scenario = base.join("risk.yaml");
    fs::write(
        &scenario,
        r#"
tick_ms: 10
duration_ms: 100
inputs:
  - at_ms: 0
    set:
      digital_inputs:
        10: true
"#,
    )
    .expect("write scenario");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("scenario-doctor")
        .arg(repo_path("examples/assembly_station.plc"))
        .arg("--scenario")
        .arg(&scenario)
        .arg("--fix-preview")
        .arg("--output")
        .arg("json")
        .output()
        .expect("run scenario-doctor");

    assert!(
        output.status.success(),
        "scenario-doctor should succeed for warning-only report, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value =
        serde_json::from_slice(&output.stdout).expect("scenario-doctor should print JSON report");
    assert_eq!(
        report.get("command").and_then(Value::as_str),
        Some("scenario-doctor")
    );

    let issues = report
        .get("issues")
        .and_then(Value::as_array)
        .expect("issues array");
    let risk = issues
        .iter()
        .find(|i| i.get("code").and_then(Value::as_str) == Some("SCN-RISK-001"));
    assert!(
        risk.is_some(),
        "expected same-tick risk warning (SCN-RISK-001)"
    );
    assert!(
        risk.and_then(|i| i.get("suggestion"))
            .and_then(Value::as_str)
            .map(|s| s.contains("digital_inputs"))
            .unwrap_or(false),
        "fix preview should include actionable snippet"
    );
}

#[test]
fn no_board_gate_and_build_support_json_output_mode() {
    let base = temp_dir("rust_plc_m6_output_mode");

    let plc = base.join("fixture.plc");
    let scenario = base.join("scenario.yaml");
    write_no_board_gate_fixture_plc(&plc);
    fs::write(
        &scenario,
        r#"
tick_ms: 10
duration_ms: 40
inputs:
  - at_ms: 10
    set:
      digital_inputs:
        0: true
"#,
    )
    .expect("write gate scenario");

    let gate_out = base.join("gate_artifacts");
    let gate = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("no-board-gate")
        .arg(&plc)
        .arg("--scenario")
        .arg(&scenario)
        .arg("--out-dir")
        .arg(&gate_out)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run no-board-gate");
    assert!(
        gate.status.success(),
        "no-board-gate should pass in JSON mode, stderr: {}",
        String::from_utf8_lossy(&gate.stderr)
    );
    let gate_json: Value = serde_json::from_slice(&gate.stdout).expect("gate json");
    assert_eq!(
        gate_json.get("command").and_then(Value::as_str),
        Some("no-board-gate")
    );

    let build_out = base.join("build_artifacts");
    let build = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("build-rp2040")
        .arg(&plc)
        .arg("--out")
        .arg(&build_out)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run build-rp2040");
    assert!(
        build.status.success(),
        "build-rp2040 should pass in JSON mode, stderr: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let build_json: Value = serde_json::from_slice(&build.stdout).expect("build json");
    assert_eq!(
        build_json.get("command").and_then(Value::as_str),
        Some("build-rp2040")
    );
    assert_eq!(
        build_json.get("status").and_then(Value::as_str),
        Some("pass")
    );
}
