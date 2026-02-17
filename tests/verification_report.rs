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
    assert!(
        report["runtime_budget"].is_object(),
        "report should include runtime_budget object"
    );
    assert!(
        report["runtime_budget"]["budget_time_estimate"].is_object(),
        "runtime_budget should include budget_time_estimate object"
    );
    assert_eq!(report["verification"]["safety"]["skipped_rules"], 0);
    assert!(
        report["verification"]["safety"]["checked_rules"].is_number(),
        "checked_rules should be numeric"
    );
    assert!(
        report["verification"]["safety"]["coverage"].is_object(),
        "safety should include coverage object"
    );
    assert!(
        report["verification"]["safety"]["rule_statuses"].is_array(),
        "safety should include rule_statuses array"
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

#[test]
fn budget_thresholds_emit_warn_entries() {
    let base = temp_dir("rust_plc_budget_warn");
    let plc_path = base.join("budget_warn.plc");
    let report_path = base.join("budget_warn_report.json");

    let source = r#"
[topology]
device X0: digital_input
device Y0: digital_output

[constraints]

[tasks]
task main:
    step s1:
        action: set Y0 on
"#;
    fs::write(&plc_path, source).expect("write plc");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg(&plc_path)
        .arg("--report")
        .arg(&report_path)
        .arg("--budget-max-actions-per-transition")
        .arg("0")
        .output()
        .expect("run rust_plc");

    assert!(
        output.status.success(),
        "runtime budget warning should not fail by default, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report_path).expect("read report"))
            .expect("report JSON should parse");
    let timing_warnings = report["verification"]["timing"]["warnings"]
        .as_array()
        .expect("timing warnings should be array");
    assert!(
        timing_warnings
            .iter()
            .any(|w| w["level"] == "warn" && w["message"].as_str().unwrap_or("").contains("runtime budget")),
        "budget threshold exceed should add warn-level timing warning"
    );
}

#[test]
fn budget_time_estimate_warns_and_deny_warnings_can_block() {
    let base = temp_dir("rust_plc_budget_time_estimate");
    let plc_path = base.join("budget_time.plc");
    let report_path = base.join("budget_time_report.json");

    let source = r#"
[topology]
device X0: digital_input
device Y0: digital_output

[constraints]

[tasks]
task main:
    step s1:
        action: set Y0 on
"#;
    fs::write(&plc_path, source).expect("write plc");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg(&plc_path)
        .arg("--report")
        .arg(&report_path)
        .arg("--budget-action-cost-us")
        .arg("20")
        .arg("--budget-transition-cost-us")
        .arg("10")
        .arg("--budget-parallel-expand-cost-us")
        .arg("5")
        .arg("--budget-max-time-estimate-us")
        .arg("10")
        .output()
        .expect("run rust_plc");

    assert!(
        output.status.success(),
        "budget time estimate warning should not fail by default, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report_path).expect("read report"))
            .expect("report JSON should parse");
    assert_eq!(
        report["runtime_budget"]["budget_time_estimate"]["max_allowed_us"].as_u64(),
        Some(10)
    );
    assert_eq!(
        report["runtime_budget"]["budget_time_estimate"]["exceeds_budget"].as_bool(),
        Some(true)
    );

    let timing_warnings = report["verification"]["timing"]["warnings"]
        .as_array()
        .expect("timing warnings should be array");
    assert!(
        timing_warnings.iter().any(|w| {
            w["level"] == "warn"
                && w["message"]
                    .as_str()
                    .unwrap_or("")
                    .contains("runtime budget time estimate")
        }),
        "over-budget estimate should emit warn-level timing warning"
    );

    let deny_output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg(&plc_path)
        .arg("--report")
        .arg(base.join("budget_time_report_deny.json"))
        .arg("--budget-action-cost-us")
        .arg("20")
        .arg("--budget-transition-cost-us")
        .arg("10")
        .arg("--budget-parallel-expand-cost-us")
        .arg("5")
        .arg("--budget-max-time-estimate-us")
        .arg("10")
        .arg("--deny-warnings")
        .output()
        .expect("run rust_plc --deny-warnings");

    assert!(
        !deny_output.status.success(),
        "deny-warnings should fail for over-budget time estimate warning"
    );
    let deny_stderr = String::from_utf8_lossy(&deny_output.stderr);
    assert!(
        deny_stderr.contains("runtime budget time estimate"),
        "deny-warnings stderr should mention budget time estimate warning"
    );
}
