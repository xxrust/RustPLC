use rust_plc::codegen::st::{StCodegenConfig, StCodegenError, generate_st};
use rust_plc::parser::parse_plc;
use rust_plc::semantic::{build_constraint_set, build_state_machine, build_topology_graph};
use rust_plc::verification::safety::verify_safety;

const PLC_SRI_CONFLICT_FIXTURE: &str = r#"
[topology]

device Y0: digital_output
device valve_feed: solenoid_valve
device cyl_feed: cylinder
device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default, motion_param_set: stepper_default_fast }

relation { from: Y0.out, to: valve_feed.coil, via: driven_by }
relation { from: valve_feed.out, to: cyl_feed.cmd, via: driven_by }

resource slide_pick_zone: semantic_resource { mode: exclusive }

[constraints]

claim: cyl_feed.extended occupies slide_pick_zone
claim: action_tag arm_pick_to_slide occupies slide_pick_zone

[tasks]

task feeder:
    step extend:
        action: extend cyl_feed
    step done:
        action: log "done"

task arm:
    step move:
        action: axis.move_relative(axis_x, distance: 5, speed: 10)
            timeout: 100ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
            semantic_tag: arm_pick_to_slide
    step done:
        action: log "done"

task fault:
    step timeout:
        action: log "timeout"
    step reject:
        action: log "reject"
    step motion_fault:
        action: log "motion"
    step safety_fault:
        action: log "safety"
"#;

const PLC_SRI_NO_CONFLICT_FIXTURE: &str = r#"
[topology]

device Y0: digital_output
device valve_feed: solenoid_valve
device cyl_feed: cylinder
device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default, motion_param_set: stepper_default_fast }

relation { from: Y0.out, to: valve_feed.coil, via: driven_by }
relation { from: valve_feed.out, to: cyl_feed.cmd, via: driven_by }

resource slide_pick_zone: semantic_resource { mode: exclusive }

[constraints]

claim: cyl_feed.extended occupies slide_pick_zone
claim: action_tag arm_pick_to_slide occupies slide_pick_zone

[tasks]

task feeder:
    step idle:
        action: log "idle"

task arm:
    step move:
        action: axis.move_relative(axis_x, distance: 5, speed: 10)
            timeout: 100ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
            semantic_tag: arm_pick_to_slide
    step done:
        action: log "done"

task fault:
    step timeout:
        action: log "timeout"
    step reject:
        action: log "reject"
    step motion_fault:
        action: log "motion"
    step safety_fault:
        action: log "safety"
"#;

const PLC_SRI_MULTI_RESOURCE_NO_CONFLICT_FIXTURE: &str = r#"
[topology]

device Y0: digital_output
device Y1: digital_output
device valve_feed: solenoid_valve
device valve_swing: solenoid_valve
device cyl_feed: cylinder
device cyl_swing: cylinder
device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default, motion_param_set: stepper_default_fast }

relation { from: Y0.out, to: valve_feed.coil, via: driven_by }
relation { from: valve_feed.out, to: cyl_feed.cmd, via: driven_by }
relation { from: Y1.out, to: valve_swing.coil, via: driven_by }
relation { from: valve_swing.out, to: cyl_swing.cmd, via: driven_by }

resource slide_feed_zone: semantic_resource { mode: exclusive }
resource slide_swing_zone: semantic_resource { mode: exclusive }

[constraints]

claim: cyl_feed.extended occupies slide_feed_zone
claim: cyl_swing.extended occupies slide_swing_zone
claim: action_tag arm_turn_to_slide occupies slide_feed_zone
claim: action_tag arm_turn_to_slide occupies slide_swing_zone

[tasks]

task feeder:
    step extend:
        action: extend cyl_feed
    step done:
        action: log "feed_done"

task swing:
    step extend:
        action: extend cyl_swing
    step done:
        action: log "swing_done"

task driver:
    step idle:
        if: 1 == 0 goto arm.move else: goto driver.idle

task arm:
    step move:
        action: axis.move_relative(axis_x, distance: 5, speed: 10)
            timeout: 100ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
            semantic_tag: arm_turn_to_slide

task fault:
    step timeout:
        action: log "timeout"
    step reject:
        action: log "reject"
    step motion_fault:
        action: log "motion"
    step safety_fault:
        action: log "safety"
"#;

#[test]
fn safety_reports_semantic_resource_conflict() {
    let program = parse_plc(PLC_SRI_CONFLICT_FIXTURE).expect("fixture should parse");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    let errors = verify_safety(&program, &constraints, &state_machine)
        .expect_err("resource conflict should be reported");
    assert!(errors.iter().any(|error| {
        error
            .constraint
            .contains("semantic_resource slide_pick_zone exclusive")
            && error.reason.contains("slide_pick_zone")
    }));
}

#[test]
fn safety_accepts_non_overlapping_semantic_resource_claims() {
    let program = parse_plc(PLC_SRI_NO_CONFLICT_FIXTURE).expect("fixture should parse");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    verify_safety(&program, &constraints, &state_machine)
        .expect("no state claim is active, so interlock should pass");
}

#[test]
fn semantic_accepts_shared_action_tag_across_multiple_resources() {
    let program =
        parse_plc(PLC_SRI_MULTI_RESOURCE_NO_CONFLICT_FIXTURE).expect("fixture should parse");

    build_constraint_set(&program).expect("multiple resources may reference the same action_tag");
    build_state_machine(&program).expect("state machine should still build with shared action_tag");
}

#[test]
fn st_codegen_rejects_semantic_resource_interlock() {
    let program = parse_plc(PLC_SRI_NO_CONFLICT_FIXTURE).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    let errors = generate_st(
        &topology,
        &constraints,
        &state_machine,
        &StCodegenConfig::default(),
    )
    .expect_err("ST backend should reject SRI");
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, StCodegenError::SemanticResourceInterlockUnsupported))
    );
}
