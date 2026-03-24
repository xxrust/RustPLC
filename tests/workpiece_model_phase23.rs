use io_traits::{AnalogInputId, AnalogOutputId, DigitalInputId, DigitalOutputId, Io, Tick};
use runtime_core::{Action, Instr, Runtime, WorkpieceSiteKind, WorkpieceTerminalStatus};
use rust_plc::ast::EffectKind;
use rust_plc::parser::parse_plc;
use rust_plc::runtime_bridge::{BridgeError, state_machine_to_runtime_program};
use rust_plc::semantic::{build_constraint_set, build_state_machine, build_topology_graph};
use rust_plc::verification::{WarningLevel, verify_all};
use std::fs;

struct MemIo {
    tick: Tick,
    di: [bool; 4],
    do_: [bool; 4],
    ai: [f32; 4],
    ao: [f32; 4],
}

impl MemIo {
    fn new() -> Self {
        Self {
            tick: Tick(0),
            di: [false; 4],
            do_: [false; 4],
            ai: [0.0; 4],
            ao: [0.0; 4],
        }
    }
}

impl Io for MemIo {
    fn tick(&self) -> Tick {
        self.tick
    }

    fn advance_tick(&mut self) {
        self.tick.0 += 1;
    }

    fn read_digital_input(&self, id: DigitalInputId) -> bool {
        self.di[id.0 as usize]
    }

    fn read_analog_input(&self, id: AnalogInputId) -> f32 {
        self.ai[id.0 as usize]
    }

    fn write_digital_output(&mut self, id: DigitalOutputId, value: bool) {
        self.do_[id.0 as usize] = value;
    }

    fn write_analog_output(&mut self, id: AnalogOutputId, value: f32) {
        self.ao[id.0 as usize] = value;
    }
}

const PLC_WORKPIECE_MOUNT_UNMOUNT: &str = r#"
[topology]

workpiece rod: workpiece_type {
    normal_terminal_states: [finished]
    ingress_sites: [steel_plate.slot[*]]
    normal_egress_sites: [outfeed]
}

carrier steel_plate: workpiece_carrier { slots: 2 }
location outfeed: workpiece_location { capacity: 1 }

[constraints]

[tasks]

task load_plate:
    step mount_a:
        effect: mount rod on steel_plate.slot[0]
    step raise:
        effect: transform carrier steel_plate to frame cut_height
    step unload_a:
        effect: unmount rod from steel_plate.slot[0] to outfeed
    step done:
        effect: finish workpiece at outfeed as finished
    on_complete: goto sink

task sink:
    step idle:
        action: log "done"
"#;

const PLC_WORKPIECE_SPLIT_RUNTIME: &str = r#"
[topology]

workpiece rod: workpiece_type {
    allows: [split_into(slice)]
}

workpiece slice: workpiece_type {
    derived_from: [rod]
}

carrier plate: workpiece_carrier { slots: 1 }
location cut_zone: workpiece_location { capacity: 4 }

[constraints]

[tasks]

task process:
    step load:
        effect: mount rod on plate.slot[0]
    step unload:
        effect: unmount rod from plate.slot[0] to cut_zone
    step cut:
        effect: split rod into slice count 4 consumed
        goto sink

task sink:
    step idle:
        action: log "done"
"#;

const PLC_WORKPIECE_SPLIT_MERGE_RUNTIME: &str = r#"
[topology]

workpiece rod: workpiece_type {
    allows: [split_into(slice)]
}

workpiece slice: workpiece_type {
    derived_from: [rod]
}

workpiece module: workpiece_type {
    derived_from: [merge(slice, slice)]
}

carrier plate: workpiece_carrier { slots: 1 }
location cut_zone: workpiece_location { capacity: 4 }

[constraints]

[tasks]

task process:
    step load:
        effect: mount rod on plate.slot[0]
    step unload:
        effect: unmount rod from plate.slot[0] to cut_zone
    step cut:
        effect: split rod into slice count 4 consumed
    step assemble:
        effect: merge [slice_a, slice_b] into module consumed_inputs
        goto sink

task sink:
    step idle:
        action: log "done"
"#;

const PLC_WORKPIECE_INVALID_SLOT_ARITY: &str = r#"
[topology]

workpiece die: workpiece_type {
    ingress_sites: [tray_scan.slot[*]]
}

carrier tray_scan: workpiece_carrier {
    layout: grid(rows: 2, cols: 2)
}
holder nozzle: workpiece_holder { capacity: 1 }

[constraints]

[tasks]

task scan:
    step pick:
        effect: acquire holder nozzle from tray_scan.slot[0]
"#;

const PLC_WORKPIECE_VERIFY_UNDERFLOW: &str = r#"
[topology]

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

task transfer_part:
    step place:
        effect: transfer from arm to outfeed
    step done:
        effect: finish workpiece at outfeed as finished
"#;

const PLC_WORKPIECE_VERIFY_CAPACITY: &str = r#"
[topology]

workpiece part: workpiece_type {
    ingress_sites: [infeed, plate.slot[*]]
}

carrier plate: workpiece_carrier { slots: 1 }
location infeed: workpiece_location { capacity: 1 }
holder arm: workpiece_holder { capacity: 1 }

[constraints]

[tasks]

task transfer_part:
    step mount_a:
        effect: mount part on plate.slot[0]
    step pick:
        effect: acquire holder arm from infeed
    step place:
        effect: transfer from arm to plate.slot[0]
    step done:
        action: log "overflow"
"#;

const PLC_WORKPIECE_VERIFY_DANGLING: &str = r#"
[topology]

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

task transfer_part:
    step pick:
        effect: acquire holder arm from infeed
    step done:
        action: log "dangling"
"#;

const PLC_WORKPIECE_VERIFY_UNMOUNT_UNDERFLOW: &str = r#"
[topology]

workpiece rod: workpiece_type {
    normal_terminal_states: [finished]
    ingress_sites: [plate.slot[*]]
    normal_egress_sites: [outfeed]
}

carrier plate: workpiece_carrier { slots: 1 }
location outfeed: workpiece_location { capacity: 1 }

[constraints]

[tasks]

task unload_part:
    step unload:
        effect: unmount rod from plate.slot[0] to outfeed
    step done:
        action: log "empty-slot"
"#;

const PLC_WORKPIECE_VERIFY_MOUNTED_CONSISTENCY: &str = r#"
[topology]

workpiece rod: workpiece_type {
    ingress_sites: [plate.slot[*]]
}

carrier plate: workpiece_carrier { slots: 1 }
location outfeed: workpiece_location { capacity: 1 }

[constraints]

[tasks]

task inspect_part:
    step mount_part:
        effect: mount rod on plate.slot[0]
    step rotate:
        effect: transform carrier plate to frame inspection
    step illegal_transfer:
        effect: transfer from plate.slot[0] to outfeed
    step done:
        action: log "unexpected"
