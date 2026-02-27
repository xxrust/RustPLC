use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn has_python3() -> bool {
    Command::new("python3")
        .arg("--version")
        .status()
        .ok()
        .is_some_and(|status| status.success())
}

fn repo_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be monotonic")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "rust_plc_{prefix}_{}_{}",
        std::process::id(),
        nanos
    ));
    fs::create_dir_all(&dir).expect("should create temp dir");
    dir
}

#[test]
fn openplc_phase2_gate_passes_for_two_core_scenarios() {
    if !has_python3() {
        eprintln!("python3 not available; skipping openplc phase-2 gate test");
        return;
    }

    let out_dir = unique_temp_dir("openplc_phase2_gate");
    let fixture_dir = repo_path("examples/openplc_trace_phase2");
    let script = repo_path("scripts/openplc_trace_phase2_gate.sh");

    let output = Command::new("bash")
        .arg(script)
        .arg(&fixture_dir)
        .arg(&out_dir)
        .output()
        .expect("should run openplc phase-2 gate script");

    assert!(
        output.status.success(),
        "openplc phase-2 gate failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    for report_name in [
        "two_cylinder.trace_compare.report.json",
        "assembly_station.trace_compare.report.json",
    ] {
        let report_path = out_dir.join(report_name);
        assert!(
            report_path.exists(),
            "expected report to exist: {}",
            report_path.display()
        );

        let report: Value = serde_json::from_str(
            &fs::read_to_string(&report_path).expect("should read trace compare report"),
        )
        .expect("report must be valid JSON");

        assert_eq!(
            report.get("passed").and_then(Value::as_bool),
            Some(true),
            "report should pass: {}",
            report_path.display()
        );
        assert!(
            report
                .get("pass_rate")
                .and_then(Value::as_f64)
                .is_some_and(|v| v >= 0.95),
            "pass_rate should be >= 0.95 in {}",
            report_path.display()
        );
        assert_eq!(
            report.get("tick_tolerance").and_then(Value::as_i64),
            Some(1),
            "tick_tolerance should be 1 in {}",
            report_path.display()
        );
    }
}
