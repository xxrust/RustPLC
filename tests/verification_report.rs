use std::fs;
use std::process::Command;

fn temp_dir(prefix: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "{prefix}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should work")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn cli_writes_structured_verification_report_with_counts() {
    let base = temp_dir("rust_plc_verification_report_ok");
    let plc_path = base.join("ok.plc");
    let report_path = base.join("my_report.json");

    let source = r#"
[topology]
device start_button: digital_input
device motor: digital_output

[constraints]

[tasks]
task main:
    step run:
        action: set motor on
"#;
    fs::write(&plc_path, source).expect("write plc");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg(&plc_path)
        .arg("--report")
        .arg(&report_path)
        .output()
        .expect("run rust_plc");

    assert!(
        output.status.success(),
        "rust_plc should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(report_path.exists(), "report file should be written");

    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report_path).expect("read report"))
            .expect("report JSON should parse");

    assert_eq!(
        report["source_plc"].as_str(),
        Some(
            plc_path
                .to_str()
                .expect("temp PLC path should be valid UTF-8")
        )
    );
    assert!(
        report["generated_at"].as_str().is_some(),
        "report should include generated_at"
    );
    assert_eq!(report["schema_version"], 1);
    assert!(
        report["tool_version"].as_str().is_some(),
        "report should include tool_version"
    );
    assert_eq!(report["verification"]["safety"]["skipped_rules"], 0);
    assert!(
        report["verification"]["safety"]["checked_rules"].is_number(),
        "checked_rules should be numeric"
    );
    assert!(report["verification"]["safety"]["warnings"].is_array());

    assert!(report["verification"]["liveness"]["warnings"].is_array());
    assert!(report["verification"]["timing"]["warnings"].is_array());
    assert!(report["verification"]["causality"]["warnings"].is_array());
    assert!(report["verification"]["liveness"]["checked_rules"].is_number());
    assert!(report["verification"]["timing"]["checked_rules"].is_number());
    assert!(report["verification"]["causality"]["checked_rules"].is_number());
}

#[test]
fn cli_report_captures_bounded_safety_warning() {
    let base = temp_dir("rust_plc_verification_report_warn");
    let plc_path = base.join("warn.plc");
    let report_path = base.join("warn_report.json");

    let source = r#"
[topology]
device mode_switch: digital_input
device out_a: digital_output
device out_b: digital_output

[constraints]
safety: out_a.on requires out_a.on

[tasks]
task choose:
    step wait_mode:
        wait: mode_switch == true
        allow_indefinite_wait: true
    step decide:
        if: mode_switch == true goto process_A else: goto process_B

task process_A:
    step run:
        action: set out_a on
    on_complete: goto choose

task process_B:
    step run:
        action: set out_b on
    on_complete: goto choose
"#;
    fs::write(&plc_path, source).expect("write plc");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg(&plc_path)
        .arg("--report")
        .arg(&report_path)
        .output()
        .expect("run rust_plc");

    assert!(
        output.status.success(),
        "rust_plc should still succeed with bounded safety, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report_path).expect("read report"))
            .expect("report JSON should parse");

    assert_eq!(report["verification"]["safety"]["level"], "有界验证");
    assert_eq!(report["verification"]["safety"]["checked_rules"], 1);
    assert_eq!(report["verification"]["safety"]["skipped_rules"], 0);

    let warnings = report["verification"]["safety"]["warnings"]
        .as_array()
        .expect("safety warnings should be an array");
    assert!(
        warnings
            .iter()
            .any(|w| w["level"] == "warn" && w["message"].as_str().is_some()),
        "bounded safety case should include warn-level warning entries"
    );
}

#[test]
fn deny_warnings_fails_process_when_warns_exist() {
    let base = temp_dir("rust_plc_verification_report_deny_warn");
    let plc_path = base.join("warn.plc");
    let report_path = base.join("warn_report.json");

    let source = r#"
[topology]
device mode_switch: digital_input
device out_a: digital_output
device out_b: digital_output

[constraints]
safety: out_a.on requires out_a.on

[tasks]
task choose:
    step wait_mode:
        wait: mode_switch == true
        allow_indefinite_wait: true
    step decide:
        if: mode_switch == true goto process_A else: goto process_B

task process_A:
    step run:
        action: set out_a on
    on_complete: goto choose

task process_B:
    step run:
        action: set out_b on
    on_complete: goto choose
"#;
    fs::write(&plc_path, source).expect("write plc");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg(&plc_path)
        .arg("--report")
        .arg(&report_path)
        .arg("--deny-warnings")
        .output()
        .expect("run rust_plc");

    assert!(
        !output.status.success(),
        "deny-warnings should fail the process when warn entries exist"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--deny-warnings"));
    assert!(stderr.contains("[safety]"));
}
