use io_traits::{AnalogInputId, DigitalInputId, DigitalOutputId, Io, Tick};
use runtime_core::{AxisMotionResult, AxisMoveKind, Runtime, RuntimeError, RuntimeTickError};
use rust_plc::extern_functions::{
    EXTERN_ERROR_CODE_INPUT_OUT_OF_RANGE, EXTERN_ERROR_CODE_RUNTIME_ERROR, ExternFunctionInfo,
    ExternFunctionRegistry, ExternRuntimeError, ValueRange, extern_runtime_error_code,
};
use rust_plc::ir::{ExternFunctionContract, TopologyGraph, VariableType};
use rust_plc::parser::parse_plc;
use rust_plc::runtime_bridge::{BridgeError, state_machine_to_runtime_program};
use rust_plc::semantic::{build_state_machine, build_topology_graph, preprocess_program};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

fn compile_to_runtime(plc_source: &str, tick_ms: u64) -> runtime_core::Program<'static> {
    let program = parse_plc(plc_source).expect("parse plc");
    // Keep preprocessing in the pipeline so repeat expansion (etc.) stays consistent.
    let expanded = preprocess_program(&program).expect("preprocess");
    let topology = build_topology_graph(&expanded).expect("topology");
    let sm = build_state_machine(&expanded).expect("state machine");
    state_machine_to_runtime_program(&topology, &sm, tick_ms).expect("bridge")
}

fn compile_to_runtime_result(
    plc_source: &str,
    tick_ms: u64,
) -> Result<runtime_core::Program<'static>, BridgeError> {
    let program = parse_plc(plc_source).expect("parse plc");
    let expanded = preprocess_program(&program).expect("preprocess");
    let topology = build_topology_graph(&expanded).expect("topology");
    let sm = build_state_machine(&expanded).expect("state machine");
    state_machine_to_runtime_program(&topology, &sm, tick_ms)
}

fn compile_runtime_and_topology(
    plc_source: &str,
    tick_ms: u64,
) -> (runtime_core::Program<'static>, TopologyGraph) {
    let program = parse_plc(plc_source).expect("parse plc");
    let expanded = preprocess_program(&program).expect("preprocess");
    let topology = build_topology_graph(&expanded).expect("topology");
    let sm = build_state_machine(&expanded).expect("state machine");
    let runtime_program =
        state_machine_to_runtime_program(&topology, &sm, tick_ms).expect("bridge");
    (runtime_program, topology)
}

fn variable_index(topology: &TopologyGraph, name: &str) -> u16 {
    topology
        .variables
        .iter()
        .find(|var| var.name == name)
        .map(|var| var.index)
        .expect("variable should exist")
}

fn current_step_name<'a>(rt: &Runtime<'a>, program: &'a runtime_core::Program<'a>) -> &'a str {
    let loc = rt.location();
    program
        .task(loc.task)
        .expect("task exists")
        .step(loc.step)
        .expect("step exists")
        .name
}

const PLC_FIXTURE: &str = r#"
[topology]

device Y0: digital_output
device X0: digital_input

device start_button: sensor

device valve_A: solenoid_valve

device cyl_A: cylinder

device sensor_ext: sensor

relation { from: start_button.out, to: X0.in, via: reports_to }
relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_ext.sense, via: detects }
relation { from: sensor_ext.out, to: X0.in, via: reports_to }

[constraints]

[tasks]

task main:
    step extend:
        action: extend cyl_A

    step wait_button:
        wait: start_button == true
        timeout: 50ms -> goto fault

    step dwell:
        delay: 20ms

    step retract:
        action: retract cyl_A

    on_complete: goto done

task fault:
    step retract_fault:
        action: retract cyl_A
    on_complete: goto done

task done:
    step halt:
"#;

#[test]
fn bridge_compiles_plc_and_produces_deterministic_trace_and_edges() {
    let tick_ms = 10;

    let run = || {
        let program = compile_to_runtime(PLC_FIXTURE, tick_ms);
        let mut rt = Runtime::new(&program).expect("runtime init");

        let mut io = sim::SimIo::new(1, 1, 0, 0);
        // Make start_button/X0 go true at tick 1.
        io.schedule_digital_input(Tick(1), DigitalInputId(0), true);

        let mut trace = sim::JsonlTraceRecorder::new();
        for _ in 0..10 {
            rt.tick_with_trace(&mut io, |e| trace.record(e))
                .expect("tick");
        }

        (trace.into_string(), io.digital_output_edges().to_vec())
    };

    let (trace1, edges1) = run();
    let (trace2, edges2) = run();

    assert_eq!(trace1, trace2, "trace should be deterministic");
    assert_eq!(edges1, edges2, "output edges should be deterministic");

    assert_eq!(
        edges1,
        vec![
            sim::DigitalEdge {
                tick: Tick(0),
                id: DigitalOutputId(0),
                value: true,
            },
            sim::DigitalEdge {
                tick: Tick(3),
                id: DigitalOutputId(0),
                value: false,
            }
        ]
    );
}

