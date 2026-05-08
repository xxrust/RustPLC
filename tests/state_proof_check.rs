use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, content).expect("write file");
}

fn run_cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .args(args)
        .output()
        .expect("run rust_plc")
}

#[test]
fn state_proof_check_json_reports_stable_issue_codes() {
    let base = temp_dir("rust_plc_state_proof_json");
    let plc = base.join("seeded_flag.plc");
    write(
        &plc,
        r#"
[topology]
variable feed_cassette_has_seed: bool = true

[constraints]

[tasks]
task main:
    step wait_seed:
        wait: feed_cassette_has_seed == true
"#,
    );

    let output = run_cli(&[
        "state-proof-check",
        plc.to_str().expect("utf8 path"),
        "--output",
        "json",
    ]);
    assert!(
        !output.status.success(),
        "seeded physical-state flag should fail the check"
    );

    let report: Value =
        serde_json::from_slice(&output.stdout).expect("state-proof-check should emit JSON");
    assert_eq!(
        report.get("command").and_then(Value::as_str),
        Some("state-proof-check")
    );
    assert_eq!(report.get("status").and_then(Value::as_str), Some("fail"));
    let issues = report
        .get("issues")
        .and_then(Value::as_array)
        .expect("issues array");
    assert!(
        issues
            .iter()
            .any(|issue| issue.get("code").and_then(Value::as_str) == Some("SPF-001")),
        "expected SPF-001 in JSON issues"
    );
}

#[test]
fn state_proof_check_human_output_includes_location_reason_and_fix() {
    let base = temp_dir("rust_plc_state_proof_human");
    let plc = base.join("ingress_stock_flag.plc");
    write(
        &plc,
        r#"
[topology]
workpiece part: workpiece_type {
    normal_terminal_states: [finished]
    ingress_sites: [feed_cassette]
    normal_egress_sites: [outfeed]
}
location feed_cassette: workpiece_location { capacity: 1 }
location outfeed: workpiece_location { capacity: 1 }
holder arm: workpiece_holder { capacity: 1 }
variable feed_cassette_has_seed: bool = true

[constraints]

[tasks]
task main:
    step wait_seed:
        wait: feed_cassette_has_seed == true
    step pick:
        effect: acquire holder arm from feed_cassette
    step place:
        effect: transfer from arm to outfeed
    step finish:
        effect: finish workpiece at outfeed as finished
"#,
    );

    let output = run_cli(&["state-proof-check", plc.to_str().expect("utf8 path")]);
    assert!(
        !output.status.success(),
        "ingress-backed stock flag should fail the human report path"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("state-proof-check: FAIL"));
    assert!(stderr.contains("SPF-001"));
    assert!(stderr.contains("reason:"));
    assert!(stderr.contains("fix:"));
    assert!(stderr.contains("ingress_stock_flag.plc"));
}

#[test]
fn project_check_marks_state_proof_step_as_failed_for_workpiece_project_without_startup_baseline() {
    let project_dir = temp_dir("rust_plc_state_proof_project_check");
    write(
        &project_dir.join("rustplc.project.toml"),
        "schema_version = 1\n\n[project]\nname = \"State Proof Fail\"\nslug = \"state_proof_fail\"\n\n[entry]\nsystem = \"plc/main.system.md\"\nplc = \"plc/main.plc\"\nscenario = \"scenarios/nominal/normal.yaml\"\nio_map = \"config/io_map.toml\"\nretain = \"config/retain.toml\"\nworkpiece = \"config/workpiece.toml\"\n",
    );
    write(
        &project_dir.join("plc/main.system.md"),
        "# State Proof Fail\n",
    );
    write(
        &project_dir.join("config/workpiece.toml"),
        "schema_version = 1\n\n[workpiece]\nrequired = true\n",
    );
    write(
        &project_dir.join("plc/main.plc"),
        r#"
[topology]
device plc_main: plc {
    purpose: "demo controller"
    model_ref: openplc_softplc
}
device start_button: sensor { purpose: "start", subtype: "push_button", debounce: 20ms }
relation { from: start_button.out, to: plc_main.X0, via: reports_to }

workpiece part: workpiece_type {
    normal_terminal_states: [finished]
    ingress_sites: [infeed]
    normal_egress_sites: [outfeed]
}
location infeed: workpiece_location { capacity: 1 }
location outfeed: workpiece_location { capacity: 1 }
holder arm: workpiece_holder { capacity: 1 }

[constraints]

[tasks]
task main:
    step wait_start:
        wait: start_button == true
        allow_indefinite_wait: true

    step pick:
        effect: acquire holder arm from infeed

    step place:
        effect: transfer from arm to outfeed

    step finish:
        effect: finish workpiece at outfeed as finished

    on_complete: goto done

task done:
    step halt:
"#,
    );
    write(
        &project_dir.join("scenarios/nominal/normal.yaml"),
        "tick_ms: 10\nduration_ms: 40\ninputs:\n  - at_ms: 0\n    set:\n      digital_inputs:\n        0: true\nforces: []\n",
    );
    write(
        &project_dir.join("config/io_map.toml"),
        "schema_version = 1\n\n[digital_inputs]\ndi0 = { gpio = 2, pull = \"up\" }\n",
    );
    write(
        &project_dir.join("config/retain.toml"),
        "schema_version = 1\n\n[retain]\nenabled = false\npath = \"out/sim/retain_state.json\"\n",
    );

    let out_dir = project_dir.join("out/check");
    let output = run_cli(&[
        "project-check",
        project_dir
            .join("plc/main.plc")
            .to_str()
            .expect("utf8 plc path"),
        "--scenario",
        project_dir
            .join("scenarios/nominal/normal.yaml")
            .to_str()
            .expect("utf8 scenario path"),
        "--out-dir",
        out_dir.to_str().expect("utf8 out dir"),
        "--output",
        "json",
    ]);
    assert!(
        !output.status.success(),
        "project-check should fail when state_proof_check fails"
    );

    let report: Value =
        serde_json::from_slice(&output.stdout).expect("project-check should emit JSON");
    let steps = report
        .get("steps")
        .and_then(Value::as_array)
        .expect("steps array");
    assert!(
        steps.iter().any(|step| {
            step.get("name").and_then(Value::as_str) == Some("state_proof_check")
                && step.get("status").and_then(Value::as_str) == Some("fail")
        }),
        "aggregate project-check report should include a failed state_proof_check step"
    );
    assert!(
        out_dir.join("state_proof_check/report.json").exists(),
        "state_proof_check JSON artifact should be preserved in project-check output"
    );
}
