use rust_plc::error::PlcError;
use rust_plc::parser::parse_plc;
use rust_plc::semantic::{
    build_constraint_set, build_state_machine, build_timing_model, build_topology_graph,
    preprocess_program,
};
use rust_plc::verification::{VerificationSummary, verify_all};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

fn example_path(file_name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(file_name)
}

fn read_example(file_name: &str) -> String {
    let path = example_path(file_name);
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read example {}: {err}", path.display()))
}

fn collect_stage<T>(result: Result<T, Vec<PlcError>>, errors: &mut Vec<PlcError>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(mut stage_errors) => {
            errors.append(&mut stage_errors);
            None
        }
    }
}

fn compile_and_verify(source: &str) -> Result<VerificationSummary, Vec<String>> {
    let program = parse_plc(source).map_err(|err| vec![err.to_string()])?;
    let expanded_program = preprocess_program(&program).map_err(|errors| {
        errors
            .into_iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
    })?;

    let mut errors = Vec::new();
    let topology = collect_stage(build_topology_graph(&expanded_program), &mut errors);
    let state_machine = collect_stage(build_state_machine(&expanded_program), &mut errors);
    let constraints = collect_stage(build_constraint_set(&expanded_program), &mut errors);
    let _timing_model = collect_stage(build_timing_model(&expanded_program), &mut errors);

    if !errors.is_empty() {
        return Err(errors.into_iter().map(|error| error.to_string()).collect());
    }

    let topology = topology.expect("topology exists when semantic errors are empty");
    let state_machine = state_machine.expect("state machine exists when semantic errors are empty");
    let constraints = constraints.expect("constraints exist when semantic errors are empty");

    verify_all(&expanded_program, &topology, &constraints, &state_machine).map_err(|issues| {
        issues
            .into_iter()
            .map(|issue| issue.to_string())
            .collect::<Vec<_>>()
    })
}

fn assert_verification_passes(summary: &VerificationSummary) {
    assert!(
        matches!(summary.safety.level.as_str(), "完备证明" | "有界验证"),
        "unexpected safety level: {}",
        summary.safety.level
    );
    assert_eq!(summary.liveness.level, "通过");
    assert_eq!(summary.timing.level, "通过");
    assert_eq!(summary.causality.level, "通过");
}

fn compile_source_to_json(source: &str) -> Result<Value, Vec<String>> {
    let program = parse_plc(source).map_err(|err| vec![err.to_string()])?;
    let expanded_program = preprocess_program(&program).map_err(|errors| {
        errors
            .into_iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
    })?;

    let mut errors = Vec::new();
    let topology = collect_stage(build_topology_graph(&expanded_program), &mut errors);
    let state_machine = collect_stage(build_state_machine(&expanded_program), &mut errors);
    let constraints = collect_stage(build_constraint_set(&expanded_program), &mut errors);
    let timing_model = collect_stage(build_timing_model(&expanded_program), &mut errors);

    if !errors.is_empty() {
        return Err(errors.into_iter().map(|error| error.to_string()).collect());
    }

    let topology = topology.expect("topology exists when semantic errors are empty");
    let state_machine = state_machine.expect("state machine exists when semantic errors are empty");
    let constraints = constraints.expect("constraints exist when semantic errors are empty");
    let timing_model = timing_model.expect("timing model exists when semantic errors are empty");

    let verification = verify_all(&expanded_program, &topology, &constraints, &state_machine)
        .map_err(|diagnostics| {
            diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.to_string())
                .collect::<Vec<_>>()
        })?;

    Ok(json!({
        "topology": topology,
        "state_machine": state_machine,
        "constraints": constraints,
        "timing_model": timing_model,
        "verification": verification,
    }))
}

macro_rules! err_contains {
    ($source:expr, $($needle:expr),+ $(,)?) => {{
        let errors = compile_and_verify($source).expect_err("expected compilation/verification error");
        let joined = errors.join("\n");
        $(
            assert!(
                joined.contains($needle),
                "expected error output to contain `{}`, got:\n{}",
                $needle,
                joined
            );
        )+
    }};
}

// -----------------------------------------------------------------------------
// Big demo examples (non-trivial size) should compile, verify, and produce non-trivial IR.
// -----------------------------------------------------------------------------

#[test]
fn large_example_assembly_station_compiles_and_has_nontrivial_ir() {
    let source = read_example("assembly_station.plc");
    let ir = compile_source_to_json(&source).expect("assembly_station should compile");

    assert!(ir["topology"]["graph"]["nodes"].as_array().unwrap().len() >= 30);
    assert!(ir["state_machine"]["states"].as_array().unwrap().len() >= 12);
    assert!(ir["state_machine"]["transitions"].as_array().unwrap().len() >= 20);
}

