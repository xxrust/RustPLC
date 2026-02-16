use std::fs;
use std::process::Command;

#[test]
fn cli_trace_diff_reports_match() {
    let base = std::env::temp_dir().join(format!(
        "rust_plc_trace_diff_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");

    let sil = base.join("sil.jsonl");
    let board = base.join("board.jsonl");
    let out = base.join("report.json");

    let jsonl = "{\"tick\":0,\"task\":0,\"from_step\":0,\"to_step\":1,\"reason\":\"action\"}\n";
    fs::write(&sil, jsonl).expect("write sil");
    fs::write(
        &board,
        "{\"tick\":0,\"task\":0,\"from_step\":0,\"to_step\":1,\"reason\":\"action\",\"timestamp_ms\":0}\n",
    )
    .expect("write board");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("trace-diff")
        .arg("--sil")
        .arg(&sil)
        .arg("--board")
        .arg(&board)
        .arg("--out")
        .arg(&out)
        .output()
        .expect("run trace-diff");

    assert!(
        output.status.success(),
        "trace-diff should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let rep: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&out).expect("read report"))
            .expect("valid json");
    assert_eq!(rep.get("is_match").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(
        rep.get("first_mismatch_tick").and_then(|v| v.as_u64()),
        None
    );
}

#[test]
fn cli_trace_diff_reports_first_mismatch() {
    let base = std::env::temp_dir().join(format!(
        "rust_plc_trace_diff_mismatch_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");

    let sil = base.join("sil.jsonl");
    let board = base.join("board.jsonl");
    let out = base.join("report.json");

    fs::write(
        &sil,
        concat!(
            "{\"tick\":0,\"task\":0,\"from_step\":0,\"to_step\":1,\"reason\":\"action\"}\n",
            "{\"tick\":1,\"task\":0,\"from_step\":1,\"to_step\":2,\"reason\":\"goto\"}\n"
        ),
    )
    .expect("write sil");
    fs::write(
        &board,
        concat!(
            "{\"tick\":0,\"task\":0,\"from_step\":0,\"to_step\":1,\"reason\":\"action\",\"timestamp_ms\":0}\n",
            "{\"tick\":1,\"task\":0,\"from_step\":1,\"to_step\":2,\"reason\":\"timeout\",\"timestamp_ms\":1}\n"
        ),
    )
    .expect("write board");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("trace-diff")
        .arg("--sil")
        .arg(&sil)
        .arg("--board")
        .arg(&board)
        .arg("--out")
        .arg(&out)
        .arg("--context")
        .arg("1")
        .output()
        .expect("run trace-diff");

    assert!(
        output.status.success(),
        "trace-diff should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let rep: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&out).expect("read report"))
            .expect("valid json");
    assert_eq!(rep.get("is_match").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(
        rep.get("first_mismatch_tick").and_then(|v| v.as_u64()),
        Some(1)
    );
    assert_eq!(
        rep.get("mismatch_type").and_then(|v| v.as_str()),
        Some("reason")
    );
    assert_eq!(
        rep.get("mismatch_index").and_then(|v| v.as_u64()),
        Some(1)
    );
    assert_eq!(
        rep.get("context").and_then(|v| v.as_array()).map(|a| a.len()),
        Some(2)
    );
}

#[test]
fn cli_trace_diff_can_fail_on_mismatch_for_regression_gate() {
    let base = std::env::temp_dir().join(format!(
        "rust_plc_trace_diff_gate_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");

    let sil = base.join("sil.jsonl");
    let board = base.join("board.jsonl");
    let out = base.join("report.json");

    fs::write(
        &sil,
        "{\"tick\":0,\"task\":0,\"from_step\":0,\"to_step\":1,\"reason\":\"action\"}\n",
    )
    .expect("write sil");
    fs::write(
        &board,
        "{\"tick\":0,\"task\":0,\"from_step\":0,\"to_step\":2,\"reason\":\"action\",\"timestamp_ms\":0}\n",
    )
    .expect("write board");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("trace-diff")
        .arg("--sil")
        .arg(&sil)
        .arg("--board")
        .arg(&board)
        .arg("--out")
        .arg(&out)
        .arg("--fail-on-mismatch")
        .output()
        .expect("run trace-diff");

    assert!(
        !output.status.success(),
        "trace-diff should fail when --fail-on-mismatch is set and traces differ"
    );
    assert!(out.exists(), "report should still be written on mismatch");
}
