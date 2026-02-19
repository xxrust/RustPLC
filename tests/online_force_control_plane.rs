use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_path(p: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(p)
}

fn temp_dir(prefix: &str) -> PathBuf {
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
fn sim_plc_online_force_requires_explicit_dev_enable_flag() {
    let base = temp_dir("rust_plc_online_force_guard");
    let scenario_path = base.join("scenario.yaml");
    let script_path = base.join("online_force.jsonl");
    let trace_out = base.join("trace.jsonl");

    fs::write(
        &scenario_path,
        "tick_ms: 10\nduration_ms: 60\ninputs: []\nforces: []\n",
    )
    .expect("write scenario yaml");
    fs::write(
        &script_path,
        r#"{"at_ms":0,"actor":"tester","source":"it","channel":"DI0","value":true}"#,
    )
    .expect("write script");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("sim-plc")
        .arg(repo_path("examples/force_override_demo.plc"))
        .arg("--scenario")
        .arg(&scenario_path)
        .arg("--out")
        .arg(&trace_out)
        .arg("--online-force-script")
        .arg(&script_path)
        .output()
        .expect("run sim-plc");

    assert!(
        !output.status.success(),
        "sim-plc should reject online force without explicit enable flag"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--enable-online-force-dev"),
        "expected guard message in stderr, got: {stderr}"
    );
}

#[test]
fn sim_plc_online_force_writes_audit_jsonl_for_set_and_clear_operations() {
    let base = temp_dir("rust_plc_online_force_audit");
    let scenario_path = base.join("scenario.yaml");
    let script_path = base.join("online_force.jsonl");
    let trace_out = base.join("trace.jsonl");
    let audit_out = base.join("online_force_audit.jsonl");

    fs::write(
        &scenario_path,
        "tick_ms: 10\nduration_ms: 80\ninputs: []\nforces: []\n",
    )
    .expect("write scenario yaml");
    fs::write(
        &script_path,
        concat!(
            "{\"at_ms\":0,\"actor\":\"commissioning\",\"source\":\"panel\",\"channel\":\"DI0\",\"value\":true}\n",
            "{\"at_ms\":20,\"actor\":\"commissioning\",\"source\":\"panel\",\"channel\":\"DI0\",\"value\":null}\n",
            "{\"at_ms\":30,\"actor\":\"commissioning\",\"source\":\"panel\",\"channel\":\"AO0\",\"value\":1.25}\n",
            "{\"at_ms\":40,\"actor\":\"commissioning\",\"source\":\"panel\",\"channel\":\"AO0\",\"value\":null}\n",
        ),
    )
    .expect("write script");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("sim-plc")
        .arg(repo_path("examples/force_override_demo.plc"))
        .arg("--scenario")
        .arg(&scenario_path)
        .arg("--out")
        .arg(&trace_out)
        .arg("--enable-online-force-dev")
        .arg("--online-force-script")
        .arg(&script_path)
        .arg("--online-force-audit-out")
        .arg(&audit_out)
        .output()
        .expect("run sim-plc");

    assert!(
        output.status.success(),
        "sim-plc should accept online-force script in dev mode, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(trace_out.exists(), "trace should be generated");
    assert!(audit_out.exists(), "audit should be generated");

    let lines = fs::read_to_string(&audit_out).expect("read audit jsonl");
    let entries = lines
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("valid json"))
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 4, "expected 4 audit entries");

    assert_eq!(entries[0]["operation"], "set");
    assert_eq!(entries[0]["channel"], "di0");
    assert_eq!(entries[0]["from"], serde_json::Value::Null);
    assert_eq!(entries[0]["to"], serde_json::Value::Bool(true));

    assert_eq!(entries[1]["operation"], "clear");
    assert_eq!(entries[1]["channel"], "di0");
    assert_eq!(entries[1]["from"], serde_json::Value::Bool(true));
    assert_eq!(entries[1]["to"], serde_json::Value::Null);

    assert_eq!(entries[2]["operation"], "set");
    assert_eq!(entries[2]["channel"], "ao0");
    let to_ao = entries[2]["to"].as_f64().expect("ao set should be numeric");
    assert!((to_ao - 1.25).abs() < 1e-6);

    assert_eq!(entries[3]["operation"], "clear");
    assert_eq!(entries[3]["channel"], "ao0");
    let from_ao = entries[3]["from"]
        .as_f64()
        .expect("ao clear should keep previous value");
    assert!((from_ao - 1.25).abs() < 1e-6);
    assert_eq!(entries[3]["to"], serde_json::Value::Null);
}