// -----------------------------------------------------------------------------
// Stress / scale tests (large topology, many steps, large repeat expansion).
// -----------------------------------------------------------------------------

#[test]
fn stress_many_steps_compiles_and_verifies() {
    let mut steps = String::new();
    for idx in 1..=120usize {
        steps.push_str(&format!(
            "    step s{idx}:\n        action: log \"{idx}\"\n"
        ));
    }

    let source = format!(
        r#"
[topology]

[constraints]

[tasks]

task main:
{steps}
"#
    );

    let summary = compile_and_verify(&source).expect("120-step program should compile and verify");
    assert_verification_passes(&summary);
}

#[test]
fn stress_many_devices_compiles_and_verifies() {
    // 30 cylinders + valves + sensors. Keep the task logic minimal so verification stays fast.
    let mut topo = String::new();
    let mut relations = String::new();
    for idx in 0..30usize {
        topo.push_str(&format!("device Y{idx}: digital_output\n"));
        topo.push_str(&format!("device X{idx}: digital_input\n"));
        topo.push_str(&format!(
            "device valve_{idx}: solenoid_valve {{ response_time: 10ms }}\n"
        ));
        topo.push_str(&format!(
            "device cyl_{idx}: cylinder {{ stroke_time: 120ms, retract_time: 110ms }}\n"
        ));
        topo.push_str(&format!("device sensor_{idx}_ext: sensor\n"));
        relations.push_str(&format!(
            "relation {{ from: Y{idx}.out, to: valve_{idx}.coil, via: driven_by }}\n"
        ));
        relations.push_str(&format!(
            "relation {{ from: valve_{idx}.out, to: cyl_{idx}.cmd, via: driven_by }}\n"
        ));
        relations.push_str(&format!(
            "relation {{ from: cyl_{idx}.extended, to: sensor_{idx}_ext.sense, via: detects }}\n"
        ));
        relations.push_str(&format!(
            "relation {{ from: sensor_{idx}_ext.out, to: X{idx}.in, via: reports_to }}\n"
        ));
    }

    let mut causality = String::new();
    for idx in 0..30usize {
        causality.push_str(&format!(
            "causality: Y{idx} -> valve_{idx} -> cyl_{idx} -> sensor_{idx}_ext\n"
        ));
    }

    let source = format!(
        r#"
[topology]
{topo}
{relations}

[constraints]
{causality}

[tasks]

task main:
    step run:
        action: log "ok"
"#
    );

    let summary =
        compile_and_verify(&source).expect("many-device topology should compile and verify");
    assert_verification_passes(&summary);
}

#[test]
fn stress_repeat_expansion_80_compiles_and_expands() {
    let source = r#"
[topology]

[constraints]

[tasks]

task main:
    step loop:
        repeat 80:
            action: log "tick"
"#;

    let ir = compile_source_to_json(source).expect("repeat 80 should compile");
    let states = ir["state_machine"]["states"]
        .as_array()
        .expect("state_machine.states should be array");
    assert!(
        states
            .iter()
            .any(|state| state["step_name"] == Value::String("loop_80".to_string())),
        "repeat expansion should include loop_80"
    );
}

#[test]
fn stress_parallel_10_branches_compiles_and_verifies() {
    let source = r#"
[topology]

[constraints]

[tasks]

task main:
    step fork:
        parallel:
            b1:
                action: log "1"
            b2:
                action: log "2"
            b3:
                action: log "3"
            b4:
                action: log "4"
            b5:
                action: log "5"
            b6:
                action: log "6"
            b7:
                action: log "7"
            b8:
                action: log "8"
            b9:
                action: log "9"
            b10:
                action: log "10"
    step after:
        action: log "done"
"#;

    let summary = compile_and_verify(source).expect("parallel 10 branches should verify");
    assert_verification_passes(&summary);
}

// -----------------------------------------------------------------------------
// Parser-level errors.
// -----------------------------------------------------------------------------

#[test]
fn parse_error_missing_tasks_section() {
    let source = r#"
[topology]

[constraints]
"#;
    err_contains!(source, "ERROR [parse]", "语法解析失败");
}

#[test]
fn parse_error_unknown_device_type() {
    let source = r#"
[topology]
device A0: thermocouple

[constraints]

[tasks]
task main:
    step s1:
        action: log "x"
"#;
    err_contains!(source, "ERROR [parse]", "expected device_type");
}

#[test]
fn parse_error_unsupported_attribute_name() {
    let source = r#"
[topology]
device valve_A: solenoid_valve {
    response_time: 20ms,
    foo: 1
}

[constraints]

[tasks]
task main:
    step s1:
        action: log "x"
"#;
    err_contains!(source, "ERROR [parse]", "expected attribute_name");
}

