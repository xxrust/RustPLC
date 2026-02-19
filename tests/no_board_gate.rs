use std::fs;
use std::path::Path;
use std::process::Command;

fn temp_dir(prefix: &str) -> std::path::PathBuf {
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

fn write_fixture_plc(path: &std::path::Path) {
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
    fs::write(path, plc).expect("write plc");
}

#[test]
fn no_board_gate_passes_for_matching_scenarios() {
    let base = temp_dir("rust_plc_no_board_gate_pass");
    let plc_path = base.join("fixture.plc");
    let scenario_path = base.join("scenario.yaml");
    let out_dir = base.join("out");
    write_fixture_plc(&plc_path);

    let scenario = r#"
tick_ms: 10
duration_ms: 40
inputs:
  - at_ms: 10
    set:
      digital_inputs:
        0: true
"#;
    fs::write(&scenario_path, scenario).expect("write scenario");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("no-board-gate")
        .arg(&plc_path)
        .arg("--scenario")
        .arg(&scenario_path)
        .arg("--out-dir")
        .arg(&out_dir)
        .output()
        .expect("run no-board-gate");

    assert!(
        output.status.success(),
        "no-board-gate should pass for matching scenarios, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let diff_report = out_dir.join("diff_report.json");
    assert!(diff_report.exists(), "diff report should exist");
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&diff_report).expect("read diff report json"))
            .expect("valid diff report json");
    assert_eq!(report.get("is_match").and_then(|v| v.as_bool()), Some(true));
}

#[test]
fn no_board_gate_fails_for_mismatching_scenarios() {
    let base = temp_dir("rust_plc_no_board_gate_fail");
    let plc_path = base.join("fixture.plc");
    let sil_scenario_path = base.join("scenario_sil.yaml");
    let board_scenario_path = base.join("scenario_board.yaml");
    let out_dir = base.join("out");
    write_fixture_plc(&plc_path);

    let sil_scenario = r#"
tick_ms: 10
duration_ms: 40
inputs:
  - at_ms: 10
    set:
      digital_inputs:
        0: true
"#;
    fs::write(&sil_scenario_path, sil_scenario).expect("write sil scenario");

    let board_scenario = r#"
tick_ms: 10
duration_ms: 40
inputs: []
"#;
    fs::write(&board_scenario_path, board_scenario).expect("write board scenario");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("no-board-gate")
        .arg(&plc_path)
        .arg("--sil-scenario")
        .arg(&sil_scenario_path)
        .arg("--board-scenario")
        .arg(&board_scenario_path)
        .arg("--out-dir")
        .arg(&out_dir)
        .output()
        .expect("run no-board-gate");

    assert!(
        !output.status.success(),
        "no-board-gate should fail when traces differ"
    );

    let diff_report = out_dir.join("diff_report.json");
    assert!(diff_report.exists(), "diff report should exist");
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&diff_report).expect("read diff report json"))
            .expect("valid diff report json");
    assert_eq!(
        report.get("is_match").and_then(|v| v.as_bool()),
        Some(false)
    );
}

