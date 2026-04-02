use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

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

fn write_fixture_plc(path: &PathBuf) {
    let plc = r#"
[topology]
device plc_main: plc {
    purpose: "project-check fixture controller",
    model_ref: rp2040_softplc
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
    fs::write(path, plc).expect("write fixture plc");
}

#[test]
fn project_check_runs_real_command_chain_and_emits_aggregate_report() {
    let base = temp_dir("rust_plc_project_check");
    let plc = base.join("fixture.plc");
    let scenario = base.join("scenario.yaml");
    let out_dir = base.join("artifacts");
    write_fixture_plc(&plc);
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
    .expect("write fixture scenario");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("project-check")
        .arg(&plc)
        .arg("--scenario")
        .arg(&scenario)
        .arg("--out-dir")
        .arg(&out_dir)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run project-check");

    assert!(
        output.status.success(),
        "project-check should pass, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value =
        serde_json::from_slice(&output.stdout).expect("project-check should print JSON");
    assert_eq!(
        report.get("command").and_then(Value::as_str),
        Some("project-check")
    );
    assert_eq!(report.get("status").and_then(Value::as_str), Some("pass"));

    let steps = report
        .get("steps")
        .and_then(Value::as_array)
        .expect("steps array");
    assert_eq!(
        steps.len(),
        4,
        "project-check should run four concrete checks"
    );

    for step_name in [
        "compile_verify",
        "sequence_lint",
        "scenario_doctor",
        "no_board_gate",
    ] {
        assert!(
            steps.iter().any(|step| {
                step.get("name").and_then(Value::as_str) == Some(step_name)
                    && step.get("status").and_then(Value::as_str) == Some("pass")
            }),
            "expected project-check step `{step_name}` to pass"
        );
    }

    for rel in [
        "project_check_report.json",
        "compile_verify/verification_report.json",
        "sequence_lint/stderr.log",
        "scenario_doctor/report.json",
        "no_board_gate/report.json",
        "no_board_gate/artifacts/diff_report.json",
        "no_board_gate/artifacts/timing_report.json",
    ] {
        assert!(
            out_dir.join(rel).exists(),
            "expected project-check artifact to exist: {rel}"
        );
    }
}

#[test]
fn project_check_reports_failed_steps_and_exits_non_zero() {
    let base = temp_dir("rust_plc_project_check_fail");
    let plc = base.join("fixture.plc");
    let scenario = base.join("bad_scenario.yaml");
    let out_dir = base.join("artifacts");
    write_fixture_plc(&plc);
    fs::write(
        &scenario,
        r#"
tick_ms: 10
duration_ms: 40
inputs:
  - at_ms: 0
    set:
      digital_inputs:
        999: true
"#,
    )
    .expect("write bad scenario");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("project-check")
        .arg(&plc)
        .arg("--scenario")
        .arg(&scenario)
        .arg("--out-dir")
        .arg(&out_dir)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run failing project-check");

    assert!(
        !output.status.success(),
        "project-check should fail for a bad scenario"
    );

    let report: Value =
        serde_json::from_slice(&output.stdout).expect("project-check should still print JSON");
    assert_eq!(
        report.get("command").and_then(Value::as_str),
        Some("project-check")
    );
    assert_eq!(report.get("status").and_then(Value::as_str), Some("fail"));
    assert!(
        report
            .get("failed_steps")
            .and_then(Value::as_u64)
            .unwrap_or_default()
            > 0,
        "failed_steps should be greater than zero"
    );

    let steps = report
        .get("steps")
        .and_then(Value::as_array)
        .expect("steps array");
    assert!(
        steps.iter().any(|step| {
            step.get("name").and_then(Value::as_str) == Some("scenario_doctor")
                && step.get("status").and_then(Value::as_str) == Some("fail")
        }),
        "scenario_doctor should be marked as failed"
    );

    assert!(
        out_dir.join("project_check_report.json").exists(),
        "project-check should still emit the aggregate report on failure"
    );
    assert!(
        out_dir.join("scenario_doctor/stderr.log").exists(),
        "failed step stderr log should be preserved"
    );
}