#[test]
fn parse_error_if_without_else() {
    let source = r#"
[topology]

[constraints]

[tasks]
task main:
    step decide:
        if: mode_switch == true goto then_task
"#;
    err_contains!(source, "ERROR [parse]");
}

#[test]
fn parse_error_wait_mixes_and_or() {
    let source = r#"
[topology]

[constraints]

[tasks]
task main:
    step wait_bad:
        wait: A == true AND B == true OR C == true
"#;
    err_contains!(source, "ERROR [parse]");
}

#[test]
fn parse_error_timeout_missing_goto() {
    let source = r#"
[topology]

[constraints]

[tasks]
task main:
    step wait_bad:
        wait: A == true
        timeout: 100ms
"#;
    err_contains!(source, "ERROR [parse]");
}

// -----------------------------------------------------------------------------
// Semantic / preprocess errors.
// -----------------------------------------------------------------------------

#[test]
fn semantic_error_empty_tasks_section() {
    let source = r#"
[topology]

[constraints]

[tasks]
"#;
    err_contains!(source, "ERROR [semantic]", "[tasks] 段至少需要一个 task");
}

#[test]
fn semantic_error_task_has_no_steps() {
    let source = r#"
[topology]

[constraints]

[tasks]
task main:
"#;
    err_contains!(source, "ERROR [semantic]", "至少需要一个 step");
}

#[test]
fn semantic_error_duplicate_task_name() {
    let source = r#"
[topology]

[constraints]

[tasks]
task main:
    step s1:
        action: log "x"

task main:
    step s2:
        action: log "y"
"#;
    err_contains!(source, "ERROR [duplicate_definition]", "重复定义task main");
}

#[test]
fn semantic_error_connected_to_references_undefined_device() {
    let source = r#"
[topology]
device Y0: digital_output

device valve_A: solenoid_valve {
    response_time: 15ms
}
relation { from: Y9.out, to: valve_A.coil, via: driven_by }

[constraints]

[tasks]
task main:
    step s1:
        action: log "x"
"#;
    err_contains!(source, "ERROR [undefined_reference]", "未定义设备 Y9");
}

#[test]
fn semantic_error_wait_references_undefined_device() {
    let source = r#"
[topology]
device sensor_A: sensor

[constraints]

[tasks]
task main:
    step wait_bad:
        wait: sensor_A == true AND sensor_X == true
        timeout: 100ms -> goto main
"#;
    err_contains!(source, "ERROR [undefined_reference]", "未定义设备 sensor_X");
}

#[test]
fn semantic_error_incompatible_connection_types() {
    let source = r#"
[topology]
device cyl_A: cylinder {
    stroke_time: 200ms,
    retract_time: 180ms
}

device valve_A: solenoid_valve {
    response_time: 15ms
}

device sensor_bad: sensor

device Y0: digital_output
relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_bad.sense, via: driven_by }

[constraints]

[tasks]
task main:
    step s1:
        action: log "x"
"#;
    err_contains!(source, "ERROR [type_mismatch]", "sensor", "cylinder");
}

#[test]
fn preprocess_error_repeat_nested_in_body() {
    let source = r#"
[topology]

[constraints]

[tasks]
task main:
    step bad:
        repeat 2:
            repeat 2:
                action: log "tick"
"#;
    err_contains!(source, "ERROR [semantic]", "不允许嵌套 repeat");
}

#[test]
fn preprocess_error_repeat_count_over_limit() {
    let source = r#"
[topology]

[constraints]

[tasks]
task main:
    step bad:
        repeat 101:
            action: log "tick"
"#;
    err_contains!(source, "ERROR [semantic]", "repeat 次数超过上限 100");
}

// -----------------------------------------------------------------------------
// Verification failures (Safety / Liveness / Timing / Causality).
// -----------------------------------------------------------------------------

#[test]
fn safety_fails_on_parallel_conflicts_with() {
    let source = r#"
[topology]
device Y0: digital_output
device Y1: digital_output
device valve_A: solenoid_valve { response_time: 15ms }
device valve_B: solenoid_valve { response_time: 15ms }
device cyl_A: cylinder { stroke_time: 200ms, retract_time: 180ms }
device cyl_B: cylinder { stroke_time: 250ms, retract_time: 220ms }
relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: Y1.out, to: valve_B.coil, via: driven_by }
relation { from: valve_B.out, to: cyl_B.cmd, via: driven_by }

[constraints]
safety: cyl_A.extended conflicts_with cyl_B.extended
    reason: "A缸和B缸同时伸出会碰撞"

[tasks]
task main:
    step together:
        parallel:
            a:
                action: extend cyl_A
            b:
                action: extend cyl_B
