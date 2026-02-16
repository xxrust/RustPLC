use std::fs;
use std::process::Command;

#[test]
fn cli_trace_parse_converts_trace_lines_to_jsonl() {
    let base = std::env::temp_dir().join(format!(
        "rust_plc_trace_parse_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");

    let input = base.join("log.txt");
    let out = base.join("trace.jsonl");
    fs::write(
        &input,
        "boot ok\nTRACE tick=0 task=0 from=0 to=1 reason=action ts_ms=0\n",
    )
    .expect("write input");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("trace-parse")
        .arg("--in")
        .arg(&input)
        .arg("--out")
        .arg(&out)
        .output()
        .expect("run trace-parse");

    assert!(
        output.status.success(),
        "trace-parse should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let jsonl = fs::read_to_string(&out).expect("read out");
    let first = jsonl.lines().next().expect("at least one line");
    let v: serde_json::Value = serde_json::from_str(first).expect("valid json");
    assert_eq!(v.get("tick").and_then(|v| v.as_u64()), Some(0));
    assert_eq!(v.get("reason").and_then(|v| v.as_str()), Some("action"));
}
