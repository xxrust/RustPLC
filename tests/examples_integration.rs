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
fn parses_flying_shear_example_into_verified_ir_json() {
    let source = read_example("flying_shear.plc");
    let ir_json = compile_source_to_json(&source).expect("flying_shear example should compile");

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
    let source = r#"
[topology]

device Y0: digital_output
device X0: digital_input
device X1: digital_input
device X2: digital_input

device start_button: sensor {
    subtype: "push_button"
    debounce: 20ms
}

device motor_ctrl: motor {
    rated_speed: 60rpm
    ramp_time: 50ms
}

device sensor_A: sensor {
    subtype: "proximity_sensor"
}

device sensor_B: sensor {
    subtype: "proximity_sensor"
}

relation { from: start_button.out, to: X2.in, via: reports_to }
relation { from: Y0.out, to: motor_ctrl.cmd, via: driven_by }
relation { from: motor_ctrl.position_A, to: sensor_A.sense, via: detects }
relation { from: sensor_A.out, to: X0.in, via: reports_to }
relation { from: motor_ctrl.position_B, to: sensor_B.sense, via: detects }
relation { from: sensor_B.out, to: X1.in, via: reports_to }

[constraints]

timing: task.search.detect must_complete_within 800ms
    reason: "半圈旋转加启动不应超过800ms"

causality: Y0 -> motor_ctrl -> sensor_A
    reason: "电机旋转应能被传感器A检测"

causality: Y0 -> motor_ctrl -> sensor_B
    reason: "电机旋转应能被传感器B检测"

[tasks]

task search:
    step start_motor:
        action: set motor_ctrl.run on
    step detect:
        race:
            branch_A:
                wait: sensor_A == true
                then: goto process_A
            branch_B:
                wait: sensor_B == true
                then: goto process_B
        timeout: 800ms -> goto motor_fault

task process_A:
    step stop_motor:
        action: set motor_ctrl.run off
    step do_work_A:
        action: log "工件在A位置，执行A工艺"
    on_complete: goto ready

task process_B:
    step stop_motor:
        action: set motor_ctrl.run off
    step do_work_B:
        action: log "工件在B位置，执行B工艺"
    on_complete: goto ready

task motor_fault:
    step emergency_stop:
        action: set motor_ctrl.run off
    step alarm:
        action: log "电机旋转超时: 半圈内未检测到任何传感器信号"
        action: log "请检查: 电机是否旋转 / 传感器A,B是否正常 / 工件是否到位"
    on_complete: goto ready

task ready:
    step wait_start:
        wait: start_button == true
        allow_indefinite_wait: true
    on_complete: goto search
"#;
    let ir_json = compile_source_to_json(source).expect("half_rotation example should compile");

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
    let source = r#"
[topology]

device Y0: digital_output
device X0: digital_input

device conveyor: motor {
    rated_speed: 60rpm
    ramp_time: 200ms
}

device sensor_arrived: sensor {
    subtype: "proximity_sensor"
}

relation { from: Y0.out, to: conveyor.cmd, via: driven_by }
relation { from: conveyor.position_A, to: sensor_arrived.sense, via: detects }
relation { from: sensor_arrived.out, to: X0.in, via: reports_to }

[constraints]

causality: Y0 -> conveyor -> sensor_arrived
    reason: "输送带动作后应能在到位传感器观测到"

timing: task.feed must_complete_within 7000ms
    reason: "单次送料必须在节拍窗口内完成"

[tasks]

task feed:
    step start:
        action: set conveyor.run on
    step stabilize:
        delay: 2000ms
    step wait_arrival:
        wait: sensor_arrived == true
        timeout: 3000ms -> goto fault_handler
    step stop:
        action: set conveyor.run off
    on_complete: goto idle

task idle:
    step hold:
        action: log "ready"

task fault_handler:
    step recover:
        action: set conveyor.run off
    step alarm:
        action: log "arrival timeout"
    on_complete: goto idle
"#;
    let ir_json = compile_source_to_json(source).expect("delay_demo example should compile");

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
    let source = r#"
[topology]

device Y0: digital_output
device X0: digital_input
device X1: digital_input
device X2: digital_input

device start_button: sensor {
    subtype: "push_button"
    debounce: 20ms
}

device valve_glue: solenoid_valve {
    response_time: 20ms
}

device cyl_glue: cylinder {
    stroke_time: 150ms
    retract_time: 150ms
}

device sensor_glue_ext: sensor {
    subtype: "limit_switch"
}

device sensor_glue_ret: sensor {
    subtype: "limit_switch"
}

relation { from: start_button.out, to: X2.in, via: reports_to }
relation { from: Y0.out, to: valve_glue.coil, via: driven_by }
relation { from: valve_glue.out, to: cyl_glue.cmd, via: driven_by }
relation { from: cyl_glue.extended, to: sensor_glue_ext.sense, via: detects }
relation { from: sensor_glue_ext.out, to: X0.in, via: reports_to }
relation { from: cyl_glue.retracted, to: sensor_glue_ret.sense, via: detects }
relation { from: sensor_glue_ret.out, to: X1.in, via: reports_to }

[constraints]

causality: Y0 -> valve_glue -> cyl_glue -> sensor_glue_ext
    reason: "涂胶缸伸出应能被传感器检测"

causality: Y0 -> valve_glue -> cyl_glue -> sensor_glue_ret
    reason: "涂胶缸缩回应能被传感器检测"

[tasks]

task glue:
    step glue_cycle:
        repeat 3:
            action: extend cyl_glue
            wait: sensor_glue_ext == true
            timeout: 400ms -> goto fault_handler
            action: retract cyl_glue
            wait: sensor_glue_ret == true
            timeout: 400ms -> goto fault_handler
    on_complete: goto ready

task fault_handler:
    step alarm:
        action: log "涂胶动作超时"
    on_complete: goto ready

task ready:
    step wait_start:
        wait: start_button == true
        allow_indefinite_wait: true
    on_complete: goto glue
"#;
    let ir_json = compile_source_to_json(source).expect("repeat_demo example should compile");

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
        safety_statuses
            .iter()
            .any(|status| { status["rule"].as_str().unwrap_or("").contains("AI1 > 0") }),
        "verification report should include the AI1 window interlock rule"
    );
    assert!(
        safety_statuses
            .iter()
            .any(|status| { status["rule"].as_str().unwrap_or("").contains("Y3.on") }),
        "verification report should include the Y3 command interlock rule"
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
            let rule = status["rule"].as_str().unwrap_or("");
            rule.contains("axis_x.run.on") || rule.contains("axis_x.on")
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
    let source = r#"
[topology]

device Y0: digital_output
device X0: digital_input
device X1: digital_input
device X2: digital_input
device X3: digital_input

device valve_A: solenoid_valve {
    response_time: 20ms
}

device cyl_A: cylinder {
    stroke_time: 300ms,
    retract_time: 300ms
}

device sensor_A_ext: sensor {
    subtype: "limit_switch"
}

device sensor_A_ext2: sensor {
    subtype: "limit_switch"
}

device sensor_A_ret: sensor {
    subtype: "limit_switch"
}

device sensor_A_ret2: sensor {
    subtype: "limit_switch"
}

relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_A_ext.sense, via: detects }
relation { from: sensor_A_ext.out, to: X0.in, via: reports_to }
relation { from: cyl_A.extended, to: sensor_A_ext2.sense, via: detects }
relation { from: sensor_A_ext2.out, to: X1.in, via: reports_to }
relation { from: cyl_A.retracted, to: sensor_A_ret.sense, via: detects }
relation { from: sensor_A_ret.out, to: X2.in, via: reports_to }
relation { from: cyl_A.retracted, to: sensor_A_ret2.sense, via: detects }
relation { from: sensor_A_ret2.out, to: X3.in, via: reports_to }