"#;

const PLC_WORKPIECE_VERIFY_UNUSED_INGRESS: &str = r#"
[topology]

workpiece part: workpiece_type {
    ingress_sites: [infeed]
}

location infeed: workpiece_location { capacity: 1 }

[constraints]

[tasks]

task idle_flow:
    step idle:
        action: log "noop"
"#;

const PLC_WORKPIECE_VERIFY_DEAD_CONTRACTS: &str = r#"
[topology]

workpiece part: workpiece_type {
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

const PLC_WORKPIECE_VERIFY_UNUSED_CONTRACTS: &str = r#"
[topology]

workpiece rod: workpiece_type {
    ingress_sites: [infeed]
    allows: [split_into(slice)]
}

workpiece slice: workpiece_type {
    derived_from: [rod]
}

workpiece module: workpiece_type {
    derived_from: [merge(slice, slice)]
}

location infeed: workpiece_location { capacity: 1 }

[constraints]

[tasks]

task idle_flow:
    step idle:
        action: log "noop"
"#;

const PLC_WORKPIECE_VERIFY_SPLIT_WITHOUT_SOURCE_INTRO: &str = r#"
[topology]

workpiece rod: workpiece_type {
    allows: [split_into(slice)]
}

workpiece slice: workpiece_type {
    derived_from: [rod]
}

[constraints]

[tasks]

task process:
    step cut:
        effect: split rod into slice count 2 consumed
    step done:
        action: log "noop"
"#;

const PLC_WORKPIECE_VERIFY_MERGE_WITHOUT_INPUT_INTRO: &str = r#"
[topology]

workpiece cell: workpiece_type {}

workpiece module: workpiece_type {
    derived_from: [merge(cell, cell)]
}

[constraints]

[tasks]

task process:
    step assemble:
        effect: merge [cell_a, cell_b] into module consumed_inputs
    step done:
        action: log "noop"
"#;

const PLC_WORKPIECE_INVALID_DUPLICATE_TERMINAL_STATE: &str = r#"
[topology]

workpiece part: workpiece_type {
    normal_terminal_states: [finished, finished]
    normal_egress_sites: [outfeed]
}

location outfeed: workpiece_location { capacity: 1 }

[constraints]

[tasks]
"#;

const PLC_WORKPIECE_INVALID_OVERLAPPING_TERMINAL_STATE: &str = r#"
[topology]

workpiece part: workpiece_type {
    normal_terminal_states: [done]
    abnormal_terminal_states: [done]
    normal_egress_sites: [outfeed]
    abnormal_egress_sites: [reject_bin]
}

location outfeed: workpiece_location { capacity: 1 }
location reject_bin: workpiece_location { capacity: 1 }

[constraints]

[tasks]
"#;

const PLC_WORKPIECE_INVALID_CONSUMED_TERMINAL_STATE: &str = r#"
[topology]

workpiece part: workpiece_type {
    normal_terminal_states: [consumed]
    normal_egress_sites: [outfeed]
}

location outfeed: workpiece_location { capacity: 1 }

[constraints]

[tasks]
"#;

const PLC_WORKPIECE_INVALID_DUPLICATE_INGRESS_SITE: &str = r#"
[topology]

workpiece part: workpiece_type {
    ingress_sites: [infeed, infeed]
}

location infeed: workpiece_location { capacity: 1 }

[constraints]

[tasks]
"#;

const PLC_WORKPIECE_INVALID_OVERLAPPING_EGRESS_SITE: &str = r#"
[topology]

workpiece part: workpiece_type {
    normal_terminal_states: [finished]
    abnormal_terminal_states: [rejected]
    normal_egress_sites: [shared_outfeed]
    abnormal_egress_sites: [shared_outfeed]
}

location shared_outfeed: workpiece_location { capacity: 1 }

[constraints]

[tasks]
"#;

const PLC_WORKPIECE_INVALID_DUPLICATE_SPLIT_RULE: &str = r#"
[topology]

workpiece rod: workpiece_type {
    allows: [split_into(slice), split_into(slice)]
}

workpiece slice: workpiece_type {
    derived_from: [rod]
}

[constraints]

[tasks]
"#;

const PLC_WORKPIECE_INVALID_DUPLICATE_DERIVATION_RULE: &str = r#"
[topology]

workpiece rod: workpiece_type {}

workpiece slice: workpiece_type {
    derived_from: [rod, rod]
}

[constraints]

[tasks]
"#;

const PLC_WORKPIECE_INVALID_SPLIT_TARGET_MISSING_DERIVED_FROM: &str = r#"
[topology]

workpiece rod: workpiece_type {
    allows: [split_into(slice)]
}

workpiece slice: workpiece_type {}

[constraints]

[tasks]
"#;

const PLC_WORKPIECE_INVALID_DERIVED_FROM_SOURCE_MISSING_SPLIT_RULE: &str = r#"
[topology]

workpiece rod: workpiece_type {}

workpiece slice: workpiece_type {
    derived_from: [rod]
}

[constraints]

[tasks]
"#;

const PLC_WORKPIECE_INVALID_DUPLICATE_MERGE_RULE_BY_PERMUTATION: &str = r#"
[topology]

workpiece cell: workpiece_type {}
workpiece shell: workpiece_type {}

workpiece module: workpiece_type {
    derived_from: [merge(cell, shell), merge(shell, cell)]
}

[constraints]

[tasks]
"#;

const PLC_WORKPIECE_INVALID_AMBIGUOUS_MERGE_RULE_ARITY: &str = r#"
[topology]

workpiece cell: workpiece_type {}
workpiece shell: workpiece_type {}

workpiece module: workpiece_type {
    derived_from: [merge(cell, cell), merge(shell, shell)]
}

[constraints]

[tasks]
"#;

const PLC_WORKPIECE_INVALID_DUPLICATE_ENUM_VALUE: &str = r#"
[topology]

workpiece part: workpiece_type {
    properties: [grade: enum(a, a, b)]
}

[constraints]

[tasks]
"#;

const PLC_WORKPIECE_INVALID_SPLIT_COUNT_ZERO: &str = r#"
[topology]

workpiece rod: workpiece_type {
    allows: [split_into(slice)]
}

workpiece slice: workpiece_type {
    derived_from: [rod]
}

[constraints]

[tasks]

task cut:
    step do_cut:
        effect: split rod into slice count 0 consumed
"#;

const PLC_WORKPIECE_INVALID_STEP_SPLIT_UNDECLARED_TARGET: &str = r#"
[topology]

workpiece rod: workpiece_type {
    allows: [split_into(chip)]
}

workpiece chip: workpiece_type {
    derived_from: [rod]
}

workpiece slice: workpiece_type {}

[constraints]

[tasks]

task cut:
    step do_cut:
        effect: split rod into slice count 1 consumed
"#;

const PLC_WORKPIECE_INVALID_STEP_MERGE_ARITY_MISMATCH: &str = r#"
[topology]

workpiece cell: workpiece_type {}

workpiece module: workpiece_type {
    derived_from: [merge(cell, cell, cell)]
}

[constraints]

[tasks]

task assemble:
    step merge_cells:
        effect: merge [cell_a, cell_b] into module consumed_inputs
"#;

const PLC_WORKPIECE_INVALID_FINISH_UNDECLARED_TERMINAL: &str = r#"
[topology]

workpiece part: workpiece_type {
    normal_terminal_states: [finished]
    normal_egress_sites: [outfeed]
}

location outfeed: workpiece_location { capacity: 1 }

[constraints]

[tasks]

task finish_part:
    step done:
        effect: finish workpiece at outfeed as scrapped
"#;

const PLC_WORKPIECE_INVALID_FINISH_WRONG_EGRESS_BUCKET: &str = r#"
[topology]

workpiece part: workpiece_type {
    normal_terminal_states: [finished]
    abnormal_terminal_states: [rejected]
    normal_egress_sites: [outfeed]
    abnormal_egress_sites: [reject_bin]
}

location outfeed: workpiece_location { capacity: 1 }
location reject_bin: workpiece_location { capacity: 1 }

[constraints]

[tasks]

task finish_part:
    step done:
        effect: finish workpiece at outfeed as rejected
"#;

const PLC_WORKPIECE_INVALID_MULTI_TYPE_UNTYPED_TRANSFER: &str = r#"
[topology]

workpiece part: workpiece_type {
    ingress_sites: [infeed]
}

workpiece rod: workpiece_type {
    ingress_sites: [infeed]
}

location infeed: workpiece_location { capacity: 1 }
location outfeed: workpiece_location { capacity: 1 }
holder arm: workpiece_holder { capacity: 1 }

[constraints]

[tasks]

task move_part:
    step pick:
        effect: acquire holder arm from infeed
    step place:
        effect: transfer from arm to outfeed
"#;

fn read_example_source(file_name: &str) -> String {
    fs::read_to_string(format!("examples/{file_name}"))
        .unwrap_or_else(|err| panic!("failed to read example {file_name}: {err}"))
}

fn collect_runtime_actions(program: &runtime_core::Program<'static>) -> Vec<runtime_core::Action> {
    program
        .tasks
        .iter()
        .flat_map(|task| task.steps.iter())
        .flat_map(|step| match step.instr {
            Instr::Action { actions, .. } => actions.to_vec(),
            _ => Vec::new(),
        })
        .collect()
}

#[test]
fn workpiece_carrier_slot_transfer_builds_ir_and_verifies() {
    let source = read_example_source("workpiece_carrier_slot_transfer.plc");
    let program = parse_plc(&source).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    assert_eq!(constraints.workpiece_carriers.len(), 1);
    assert!(state_machine.transitions.iter().any(|transition| {
        transition.effects.iter().any(|effect| {
            matches!(effect, rust_plc::ir::WorkpieceEffect::Acquire { from, .. } if from == "tray_a.slot[0]")
        })
    }));

    verify_all(&program, &topology, &constraints, &state_machine)
        .expect("carrier slot transfer fixture should pass verification");

    let runtime_program =
        state_machine_to_runtime_program(&topology, &constraints, &state_machine, 10)
            .expect("runtime bridge should lower carrier slot endpoints");
    let slot_sites = runtime_program
        .workpiece_sites
        .iter()
        .filter(|site| site.name.starts_with("tray_a.slot["))
        .collect::<Vec<_>>();
    assert_eq!(slot_sites.len(), 4);
    assert!(slot_sites.iter().all(|site| {
        site.capacity == 1 && matches!(site.kind, WorkpieceSiteKind::CarrierLocation)
    }));
    assert_eq!(
        runtime_program.workpiece_types[0].ingress_sites,
        &[
            "tray_a.slot[0]",
            "tray_a.slot[1]",
            "tray_a.slot[2]",
            "tray_a.slot[3]"
        ]
    );
    assert!(
        collect_runtime_actions(&runtime_program)
            .iter()
            .any(|action| {
                matches!(
                    action,
                    Action::WorkpieceAcquire {
                        workpiece_type,
                        holder,
                        from,
                    } if *workpiece_type == "part" && *holder == "arm" && *from == "tray_a.slot[0]"
                )
            })
    );
}

#[test]
fn runtime_executes_workpiece_carrier_slot_transfer_example_end_to_end() {
    let source = read_example_source("workpiece_carrier_slot_transfer.plc");
    let program = parse_plc(&source).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    let runtime_program =
        state_machine_to_runtime_program(&topology, &constraints, &state_machine, 10)
            .expect("runtime bridge should lower carrier slot example");

    let mut io = MemIo::new();
    let mut runtime = Runtime::new(&runtime_program).expect("runtime should initialize");
    runtime
        .tick(&mut io)
        .expect("carrier slot example should execute");

    assert_eq!(runtime.workpiece_tokens().active_tokens(), 0);
    let finished = runtime
        .workpiece_tokens()
        .token(0)
        .expect("seeded token should remain traceable after finish");
    assert_eq!(finished.current_location, "outfeed");
    assert_eq!(finished.mounted_slot, None);
    assert!(!finished.active);
    assert_eq!(
        finished.terminal_status,
        Some(WorkpieceTerminalStatus::TerminalState { state: "finished" })
    );
}

#[test]
fn workpiece_mount_unmount_and_transform_lower_into_ir() {
    let program = parse_plc(PLC_WORKPIECE_MOUNT_UNMOUNT).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    assert_eq!(constraints.workpiece_carriers.len(), 1);
    assert_eq!(constraints.workpiece_types.len(), 1);
    let effects = state_machine
        .transitions
        .iter()
        .flat_map(|transition| transition.effects.iter())
        .collect::<Vec<_>>();
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, rust_plc::ir::WorkpieceEffect::Mount { .. }))
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, rust_plc::ir::WorkpieceEffect::Unmount { .. }))
    );
    assert!(effects.iter().any(|effect| matches!(
        effect,
        rust_plc::ir::WorkpieceEffect::TransformCarrier { .. }
    )));

    let summary = verify_all(&program, &topology, &constraints, &state_machine)
        .expect("mount/transform/unmount flow should preserve mounted workpiece consistency");
    assert!(!summary.safety.warnings.iter().any(|warning| {
        warning.message.contains("ingress site")
            || warning.message.contains("terminal state")
            || warning.message.contains("egress site")
    }));
}