"#;
    err_contains!(source, "ERROR [safety]", "conflicts_with");
}

#[test]
fn liveness_fails_on_wait_without_timeout_or_allow() {
    let source = r#"
[topology]

[constraints]

[tasks]
task main:
    step wait_forever:
        wait: sensor_X == true
"#;
    err_contains!(source, "ERROR [liveness]", "缺少 timeout 分支");
}

#[test]
fn liveness_passes_when_allow_indefinite_wait_is_set() {
    let source = r#"
[topology]

[constraints]

[tasks]
task main:
    step wait_ok:
        wait: sensor_X == true
        allow_indefinite_wait: true
"#;
    let summary = compile_and_verify(source).expect("allow_indefinite_wait should pass liveness");
    assert_verification_passes(&summary);
}

#[test]
fn timing_fails_on_must_complete_within_too_small() {
    let source = r#"
[topology]
device Y0: digital_output
device X0: digital_input
device valve_A: solenoid_valve { response_time: 20ms }
device cyl_A: cylinder { stroke_time: 200ms, retract_time: 180ms }
device sensor_A_ext: sensor
relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_A_ext.sense, via: detects }
relation { from: sensor_A_ext.out, to: X0.in, via: reports_to }

[constraints]
timing: task.main.step_extend must_complete_within 100ms
    reason: "过严约束"
causality: Y0 -> valve_A -> cyl_A -> sensor_A_ext

[tasks]
task main:
    step step_extend:
        action: extend cyl_A
        wait: sensor_A_ext == true
        timeout: 500ms -> goto main
"#;
    err_contains!(source, "ERROR [timing]", "must_complete_within");
}

#[test]
fn timing_fails_on_must_start_after_when_shortest_is_insufficient() {
    let source = r#"
[topology]
device Y0: digital_output
device X0: digital_input
device valve_A: solenoid_valve { response_time: 10ms }
device cyl_A: cylinder { stroke_time: 50ms, retract_time: 50ms }
device sensor_A_ext: sensor
relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_A_ext.sense, via: detects }
relation { from: sensor_A_ext.out, to: X0.in, via: reports_to }

[constraints]
timing: task.cooldown must_start_after 500ms
    reason: "冷却前必须等待500ms"
causality: Y0 -> valve_A -> cyl_A -> sensor_A_ext

[tasks]
task work:
    step step_a:
        action: extend cyl_A
        wait: sensor_A_ext == true
        timeout: 80ms -> goto cooldown

task cooldown:
    step begin:
        action: log "cooldown"
"#;
    err_contains!(source, "ERROR [timing]", "must_start_after");
}

#[test]
fn causality_fails_when_declared_chain_is_not_connected() {
    let source = r#"
[topology]
device Y0: digital_output
device X0: digital_input
device valve_A: solenoid_valve { response_time: 20ms }
device cyl_A: cylinder { stroke_time: 200ms, retract_time: 180ms } # missing driven_by: valve_A
device sensor_A_ext: sensor
relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: cyl_A.extended, to: sensor_A_ext.sense, via: detects }
relation { from: sensor_A_ext.out, to: X0.in, via: reports_to }

[constraints]
causality: Y0 -> valve_A -> cyl_A -> sensor_A_ext

[tasks]
task main:
    step step_extend:
        action: extend cyl_A
        wait: sensor_A_ext == true
        timeout: 500ms -> goto main
"#;
    err_contains!(source, "ERROR [causality]", "因果链断裂");
}

#[test]
fn combined_failures_report_multiple_checkers() {
    let source = r#"
[topology]
device Y0: digital_output
device Y1: digital_output
device valve_A: solenoid_valve { response_time: 20ms }
device valve_B: solenoid_valve { response_time: 20ms }
device cyl_A: cylinder { stroke_time: 200ms, retract_time: 180ms }
device cyl_B: cylinder { stroke_time: 200ms, retract_time: 180ms }
relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: Y1.out, to: valve_B.coil, via: driven_by }
relation { from: valve_B.out, to: cyl_B.cmd, via: driven_by }

[constraints]
safety: cyl_A.extended conflicts_with cyl_B.extended
    reason: "冲突"
timing: task.main.step_a must_complete_within 10ms
    reason: "过严"

[tasks]
task main:
    step step_a:
        parallel:
            a:
                action: extend cyl_A
            b:
                action: extend cyl_B
        wait: sensor_X == true
"#;

    let errors = compile_and_verify(source).expect_err("should fail with multiple issues");
    let joined = errors.join("\n");

    assert!(joined.contains("ERROR [safety]"));
    assert!(joined.contains("ERROR [timing]") || joined.contains("ERROR [liveness]"));
}
