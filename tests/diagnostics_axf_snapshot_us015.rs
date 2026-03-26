use serde_json::{Value, json};
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
    let plc = r#"[topology]
device plc_main: plc {
    purpose: "AXF snapshot fixture",
    model_ref: openplc_softplc
}

[constraints]

[tasks]
task cycle:
    step wait_start:
        wait: X0 == true
        timeout: 20ms -> goto fault
    step run:
        action: set Y0 on

task fault:
    step safe_stop:
        action: set Y0 off
"#;
    fs::write(path, plc).expect("write fixture plc");
}

#[test]
fn trace_doctor_json_candidate_contract_snapshot_includes_axf_and_source_location() {
    let base = temp_dir("rust_plc_diag_axf_snapshot");
    let plc = base.join("fixture.plc");
    let scenario = base.join("scenario.yaml");
    let trace = base.join("trace.jsonl");
    write_fixture_plc(&plc);

    fs::write(
        &scenario,
        r#"
tick_ms: 10
duration_ms: 50
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
        .arg("--evidence-source")
        .arg("no_board")
        .arg("--output")
        .arg("json")
        .output()
        .expect("run trace-doctor");

    assert!(
        output.status.success(),
        "trace-doctor should pass, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value = serde_json::from_slice(&output.stdout).expect("trace-doctor JSON");
    let first = report
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|candidates| candidates.first())
        .cloned()
        .expect("at least one diagnosis candidate");

    let mut normalized = first;
    let source_line = normalized
        .get("source_location")
        .and_then(Value::as_object)
        .and_then(|location| location.get("line"))
        .and_then(Value::as_u64)
        .expect("candidate should include source_location.line");
    assert!(source_line > 0, "source_location.line should be positive");
    let confidence = normalized
        .get("confidence")
        .and_then(Value::as_f64)
        .expect("candidate should include confidence");
    assert!(
        (0.0..=1.0).contains(&confidence),
        "confidence should be within [0, 1]"
    );

    if let Some(location) = normalized
        .get_mut("source_location")
        .and_then(Value::as_object_mut)
    {
        location.insert(
            "line".to_string(),
            Value::String("<dynamic-line>".to_string()),
        );
    }
    if let Some(raw_confidence) = normalized.get_mut("confidence") {
        *raw_confidence = Value::String("<dynamic-confidence>".to_string());
    }

    assert_eq!(
        normalized,
        json!({
            "issue_code": "AXF-IN-001",
            "legacy_issue_code": "DIAG-IN-001",
            "category": "expected_input_never_changed",
            "rank": 1,
            "confidence": "<dynamic-confidence>",
            "evidence": [
                "timeout anchor at tick 3",
                "no DI/AI changes scheduled before timeout tick 3",
                "wait-related channels were never toggled before timeout",
                "scenario has no scripted input/fault/force DI/AI mutations",
                "wait predicates reference X0"
            ],
            "source_location": {
                "file": "<input>",
                "line": "<dynamic-line>",
                "column": 1
            },
            "suggested_fix": "Inject or wire expected DI/AI changes earlier, then re-run trace verification.",
            "evidence_source": "no_board"
        })
    );
}