#[test]
fn runtime_bridge_lowers_mount_unmount_and_transform_actions() {
    let program = parse_plc(PLC_WORKPIECE_MOUNT_UNMOUNT).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    let runtime_program =
        state_machine_to_runtime_program(&topology, &constraints, &state_machine, 10)
            .expect("runtime bridge should lower phase2 carrier actions");
    let actions = collect_runtime_actions(&runtime_program);
    assert!(actions.iter().any(|action| matches!(
        action,
        Action::WorkpieceMount {
            workpiece_type,
            slot,
        } if *workpiece_type == "rod" && *slot == "steel_plate.slot[0]"
    )));
    assert!(actions.iter().any(|action| matches!(
        action,
        Action::WorkpieceUnmount {
            workpiece_type,
            slot,
            to,
        } if *workpiece_type == "rod" && *slot == "steel_plate.slot[0]" && *to == "outfeed"
    )));
    assert!(actions.iter().any(|action| matches!(
        action,
        Action::WorkpieceTransformCarrier { carrier, frame }
            if *carrier == "steel_plate" && *frame == "cut_height"
    )));
    assert!(runtime_program.workpiece_sites.iter().any(|site| {
        site.name == "steel_plate.slot[1]"
            && site.capacity == 1
            && matches!(site.kind, WorkpieceSiteKind::CarrierLocation)
    }));
}

