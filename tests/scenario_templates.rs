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
fn scenario_init_presets_generate_validateable_scenarios() {
    let plc = repo_path("examples/dual_axis_platform.plc");
    assert!(plc.exists(), "expected PLC example to exist");

    let base = temp_dir("rust_plc_scenario_templates");

    let presets = ["normal", "timeout", "sensor_stuck", "bounce"];
    for preset in presets {
        let out = base.join(format!("dual_axis_platform_{preset}.yaml"));

        let init = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
            .arg("scenario-init")
            .arg(&plc)
            .arg("--out")
            .arg(&out)
            .arg("--preset")
            .arg(preset)
            .output()
            .expect("run scenario-init");

        assert!(
            init.status.success(),
            "scenario-init {preset} should succeed, stderr: {}",
            String::from_utf8_lossy(&init.stderr)
        );
        assert!(out.exists(), "expected generated scenario to exist");

        let validate = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
            .arg("scenario-validate")
            .arg(&plc)
            .arg("--scenario")
            .arg(&out)
            .output()
            .expect("run scenario-validate");

        assert!(
            validate.status.success(),
            "scenario-validate should succeed for preset {preset}, stderr: {}",
            String::from_utf8_lossy(&validate.stderr)
        );
    }
}
