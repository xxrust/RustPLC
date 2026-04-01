use io_traits::{AnalogInputId, DigitalInputId, DigitalOutputId, Io, Tick};
use runtime_core::{
    AXIS_FAULT_POLICY_LOG_MESSAGE, AXIS_STOP_TRANSITION_COMPLETED_LOG_MESSAGE,
    AXIS_STOP_TRANSITION_ENTER_LOG_MESSAGE, AxisAutoResetPolicy, AxisFault, AxisFaultKind,
    AxisFaultPropagationScope, AxisFaultSeverity, AxisMotionResult, AxisMoveKind, AxisStopMode,
    AxisStopState, AxisStopTransitionPhase, CylinderFeedbackFault, Instr, Runtime, RuntimeError,
    RuntimeTickError,
    axis_fault_policy_log_message_id, axis_stop_transition_log_message_id,
};
use rust_plc::extern_functions::{
    EXTERN_ERROR_CODE_INPUT_OUT_OF_RANGE, EXTERN_ERROR_CODE_RUNTIME_ERROR, ExternFunctionInfo,
    ExternFunctionRegistry, ExternRuntimeError, ValueRange, extern_runtime_error_code,
};
use rust_plc::ir::{ExternFunctionContract, TopologyGraph, VariableType};
use rust_plc::parser::parse_plc;
use rust_plc::runtime_bridge::{BridgeError, state_machine_to_runtime_program};
use rust_plc::semantic::{
    build_constraint_set, build_state_machine, build_topology_graph, preprocess_program,
};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

fn compile_to_runtime(plc_source: &str, tick_ms: u64) -> runtime_core::Program<'static> {
    let program = parse_plc(plc_source).expect("parse plc");
    // Keep preprocessing in the pipeline so repeat expansion (etc.) stays consistent.
    let expanded = preprocess_program(&program).expect("preprocess");
    let topology = build_topology_graph(&expanded).expect("topology");
    let constraints = build_constraint_set(&expanded).expect("constraints");
    let sm = build_state_machine(&expanded).expect("state machine");
    state_machine_to_runtime_program(&topology, &constraints, &sm, tick_ms).expect("bridge")
}

fn compile_to_runtime_result(
    plc_source: &str,
    tick_ms: u64,
) -> Result<runtime_core::Program<'static>, BridgeError> {
    let program = parse_plc(plc_source).expect("parse plc");
    let expanded = preprocess_program(&program).expect("preprocess");
    let topology = build_topology_graph(&expanded).expect("topology");
    let constraints = build_constraint_set(&expanded).expect("constraints");
    let sm = build_state_machine(&expanded).expect("state machine");
    state_machine_to_runtime_program(&topology, &constraints, &sm, tick_ms)
}

fn compile_runtime_and_topology(
    plc_source: &str,
    tick_ms: u64,
) -> (runtime_core::Program<'static>, TopologyGraph) {
    let program = parse_plc(plc_source).expect("parse plc");
    let expanded = preprocess_program(&program).expect("preprocess");
    let topology = build_topology_graph(&expanded).expect("topology");
    let constraints = build_constraint_set(&expanded).expect("constraints");
    let sm = build_state_machine(&expanded).expect("state machine");
    let runtime_program =
        state_machine_to_runtime_program(&topology, &constraints, &sm, tick_ms).expect("bridge");
    (runtime_program, topology)
}

fn compile_example_to_runtime(file_name: &str, tick_ms: u64) -> runtime_core::Program<'static> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(file_name);
    let source = fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {} failed: {err}", path.display()));
    compile_to_runtime(&source, tick_ms)
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

fn task_step_name<'a>(
    rt: &Runtime<'a>,
    program: &'a runtime_core::Program<'a>,
    task_idx: usize,
) -> &'a str {
    let step_id = rt
        .task_context(task_idx)
        .expect("task context should exist")
        .current_step;
    program
        .task(task_idx)
        .expect("task should exist")
        .step(step_id)
        .expect("step should exist")
        .name
}

const PLC_FIXTURE: &str = r#"
[topology]

device Y0: digital_output
device X0: digital_input

device start_button: sensor

relation { from: start_button.out, to: X0.in, via: reports_to }

[constraints]

[tasks]

task main:
    step extend:
        action: set Y0 on

    step wait_button:
        wait: start_button == true
        timeout: 50ms -> goto fault

    step dwell:
        delay: 20ms

    step retract:
        action: set Y0 off

    on_complete: goto done

task fault:
    step retract_fault:
        action: set Y0 off
    on_complete: goto done

task done:
    step halt:
"#;

const PLC_MULTI_ROOT_TASK_FIXTURE: &str = r#"
[topology]

[constraints]

[tasks]
task load:
    step run:
        action: log "load"
    step halt:

task unload:
    step run:
        action: log "unload"
    step halt:
"#;

#[test]
fn bridge_preserves_task_boundaries_for_independent_roots() {
    let program = compile_to_runtime(PLC_MULTI_ROOT_TASK_FIXTURE, 1);
    assert_eq!(program.tasks.len(), 2, "bridge should keep both root tasks");
    assert_eq!(program.tasks[0].name, "load");
    assert_eq!(program.tasks[1].name, "unload");
    assert_eq!(
        program.tasks[0]
            .step(program.tasks[0].entry)
            .expect("load entry step")
            .name,
        "load.run"
    );
    assert_eq!(
        program.tasks[1]
            .step(program.tasks[1].entry)
            .expect("unload entry step")
            .name,
        "unload.run"
    );
    for task in program.tasks {
        let task_prefix = format!("{}.", task.name);
        assert!(
            task.steps
                .iter()
                .all(|step| step.name.starts_with(&task_prefix)),
            "task {} should only contain local steps",
            task.name
        );
    }
}

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