#[test]
fn runtime_executes_mount_unmount_and_transform_actions_end_to_end() {
    let program = parse_plc(PLC_WORKPIECE_MOUNT_UNMOUNT).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    let runtime_program =
        state_machine_to_runtime_program(&topology, &constraints, &state_machine, 10)
            .expect("runtime bridge should lower phase2 carrier actions");

    let mut io = MemIo::new();
    let mut runtime = Runtime::new(&runtime_program).expect("runtime should initialize");
    runtime
        .tick(&mut io)
        .expect("mount/transform/unmount flow should execute");

    assert_eq!(runtime.workpiece_tokens().active_tokens(), 0);
    let finished = runtime
        .workpiece_tokens()
        .token(0)
        .expect("mounted token should remain traceable after finish");
    assert_eq!(finished.current_location, "outfeed");
    assert_eq!(finished.mounted_slot, None);
    assert!(!finished.active);
    assert_eq!(
        finished.terminal_status,
        Some(WorkpieceTerminalStatus::TerminalState { state: "finished" })
    );
}

#[test]
fn runtime_bridge_lowers_split_effect_into_runtime_action() {
    let program = parse_plc(PLC_WORKPIECE_SPLIT_RUNTIME).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    let runtime_program =
        state_machine_to_runtime_program(&topology, &constraints, &state_machine, 10)
            .expect("runtime bridge should lower split actions");

    assert!(runtime_program.tasks.iter().any(|task| {
        task.steps.iter().any(|step| {
            let Instr::Action { actions, .. } = step.instr else {
                return false;
            };
            actions.iter().any(|action| {
                matches!(
                    action,
                    Action::WorkpieceSplit {
                        source_type,
                        target_type,
                        count,
                        consumed
                    } if *source_type == "rod"
                        && *target_type == "slice"
                        && *count == 4
                        && *consumed
                )
            })
        })
    }));
}

#[test]
fn runtime_executes_split_action_end_to_end() {
    let program = parse_plc(PLC_WORKPIECE_SPLIT_RUNTIME).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    let runtime_program =
        state_machine_to_runtime_program(&topology, &constraints, &state_machine, 10)
            .expect("runtime bridge should lower split actions");

    let mut io = MemIo::new();
    let mut runtime = Runtime::new(&runtime_program).expect("runtime should initialize");
    runtime.tick(&mut io).expect("split flow should execute");

    assert_eq!(runtime.workpiece_tokens().active_tokens(), 4);
    let source = runtime
        .workpiece_tokens()
        .token(0)
        .expect("source token should remain traceable");
    assert_eq!(source.workpiece_type, "rod");
    assert_eq!(source.current_location, "cut_zone");
    assert_eq!(
        source.terminal_status,
        Some(WorkpieceTerminalStatus::Consumed)
    );
    assert!(!source.active);
    assert_eq!(runtime.workpiece_lineage().len(), 4);
}

#[test]
fn runtime_bridge_lowers_merge_effect_into_runtime_action() {
    let program = parse_plc(PLC_WORKPIECE_SPLIT_MERGE_RUNTIME).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    let runtime_program =
        state_machine_to_runtime_program(&topology, &constraints, &state_machine, 10)
            .expect("runtime bridge should lower merge effects");

    assert!(
        collect_runtime_actions(&runtime_program)
            .iter()
            .any(|action| {
                matches!(
                    action,
                    Action::WorkpieceMerge {
                        input_refs,
                        input_types,
                        target_type,
                        consumed_inputs,
                    }
                        if *input_refs == ["slice_a", "slice_b"]
                            && *input_types == ["slice", "slice"]
                            && *target_type == "module"
                            && *consumed_inputs
                )
            })
    );
}

