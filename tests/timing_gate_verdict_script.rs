use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn has_python3() -> bool {
    Command::new("python3")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn temp_dir(prefix: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_nanos();
    path.push(format!("{}_{}_{}", prefix, std::process::id(), ts));
    fs::create_dir_all(&path).expect("create temp dir");
    path
}

fn repo_path(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn write_timing_report(path: &Path, p99: u64, overrun_count: u64) {
    let report = serde_json::json!({
        "count": 200,
        "exec_us_p50": 800,
        "exec_us_p95": 1200,
        "exec_us_p99": p99,
        "exec_us_max": 2200,
        "exec_us_mean": 930,
        "overrun_count": overrun_count,
    });
    fs::write(
        path,
        serde_json::to_string_pretty(&report).expect("serialize timing report"),
    )
    .expect("write timing report");
}

#[test]
fn timing_gate_verdict_passes_when_thresholds_are_satisfied() {
    if !has_python3() {
        eprintln!("python3 not available; skipping timing gate verdict script test");
        return;
    }

    let base = temp_dir("rust_plc_timing_gate_verdict_pass");
    let timing_report = base.join("timing_report.json");
    let verdict = base.join("timing_gate_verdict.json");
    write_timing_report(&timing_report, 1800, 0);

    let output = Command::new("python3")
        .arg(repo_path("scripts/timing_gate_verdict.py"))
        .arg("--timing-report")
        .arg(&timing_report)
        .arg("--out")
        .arg(&verdict)
        .arg("--max-p99-exec-us")
        .arg("2000")
        .arg("--max-overrun-count")
        .arg("0")
        .output()
        .expect("run timing_gate_verdict.py pass case");

    assert!(
        output.status.success(),
        "timing gate verdict should pass, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let verdict_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&verdict).expect("read timing gate verdict"))
            .expect("parse timing gate verdict json");

    assert_eq!(
        verdict_json.get("status").and_then(|v| v.as_str()),
        Some("pass")
    );
    assert_eq!(
        verdict_json.get("status_code").and_then(|v| v.as_i64()),
        Some(0)
    );
    assert_eq!(
        verdict_json
            .get("violations")
            .and_then(|v| v.as_array())
            .map(|rows| rows.len()),
        Some(0)
    );
}

#[test]
fn timing_gate_verdict_fails_when_thresholds_are_exceeded() {
    if !has_python3() {
        eprintln!("python3 not available; skipping timing gate verdict script test");
        return;
    }

    let base = temp_dir("rust_plc_timing_gate_verdict_fail");
    let timing_report = base.join("timing_report.json");
    let verdict = base.join("timing_gate_verdict.json");
    write_timing_report(&timing_report, 1800, 0);

    let output = Command::new("python3")
        .arg(repo_path("scripts/timing_gate_verdict.py"))
        .arg("--timing-report")
        .arg(&timing_report)
        .arg("--out")
        .arg(&verdict)
        .arg("--max-p99-exec-us")
        .arg("1000")
        .arg("--max-overrun-count")
        .arg("0")
        .output()
        .expect("run timing_gate_verdict.py fail case");

    assert!(
        !output.status.success(),
        "timing gate verdict should fail when thresholds are exceeded"
    );

    let verdict_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&verdict).expect("read timing gate verdict"))
            .expect("parse timing gate verdict json");

    assert_eq!(
        verdict_json.get("status").and_then(|v| v.as_str()),
        Some("fail")
    );
    assert_eq!(
        verdict_json.get("message").and_then(|v| v.as_str()),
        Some("realtime threshold exceeded")
    );

    let violations = verdict_json
        .get("violations")
        .and_then(|v| v.as_array())
        .expect("violations should be array");
    assert!(
        violations
            .iter()
            .any(|row| { row.get("metric").and_then(|v| v.as_str()) == Some("exec_us_p99") }),
        "violations should include exec_us_p99 breach"
    );
}