#[test]
fn bridge_supports_timeout_to_goto_branch() {
    let tick_ms = 10;
    let program = compile_to_runtime(PLC_FIXTURE, tick_ms);
    let mut rt = Runtime::new(&program).expect("runtime init");

    let mut io = sim::SimIo::new(1, 1, 0, 0);
    let mut trace = sim::JsonlTraceRecorder::new();

    // Run until tick 5 (50ms) to trigger timeout.
    for _ in 0..6 {
        rt.tick_with_trace(&mut io, |e| trace.record(e))
            .expect("tick");
    }

    let out = trace.into_string();
    assert!(
        out.contains("\"reason\":\"timeout\""),
        "trace should include timeout transition, got: {out}"
    );

    // DO0: extend at tick 0, retract on timeout at tick 5.
    assert_eq!(
        io.digital_output_edges(),
        &[
            sim::DigitalEdge {
                tick: Tick(0),
                id: DigitalOutputId(0),
                value: true,
            },
            sim::DigitalEdge {
                tick: Tick(5),
                id: DigitalOutputId(0),
                value: false,
            },
        ]
    );
}

const PLC_ANALOG_FIXTURE: &str = r#"
[topology]

device AO0: analog_output { range: 0..10, unit: "V" }

[constraints]

[tasks]

task main:
    step set_output:
        action: set_analog AO0 4.2
    on_complete: goto done

task done:
    step halt:
"#;

#[test]
fn bridge_supports_set_analog_action_for_ao_channels() {
    let program = compile_to_runtime(PLC_ANALOG_FIXTURE, 1);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(1, 1, 0, 1);

    rt.tick(&mut io).expect("tick");
    let edges = io.analog_output_edges();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].tick, Tick(0));
    assert_eq!(edges[0].id.0, 0);
    assert!((edges[0].value - 4.2).abs() < f32::EPSILON);
}

const PLC_ANALOG_WAIT_FIXTURE: &str = r#"
[topology]

device AI0: analog_input { range: 0..100, unit: "bar", external: true }
device X0: digital_input

device start_button: sensor
relation { from: start_button.out, to: X0.in, via: reports_to }

[constraints]

[tasks]

task main:
    step wait_pressure:
        wait: AI0 > 80
        timeout: 5ms -> goto done
    on_complete: goto done

task done:
    step halt:
"#;

#[test]
fn bridge_supports_analog_wait_guard_mapped_to_regions() {
    let program = compile_to_runtime(PLC_ANALOG_WAIT_FIXTURE, 1);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(1, 1, 1, 0);
    io.schedule_analog_input(Tick(0), io_traits::AnalogInputId(0), 90.0);

    let mut trace = sim::JsonlTraceRecorder::new();
    rt.tick_with_trace(&mut io, |e| trace.record(e))
        .expect("tick");
    let out = trace.into_string();
    assert!(
        out.contains("\"reason\":\"wait_satisfied\""),
        "expected analog wait to satisfy immediately, got: {out}"
    );
}

const PLC_STEPPER_PORT_FIXTURE: &str = r#"
[topology]

device plc_main: plc { ports: [Y0:digital:producer, Y1:digital:producer] }
device axis_x: stepper_motor

relation { from: plc_main.Y0, to: axis_x.enable, via: driven_by }
relation { from: plc_main.Y1, to: axis_x.direction, via: driven_by }

[constraints]

[tasks]

task main:
    step enable_axis:
        action: set axis_x.enable on
    step dir_forward:
        action: set axis_x.direction forward
    step done:
        action: log "done"
"#;

#[test]
fn bridge_routes_stepper_ports_to_distinct_digital_outputs() {
    let program = compile_to_runtime(PLC_STEPPER_PORT_FIXTURE, 1);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(2, 1, 0, 0);

    rt.tick(&mut io).expect("tick");

    assert_eq!(
        io.digital_output_edges(),
        &[
            sim::DigitalEdge {
                tick: Tick(0),
                id: DigitalOutputId(0),
                value: true,
            },
            sim::DigitalEdge {
                tick: Tick(0),
                id: DigitalOutputId(1),
                value: true,
            },
        ],
        "enable/direction should be routed by port, not collapsed onto one output"
    );
}

const PLC_PID_FIXTURE: &str = r#"
[topology]

device AI0: analog_input { range: 0..100, unit: "bar", external: true }
device AO0: analog_output { range: 0..100, unit: "%" }
device loop_pressure: pid {
    pv: AI0,
    sp: 60bar,
    kp: 2.0,
    ki: 0.5,
    kd: 0.0,
    out: AO0,
    period_ms: 100,
    limit: 0..100
}

[constraints]

[tasks]

task main:
    step hold:
"#;

