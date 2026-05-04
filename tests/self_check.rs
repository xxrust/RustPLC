use rust_plc::intent_alignment::{
    IntentAlignmentReport, IntentAlignmentVerdict, reduce_intent_alignment_report,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
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
device plc_main: plc {
    purpose: "project-check fixture controller",
    model_ref: rp2040_softplc
}

[constraints]

[tasks]
task main:
    step wait_start:
        wait: X0 == true
        timeout: 20ms -> goto done
    step run:
        action: set Y0 on

task done:
    step halt:
        action: log "done"
"#;
    fs::write(path, plc).expect("write fixture plc");
}

fn write_intent_alignment_plc(path: &PathBuf) {
    let plc = r#"
[topology]
device plc_main: plc {
    purpose: "project-check intent fixture controller"
    model_ref: rp2040_softplc
}

[constraints]

[tasks]
task main:
    step wait_start:
        wait: X0 == true
        allow_indefinite_wait: true
    step run_delay:
        delay: 10ms
    step finish:
        action: log "done"
"#;
    fs::write(path, plc).expect("write intent fixture plc");
}

fn write_intent_alignment_fixture(contract_path: &PathBuf) {
    let source_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("docs/architecture/intent_alignment_verification.md");
    let source_text =
        fs::read_to_string(&source_path).expect("read intent alignment architecture doc");
    let normalized_source = source_text.replace("\r\n", "\n").replace('\r', "\n");
    let digest = hex::encode(Sha256::digest(
        normalized_source.as_bytes(),
    ));
    let contract = format!(
        r#"
{{
  "contract_version": "phase-2.v1",
  "source_ref": {{
    "kind": "architecture_doc",
    "path": "docs/architecture/intent_alignment_verification.md",
    "description": "fixture"
  }},
  "source_digest": {{
    "algorithm": "sha256",
    "value": "{digest}"
  }},
  "metadata": {{
    "contract_id": "project-check-intent-fixture",
    "title": "Project-check intent fixture",
    "business_owner": "test-owner",
    "authoritative_intent_source": {{
      "kind": "architecture_doc",
      "path": "docs/architecture/intent_alignment_verification.md",
      "description": "fixture"
    }},
    "review_basis": [
      {{
        "label": "Fixture review",
        "source": {{
          "kind": "architecture_doc",
          "path": "docs/architecture/intent_alignment_verification.md",
          "description": "fixture"
        }}
      }}
    ]
  }},
  "contract_core": {{
    "expected_milestones": [
      {{
        "milestone_id": "entered_run_delay",
        "business_milestone": {{ "label": "Entered run_delay", "description": "runtime moved from wait_start to run_delay" }}
      }},
      {{
        "milestone_id": "cycle_restartable",
        "business_milestone": {{ "label": "Reached finish", "description": "runtime moved from run_delay to finish" }}
      }}
    ],
    "required_edges": [
      {{ "predecessor": "entered_run_delay", "successor": "cycle_restartable" }}
    ],
    "postconditions": [],
    "cycle_semantics": {{
      "cycle_start_milestone": "entered_run_delay",
      "cycle_complete_milestone": "cycle_restartable",
      "restart_semantics": {{
        "restartable_milestone": "cycle_restartable",
        "next_cycle_start_milestone": "entered_run_delay",
        "required_postconditions": []
      }}
    }}
  }},
  "observation_bindings": [
    {{
      "binding_id": "entered_run_delay",
      "subject": {{ "kind": "milestone", "milestone_id": "entered_run_delay" }},
      "combination": "all_of",
      "evidence": [{{ "source": "trace_event", "key": "transition", "expected": "task=0;from=0;to=1;reason=wait_satisfied" }}]
    }},
    {{
      "binding_id": "cycle_restartable",
      "subject": {{ "kind": "milestone", "milestone_id": "cycle_restartable" }},
      "combination": "all_of",
      "evidence": [{{ "source": "trace_event", "key": "transition", "expected": "task=0;from=1;to=2;reason=delay_elapsed" }}]
    }}
  ]
}}
"#
    );

    fs::write(contract_path, contract).expect("write fixture contract");
}