#[test]
fn runtime_executes_merge_action_end_to_end() {
    let program = parse_plc(PLC_WORKPIECE_SPLIT_MERGE_RUNTIME).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    let runtime_program =
        state_machine_to_runtime_program(&topology, &constraints, &state_machine, 10)
            .expect("runtime bridge should lower merge actions");

    let mut io = MemIo::new();
    let mut runtime = Runtime::new(&runtime_program).expect("runtime should initialize");
    runtime
        .tick(&mut io)
        .expect("split/merge flow should execute");

    assert_eq!(runtime.workpiece_tokens().active_tokens(), 3);
    let module = runtime
        .workpiece_tokens()
        .token(5)
        .expect("merge output should stay traceable");
    assert_eq!(module.workpiece_type, "module");
    assert_eq!(module.current_location, "cut_zone");
    assert!(module.active);
    assert_eq!(
        runtime
            .workpiece_lineage()
            .merge_inputs_of(5)
            .map(|record| record.source_token_id)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
}

#[test]
fn runtime_bridge_rejects_unknown_workpiece_carrier_explicitly() {
    let program = parse_plc(PLC_WORKPIECE_MOUNT_UNMOUNT).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let mut constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    constraints.workpiece_carriers.clear();

    let err = state_machine_to_runtime_program(&topology, &constraints, &state_machine, 10)
        .expect_err("undeclared carrier should fail bridge validation");
    assert!(matches!(
        err,
        BridgeError::UnknownWorkpieceCarrier { ref carrier } if carrier == "steel_plate"
    ));
}

#[test]
fn runtime_bridge_rejects_invalid_slot_reference_explicitly() {
    let program = parse_plc(PLC_WORKPIECE_MOUNT_UNMOUNT).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let mut state_machine = build_state_machine(&program).expect("state machine should build");

    state_machine.transitions[0].effects = vec![rust_plc::ir::WorkpieceEffect::Mount {
        workpiece_type: "rod".to_string(),
        slot: "steel_plate.slot[9]".to_string(),
    }];

    let err = state_machine_to_runtime_program(&topology, &constraints, &state_machine, 10)
        .expect_err("out-of-range slot should fail bridge validation");
    assert!(matches!(
        err,
        BridgeError::InvalidWorkpieceSlotReference { ref slot, .. }
            if slot == "steel_plate.slot[9]"
    ));
}

#[test]
fn runtime_bridge_lowers_workpiece_split_merge_example_into_runtime_actions() {
    let source = read_example_source("workpiece_split_merge.plc");
    let program = parse_plc(&source).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    let runtime_program =
        state_machine_to_runtime_program(&topology, &constraints, &state_machine, 10)
            .expect("runtime bridge should lower split/merge example");
    let actions = collect_runtime_actions(&runtime_program);

    assert!(actions.iter().any(|action| {
        matches!(
            action,
            Action::WorkpieceSplit {
                source_type,
                target_type,
                count,
                consumed
            } if *source_type == "rod"
                && *target_type == "slice"
                && *count == 4
                && *consumed
        )
    }));
    assert!(actions.iter().any(|action| {
        matches!(
            action,
            Action::WorkpieceMerge {
                input_refs,
                input_types,
                target_type,
                consumed_inputs,
            } if input_refs == &["slice_a", "slice_b"]
                && input_types == &["slice", "slice"]
                && *target_type == "module"
                && *consumed_inputs
        )
    }));
}

#[test]
fn workpiece_split_merge_example_lowers_into_ir_and_fails_exact_safety_without_source_seeding() {
    let source = read_example_source("workpiece_split_merge.plc");
    let program = parse_plc(&source).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    assert_eq!(constraints.workpiece_types.len(), 3);
    assert!(constraints.workpiece_types.iter().any(|workpiece| {
        workpiece.name == "rod"
            && workpiece.allows.iter().any(|allow| {
                matches!(allow, rust_plc::ir::WorkpieceAllowDef::SplitInto { target } if target == "slice")
            })
    }));
    assert!(constraints.workpiece_types.iter().any(|workpiece| {
        workpiece.name == "module"
            && workpiece.derived_from.iter().any(|rule| {
                matches!(rule, rust_plc::ir::WorkpieceDerivationDef::Merge { inputs } if inputs == &vec!["slice".to_string(), "slice".to_string()])
            })
    }));
    assert!(state_machine.transitions.iter().any(|transition| {
        transition
            .effects
            .iter()
            .any(|effect| matches!(effect, rust_plc::ir::WorkpieceEffect::Split { count, .. } if *count == 4))
    }));
    assert!(state_machine.transitions.iter().any(|transition| {
        transition
            .effects
            .iter()
            .any(|effect| matches!(effect, rust_plc::ir::WorkpieceEffect::Merge { inputs, .. } if inputs.len() == 2))
    }));

    let errors =
        verify_all(&program, &topology, &constraints, &state_machine).expect_err("must fail");
    assert!(errors.iter().any(|error| {
        error.checker == "safety"
            && error
                .reason
                .contains("requires a valid active source token of type 'rod'")
    }));
}

#[test]
fn workpiece_rejects_slot_arity_mismatch() {
    let program = parse_plc(PLC_WORKPIECE_INVALID_SLOT_ARITY).expect("fixture should parse");
    let errors = build_constraint_set(&program).expect_err("slot arity mismatch should fail");
    assert!(
        errors
            .iter()
            .any(|error| error.to_string().contains("expects 2 slot dimensions"))
    );
}

#[test]
fn workpiece_rejects_duplicate_terminal_state_entries() {
    let program =
        parse_plc(PLC_WORKPIECE_INVALID_DUPLICATE_TERMINAL_STATE).expect("fixture should parse");
    let errors = build_constraint_set(&program).expect_err("duplicate terminal state should fail");
    assert!(errors.iter().any(|error| {
        error
            .to_string()
            .contains("repeats normal terminal state 'finished'")
    }));
}

#[test]
fn workpiece_rejects_terminal_state_in_both_normal_and_abnormal_sets() {
    let program =
        parse_plc(PLC_WORKPIECE_INVALID_OVERLAPPING_TERMINAL_STATE).expect("fixture should parse");
    let errors =
        build_constraint_set(&program).expect_err("overlapping terminal state should fail");
    assert!(errors.iter().any(|error| {
        error
            .to_string()
            .contains("declares terminal state 'done' in both normal and abnormal categories")
    }));
}

#[test]
fn workpiece_rejects_consumed_as_declared_terminal_state() {
    let program =
        parse_plc(PLC_WORKPIECE_INVALID_CONSUMED_TERMINAL_STATE).expect("fixture should parse");
    let errors = build_constraint_set(&program).expect_err("consumed terminal state should fail");
    assert!(errors.iter().any(|error| {
        error
            .to_string()
            .contains("cannot declare reserved terminal state 'consumed' in normal category")
    }));
}

#[test]
fn workpiece_rejects_duplicate_ingress_sites() {
    let program =
        parse_plc(PLC_WORKPIECE_INVALID_DUPLICATE_INGRESS_SITE).expect("fixture should parse");
    let errors = build_constraint_set(&program).expect_err("duplicate ingress site should fail");
    assert!(
        errors
            .iter()
            .any(|error| { error.to_string().contains("repeats ingress site 'infeed'") })
    );
}

#[test]
fn workpiece_rejects_overlapping_normal_and_abnormal_egress_sites() {
    let program =
        parse_plc(PLC_WORKPIECE_INVALID_OVERLAPPING_EGRESS_SITE).expect("fixture should parse");
    let errors = build_constraint_set(&program).expect_err("overlapping egress site should fail");
    assert!(errors.iter().any(|error| {
        error.to_string().contains(
            "declares egress site 'shared_outfeed' in both normal and abnormal categories",
        )
    }));
}

#[test]
fn workpiece_rejects_duplicate_split_into_rules() {
    let program =
        parse_plc(PLC_WORKPIECE_INVALID_DUPLICATE_SPLIT_RULE).expect("fixture should parse");
    let errors = build_constraint_set(&program).expect_err("duplicate split rule should fail");
    assert!(errors.iter().any(|error| {
        error
            .to_string()
            .contains("repeats split_into rule 'split_into(slice)'")
    }));
}

#[test]
fn workpiece_rejects_duplicate_derived_from_rules() {
    let program =
        parse_plc(PLC_WORKPIECE_INVALID_DUPLICATE_DERIVATION_RULE).expect("fixture should parse");
    let errors = build_constraint_set(&program).expect_err("duplicate derived_from should fail");
    assert!(errors.iter().any(|error| {
        error
            .to_string()
            .contains("repeats derived_from rule 'derived_from(rod)'")
    }));
}

#[test]
fn workpiece_rejects_split_into_without_matching_target_derivation() {
    let program = parse_plc(PLC_WORKPIECE_INVALID_SPLIT_TARGET_MISSING_DERIVED_FROM)
        .expect("fixture should parse");
    let errors =
        build_constraint_set(&program).expect_err("missing target derived_from should fail");
    assert!(errors.iter().any(|error| {
        error.to_string().contains(
            "declares split_into(slice), but target type 'slice' is missing derived_from(rod)",
        )
    }));
}

#[test]
fn workpiece_rejects_derived_from_without_matching_source_split_rule() {
    let program = parse_plc(PLC_WORKPIECE_INVALID_DERIVED_FROM_SOURCE_MISSING_SPLIT_RULE)
        .expect("fixture should parse");
    let errors = build_constraint_set(&program).expect_err("missing source split_into should fail");
    assert!(errors.iter().any(|error| {
        error.to_string().contains(
            "declares derived_from(rod), but source type 'rod' is missing split_into(slice)",
        )
    }));
}

#[test]
fn workpiece_rejects_duplicate_merge_rules_even_if_input_order_differs() {
    let program = parse_plc(PLC_WORKPIECE_INVALID_DUPLICATE_MERGE_RULE_BY_PERMUTATION)
        .expect("fixture should parse");
    let errors =
        build_constraint_set(&program).expect_err("permuted duplicate merge rule should fail");
    assert!(errors.iter().any(|error| {
        error
            .to_string()
            .contains("repeats derived_from rule 'merge(cell, shell)'")
    }));
}

#[test]
fn workpiece_rejects_ambiguous_merge_rules_with_same_input_arity() {
    let program =
        parse_plc(PLC_WORKPIECE_INVALID_AMBIGUOUS_MERGE_RULE_ARITY).expect("fixture should parse");
    let errors = build_constraint_set(&program).expect_err("ambiguous merge arity should fail");
    assert!(errors.iter().any(|error| {
        error
            .to_string()
            .contains("declares multiple merge(...) derivations with 2 inputs")
    }));
}

#[test]
fn workpiece_rejects_duplicate_enum_property_values() {
    let program =
        parse_plc(PLC_WORKPIECE_INVALID_DUPLICATE_ENUM_VALUE).expect("fixture should parse");
    let errors = build_constraint_set(&program).expect_err("duplicate enum value should fail");
    assert!(errors.iter().any(|error| {
        error
            .to_string()
            .contains("enum property 'grade' repeats value 'a'")
    }));
}

#[test]
fn workpiece_rejects_split_effect_with_zero_count() {
    let program = parse_plc(PLC_WORKPIECE_INVALID_SPLIT_COUNT_ZERO).expect("fixture should parse");
    let errors = build_state_machine(&program).expect_err("zero split count should fail");
    assert!(errors.iter().any(|error| {
        error
            .to_string()
            .contains("split count must be greater than zero")
    }));
}

#[test]
fn workpiece_rejects_split_effect_without_declared_target_contract() {
    let program = parse_plc(PLC_WORKPIECE_INVALID_STEP_SPLIT_UNDECLARED_TARGET)
        .expect("fixture should parse");
    let errors = build_state_machine(&program).expect_err("step split target contract should fail");
    assert!(errors.iter().any(|error| {
        error
            .to_string()
            .contains("workpiece type 'rod' does not allow split_into(slice)")
    }));
    assert!(errors.iter().any(|error| {
        error
            .to_string()
            .contains("workpiece type 'slice' is not derived_from 'rod'")
    }));
}

#[test]
fn workpiece_rejects_merge_effect_with_input_arity_mismatch() {
    let program =
        parse_plc(PLC_WORKPIECE_INVALID_STEP_MERGE_ARITY_MISMATCH).expect("fixture should parse");
    let errors = build_state_machine(&program).expect_err("merge arity mismatch should fail");
    assert!(errors.iter().any(|error| {
        error
            .to_string()
            .contains("workpiece type 'module' has no merge(...) derivation matching 2 inputs")
    }));
}

#[test]
fn workpiece_rejects_finish_effect_with_undeclared_terminal_state() {
    let program =
        parse_plc(PLC_WORKPIECE_INVALID_FINISH_UNDECLARED_TERMINAL).expect("fixture should parse");
    let errors = build_state_machine(&program).expect_err("undeclared finish terminal should fail");
    assert!(errors.iter().any(|error| {
        error
            .to_string()
            .contains("terminal state 'scrapped' is not declared on workpiece type 'part'")
    }));
}

#[test]
fn workpiece_rejects_finish_effect_on_wrong_egress_bucket() {
    let program =
        parse_plc(PLC_WORKPIECE_INVALID_FINISH_WRONG_EGRESS_BUCKET).expect("fixture should parse");
    let errors = build_state_machine(&program).expect_err("wrong egress bucket should fail");
    assert!(errors.iter().any(|error| {
        error
            .to_string()
            .contains("finish endpoint 'outfeed' does not satisfy the declared egress contract")
    }));
}

#[test]
fn workpiece_rejects_untyped_transfer_effects_when_multiple_types_exist() {
    let program =
        parse_plc(PLC_WORKPIECE_INVALID_MULTI_TYPE_UNTYPED_TRANSFER).expect("fixture should parse");
    let errors = build_state_machine(&program).expect_err("multi-type transfer should fail");
    assert!(errors.iter().any(|error| {
        error
            .to_string()
            .contains("acquire/transfer/finish effects remain single-type in this phase")
    }));
}

#[test]
fn parser_supports_new_workpiece_effect_variants() {
    let program = parse_plc(PLC_WORKPIECE_MOUNT_UNMOUNT).expect("fixture should parse");
    let effects = program
        .tasks
        .tasks
        .iter()
        .flat_map(|task| task.steps.iter())
        .flat_map(|step| step.statements.iter())
        .filter_map(|statement| match statement {
            rust_plc::ast::StepStatement::Effect(effect) => Some(&effect.kind),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, EffectKind::Mount { .. }))
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, EffectKind::Unmount { .. }))
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, EffectKind::TransformCarrier { .. }))
    );
}