#[test]
fn bridge_maps_pid_declaration_into_runtime_program_and_clamps_output() {
    let tick_ms = 100;
    let program = compile_to_runtime(PLC_PID_FIXTURE, tick_ms);
    assert_eq!(program.pid_loops.len(), 1, "PID config should be bridged");
    let cfg = program.pid_loops[0];
    assert_eq!(cfg.pv.0, 0);
    assert_eq!(cfg.out.0, 0);
    assert_eq!(cfg.period_ticks, 1);
    assert!(matches!(
        cfg.anti_windup,
        runtime_core::AntiWindup::ConditionalIntegration
    ));

    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(1, 1, 1, 1);

    // Simple deterministic plant loop.
    for _ in 0..20 {
        rt.tick(&mut io).expect("tick");
        let u = io
            .analog_output_edges()
            .last()
            .map(|edge| edge.value)
            .unwrap_or(0.0);
        let y = io.read_analog_input(AnalogInputId(0));
        let next = y + 0.3 * (u - y);
        io.schedule_analog_input(io.tick(), AnalogInputId(0), next);
    }

    let final_u = io
        .analog_output_edges()
        .last()
        .map(|edge| edge.value)
        .unwrap_or(0.0);
    assert!(
        (0.0..=100.0).contains(&final_u),
        "PID output should be clamped within limit, got {final_u}"
    );
}

const PLC_CAM_FIXTURE: &str = r#"
[topology]
device AI0: analog_input { range: 0..360, unit: "deg", external: true }
device AO0: analog_output { range: 0..360, unit: "deg" }
cam_table cam_a: periodic [
    (0, 0),
    (360, 0),
]
cam_table cam_b: periodic [
    (0, 0),
    (180, 180),
    (360, 0),
]
device cam_xy: cam_coupling {
    master: AI0,
    slave: AO0,
    table: cam_a,
    interpolation: linear,
    gear_ratio: 1.0,
    phase_offset: 0.0,
    following_error_limit: 999.0,
    slave_feedback: AI0,
}

[constraints]

[tasks]
task main:
    step engage:
        action: cam_engage cam_xy
    step switch_and_phase:
        action: cam_switch cam_xy cam_b
        action: cam_phase cam_xy 10.0
    step wait_master:
        wait: cam_xy.master_pos > 5
        timeout: 5ms -> goto done
    on_complete: goto done

task done:
    step halt:
"#;

const PLC_AXIS_BRIDGE_FIXTURE: &str = r#"
[topology]
device axis_x: stepper_motor

[constraints]

[tasks]
task motion:
    step run:
        action: axis.move_relative(axis_x, distance: 10, speed: 2)
            timeout: 500ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
    on_complete: goto done

task fault:
    step timeout:
    step reject:
    step motion_fault:
    step safety_fault:

task done:
    step halt:
"#;

#[test]
fn bridge_maps_cam_tables_configs_and_actions() {
    let program = compile_to_runtime(PLC_CAM_FIXTURE, 1);
    assert_eq!(program.cam_tables.len(), 2, "cam tables should be bridged");
    assert_eq!(program.cam_configs.len(), 1, "cam config should be bridged");
    assert_eq!(
        program.cam_configs[0].table_index, 0,
        "default table should map to index 0"
    );

    let mut saw_switch = false;
    let mut saw_phase = false;
    for task in program.tasks {
        for step in task.steps {
            if let runtime_core::Instr::Action { actions, .. } = step.instr {
                for action in actions {
                    match action {
                        runtime_core::Action::CamSwitch { .. } => saw_switch = true,
                        runtime_core::Action::CamPhase { .. } => saw_phase = true,
                        _ => {}
                    }
                }
            }
        }
    }
    assert!(
        saw_switch,
        "cam_switch should be lowered into runtime action"
    );
    assert!(saw_phase, "cam_phase should be lowered into runtime action");
}

#[test]
fn bridge_maps_axis_move_actions_without_unsupported_action() {
    let program = compile_to_runtime(PLC_AXIS_BRIDGE_FIXTURE, 10);
    let mut saw_axis = false;
    for task in program.tasks {
        for step in task.steps {
            if let runtime_core::Instr::Action { actions, .. } = step.instr {
                for action in actions {
                    if let runtime_core::Action::AxisMove { command } = action {
                        saw_axis = true;
                        assert_eq!(command.target, "axis_x");
                        assert_eq!(command.kind, AxisMoveKind::Relative);
                    }
                }
            }
        }
    }
    assert!(saw_axis, "axis_move should be lowered into runtime action");
}

#[test]
fn runtime_tick_with_axis_handler_done_for_bridged_axis_action() {
    let program = compile_to_runtime(PLC_AXIS_BRIDGE_FIXTURE, 10);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(1, 1, 0, 0);

    rt.tick_with_axis(&mut io, |command| {
        assert_eq!(command.target, "axis_x");
        AxisMotionResult::Done
    })
    .expect("bridged axis action should run with handler");
}