const PLC_CYLINDER_STATE_WAIT_FIXTURE: &str = r#"
[topology]

device plc_main: plc {
    model_ref: openplc_softplc
}

device valve_A: solenoid_valve
device cyl_A: cylinder
device sensor_ext: sensor
device sensor_ret: sensor

relation { from: plc_main.Y0, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_ext.sense, via: detects }
relation { from: sensor_ext.out, to: plc_main.X0, via: reports_to }
relation { from: cyl_A.retracted, to: sensor_ret.sense, via: detects }
relation { from: sensor_ret.out, to: plc_main.X1, via: reports_to }

[constraints]

[tasks]

task main:
    step extend:
        action: extend cyl_A

    step wait_extended:
        wait: cyl_A.extended == true
        timeout: 50ms -> goto fault

    step done:
        goto done.halt

task fault:
    step halt:

task done:
    step halt:
"#;

const PLC_CYLINDER_STATE_FALSE_FIXTURE: &str = r#"
[topology]

device plc_main: plc {
    model_ref: openplc_softplc
}

device valve_A: solenoid_valve
device cyl_A: cylinder
device sensor_ext: sensor
device sensor_ret: sensor

relation { from: plc_main.Y0, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_ext.sense, via: detects }
relation { from: sensor_ext.out, to: plc_main.X0, via: reports_to }
relation { from: cyl_A.retracted, to: sensor_ret.sense, via: detects }
relation { from: sensor_ret.out, to: plc_main.X1, via: reports_to }

[constraints]

[tasks]

task main:
    step wait_not_retracted:
        wait: cyl_A.retracted == false
        timeout: 50ms -> goto done

    step halt:

task done:
    step halt:
"#;

const PLC_CYLINDER_SENSOR_WAIT_FIXTURE: &str = r#"
[topology]

device plc_main: plc {
    model_ref: openplc_softplc
}

device valve_A: solenoid_valve
device cyl_A: cylinder
device sensor_ext: sensor
device sensor_ret: sensor

relation { from: plc_main.Y0, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_ext.sense, via: detects }
relation { from: sensor_ext.out, to: plc_main.X0, via: reports_to }
relation { from: cyl_A.retracted, to: sensor_ret.sense, via: detects }
relation { from: sensor_ret.out, to: plc_main.X1, via: reports_to }

[constraints]

[tasks]

task main:
    step wait_sensor:
        wait: sensor_ext == true
        timeout: 50ms -> goto done

    step halt:

task done:
    step halt:
"#;

const PLC_CYLINDER_INPUT_WAIT_FIXTURE: &str = r#"
[topology]

device plc_main: plc {
    model_ref: openplc_softplc
}

device valve_A: solenoid_valve
device cyl_A: cylinder
device sensor_ext: sensor
device sensor_ret: sensor

relation { from: plc_main.Y0, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_ext.sense, via: detects }
relation { from: sensor_ext.out, to: plc_main.X0, via: reports_to }
relation { from: cyl_A.retracted, to: sensor_ret.sense, via: detects }
relation { from: sensor_ret.out, to: plc_main.X1, via: reports_to }

[constraints]

[tasks]

task main:
    step wait_input:
        wait: plc_main.X0 == true
        timeout: 50ms -> goto done

    step halt:

task done:
    step halt:
"#;

const PLC_CYLINDER_INPUT_ALIAS_WAIT_FIXTURE: &str = r#"
[topology]

device Y0: digital_output
device X0: digital_input
device X1: digital_input

device valve_A: solenoid_valve
device cyl_A: cylinder
device sensor_ext: sensor
device sensor_ret: sensor

relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_ext.sense, via: detects }
relation { from: sensor_ext.out, to: X0.in, via: reports_to }
relation { from: cyl_A.retracted, to: sensor_ret.sense, via: detects }
relation { from: sensor_ret.out, to: X1.in, via: reports_to }

[constraints]

[tasks]

task main:
    step wait_input_alias:
        wait: X0 == true
        timeout: 50ms -> goto done

    step halt:

task done:
    step halt:
"#;

const PLC_CYLINDER_ACTION_TIMEOUT_FIXTURE: &str = r#"
[topology]

device plc_main: plc {
    model_ref: openplc_softplc
}

device valve_A: solenoid_valve
device cyl_A: cylinder
device sensor_ext: sensor
device sensor_ret: sensor

relation { from: plc_main.Y0, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_ext.sense, via: detects }
relation { from: sensor_ext.out, to: plc_main.X0, via: reports_to }
relation { from: cyl_A.retracted, to: sensor_ret.sense, via: detects }
relation { from: sensor_ret.out, to: plc_main.X1, via: reports_to }

[constraints]

[tasks]

task main:
    step extend:
        action: extend cyl_A
        timeout: 50ms -> goto fault

    step done:
        goto done.halt

task fault:
    step halt:

task done:
    step halt:
"#;

