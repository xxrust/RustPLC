use serde_json::Value;
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

fn write_fixture_plc(path: &Path) {
    let plc = r#"
[topology]
device plc_main: plc {
    purpose: "alarm test controller"
    model_ref: openplc_softplc
}
device start_button: sensor {
    purpose: "alarm test start input"
    subtype: "push_button"
    debounce: 20ms
}
device run_lamp: solenoid_valve {
    purpose: "alarm test output"
    response_time: 20ms
}

relation { from: start_button.out, to: plc_main.X0, via: reports_to }
relation { from: plc_main.Y0, to: run_lamp.coil, via: driven_by }

[constraints]

[tasks]
task cycle:
    step wait_start:
        wait: start_button == true
        timeout: 20ms -> goto fault
    step run:
        action: set run_lamp on
    on_complete: goto done

task fault:
    step safe_stop:
        action: set run_lamp off

task done:
    step halt:
"#;
    fs::write(path, plc).expect("write fixture plc");
}

#[test]
fn sim_plc_emits_alarm_event_audit_with_required_fields() {
    let base = temp_dir("rust_plc_runtime_alarm_audit");
    let plc = base.join("fixture.plc");
    let scenario = base.join("scenario.yaml");
    let trace = base.join("trace.jsonl");
    let alarms = base.join("alarm_events.ndjson");
    write_fixture_plc(&plc);

    fs::write(
        &scenario,
        r#"
tick_ms: 10
duration_ms: 60
inputs: []
"#,
    )
    .expect("write scenario");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("sim-plc")
        .arg(&plc)
        .arg("--scenario")
        .arg(&scenario)
        .arg("--out")
        .arg(&trace)
        .arg("--alarm-audit-out")
        .arg(&alarms)
        .arg("--alarm-scenario-id")
        .arg("recipe_timeout_case")
        .arg("--alarm-top")
        .arg("2")
        .output()
        .expect("run sim-plc with alarm audit");

    assert!(
        output.status.success(),
        "sim-plc should succeed with alarm publishing enabled, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let body = fs::read_to_string(&alarms).expect("read alarm events");
    let first = body
        .lines()
        .next()
        .expect("expected at least one alarm event");
    let event: Value = serde_json::from_str(first).expect("valid alarm event JSON");

    assert!(event.get("alarm_id").and_then(Value::as_str).is_some());
    assert_eq!(
        event.get("severity").and_then(Value::as_str),
        Some("critical")
    );
    assert!(
        event
            .get("first_seen_ms")
            .and_then(Value::as_u64)
            .is_some_and(|value| value > 0)
    );
    assert_eq!(
        event.get("evidence_source").and_then(Value::as_str),
        Some("runtime_live")
    );
    assert_eq!(
        event.get("scenario_or_recipe_id").and_then(Value::as_str),
        Some("recipe_timeout_case")
    );
    assert!(
        event
            .get("evidence_ref")
            .and_then(Value::as_str)
            .is_some_and(|value| value.contains("trace.jsonl")),
        "evidence_ref should point to the generated trace"
    );
    let top_candidates = event
        .get("top_candidates")
        .and_then(Value::as_array)
        .expect("top_candidates should exist");
    assert!(
        !top_candidates.is_empty(),
        "top_candidates should include diagnosis candidates"
    );
    assert!(
        top_candidates[0]
            .get("issue_code")
            .and_then(Value::as_str)
            .is_some_and(|code| code.starts_with("AXF-")),
        "top candidate should carry AXF-* issue code"
    );
    assert!(
        top_candidates[0]
            .get("legacy_issue_code")
            .and_then(Value::as_str)
            .is_some_and(|code| code.starts_with("DIAG-")),
        "top candidate should retain DIAG-* compatibility code"
    );
}

#[test]
fn sim_plc_keeps_running_when_hmi_websocket_is_unavailable() {
    let base = temp_dir("rust_plc_runtime_alarm_ws_fallback");
    let plc = base.join("fixture.plc");
    let scenario = base.join("scenario.yaml");
    let trace = base.join("trace.jsonl");
    let alarms = base.join("alarm_events.ndjson");
    write_fixture_plc(&plc);

    fs::write(
        &scenario,
        r#"
tick_ms: 10
duration_ms: 60
inputs: []
"#,
    )
    .expect("write scenario");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("sim-plc")
        .arg(&plc)
        .arg("--scenario")
        .arg(&scenario)
        .arg("--out")
        .arg(&trace)
        .arg("--alarm-audit-out")
        .arg(&alarms)
        .arg("--alarm-hmi-ws")
        .arg("ws://127.0.0.1:9/runtime-alarm")
        .output()
        .expect("run sim-plc with offline websocket target");

    assert!(
        output.status.success(),
        "sim-plc should not fail when realtime websocket is unavailable, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body = fs::read_to_string(&alarms).expect("read alarm events");
    assert!(
        !body.trim().is_empty(),
        "audit log should still contain alarm events when websocket is offline"
    );
}
