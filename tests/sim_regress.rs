use std::fs;
use std::path::Path;
use std::process::Command;

const PLC_FIXTURE: &str = r#"
[topology]

device Y0: digital_output
device X0: digital_input

device start_button: sensor

device valve_A: solenoid_valve

device cyl_A: cylinder

device sensor_ext: sensor

relation { from: start_button.out, to: X0.in, via: reports_to }
relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_ext.sense, via: detects }
relation { from: sensor_ext.out, to: X0.in, via: reports_to }

[constraints]

[tasks]

task main:
    step extend:
        action: extend cyl_A

    step wait_button:
        wait: start_button == true
        timeout: 50ms -> goto fault

    step dwell:
        delay: 20ms

    step retract:
        action: retract cyl_A

    on_complete: goto done

task fault:
    step retract_fault:
        action: retract cyl_A
    on_complete: goto done

task done:
    step halt:
"#;

const PLC_FIXTURE_MULTI_TIMEOUT: &str = r#"
[topology]

device X0: digital_input
device X1: digital_input

device start_a: sensor
device start_b: sensor
relation { from: start_a.out, to: X0.in, via: reports_to }
relation { from: start_b.out, to: X1.in, via: reports_to }

[constraints]

[tasks]

task main:
    step wait_a:
        wait: start_a == true
        timeout: 30ms -> goto fault

    step wait_b:
        wait: start_b == true
        timeout: 40ms -> goto fault

    on_complete: goto done

task fault:
    step halt:

task done:
    step halt_done:
"#;

