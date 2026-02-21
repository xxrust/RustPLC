use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_path(p: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(p)
}

fn temp_dir(prefix: &str) -> PathBuf {
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

fn read_trace(path: &Path) -> Vec<Value> {
    let body = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read trace {}: {err}", path.display()));
    body.lines()
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|err| panic!("invalid JSON trace line `{line}`: {err}"))
        })
        .collect()
}

#[test]
fn rp2040_motion_minimal_scenarios_cover_nominal_and_fault_paths() {
    let plc = repo_path("examples/rp2040_motion_minimal.plc");
    assert!(plc.exists(), "expected PLC example to exist");

    let scenario_dir = repo_path("scenarios/rp2040_motion_minimal");
    let cases = [
        ("normal.yaml", false),
        ("count_stuck.yaml", true),
        ("wrong_direction.yaml", true),
    ];

    let base = temp_dir("rust_plc_rp2040_motion_minimal");
    let mut timeout_case_count = 0usize;

    for (name, expect_timeout) in cases {
        let scenario = scenario_dir.join(name);
        assert!(scenario.exists(), "expected scenario to exist: {name}");

        let validate = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
            .arg("scenario-validate")
            .arg(&plc)
            .arg("--scenario")
            .arg(&scenario)
            .output()
            .expect("run scenario-validate");
        assert!(
            validate.status.success(),
            "scenario-validate should succeed for {name}, stderr: {}",
            String::from_utf8_lossy(&validate.stderr)
        );

        let trace_out = base.join(format!("{name}.trace.jsonl"));
        let sim = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
            .arg("sim-plc")
            .arg(&plc)
            .arg("--scenario")
            .arg(&scenario)
            .arg("--out")
            .arg(&trace_out)
            .output()
            .expect("run sim-plc");
        assert!(
            sim.status.success(),
            "sim-plc should succeed for {name}, stderr: {}",
            String::from_utf8_lossy(&sim.stderr)
        );

        let trace = read_trace(&trace_out);
        assert!(!trace.is_empty(), "trace should be non-empty for {name}");

        let has_timeout = trace
            .iter()
            .any(|event| event.get("reason").and_then(Value::as_str) == Some("timeout"));
        assert_eq!(
            has_timeout, expect_timeout,
            "timeout expectation mismatch for {name}"
        );

        if has_timeout {
            timeout_case_count += 1;
        }
    }

    assert_eq!(
        timeout_case_count, 2,
        "expected two fault scenarios to hit timeout path"
    );
}
