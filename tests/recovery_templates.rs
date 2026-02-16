use std::fs;
use std::path::Path;
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

fn template_path(file_name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("recovery_templates")
        .join(file_name)
}

#[test]
fn recovery_templates_compile_and_pass_sequence_lint() {
    let templates = [
        "estop_recovery.plc",
        "power_loss_recovery.plc",
        "sensor_stuck_recovery.plc",
    ];

    for file_name in templates {
        let plc_path = template_path(file_name);
        let base = temp_dir(&format!("rust_plc_template_{file_name}"));
        let report_path = base.join("verification_report.json");

        let compile_output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
            .arg(&plc_path)
            .arg("--report")
            .arg(&report_path)
            .output()
            .expect("run compile command on template");

        assert!(
            compile_output.status.success(),
            "template {} should compile, stderr: {}",
            file_name,
            String::from_utf8_lossy(&compile_output.stderr)
        );
        assert!(report_path.exists(), "report should exist for {file_name}");

        let lint_output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
            .arg("sequence-lint")
            .arg(&plc_path)
            .arg("--critical-wait-level")
            .arg("error")
            .output()
            .expect("run sequence-lint on template");

        assert!(
            lint_output.status.success(),
            "template {} should pass sequence-lint, stderr: {}",
            file_name,
            String::from_utf8_lossy(&lint_output.stderr)
        );
    }
}
