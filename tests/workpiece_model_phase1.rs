use runtime_core::{Action, Instr};
use rust_plc::codegen::st::{StCodegenConfig, StCodegenError, generate_st};
use rust_plc::parser::parse_plc;
use rust_plc::runtime_bridge::state_machine_to_runtime_program;
use rust_plc::semantic::{build_constraint_set, build_state_machine, build_topology_graph};
use rust_plc::verification::verify_all;
use std::fs;

const PLC_WORKPIECE_PHASE1: &str = r#"
[topology]

workpiece part: workpiece_type {
    properties: [inspected: bool]
    normal_terminal_states: [finished]
    abnormal_terminal_states: [rejected]
    ingress_sites: [infeed]
    normal_egress_sites: [outfeed]
    abnormal_egress_sites: [reject_bin]
}

location infeed: workpiece_location { capacity: 1 }
location outfeed: workpiece_location { capacity: 1 }
location reject_bin: workpiece_location { capacity: 1 }
holder arm: workpiece_holder { capacity: 1 }

[constraints]

[tasks]

task transfer_part:
    step pick:
        effect: acquire holder arm from infeed
    step place:
        effect: transfer from arm to outfeed
    step done:
        effect: finish workpiece at outfeed as finished
    on_complete: goto sink

task sink:
    step idle:
        action: log "done"
"#;

const PLC_WORKPIECE_MULTI_TYPE_EFFECT: &str = r#"
[topology]

workpiece part_a: workpiece_type {
    normal_terminal_states: [finished]
    ingress_sites: [infeed]
    normal_egress_sites: [outfeed]
}

workpiece part_b: workpiece_type {
    normal_terminal_states: [finished]
    ingress_sites: [infeed]
    normal_egress_sites: [outfeed]
}

location infeed: workpiece_location { capacity: 1 }
location outfeed: workpiece_location { capacity: 1 }
holder arm: workpiece_holder { capacity: 1 }

[constraints]

[tasks]

task transfer_part:
    step pick:
        effect: acquire holder arm from infeed
"#;

#[test]
fn workpiece_phase1_builds_ir_and_verifies() {
    let program = parse_plc(PLC_WORKPIECE_PHASE1).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    assert_eq!(constraints.workpiece_types.len(), 1);
    assert_eq!(constraints.workpiece_sites.len(), 3);
    assert_eq!(constraints.workpiece_holders.len(), 1);
    assert_eq!(
        state_machine
            .transitions
            .iter()
            .flat_map(|transition| transition.effects.iter())
            .count(),
        3
    );

    verify_all(&program, &topology, &constraints, &state_machine)
        .expect("workpiece phase1 fixture should pass verification");
}

#[test]
fn workpiece_phase1_rejects_multiple_types_when_effects_are_used() {
    let program = parse_plc(PLC_WORKPIECE_MULTI_TYPE_EFFECT).expect("fixture should parse");
    let errors = build_constraint_set(&program).expect_err("multi-type effect should fail");
    if errors
        .iter()
        .any(|error| error.to_string().contains("single-type"))
    {
        return;
    }
    assert!(
        errors
            .iter()
            .any(|error| error.to_string().contains("单工件类型"))
    );
}

#[test]
fn runtime_bridge_lowers_phase1_example_into_runtime_program() {
    let source = fs::read_to_string("examples/workpiece_phase1_transfer.plc")
        .expect("phase1 example should be readable");
    let program = parse_plc(&source).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    let runtime_program =
        state_machine_to_runtime_program(&topology, &constraints, &state_machine, 10)
            .expect("runtime bridge should lower phase1 workpiece model");

    assert_eq!(runtime_program.workpiece_types.len(), 1);
    assert_eq!(runtime_program.workpiece_sites.len(), 3);
    assert_eq!(runtime_program.workpiece_holders.len(), 1);
    assert_eq!(runtime_program.workpiece_types[0].name, "part");
    assert_eq!(runtime_program.workpiece_holders[0].name, "arm");

    let lowered_actions = runtime_program
        .tasks
        .iter()
        .flat_map(|task| task.steps.iter())
        .filter_map(|step| match step.instr {
            Instr::Action { actions, .. } => Some(actions),
            _ => None,
        })
        .flat_map(|actions| actions.iter())
        .collect::<Vec<_>>();
    assert!(lowered_actions.iter().any(|action| matches!(
        action,
        Action::WorkpieceAcquire {
            workpiece_type,
            holder,
            from,
        } if *workpiece_type == "part" && *holder == "arm" && *from == "infeed"
    )));
    assert!(lowered_actions.iter().any(|action| matches!(
        action,
        Action::WorkpieceTransfer { from, to } if *from == "arm" && *to == "outfeed"
    )));
    assert!(lowered_actions.iter().any(|action| matches!(
        action,
        Action::WorkpieceFinish { at, terminal_state }
            if *at == "outfeed" && *terminal_state == "finished"
    )));
}

#[test]
fn st_codegen_rejects_workpiece_model_for_now() {
    let program = parse_plc(PLC_WORKPIECE_PHASE1).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    let errors = generate_st(
        &topology,
        &constraints,
        &state_machine,
        &StCodegenConfig::default(),
    )
    .expect_err("ST backend should reject workpiece model");
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, StCodegenError::WorkpieceModelUnsupported))
    );
}
