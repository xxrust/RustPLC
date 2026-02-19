use std::process::Command;

#[test]
fn component_model_examples_validate_and_simulate() {
    let topology = "examples/component_model/topology.json";
    let normal = "examples/component_model/scenario_normal.json";
    let faults = "examples/component_model/scenario_faults.json";

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
        .arg("--output")
        .arg("json")
        .output()
        .expect("run component-sim normal");
    assert!(
        sim_normal.status.success(),
        "component-sim normal should pass, stderr: {}",
        String::from_utf8_lossy(&sim_normal.stderr)
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
