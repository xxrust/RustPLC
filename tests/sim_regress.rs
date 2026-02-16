use std::fs;
use std::path::Path;

const PLC_FIXTURE: &str = r#"
[topology]

device Y0: digital_output
device X0: digital_input

device start_button: digital_input {
    connected_to: X0
}

device valve_A: solenoid_valve {
    connected_to: Y0
}

device cyl_A: cylinder {
    connected_to: valve_A
}

device sensor_ext: sensor {
    connected_to: X0
    detects: cyl_A.extended
}

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
    assert!(mini.minimized_faults <= mini.original_faults);
}