#[test]
fn no_board_gate_fail_json_includes_diagnosis_artifact_fields() {
    let base = temp_dir("rust_plc_no_board_gate_fail_json");
    let plc_path = base.join("fixture.plc");
    let sil_scenario_path = base.join("scenario_sil.yaml");
    let board_scenario_path = base.join("scenario_board.yaml");
    let out_dir = base.join("out");
    write_fixture_plc(&plc_path);

    let sil_scenario = r#"
tick_ms: 10
duration_ms: 40
inputs:
  - at_ms: 10
    set:
      digital_inputs:
        0: true
"#;
    fs::write(&sil_scenario_path, sil_scenario).expect("write sil scenario");

    let board_scenario = r#"
tick_ms: 10
duration_ms: 40
inputs: []
"#;
    fs::write(&board_scenario_path, board_scenario).expect("write board scenario");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("no-board-gate")
        .arg(&plc_path)
        .arg("--sil-scenario")
        .arg(&sil_scenario_path)
        .arg("--board-scenario")
        .arg(&board_scenario_path)
        .arg("--out-dir")
        .arg(&out_dir)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run no-board-gate json fail");

    assert!(
        !output.status.success(),
        "no-board-gate should fail when traces differ"
    );

    let payload: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("no-board-gate should emit json payload on fail");
    assert_eq!(payload.get("status").and_then(|v| v.as_str()), Some("fail"));
    assert_eq!(
        payload
            .get("diagnosis_evidence_source")
            .and_then(|v| v.as_str()),
        Some("no_board")
    );
    let diagnosis_rel = payload
        .get("diagnosis_report")
        .and_then(|v| v.as_str())
        .expect("diagnosis_report should exist in fail json");
    assert!(
        payload
            .get("diagnosis_top_candidate_code")
            .and_then(|v| v.as_str())
            .is_some(),
        "diagnosis_top_candidate_code should be present"
    );

    let diagnosis_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(diagnosis_rel);
    assert!(diagnosis_path.exists(), "diagnosis report should exist");
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&diagnosis_path).expect("read diagnosis report"))
            .expect("diagnosis report should be valid json");
    assert!(
        report
            .get("candidates")
            .and_then(|v| v.as_array())
            .map(|items| !items.is_empty())
            .unwrap_or(false),
        "diagnosis report should contain candidates"
    );
}

#[test]
fn no_board_gate_passes_when_realtime_thresholds_are_within_limits() {
    let base = temp_dir("rust_plc_no_board_gate_rt_pass");
    let plc_path = base.join("fixture.plc");
    let scenario_path = base.join("scenario.yaml");
    let out_dir = base.join("out");
    write_fixture_plc(&plc_path);

    let scenario = r#"
tick_ms: 10
duration_ms: 40
inputs:
  - at_ms: 10
    set:
      digital_inputs:
        0: true
"#;
    fs::write(&scenario_path, scenario).expect("write scenario");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("no-board-gate")
        .arg(&plc_path)
        .arg("--scenario")
        .arg(&scenario_path)
        .arg("--out-dir")
        .arg(&out_dir)
        .arg("--max-p99-exec-us")
        .arg("200")
        .arg("--max-overrun-count")
        .arg("0")
        .output()
        .expect("run no-board-gate");

    assert!(
        output.status.success(),
        "no-board-gate should pass with relaxed realtime thresholds, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let timing_report = out_dir.join("timing_report.json");
    assert!(timing_report.exists(), "timing_report.json should exist");
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&timing_report).expect("read timing report json"))
            .expect("valid timing report json");
    assert!(
        report.get("count").and_then(|v| v.as_u64()).unwrap_or(0) > 0,
        "timing report should contain at least one tick sample"
    );
    assert_eq!(
        report.get("overrun_count").and_then(|v| v.as_u64()),
        Some(0)
    );
}

#[test]
fn no_board_gate_fails_when_realtime_threshold_is_exceeded() {
    let base = temp_dir("rust_plc_no_board_gate_rt_fail");
    let plc_path = base.join("fixture.plc");
    let scenario_path = base.join("scenario.yaml");
    let out_dir = base.join("out");
    write_fixture_plc(&plc_path);

    let scenario = r#"
tick_ms: 10
duration_ms: 40
inputs:
  - at_ms: 10
    set:
      digital_inputs:
        0: true
"#;
    fs::write(&scenario_path, scenario).expect("write scenario");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("no-board-gate")
        .arg(&plc_path)
        .arg("--scenario")
        .arg(&scenario_path)
        .arg("--out-dir")
        .arg(&out_dir)
        .arg("--max-p99-exec-us")
        .arg("1")
        .output()
        .expect("run no-board-gate");

    assert!(
        !output.status.success(),
        "no-board-gate should fail when realtime threshold is exceeded"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("realtime threshold exceeded"),
        "stderr should contain realtime threshold failure, got: {stderr}"
    );

    let diff_report = out_dir.join("diff_report.json");
    assert!(diff_report.exists(), "diff report should exist");
    let diff: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&diff_report).expect("read diff report json"))
            .expect("valid diff report json");
    assert_eq!(diff.get("is_match").and_then(|v| v.as_bool()), Some(true));
}
