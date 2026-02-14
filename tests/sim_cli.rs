use std::fs;
use std::process::Command;

#[test]
fn cli_sim_writes_a_non_empty_jsonl_trace_file() {
    let base = std::env::temp_dir().join(format!(
        "rust_plc_sim_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");

    let scenario_path = base.join("scenario.yaml");
    let out_path = base.join("trace.jsonl");

    // Tick 0: DI0 false; tick 1: DI0 true -> program unblocks and emits trace events.
    let yaml = r#"
tick_ms: 10
duration_ms: 30
inputs:
  - at_ms: 10
    set:
      digital_inputs:
        0: true
"#;
    fs::write(&scenario_path, yaml).expect("write scenario yaml");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("sim")
        .arg(&scenario_path)
        .arg("--out")
        .arg(&out_path)
        .output()
        .expect("should run rust_plc sim");

    assert!(
        output.status.success(),
        "sim should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let trace = fs::read_to_string(&out_path).expect("read trace");
    assert!(!trace.trim().is_empty(), "trace file should be non-empty");

    let first_line = trace.lines().next().expect("has at least one line");
    let v: serde_json::Value =
        serde_json::from_str(first_line).expect("trace line should be valid JSON");
    assert!(v.get("tick").is_some());
    assert!(v.get("task").is_some());
    assert!(v.get("from_step").is_some());
    assert!(v.get("to_step").is_some());
    assert!(v.get("reason").is_some());
}

