use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use runtime_core::MAX_TRANSITIONS_PER_TASK_PER_TICK;
use rust_plc::verification::WarningEntry;

#[derive(Debug, serde::Deserialize)]
struct LegacyWarningEntry {
    level: String,
    message: String,
}

#[derive(Debug, serde::Deserialize)]
struct LegacyCheckerSummary {
    warnings: Vec<LegacyWarningEntry>,
}

#[derive(Debug, serde::Deserialize)]
struct LegacyVerificationSummary {
    liveness: LegacyCheckerSummary,
}

#[derive(Debug, serde::Deserialize)]
struct LegacyVerificationReport {
    verification: LegacyVerificationSummary,
}

fn temp_dir(prefix: &str) -> PathBuf {
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

fn write_plc(base: &Path, name: &str, source: &str) -> PathBuf {
    let plc_path = base.join(name);
    fs::write(&plc_path, source).expect("write plc");
    plc_path
}

fn read_report(report_path: &Path) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(report_path).expect("read report"))
        .expect("report JSON should parse")
}

fn run_compile_report(plc_path: &Path, report_path: &Path, extra_args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rust_plc"));
    cmd.arg(plc_path).arg("--report").arg(report_path);
    cmd.args(extra_args);
    cmd.output().expect("run rust_plc")
}

fn report_ok_source() -> &'static str {
    r#"
[topology]
device plc_main: plc {
    purpose: "controller"
    model_ref: openplc_softplc
}
device run_lamp: solenoid_valve {
    purpose: "demo output"
    response_time: 20ms
}
relation { from: plc_main.Y0, to: run_lamp.coil, via: driven_by }

[constraints]

[tasks]
task main:
    step run:
        action: set run_lamp on
"#
}

fn budget_single_output_source() -> &'static str {
    r#"
[topology]
device plc_main: plc {
    purpose: "controller"
    model_ref: openplc_softplc
}
device run_lamp: solenoid_valve {
    purpose: "test output"
    response_time: 20ms
}
relation { from: plc_main.Y0, to: run_lamp.coil, via: driven_by }

[constraints]

[tasks]
task main:
    step s1:
        action: set run_lamp on
"#
}

fn budget_two_tasks_source(with_cycle: bool) -> String {
    let task_suffix = if with_cycle {
        r#"
        allow_indefinite_wait: true
    on_complete: goto station_a

task station_b:
    step run:
        action: set station_b_lamp on
        allow_indefinite_wait: true
    on_complete: goto station_b
"#
    } else {
        r#"

task station_b:
    step run:
        action: set station_b_lamp on
"#
    };

    format!(
        r#"
[topology]
device plc_main: plc {{
    purpose: "controller"
    model_ref: openplc_softplc
}}
device station_a_lamp: solenoid_valve {{ purpose: "station A output", response_time: 20ms }}
device station_b_lamp: solenoid_valve {{ purpose: "station B output", response_time: 20ms }}
relation {{ from: plc_main.Y0, to: station_a_lamp.coil, via: driven_by }}
relation {{ from: plc_main.Y1, to: station_b_lamp.coil, via: driven_by }}

[constraints]

[tasks]
task station_a:
    step run:
        action: set station_a_lamp on{task_suffix}"#
    )
}

fn axis_blocking_warning_source() -> &'static str {
    r#"
[topology]
device axis_x: stepper_motor {
    purpose: "x axis"
    model_ref: stepper_generic
    config_ref: stepper_default
    motion_param_set: stepper_default_fast
}

[constraints]

[tasks]
task motion:
    step run:
        action: set axis_x.enable on
        action: axis.move_relative(axis_x, distance: 10, params: stepper_default_fast, speed: 2)
            timeout: 500ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
    on_complete: goto done

task fault:
    step timeout:
    step reject:
    step motion_fault:
    step safety_fault:

task done:
    step halt:
"#
}

fn multi_controller_without_station_protocol_source() -> &'static str {
    r#"
[topology]
device plc_load: plc {
    purpose: "load station controller"
    model_ref: openplc_softplc
}
device plc_press: plc {
    purpose: "press station controller"
    model_ref: openplc_softplc
}
device load_lamp: solenoid_valve {
    purpose: "load station output"
    response_time: 20ms
}
device press_lamp: solenoid_valve {
    purpose: "press station output"
    response_time: 20ms
}
relation { from: plc_load.Y0, to: load_lamp.coil, via: driven_by }
relation { from: plc_press.Y0, to: press_lamp.coil, via: driven_by }

[constraints]

[tasks]
task main:
    step idle:
        action: set load_lamp on
"#
}

fn station_protocol_contract_source() -> &'static str {
    r#"
