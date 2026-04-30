use std::fs;
use std::process::Command;

fn temp_dir(prefix: &str) -> std::path::PathBuf {
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
fn pil_run_replays_runtime_trace_for_valid_fixture() {
    let base = temp_dir("rust_plc_pil_run");
    let plc_path = base.join("fixture.plc");
    let scenario_path = base.join("scenario.yaml");

    let plc = r#"
[topology]
device plc_main: plc {
    purpose: "pil-run fixture controller",
    model_ref: openplc_softplc
}

[constraints]

[tasks]
task main:
    step wait_start:
        wait: X0 == true
        timeout: 20ms -> goto done
    step run:
        action: set Y0 on

task done:
    step halt:
        action: log "done"
"#;
    fs::write(&plc_path, plc).expect("write plc");

    let scenario = r#"
tick_ms: 10
duration_ms: 40
inputs:
  - at_ms: 10
    set:
      digital_inputs:
        0: true
"#;
    fs::write(&scenario_path, scenario).expect("write scenario");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("pil-run")
        .arg(&plc_path)
        .arg("--scenario")
        .arg(&scenario_path)
        .output()
        .expect("run pil-run");

    assert!(
        output.status.success(),
        "pil-run should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("boot ok"), "stdout: {stdout}");
    assert!(stdout.contains("TICK tick=0"), "stdout: {stdout}");
    assert!(stdout.contains("TRACE "), "stdout: {stdout}");
}