#[test]
fn runtime_tick_with_axis_handler_propagates_classified_faults_for_bridged_axis_action() {
    let cases = [
        (
            AxisMotionResult::Reject { error_code: 41 },
            RuntimeError::AxisMotionRejected {
                target: "axis_x",
                error_code: 41,
            },
        ),
        (
            AxisMotionResult::MotionFault { error_code: 42 },
            RuntimeError::AxisMotionFault {
                target: "axis_x",
                error_code: 42,
            },
        ),
        (
            AxisMotionResult::SafetyFault { error_code: 43 },
            RuntimeError::AxisSafetyFault {
                target: "axis_x",
                error_code: 43,
            },
        ),
    ];

    for (result, expected) in cases {
        let program = compile_to_runtime(PLC_AXIS_BRIDGE_FIXTURE, 10);
        let mut rt = Runtime::new(&program).expect("runtime init");
        let mut io = sim::SimIo::new(1, 1, 0, 0);
        let err = rt
            .tick_with_axis(&mut io, |_| result)
            .expect_err("fault result should be surfaced");
        assert_eq!(err, expected);
    }
}

#[test]
fn bridge_waits_on_cam_runtime_state_and_drives_output() {
    let program = compile_to_runtime(PLC_CAM_FIXTURE, 1);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(1, 1, 1, 1);

    io.schedule_analog_input(Tick(1), AnalogInputId(0), 20.0);
    for _ in 0..4 {
        rt.tick(&mut io).expect("tick");
    }

    let last_ao = io
        .analog_output_edges()
        .last()
        .map(|edge| edge.value)
        .unwrap_or(0.0);
    assert!(
        (last_ao - 30.0).abs() < 1e-3,
        "cam output should include phase offset after wait path, got {last_ao}"
    );
}

const PLC_BOOL_EXPR_FIXTURE: &str = r#"
[topology]
variable a: bool = false
variable b: bool = true
variable x: float = 0.0
variable flag: bool = false

[constraints]

[tasks]
task main:
    step run:
        action: compute flag = NOT a OR (b AND x > 0)
    on_complete: goto done

task done:
    step halt:
"#;

#[test]
fn bridge_executes_compute_boolean_expression_into_bool_slot() {
    let (program, topology) = compile_runtime_and_topology(PLC_BOOL_EXPR_FIXTURE, 1);
    let flag_var = variable_index(&topology, "flag");
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(0, 0, 0, 0);

    rt.tick(&mut io).expect("tick");

    assert_eq!(rt.variables()[flag_var as usize], 1.0);
    assert_eq!(current_step_name(&rt, &program), "done.halt");
}

const PLC_EXTERN_FIXTURE: &str = r#"
[topology]

extern function add(a: float, b: float) -> float {
    rust_module: "math::basic",
    pure: true,
    time_bound_us: 1000
}

variable x: float = 1.5
variable y: float = 2.0
variable out: float = 0.0

[constraints]

[tasks]

task main:
    step compute:
        action: call add(x, y) -> out
    on_complete: goto done

task done:
    step halt:
"#;

fn make_add_registry() -> ExternFunctionRegistry {
    let mut registry = ExternFunctionRegistry::with_time_source(|| 0);
    registry
        .register(ExternFunctionInfo::new(
            "add",
            vec![VariableType::Float, VariableType::Float],
            vec![VariableType::Float],
            ExternFunctionContract {
                rust_module: "math::basic".to_string(),
                pure: true,
                time_bound_us: 1000,
            },
            |args| Ok(vec![args[0] + args[1]]),
        ))
        .expect("register add");
    registry
}

fn call_registry(
    registry: &ExternFunctionRegistry,
    function: &'static str,
    args: &[f32],
    outputs: &mut [f32],
) -> Result<usize, ExternRuntimeError> {
    let values = registry.call(function, args)?;
    for (index, value) in values.iter().enumerate().take(outputs.len()) {
        outputs[index] = *value;
    }
    Ok(values.len())
}

#[test]
fn bridge_executes_call_extern_and_writes_bound_variable() {
    let program = compile_to_runtime(PLC_EXTERN_FIXTURE, 1);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(1, 1, 0, 0);
    let registry = make_add_registry();

    rt.tick_with_extern(&mut io, |function, args, outputs| {
        call_registry(&registry, function, args, outputs)
    })
    .expect("extern call should execute");

    assert!(
        (rt.variables()[2] - 3.5).abs() < f32::EPSILON,
        "extern result should be written into bound variable"
    );
}

#[test]
fn runtime_tick_without_handler_reports_extern_handler_requirement() {
    let program = compile_to_runtime(PLC_EXTERN_FIXTURE, 1);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(1, 1, 0, 0);

    let err = rt
        .tick(&mut io)
        .expect_err("extern action requires handler");
    assert_eq!(
        err,
        runtime_core::RuntimeError::ExternCallRequiresHandler { function: "add" }
    );
}