[constraints]

causality: Y0 -> valve_A -> cyl_A -> sensor_A_ext
causality: Y0 -> valve_A -> cyl_A -> sensor_A_ext2
causality: Y0 -> valve_A -> cyl_A -> sensor_A_ret
causality: Y0 -> valve_A -> cyl_A -> sensor_A_ret2

[tasks]

task main:
    step extend_and_wait:
        action: extend cyl_A
        wait: sensor_A_ext == true AND sensor_A_ext2 == true
        timeout: 800ms -> goto fault

    step retract_and_wait:
        action: retract cyl_A
        wait: sensor_A_ret == true OR sensor_A_ret2 == true
        timeout: 800ms -> goto fault

    on_complete: goto main

task fault:
    step stop:
        action: log "wait AND/OR demo fault"
    on_complete: goto main
"#;
    let ir_json = compile_source_to_json(source).expect("and_or_wait_demo should compile");

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
    let source = r#"
[topology]

device mode_switch: digital_input { subtype: "selector_switch" }

[constraints]

[tasks]

task choose:
    step decide:
        if: mode_switch == true goto process_A else: goto process_B

task process_A:
    step run:
        action: log "process A selected"
    on_complete: goto done

task process_B:
    step run:
        action: log "process B selected"
    on_complete: goto done

