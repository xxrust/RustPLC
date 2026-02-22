use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;

fn repo_path(p: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(p)
}

#[test]
fn perf_baseline_fixture_has_expected_scale() {
    let fixture_path = repo_path("examples/topology_perf_500.topology.json");
    let fixture: Value = serde_json::from_str(
        &fs::read_to_string(&fixture_path).expect("read topology perf baseline fixture"),
    )
    .expect("parse topology perf baseline fixture");
    let components = fixture
        .get("components")
        .and_then(Value::as_array)
        .expect("components array");
    let connections = fixture
        .get("connections")
        .and_then(Value::as_array)
        .expect("connections array");

    assert_eq!(components.len(), 500);
    assert_eq!(connections.len(), 2000);
}

#[test]
fn perf_baseline_fixture_passes_validate_and_simulate_commands() {
    let topology_path = repo_path("examples/topology_perf_500.topology.json");
    let scenario_path = repo_path("examples/topology_perf_500.scenario.json");

    let validate = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("component-topology-validate")
        .arg(&topology_path)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run component-topology-validate for perf baseline");
    assert!(
        validate.status.success(),
        "component-topology-validate should pass, stderr: {}",
        String::from_utf8_lossy(&validate.stderr)
    );
    let validate_payload: Value =
        serde_json::from_slice(&validate.stdout).expect("parse validate JSON output");
    assert_eq!(
        validate_payload
            .get("component_count")
            .and_then(Value::as_u64),
        Some(500)
    );
    assert_eq!(
        validate_payload
            .get("connection_count")
            .and_then(Value::as_u64),
        Some(2000)
    );

    let simulate = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("component-sim")
        .arg(&topology_path)
        .arg("--scenario")
        .arg(&scenario_path)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run component-sim for perf baseline");
    assert!(
        simulate.status.success(),
        "component-sim should pass, stderr: {}",
        String::from_utf8_lossy(&simulate.stderr)
    );
    let sim_payload: Value =
        serde_json::from_slice(&simulate.stdout).expect("parse component-sim JSON output");
    assert_eq!(
        sim_payload.get("status").and_then(Value::as_str),
        Some("pass")
    );
}
