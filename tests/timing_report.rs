use std::fs;
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

#[test]
fn timing_report_generates_expected_stats_from_tick_timing_jsonl() {
    let base = temp_dir("rust_plc_timing_report");
    let in_path = base.join("tick_timing.jsonl");
    let out_path = base.join("timing_report.json");

    let samples = vec![90_u64, 10, 60, 20, 100, 70, 30, 80, 40, 50];
    let mut jsonl = String::new();
    for (tick, exec_us) in samples.into_iter().enumerate() {
        let row = rust_plc::tick_timing::TickTimingSample {
            tick: tick as u64,
            ts_start_us: (tick as u64) * 1_000,
            ts_end_us: (tick as u64) * 1_000 + exec_us,
            exec_us,
            slack_us: 1_000_u64.saturating_sub(exec_us),
            overrun: exec_us >= 95,
        };
        jsonl.push_str(&serde_json::to_string(&row).expect("serialize row"));
        jsonl.push('\n');
    }
    fs::write(&in_path, jsonl).expect("write tick_timing.jsonl");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("timing-report")
        .arg("--in")
        .arg(&in_path)
        .arg("--out")
        .arg(&out_path)
        .output()
        .expect("run timing-report");

    assert!(
        output.status.success(),
        "timing-report should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(out_path.exists(), "timing_report.json should exist");

    let v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&out_path).expect("read report"))
            .expect("valid json");

    assert_eq!(v.get("schema_version").and_then(|x| x.as_u64()), Some(1));
    assert_eq!(v.get("count").and_then(|x| x.as_u64()), Some(10));
    assert_eq!(v.get("overrun_count").and_then(|x| x.as_u64()), Some(1));
    assert_eq!(v.get("exec_us_min").and_then(|x| x.as_u64()), Some(10));
    assert_eq!(v.get("exec_us_max").and_then(|x| x.as_u64()), Some(100));
    assert_eq!(v.get("exec_us_p50").and_then(|x| x.as_u64()), Some(50));
    assert_eq!(v.get("exec_us_p95").and_then(|x| x.as_u64()), Some(100));
    assert_eq!(v.get("exec_us_p99").and_then(|x| x.as_u64()), Some(100));
    assert_eq!(v.get("exec_us_mean").and_then(|x| x.as_f64()), Some(55.0));
}