#[test]
fn runtime_tick_with_extern_propagates_registry_error_with_function_context() {
    let program = compile_to_runtime(PLC_EXTERN_FIXTURE, 1);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(1, 1, 0, 0);
    let registry = ExternFunctionRegistry::with_time_source(|| 0);

    let err = rt
        .tick_with_extern(&mut io, |function, args, outputs| {
            call_registry(&registry, function, args, outputs)
        })
        .expect_err("missing function should propagate");
    match err {
        RuntimeTickError::ExternCallFailed { function, error } => {
            assert_eq!(function, "add");
            assert_eq!(
                error,
                ExternRuntimeError::FunctionNotFound {
                    name: "add".to_string()
                }
            );
        }
        other => panic!("unexpected error variant: {other:?}"),
    }
}

fn make_range_checked_registry() -> ExternFunctionRegistry {
    let mut registry = ExternFunctionRegistry::with_time_source(|| 0);
    registry
        .register(
            ExternFunctionInfo::new(
                "add",
                vec![VariableType::Float, VariableType::Float],
                vec![VariableType::Float],
                ExternFunctionContract {
                    rust_module: "math::basic".to_string(),
                    pure: true,
                    time_bound_us: 1000,
                },
                |args| Ok(vec![args[0] + args[1]]),
            )
            .with_input_ranges(vec![
                ValueRange::new(-10.0, 10.0),
                ValueRange::new(-1.0, 1.0),
            ])
            .with_output_ranges(vec![ValueRange::new(-10.0, 10.0)]),
        )
        .expect("register add with ranges");
    registry
}

#[test]
fn runtime_tick_with_extern_propagates_input_range_violation_details() {
    let program = compile_to_runtime(PLC_EXTERN_FIXTURE, 1);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(1, 1, 0, 0);
    let registry = make_range_checked_registry();

    let err = rt
        .tick_with_extern(&mut io, |function, args, outputs| {
            call_registry(&registry, function, args, outputs)
        })
        .expect_err("input range should fail");

    match err {
        RuntimeTickError::ExternCallFailed { function, error } => {
            assert_eq!(function, "add");
            assert_eq!(
                error,
                ExternRuntimeError::InputOutOfRange {
                    function: "add".to_string(),
                    arg_index: 1,
                    value: 2.0,
                    min: -1.0,
                    max: 1.0,
                }
            );
        }
        other => panic!("unexpected error variant: {other:?}"),
    }
}

#[test]
fn runtime_tick_with_extern_propagates_output_range_violation_details() {
    let program = compile_to_runtime(PLC_EXTERN_FIXTURE, 1);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(1, 1, 0, 0);
    let mut registry = ExternFunctionRegistry::with_time_source(|| 0);
    registry
        .register(
            ExternFunctionInfo::new(
                "add",
                vec![VariableType::Float, VariableType::Float],
                vec![VariableType::Float],
                ExternFunctionContract {
                    rust_module: "math::basic".to_string(),
                    pure: true,
                    time_bound_us: 1000,
                },
                |args| Ok(vec![args[0] + args[1]]),
            )
            .with_output_ranges(vec![ValueRange::new(-1.0, 1.0)]),
        )
        .expect("register add with output range");

    let err = rt
        .tick_with_extern(&mut io, |function, args, outputs| {
            call_registry(&registry, function, args, outputs)
        })
        .expect_err("output range should fail");

    match err {
        RuntimeTickError::ExternCallFailed { function, error } => {
            assert_eq!(function, "add");
            assert_eq!(
                error,
                ExternRuntimeError::OutputOutOfRange {
                    function: "add".to_string(),
                    result_index: 0,
                    value: 3.5,
                    min: -1.0,
                    max: 1.0,
                }
            );
        }
        other => panic!("unexpected error variant: {other:?}"),
    }
}

#[test]
fn runtime_tick_with_extern_propagates_timeout_details() {
    let program = compile_to_runtime(PLC_EXTERN_FIXTURE, 1);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(1, 1, 0, 0);

    let ticks = Arc::new(AtomicU64::new(0));
    let clock = Arc::clone(&ticks);
    let mut registry = ExternFunctionRegistry::with_time_source(move || {
        let now = clock.load(Ordering::Relaxed);
        clock.store(now + 50, Ordering::Relaxed);
        now
    });
    registry
        .register(ExternFunctionInfo::new(
            "add",
            vec![VariableType::Float, VariableType::Float],
            vec![VariableType::Float],
            ExternFunctionContract {
                rust_module: "math::basic".to_string(),
                pure: true,
                time_bound_us: 10,
            },
            |args| Ok(vec![args[0] + args[1]]),
        ))
        .expect("register add");

    let err = rt
        .tick_with_extern(&mut io, |function, args, outputs| {
            call_registry(&registry, function, args, outputs)
        })
        .expect_err("timeout should fail");

    match err {
        RuntimeTickError::ExternCallFailed { function, error } => {
            assert_eq!(function, "add");
            assert_eq!(
                error,
                ExternRuntimeError::TimeoutExceeded {
                    function: "add".to_string(),
                    elapsed_us: 50,
                    limit_us: 10,
                }
            );
        }
        other => panic!("unexpected error variant: {other:?}"),
    }
}

