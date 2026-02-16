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

fn write_plc(path: &Path) {
    let source = r#"
[topology]

[constraints]

[tasks]

task main:
    step wait_sensor:
        wait: X0 == true
    step done:
        action: log "done"
"#;
    fs::write(path, source).expect("write plc fixture");
}

#[test]
fn sequence_lint_warn_mode_reports_but_does_not_fail() {
    let base = temp_dir("rust_plc_sequence_lint_warn");
    let plc_path = base.join("fixture.plc");
    write_plc(&plc_path);

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("sequence-lint")
        .arg(&plc_path)
        .arg("--critical-wait-level")
        .arg("warn")
        .output()
        .expect("run sequence-lint warn");

    assert!(
        output.status.success(),
        "warn mode should not fail, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("WARN [sequence-lint]"),
        "warn mode should print lint warnings"
    );
}

#[test]
fn sequence_lint_error_mode_fails_for_missing_timeout() {
    let base = temp_dir("rust_plc_sequence_lint_error");
    let plc_path = base.join("fixture.plc");
    write_plc(&plc_path);

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("sequence-lint")
        .arg(&plc_path)
        .arg("--critical-wait-level")
        .arg("error")
        .output()
        .expect("run sequence-lint error");

    assert!(
        !output.status.success(),
        "error mode should fail when critical wait is not recoverable"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERROR [sequence-lint]"));
    assert!(stderr.contains("critical wait finding"));
}

#[test]
fn sequence_lint_exemption_suppresses_error() {
    let base = temp_dir("rust_plc_sequence_lint_exempt");
    let plc_path = base.join("fixture.plc");
    write_plc(&plc_path);

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("sequence-lint")
        .arg(&plc_path)
        .arg("--critical-wait-level")
        .arg("error")
        .arg("--critical-wait-exempt")
        .arg("main.wait_sensor")
        .output()
        .expect("run sequence-lint with exemption");

    assert!(
        output.status.success(),
        "exempted step should pass, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("sequence-lint: PASS"),
        "expected pass message"
    );
}
