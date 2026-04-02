use std::fs;
use std::path::PathBuf;
use std::process::Command;

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

fn write_bundle_fixture(base: &PathBuf, topology: &str, constraints: &str, tasks: &str) -> PathBuf {
    let fragments = base.join("fragments");
    fs::create_dir_all(&fragments).expect("create fragments");
    fs::write(fragments.join("topology.plcfrag"), topology).expect("write topology fragment");
    fs::write(fragments.join("constraints.plcfrag"), constraints)
        .expect("write constraints fragment");
    fs::write(fragments.join("tasks.plcfrag"), tasks).expect("write tasks fragment");

    let bundle_path = base.join("main.bundle.toml");
    fs::write(
        &bundle_path,
        "schema_version = 1\n[topology]\nfragments = [\"fragments/topology.plcfrag\"]\n[constraints]\nfragments = [\"fragments/constraints.plcfrag\"]\n[tasks]\nfragments = [\"fragments/tasks.plcfrag\"]\n",
    )
    .expect("write bundle");
    bundle_path
}

#[test]
fn default_compile_accepts_bundle_input() {
    let base = temp_dir("rust_plc_bundle_compile");
    let bundle_path = write_bundle_fixture(
        &base,
        "device plc_main: plc { purpose: \"bundle compile controller\", model_ref: openplc_softplc }\n",
        "",
        "task main:\n    step idle:\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg(&bundle_path)
        .arg("--no-print-ir")
        .output()
        .expect("run rust_plc");

    assert!(
        output.status.success(),
        "bundle compile should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn scenario_validate_accepts_bundle_input() {
    let base = temp_dir("rust_plc_bundle_scenario_validate");
    let bundle_path = write_bundle_fixture(
        &base,
        "device plc_main: plc { purpose: \"bundle scenario controller\", model_ref: openplc_softplc }\n",
        "",
        "task main:\n    step wait_start:\n        wait: X0 == true\n        timeout: 100ms -> goto fault\n\n    step run:\n        action: set Y0 on\n        delay: 20ms\n\n    step stop:\n        action: set Y0 off\n\n    on_complete: goto done\n\ntask fault:\n    step safe_stop:\n        action: set Y0 off\n    on_complete: goto done\n\ntask done:\n    step halt:\n",
    );
    let scenario_path = base.join("normal.yaml");
    fs::write(
        &scenario_path,
        "tick_ms: 10\nduration_ms: 300\ninputs:\n  - at_ms: 0\n    set:\n      digital_inputs:\n        0: true\n  - at_ms: 50\n    set:\n      digital_inputs:\n        0: false\nforces: []\n",
    )
    .expect("write scenario");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("scenario-validate")
        .arg(&bundle_path)
        .arg("--scenario")
        .arg(&scenario_path)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run scenario-validate");

    assert!(
        output.status.success(),
        "bundle scenario-validate should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn bundle_errors_point_back_to_fragment_file() {
    let base = temp_dir("rust_plc_bundle_error_location");
    let bundle_path = write_bundle_fixture(
        &base,
        "device ghost_input: digital_input { purpose: \"should be rejected\" }\n",
        "",
        "task main:\n    step idle:\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg(&bundle_path)
        .output()
        .expect("run rust_plc");

    assert!(
        !output.status.success(),
        "bundle compile should fail for virtual signal device"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("topology.plcfrag"),
        "stderr should point to the fragment file, got: {stderr}"
    );
}