const PLC_EXTERN_ERROR_FLOW_FIXTURE: &str = r#"
[topology]

extern function risky(v: float) -> float {
    rust_module: "math::risky",
    pure: true,
    time_bound_us: 1000
}

variable input: float = 2.0
variable output: float = 0.0
variable last_error: int = 0

[constraints]

[tasks]

task main:
    step invoke:
        action: call risky(input) -> output
    on_complete: goto check

task check:
    step branch:
        wait: last_error == 0
        timeout: 1ms -> goto fallback
    on_complete: goto success

task success:
    step halt:

task fallback:
    step halt:
"#;

#[test]
fn runtime_tick_with_error_code_variable_routes_to_fallback_branch() {
    let (program, topology) = compile_runtime_and_topology(PLC_EXTERN_ERROR_FLOW_FIXTURE, 1);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(1, 1, 0, 0);
    let last_error_var = variable_index(&topology, "last_error");

    let mut registry = ExternFunctionRegistry::with_time_source(|| 0);
    registry
        .register(
            ExternFunctionInfo::new(
                "risky",
                vec![VariableType::Float],
                vec![VariableType::Float],
                ExternFunctionContract {
                    rust_module: "math::risky".to_string(),
                    pure: true,
                    time_bound_us: 1000,
                },
                |args| Ok(vec![args[0]]),
            )
            .with_input_ranges(vec![ValueRange::new(-1.0, 1.0)]),
        )
        .expect("register risky");

    rt.tick_with_extern_error_code(
        &mut io,
        last_error_var,
        |function, args, outputs| call_registry(&registry, function, args, outputs),
        |_function, error| extern_runtime_error_code(error) as f32,
    )
    .expect("extern failure should be captured into last_error");

    assert_eq!(
        rt.variables()[last_error_var as usize],
        EXTERN_ERROR_CODE_INPUT_OUT_OF_RANGE as f32
    );
    assert_eq!(
        rt.variables()[1],
        0.0,
        "failed call must not overwrite outputs"
    );
    assert_eq!(current_step_name(&rt, &program), "check.branch");

    rt.tick_with_extern_error_code(
        &mut io,
        last_error_var,
        |function, args, outputs| call_registry(&registry, function, args, outputs),
        |_function, error| extern_runtime_error_code(error) as f32,
    )
    .expect("timeout branch should execute normally");

    assert_eq!(current_step_name(&rt, &program), "fallback.halt");
}

const PLC_EXTERN_RETRY_FLOW_FIXTURE: &str = r#"
[topology]

extern function flaky(v: float) -> float {
    rust_module: "math::flaky",
    pure: true,
    time_bound_us: 400
}

variable input: float = 2.0
variable output: float = 0.0
variable last_error: int = 0

[constraints]

[tasks]

task main:
    step invoke:
        action: call flaky(input) -> output
    on_complete: goto check_first

task check_first:
    step branch:
        wait: last_error == 0
        timeout: 1ms -> goto retry
    on_complete: goto success

task retry:
    step invoke_retry:
        action: call flaky(input) -> output
    on_complete: goto check_second

task check_second:
    step branch:
        wait: last_error == 0
        timeout: 1ms -> goto error
    on_complete: goto success

task success:
    step halt:

task error:
    step halt:
"#;

#[test]
fn runtime_tick_with_error_code_supports_retry_then_success_flow() {
    let (program, topology) = compile_runtime_and_topology(PLC_EXTERN_RETRY_FLOW_FIXTURE, 1);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(1, 1, 0, 0);
    let last_error_var = variable_index(&topology, "last_error");
    let mut attempts = 0usize;

    rt.tick_with_extern_error_code(
        &mut io,
        last_error_var,
        |function, args, outputs| {
            assert_eq!(function, "flaky");
            attempts += 1;
            if attempts == 1 {
                Err(ExternRuntimeError::RuntimeError {
                    function: function.to_string(),
                    message: "simulated failure".to_string(),
                })
            } else {
                outputs[0] = args[0] * 2.0;
                Ok(1)
            }
        },
        |_function, error| extern_runtime_error_code(error) as f32,
    )
    .expect("first failure should be captured");
    assert_eq!(
        rt.variables()[last_error_var as usize],
        EXTERN_ERROR_CODE_RUNTIME_ERROR as f32
    );
    assert_eq!(current_step_name(&rt, &program), "check_first.branch");

    rt.tick_with_extern_error_code(
        &mut io,
        last_error_var,
        |function, args, outputs| {
            assert_eq!(function, "flaky");
            attempts += 1;
            if attempts == 1 {
                Err(ExternRuntimeError::RuntimeError {
                    function: function.to_string(),
                    message: "simulated failure".to_string(),
                })
            } else {
                outputs[0] = args[0] * 2.0;
                Ok(1)
            }
        },
        |_function, error| extern_runtime_error_code(error) as f32,
    )
    .expect("retry path should succeed");

    assert_eq!(attempts, 2, "retry branch should execute one extra call");
    assert_eq!(rt.variables()[last_error_var as usize], 0.0);
    assert!((rt.variables()[1] - 4.0).abs() < f32::EPSILON);
    assert_eq!(current_step_name(&rt, &program), "success.halt");
}

