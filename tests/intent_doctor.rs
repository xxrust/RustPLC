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

#[test]
fn intent_doctor_help_mentions_contract_and_trace_inputs() {
    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("help")
        .arg("intent-doctor")
        .output()
        .expect("run help intent-doctor");

    assert!(
        output.status.success(),
        "help intent-doctor should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("intent-doctor"));
    assert!(stderr.contains("--trace <trace.jsonl>"));
    assert!(stderr.contains("--intent-contract <file>"));
}

#[test]
fn intent_doctor_json_reports_candidates_on_real_trace() {
    let base = temp_dir("rust_plc_intent_doctor");
    let project_dir = base.join("demo_project");
    let trace_path = base.join("trace.jsonl");

    let new_output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("new")
        .arg(&project_dir)
        .output()
        .expect("create project");
    assert!(
        new_output.status.success(),
        "rust_plc new should succeed, stderr: {}",
        String::from_utf8_lossy(&new_output.stderr)
    );

    let plc_path = project_dir.join("plc/main.plc");
    let scenario_path = project_dir.join("scenarios/nominal/normal.yaml");
    let sim_output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("sim-plc")
        .arg(&plc_path)
        .arg("--scenario")
        .arg(&scenario_path)
        .arg("--out")
        .arg(&trace_path)
        .output()
        .expect("run sim-plc");
    assert!(
        sim_output.status.success(),
        "sim-plc should succeed, stderr: {}",
        String::from_utf8_lossy(&sim_output.stderr)
    );

    let doctor_output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("intent-doctor")
        .arg(&plc_path)
        .arg("--trace")
        .arg(&trace_path)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run intent-doctor");
    assert!(
        doctor_output.status.success(),
        "intent-doctor should succeed, stderr: {}",
        String::from_utf8_lossy(&doctor_output.stderr)
    );

    let report: Value =
        serde_json::from_slice(&doctor_output.stdout).expect("intent-doctor json report");
    assert_eq!(
        report.get("command").and_then(Value::as_str),
        Some("intent-doctor")
    );
    assert!(report.get("analysis").is_some(), "analysis should exist");
    assert!(
        report
            .get("summary")
            .and_then(|summary| summary.get("candidate_count"))
            .and_then(Value::as_u64)
            .is_some_and(|count| count > 0),
        "intent-doctor should surface at least one candidate"
    );
    assert!(
        report
            .get("analysis")
            .and_then(|analysis| analysis.get("transition_summaries"))
            .and_then(Value::as_array)
            .is_some_and(|summaries| !summaries.is_empty()),
        "transition summaries should exist"
    );
}
