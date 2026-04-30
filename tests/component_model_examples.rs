use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "{prefix}_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn component_model_examples_validate_and_simulate() {
    let topology = "examples/component_model/topology.json";
    let normal = "examples/component_model/scenario_normal.json";
    let faults = "examples/component_model/scenario_faults.json";
    let out_dir = temp_dir("rust_plc_component_model_example");
    let normal_trace = out_dir.join("component_trace.jsonl");

    let topo_out = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("component-topology-validate")
        .arg(topology)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run component-topology-validate");
    assert!(
        topo_out.status.success(),
        "topology should validate, stderr: {}",
        String::from_utf8_lossy(&topo_out.stderr)
    );

    let normal_validate = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("component-scenario-validate")
        .arg(normal)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run component-scenario-validate normal");
    assert!(
        normal_validate.status.success(),
        "normal scenario should validate, stderr: {}",
        String::from_utf8_lossy(&normal_validate.stderr)
    );

    let faults_validate = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("component-scenario-validate")
        .arg(faults)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run component-scenario-validate faults");
    assert!(
        faults_validate.status.success(),
        "fault scenario should validate, stderr: {}",
        String::from_utf8_lossy(&faults_validate.stderr)
    );

    let sim_normal = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("component-sim")
        .arg(topology)
        .arg("--scenario")
        .arg(normal)
        .arg("--out")
        .arg(&normal_trace)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run component-sim normal");
    assert!(
        sim_normal.status.success(),
        "component-sim normal should pass, stderr: {}",
        String::from_utf8_lossy(&sim_normal.stderr)
    );
    assert!(normal_trace.exists(), "normal trace should exist");

    let trace_text = fs::read_to_string(&normal_trace).expect("read normal trace");
    let rows: Vec<Value> = trace_text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("trace row json"))
        .collect();
    assert!(
        rows.iter().any(|row| {
            row.pointer("/components/cyl_a/state")
                .and_then(Value::as_str)
                .is_some_and(|state| state == "extended")
        }),
        "normal trace should show the cylinder reaching extended"
    );
    assert!(
        rows.iter().any(|row| {
            row.pointer("/components/cyl_a/state")
                .and_then(Value::as_str)
                .is_some_and(|state| state == "moving_retract")
        }),
        "normal trace should show the cylinder retracting after x_front turns on"
    );
    assert!(
        rows.iter().take(4).all(|row| {
            row.pointer("/components/x_front/state")
                .and_then(Value::as_str)
                .is_some_and(|state| state == "off")
        }),
        "x_front should not start as always-on in the default scenario"
    );
    assert!(
        rows.iter().any(|row| {
            row.pointer("/components/x_front/state")
                .and_then(Value::as_str)
                .is_some_and(|state| state == "on")
        }),
        "x_front should still become active later in the scenario"
    );

    let sim_faults = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("component-sim")
        .arg(topology)
        .arg("--scenario")
        .arg(faults)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run component-sim faults");
    assert!(
        sim_faults.status.success(),
        "component-sim faults should pass, stderr: {}",
        String::from_utf8_lossy(&sim_faults.stderr)
    );
}