const PLC_CYLINDER_PARTIAL_FEEDBACK_FIXTURE: &str = r#"
[topology]

device plc_main: plc {
    model_ref: openplc_softplc
}

device valve_A: solenoid_valve
device cyl_A: cylinder
device sensor_ext: sensor

relation { from: plc_main.Y0, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_ext.sense, via: detects }
relation { from: sensor_ext.out, to: plc_main.X0, via: reports_to }

[constraints]

[tasks]

task main:
    step extend:
        action: extend cyl_A
        timeout: 50ms -> goto fault

    step done:
        goto done.halt

task fault:
    step halt:

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

#[test]
fn bridge_rejects_cylinder_state_true_guard_now_that_feedback_is_action_owned() {
    let err = compile_to_runtime_result(PLC_CYLINDER_STATE_WAIT_FIXTURE, 10)
        .expect_err("cylinder state == true guard should be rejected");
    assert!(
        matches!(
            err,
            BridgeError::UnsupportedGuardExpression { ref expression, .. }
            if expression == "cyl_A.extended == true"
        ),
        "expected unsupported guard error, got {err:?}"
    );
}

#[test]
fn bridge_lowers_closed_loop_cylinder_action_timeout_into_pending_motion() {
    let program = compile_to_runtime(PLC_CYLINDER_ACTION_TIMEOUT_FIXTURE, 10);
    let extend_step = program.tasks[0]
        .steps
        .iter()
        .find(|step| step.name == "main.extend")
        .expect("extend step exists");

    match extend_step.instr {
        Instr::Action { actions, .. } => match actions {
            [runtime_core::Action::CylinderMotion {
                target,
                expect_extended,
                confirm_inputs,
                opposing_inputs,
                timeout: Some(timeout),
                ..
            }] => {
                assert_eq!(*target, "cyl_A");
                assert!(*expect_extended);
                assert_eq!(*confirm_inputs, [DigitalInputId(0)]);
                assert_eq!(*opposing_inputs, [DigitalInputId(1)]);
                assert_eq!(timeout.after_ticks, 5);
            }
            other => panic!("expected single cylinder motion action, got {other:?}"),
        },
        other => panic!("expected Action instr, got {other:?}"),
    }
}

#[test]
fn bridge_rejects_cylinder_state_false_guard_until_semantics_are_closed() {
    let err = compile_to_runtime_result(PLC_CYLINDER_STATE_FALSE_FIXTURE, 10)
        .expect_err("state == false should be rejected for cylinders");
    assert!(
        matches!(
            err,
            BridgeError::UnsupportedGuardExpression { ref expression, .. }
            if expression == "cyl_A.retracted == false"
        ),
        "expected unsupported guard error, got {err:?}"
    );
}

#[test]
fn bridge_rejects_raw_sensor_wait_for_cylinder_end_feedback() {
    let err = compile_to_runtime_result(PLC_CYLINDER_SENSOR_WAIT_FIXTURE, 10)
        .expect_err("raw sensor wait should be rejected for cylinder end feedback");
    assert!(
        matches!(
            err,
            BridgeError::UnsupportedGuardExpression { ref expression, .. }
            if expression == "sensor_ext == true"
        ),
        "expected unsupported guard error, got {err:?}"
    );
}

#[test]
fn bridge_rejects_raw_plc_input_wait_for_cylinder_end_feedback() {
    let err = compile_to_runtime_result(PLC_CYLINDER_INPUT_WAIT_FIXTURE, 10)
        .expect_err("raw plc input wait should be rejected for cylinder end feedback");
    assert!(
        matches!(
            err,
            BridgeError::UnsupportedGuardExpression { ref expression, .. }
            if expression == "plc_main.X0"
        ),
        "expected unsupported guard error, got {err:?}"
    );
}

#[test]
fn bridge_rejects_raw_input_alias_wait_for_cylinder_end_feedback() {
    let err = compile_to_runtime_result(PLC_CYLINDER_INPUT_ALIAS_WAIT_FIXTURE, 10)
        .expect_err("raw digital_input alias wait should be rejected for cylinder end feedback");
    assert!(
        matches!(
            err,
            BridgeError::UnsupportedGuardExpression { ref expression, .. }
            if expression == "X0 == true"
        ),
        "expected unsupported guard error, got {err:?}"
    );
}

#[test]
fn runtime_completes_closed_loop_cylinder_action_via_feedback_without_explicit_wait() {
    let program = compile_to_runtime(PLC_CYLINDER_ACTION_TIMEOUT_FIXTURE, 10);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(2, 1, 0, 0);

    io.schedule_digital_input(Tick(1), DigitalInputId(0), true);
    io.schedule_digital_input(Tick(1), DigitalInputId(1), false);

    rt.tick(&mut io).expect("tick extend");
    rt.tick(&mut io).expect("tick complete");

    assert_eq!(
        io.digital_output_edges(),
        &[sim::DigitalEdge {
            tick: Tick(0),
            id: DigitalOutputId(0),
            value: true,
        }]
    );
    assert_eq!(current_step_name(&rt, &program), "done.halt");
}

