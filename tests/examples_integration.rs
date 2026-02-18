use rust_plc::error::PlcError;
use rust_plc::parser::parse_plc;
use rust_plc::semantic::{
    build_constraint_set, build_state_machine, build_timing_model, build_topology_graph,
    preprocess_program,
};
use rust_plc::verification::verify_all;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

    let payload = json!({
        "topology": topology,
        "state_machine": state_machine,
        "constraints": constraints,
        "timing_model": timing_model,
        "verification": verification,
    });

    let serialized = serde_json::to_string_pretty(&payload).expect("IR payload should serialize");
    let decoded: Value =
        serde_json::from_str(&serialized).expect("serialized IR payload should be valid JSON");

    Ok(decoded)
}

#[test]
fn parses_two_cylinder_example_into_verified_ir_json() {
    let source = read_example("two_cylinder.plc");
    let ir_json = compile_source_to_json(&source).expect("two_cylinder example should compile");

    assert!(ir_json.get("topology").is_some());
    assert!(ir_json.get("state_machine").is_some());
    assert!(ir_json.get("constraints").is_some());
    assert!(ir_json.get("timing_model").is_some());

    let states = ir_json["state_machine"]["states"]
        .as_array()
        .expect("state machine should include states array");
    assert!(!states.is_empty(), "state machine should have states");

    let safety_level = ir_json["verification"]["safety"]["level"]
        .as_str()
        .expect("verification.safety.level should be present");
    assert!(
        matches!(safety_level, "完备证明" | "有界验证"),
        "safety level should report proof quality"
    );
    assert_eq!(
        ir_json["verification"]["liveness"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["timing"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["causality"]["level"],
        Value::String("通过".to_string())
    );
}

#[test]
fn parses_half_rotation_example_into_verified_ir_json() {
    let source = read_example("half_rotation.plc");
    let ir_json = compile_source_to_json(&source).expect("half_rotation example should compile");

    let transitions = ir_json["state_machine"]["transitions"]
        .as_array()
        .expect("state machine should include transitions array");
    assert!(
        !transitions.is_empty(),
        "state machine should have transitions"
    );

    let timing_rules = ir_json["constraints"]["timing"]
        .as_array()
        .expect("constraints should include timing array");
    assert_eq!(
        timing_rules.len(),
        1,
        "half_rotation should define one timing rule"
    );

    assert_eq!(
        ir_json["verification"]["liveness"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["timing"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["causality"]["level"],
        Value::String("通过".to_string())
    );
}

#[test]
fn parses_delay_demo_example_into_verified_ir_json() {
    let source = read_example("delay_demo.plc");
    let ir_json = compile_source_to_json(&source).expect("delay_demo example should compile");

    let transitions = ir_json["state_machine"]["transitions"]
        .as_array()
        .expect("state machine should include transitions array");
    assert!(
        transitions.iter().any(|transition| {
            transition["guard"]["kind"] == Value::String("delay".to_string())
                && transition["guard"]["duration_ms"] == Value::Number(2000u64.into())
        }),
        "delay_demo should include a delay guard transition"
    );

    assert_eq!(
        ir_json["verification"]["liveness"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["timing"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["causality"]["level"],
        Value::String("通过".to_string())
    );
}

#[test]
fn parses_repeat_demo_example_into_verified_ir_json() {
    let source = read_example("repeat_demo.plc");
    let ir_json = compile_source_to_json(&source).expect("repeat_demo example should compile");

    let states = ir_json["state_machine"]["states"]
        .as_array()
        .expect("state machine should include states array");
    assert!(
        states
            .iter()
            .any(|state| state["step_name"] == Value::String("glue_cycle_1".to_string())),
        "repeat_demo should include expanded glue_cycle_1 step"
    );
    assert!(
        states
            .iter()
            .any(|state| state["step_name"] == Value::String("glue_cycle_3".to_string())),
        "repeat_demo should include expanded glue_cycle_3 step"
    );

    assert_eq!(
        ir_json["verification"]["liveness"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["timing"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["causality"]["level"],
        Value::String("通过".to_string())
    );
}

#[test]
fn parses_stepper_collision_guard_example_into_verified_ir_json() {
    let source = read_example("stepper_collision_guard.plc");
    let ir_json =
        compile_source_to_json(&source).expect("stepper_collision_guard example should compile");

    let safety_rules = ir_json["constraints"]["safety"]
        .as_array()
        .expect("constraints.safety should be an array");
    assert_eq!(
        safety_rules.len(),
        3,
        "stepper_collision_guard should define three safety rules (alarm interlock + window + command interlock)"
    );

    let safety_statuses = ir_json["verification"]["safety"]["rule_statuses"]
        .as_array()
        .expect("verification.safety.rule_statuses should be an array");
    assert_eq!(
        safety_statuses.len(),
        3,
        "verification report should include a status entry for each safety rule"
    );
    assert!(
        safety_statuses.iter().any(|status| {
            status["rule"]
                .as_str()
                .unwrap_or("")
                .contains("zone_code > 0")
        }),
        "verification report should include the zone_code window interlock rule"
    );
    assert!(
        safety_statuses.iter().any(|status| {
            status["rule"]
                .as_str()
                .unwrap_or("")
                .contains("move_cmd.on")
        }),
        "verification report should include the move_cmd command interlock rule"
    );
    assert_eq!(
        ir_json["verification"]["safety"]["skipped_rules"]
            .as_u64()
            .expect("skipped_rules should be numeric"),
        0,
        "no safety rules should be skipped in this example"
    );

    let safety_level = ir_json["verification"]["safety"]["level"]
        .as_str()
        .expect("verification.safety.level should be present");
    assert!(
        matches!(safety_level, "完备证明" | "有界验证"),
        "safety level should report proof quality"
    );
    assert_eq!(
        ir_json["verification"]["liveness"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["timing"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["causality"]["level"],
        Value::String("通过".to_string())
    );
}

#[test]
fn parses_stepper_multi_sensor_consistency_example_into_verified_ir_json() {
    let source = read_example("stepper_multi_sensor_consistency.plc");
    let ir_json = compile_source_to_json(&source)
        .expect("stepper_multi_sensor_consistency example should compile");

    let safety_rules = ir_json["constraints"]["safety"]
        .as_array()
        .expect("constraints.safety should be an array");
    assert_eq!(
        safety_rules.len(),
        1,
        "stepper_multi_sensor_consistency should define one safety rule (alarm interlock)"
    );

    let safety_statuses = ir_json["verification"]["safety"]["rule_statuses"]
        .as_array()
        .expect("verification.safety.rule_statuses should be an array");
    assert_eq!(
        safety_statuses.len(),
        1,
        "verification report should include a status entry for the safety rule"
    );
    assert!(
        safety_statuses.iter().any(|status| {
            status["rule"]
                .as_str()
                .unwrap_or("")
                .contains("axis_x.on")
        }),
        "verification report should include the axis_x alarm interlock rule"
    );
    assert_eq!(
        ir_json["verification"]["safety"]["skipped_rules"]
            .as_u64()
            .expect("skipped_rules should be numeric"),
        0,
        "no safety rules should be skipped in this example"
    );

    assert_eq!(
        ir_json["verification"]["liveness"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["timing"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["causality"]["level"],
        Value::String("通过".to_string())
    );
}

#[test]
fn parses_force_override_demo_example_into_verified_ir_json() {
    let source = read_example("force_override_demo.plc");
    let ir_json =
        compile_source_to_json(&source).expect("force_override_demo example should compile");

    assert_eq!(
        ir_json["verification"]["liveness"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["timing"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["causality"]["level"],
        Value::String("通过".to_string())
    );
}

#[test]
fn parses_and_or_wait_demo_example_into_verified_ir_json() {
    let source = read_example("and_or_wait_demo.plc");
    let ir_json = compile_source_to_json(&source).expect("and_or_wait_demo should compile");

    assert_eq!(
        ir_json["verification"]["liveness"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["timing"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["causality"]["level"],
        Value::String("通过".to_string())
    );
}

#[test]
fn parses_if_else_demo_example_into_verified_ir_json() {
    let source = read_example("if_else_demo.plc");
    let ir_json = compile_source_to_json(&source).expect("if_else_demo should compile");

    let transitions = ir_json["state_machine"]["transitions"]
        .as_array()
        .expect("state machine should include transitions array");

    let from_decide = transitions
        .iter()
        .filter(|transition| {
            transition["from"]["task_name"] == Value::String("choose".to_string())
                && transition["from"]["step_name"] == Value::String("decide".to_string())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        from_decide.len(),
        2,
        "if_else_demo should create two outgoing transitions for choose.decide"
    );

    assert!(
        from_decide.iter().any(|transition| {
            transition["guard"]["kind"] == Value::String("condition".to_string())
                && transition["guard"]["expression"]
                    == Value::String("mode_switch == true".to_string())
        }),
        "if_else_demo should include then branch guard"
    );
    assert!(
        from_decide.iter().any(|transition| {
            transition["guard"]["kind"] == Value::String("condition".to_string())
                && transition["guard"]["expression"]
                    == Value::String("NOT(mode_switch == true)".to_string())
        }),
        "if_else_demo should include else branch guard"
    );

    assert_eq!(
        ir_json["verification"]["liveness"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["timing"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["causality"]["level"],
        Value::String("通过".to_string())
    );
}

#[test]
fn goto_task_step_jumps_to_named_step() {
    let source = r#"
[topology]

[constraints]

[tasks]

task cycle:
    step prep:
        action: log "prep"
    step press_down:
        action: log "press"
    on_complete: goto done

task main:
    step start:
        goto cycle.press_down

task done:
    step finish:
        action: log "done"
"#;

    let ir_json = compile_source_to_json(source).expect("goto task.step source should compile");

    let transitions = ir_json["state_machine"]["transitions"]
        .as_array()
        .expect("state machine should include transitions array");
    assert!(
        transitions.iter().any(|transition| {
            transition["from"]["task_name"] == Value::String("main".to_string())
                && transition["from"]["step_name"] == Value::String("start".to_string())
                && transition["to"]["task_name"] == Value::String("cycle".to_string())
                && transition["to"]["step_name"] == Value::String("press_down".to_string())
        }),
        "goto task.step should jump to the named step instead of the task initial step"
    );

    assert_eq!(
        ir_json["verification"]["liveness"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["timing"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["causality"]["level"],
        Value::String("通过".to_string())
    );
}

#[test]
fn goto_to_missing_step_reports_semantic_error() {
    let source = r#"
[topology]

[constraints]

[tasks]

task cycle:
    step prep:
        action: log "prep"

task main:
    step start:
        goto cycle.missing_step
"#;

    let errors =
        compile_source_to_json(source).expect_err("missing step should be a semantic error");
    let joined = errors.join("\n");
    assert!(
        joined.contains("未定义 step cycle.missing_step"),
        "error should mention the missing task.step target"
    );
}

#[test]
fn parses_custom_states_demo_example_into_verified_ir_json() {
    let source = read_example("custom_states_demo.plc");
    let ir_json = compile_source_to_json(&source).expect("custom_states_demo should compile");

    assert_eq!(
        ir_json["verification"]["liveness"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["timing"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["causality"]["level"],
        Value::String("通过".to_string())
    );
}

#[test]
fn repeat_expansion_produces_same_verification_result_as_manual_unrolling() {
    let repeat_source = r#"
[topology]

device Y0: digital_output
device X0: digital_input
device X1: digital_input

device valve_glue: solenoid_valve {
    connected_to: Y0,
    response_time: 15ms
}

device cyl_glue: cylinder {
    connected_to: valve_glue,
    type: double_acting,
    stroke: 50mm,
    stroke_time: 120ms,
    retract_time: 110ms
}

device sensor_glue_ext: sensor {
    connected_to: X0,
    detects: cyl_glue.extended
}

device sensor_glue_ret: sensor {
    connected_to: X1,
    detects: cyl_glue.retracted
}

[constraints]

causality: Y0 -> valve_glue -> cyl_glue -> sensor_glue_ext
causality: Y0 -> valve_glue -> cyl_glue -> sensor_glue_ret

[tasks]

task glue:
    step glue_cycle:
        repeat 3:
            action: extend cyl_glue
            wait: sensor_glue_ext == true
            timeout: 300ms -> goto fault_handler
            action: retract cyl_glue
            wait: sensor_glue_ret == true
            timeout: 300ms -> goto fault_handler
    on_complete: goto idle

task idle:
    step ready:
        action: log "glue done"

task fault_handler:
    step safe:
        action: retract cyl_glue
    on_complete: goto idle
"#;

    let manual_source = r#"
[topology]

device Y0: digital_output
device X0: digital_input
device X1: digital_input

device valve_glue: solenoid_valve {
    connected_to: Y0,
    response_time: 15ms
}

device cyl_glue: cylinder {
    connected_to: valve_glue,
    type: double_acting,
    stroke: 50mm,
    stroke_time: 120ms,
    retract_time: 110ms
}

device sensor_glue_ext: sensor {
    connected_to: X0,
    detects: cyl_glue.extended
}

device sensor_glue_ret: sensor {
    connected_to: X1,
    detects: cyl_glue.retracted
}

[constraints]

causality: Y0 -> valve_glue -> cyl_glue -> sensor_glue_ext
causality: Y0 -> valve_glue -> cyl_glue -> sensor_glue_ret

[tasks]

task glue:
    step glue_cycle_1:
        action: extend cyl_glue
        wait: sensor_glue_ext == true
        timeout: 300ms -> goto fault_handler
        action: retract cyl_glue
        wait: sensor_glue_ret == true
        timeout: 300ms -> goto fault_handler
    step glue_cycle_2:
        action: extend cyl_glue
        wait: sensor_glue_ext == true
        timeout: 300ms -> goto fault_handler
        action: retract cyl_glue
        wait: sensor_glue_ret == true
        timeout: 300ms -> goto fault_handler
    step glue_cycle_3:
        action: extend cyl_glue
        wait: sensor_glue_ext == true
        timeout: 300ms -> goto fault_handler
        action: retract cyl_glue
        wait: sensor_glue_ret == true
        timeout: 300ms -> goto fault_handler
    on_complete: goto idle

task idle:
    step ready:
        action: log "glue done"

task fault_handler:
    step safe:
        action: retract cyl_glue
    on_complete: goto idle
"#;

    let repeat_ir = compile_source_to_json(repeat_source).expect("repeat source should compile");
    let manual_ir =
        compile_source_to_json(manual_source).expect("manual unrolled source should compile");

    assert_eq!(
        repeat_ir["verification"], manual_ir["verification"],
        "repeat and manual unrolling should produce identical verification results"
    );
}

#[test]
fn reports_undefined_device_for_error_example() {
    let source = read_example("error_missing_device.plc");
    let errors = compile_source_to_json(&source)
        .expect_err("error_missing_device should fail semantic checks");

    assert!(
        errors.iter().any(|error| error.contains("未定义设备 Y9")),
        "error output should include missing device name"
    );
}

#[test]
fn reports_all_four_verifier_failures_for_combined_error_example() {
    let source = read_example("error_all_verifiers.plc");
    let errors = compile_source_to_json(&source)
        .expect_err("combined verifier error example should fail verification");

    let joined = errors.join("\n\n");
    assert!(
        joined.contains("ERROR [safety]"),
        "should report safety error"
    );
    assert!(
        joined.contains("ERROR [liveness]"),
        "should report liveness error"
    );
    assert!(
        joined.contains("ERROR [timing]"),
        "should report timing error"
    );
    assert!(
        joined.contains("ERROR [causality]"),
        "should report causality error"
    );
    assert!(
        joined.contains("位置:"),
        "errors should include source location"
    );
    assert!(
        joined.contains("建议:"),
        "errors should include fix suggestions"
    );
}

#[test]
fn cli_prints_verified_json_and_summary_for_two_cylinder_example() {
    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg(example_path("two_cylinder.plc"))
        .output()
        .expect("should run rust_plc binary");

    assert!(
        output.status.success(),
        "CLI should succeed for valid example, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let decoded: Value =
        serde_json::from_slice(&output.stdout).expect("CLI stdout should be valid JSON");
    assert!(decoded.get("topology").is_some());
    assert!(decoded.get("state_machine").is_some());
    assert!(decoded.get("constraints").is_some());
    assert!(decoded.get("timing_model").is_some());
    assert!(decoded.get("verification").is_some());

    let stderr_text = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr_text.contains("验证通过"),
        "CLI should print success summary to stderr"
    );
    assert!(
        stderr_text.contains("Safety:"),
        "CLI summary should include safety proof level"
    );
}

#[test]
fn parses_analog_pressure_demo_example_into_verified_ir_json() {
    let source = read_example("analog_pressure_demo.plc");
    let ir_json =
        compile_source_to_json(&source).expect("analog_pressure_demo example should compile");

    assert!(ir_json.get("topology").is_some());
    assert!(ir_json.get("state_machine").is_some());
    assert!(ir_json.get("constraints").is_some());
    assert!(ir_json.get("timing_model").is_some());

    // Verify analog device types appear in topology
    let nodes = ir_json["topology"]["graph"]["nodes"]
        .as_array()
        .expect("topology should have nodes");
    let kinds: Vec<&str> = nodes.iter().filter_map(|n| n["kind"].as_str()).collect();
    assert!(
        kinds.contains(&"analog_input"),
        "should contain analog_input device"
    );
    assert!(
        kinds.contains(&"analog_output"),
        "should contain analog_output device"
    );

    // Verify set_analog actions appear in transitions
    let transitions = ir_json["state_machine"]["transitions"]
        .as_array()
        .expect("state machine should have transitions");
    let has_set_analog = transitions.iter().any(|t| {
        t["actions"]
            .as_array()
            .map(|actions| actions.iter().any(|a| a["action"] == "set_analog"))
            .unwrap_or(false)
    });
    assert!(
        has_set_analog,
        "transitions should include set_analog actions"
    );

    // Verify analog connection type in edges
    let edges = ir_json["topology"]["graph"]["edges"]
        .as_array()
        .expect("topology should have edges");
    let has_analog_edge = edges.iter().any(|e| e[2] == "analog");
    assert!(
        has_analog_edge,
        "topology should have analog connection type"
    );

    // All four verifiers pass
    let safety_level = ir_json["verification"]["safety"]["level"]
        .as_str()
        .expect("verification.safety.level should be present");
    assert!(
        matches!(safety_level, "完备证明" | "有界验证"),
        "safety level should report proof quality"
    );
    assert_eq!(
        ir_json["verification"]["liveness"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["timing"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["causality"]["level"],
        Value::String("通过".to_string())
    );
}
