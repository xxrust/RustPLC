use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_path(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn read_json(path: &Path) -> Value {
    let body = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    serde_json::from_str(&body)
        .unwrap_or_else(|err| panic!("failed to parse JSON {}: {err}", path.display()))
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

fn has_python3() -> bool {
    Command::new("python3")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

#[test]
fn abnormal_exit_matrix_declares_complete_abcd_contract() {
    let matrix_path = repo_path("scenarios/rp2040_hil_gate/abnormal_exit/matrix.json");
    let matrix = read_json(&matrix_path);

    let classes = matrix
        .get("classes")
        .and_then(Value::as_array)
        .expect("matrix.classes must be an array");
    assert_eq!(classes.len(), 4, "matrix must define A/B/C/D classes");

    let mut by_id: BTreeMap<String, &Value> = BTreeMap::new();
    for class in classes {
        let id = class
            .get("id")
            .and_then(Value::as_str)
            .expect("class.id must be string");
        by_id.insert(id.to_string(), class);

        let trigger_method = class
            .get("trigger_method")
            .and_then(Value::as_str)
            .expect("trigger_method must be string");
        assert!(
            !trigger_method.trim().is_empty(),
            "trigger_method cannot be empty"
        );

        let expected_io_behavior = class
            .get("expected_io_behavior")
            .and_then(Value::as_str)
            .expect("expected_io_behavior must be string");
        assert!(
            !expected_io_behavior.trim().is_empty(),
            "expected_io_behavior cannot be empty"
        );

        let checks = class
            .get("acceptance_checks")
            .and_then(Value::as_array)
            .expect("acceptance_checks must be array");
        assert!(!checks.is_empty(), "acceptance_checks cannot be empty");
    }

    let ids: BTreeSet<_> = by_id.keys().cloned().collect();
    let expected_ids: BTreeSet<_> = ["A", "B", "C", "D"].iter().map(|v| v.to_string()).collect();
    assert_eq!(ids, expected_ids, "matrix ids must be exactly A/B/C/D");

    let class_d = by_id.get("D").expect("class D must exist");
    assert_eq!(
        class_d.get("automation").and_then(Value::as_str),
        Some("hardware_only"),
        "class D must be marked hardware_only"
    );
    let checklist = class_d
        .get("electrical_checklist_notes")
        .and_then(Value::as_array)
        .expect("class D should carry electrical checklist notes");
    assert!(
        checklist.len() >= 2,
        "class D should include at least two electrical checklist notes"
    );
}

#[test]
fn abnormal_exit_evidence_files_publish_required_fields() {
    for class_id in ["A", "B", "C", "D"] {
        let path = repo_path(&format!(
            "scenarios/rp2040_hil_gate/abnormal_exit/evidence/{class_id}.json"
        ));
        let evidence = read_json(&path);

        assert_eq!(
            evidence.get("class").and_then(Value::as_str),
            Some(class_id),
            "evidence.class mismatch for {class_id}"
        );
        assert!(
            evidence.get("trigger").and_then(Value::as_object).is_some(),
            "trigger object missing for {class_id}"
        );
        assert!(
            evidence
                .get("observed_outputs")
                .and_then(Value::as_array)
                .is_some(),
            "observed_outputs missing for {class_id}"
        );
        assert!(
            evidence.get("verdict").and_then(Value::as_str).is_some(),
            "verdict missing for {class_id}"
        );

        let artifacts = evidence
            .get("artifacts")
            .and_then(Value::as_object)
            .expect("artifacts must be object");
        for key in ["trigger_log", "output_log"] {
            let value = artifacts
                .get(key)
                .and_then(Value::as_str)
                .expect("artifact fields must be strings");
            assert!(
                !value.trim().is_empty(),
                "{class_id} artifacts.{key} should not be empty"
            );
        }
    }
}

#[test]
fn abnormal_exit_verifier_passes_for_abc_and_marks_d_manual() {
    if !has_python3() {
        eprintln!("python3 not available; skipping abnormal_exit_verifier test");
        return;
    }

    let out_dir = temp_dir("rust_plc_abnormal_exit_verify");
    let report_path = out_dir.join("report.json");
    let matrix_path = repo_path("scenarios/rp2040_hil_gate/abnormal_exit/matrix.json");
    let evidence_dir = repo_path("scenarios/rp2040_hil_gate/abnormal_exit/evidence");
    let script_path = repo_path("scripts/abnormal_exit_matrix_verify.py");

    let output = Command::new("python3")
        .arg(&script_path)
        .arg("--matrix")
        .arg(&matrix_path)
        .arg("--evidence-dir")
        .arg(&evidence_dir)
        .arg("--out")
        .arg(&report_path)
        .output()
        .expect("run abnormal_exit_matrix_verify.py");
    assert!(
        output.status.success(),
        "verifier should pass for default A/B/C classes, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report = read_json(&report_path);
    assert_eq!(report.get("status").and_then(Value::as_str), Some("pass"));

    let mut result_by_class: BTreeMap<String, &Value> = BTreeMap::new();
    for row in report
        .get("results")
        .and_then(Value::as_array)
        .expect("results should be array")
    {
        let class_id = row
            .get("class")
            .and_then(Value::as_str)
            .expect("result.class should be string");
        result_by_class.insert(class_id.to_string(), row);
    }

    for class_id in ["A", "B", "C"] {
        let row = result_by_class
            .get(class_id)
            .unwrap_or_else(|| panic!("missing result row for class {class_id}"));
        assert_eq!(
            row.get("status").and_then(Value::as_str),
            Some("pass"),
            "class {class_id} should pass automated verification"
        );
    }

    let class_d = result_by_class
        .get("D")
        .expect("class D should be present in report");
    assert_eq!(
        class_d.get("status").and_then(Value::as_str),
        Some("manual_hardware_chain")
    );
}

#[test]
fn abnormal_exit_verifier_fails_when_hardware_only_class_is_required() {
    if !has_python3() {
        eprintln!("python3 not available; skipping abnormal_exit_verifier test");
        return;
    }

    let out_dir = temp_dir("rust_plc_abnormal_exit_verify_required_d");
    let report_path = out_dir.join("report.json");
    let matrix_path = repo_path("scenarios/rp2040_hil_gate/abnormal_exit/matrix.json");
    let evidence_dir = repo_path("scenarios/rp2040_hil_gate/abnormal_exit/evidence");
    let script_path = repo_path("scripts/abnormal_exit_matrix_verify.py");

    let output = Command::new("python3")
        .arg(&script_path)
        .arg("--matrix")
        .arg(&matrix_path)
        .arg("--evidence-dir")
        .arg(&evidence_dir)
        .arg("--out")
        .arg(&report_path)
        .arg("--require-classes")
        .arg("A,B,C,D")
        .output()
        .expect("run abnormal_exit_matrix_verify.py requiring D");

    assert_eq!(
        output.status.code(),
        Some(2),
        "requiring class D should fail with code 2, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report = read_json(&report_path);
    assert_eq!(report.get("status").and_then(Value::as_str), Some("fail"));

    let results = report
        .get("results")
        .and_then(Value::as_array)
        .expect("results should be array");
    let class_d = results
        .iter()
        .find(|row| row.get("class").and_then(Value::as_str) == Some("D"))
        .expect("result row for class D should exist");
    assert_eq!(
        class_d.get("status").and_then(Value::as_str),
        Some("manual_hardware_chain")
    );
    let errors = class_d
        .get("errors")
        .and_then(Value::as_array)
        .expect("class D errors should be array");
    assert!(
        errors.iter().any(|err| {
            err.as_str()
                .map(|text| text.contains("cannot be auto-verified"))
                .unwrap_or(false)
        }),
        "class D should explain why it cannot be auto-verified"
    );
}