#[test]
fn runtime_rejects_contradictory_cylinder_feedback_for_closed_loop_action() {
    let program = compile_to_runtime(PLC_CYLINDER_ACTION_TIMEOUT_FIXTURE, 10);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(2, 1, 0, 0);
    io.schedule_digital_input(Tick(1), DigitalInputId(0), true);
    io.schedule_digital_input(Tick(1), DigitalInputId(1), true);

    rt.tick(&mut io).expect("tick pending action start");
    let err = rt.tick(&mut io).expect_err("contradictory feedback should fault at action layer");
    assert_eq!(
        err,
        RuntimeError::CylinderFeedbackFault {
            target: "cyl_A",
            fault: CylinderFeedbackFault::ContradictoryFeedback,
        }
    );
}

#[test]
fn runtime_rejects_reasserted_opposing_feedback_for_closed_loop_action() {
    let program = compile_to_runtime(PLC_CYLINDER_ACTION_TIMEOUT_FIXTURE, 10);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(2, 1, 0, 0);
    io.schedule_digital_input(Tick(0), DigitalInputId(1), true);
    io.schedule_digital_input(Tick(1), DigitalInputId(1), false);
    io.schedule_digital_input(Tick(2), DigitalInputId(1), true);

    rt.tick(&mut io).expect("tick start while still on opposing end");
    rt.tick(&mut io).expect("tick after opposing feedback clears");
    let err = rt
        .tick(&mut io)
        .expect_err("reasserted opposing feedback should fault after motion leaves start end");
    assert_eq!(
        err,
        RuntimeError::CylinderFeedbackFault {
            target: "cyl_A",
            fault: CylinderFeedbackFault::OppositeFeedback,
        }
    );
}

#[test]
fn runtime_times_out_closed_loop_cylinder_action_without_feedback() {
    let program = compile_to_runtime(PLC_CYLINDER_ACTION_TIMEOUT_FIXTURE, 10);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(2, 1, 0, 0);
    let mut trace = sim::JsonlTraceRecorder::new();

    for _ in 0..7 {
        rt.tick_with_trace(&mut io, |event| trace.record(event))
            .expect("tick");
    }

    let out = trace.into_string();
    assert!(
        out.contains("\"reason\":\"timeout\""),
        "missing cylinder feedback should trigger action timeout, got trace: {out}"
    );
    assert_eq!(current_step_name(&rt, &program), "fault.halt");
}

#[test]
fn bridge_rejects_partially_wired_closed_loop_cylinder_motion() {
    let err = compile_to_runtime_result(PLC_CYLINDER_PARTIAL_FEEDBACK_FIXTURE, 10)
        .expect_err("partially wired cylinder feedback should not silently degrade");
    assert!(
        matches!(err, BridgeError::IncompleteClosedLoopCylinderMotion { .. }),
        "expected incomplete closed-loop cylinder motion error, got {err:?}"
    );
}

const PLC_STEPPER_PORT_FIXTURE: &str = r#"
[topology]

device plc_main: plc { model_ref: openplc_softplc }
device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default }

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
device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default }

[constraints]

[tasks]
task motion:
    step run:
        action: axis.move_relative(axis_x, distance: 10, params: stepper_default_fast, speed: 2)
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

const PLC_AXIS_OVERSPEED_FIXTURE: &str = r#"
[topology]
device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default }

[constraints]

[tasks]
task motion:
    step run:
        action: axis.move_relative(axis_x, distance: 10, params: stepper_default_fast, speed: 3500)
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

const PLC_AXIS_ABSOLUTE_ONLY_FIXTURE: &str = r#"
[topology]
device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default }

[constraints]

[tasks]
task motion:
    step run:
        action: axis.move_absolute(axis_x, position: 100, params: stepper_default_fast, speed: 2)
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

const PLC_AXIS_RELATIVE_THEN_ABSOLUTE_FIXTURE: &str = r#"
[topology]
device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default }

[constraints]

[tasks]
task motion:
    step home:
        action: axis.move_relative(axis_x, distance: 10, params: stepper_default_fast, speed: 2)
            timeout: 500ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
    step run:
        action: axis.move_absolute(axis_x, position: 100, params: stepper_default_fast, speed: 2)
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

const PLC_AXIS_ROUTE_TERMINAL_FIXTURE: &str = r#"
[topology]
device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default }

[constraints]

[tasks]
task motion:
    step run:
        action: axis.move_relative(axis_x, distance: 10, params: stepper_default_fast, speed: 2)
            timeout: 500ms -> timeout_fault.halt
            on_reject -> reject_fault.halt
            on_motion_fault -> motion_fault.halt
            on_safety_fault -> safety_fault.halt
    on_complete: goto done.halt

task timeout_fault:
    step halt:

task reject_fault:
    step halt:

task motion_fault:
    step halt:

task safety_fault:
    step halt:

task done:
    step halt:
"#;

fn axis_fault_policy_fixture(
    severity: &str,
    stop_mode: &str,
    auto_reset_policy: &str,
    manual_ack_required: bool,
    propagation_scope: &str,
    propagation_targets: Option<&str>,
) -> String {
    let propagation_targets_line = propagation_targets
        .map(|targets| format!("\n    propagation_targets: {targets}"))
        .unwrap_or_default();
    format!(
        r#"
[topology]
device axis_x: stepper_motor {{ model_ref: stepper_generic, config_ref: stepper_default }}

axis_fault_contract axis_x_fault {{
    axis: axis_x
    severity: {severity}
    stop_mode: {stop_mode}
    auto_reset_policy: {auto_reset_policy}
    manual_ack_required: {manual_ack_required}
    propagation_scope: {propagation_scope}{propagation_targets_line}
}}

[constraints]

[tasks]
task motion:
    step run:
        action: axis.move_relative(axis_x, distance: 10, params: stepper_default_fast, speed: 2)
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
"#
    )
}

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
                        assert!(!command.require_homed);
                    }
                }
            }
        }
    }
    assert!(saw_axis, "axis_move should be lowered into runtime action");
}

