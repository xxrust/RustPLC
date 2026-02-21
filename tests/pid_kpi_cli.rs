use std::fs;
use std::process::Command;

#[test]
fn sim_pid_kpi_cli_is_deterministic_and_within_thresholds() {
    let base = std::env::temp_dir().join(format!(
        "rust_plc_pid_kpi_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");

    let plc_path = base.join("pid.plc");
    let scenario_path = base.join("scenario.yaml");
    let out1_path = base.join("kpi1.json");
    let out2_path = base.join("kpi2.json");

    let plc = r#"
[topology]
device AI0: analog_input { range: 0..100, unit: "bar", external: true }
device AO0: analog_output { range: 0..100, unit: "%" }
device loop_pressure: pid {
    pv: AI0,
    sp: 60bar,
    kp: 2.0,
    ki: 0.5,
    kd: 0.0,
    out: AO0,
    period_ms: 100,
    limit: 0..100
}

[constraints]

[tasks]
task main:
    step hold:
"#;
    fs::write(&plc_path, plc).expect("write plc");

    let scenario = r#"
tick_ms: 100
duration_ms: 10000
loop_index: 0
initial_pv: 0.0
model:
  kind: first_order
  gain: 1.0
  tau_ms: 1200
"#;
    fs::write(&scenario_path, scenario).expect("write scenario");

    for out in [&out1_path, &out2_path] {
        let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
            .arg("sim-pid-kpi")
            .arg(&plc_path)
            .arg("--scenario")
            .arg(&scenario_path)
            .arg("--out")
            .arg(out)
            .output()
            .expect("run sim-pid-kpi");
        assert!(
            output.status.success(),
            "sim-pid-kpi failed, stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let kpi1_text = fs::read_to_string(&out1_path).expect("read kpi1");
    let kpi2_text = fs::read_to_string(&out2_path).expect("read kpi2");
    let kpi1: serde_json::Value = serde_json::from_str(&kpi1_text).expect("kpi1 json");
    let kpi2: serde_json::Value = serde_json::from_str(&kpi2_text).expect("kpi2 json");

    assert_eq!(
        kpi1, kpi2,
        "same scenario should produce deterministic KPI JSON"
    );

    let overshoot = kpi1["kpi"]["overshoot_percent"]
        .as_f64()
        .expect("overshoot as f64");
    let steady_err = kpi1["kpi"]["steady_state_error"]
        .as_f64()
        .expect("steady_state_error as f64");
    let settling = kpi1["kpi"]["settling_time_ms"].as_u64().unwrap_or(10_000);

    assert!(
        overshoot <= 20.0,
        "overshoot should stay under threshold, got {overshoot}"
    );
    assert!(
        steady_err <= 5.0,
        "steady-state error should stay under threshold, got {steady_err}"
    );
    assert!(settling <= 10_000);
}