#[test]
fn runtime_tick_with_error_code_rejects_out_of_range_variable_slot() {
    let program = compile_to_runtime(PLC_EXTERN_FIXTURE, 1);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(1, 1, 0, 0);
    let registry = make_add_registry();

    let err = rt
        .tick_with_extern_error_code(
            &mut io,
            999,
            |function, args, outputs| call_registry(&registry, function, args, outputs),
            |_function, _error| 1.0,
        )
        .expect_err("invalid variable slot should fail fast");
    assert_eq!(
        err,
        RuntimeTickError::Core(RuntimeError::ExternErrorCodeVariableOutOfRange {
            function: "add",
            variable: 999
        })
    );
}

const PLC_EXTERN_TICK_BUDGET_FIXTURE: &str = r#"
[topology]

extern function slow_a(v: float) -> float {
    rust_module: "math::slow_a",
    pure: true,
    time_bound_us: 700
}
extern function slow_b(v: float) -> float {
    rust_module: "math::slow_b",
    pure: true,
    time_bound_us: 500
}

variable x: float = 1.0
variable y: float = 0.0
variable z: float = 0.0

[constraints]

[tasks]

task main:
    step compute:
        action: call slow_a(x) -> y
        action: call slow_b(y) -> z
    on_complete: goto done

task done:
    step halt:
"#;

#[test]
fn bridge_rejects_program_when_extern_worst_case_exceeds_tick_budget() {
    let err = compile_to_runtime_result(PLC_EXTERN_TICK_BUDGET_FIXTURE, 1)
        .expect_err("extern tick budget overflow should fail at compile/bridge stage");

    match err {
        BridgeError::ExternTickBudgetExceeded {
            tick_ms,
            tick_budget_us,
            worst_case_us,
        } => {
            assert_eq!(tick_ms, 1);
            assert_eq!(tick_budget_us, 1_000);
            assert_eq!(worst_case_us, 1_200);
        }
        other => panic!("unexpected bridge error: {other:?}"),
    }
}

const PLC_EXTERN_QUADRATIC_TUPLE_FIXTURE: &str = r#"
[topology]

extern function quadratic_fit(
    x1: float,
    x2: float,
    x3: float,
    x4: float,
    x5: float,
    y1: float,
    y2: float,
    y3: float,
    y4: float,
    y5: float
) -> (float, float, float) {
    rust_module: "math::fit",
    pure: true,
    time_bound_us: 1000
}

variable x1: float = 0.0
variable x2: float = 1.0
variable x3: float = 2.0
variable x4: float = 3.0
variable x5: float = 4.0
variable y1: float = 1.0
variable y2: float = 6.0
variable y3: float = 17.0
variable y4: float = 34.0
variable y5: float = 57.0
variable coef_a: float = 0.0
variable coef_b: float = 0.0
variable coef_c: float = 0.0

[constraints]

[tasks]

task main:
    step fit_curve:
        action: call quadratic_fit(x1, x2, x3, x4, x5, y1, y2, y3, y4, y5) -> (coef_a, coef_b, coef_c)
    on_complete: goto done

task done:
    step halt:
"#;

#[test]
fn bridge_executes_quadratic_fit_with_tuple_binding_end_to_end() {
    let (program, topology) = compile_runtime_and_topology(PLC_EXTERN_QUADRATIC_TUPLE_FIXTURE, 2);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(1, 1, 0, 0);
    let registry = ExternFunctionRegistry::new();

    rt.tick_with_extern(&mut io, |function, args, outputs| {
        call_registry(&registry, function, args, outputs)
    })
    .expect("quadratic_fit should execute");

    let coef_a = rt.variables()[variable_index(&topology, "coef_a") as usize];
    let coef_b = rt.variables()[variable_index(&topology, "coef_b") as usize];
    let coef_c = rt.variables()[variable_index(&topology, "coef_c") as usize];
    assert!((coef_a - 1.0).abs() < 1e-4, "expected a≈1, got {coef_a}");
    assert!((coef_b - 2.0).abs() < 1e-4, "expected b≈2, got {coef_b}");
    assert!((coef_c - 3.0).abs() < 1e-4, "expected c≈3, got {coef_c}");
}

