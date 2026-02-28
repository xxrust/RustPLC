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

fn write_json(path: &Path, value: serde_json::Value) {
    fs::write(
        path,
        serde_json::to_string_pretty(&value).expect("serialize json"),
    )
    .expect("write json file");
}

#[test]
fn extern_perf_gate_passes_for_payload_within_threshold_and_baseline() {
    if !has_python3() {
        eprintln!("python3 not available; skipping extern perf gate script test");
        return;
    }

    let base = temp_dir("rust_plc_extern_perf_gate_pass");
    let benchmark = base.join("benchmark.json");
    let baseline = base.join("baseline.json");
    let thresholds = base.join("thresholds.json");

    write_json(
        &benchmark,
        serde_json::json!({
          "schema_version": 1,
          "samples": 5,
          "warmups": 1,
          "simple_iterations": 1000,
          "complex_iterations": 200,
          "metrics_us_per_call": {
            "simple_add": {"p95_us": 0.8},
            "complex_quadratic_fit": {"p95_us": 2.8}
          }
        }),
    );

    write_json(
        &baseline,
        serde_json::json!({
          "schema_version": 1,
          "metrics_p95_us": {
            "simple_add": 0.7,
            "complex_quadratic_fit": 2.5
          }
        }),
    );

    write_json(
        &thresholds,
        serde_json::json!({
          "schema_version": 1,
          "thresholds_p95_us": {
            "simple_add": 5.0,
            "complex_quadratic_fit": 20.0
          },
          "max_regression_pct_vs_baseline": 35.0
        }),
    );

    let output = Command::new("python3")
        .arg(repo_path("scripts/extern_perf_gate.py"))
        .arg("--benchmark-json")
        .arg(&benchmark)
        .arg("--baseline")
        .arg(&baseline)
        .arg("--thresholds")
        .arg(&thresholds)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run extern_perf_gate.py pass case");

    assert!(
        output.status.success(),
        "extern perf gate should pass, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse gate JSON output");
    assert_eq!(payload.get("status").and_then(|v| v.as_str()), Some("pass"));
}

#[test]
fn extern_perf_gate_fails_when_p95_exceeds_limits() {
    if !has_python3() {
        eprintln!("python3 not available; skipping extern perf gate script test");
        return;
    }

    let base = temp_dir("rust_plc_extern_perf_gate_fail");
    let benchmark = base.join("benchmark.json");
    let baseline = base.join("baseline.json");
    let thresholds = base.join("thresholds.json");

    write_json(
        &benchmark,
        serde_json::json!({
          "schema_version": 1,
          "samples": 5,
          "warmups": 1,
          "simple_iterations": 1000,
          "complex_iterations": 200,
          "metrics_us_per_call": {
            "simple_add": {"p95_us": 7.0},
            "complex_quadratic_fit": {"p95_us": 30.0}
          }
        }),
    );

    write_json(
        &baseline,
        serde_json::json!({
          "schema_version": 1,
          "metrics_p95_us": {
            "simple_add": 1.0,
            "complex_quadratic_fit": 4.0
          }
        }),
    );

    write_json(
        &thresholds,
        serde_json::json!({
          "schema_version": 1,
          "thresholds_p95_us": {
            "simple_add": 5.0,
            "complex_quadratic_fit": 20.0
          },
          "max_regression_pct_vs_baseline": 25.0
        }),
    );

    let output = Command::new("python3")
        .arg(repo_path("scripts/extern_perf_gate.py"))
        .arg("--benchmark-json")
        .arg(&benchmark)
        .arg("--baseline")
        .arg(&baseline)
        .arg("--thresholds")
        .arg(&thresholds)
        .arg("--output")
        .arg("human")
        .output()
        .expect("run extern_perf_gate.py fail case");

    assert!(
        !output.status.success(),
        "extern perf gate should fail when limits are exceeded"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("REGRESSION"),
        "expected regression marker in stdout, got: {stdout}"
    );
}
