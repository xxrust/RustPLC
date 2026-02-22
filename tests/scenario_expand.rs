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

#[test]
fn scenario_expand_writes_parseable_expanded_yaml() {
    let plc = repo_path("examples/assembly_station.plc");
    let scenario = repo_path("examples/scenarios/pulse_hold.yaml");
    assert!(plc.exists(), "expected PLC example to exist");
    assert!(scenario.exists(), "expected example scenario to exist");

    let base = temp_dir("rust_plc_scenario_expand");
    let out = base.join("expanded.yaml");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("scenario-expand")
        .arg(&plc)
        .arg("--scenario")
        .arg(&scenario)
        .arg("--out")
        .arg(&out)
        .output()
        .expect("run scenario-expand");

    assert!(
        output.status.success(),
        "scenario-expand should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(out.exists(), "expected output file to exist");

    let yaml = fs::read_to_string(&out).expect("read expanded yaml");
    assert!(
        !yaml.contains("\npulse:") && !yaml.contains("\nhold:"),
        "expanded YAML should not contain sugar fields"
    );
    let _scenario = sim::Scenario::from_yaml_str(&yaml).expect("expanded scenario should parse");
}