const PLC_EXTERN_PID_CONTROL_FIXTURE: &str = r#"
[topology]

extern function pid_update(error: float, kp: float, ki: float, kd: float, dt: float) -> float {
    rust_module: "control::pid",
    pure: false,
    time_bound_us: 1000
}

extern function add(lhs: float, rhs: float) -> float {
    rust_module: "math::basic",
    pure: true,
    time_bound_us: 1000
}

variable error: float = 2.0
variable kp: float = 1.5
variable ki: float = 0.5
variable kd: float = 0.1
variable dt: float = 1.0
variable bias: float = 1.0
variable pid_out: float = 0.0
variable command: float = 0.0

[constraints]

[tasks]

task main:
    step control_tick_1:
        action: call pid_update(error, kp, ki, kd, dt) -> pid_out
        action: call add(pid_out, bias) -> command
    step control_tick_2:
        action: call pid_update(error, kp, ki, kd, dt) -> pid_out
        action: call add(pid_out, bias) -> command
    on_complete: goto done

task done:
    step halt:
"#;

#[test]
fn bridge_executes_pid_then_uses_output_in_followup_extern_action() {
    let (program, topology) = compile_runtime_and_topology(PLC_EXTERN_PID_CONTROL_FIXTURE, 5);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(1, 1, 0, 0);
    let registry = ExternFunctionRegistry::new();
    let pid_out_idx = variable_index(&topology, "pid_out") as usize;
    let command_idx = variable_index(&topology, "command") as usize;

    rt.tick_with_extern(&mut io, |function, args, outputs| {
        call_registry(&registry, function, args, outputs)
    })
    .expect("first control tick should execute");

    assert!(
        (rt.variables()[pid_out_idx] - 5.0).abs() < 1e-4,
        "two PID updates should run through the task in one runtime tick, got {}",
        rt.variables()[pid_out_idx]
    );
    assert!(
        (rt.variables()[command_idx] - 6.0).abs() < 1e-4,
        "follow-up add call should consume fresh pid_out from pid_update, got {}",
        rt.variables()[command_idx]
    );

    rt.tick_with_extern(&mut io, |function, args, outputs| {
        call_registry(&registry, function, args, outputs)
    })
    .expect("second control tick should execute");

    assert!(
        (rt.variables()[pid_out_idx] - 5.0).abs() < 1e-4,
        "once task reaches halt, pid output should stay stable, got {}",
        rt.variables()[pid_out_idx]
    );
    assert!(
        (rt.variables()[command_idx] - 6.0).abs() < 1e-4,
        "command should remain pid output + bias, got {}",
        rt.variables()[command_idx]
    );
}

const PLC_EXTERN_QUADRATIC_ERROR_FIXTURE: &str = r#"
[topology]

extern function quadratic_fit(
    x1: float,
    x2: float,
    x3: float,
    x4: float,
    x5: float,
    y1: float,
    y2: float,
    y3: float,
    y4: float,
    y5: float
) -> (float, float, float) {
    rust_module: "math::fit",
    pure: true,
    time_bound_us: 1000
}

variable x1: float = 1.0
variable x2: float = 1.0
variable x3: float = 1.0
variable x4: float = 1.0
variable x5: float = 1.0
variable y1: float = 2.0
variable y2: float = 3.0
variable y3: float = 4.0
variable y4: float = 5.0
variable y5: float = 6.0
variable coef_a: float = 0.0
variable coef_b: float = 0.0
variable coef_c: float = 0.0

[constraints]

[tasks]

task main:
    step fit_curve:
        action: call quadratic_fit(x1, x2, x3, x4, x5, y1, y2, y3, y4, y5) -> (coef_a, coef_b, coef_c)
    on_complete: goto done

task done:
    step halt:
"#;

#[test]
fn runtime_tick_with_extern_propagates_quadratic_fit_runtime_error() {
    let program = compile_to_runtime(PLC_EXTERN_QUADRATIC_ERROR_FIXTURE, 2);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(1, 1, 0, 0);
    let registry = ExternFunctionRegistry::new();

    let err = rt
        .tick_with_extern(&mut io, |function, args, outputs| {
            call_registry(&registry, function, args, outputs)
        })
        .expect_err("singular quadratic inputs should fail at runtime");

    match err {
        RuntimeTickError::ExternCallFailed { function, error } => {
            assert_eq!(function, "quadratic_fit");
            match error {
                ExternRuntimeError::RuntimeError { function, message } => {
                    assert_eq!(function, "quadratic_fit");
                    assert!(
                        message.contains("singular matrix"),
                        "expected singular-matrix message, got {message}"
                    );
                }
                other => panic!("unexpected nested error variant: {other:?}"),
            }
        }
        other => panic!("unexpected tick error variant: {other:?}"),
    }
}
