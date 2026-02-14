use std::fs;
use std::process::Command;

#[test]
fn cli_sim_writes_trace_and_waveforms() {
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
    let vcd_out_path = base.join("wave.vcd");
    let analog_out_path = base.join("analog.csv");

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
        .arg("--vcd-out")
        .arg(&vcd_out_path)
        .arg("--analog-out")
        .arg(&analog_out_path)
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

    let vcd = fs::read_to_string(&vcd_out_path).expect("read vcd");
    assert!(vcd.contains("di0"));
    assert!(vcd.contains("do0"));
    assert!(
        vcd.contains("1\""),
        "expected at least one edge on do0 in VCD, got:\n{vcd}"
    );

    let analog_csv = fs::read_to_string(&analog_out_path).expect("read analog csv");
    assert!(
        analog_csv.starts_with("time_ms,tick,ao_id,value"),
        "analog csv should at least have a header, got:\n{analog_csv}"
    );
}
