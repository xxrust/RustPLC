use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "{prefix}_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn repo_path(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn read_json(path: &Path) -> Value {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|err| panic!("failed to parse {} as JSON: {err}", path.display()))
}

#[test]
fn commissioning_run_executes_nominal_and_fault_rehearsals_and_emits_index() {
    let base = temp_dir("rust_plc_commissioning_run");
    let out_dir = base.join("out");
    let plc = repo_path("examples/force_override_demo.plc");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("commissioning-run")
        .arg(&plc)
        .arg("--out-dir")
        .arg(&out_dir)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run commissioning-run");

    assert!(
        output.status.success(),
        "commissioning-run should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout_json: Value = serde_json::from_slice(&output.stdout)
        .expect("commissioning-run --output json should emit valid JSON");
    assert_eq!(
        stdout_json.get("status").and_then(Value::as_str),
        Some("pass")
    );

    let index_path = out_dir.join("commissioning_index.json");
    assert!(index_path.exists(), "commissioning_index.json should exist");
    let index_json = read_json(&index_path);

    assert_eq!(
        index_json.get("command").and_then(Value::as_str),
        Some("commissioning-run")
    );
    assert_eq!(
        index_json.get("status").and_then(Value::as_str),
        Some("pass")
    );

    let steps = index_json
        .get("steps")
        .and_then(Value::as_array)
        .expect("steps should be array");
    assert_eq!(steps.len(), 10, "expected 10 commissioning steps");
    for step in steps {
        assert_eq!(step.get("status").and_then(Value::as_str), Some("pass"));
    }

    for required in [
        out_dir.join("nominal.yaml"),
        out_dir.join("doctor_nominal.json"),
        out_dir.join("retain.toml"),
        out_dir.join("nominal_trace.jsonl"),
        out_dir.join("gate_nominal/diff_report.json"),
        out_dir.join("fault.yaml"),
        out_dir.join("doctor_fault.json"),
        out_dir.join("online_force_audit.jsonl"),
        out_dir.join("online_var_audit.jsonl"),
        out_dir.join("gate_fault/diff_report.json"),
        out_dir.join("commissioning_index.json"),
    ] {
        assert!(required.exists(), "missing artifact {}", required.display());
    }
}