#[test]
fn bridge_requires_homing_guard_for_unproven_axis_move_absolute() {
    let program = compile_to_runtime(PLC_AXIS_ABSOLUTE_ONLY_FIXTURE, 10);
    let mut saw_absolute = false;
    for task in program.tasks {
        for step in task.steps {
            if let runtime_core::Instr::Action { actions, .. } = step.instr {
                for action in actions {
                    if let runtime_core::Action::AxisMove { command } = action {
                        if command.kind == AxisMoveKind::Absolute {
                            saw_absolute = true;
                            assert!(
                                command.require_homed,
                                "unproven absolute move should keep runtime homing guard"
                            );
                        }
                    }
                }
            }
        }
    }
    assert!(saw_absolute, "fixture should lower one absolute axis move");
}

#[test]
fn bridge_elides_homing_guard_after_proven_relative_move() {
    let program = compile_to_runtime(PLC_AXIS_RELATIVE_THEN_ABSOLUTE_FIXTURE, 10);
    let mut saw_absolute = false;
    for task in program.tasks {
        for step in task.steps {
            if let runtime_core::Instr::Action { actions, .. } = step.instr {
                for action in actions {
                    if let runtime_core::Action::AxisMove { command } = action {
                        if command.kind == AxisMoveKind::Absolute {
                            saw_absolute = true;
                            assert!(
                                !command.require_homed,
                                "semantic proof should elide runtime homing guard"
                            );
                        }
                    }
                }
            }
        }
    }
    assert!(saw_absolute, "fixture should lower one absolute axis move");
}

#[test]
fn bridge_rejects_axis_move_when_axis_profile_is_missing() {
    let program = parse_plc(PLC_AXIS_BRIDGE_FIXTURE).expect("parse plc");
    let expanded = preprocess_program(&program).expect("preprocess");
    let mut topology = build_topology_graph(&expanded).expect("topology");
    let constraints = build_constraint_set(&expanded).expect("constraints");
    let sm = build_state_machine(&expanded).expect("state machine");
    topology.axis_profiles.clear();

    let err = state_machine_to_runtime_program(&topology, &constraints, &sm, 10)
        .expect_err("missing axis profile should fail at bridge");
    assert!(matches!(
        err,
        BridgeError::MissingAxisProfile { ref target, .. } if target == "axis_x"
    ));
}

