use serde_json::Value;
use std::process::Command;

#[test]
fn extern_perf_bench_emits_expected_json_metrics() {
    let output = Command::new(env!("CARGO_BIN_EXE_extern_perf_bench"))
        .arg("--output")
        .arg("json")
        .arg("--samples")
        .arg("2")
        .arg("--warmups")
        .arg("0")
        .arg("--simple-iterations")
        .arg("100")
        .arg("--complex-iterations")
        .arg("20")
        .output()
        .expect("run extern_perf_bench");

    assert!(
        output.status.success(),
        "extern_perf_bench should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse bench JSON output");
    assert_eq!(
        payload.get("schema_version").and_then(Value::as_u64),
        Some(1)
    );

    let metrics = payload
        .get("metrics_us_per_call")
        .and_then(Value::as_object)
        .expect("metrics_us_per_call object");

    for key in ["simple_add", "complex_quadratic_fit"] {
        let metric = metrics
            .get(key)
            .and_then(Value::as_object)
            .expect("metric object");
        let p95 = metric
            .get("p95_us")
            .and_then(Value::as_f64)
            .expect("p95_us number");
        assert!(p95 >= 0.0, "p95 should be non-negative");
    }
}