task done:
    step finish:
        action: log "workflow complete"
"#;
    let ir_json = compile_source_to_json(source).expect("if_else_demo should compile");

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
    let source = r#"
[topology]

device valve_3pos: solenoid_valve {
    states: [extend, neutral, retract]
}

[constraints]

safety: valve_3pos.extend conflicts_with valve_3pos.retract reason: "3-position valve should not be both extend and retract"

[tasks]

task main:
    step wait_extend:
        wait: valve_3pos == extend
        allow_indefinite_wait: true
    step done:
        action: log "custom states demo complete"
"#;
    let ir_json = compile_source_to_json(source).expect("custom_states_demo should compile");

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
    response_time: 15ms
}

device cyl_glue: cylinder {
    subtype: double_acting,
    stroke: 50mm,
    stroke_time: 120ms,
    retract_time: 110ms
}

device sensor_glue_ext: sensor
device sensor_glue_ret: sensor

relation { from: Y0.out, to: valve_glue.coil, via: driven_by }
relation { from: valve_glue.out, to: cyl_glue.cmd, via: driven_by }
relation { from: cyl_glue.extended, to: sensor_glue_ext.sense, via: detects }
relation { from: sensor_glue_ext.out, to: X0.in, via: reports_to }
relation { from: cyl_glue.retracted, to: sensor_glue_ret.sense, via: detects }
relation { from: sensor_glue_ret.out, to: X1.in, via: reports_to }

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
    response_time: 15ms
}

device cyl_glue: cylinder {
    subtype: double_acting,
    stroke: 50mm,
    stroke_time: 120ms,
    retract_time: 110ms
}

device sensor_glue_ext: sensor
device sensor_glue_ret: sensor

