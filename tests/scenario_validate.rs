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

fn run_scenario_validate(plc: &Path, scenario: &Path) -> std::process::Output {
    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("scenario-validate")
        .arg(plc)
        .arg("--scenario")
        .arg(scenario)
        .output()
        .expect("run scenario-validate");
    output
}

fn run_scenario_init(plc: &Path, out: &Path, preset: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("scenario-init")
        .arg(plc)
        .arg("--out")
        .arg(out)
        .arg("--preset")
        .arg(preset)
        .output()
        .expect("run scenario-init")
}

#[test]
fn scenario_validate_passes_for_examples() {
    let cases = [
        ("examples/assembly_station.plc", "normal"),
        ("examples/two_cylinder.plc", "minimal"),
    ];

    let base = temp_dir("rust_plc_scenario_validate_examples");
    for (plc_rel, preset) in cases {
        let plc = repo_path(plc_rel);
        assert!(plc.exists(), "expected PLC example to exist: {plc_rel}");
        let scenario = base.join(format!(
            "{}_{preset}.yaml",
            plc.file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("scenario")
        ));

        let init = run_scenario_init(&plc, &scenario, preset);
        assert!(
            init.status.success(),
            "scenario-init should succeed for {plc_rel} ({preset}), stderr: {}",
            String::from_utf8_lossy(&init.stderr)
        );
        assert!(
            scenario.exists(),
            "expected generated scenario at {scenario:?}"
        );

        let output = run_scenario_validate(&plc, &scenario);

        assert!(
            output.status.success(),
            "scenario-validate should succeed for {plc_rel} ({preset}), stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("scenario-validate: PASS"),
            "expected PASS marker for {plc_rel} ({preset})"
        );
    }
}

#[test]
fn scenario_validate_rejects_unknown_io_references() {
    let cases = [
        (
            "rust_plc_scenario_validate_unknown_input",
            "bad_input.yaml",
            r#"
tick_ms: 10
duration_ms: 100
inputs:
  - at_ms: 0
    set:
      digital_inputs:
        999: true
"#,
            "DI999 does not exist",
        ),
        (
            "rust_plc_scenario_validate_unknown_output_force",
            "bad_force.yaml",
            r#"
tick_ms: 10
duration_ms: 100
forces:
  - at_ms: 0
    set:
      digital_outputs:
        999: true
"#,
            "DO999 does not exist",
        ),
    ];

    let plc = repo_path("examples/assembly_station.plc");
    for (dir_prefix, file_name, scenario_yaml, expected_error) in cases {
        let base = temp_dir(dir_prefix);
        let scenario_path = base.join(file_name);
        fs::write(&scenario_path, scenario_yaml).expect("write scenario yaml");

        let output = run_scenario_validate(&plc, &scenario_path);
        assert!(
            !output.status.success(),
            "scenario-validate should fail for {file_name}"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(expected_error),
            "expected `{expected_error}` for {file_name}; stderr was:\n{stderr}"
        );
    }
}