#[test]
fn verify_all_rejects_workpiece_source_underflow() {
    let program = parse_plc(PLC_WORKPIECE_VERIFY_UNDERFLOW).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    let errors =
        verify_all(&program, &topology, &constraints, &state_machine).expect_err("must fail");
    assert!(errors.iter().any(|error| {
        error.checker == "safety"
            && error
                .reason
                .contains("before any free-standing workpiece is available")
    }));
}

#[test]
fn verify_all_rejects_phase1_effect_from_undeclared_ingress_site() {
    let program = parse_plc(PLC_WORKPIECE_VERIFY_DEAD_CONTRACTS).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let mut constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    constraints.workpiece_types[0].ingress_sites = vec!["outfeed".to_string()];

    let errors =
        verify_all(&program, &topology, &constraints, &state_machine).expect_err("must fail");
    assert!(errors.iter().any(|error| {
        error.checker == "safety"
            && error
                .reason
                .contains("before any free-standing workpiece is available")
            && error.reason.contains("not a declared ingress site")
    }));
}

#[test]
fn verify_all_rejects_workpiece_capacity_overflow() {
    let program = parse_plc(PLC_WORKPIECE_VERIFY_CAPACITY).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    let errors =
        verify_all(&program, &topology, &constraints, &state_machine).expect_err("must fail");
    assert!(
        errors
            .iter()
            .any(|error| error.checker == "safety" && error.reason.contains("exceed capacity"))
    );
}

