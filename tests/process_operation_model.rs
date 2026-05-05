use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

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

fn write_fixture_plc(path: &PathBuf) {
    let plc = r#"
[topology]

workpiece part: workpiece_type {
    normal_terminal_states: [loaded]
    ingress_sites: [storage_box.slot[0], storage_box.slot[1]]
    normal_egress_sites: [station_0, station_1]
}

carrier storage_box: workpiece_carrier { slots: 2 }
location pickup: workpiece_location { capacity: 1 }
location station_0: workpiece_location { capacity: 1 }
location station_1: workpiece_location { capacity: 1 }
holder arm: workpiece_holder { capacity: 1 }

[constraints]

[tasks]

task feed:
    step feed_first:
        effect: transfer from storage_box.slot[0] to pickup
    step pick_first:
        effect: acquire holder arm from pickup
    step place_first:
        effect: transfer from arm to station_0
    step finish_first:
        effect: finish workpiece at station_0 as loaded
    step feed_second:
        effect: transfer from storage_box.slot[1] to pickup
    step pick_second:
        effect: acquire holder arm from pickup
    step place_second:
        effect: transfer from arm to station_1
    step finish_second:
        effect: finish workpiece at station_1 as loaded
    on_complete: goto sink.idle

task sink:
    step idle:
        action: log "done"
"#;
    fs::write(path, plc).expect("write fixture plc");
}

fn write_refined_fixture_plc(path: &PathBuf) {
    let plc = r#"
[topology]

workpiece part: workpiece_type {
    normal_terminal_states: [loaded]
    ingress_sites: [storage_box]
    normal_egress_sites: [station]
}

location storage_box: workpiece_location { capacity: 10 }
location station: workpiece_location { capacity: 1 }
holder arm: workpiece_holder { capacity: 1 }

[constraints]

[tasks]

task feed:
    step pick:
        effect: acquire holder arm from storage_box
    step place:
        effect: transfer from arm to station
    step finish:
        effect: finish workpiece at station as loaded
"#;
    fs::write(path, plc).expect("write refined fixture plc");
}

fn write_guarded_independent_fixture_plc(path: &PathBuf) {
    let plc = r#"
[topology]

workpiece part: workpiece_type {
    normal_terminal_states: [done]
    ingress_sites: [source_a, source_b]
    normal_egress_sites: [out_a, out_b]
}

location source_a: workpiece_location { capacity: 1 }
location source_b: workpiece_location { capacity: 1 }
location out_a: workpiece_location { capacity: 1 }
location out_b: workpiece_location { capacity: 1 }

[constraints]

[tasks]

task flow:
    step move_a:
        effect: transfer from source_a to out_a
    step move_b:
        effect: transfer from source_b to out_b
        if: auto_enabled == true goto done.halt else: goto done.halt

task done:
    step halt:
        action: log "done"
"#;
    fs::write(path, plc).expect("write guarded fixture plc");
}