#[test]
fn bridge_rejects_axis_move_speed_exceeding_profile_limit() {
    let err = compile_to_runtime_result(PLC_AXIS_OVERSPEED_FIXTURE, 10)
        .expect_err("overspeed should fail at bridge");
    match err {
        BridgeError::AxisSpeedOutOfRange {
            target,
            speed,
            max_speed,
            ..
        } => {
            assert_eq!(target, "axis_x");
            assert!((speed - 3500.0).abs() < f32::EPSILON);
            assert!((max_speed - 3000.0).abs() < f32::EPSILON);
        }
        other => panic!("expected AxisSpeedOutOfRange, got {other:?}"),
    }
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
fn axis_move_blocks_current_step_without_explicit_wait_until_done() {
    let program = compile_to_runtime(PLC_AXIS_BRIDGE_FIXTURE, 10);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(1, 1, 0, 0);
    let mut calls = 0usize;

    rt.tick_with_axis(&mut io, |command| {
        calls += 1;
        assert_eq!(command.target, "axis_x");
        AxisMotionResult::Pending
    })
    .expect("pending axis move should keep the action step active");
    assert_eq!(calls, 1);
    assert_eq!(current_step_name(&rt, &program), "motion.run");

    rt.tick_with_axis(&mut io, |command| {
        calls += 1;
        assert_eq!(command.target, "axis_x");
        AxisMotionResult::Done
    })
    .expect("done polling result should release the blocked action step");
    assert_eq!(calls, 2);
    assert_eq!(current_step_name(&rt, &program), "done.halt");
}

#[test]
fn axis_move_blocking_baseline_example_blocks_without_explicit_wait_until_done() {
    let program = compile_example_to_runtime("axis_move_blocking_baseline.plc", 10);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(1, 1, 0, 0);
    let mut calls = 0usize;

    rt.tick_with_axis(&mut io, |_| {
        calls += 1;
        AxisMotionResult::Pending
    })
    .expect("pending axis move should keep blocking baseline step active");
    assert_eq!(calls, 1);
    assert_eq!(current_step_name(&rt, &program), "main.move_axis");

    rt.tick_with_axis(&mut io, |_| {
        calls += 1;
        AxisMotionResult::Done
    })
    .expect("done polling result should release blocking baseline step");
    assert_eq!(calls, 2);
    assert_eq!(current_step_name(&rt, &program), "main.move_done");
}

#[test]
fn load_unload_concurrent_example_keeps_load_blocked_while_unload_advances() {
    let program = compile_example_to_runtime("load_unload_concurrent_tasks.plc", 100);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(2, 2, 0, 0);

    assert_eq!(
        rt.active_task_count(),
        2,
        "example should activate two root tasks"
    );
    assert_eq!(
        task_step_name(&rt, &program, 0),
        "load_station.wait_load_request"
    );
    assert_eq!(
        task_step_name(&rt, &program, 1),
        "unload_station.wait_unload_ready"
    );

    rt.tick(&mut io)
        .expect("tick0 should keep both tasks waiting");
    assert_eq!(
        task_step_name(&rt, &program, 0),
        "load_station.wait_load_request"
    );
    assert_eq!(
        task_step_name(&rt, &program, 1),
        "unload_station.wait_unload_ready"
    );

    io.schedule_digital_input(Tick(1), DigitalInputId(1), true);
    rt.tick(&mut io)
        .expect("tick1 should allow unload task to progress independently");

    assert_eq!(
        task_step_name(&rt, &program, 0),
        "load_station.wait_load_request",
        "load task should remain blocked on missing load request"
    );
    assert_eq!(
        task_step_name(&rt, &program, 1),
        "unload_station.unload_dwell",
        "unload task should continue to its local blocking delay"
    );
}

#[test]
fn axis_move_pending_fault_routes_to_declared_branch_targets() {
    let cases = [
        (AxisMotionResult::reject(66), "reject_fault.halt"),
        (AxisMotionResult::motion_fault(67), "motion_fault.halt"),
        (AxisMotionResult::safety_fault(68), "safety_fault.halt"),
    ];

    for (fault_result, expected_step) in cases {
        let program = compile_to_runtime(PLC_AXIS_ROUTE_TERMINAL_FIXTURE, 10);
        let mut rt = Runtime::new(&program).expect("runtime init");
        let mut io = sim::SimIo::new(1, 1, 0, 0);
        let mut calls = 0usize;

        rt.tick_with_axis(&mut io, |_| {
            calls += 1;
            AxisMotionResult::Pending
        })
        .expect("pending axis move should keep the action step active");
        assert_eq!(calls, 1);
        assert_eq!(current_step_name(&rt, &program), "motion.run");

        rt.tick_with_axis(&mut io, |_| {
            calls += 1;
            fault_result
        })
        .expect("pending fault should route to declared fault branch");
        assert_eq!(calls, 2);
        assert_eq!(current_step_name(&rt, &program), expected_step);
    }
}

#[test]
fn axis_move_pending_timeout_routes_to_declared_timeout_target() {
    let program = compile_to_runtime(PLC_AXIS_ROUTE_TERMINAL_FIXTURE, 10);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(1, 1, 0, 0);

    rt.tick_with_axis(&mut io, |_| AxisMotionResult::Pending)
        .expect("first tick should start pending axis action");
    assert_eq!(current_step_name(&rt, &program), "motion.run");

    for _ in 0..49 {
        rt.tick_with_axis(&mut io, |_| AxisMotionResult::Pending)
            .expect("pending ticks before timeout should keep waiting");
        assert_eq!(current_step_name(&rt, &program), "motion.run");
    }

    rt.tick_with_axis(&mut io, |_| AxisMotionResult::Pending)
        .expect("timeout tick should route without surfacing runtime error");
    assert_eq!(current_step_name(&rt, &program), "timeout_fault.halt");
}

#[test]
fn runtime_tick_with_axis_handler_propagates_classified_faults_for_bridged_axis_action() {
    let cases = [
        (
            AxisMotionResult::reject(41),
            RuntimeError::AxisFault {
                target: "axis_x",
                fault: AxisFault::reject(41),
            },
        ),
        (
            AxisMotionResult::motion_fault(42),
            RuntimeError::AxisFault {
                target: "axis_x",
                fault: AxisFault::motion(42),
            },
        ),
        (
            AxisMotionResult::safety_fault(43),
            RuntimeError::AxisFault {
                target: "axis_x",
                fault: AxisFault::safety(43),
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
fn runtime_bridge_applies_axis_fault_policy_matrix_and_emits_policy_logs() {
    let cases = [
        (
            "recoverable",
            "controlled",
            "never",
            true,
            AxisFaultSeverity::Recoverable,
            AxisStopMode::Controlled,
            AxisAutoResetPolicy::Never,
            AxisMotionResult::reject(101),
            RuntimeError::AxisFault {
                target: "axis_x",
                fault: AxisFault::reject(101),
            },
            AxisFaultKind::Reject,
        ),
        (
            "non_recoverable",
            "quick",
            "on_clear",
            false,
            AxisFaultSeverity::NonRecoverable,
            AxisStopMode::Quick,
            AxisAutoResetPolicy::OnClear,
            AxisMotionResult::motion_fault(102),
            RuntimeError::AxisFault {
                target: "axis_x",
                fault: AxisFault::motion(102),
            },
            AxisFaultKind::Motion,
        ),
        (
            "safety",
            "immediate",
            "immediate",
            true,
            AxisFaultSeverity::Safety,
            AxisStopMode::Immediate,
            AxisAutoResetPolicy::Immediate,
            AxisMotionResult::safety_fault(103),
            RuntimeError::AxisFault {
                target: "axis_x",
                fault: AxisFault::safety(103),
            },
            AxisFaultKind::Safety,
        ),
    ];

    for (
        severity_src,
        stop_mode_src,
        auto_reset_src,
        manual_ack_required,
        expected_severity,
        expected_stop_mode,
        expected_auto_reset,
        axis_result,
        expected_error,
        expected_fault_kind,
    ) in cases
    {
        let source = axis_fault_policy_fixture(
            severity_src,
            stop_mode_src,
            auto_reset_src,
            manual_ack_required,
            "self",
            None,
        );
        let program = compile_to_runtime(&source, 10);

        assert_eq!(program.axis_fault_policies.len(), 1);
        let policy = &program.axis_fault_policies[0];
        assert_eq!(policy.axis, "axis_x");
        assert_eq!(policy.severity, expected_severity);
        assert_eq!(policy.stop_mode, expected_stop_mode);
        assert_eq!(policy.auto_reset_policy, expected_auto_reset);
        assert_eq!(policy.manual_ack_required, manual_ack_required);
        assert_eq!(
            policy.propagation_scope,
            AxisFaultPropagationScope::SelfOnly
        );
        assert_eq!(policy.propagation_targets, ["axis_x"]);

        let mut rt = Runtime::new(&program).expect("runtime init");
        let mut io = sim::SimIo::new(1, 1, 0, 0);
        let mut logs = Vec::new();

        let err = rt
            .tick_with_axis_and_logs(&mut io, |event| logs.push(event), |_| axis_result)
            .expect_err("axis fault should be surfaced");
        assert_eq!(err, expected_error);
        assert_eq!(rt.axis_stop_state(), AxisStopState::Stopped);
        assert_eq!(
            logs.len(),
            3,
            "axis fault policy should emit policy+stop logs"
        );
        assert_eq!(logs[0].message, AXIS_FAULT_POLICY_LOG_MESSAGE);
        assert_eq!(
            logs[0].message_id,
            axis_fault_policy_log_message_id(
                expected_severity,
                expected_stop_mode,
                expected_auto_reset,
                manual_ack_required,
                expected_fault_kind,
            )
        );
        assert_eq!(logs[1].message, AXIS_STOP_TRANSITION_ENTER_LOG_MESSAGE);
        assert_eq!(
            logs[1].message_id,
            axis_stop_transition_log_message_id(expected_stop_mode, AxisStopTransitionPhase::Enter,)
        );
        assert_eq!(logs[2].message, AXIS_STOP_TRANSITION_COMPLETED_LOG_MESSAGE);
        assert_eq!(
            logs[2].message_id,
            axis_stop_transition_log_message_id(
                expected_stop_mode,
                AxisStopTransitionPhase::Completed,
            )
        );
    }
}

#[test]
fn runtime_bridge_lowers_axis_fault_propagation_scope_matrix() {
    let cases = [
        (
            "self",
            None,
            AxisFaultPropagationScope::SelfOnly,
            vec!["axis_x"],
        ),
        ("all", None, AxisFaultPropagationScope::All, vec!["axis_x"]),
        (
            "group",
            None,
            AxisFaultPropagationScope::Group,
            vec!["axis_x"],
        ),
        (
            "custom",
            Some("[axis_x]"),
            AxisFaultPropagationScope::Custom,
            vec!["axis_x"],
        ),
    ];

    for (scope_src, targets_src, expected_scope, expected_targets) in cases {
        let source = axis_fault_policy_fixture(
            "recoverable",
            "controlled",
            "never",
            false,
            scope_src,
            targets_src,
        );
        let program = compile_to_runtime(&source, 10);
        assert_eq!(program.axis_fault_policies.len(), 1);
        let policy = &program.axis_fault_policies[0];
        assert_eq!(policy.propagation_scope, expected_scope);
        assert_eq!(policy.propagation_targets, expected_targets.as_slice());
    }
}

#[test]
fn runtime_bridge_resolves_master_fault_followers_and_follower_isolation() {
    let source = r#"
[topology]
device axis_master: servo_drive {
    model_ref: servo_generic
    config_ref: servo_default
}
device axis_follower: servo_drive {
    model_ref: servo_generic
    config_ref: servo_default
}
device cam_link: cam_coupling {
    master: axis_master
    slave: axis_follower
    table: servo_cam_profile
}
cam_table servo_cam_profile: oneshot[(0.0, 0.0), (180.0, 180.0)]

axis_fault_contract axis_master_fault {
    axis: axis_master
    severity: safety
    stop_mode: immediate
    auto_reset_policy: never
    manual_ack_required: true
    propagation_scope: followers
}
axis_fault_contract axis_follower_fault {
    axis: axis_follower
    severity: recoverable
    stop_mode: controlled
    auto_reset_policy: on_clear
    manual_ack_required: false
    propagation_scope: followers
}

[constraints]

[tasks]
task main:
    step run:
        action: axis.move_relative(axis_master, distance: 10, speed: 5, acc: 5, dec: 5)
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

    let program = parse_plc(source).expect("parse plc");
    let expanded = preprocess_program(&program).expect("preprocess");
    let topology = build_topology_graph(&expanded).expect("topology");

    assert_eq!(topology.axis_fault_contracts.len(), 2);

    let master_policy = topology
        .axis_fault_contracts
        .iter()
        .find(|contract| contract.axis == "axis_master")
        .expect("master policy should exist");
    assert_eq!(
        master_policy.propagation_targets,
        ["axis_master", "axis_follower"]
    );

    let follower_policy = topology
        .axis_fault_contracts
        .iter()
        .find(|contract| contract.axis == "axis_follower")
        .expect("follower policy should exist");
    assert_eq!(follower_policy.propagation_targets, ["axis_follower"]);
}

#[test]
fn bridge_executes_axis_stepper_example_done_path_end_to_end() {
    let program = compile_example_to_runtime("axis_stepper_fault_routing.plc", 10);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(1, 1, 0, 0);
    let mut saw_axis = false;

    rt.tick_with_axis(&mut io, |command| {
        saw_axis = true;
        assert_eq!(command.target, "axis_stepper");
        assert_eq!(command.kind, AxisMoveKind::Relative);
        AxisMotionResult::Done
    })
    .expect("axis stepper example should execute done path");

    assert!(
        saw_axis,
        "axis handler should be invoked for stepper example"
    );
    assert_eq!(current_step_name(&rt, &program), "main.done");
}

#[test]
fn bridge_executes_axis_servo_example_fault_path_end_to_end() {
    let program = compile_example_to_runtime("axis_servo_fault_routing.plc", 10);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(1, 1, 0, 0);
    let mut invoked = false;

    let err = rt
        .tick_with_axis(&mut io, |command| {
            invoked = true;
            assert_eq!(command.target, "axis_servo");
            assert_eq!(command.kind, AxisMoveKind::Absolute);
            AxisMotionResult::motion_fault(88)
        })
        .expect_err("servo absolute move should be blocked before homing");
    assert_eq!(
        err,
        RuntimeError::AxisNotHomed {
            target: "axis_servo"
        }
    );
    assert!(
        !invoked,
        "runtime homing guard should run before axis handler"
    );
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

const PLC_SRI_STATE_CONFLICT_FIXTURE: &str = r#"
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
    step hold:
        action: log "hold"

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

const PLC_SRI_PENDING_CONFLICT_FIXTURE: &str = r#"
[topology]

device axis_a: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default, motion_param_set: stepper_default_fast }
device axis_b: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default, motion_param_set: stepper_default_fast }

resource slide_pick_zone: semantic_resource { mode: exclusive }

[constraints]

claim: action_tag arm_pick_to_slide_a occupies slide_pick_zone
claim: action_tag arm_pick_to_slide_b occupies slide_pick_zone

[tasks]

task arm_a:
    step move:
        action: axis.move_relative(axis_a, distance: 5, speed: 10)
            timeout: 100ms -> fault.timeout_a
            on_reject -> fault.reject_a
            on_motion_fault -> fault.motion_fault_a
            on_safety_fault -> fault.safety_fault_a
            semantic_tag: arm_pick_to_slide_a
    step done:
        action: log "done_a"

task arm_b:
    step move:
        action: axis.move_relative(axis_b, distance: 5, speed: 10)
            timeout: 100ms -> fault.timeout_b
            on_reject -> fault.reject_b
            on_motion_fault -> fault.motion_fault_b
            on_safety_fault -> fault.safety_fault_b
            semantic_tag: arm_pick_to_slide_b
    step done:
        action: log "done_b"

task fault:
    step timeout_a:
        action: log "timeout_a"
    step reject_a:
        action: log "reject_a"
    step motion_fault_a:
        action: log "motion_a"
    step safety_fault_a:
        action: log "safety_a"
    step timeout_b:
        action: log "timeout_b"
    step reject_b:
        action: log "reject_b"
    step motion_fault_b:
        action: log "motion_b"
    step safety_fault_b:
        action: log "safety_b"
"#;

#[test]
fn runtime_routes_axis_move_to_safety_fault_when_state_claim_occupies_resource() {
    let program = compile_to_runtime(PLC_SRI_STATE_CONFLICT_FIXTURE, 10);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(0, 1, 0, 0);
    let mut motion_calls = 0usize;

    rt.tick_with_axis(&mut io, |_| {
        motion_calls += 1;
        AxisMotionResult::Done
    })
    .expect("runtime tick should route instead of failing");

    assert_eq!(
        motion_calls, 0,
        "conflicted axis move should be blocked before handler"
    );
    assert_eq!(task_step_name(&rt, &program, 0), "feeder.hold");
    assert_eq!(task_step_name(&rt, &program, 1), "fault.safety_fault");
}

#[test]
fn runtime_keeps_action_tag_claim_active_while_axis_move_is_pending() {
    let program = compile_to_runtime(PLC_SRI_PENDING_CONFLICT_FIXTURE, 10);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(0, 0, 0, 0);
    let mut seen_tags = Vec::new();

    rt.tick_with_axis(&mut io, |command| {
        seen_tags.push(command.semantic_tag.map(str::to_string));
        if command.target == "axis_a" {
            AxisMotionResult::Pending
        } else {
            AxisMotionResult::Done
        }
    })
    .expect("tick should succeed with safety routing");

    assert_eq!(
        seen_tags.len(),
        1,
        "second axis should be blocked by pending claim"
    );
    assert_eq!(seen_tags[0].as_deref(), Some("arm_pick_to_slide_a"));
    assert_eq!(task_step_name(&rt, &program, 0), "arm_a.move");
    assert_eq!(task_step_name(&rt, &program, 1), "fault.safety_fault_b");

    rt.tick_with_axis(&mut io, |_command| AxisMotionResult::Done)
        .expect("pending axis should complete on polling tick");
    assert_eq!(task_step_name(&rt, &program, 0), "arm_a.done");
}
