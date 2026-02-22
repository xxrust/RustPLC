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

fn run_virtual_board_and_timing_report(
    plc_path: &std::path::Path,
    scenario_path: &std::path::Path,
    out_dir: &std::path::Path,
) -> serde_json::Value {
    let vb = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("virtual-board")
        .arg(plc_path)
        .arg("--scenario")
        .arg(scenario_path)
        .arg("--out-dir")
        .arg(out_dir)
        .output()
        .expect("run virtual-board");
    assert!(
        vb.status.success(),
        "virtual-board should succeed, stderr: {}",
        String::from_utf8_lossy(&vb.stderr)
    );

    let timing_report_path = out_dir.join("timing_report.json");
    let tr = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("timing-report")
        .arg("--in")
        .arg(out_dir.join("tick_timing.jsonl"))
        .arg("--out")
        .arg(&timing_report_path)
        .output()
        .expect("run timing-report");
    assert!(
        tr.status.success(),
        "timing-report should succeed, stderr: {}",
        String::from_utf8_lossy(&tr.stderr)
    );

    serde_json::from_str(&fs::read_to_string(&timing_report_path).expect("read timing report"))
        .expect("timing report json")
}

#[test]
fn high_load_scenarios_are_deterministic_and_gateable() {
    let base = temp_dir("rust_plc_realtime_stress");
    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/realtime_stress");
    let plc_path = fixture_root.join("stress_case.plc");
    let safe_scenario_path = fixture_root.join("scenarios/safe.yaml");
    let overload_scenario_path = fixture_root.join("scenarios/overload.yaml");

    let safe_out = base.join("safe_out");
    let overload_out = base.join("overload_out");
    let overload_out_again = base.join("overload_out_again");

    let safe_report =
        run_virtual_board_and_timing_report(&plc_path, &safe_scenario_path, &safe_out);
    let overload_report =
        run_virtual_board_and_timing_report(&plc_path, &overload_scenario_path, &overload_out);
    let overload_report_again = run_virtual_board_and_timing_report(
        &plc_path,
        &overload_scenario_path,
        &overload_out_again,
    );

    let safe_p99 = safe_report["exec_us_p99"].as_u64().expect("safe p99");
    let overload_p99 = overload_report["exec_us_p99"]
        .as_u64()
        .expect("overload p99");
    let overload_p99_again = overload_report_again["exec_us_p99"]
        .as_u64()
        .expect("overload p99 repeat");

    assert!(
        overload_p99 > safe_p99,
        "overload p99 ({overload_p99}) should be greater than safe p99 ({safe_p99})"
    );
    assert_eq!(
        overload_p99, overload_p99_again,
        "same seed overload scenario should produce deterministic p99"
    );
    assert_eq!(
        overload_report["overrun_count"], overload_report_again["overrun_count"],
        "same seed overload scenario should produce deterministic overrun count"
    );

    let threshold = safe_p99.saturating_add(10);

    let safe_gate = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("no-board-gate")
        .arg(&plc_path)
        .arg("--scenario")
        .arg(&safe_scenario_path)
        .arg("--out-dir")
        .arg(base.join("safe_gate"))
        .arg("--max-p99-exec-us")
        .arg(threshold.to_string())
        .arg("--max-overrun-count")
        .arg("0")
        .output()
        .expect("run safe no-board-gate");
    assert!(
        safe_gate.status.success(),
        "safe scenario should pass realtime gate, stderr: {}",
        String::from_utf8_lossy(&safe_gate.stderr)
    );

    let overload_gate = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("no-board-gate")
        .arg(&plc_path)
        .arg("--scenario")
        .arg(&overload_scenario_path)
        .arg("--out-dir")
        .arg(base.join("overload_gate"))
        .arg("--max-p99-exec-us")
        .arg(threshold.to_string())
        .arg("--max-overrun-count")
        .arg("0")
        .output()
        .expect("run overload no-board-gate");
    assert!(
        !overload_gate.status.success(),
        "overload scenario should fail realtime gate"
    );
    assert!(
        String::from_utf8_lossy(&overload_gate.stderr).contains("realtime threshold exceeded"),
        "overload gate stderr should mention realtime threshold"
    );
}