#[test]
fn sim_regress_reports_one_pass_one_fail() {
    let base = std::env::temp_dir().join(format!(
        "rust_plc_sim_regress_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");

    let plc_dir = base.join("plcs");
    let scenario_dir = base.join("scenarios");
    let artifacts_dir = base.join("artifacts");
    fs::create_dir_all(&plc_dir).unwrap();
    fs::create_dir_all(&scenario_dir).unwrap();

    let plc_path = plc_dir.join("fixture.plc");
    fs::write(&plc_path, PLC_FIXTURE).expect("write plc");

    // Pass: satisfies the wait before timeout (50ms), so no timeout transition occurs.
    let pass_yaml = r#"
tick_ms: 10
duration_ms: 200
inputs:
  - at_ms: 10
    set:
      digital_inputs:
        0: true
"#;
    fs::write(scenario_dir.join("pass.yaml"), pass_yaml).expect("write pass scenario");

    // Fail: never satisfies the wait, so timeout triggers and the sim report records it as a failure.
    let fail_yaml = r#"
tick_ms: 10
duration_ms: 200
"#;
    fs::write(scenario_dir.join("fail.yaml"), fail_yaml).expect("write fail scenario");

    let summary = rust_plc::sim_regress::run_sim_regress(&plc_dir, &scenario_dir, &artifacts_dir)
        .expect("sim-regress should succeed");

    assert_eq!(summary.total, 2);
    assert_eq!(summary.pass, 1);
    assert_eq!(summary.fail, 1);
    assert_eq!(summary.failures.len(), 1);

    let f = &summary.failures[0];
    assert!(f.plc.ends_with("fixture.plc"));
    assert!(f.scenario.ends_with("fail.yaml"));
    assert_eq!(f.failure.kind, "timeout");
    assert!(Path::new(&f.artifact_dir).exists());

    let trace = f
        .trace_path
        .as_ref()
        .expect("failure should have trace_path");
    let report = f
        .report_path
        .as_ref()
        .expect("failure should have report_path");
    assert!(Path::new(trace).exists());
    assert!(Path::new(report).exists());
}

#[test]
fn sim_regress_can_minimize_failure_scenario() {
    let base = std::env::temp_dir().join(format!(
        "rust_plc_sim_regress_minimize_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");

    let plc_dir = base.join("plcs");
    let scenario_dir = base.join("scenarios");
    let artifacts_dir = base.join("artifacts");
    fs::create_dir_all(&plc_dir).unwrap();
    fs::create_dir_all(&scenario_dir).unwrap();

    let plc_path = plc_dir.join("fixture.plc");
    fs::write(&plc_path, PLC_FIXTURE).expect("write plc");

    // Fail with noise: extra input event + redundant fault. Minimization should strip it.
    let fail_yaml = r#"
tick_ms: 10
duration_ms: 200
inputs:
  - at_ms: 10
    set:
      digital_inputs:
        0: false
faults:
  - sensor_stuck:
      at_ms: 0
      target: 0
      value: false
"#;
    fs::write(scenario_dir.join("fail.yaml"), fail_yaml).expect("write fail scenario");

    let summary = rust_plc::sim_regress::run_sim_regress_with_options(
        &plc_dir,
        &scenario_dir,
        &artifacts_dir,
        rust_plc::sim_regress::SimRegressOptions { minimize: true },
    )
    .expect("sim-regress should succeed");

    assert_eq!(summary.total, 1);
    assert_eq!(summary.fail, 1);
    let f = &summary.failures[0];
    assert!(f.minimized_scenario_path.is_some());
    assert!(f.minimization.is_some());
    let mini = f.minimization.as_ref().unwrap();
    assert!(mini.minimized_duration_ms <= mini.original_duration_ms);
    assert!(mini.minimized_inputs <= mini.original_inputs);
    assert!(mini.minimized_input_assignments <= mini.original_input_assignments);
    assert!(mini.minimized_faults <= mini.original_faults);

    let minimized_path = f
        .minimized_scenario_path
        .as_ref()
        .map(Path::new)
        .expect("minimized scenario path");
    let minimized_yaml = fs::read_to_string(minimized_path).expect("read minimized scenario yaml");
    assert!(
        minimized_yaml.starts_with("# Minimized by `rust_plc sim-regress --minimize-failure`."),
        "minimized scenario should include a friendly header"
    );
    assert!(
        minimized_yaml.contains("# Source scenario:"),
        "minimized scenario should include source info"
    );
}

#[test]
fn sim_regress_minimization_keeps_failure_step_signature() {
    let base = std::env::temp_dir().join(format!(
        "rust_plc_sim_regress_signature_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");

    let plc_dir = base.join("plcs");
    let scenario_dir = base.join("scenarios");
    let artifacts_dir = base.join("artifacts");
    fs::create_dir_all(&plc_dir).unwrap();
    fs::create_dir_all(&scenario_dir).unwrap();

    fs::write(plc_dir.join("fixture.plc"), PLC_FIXTURE_MULTI_TIMEOUT).expect("write plc");

    // Required event: DI0 true to pass wait_a. Noise event: DI2 true (unused).
    // Failure should stay at wait_b timeout (step=1), not regress to wait_a timeout (step=0).
    let fail_yaml = r#"
tick_ms: 10
duration_ms: 200
inputs:
  - at_ms: 10
    set:
      digital_inputs:
        0: true
  - at_ms: 20
    set:
      digital_inputs:
        2: true
"#;
    fs::write(scenario_dir.join("fail.yaml"), fail_yaml).expect("write fail scenario");

    let summary = rust_plc::sim_regress::run_sim_regress_with_options(
        &plc_dir,
        &scenario_dir,
        &artifacts_dir,
        rust_plc::sim_regress::SimRegressOptions { minimize: true },
    )
    .expect("sim-regress should succeed");

    assert_eq!(summary.total, 1);
    assert_eq!(summary.fail, 1);
    let f = &summary.failures[0];
    assert_eq!(f.failure.kind, "timeout");
    assert_eq!(f.failure.task, Some(0));
    assert_eq!(f.failure.step, Some(1));

    let mini = f.minimization.as_ref().expect("minimization summary");
    assert!(mini.minimized_input_assignments <= mini.original_input_assignments);
    // DI0 assignment is still required to keep failure at step=1.
    assert!(mini.minimized_input_assignments >= 1);
}

#[test]
fn sim_regress_minimization_keeps_failure_signature_for_sugar_scenarios() {
    let base = std::env::temp_dir().join(format!(
        "rust_plc_sim_regress_sugar_minimize_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");

    let plc_dir = base.join("plcs");
    let scenario_dir = base.join("scenarios");
    let artifacts_dir = base.join("artifacts");
    fs::create_dir_all(&plc_dir).unwrap();
    fs::create_dir_all(&scenario_dir).unwrap();

    let plc = r#"
[topology]
device X0: digital_input
device X1: digital_input

device start_button: sensor
device noise_button: sensor
relation { from: start_button.out, to: X0.in, via: reports_to }
relation { from: noise_button.out, to: X1.in, via: reports_to }

[constraints]

[tasks]
task main:
    step wait_start:
        wait: start_button == true
        timeout: 30ms -> goto fault
    on_complete: goto done

task fault:
    step halt:

task done:
    step halt_done:
"#;
    fs::write(plc_dir.join("fixture.plc"), plc).expect("write plc");

    // This scenario uses `hold` sugar, but still fails with a timeout (start_button never true).
    let fail_yaml = r#"
tick_ms: 10
duration_ms: 200
hold:
  - from_ms: 0
    to_ms: 100
    digital: noise_button
    value: true
"#;
    fs::write(scenario_dir.join("fail.yaml"), fail_yaml).expect("write fail scenario");

    let summary = rust_plc::sim_regress::run_sim_regress_with_options(
        &plc_dir,
        &scenario_dir,
        &artifacts_dir,
        rust_plc::sim_regress::SimRegressOptions { minimize: true },
    )
    .expect("sim-regress should succeed");

    assert_eq!(summary.total, 1);
    assert_eq!(summary.fail, 1);
    let f = &summary.failures[0];
    assert_eq!(f.failure.kind, "timeout");
    assert!(f.minimized_scenario_path.is_some());

    let minimized_yaml =
        fs::read_to_string(f.minimized_scenario_path.as_ref().unwrap()).expect("read minimized");
    assert!(
        minimized_yaml.contains("Note: source scenario uses pulse/hold sugar"),
        "minimized scenario should mention sugar expansion"
    );
    assert!(
        minimized_yaml.contains("Failure signature:"),
        "minimized scenario should include failure signature"
    );
}

#[test]
fn sim_regress_cli_emits_feedback_json_when_minimize_failure_is_enabled() {
    let base = std::env::temp_dir().join(format!(
        "rust_plc_sim_regress_feedback_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");

    let plc_dir = base.join("plcs");
    let scenario_dir = base.join("scenarios");
    let artifacts_dir = base.join("artifacts");
    fs::create_dir_all(&plc_dir).unwrap();
    fs::create_dir_all(&scenario_dir).unwrap();

    fs::write(plc_dir.join("fixture.plc"), PLC_FIXTURE).expect("write plc");
    fs::write(
        scenario_dir.join("fail.yaml"),
        "tick_ms: 10\nduration_ms: 200\n",
    )
    .expect("write fail scenario");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("sim-regress")
        .arg("--plc-dir")
        .arg(&plc_dir)
        .arg("--scenario-dir")
        .arg(&scenario_dir)
        .arg("--artifacts-dir")
        .arg(&artifacts_dir)
        .arg("--minimize-failure")
        .output()
        .expect("run sim-regress cli");
    assert!(
        output.status.success(),
        "sim-regress cli should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let feedback_path = artifacts_dir.join("feedback.json");
    assert!(feedback_path.exists(), "feedback.json should be emitted");
    let feedback: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(feedback_path).expect("read feedback json"))
            .expect("feedback json");
    assert_eq!(feedback["schema_version"], 1);
    assert_eq!(feedback["total_failures"].as_u64(), Some(1));
    assert!(
        feedback["feedback"]
            .as_array()
            .and_then(|items| items.first())
            .and_then(|first| first.get("template_hint"))
            .and_then(|v| v.as_str())
            .is_some(),
        "feedback entry should include template_hint"
    );
    assert!(
        feedback["feedback"]
            .as_array()
            .and_then(|items| items.first())
            .and_then(|first| first.get("parameter_hints"))
            .and_then(|v| v.as_array())
            .map(|hints| !hints.is_empty())
            == Some(true),
        "feedback entry should include parameter_hints"
    );
}
