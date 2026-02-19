use std::fs;
use std::path::Path;
use std::process::Command;

fn repo_path(p: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(p)
}

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

#[test]
fn scenario_gen_is_deterministic_and_emits_expected_number_of_cases() {
    let plc = repo_path("examples/assembly_station.plc");
    let config = repo_path("examples/scenario_gen/basic.yaml");
    assert!(plc.exists(), "expected PLC example to exist");
    assert!(config.exists(), "expected scenario-gen config to exist");

    let base = temp_dir("rust_plc_scenario_gen");
    let out_a = base.join("a");
    let out_b = base.join("b");

    let run = |out_dir: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_rust_plc"))
            .arg("scenario-gen")
            .arg("--plc")
            .arg(&plc)
            .arg("--config")
            .arg(&config)
            .arg("--out-dir")
            .arg(out_dir)
            .output()
            .expect("run scenario-gen")
    };

    let a = run(&out_a);
    assert!(
        a.status.success(),
        "scenario-gen should succeed, stderr: {}",
        String::from_utf8_lossy(&a.stderr)
    );
    let b = run(&out_b);
    assert!(
        b.status.success(),
        "scenario-gen should succeed (2nd run), stderr: {}",
        String::from_utf8_lossy(&b.stderr)
    );

    let summary_a = out_a.join("summary.json");
    let summary_b = out_b.join("summary.json");
    assert!(summary_a.exists(), "summary.json should exist");
    assert!(summary_b.exists(), "summary.json should exist (2nd run)");

    let json_a = fs::read_to_string(&summary_a).expect("read summary.json");
    let json_b = fs::read_to_string(&summary_b).expect("read summary.json 2nd");
    assert_eq!(json_a, json_b, "summary.json should be deterministic");

    let summary: serde_json::Value = serde_json::from_str(&json_a).expect("valid JSON");
    assert_eq!(
        summary.get("schema_version").and_then(|v| v.as_u64()),
        Some(1)
    );
    assert_eq!(summary.get("count").and_then(|v| v.as_u64()), Some(6));

    // The files referenced by summary should exist, and YAML should be stable.
    let cases = summary
        .get("cases")
        .and_then(|v| v.as_array())
        .expect("cases should be an array");
    assert_eq!(cases.len(), 6);

    for (i, case) in cases.iter().enumerate() {
        let rel = case
            .get("path")
            .and_then(|v| v.as_str())
            .expect("case path should be string");
        let file_a = out_a.join(rel);
        let file_b = out_b.join(rel);
        assert!(file_a.exists(), "expected case YAML to exist: {rel}");
        assert!(
            file_b.exists(),
            "expected case YAML to exist in 2nd run: {rel}"
        );
        let ya = fs::read_to_string(&file_a).expect("read YAML");
        let yb = fs::read_to_string(&file_b).expect("read YAML 2nd");
        assert_eq!(ya, yb, "YAML should be deterministic for case {i}");
    }

    // Sanity: the first generated scenario can be consumed by sim-plc.
    let trace_out = base.join("trace.jsonl");
    let first = out_a.join("scenario_0001.yaml");
    let sim = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("sim-plc")
        .arg(&plc)
        .arg("--scenario")
        .arg(&first)
        .arg("--out")
        .arg(&trace_out)
        .output()
        .expect("run sim-plc for generated case");
    assert!(
        sim.status.success(),
        "sim-plc should succeed for generated scenario, stderr: {}",
        String::from_utf8_lossy(&sim.stderr)
    );
    let trace = fs::read_to_string(&trace_out).expect("read trace");
    assert!(!trace.trim().is_empty(), "trace should be non-empty");
}

#[test]
fn scenario_gen_supports_dry_run_and_boundary_first_summary_contract() {
    let plc = repo_path("examples/assembly_station.plc");
    let config = repo_path("examples/scenario_gen/basic.yaml");
    let base = temp_dir("rust_plc_scenario_gen_dry_run");
    let out_dir = base.join("dry_run");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("scenario-gen")
        .arg("--plc")
        .arg(&plc)
        .arg("--config")
        .arg(&config)
        .arg("--out-dir")
        .arg(&out_dir)
        .arg("--coverage-mode")
        .arg("boundary-first")
        .arg("--dry-run")
        .output()
        .expect("run scenario-gen dry-run");
    assert!(
        output.status.success(),
        "scenario-gen dry-run should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let summary_path = out_dir.join("summary.json");
    assert!(
        summary_path.exists(),
        "dry-run should still emit summary.json"
    );
    assert!(
        !out_dir.join("scenario_0001.yaml").exists(),
        "dry-run should not emit scenario YAML files"
    );

    let summary: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(summary_path).expect("read summary"))
            .expect("summary json");
    assert_eq!(summary["coverage_mode"], "boundary-first");
    assert_eq!(summary["dry_run"], true);
    assert_eq!(summary["count"].as_u64(), Some(6));
    assert!(
        summary["templates"].as_array().map(|v| !v.is_empty()) == Some(true),
        "summary should include template metadata snapshot"
    );
    assert!(
        summary["cases"]
            .as_array()
            .and_then(|cases| cases.first())
            .and_then(|first| first.get("template_id"))
            .and_then(|v| v.as_str())
            .is_some(),
        "generated case should include template_id"
    );
}

#[test]
fn scenario_gen_risk_first_prioritizes_fault_cases() {
    let plc = repo_path("examples/assembly_station.plc");
    let base = temp_dir("rust_plc_scenario_gen_risk_first");
    let out_dir = base.join("risk_first");
    let config_path = base.join("risk.yaml");

    fs::write(
        &config_path,
        "seed_base: 7\ntick_ms: 10\nduration_ms: [100, 200]\nstart_pulse_ms: [10]\nsensor_window_ms: [10]\ninject_sensor_stuck: [false, true]\nmax_cases: 2\n",
    )
    .expect("write risk config");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("scenario-gen")
        .arg("--plc")
        .arg(&plc)
        .arg("--config")
        .arg(&config_path)
        .arg("--out-dir")
        .arg(&out_dir)
        .arg("--coverage-mode")
        .arg("risk-first")
        .output()
        .expect("run scenario-gen risk-first");
    assert!(
        output.status.success(),
        "scenario-gen risk-first should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let summary: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out_dir.join("summary.json")).expect("summary"))
            .expect("summary json");
    let first = summary["cases"]
        .as_array()
        .and_then(|cases| cases.first())
        .expect("at least one generated case");
    assert_eq!(
        first["inject_sensor_stuck"].as_bool(),
        Some(true),
        "risk-first should prioritize fault-injection cases first"
    );
}
