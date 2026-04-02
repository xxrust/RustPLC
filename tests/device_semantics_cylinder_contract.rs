use rust_plc::parser::parse_plc;
use rust_plc::semantic::build_state_machine;

#[test]
fn accepts_cylinder_fault_routing_without_timeout_in_semantic_layer() {
    let input = r#"
[topology]
device cyl_a: cylinder

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
    let errors = build_state_machine(&program).expect_err("semantic should reject non-cylinder target");
    let joined = errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(joined.contains("[CYL-001]"), "expected CYL-001, got: {joined}");
}
