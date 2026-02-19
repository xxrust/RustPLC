use serde_json::Value;
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

fn write_fixture_plc(path: &Path) {
    let plc = r#"
[topology]
device X0: digital_input
device X1: digital_input
device Y0: digital_output

[constraints]
safety: Y0.on requires X1.on

[tasks]
task cycle:
    step wait_start:
        wait: X0 == true
        timeout: 30ms -> goto fault
    step run:
        action: set Y0 on
    on_complete: goto done

task fault:
    step safe_stop:
        action: set Y0 off

task done:
    step halt:
"#;
    fs::write(path, plc).expect("write fixture plc");
}

#[test]
fn trace_doctor_json_contract_contains_required_fields() {
    let base = temp_dir("rust_plc_trace_doctor_json");
    let plc = base.join("fixture.plc");
    let scenario = base.join("scenario.yaml");
    let trace = base.join("trace.jsonl");
    let diff = base.join("diff_report.json");
    let timing = base.join("timing_report.json");
    write_fixture_plc(&plc);

    fs::write(
        &scenario,
        r#"
tick_ms: 10
duration_ms: 100
inputs: []
"#,
    )
    .expect("write scenario");

    fs::write(
        &trace,
        r#"{"tick":0,"task":0,"from_step":0,"to_step":1,"reason":"action"}
{"tick":3,"task":0,"from_step":1,"to_step":2,"reason":"timeout"}
"#,
    )
    .expect("write trace");

    fs::write(
        &diff,
        r#"{
  "is_match": false,
  "sil_events": 2,
  "board_events": 1,
  "first_mismatch_tick": 3,
  "mismatch_type": "step",
  "mismatch_index": 1,
  "context_window": 2,
  "context": [
    {
      "index": 1,
      "sil": {
        "tick": 3,
        "task": 0,
        "from_step": 1,
        "to_step": 2,
        "reason": "timeout"
      },
      "board": null
    }
  ]
}
"#,
    )
    .expect("write diff report");

    fs::write(
        &timing,
        r#"{
  "schema_version": 1,
  "count": 8,
  "overrun_count": 1,
  "exec_us_min": 500,
  "exec_us_max": 11000,
  "exec_us_p50": 2000,
  "exec_us_p95": 9500,
  "exec_us_p99": 9200,
  "exec_us_mean": 3400.0
}
"#,
    )
    .expect("write timing report");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("trace-doctor")
        .arg(&plc)
        .arg("--scenario")
        .arg(&scenario)
        .arg("--trace")
        .arg(&trace)
        .arg("--diff")
        .arg(&diff)
        .arg("--timing-report")
        .arg(&timing)
        .arg("--evidence-source")
        .arg("no_board")
        .arg("--output")
        .arg("json")
        .output()
        .expect("run trace-doctor");

    assert!(
        output.status.success(),
        "trace-doctor json should pass, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value = serde_json::from_slice(&output.stdout).expect("trace-doctor JSON");
    assert_eq!(
        report.get("schema_version").and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        report.get("command").and_then(Value::as_str),
        Some("trace-doctor")
    );
    assert_eq!(
        report.get("evidence_source").and_then(Value::as_str),
        Some("no_board")
    );
    assert!(
        report.get("anchors").and_then(Value::as_array).is_some(),
        "anchors should exist"
    );

    let candidates = report
        .get("candidates")
        .and_then(Value::as_array)
        .expect("candidates array");
    assert!(
        candidates.iter().all(|candidate| {
            candidate
                .get("issue_code")
                .and_then(Value::as_str)
                .map(|code| code.starts_with("DIAG-"))
                .unwrap_or(false)
        }),
        "all candidates should have DIAG-* issue codes"
    );

    let summary = report.get("summary").expect("summary should exist");
    assert_eq!(
        summary.get("candidate_count").and_then(Value::as_u64),
        Some(candidates.len() as u64)
    );
    assert!(report.get("artifacts").is_some(), "artifacts should exist");
}

#[test]
fn trace_doctor_human_output_prints_top_candidates_and_next_steps() {
    let base = temp_dir("rust_plc_trace_doctor_human");
    let plc = base.join("fixture.plc");
    let scenario = base.join("scenario.yaml");
    let trace = base.join("trace.jsonl");
    write_fixture_plc(&plc);

    fs::write(
        &scenario,
        r#"
tick_ms: 10
duration_ms: 100
inputs: []
"#,
    )
    .expect("write scenario");

    fs::write(
        &trace,
        r#"{"tick":0,"task":0,"from_step":0,"to_step":1,"reason":"action"}
{"tick":3,"task":0,"from_step":1,"to_step":2,"reason":"timeout"}
"#,
    )
    .expect("write trace");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("trace-doctor")
        .arg(&plc)
        .arg("--scenario")
        .arg(&scenario)
        .arg("--trace")
        .arg(&trace)
        .arg("--top")
        .arg("2")
        .arg("--output")
        .arg("human")
        .output()
        .expect("run trace-doctor human");

    assert!(
        output.status.success(),
        "trace-doctor human should pass, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("trace-doctor: PASS"), "stderr: {stderr}");
    assert!(stderr.contains("Top 2 candidate(s):"), "stderr: {stderr}");
    assert!(
        stderr.contains("[DIAG-"),
        "stderr should include issue code"
    );
    assert!(
        stderr.contains("evidence:"),
        "stderr should include evidence summary"
    );
    assert!(
        stderr.contains("next:"),
        "stderr should include next-step suggestion"
    );
}
