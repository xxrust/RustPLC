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
fn sim_plc_online_variable_requires_explicit_dev_enable_flag() {
    let base = temp_dir("rust_plc_online_var_guard");
    let scenario_path = base.join("scenario.yaml");
    let script_path = base.join("online_var.jsonl");
    let trace_out = base.join("trace.jsonl");

    fs::write(
        &scenario_path,
        "tick_ms: 10\nduration_ms: 60\ninputs: []\nforces: []\n",
    )
    .expect("write scenario yaml");
    fs::write(
        &script_path,
        r#"{"at_ms":0,"actor":"tester","source":"it","variable":"BOOL:diag_latch","value":true}"#,
    )
    .expect("write script");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("sim-plc")
        .arg(repo_path("examples/force_override_demo.plc"))
        .arg("--scenario")
        .arg(&scenario_path)
        .arg("--out")
        .arg(&trace_out)
        .arg("--online-var-script")
        .arg(&script_path)
        .output()
        .expect("run sim-plc");

    assert!(
        !output.status.success(),
        "sim-plc should reject online variable script without explicit enable flag"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--enable-online-force-dev"),
        "expected guard message in stderr, got: {stderr}"
    );
}

#[test]
fn sim_plc_online_variable_writes_bool_and_real_audit_entries() {
    let base = temp_dir("rust_plc_online_var_audit");
    let scenario_path = base.join("scenario.yaml");
    let script_path = base.join("online_var.jsonl");
    let trace_out = base.join("trace.jsonl");
    let audit_out = base.join("online_var_audit.jsonl");

    fs::write(
        &scenario_path,
        "tick_ms: 10\nduration_ms: 100\ninputs: []\nforces: []\n",
    )
    .expect("write scenario yaml");
    fs::write(
        &script_path,
        concat!(
            "{\"at_ms\":0,\"actor\":\"commissioning\",\"source\":\"panel\",\"variable\":\"BOOL:diag_latch\",\"value\":true}\n",
            "{\"at_ms\":20,\"actor\":\"commissioning\",\"source\":\"panel\",\"variable\":\"REAL:gain_k\",\"value\":1.25}\n",
            "{\"at_ms\":30,\"actor\":\"commissioning\",\"source\":\"panel\",\"variable\":\"BOOL:diag_latch\",\"value\":null}\n",
            "{\"at_ms\":40,\"actor\":\"commissioning\",\"source\":\"panel\",\"variable\":\"REAL:gain_k\",\"value\":null}\n",
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
        .arg("--online-var-script")
        .arg(&script_path)
        .arg("--online-var-audit-out")
        .arg(&audit_out)
        .output()
        .expect("run sim-plc");

    assert!(
        output.status.success(),
        "sim-plc should accept online-variable script in dev mode, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(trace_out.exists(), "trace should be generated");
    assert!(audit_out.exists(), "variable audit should be generated");

    let first_lines = fs::read_to_string(&audit_out).expect("read variable audit jsonl");
    let entries = first_lines
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("valid json"))
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 4, "expected 4 variable audit entries");

    assert_eq!(entries[0]["operation"], "set");
    assert_eq!(entries[0]["variable"], "bool:diag_latch");
    assert_eq!(entries[0]["from"], serde_json::Value::Null);
    assert_eq!(entries[0]["to"], serde_json::Value::Bool(true));

    assert_eq!(entries[1]["operation"], "set");
    assert_eq!(entries[1]["variable"], "real:gain_k");
    let to_real = entries[1]["to"]
        .as_f64()
        .expect("REAL set should be numeric");
    assert!((to_real - 1.25).abs() < 1e-6);

    assert_eq!(entries[2]["operation"], "clear");
    assert_eq!(entries[2]["variable"], "bool:diag_latch");
    assert_eq!(entries[2]["from"], serde_json::Value::Bool(true));
    assert_eq!(entries[2]["to"], serde_json::Value::Null);

    assert_eq!(entries[3]["operation"], "clear");
    assert_eq!(entries[3]["variable"], "real:gain_k");
    let from_real = entries[3]["from"]
        .as_f64()
        .expect("REAL clear should keep previous value");
    assert!((from_real - 1.25).abs() < 1e-6);
    assert_eq!(entries[3]["to"], serde_json::Value::Null);

    let replay_out = base.join("trace_replay.jsonl");
    let replay_audit = base.join("online_var_audit_replay.jsonl");
    let replay = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("sim-plc")
        .arg(repo_path("examples/force_override_demo.plc"))
        .arg("--scenario")
        .arg(&scenario_path)
        .arg("--out")
        .arg(&replay_out)
        .arg("--enable-online-force-dev")
        .arg("--online-var-script")
        .arg(&script_path)
        .arg("--online-var-audit-out")
        .arg(&replay_audit)
        .output()
        .expect("run sim-plc replay");
    assert!(
        replay.status.success(),
        "replay run should succeed, stderr: {}",
        String::from_utf8_lossy(&replay.stderr)
    );

    let replay_lines = fs::read_to_string(&replay_audit).expect("read replay audit");
    assert_eq!(
        first_lines, replay_lines,
        "same script + tick alignment should replay deterministically"
    );
}

#[test]
fn sim_plc_online_variable_rejects_tick_misaligned_script() {
    let base = temp_dir("rust_plc_online_var_tick_align");
    let scenario_path = base.join("scenario.yaml");
    let script_path = base.join("online_var.jsonl");
    let trace_out = base.join("trace.jsonl");

    fs::write(
        &scenario_path,
        "tick_ms: 10\nduration_ms: 80\ninputs: []\nforces: []\n",
    )
    .expect("write scenario yaml");
    fs::write(
        &script_path,
        r#"{"at_ms":15,"actor":"commissioning","source":"panel","variable":"REAL:gain_k","value":0.5}"#,
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
        .arg("--online-var-script")
        .arg(&script_path)
        .output()
        .expect("run sim-plc");

    assert!(
        !output.status.success(),
        "misaligned variable command should be rejected"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not aligned to tick_ms"),
        "expected tick alignment error, got: {stderr}"
    );
}
