use rust_plc::parser::parse_plc;
use rust_plc::semantic::{build_topology_graph, preprocess_program};

#[test]
fn rejects_axis_move_alias_fields_with_stable_axis_013_code() {
    let input = r#"
[topology]
device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default }

[constraints]

[tasks]
task main:
    step start:
        action: axis.move_relative(axis_x, distance: 10, vel: 2)
            timeout: 500ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
task fault:
    step timeout:
    step reject:
    step motion_fault:
    step safety_fault:
"#;

    let err = parse_plc(input).expect_err("axis.move 参数别名应被拒绝");
    let rendered = err.to_string();
    assert!(
        rendered.contains("[AXIS-013]"),
        "unexpected error: {rendered}"
    );
    assert!(rendered.contains("vel"), "unexpected error: {rendered}");
}

#[test]
fn rejects_axis_device_non_whitelist_fields_with_stable_axp_006_code() {
    let input = r#"
[topology]
device axis_x: stepper_motor {
    model_ref: stepper_generic,
    config_ref: stepper_default,
    max_speed: 1200
}

[constraints]

[tasks]
task main:
    step idle:
"#;

    let program =
        parse_plc(input).expect("parser should accept known legacy key then semantic gate rejects");
    let expanded = preprocess_program(&program).expect("preprocess should succeed");
    let errors =
        build_topology_graph(&expanded).expect_err("non-whitelist axis fields should fail");
    let joined = errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(joined.contains("[AXP-006]"), "unexpected errors: {joined}");
    assert!(joined.contains("max_speed"), "unexpected errors: {joined}");
}