[topology]
device plc_load: plc {
    purpose: "load station controller"
    model_ref: openplc_softplc
}
device plc_press: plc {
    purpose: "press station controller"
    model_ref: openplc_softplc
}
device load_fixture: cylinder {
    purpose: "load station fixture"
}
device press_fixture: cylinder {
    purpose: "press station fixture"
}
workpiece part: workpiece_type {
    ingress_sites: [handoff]
}
site handoff: workpiece_location { capacity: 1 }

station load_station { owns: [load_fixture], tasks: [load_cycle] }
station press_station { owns: [press_fixture], tasks: [press_cycle] }
handshake load_to_press_ready {
    from: load_station,
    to: press_station,
    request: load_request,
    allow: press_allow,
    complete: load_complete,
    timeout: 500ms -> goto fault.timeout
}
transfer_point load_to_press {
    from_station: load_station,
    to_station: press_station,
    site: handoff,
    handshake: load_to_press_ready
}
controller_sync load_press_sync {
    controllers: [plc_load, plc_press],
    max_skew: 5ms,
    heartbeat: 100ms
}

[constraints]

[tasks]
task load_cycle:
    step idle:
task press_cycle:
    step idle:
task fault:
    step timeout:
"#
}

#[test]
fn cli_writes_structured_verification_report_with_counts() {
    let base = temp_dir("rust_plc_verification_report_ok");
    let plc_path = write_plc(&base, "ok.plc", report_ok_source());
    let report_path = base.join("my_report.json");

    let output = run_compile_report(&plc_path, &report_path, &[]);
    assert!(
        output.status.success(),
        "rust_plc should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(report_path.exists(), "report file should be written");

    let report = read_report(&report_path);
    assert_eq!(
        report["source_plc"].as_str(),
        Some(
            plc_path
                .to_str()
                .expect("temp PLC path should be valid UTF-8")
        )
    );
    assert!(report["generated_at"].as_str().is_some());
    assert_eq!(report["schema_version"], 1);
    assert!(report["tool_version"].as_str().is_some());
    assert!(report["runtime_budget"].is_object());
    assert!(report["runtime_budget"]["budget_time_estimate"].is_object());
    assert_eq!(report["verification"]["safety"]["skipped_rules"], 0);
    assert!(report["verification"]["safety"]["checked_rules"].is_number());
    assert!(report["verification"]["safety"]["coverage"].is_object());
    assert!(report["verification"]["safety"]["rule_statuses"].is_array());
    assert!(report["verification"]["safety"]["warnings"].is_array());
    assert!(report["verification"]["liveness"]["warnings"].is_array());
    assert!(report["verification"]["timing"]["warnings"].is_array());
    assert!(report["verification"]["causality"]["warnings"].is_array());
    assert!(report["verification"]["station_protocol"]["warnings"].is_array());
    assert!(report["verification"]["liveness"]["checked_rules"].is_number());
    assert!(report["verification"]["timing"]["checked_rules"].is_number());
    assert!(report["verification"]["causality"]["checked_rules"].is_number());
    assert!(report["verification"]["station_protocol"]["checked_rules"].is_number());
}

#[test]
fn station_protocol_checker_warns_on_multi_controller_without_contract() {
    let base = temp_dir("rust_plc_verification_report_station_protocol_warn");
    let plc_path = write_plc(
        &base,
        "multi_controller.plc",
        multi_controller_without_station_protocol_source(),
    );
    let report_path = base.join("station_protocol_report.json");

    let output = run_compile_report(&plc_path, &report_path, &[]);
    assert!(
        output.status.success(),
        "multi-controller source should compile with station protocol warning, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report = read_report(&report_path);
    let station_protocol = &report["verification"]["station_protocol"];
    assert_eq!(station_protocol["checked_rules"], 0);
    let warnings = station_protocol["warnings"]
        .as_array()
        .expect("station protocol warnings should be an array");
    assert!(
        warnings.iter().any(|warning| warning["code"] == "STP-001"
            && warning["message"]
                .as_str()
                .is_some_and(|message| message.contains("2 PLC controllers"))),
        "expected STP-001 warning in station protocol summary, got: {warnings:?}"
    );
    assert!(
        warnings.iter().any(|warning| warning["code"] == "STP-002"
            && warning["message"]
                .as_str()
                .is_some_and(|message| message.contains("controller_sync"))),
        "expected STP-002 warning in station protocol summary, got: {warnings:?}"
    );
}

#[test]
fn station_protocol_checker_counts_explicit_contract() {
    let base = temp_dir("rust_plc_verification_report_station_protocol_ok");
    let plc_path = write_plc(
        &base,
        "station_protocol.plc",
        station_protocol_contract_source(),
    );
    let report_path = base.join("station_protocol_report.json");

    let output = run_compile_report(&plc_path, &report_path, &[]);
    assert!(
        output.status.success(),
        "station protocol source should compile, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report = read_report(&report_path);
    let station_protocol = &report["verification"]["station_protocol"];
    assert_eq!(station_protocol["checked_rules"], 5);
    assert!(
        station_protocol["warnings"]
            .as_array()
            .expect("station protocol warnings should be an array")
            .is_empty(),
        "explicit station protocol should not emit warnings: {station_protocol:?}"
    );
}

#[test]
fn cli_report_captures_warn_entry_payload() {
    let base = temp_dir("rust_plc_verification_report_warn");
    let plc_path = write_plc(&base, "warn.plc", budget_single_output_source());
    let report_path = base.join("warn_report.json");

    let output = run_compile_report(
        &plc_path,
        &report_path,
        &["--budget-max-actions-per-transition", "0"],
    );
    assert!(
        output.status.success(),
        "rust_plc should still succeed with warn entries, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report = read_report(&report_path);
    let warnings = report["verification"]["timing"]["warnings"]
        .as_array()
        .expect("timing warnings should be an array");
    assert!(
        warnings.iter().any(|w| {
            w["level"] == "warn"
                && w["message"]
                    .as_str()
                    .unwrap_or("")
                    .contains("runtime budget")
        }),
        "report should include warn-level warning entries"
    );
}

#[test]
fn deny_warnings_fails_process_when_warns_exist() {
    let base = temp_dir("rust_plc_verification_report_deny_warn");
    let plc_path = write_plc(&base, "warn.plc", budget_single_output_source());
    let report_path = base.join("warn_report.json");

    let output = run_compile_report(
        &plc_path,
        &report_path,
        &[
            "--budget-max-actions-per-transition",
            "0",
            "--deny-warnings",
        ],
    );
    assert!(
        !output.status.success(),
        "deny-warnings should fail the process when warn entries exist"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--deny-warnings"));
    assert!(stderr.contains("verification warnings"));
}

#[test]
fn budget_thresholds_emit_warn_entries() {
    let base = temp_dir("rust_plc_budget_warn");
    let plc_path = write_plc(&base, "budget_warn.plc", budget_single_output_source());
    let report_path = base.join("budget_warn_report.json");

    let output = run_compile_report(
        &plc_path,
        &report_path,
        &["--budget-max-actions-per-transition", "0"],
    );
    assert!(
        output.status.success(),
        "runtime budget warning should not fail by default, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report = read_report(&report_path);
    let timing_warnings = report["verification"]["timing"]["warnings"]
        .as_array()
        .expect("timing warnings should be array");
    assert!(
        timing_warnings.iter().any(|w| {
            w["level"] == "warn"
                && w["message"]
                    .as_str()
                    .unwrap_or("")
                    .contains("runtime budget")
        }),
        "budget threshold exceed should add warn-level timing warning"
    );
}

#[test]
fn budget_time_estimate_warns_and_deny_warnings_can_block() {
    let base = temp_dir("rust_plc_budget_time_estimate");
    let plc_path = write_plc(&base, "budget_time.plc", budget_single_output_source());
    let report_path = base.join("budget_time_report.json");

    let warn_args = [
        "--budget-action-cost-us",
        "20",
        "--budget-transition-cost-us",
        "10",
        "--budget-parallel-expand-cost-us",
        "5",
        "--budget-max-time-estimate-us",
        "10",
    ];

    let output = run_compile_report(&plc_path, &report_path, &warn_args);
    assert!(
        output.status.success(),
        "budget time estimate warning should not fail by default, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report = read_report(&report_path);
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

    let deny_output = run_compile_report(
        &plc_path,
        &base.join("budget_time_report_deny.json"),
        &[
            "--budget-action-cost-us",
            "20",
            "--budget-transition-cost-us",
            "10",
            "--budget-parallel-expand-cost-us",
            "5",
            "--budget-max-time-estimate-us",
            "10",
            "--deny-warnings",
        ],
    );
    assert!(
        !deny_output.status.success(),
        "deny-warnings should fail for over-budget time estimate warning"
    );
    let deny_stderr = String::from_utf8_lossy(&deny_output.stderr);
    assert!(deny_stderr.contains("runtime budget time estimate"));
}

#[test]
fn runtime_budget_reports_per_task_scope_for_two_active_tasks() {
    let base = temp_dir("rust_plc_budget_two_tasks_scope");
    let plc_path = write_plc(
        &base,
        "budget_two_tasks_scope.plc",
        &budget_two_tasks_source(false),
    );
    let report_path = base.join("budget_two_tasks_scope_report.json");

    let output = run_compile_report(&plc_path, &report_path, &[]);
    assert!(
        output.status.success(),
        "rust_plc should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report = read_report(&report_path);
    let cap = MAX_TRANSITIONS_PER_TASK_PER_TICK as u64;
    assert_eq!(
        report["runtime_budget"]["transition_budget_scope"].as_str(),
        Some("per_task_per_tick")
    );
    assert_eq!(
        report["runtime_budget"]["active_task_count"].as_u64(),
        Some(2)
    );
    assert_eq!(
        report["runtime_budget"]["max_transitions_per_tick_cap"].as_u64(),
        Some(cap)
    );
    assert_eq!(
        report["runtime_budget"]["max_transitions_all_tasks_per_tick_upper_bound"].as_u64(),
        Some(cap.saturating_mul(2))
    );
    assert_eq!(
        report["runtime_budget"]["max_actions_per_tick_upper_bound"].as_u64(),
        Some(cap.saturating_mul(2))
    );
}

#[test]
fn runtime_budget_cycle_warning_keeps_per_task_cap_with_two_active_tasks() {
    let base = temp_dir("rust_plc_budget_two_tasks_cycle");
    let plc_path = write_plc(
        &base,
        "budget_two_tasks_cycle.plc",
        &budget_two_tasks_source(true),
    );
    let report_path = base.join("budget_two_tasks_cycle_report.json");

    let output = run_compile_report(&plc_path, &report_path, &[]);
    assert!(
        output.status.success(),
        "rust_plc should succeed even with budget warning, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report = read_report(&report_path);
    let cap = MAX_TRANSITIONS_PER_TASK_PER_TICK as u64;
    assert_eq!(
        report["runtime_budget"]["active_task_count"].as_u64(),
        Some(2)
    );
    assert_eq!(
        report["runtime_budget"]["max_transitions_same_tick_upper_bound"].as_u64(),
        Some(cap)
    );
    assert_eq!(
        report["runtime_budget"]["max_transitions_all_tasks_per_tick_upper_bound"].as_u64(),
        Some(cap.saturating_mul(2))
    );
    assert_eq!(
        report["runtime_budget"]["has_same_tick_cycle"].as_bool(),
        Some(true)
    );

    let timing_warnings = report["verification"]["timing"]["warnings"]
        .as_array()
        .expect("timing warnings should be array");
    assert!(
        timing_warnings.iter().any(|w| {
            let msg = w["message"].as_str().unwrap_or("");
            w["level"] == "warn"
                && msg.contains("per task per tick")
                && msg.contains("active_tasks=2")
        }),
        "cycle warning should explain per-task cap with active task count"
    );
}

#[test]
fn report_emits_axis_blocking_migration_warning_with_stable_code() {
    let base = temp_dir("rust_plc_axis_blocking_migration_warning");
    let plc_path = write_plc(
        &base,
        "axis_blocking_warning.plc",
        axis_blocking_warning_source(),
    );
    let report_path = base.join("axis_blocking_warning_report.json");

    let output = run_compile_report(&plc_path, &report_path, &[]);
    assert!(
        output.status.success(),
        "rust_plc should succeed with migration warning, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report_text = fs::read_to_string(&report_path).expect("read report");
    let report: serde_json::Value =
        serde_json::from_str(&report_text).expect("report JSON should parse");
    let liveness_warnings = report["verification"]["liveness"]["warnings"]
        .as_array()
        .expect("liveness warnings should be array");
    assert!(
        liveness_warnings.iter().any(|warning| {
            warning["level"] == "warn"
                && warning["code"] == "MIG-AXIS-BLOCK-001"
                && warning["message"]
                    .as_str()
                    .unwrap_or("")
                    .contains("axis.move_* now executes with default blocking semantics")
        }),
        "axis migration warning should include stable code MIG-AXIS-BLOCK-001"
    );

    let legacy_report: LegacyVerificationReport =
        serde_json::from_str(&report_text).expect("legacy parser should accept new report payload");
    assert!(
        legacy_report
            .verification
            .liveness
            .warnings
            .iter()
            .any(|warning| warning.level == "warn" && warning.message.contains("axis.move_*")),
        "legacy warning parser should still read level/message from code-aware warnings"
    );
}

#[test]
fn warning_entry_new_schema_parses_legacy_warning_payload() {
    let legacy_payload = r#"{"level":"warn","message":"legacy-only warning"}"#;
    let parsed: WarningEntry = serde_json::from_str(legacy_payload)
        .expect("new WarningEntry schema should accept payload without code");
    assert_eq!(parsed.level, rust_plc::verification::WarningLevel::Warn);
    assert!(parsed.code.is_none());
}
