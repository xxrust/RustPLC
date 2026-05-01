use rust_plc::parser::parse_plc;
use rust_plc::semantic::build_state_machine;

#[test]
fn accepts_cylinder_fault_routing_without_timeout_in_semantic_layer() {
    let input = r#"
[topology]
device plc_main: plc {
    model_ref: openplc_softplc
}
device valve_a: solenoid_valve
device cyl_a: cylinder
device sensor_ext: sensor
device sensor_ret: sensor

relation { from: plc_main.Y0, to: valve_a.coil, via: driven_by }
relation { from: valve_a.out, to: cyl_a.cmd, via: driven_by }
relation { from: cyl_a.extended, to: sensor_ext.sense, via: detects }
relation { from: sensor_ext.out, to: plc_main.X0, via: reports_to }
relation { from: cyl_a.retracted, to: sensor_ret.sense, via: detects }
relation { from: sensor_ret.out, to: plc_main.X1, via: reports_to }

[constraints]

[tasks]
task main:
    step extend:
        action: extend cyl_a
        on_motion_fault -> fault.motion_fault
        on_safety_fault -> fault.safety_fault
task fault:
    step motion_fault:
    step safety_fault:
"#;

    let program = parse_plc(input).expect("fixture should parse");
    build_state_machine(&program)
        .expect("semantic should accept cylinder fault routing when both branches are declared");
}

#[test]
fn rejects_closed_loop_cylinder_action_when_dual_feedback_is_missing_before_bridge() {
    let input = r#"
[topology]
device plc_main: plc {
    model_ref: openplc_softplc
}
device valve_a: solenoid_valve
device cyl_a: cylinder
device sensor_ext: sensor

relation { from: plc_main.Y0, to: valve_a.coil, via: driven_by }
relation { from: valve_a.out, to: cyl_a.cmd, via: driven_by }
relation { from: cyl_a.extended, to: sensor_ext.sense, via: detects }
relation { from: sensor_ext.out, to: plc_main.X0, via: reports_to }

[constraints]

[tasks]
task main:
    step extend:
        action: extend cyl_a
        timeout: 50ms -> goto fault.timeout
task fault:
    step timeout:
"#;

    let program = parse_plc(input).expect("fixture should parse");
    let errors =
        build_state_machine(&program).expect_err("semantic should reject incomplete feedback");
    let joined = errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        joined.contains("[CYL-004]"),
        "expected CYL-004, got: {joined}"
    );
}

#[test]
fn rejects_cylinder_action_target_that_is_not_cylinder() {
    let input = r#"
[topology]
device motor_a: motor

[constraints]

[tasks]
task main:
    step extend:
        action: extend motor_a
"#;

    let program = parse_plc(input).expect("fixture should parse");
    let errors =
        build_state_machine(&program).expect_err("semantic should reject non-cylinder target");
    let joined = errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        joined.contains("[CYL-001]"),
        "expected CYL-001, got: {joined}"
    );
}
