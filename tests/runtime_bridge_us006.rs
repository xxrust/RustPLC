use io_traits::{DigitalInputId, DigitalOutputId, Tick};
use rust_plc::parser::parse_plc;
use rust_plc::runtime_bridge::state_machine_to_runtime_program;
use rust_plc::semantic::{build_state_machine, build_topology_graph, preprocess_program};
use runtime_core::Runtime;

fn compile_to_runtime(plc_source: &str, tick_ms: u64) -> runtime_core::Program<'static> {
    let program = parse_plc(plc_source).expect("parse plc");
    // Keep preprocessing in the pipeline so repeat expansion (etc.) stays consistent.
    let expanded = preprocess_program(&program).expect("preprocess");
    let topology = build_topology_graph(&expanded).expect("topology");
    let sm = build_state_machine(&expanded).expect("state machine");
    state_machine_to_runtime_program(&topology, &sm, tick_ms).expect("bridge")
}

const PLC_FIXTURE: &str = r#"
[topology]

device Y0: digital_output
device X0: digital_input

device start_button: digital_input {
    connected_to: X0
}

device valve_A: solenoid_valve {
    connected_to: Y0
}

device cyl_A: cylinder {
    connected_to: valve_A
}

device sensor_ext: sensor {
    connected_to: X0
    detects: cyl_A.extended
}

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
        rt.tick_with_trace(&mut io, |e| trace.record(e)).expect("tick");
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