fn write_placeholder_intent_alignment_fixture(contract_path: &PathBuf) {
    let contract = r#"
{
  "contract_version": "phase-2.v1",
  "source_ref": {
    "kind": "architecture_doc",
    "path": "docs/architecture/intent_alignment_verification.md",
    "description": "fixture"
  },
  "source_digest": {
    "algorithm": "sha256",
    "value": "replace_me_after_authoring"
  },
  "metadata": {
    "contract_id": "project-check-intent-placeholder",
    "title": "Project-check placeholder intent fixture",
    "business_owner": "test-owner",
    "authoritative_intent_source": {
      "kind": "architecture_doc",
      "path": "docs/architecture/intent_alignment_verification.md",
      "description": "fixture"
    },
    "review_basis": [
      {
        "label": "Fixture review",
        "source": {
          "kind": "architecture_doc",
          "path": "docs/architecture/intent_alignment_verification.md",
          "description": "fixture"
        }
      }
    ]
  },
  "contract_core": {
    "expected_milestones": [
      {
        "milestone_id": "cycle_started",
        "business_milestone": { "label": "Cycle started", "description": "Starter milestone placeholder. Replace with a real business milestone." }
      },
      {
        "milestone_id": "cycle_completed",
        "business_milestone": { "label": "Cycle completed", "description": "Starter milestone placeholder. Replace with a real business milestone." }
      }
    ],
    "required_edges": [
      { "predecessor": "cycle_started", "successor": "cycle_completed" }
    ],
    "postconditions": [],
    "cycle_semantics": {
      "cycle_start_milestone": "cycle_started",
      "cycle_complete_milestone": "cycle_completed",
      "restart_semantics": {
        "restartable_milestone": "cycle_completed",
        "next_cycle_start_milestone": "cycle_started",
        "required_postconditions": []
      }
    }
  },
  "observation_bindings": [
    {
      "binding_id": "replace_with_real_anchor",
      "subject": { "kind": "milestone", "milestone_id": "cycle_started" },
      "combination": "all_of",
      "evidence": [{ "source": "trace_event", "key": "transition", "expected": "replace_after_intent_doctor" }]
    }
  ]
}
"#;

    fs::write(contract_path, contract).expect("write placeholder fixture contract");
}