relation { from: Y0.out, to: valve_glue.coil, via: driven_by }
relation { from: valve_glue.out, to: cyl_glue.cmd, via: driven_by }
relation { from: cyl_glue.extended, to: sensor_glue_ext.sense, via: detects }
relation { from: sensor_glue_ext.out, to: X0.in, via: reports_to }
relation { from: cyl_glue.retracted, to: sensor_glue_ret.sense, via: detects }
relation { from: sensor_glue_ret.out, to: X1.in, via: reports_to }

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
fn reports_undefined_cam_table_for_cam_coupling_error_example() {
    let source = read_example("error_cam_missing_table.plc");
    let errors = compile_source_to_json(&source)
        .expect_err("error_cam_missing_table should fail semantic checks");

    assert!(
        errors
            .iter()
            .any(|error| error.contains("cam_coupling cam_xy 的 table 引用了未定义表")),
        "error output should include undefined cam_table reference"
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("ERROR [undefined_reference]")),
        "error output should include undefined_reference class"
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

#[test]
fn parses_three_station_assembly_example_into_verified_ir_json() {
    let source = read_example("three_station_assembly.plc");
    let ir_json =
        compile_source_to_json(&source).expect("three_station_assembly example should compile");
    let safety_level = ir_json["verification"]["safety"]["level"].as_str().unwrap();
    assert!(matches!(safety_level, "完备证明" | "有界验证"));
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
fn parses_hydraulic_bender_example_into_verified_ir_json() {
    let source = read_example("hydraulic_bender.plc");
    let ir_json = compile_source_to_json(&source).expect("hydraulic_bender example should compile");
    let safety_level = ir_json["verification"]["safety"]["level"].as_str().unwrap();
    assert!(matches!(safety_level, "完备证明" | "有界验证"));
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
fn parses_dual_axis_platform_example_into_verified_ir_json() {
    let source = read_example("dual_axis_platform.plc");
    let ir_json =
        compile_source_to_json(&source).expect("dual_axis_platform example should compile");
    let safety_level = ir_json["verification"]["safety"]["level"].as_str().unwrap();
    assert!(matches!(safety_level, "完备证明" | "有界验证"));
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
fn parses_thermal_oven_example_into_verified_ir_json() {
    let source = read_example("thermal_oven.plc");
    let ir_json = compile_source_to_json(&source).expect("thermal_oven example should compile");
    let safety_level = ir_json["verification"]["safety"]["level"].as_str().unwrap();
    assert!(matches!(safety_level, "完备证明" | "有界验证"));
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
fn parses_welding_station_example_into_verified_ir_json() {
    let source = read_example("welding_station.plc");
    let ir_json = compile_source_to_json(&source).expect("welding_station example should compile");
    let safety_level = ir_json["verification"]["safety"]["level"].as_str().unwrap();
    assert!(matches!(safety_level, "完备证明" | "有界验证"));
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
fn parses_axis_stepper_fault_routing_example_into_verified_ir_json() {
    let source = read_example("axis_stepper_fault_routing.plc");
    let ir_json =
        compile_source_to_json(&source).expect("axis_stepper_fault_routing example should compile");

    let transitions = ir_json["state_machine"]["transitions"]
        .as_array()
        .expect("state machine should include transitions array");
    assert!(
        transitions.iter().any(|transition| {
            transition["actions"].as_array().is_some_and(|actions| {
                actions.iter().any(|action| {
                    action["action"] == Value::String("axis_move_relative".to_string())
                })
            })
        }),
        "stepper example should include axis_move_relative action in transitions"
    );
    assert!(
        ir_json["verification"]["safety"]["rule_statuses"]
            .as_array()
            .is_some_and(|rules| !rules.is_empty()),
        "stepper example should bind at least one safety rule"
    );
}

#[test]
fn parses_axis_servo_fault_routing_example_into_verified_ir_json() {
    let source = read_example("axis_servo_fault_routing.plc");
    let ir_json =
        compile_source_to_json(&source).expect("axis_servo_fault_routing example should compile");

    let transitions = ir_json["state_machine"]["transitions"]
        .as_array()
        .expect("state machine should include transitions array");
    assert!(
        transitions.iter().any(|transition| {
            transition["actions"].as_array().is_some_and(|actions| {
                actions.iter().any(|action| {
                    action["action"] == Value::String("axis_move_absolute".to_string())
                })
            })
        }),
        "servo example should include axis_move_absolute action in transitions"
    );
    assert!(
        ir_json["verification"]["safety"]["rule_statuses"]
            .as_array()
            .is_some_and(|rules| !rules.is_empty()),
        "servo example should bind at least one safety rule"
    );
}

#[test]
fn parses_axis_fault_normal_path_example_into_verified_ir_json() {
    let source = read_example("axis_fault_normal_path.plc");
    let ir_json = compile_source_to_json(&source).expect("axis_fault_normal_path should compile");

    let transitions = ir_json["state_machine"]["transitions"]
        .as_array()
        .expect("state machine should include transitions array");
    assert!(
        transitions.iter().any(|transition| {
            transition["actions"].as_array().is_some_and(|actions| {
                actions.iter().any(|action| {
                    action["action"] == Value::String("axis_move_relative".to_string())
                })
            })
        }),
        "normal path example should include axis_move_relative"
    );
}

#[test]
fn parses_axis_fault_recoverable_path_example_with_policy_into_verified_ir_json() {
    let source = read_example("axis_fault_recoverable_path.plc");
    let ir_json =
        compile_source_to_json(&source).expect("axis_fault_recoverable_path should compile");

    let contracts = ir_json["topology"]["axis_fault_contracts"]
        .as_array()
        .expect("topology should include axis fault contracts array");
    assert_eq!(contracts.len(), 1, "recoverable example should declare one policy");
    assert_eq!(
        contracts[0]["severity"],
        Value::String("recoverable".to_string())
    );
}

#[test]
fn parses_axis_fault_nonrecoverable_path_example_with_policy_into_verified_ir_json() {
    let source = read_example("axis_fault_nonrecoverable_path.plc");
    let ir_json =
        compile_source_to_json(&source).expect("axis_fault_nonrecoverable_path should compile");

    let contracts = ir_json["topology"]["axis_fault_contracts"]
        .as_array()
        .expect("topology should include axis fault contracts array");
    assert_eq!(
        contracts[0]["severity"],
        Value::String("non_recoverable".to_string())
    );
}

#[test]
fn parses_axis_fault_safety_path_example_with_policy_into_verified_ir_json() {
    let source = read_example("axis_fault_safety_path.plc");
    let ir_json = compile_source_to_json(&source).expect("axis_fault_safety_path should compile");

    let contracts = ir_json["topology"]["axis_fault_contracts"]
        .as_array()
        .expect("topology should include axis fault contracts array");
    assert_eq!(
        contracts[0]["severity"],
        Value::String("safety".to_string())
    );
}
