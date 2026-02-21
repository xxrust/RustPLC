use std::fs;
use std::process::Command;

#[test]
fn cli_board_parse_converts_trace_and_timing_lines_to_jsonl_artifacts() {
    let base = std::env::temp_dir().join(format!(
        "rust_plc_board_parse_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");

    let input = base.join("log.txt");
    let out_dir = base.join("out");
    fs::write(
        &input,
        "boot ok\nTRACE tick=0 task=0 from=0 to=1 reason=action ts_ms=0\nTIMING tick=0 ts_start_us=0 ts_end_us=10 exec_us=10 slack_us=990 overrun=false\n",
    )
    .expect("write input");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("board-parse")
        .arg("--in")
        .arg(&input)
        .arg("--out-dir")
        .arg(&out_dir)
        .output()
        .expect("run board-parse");

    assert!(
        output.status.success(),
        "board-parse should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let trace_path = out_dir.join("board_trace.jsonl");
    let timing_path = out_dir.join("tick_timing.jsonl");
    assert!(trace_path.exists(), "board_trace.jsonl should exist");
    assert!(timing_path.exists(), "tick_timing.jsonl should exist");

    let jsonl = fs::read_to_string(&trace_path).expect("read out");
    let first = jsonl.lines().next().expect("at least one line");
    let v: serde_json::Value = serde_json::from_str(first).expect("valid json");
    assert_eq!(v.get("tick").and_then(|v| v.as_u64()), Some(0));
    assert_eq!(v.get("reason").and_then(|v| v.as_str()), Some("action"));

    let timing_jsonl = fs::read_to_string(&timing_path).expect("read timing");
    let timing_first = timing_jsonl.lines().next().expect("timing row");
    let timing: serde_json::Value = serde_json::from_str(timing_first).expect("valid json");
    assert_eq!(timing.get("tick").and_then(|v| v.as_u64()), Some(0));
    assert_eq!(timing.get("exec_us").and_then(|v| v.as_u64()), Some(10));
}