#[test]
fn project_check_runs_real_command_chain_and_emits_aggregate_report() {
    let base = temp_dir("rust_plc_project_check");
    let plc = base.join("fixture.plc");
    let scenario = base.join("scenario.yaml");
    let out_dir = base.join("artifacts");
    write_fixture_plc(&plc);
    fs::write(
        &scenario,
        r#"
tick_ms: 10
duration_ms: 40
inputs:
  - at_ms: 10
    set:
      digital_inputs:
        0: true
"#,
    )
    .expect("write fixture scenario");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("project-check")
        .arg(&plc)
        .arg("--scenario")
        .arg(&scenario)
        .arg("--out-dir")
        .arg(&out_dir)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run project-check");

    assert!(
        output.status.success(),
        "project-check should pass, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value =
        serde_json::from_slice(&output.stdout).expect("project-check should print JSON");
    assert_eq!(
        report.get("command").and_then(Value::as_str),
        Some("project-check")
    );
    assert_eq!(report.get("status").and_then(Value::as_str), Some("pass"));

    let steps = report
        .get("steps")
        .and_then(Value::as_array)
        .expect("steps array");
    assert_eq!(
        steps.len(),
        4,
        "project-check should run four concrete checks"
    );

    for step_name in [
        "compile_verify",
        "sequence_lint",
        "scenario_doctor",
        "no_board_gate",
    ] {
        assert!(
            steps.iter().any(|step| {
                step.get("name").and_then(Value::as_str) == Some(step_name)
                    && step.get("status").and_then(Value::as_str) == Some("pass")
            }),
            "expected project-check step `{step_name}` to pass"
        );
    }

    for rel in [
        "project_check_report.json",
        "compile_verify/verification_report.json",
        "sequence_lint/stderr.log",
        "scenario_doctor/report.json",
        "no_board_gate/report.json",
        "no_board_gate/artifacts/diff_report.json",
        "no_board_gate/artifacts/timing_report.json",
    ] {
        assert!(
            out_dir.join(rel).exists(),
            "expected project-check artifact to exist: {rel}"
        );
    }
}

#[test]
fn project_check_reports_failed_steps_and_exits_non_zero() {
    let base = temp_dir("rust_plc_project_check_fail");
    let plc = base.join("fixture.plc");
    let scenario = base.join("bad_scenario.yaml");
    let out_dir = base.join("artifacts");
    write_fixture_plc(&plc);
    fs::write(
        &scenario,
        r#"
tick_ms: 10
duration_ms: 40
inputs:
  - at_ms: 0
    set:
      digital_inputs:
        999: true
"#,
    )
    .expect("write bad scenario");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("project-check")
        .arg(&plc)
        .arg("--scenario")
        .arg(&scenario)
        .arg("--out-dir")
        .arg(&out_dir)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run failing project-check");

    assert!(
        !output.status.success(),
        "project-check should fail for a bad scenario"
    );

    let report: Value =
        serde_json::from_slice(&output.stdout).expect("project-check should still print JSON");
    assert_eq!(
        report.get("command").and_then(Value::as_str),
        Some("project-check")
    );
    assert_eq!(report.get("status").and_then(Value::as_str), Some("fail"));
    assert!(
        report
            .get("failed_steps")
            .and_then(Value::as_u64)
            .unwrap_or_default()
            > 0,
        "failed_steps should be greater than zero"
    );

    let steps = report
        .get("steps")
        .and_then(Value::as_array)
        .expect("steps array");
    assert!(
        steps.iter().any(|step| {
            step.get("name").and_then(Value::as_str) == Some("scenario_doctor")
                && step.get("status").and_then(Value::as_str) == Some("fail")
        }),
        "scenario_doctor should be marked as failed"
    );

    assert!(
        out_dir.join("project_check_report.json").exists(),
        "project-check should still emit the aggregate report on failure"
    );
    assert!(
        out_dir.join("scenario_doctor/stderr.log").exists(),
        "failed step stderr log should be preserved"
    );
}

#[test]
fn project_check_runs_intent_alignment_step_from_sidecar_and_gate_evidence() {
    let base = temp_dir("rust_plc_project_check_intent");
    let plc = base.join("fixture.plc");
    let scenario = base.join("scenario.yaml");
    let out_dir = base.join("artifacts");
    let contract = base.join("fixture.intent_alignment.contract.json");
    write_intent_alignment_plc(&plc);
    write_intent_alignment_fixture(&contract);
    fs::write(
        &scenario,
        r#"
tick_ms: 10
duration_ms: 40
inputs:
  - at_ms: 10
    set:
      digital_inputs:
        0: true
"#,
    )
    .expect("write fixture scenario");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("project-check")
        .arg(&plc)
        .arg("--scenario")
        .arg(&scenario)
        .arg("--out-dir")
        .arg(&out_dir)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run project-check with sidecar intent-alignment");

    assert!(
        output.status.success(),
        "project-check should pass with aligned sidecar intent evidence, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value =
        serde_json::from_slice(&output.stdout).expect("project-check should print JSON");
    let steps = report
        .get("steps")
        .and_then(Value::as_array)
        .expect("steps array");
    assert_eq!(
        steps.len(),
        5,
        "project-check should run five concrete checks"
    );
    assert!(
        steps.iter().any(|step| {
            step.get("name").and_then(Value::as_str) == Some("intent_alignment")
                && step.get("status").and_then(Value::as_str) == Some("pass")
                && step.get("intent_alignment_verdict").and_then(Value::as_str) == Some("aligned")
        }),
        "intent_alignment step should be marked as passed"
    );
    assert!(
        out_dir.join("intent_alignment/report.json").exists(),
        "expected intent-alignment report artifact to exist"
    );

    let report_text = fs::read_to_string(out_dir.join("intent_alignment/report.json"))
        .expect("intent-alignment report should exist");
    let report: IntentAlignmentReport =
        serde_json::from_str(&report_text).expect("intent-alignment report should deserialize");
    let summary = reduce_intent_alignment_report(&report);
    assert_eq!(summary.verdict, IntentAlignmentVerdict::Aligned);
}

#[test]
fn project_check_blocks_placeholder_intent_contracts() {
    let base = temp_dir("rust_plc_project_check_intent_placeholder");
    let plc = base.join("fixture.plc");
    let scenario = base.join("scenario.yaml");
    let out_dir = base.join("artifacts");
    let contract = base.join("fixture.intent_alignment.contract.json");
    write_intent_alignment_plc(&plc);
    write_placeholder_intent_alignment_fixture(&contract);
    fs::write(
        &scenario,
        r#"
tick_ms: 10
duration_ms: 40
inputs:
  - at_ms: 10
    set:
      digital_inputs:
        0: true
"#,
    )
    .expect("write fixture scenario");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("project-check")
        .arg(&plc)
        .arg("--scenario")
        .arg(&scenario)
        .arg("--out-dir")
        .arg(&out_dir)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run project-check with placeholder intent contract");

    assert!(
        !output.status.success(),
        "project-check should fail for a placeholder intent contract"
    );

    let report: Value =
        serde_json::from_slice(&output.stdout).expect("project-check should print JSON");
    let steps = report
        .get("steps")
        .and_then(Value::as_array)
        .expect("steps array");
    assert!(
        steps.iter().any(|step| {
            step.get("name").and_then(Value::as_str) == Some("intent_alignment")
                && step.get("status").and_then(Value::as_str) == Some("fail")
                && step
                    .get("intent_alignment_blocker_kind")
                    .and_then(Value::as_str)
                    == Some("invalid_contract")
        }),
        "placeholder contract should be reported as invalid_contract"
    );

    let stderr_log = fs::read_to_string(out_dir.join("intent_alignment/stderr.log"))
        .expect("intent_alignment stderr log");
    assert!(
        stderr_log.contains("scaffold placeholder"),
        "stderr should explain that the contract is still scaffold-grade: {stderr_log}"
    );
}
