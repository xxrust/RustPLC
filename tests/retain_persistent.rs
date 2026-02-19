use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

fn run_sim_plc_with_retain(
    scenario_path: &Path,
    trace_out: &Path,
    retain_config: &Path,
    retain_state: &Path,
) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("sim-plc")
        .arg(repo_path("examples/force_override_demo.plc"))
        .arg("--scenario")
        .arg(scenario_path)
        .arg("--out")
        .arg(trace_out)
        .arg("--retain-config")
        .arg(retain_config)
        .arg("--retain-state")
        .arg(retain_state)
        .output()
        .expect("run sim-plc")
}

#[test]
fn retain_state_survives_restart_for_configured_channels() {
    let base = temp_dir("rust_plc_retain_restart");
    let scenario_first = base.join("first.yaml");
    let scenario_second = base.join("second.yaml");
    let trace_first = base.join("trace_first.jsonl");
    let trace_second = base.join("trace_second.jsonl");
    let retain_config = base.join("retain.toml");
    let retain_state = base.join("retain_state.json");

    fs::write(
        &retain_config,
        "schema_version = 1\n[digital_inputs]\ndi0 = false\n",
    )
    .expect("write retain config");
    fs::write(
        &scenario_first,
        "tick_ms: 10\nduration_ms: 80\ninputs:\n  - at_ms: 0\n    set:\n      digital_inputs:\n        0: true\nforces: []\n",
    )
    .expect("write first scenario");
    fs::write(
        &scenario_second,
        "tick_ms: 10\nduration_ms: 80\ninputs: []\nforces: []\n",
    )
    .expect("write second scenario");

    let first =
        run_sim_plc_with_retain(&scenario_first, &trace_first, &retain_config, &retain_state);
    assert!(
        first.status.success(),
        "first run should succeed, stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_trace = fs::read_to_string(&trace_first).expect("read first trace");
    assert!(
        !first_trace.trim().is_empty(),
        "first run should transition due DI0=true at boot"
    );

    let second = run_sim_plc_with_retain(
        &scenario_second,
        &trace_second,
        &retain_config,
        &retain_state,
    );
    assert!(
        second.status.success(),
        "second run should succeed, stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    let second_trace = fs::read_to_string(&trace_second).expect("read second trace");
    assert!(
        !second_trace.trim().is_empty(),
        "second run should restore retained DI0=true and transition"
    );

    let state: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&retain_state).expect("read retain state"))
            .expect("retain state json");
    assert_eq!(state["schema_version"], 1);
    assert!(
        state
            .get("checksum_sha256")
            .and_then(|v| v.as_str())
            .is_some(),
        "retain state should include checksum"
    );
    assert_eq!(state["payload"]["digital_inputs"]["0"], true);
}

#[test]
fn retain_state_corruption_falls_back_to_config_defaults() {
    let base = temp_dir("rust_plc_retain_corrupt");
    let scenario_first = base.join("first.yaml");
    let scenario_second = base.join("second.yaml");
    let trace_second = base.join("trace_second.jsonl");
    let retain_config = base.join("retain.toml");
    let retain_state = base.join("retain_state.json");

    fs::write(
        &retain_config,
        "schema_version = 1\n[digital_inputs]\ndi0 = false\n",
    )
    .expect("write retain config");
    fs::write(
        &scenario_first,
        "tick_ms: 10\nduration_ms: 80\ninputs:\n  - at_ms: 0\n    set:\n      digital_inputs:\n        0: true\nforces: []\n",
    )
    .expect("write first scenario");
    fs::write(
        &scenario_second,
        "tick_ms: 10\nduration_ms: 80\ninputs: []\nforces: []\n",
    )
    .expect("write second scenario");

    let first = run_sim_plc_with_retain(
        &scenario_first,
        &base.join("trace_first.jsonl"),
        &retain_config,
        &retain_state,
    );
    assert!(
        first.status.success(),
        "first run should succeed, stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    let mut corrupted: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&retain_state).expect("read retain state"))
            .expect("retain state json");
    corrupted["checksum_sha256"] = serde_json::Value::String("deadbeef".to_string());
    let mut text = serde_json::to_string_pretty(&corrupted).expect("serialize corrupted state");
    text.push('\n');
    fs::write(&retain_state, text).expect("write corrupted retain state");

    let second = run_sim_plc_with_retain(
        &scenario_second,
        &trace_second,
        &retain_config,
        &retain_state,
    );
    assert!(
        second.status.success(),
        "second run should still succeed with fallback defaults, stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        String::from_utf8_lossy(&second.stderr).contains("checksum mismatch"),
        "stderr should indicate checksum fallback, got: {}",
        String::from_utf8_lossy(&second.stderr)
    );

    let second_trace = fs::read_to_string(&trace_second).expect("read second trace");
    assert!(
        second_trace.trim().is_empty(),
        "with checksum fallback to default DI0=false, no transition should occur"
    );
}