#[test]
fn verify_all_rejects_consuming_mounted_workpiece_after_transform() {
    let program =
        parse_plc(PLC_WORKPIECE_VERIFY_MOUNTED_CONSISTENCY).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    let errors =
        verify_all(&program, &topology, &constraints, &state_machine).expect_err("must fail");
    assert!(errors.iter().any(|error| {
        error.checker == "safety"
            && error
                .reason
                .contains("before any free-standing workpiece is available")
            && error.reason.contains("plate.slot[0]")
    }));
}

#[test]
fn verify_all_rejects_reachable_duplicate_workpiece_occupancy() {
    let program = parse_plc(PLC_WORKPIECE_VERIFY_DEAD_CONTRACTS).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let mut constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    let mut duplicate_type = constraints.workpiece_types[0].clone();
    duplicate_type.name = "part_clone".to_string();
    constraints.workpiece_types.push(duplicate_type);

    let errors =
        verify_all(&program, &topology, &constraints, &state_machine).expect_err("must fail");
    assert!(errors.iter().any(|error| {
        error.checker == "safety"
            && error.reason.contains("duplicate occupancy")
            && error.reason.contains("infeed")
    }));
}

#[test]
fn verify_all_rejects_terminal_state_with_unfinished_workpiece() {
    let program = parse_plc(PLC_WORKPIECE_VERIFY_DANGLING).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    let errors =
        verify_all(&program, &topology, &constraints, &state_machine).expect_err("must fail");
    assert!(errors.iter().any(|error| {
        error.checker == "safety" && error.reason.contains("still holds workpieces")
    }));
}

#[test]
fn verify_all_rejects_finish_through_wrong_egress_bucket_on_reachable_state() {
    let program = parse_plc(PLC_WORKPIECE_VERIFY_DEAD_CONTRACTS).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let mut constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    constraints.workpiece_types[0]
        .normal_terminal_states
        .clear();
    constraints.workpiece_types[0].abnormal_terminal_states = vec!["finished".to_string()];

    let errors =
        verify_all(&program, &topology, &constraints, &state_machine).expect_err("must fail");
    assert!(errors.iter().any(|error| {
        error.checker == "safety"
            && error.reason.contains("abnormal terminal state 'finished'")
            && error.reason.contains("outfeed")
    }));
}

#[test]
fn verify_all_warns_when_abnormal_workpiece_contract_is_unreachable() {
    let program = parse_plc(PLC_WORKPIECE_VERIFY_DEAD_CONTRACTS).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    let summary =
        verify_all(&program, &topology, &constraints, &state_machine).expect("must succeed");
    assert!(summary.safety.warnings.iter().any(|warning| {
        warning.level == WarningLevel::Warn
            && warning
                .message
                .contains("declares abnormal terminal state 'rejected'")
    }));
    assert!(summary.safety.warnings.iter().any(|warning| {
        warning.level == WarningLevel::Warn
            && warning
                .message
                .contains("declares abnormal egress site 'reject_bin'")
    }));
}

#[test]
fn verify_all_warns_when_declared_split_merge_contracts_are_unused() {
    let program = parse_plc(PLC_WORKPIECE_VERIFY_UNUSED_CONTRACTS).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    let summary =
        verify_all(&program, &topology, &constraints, &state_machine).expect("must succeed");
    assert!(summary.safety.warnings.iter().any(|warning| {
        warning.level == WarningLevel::Warn && warning.message.contains("split_into(slice)")
    }));
    assert!(summary.safety.warnings.iter().any(|warning| {
        warning.level == WarningLevel::Warn && warning.message.contains("merge(slice, slice)")
    }));
}

#[test]
fn verify_all_warns_when_declared_ingress_is_unused() {
    let program = parse_plc(PLC_WORKPIECE_VERIFY_UNUSED_INGRESS).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    let summary =
        verify_all(&program, &topology, &constraints, &state_machine).expect("must succeed");
    assert!(summary.safety.warnings.iter().any(|warning| {
        warning.level == WarningLevel::Warn
            && warning.message.contains("declares ingress site 'infeed'")
    }));
}

#[test]
fn verify_all_rejects_unmount_from_empty_slot() {
    let program = parse_plc(PLC_WORKPIECE_VERIFY_UNMOUNT_UNDERFLOW).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    let errors =
        verify_all(&program, &topology, &constraints, &state_machine).expect_err("must fail");
    assert!(errors.iter().any(|error| {
        error.checker == "safety"
            && error
                .reason
                .contains("before any mounted workpiece is available")
            && error.reason.contains("plate.slot[0]")
    }));
}

#[test]
fn verify_all_rejects_split_without_a_valid_source_token_instance() {
    let program =
        parse_plc(PLC_WORKPIECE_VERIFY_SPLIT_WITHOUT_SOURCE_INTRO).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    let errors =
        verify_all(&program, &topology, &constraints, &state_machine).expect_err("must fail");
    assert!(errors.iter().any(|error| {
        error.checker == "safety"
            && error
                .reason
                .contains("requires a valid active source token of type 'rod'")
    }));
}

#[test]
fn verify_all_rejects_merge_without_declared_input_instances() {
    let program =
        parse_plc(PLC_WORKPIECE_VERIFY_MERGE_WITHOUT_INPUT_INTRO).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let constraints = build_constraint_set(&program).expect("constraints should build");
    let state_machine = build_state_machine(&program).expect("state machine should build");

    let errors =
        verify_all(&program, &topology, &constraints, &state_machine).expect_err("must fail");
    assert!(errors.iter().any(|error| {
        error.checker == "safety"
            && error
                .reason
                .contains("requires the declared legal input set [cell, cell]")
            && error.reason.contains("2x cell")
    }));
}