#[test]
fn operation_model_cli_exports_scheduling_intent_classes() {
    let base = temp_dir("rust_plc_operation_model");
    let plc = base.join("fixture.plc");
    let out = base
        .join("process_model")
        .join("process_operation_model.toml");
    write_fixture_plc(&plc);

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("operation-model")
        .arg(&plc)
        .arg("--out")
        .arg(&out)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run operation-model");

    assert!(
        output.status.success(),
        "operation-model should pass, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let summary: Value =
        serde_json::from_slice(&output.stdout).expect("operation-model should print JSON");
    assert_eq!(
        summary.get("command").and_then(Value::as_str),
        Some("operation-model")
    );
    assert_eq!(
        summary.get("operation_count").and_then(Value::as_u64),
        Some(8)
    );

    let artifact_text = fs::read_to_string(&out).expect("read operation model");
    let artifact: toml::Value = toml::from_str(&artifact_text).expect("operation model toml");
    assert_eq!(
        artifact
            .get("schema_version")
            .and_then(toml::Value::as_integer),
        Some(1)
    );
    assert_eq!(
        artifact.get("policy").and_then(toml::Value::as_str),
        Some("opportunistic_admission")
    );

    let classes = artifact
        .get("operation_classes")
        .and_then(toml::Value::as_array)
        .expect("operation classes");
    let feed_class = classes
        .iter()
        .find(|class| {
            class.get("key").and_then(toml::Value::as_str)
                == Some("move:Transfer:storage_box.slot[*]->pickup")
        })
        .expect("slot-indexed feeds should normalize into one operation class");
    assert_eq!(
        feed_class
            .get("operation_ids")
            .and_then(toml::Value::as_array)
            .map(Vec::len),
        Some(2)
    );

    let operations = artifact
        .get("operations")
        .and_then(toml::Value::as_array)
        .expect("operations");
    let pick = operations
        .iter()
        .find(|operation| {
            operation.get("step_name").and_then(toml::Value::as_str) == Some("pick_first")
        })
        .expect("pick operation");
    assert!(
        pick.get("admissions")
            .and_then(toml::Value::as_array)
            .expect("pick admissions")
            .iter()
            .any(|rule| {
                rule.get("kind").and_then(toml::Value::as_str) == Some("source_available")
                    && rule.get("endpoint").and_then(toml::Value::as_str) == Some("pickup")
            })
    );
}

#[test]
fn process_model_check_passes_when_task_flow_refines_authored_model() {
    let base = temp_dir("rust_plc_process_model_check_pass");
    let plc = base.join("fixture.plc");
    let model = base
        .join("process_model")
        .join("process_operation_model.toml");
    write_refined_fixture_plc(&plc);

    let export = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("operation-model")
        .arg(&plc)
        .arg("--out")
        .arg(&model)
        .arg("--output")
        .arg("json")
        .output()
        .expect("export operation model");
    assert!(
        export.status.success(),
        "operation-model should pass, stderr: {}",
        String::from_utf8_lossy(&export.stderr)
    );

    let check = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("process-model-check")
        .arg(&plc)
        .arg("--model")
        .arg(&model)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run process-model-check");
    assert!(
        check.status.success(),
        "process-model-check should pass, stderr: {}",
        String::from_utf8_lossy(&check.stderr)
    );
    let report: Value =
        serde_json::from_slice(&check.stdout).expect("process-model-check should print JSON");
    assert_eq!(
        report.get("command").and_then(Value::as_str),
        Some("process-model-check")
    );
    assert_eq!(report.get("status").and_then(Value::as_str), Some("pass"));
    assert_eq!(report.get("issue_count").and_then(Value::as_u64), Some(0));
}

#[test]
fn process_model_check_matches_by_semantic_contract_not_task_step_name() {
    let base = temp_dir("rust_plc_process_model_check_semantic_key");
    let plc = base.join("fixture.plc");
    let model = base
        .join("process_model")
        .join("process_operation_model.toml");
    write_refined_fixture_plc(&plc);

    let export = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("operation-model")
        .arg(&plc)
        .arg("--out")
        .arg(&model)
        .arg("--output")
        .arg("json")
        .output()
        .expect("export operation model");
    assert!(
        export.status.success(),
        "operation-model should pass, stderr: {}",
        String::from_utf8_lossy(&export.stderr)
    );

    let authored = fs::read_to_string(&model)
        .expect("read generated model")
        .replace("task_name = \"feed\"", "task_name = \"authored_flow\"")
        .replace("step_name = \"pick\"", "step_name = \"authored_pick\"");
    fs::write(&model, authored).expect("write authored semantic-key model");

    let check = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("process-model-check")
        .arg(&plc)
        .arg("--model")
        .arg(&model)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run process-model-check");
    assert!(
        check.status.success(),
        "process-model-check should not depend on task/step binding when contract_key matches, stderr: {}",
        String::from_utf8_lossy(&check.stderr)
    );
}

#[test]
fn process_model_check_fails_unjustified_same_task_serialization() {
    let base = temp_dir("rust_plc_process_model_check_serial");
    let plc = base.join("fixture.plc");
    let model = base
        .join("process_model")
        .join("process_operation_model.toml");
    write_fixture_plc(&plc);

    let export = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("operation-model")
        .arg(&plc)
        .arg("--out")
        .arg(&model)
        .arg("--output")
        .arg("json")
        .output()
        .expect("export operation model");
    assert!(
        export.status.success(),
        "operation-model should pass, stderr: {}",
        String::from_utf8_lossy(&export.stderr)
    );

    let check = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("process-model-check")
        .arg(&plc)
        .arg("--model")
        .arg(&model)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run process-model-check");
    assert!(
        !check.status.success(),
        "process-model-check should fail for unjustified serialization"
    );
    let report: Value =
        serde_json::from_slice(&check.stdout).expect("process-model-check should print JSON");
    assert_eq!(report.get("status").and_then(Value::as_str), Some("fail"));
    let issues = report
        .get("issues")
        .and_then(Value::as_array)
        .expect("issues array");
    assert!(
        issues
            .iter()
            .any(|issue| issue.get("code").and_then(Value::as_str) == Some("OP-002")),
        "expected OP-002 unjustified serialization issue, got {issues:?}"
    );
}

#[test]
fn process_model_check_does_not_treat_generic_guard_as_predecessor_proof() {
    let base = temp_dir("rust_plc_process_model_check_guard");
    let plc = base.join("fixture.plc");
    let model = base
        .join("process_model")
        .join("process_operation_model.toml");
    write_guarded_independent_fixture_plc(&plc);

    let export = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("operation-model")
        .arg(&plc)
        .arg("--out")
        .arg(&model)
        .arg("--output")
        .arg("json")
        .output()
        .expect("export operation model");
    assert!(
        export.status.success(),
        "operation-model should pass, stderr: {}",
        String::from_utf8_lossy(&export.stderr)
    );

    let check = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("process-model-check")
        .arg(&plc)
        .arg("--model")
        .arg(&model)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run process-model-check");
    assert!(
        !check.status.success(),
        "generic condition guard must not suppress OP-002"
    );
    let report: Value =
        serde_json::from_slice(&check.stdout).expect("process-model-check should print JSON");
    let issues = report
        .get("issues")
        .and_then(Value::as_array)
        .expect("issues array");
    assert!(
        issues
            .iter()
            .any(|issue| issue.get("code").and_then(Value::as_str) == Some("OP-002")),
        "expected OP-002 for guarded independent serialization, got {issues:?}"
    );
}
