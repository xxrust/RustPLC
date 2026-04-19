use std::fs;
use std::process::Command;

#[test]
fn cli_build_renode_stm32_emits_expected_artifacts() {
    let target_available = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|stdout| stdout.lines().any(|line| line.trim() == "thumbv7em-none-eabi"))
        .unwrap_or(false);
    if !target_available {
        eprintln!("skip: host thumbv7em-none-eabi target not installed");
        return;
    }

    let base = std::env::temp_dir().join(format!(
        "rust_plc_build_renode_stm32_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");

    let out_dir = base.join("out");
    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("build-renode-stm32")
        .arg("examples/pil_baselines/case_timeout/case.plc")
        .arg("--scenario")
        .arg("examples/pil_baselines/case_timeout/scenarios/base.yaml")
        .arg("--out")
        .arg(&out_dir)
        .output()
        .expect("run build-renode-stm32");

    assert!(
        output.status.success(),
        "build-renode-stm32 should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(out_dir.join("generated_program.rs").exists());
    assert!(out_dir.join("scenario.resolved.yaml").exists());
    assert!(out_dir.join("build_meta.json").exists());
    assert!(out_dir.join("board-renode-stm32.elf").exists());
}
