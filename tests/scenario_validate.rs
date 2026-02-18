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
fn scenario_validate_passes_for_examples() {
    let plc = repo_path("examples/assembly_station.plc");
    let scenario = repo_path("scenarios/normal.yaml");
    assert!(plc.exists(), "expected PLC example to exist");
    assert!(scenario.exists(), "expected scenario to exist");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("scenario-validate")
        .arg(&plc)
        .arg("--scenario")
        .arg(&scenario)
        .output()
        .expect("run scenario-validate");

    assert!(
        output.status.success(),
        "scenario-validate should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("scenario-validate: PASS"),
        "expected PASS marker"
    );
}

#[test]
fn scenario_validate_fails_for_unknown_inputs() {
    let base = temp_dir("rust_plc_scenario_validate_unknown");
    let scenario_path = base.join("bad.yaml");
    fs::write(
        &scenario_path,
        r#"
tick_ms: 10
duration_ms: 100
inputs:
  - at_ms: 0
    set:
      digital_inputs:
        999: true
"#,
    )
    .expect("write scenario yaml");

    let plc = repo_path("examples/assembly_station.plc");
    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("scenario-validate")
        .arg(&plc)
        .arg("--scenario")
        .arg(&scenario_path)
        .output()
        .expect("run scenario-validate");

    assert!(
        !output.status.success(),
        "scenario-validate should fail for unknown inputs"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("DI999 does not exist"),
        "expected unknown input error; stderr was:\n{stderr}"
    );
}

#[test]
fn scenario_validate_fails_for_unknown_force_outputs() {
    let base = temp_dir("rust_plc_scenario_validate_unknown_force");
    let scenario_path = base.join("bad_force.yaml");
    fs::write(
        &scenario_path,
        r#"
tick_ms: 10
duration_ms: 100
forces:
  - at_ms: 0
    set:
      digital_outputs:
        999: true
"#,
    )
    .expect("write scenario yaml");

    let plc = repo_path("examples/assembly_station.plc");
    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("scenario-validate")
        .arg(&plc)
        .arg("--scenario")
        .arg(&scenario_path)
        .output()
        .expect("run scenario-validate");

    assert!(
        !output.status.success(),
        "scenario-validate should fail for unknown force outputs"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("DO999 does not exist"),
        "expected unknown output error; stderr was:\n{stderr}"
    );
}
