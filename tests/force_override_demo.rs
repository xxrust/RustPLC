use std::path::Path;
use std::process::Command;

fn repo_path(p: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(p)
}

#[test]
fn sim_plc_force_override_demo_runs() {
    let plc = repo_path("examples/force_override_demo.plc");
    let scenario = repo_path("scenarios/force_override_demo/force.yaml");
    assert!(plc.exists(), "expected PLC example to exist");
    assert!(scenario.exists(), "expected scenario to exist");

    let out = std::env::temp_dir().join(format!(
        "rust_plc_force_override_demo_{}_{}.jsonl",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("sim-plc")
        .arg(&plc)
        .arg("--scenario")
        .arg(&scenario)
        .arg("--out")
        .arg(&out)
        .output()
        .expect("run sim-plc");

    assert!(
        output.status.success(),
        "sim-plc should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(out.exists(), "trace output should exist");
}