#[test]
fn verify_all_rejects_merge_when_split_produces_too_few_instances() {
    let program = parse_plc(PLC_WORKPIECE_SPLIT_MERGE_RUNTIME).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let mut constraints = build_constraint_set(&program).expect("constraints should build");
    constraints.workpiece_types[0].ingress_sites = vec!["plate.slot[*]".to_string()];
    let mut state_machine = build_state_machine(&program).expect("state machine should build");
    let split = state_machine
        .transitions
        .iter_mut()
        .find_map(|transition| {
            transition
                .effects
                .iter_mut()
                .find_map(|effect| match effect {
                    rust_plc::ir::WorkpieceEffect::Split { count, .. } => Some(count),
                    _ => None,
                })
        })
        .expect("split effect should exist");
    *split = 1;

    let errors =
        verify_all(&program, &topology, &constraints, &state_machine).expect_err("must fail");
    assert!(errors.iter().any(|error| {
        error.checker == "safety"
            && error
                .reason
                .contains("requires the declared legal input set [slice, slice]")
            && error.reason.contains("1x slice")
    }));
}

#[test]
fn verify_all_rejects_reusing_consumed_merge_input_instances() {
    let program = parse_plc(PLC_WORKPIECE_SPLIT_MERGE_RUNTIME).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let mut constraints = build_constraint_set(&program).expect("constraints should build");
    constraints.workpiece_types[0].ingress_sites = vec!["plate.slot[*]".to_string()];
    let mut state_machine = build_state_machine(&program).expect("state machine should build");

    let split = state_machine
        .transitions
        .iter_mut()
        .find_map(|transition| {
            transition
                .effects
                .iter_mut()
                .find_map(|effect| match effect {
                    rust_plc::ir::WorkpieceEffect::Split { count, .. } => Some(count),
                    _ => None,
                })
        })
        .expect("split effect should exist");
    *split = 2;

    let assemble_state = rust_plc::ir::State {
        task_name: "process".to_string(),
        step_name: "assemble_b".to_string(),
    };
    state_machine.states.push(assemble_state.clone());
    let merge_effect = state_machine
        .transitions
        .iter()
        .find_map(|transition| {
            transition.effects.iter().find_map(|effect| match effect {
                rust_plc::ir::WorkpieceEffect::Merge { .. } => Some(effect.clone()),
                _ => None,
            })
        })
        .expect("merge effect should exist");
    let merge_transition = state_machine
        .transitions
        .iter_mut()
        .find(|transition| {
            transition.from.task_name == "process" && transition.from.step_name == "assemble"
        })
        .expect("assemble transition should exist");
    merge_transition.to = assemble_state.clone();
    state_machine.transitions.push(rust_plc::ir::Transition {
        from: assemble_state,
        to: rust_plc::ir::State {
            task_name: "sink".to_string(),
            step_name: "idle".to_string(),
        },
        guard: rust_plc::ir::TransitionGuard::Always,
        actions: Vec::new(),
        effects: vec![merge_effect],
        timers: Vec::new(),
    });

    let errors =
        verify_all(&program, &topology, &constraints, &state_machine).expect_err("must fail");
    assert!(errors.iter().any(|error| {
        error.checker == "safety"
            && error
                .reason
                .contains("requires the declared legal input set [slice, slice]")
            && error.reason.contains("2x slice")
    }));
}

#[test]
fn verify_all_tracks_wide_split_instances_without_false_merge_underflow() {
    let program = parse_plc(PLC_WORKPIECE_SPLIT_MERGE_RUNTIME).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let mut constraints = build_constraint_set(&program).expect("constraints should build");
    constraints.workpiece_types[0].ingress_sites = vec!["plate.slot[*]".to_string()];
    let mut state_machine = build_state_machine(&program).expect("state machine should build");

    let assemble_state = rust_plc::ir::State {
        task_name: "process".to_string(),
        step_name: "assemble_b".to_string(),
    };
    state_machine.states.push(assemble_state.clone());
    let merge_effect = state_machine
        .transitions
        .iter()
        .find_map(|transition| {
            transition.effects.iter().find_map(|effect| match effect {
                rust_plc::ir::WorkpieceEffect::Merge { .. } => Some(effect.clone()),
                _ => None,
            })
        })
        .expect("merge effect should exist");
    let merge_transition = state_machine
        .transitions
        .iter_mut()
        .find(|transition| {
            transition.from.task_name == "process" && transition.from.step_name == "assemble"
        })
        .expect("assemble transition should exist");
    merge_transition.to = assemble_state.clone();
    state_machine.transitions.push(rust_plc::ir::Transition {
        from: assemble_state,
        to: rust_plc::ir::State {
            task_name: "sink".to_string(),
            step_name: "idle".to_string(),
        },
        guard: rust_plc::ir::TransitionGuard::Always,
        actions: Vec::new(),
        effects: vec![merge_effect],
        timers: Vec::new(),
    });

    let errors =
        verify_all(&program, &topology, &constraints, &state_machine).expect_err("must fail");
    assert!(!errors.iter().any(|error| {
        error.checker == "safety"
            && error
                .reason
                .contains("requires the declared legal input set [slice, slice]")
    }));
    assert!(errors.iter().any(|error| {
        error.checker == "safety" && error.reason.contains("still holds workpieces")
    }));
}

#[test]
fn verify_all_rejects_merge_when_declared_input_set_does_not_match_produced_instances() {
    let program = parse_plc(PLC_WORKPIECE_SPLIT_MERGE_RUNTIME).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let mut constraints = build_constraint_set(&program).expect("constraints should build");
    constraints.workpiece_types[0].ingress_sites = vec!["plate.slot[*]".to_string()];
    let state_machine = build_state_machine(&program).expect("state machine should build");

    let module = constraints
        .workpiece_types
        .iter_mut()
        .find(|workpiece| workpiece.name == "module")
        .expect("module type should exist");
    module.derived_from = vec![rust_plc::ir::WorkpieceDerivationDef::Merge {
        inputs: vec!["rod".to_string(), "rod".to_string()],
    }];

    let errors =
        verify_all(&program, &topology, &constraints, &state_machine).expect_err("must fail");
    assert!(errors.iter().any(|error| {
        error.checker == "safety"
            && error
                .reason
                .contains("requires the declared legal input set [rod, rod]")
            && error.reason.contains("2x rod")
    }));
}

#[test]
fn verify_all_rejects_terminal_states_that_still_hold_split_merge_instances() {
    let program = parse_plc(PLC_WORKPIECE_SPLIT_MERGE_RUNTIME).expect("fixture should parse");
    let topology = build_topology_graph(&program).expect("topology should build");
    let mut constraints = build_constraint_set(&program).expect("constraints should build");
    constraints.workpiece_types[0].ingress_sites = vec!["plate.slot[*]".to_string()];
    let state_machine = build_state_machine(&program).expect("state machine should build");

    let errors =
        verify_all(&program, &topology, &constraints, &state_machine).expect_err("must fail");
    assert!(errors.iter().any(|error| {
        error.checker == "safety"
            && error
                .reason
                .contains("reachable terminal state still holds workpieces")
            && error.reason.contains("cut_zone")
    }));
}
