use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn golden_trace_pair_matches_with_gate() {
    let base = std::env::temp_dir().join(format!(
        "rust_plc_trace_golden_match_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");
    let out = base.join("report.json");

    let sil = Path::new("examples/trace_golden/sil_trace.jsonl");
    let board = Path::new("examples/trace_golden/board_trace_match.jsonl");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("trace-diff")
        .arg("--sil")
        .arg(sil)
        .arg("--board")
        .arg(board)
        .arg("--out")
        .arg(&out)
        .arg("--fail-on-mismatch")
        .output()
        .expect("run trace-diff");

    assert!(
        output.status.success(),
        "golden match should pass gate, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn golden_trace_pair_mismatch_fails_gate() {
    let base = std::env::temp_dir().join(format!(
        "rust_plc_trace_golden_mismatch_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");
    let out = base.join("report.json");

    let sil = Path::new("examples/trace_golden/sil_trace.jsonl");
    let board = Path::new("examples/trace_golden/board_trace_mismatch.jsonl");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("trace-diff")
        .arg("--sil")
        .arg(sil)
        .arg("--board")
        .arg(board)
        .arg("--out")
        .arg(&out)
        .arg("--fail-on-mismatch")
        .output()
        .expect("run trace-diff");

    assert!(
        !output.status.success(),
        "golden mismatch should fail gate"
    );
    assert!(out.exists(), "report should be materialized for mismatch diagnostics");
}

