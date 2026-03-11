use io_traits::{DigitalInputId, DigitalOutputId, Tick};
use runtime_core::Runtime;
use rust_plc::parser::parse_plc;
use rust_plc::runtime_bridge::state_machine_to_runtime_program;
use rust_plc::semantic::{build_state_machine, build_topology_graph, preprocess_program};

fn compile_to_runtime(plc_source: &str, tick_ms: u64) -> runtime_core::Program<'static> {
    let program = parse_plc(plc_source).expect("parse plc");
    let expanded = preprocess_program(&program).expect("preprocess");
    let topology = build_topology_graph(&expanded).expect("topology");
    let sm = build_state_machine(&expanded).expect("state machine");
    state_machine_to_runtime_program(&topology, &sm, tick_ms).expect("bridge")
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

#[test]
fn parallel_lowering_uses_branch_active_and_done_states() {
    let plc = r#"
[topology]

device plc_main: plc {
    purpose: "并行分支状态机命名验证",
    ports: [X0:digital:consumer, X1:digital:consumer]
}

device sensor_a: sensor {
    purpose: "分支 A 传感器"
}

device sensor_b: sensor {
    purpose: "分支 B 传感器"
}

relation { from: sensor_a.out, to: plc_main.X0, via: reports_to }
relation { from: sensor_b.out, to: plc_main.X1, via: reports_to }

[constraints]

[tasks]

task main:
    step sync:
        parallel:
            branch_a:
                wait: sensor_a == true
            branch_b:
                wait: sensor_b == true
    on_complete: goto done

task done:
    step halt:
"#;

    let program = parse_plc(plc).expect("parse plc");
    let expanded = preprocess_program(&program).expect("preprocess");
    let state_machine = build_state_machine(&expanded).expect("state machine");

    let state_names = state_machine
        .states
        .iter()
        .map(|state| format!("{}.{}", state.task_name, state.step_name))
        .collect::<Vec<_>>();

    assert!(
        state_names
            .iter()
            .any(|name| name == "main.sync__parallel_1_fork")
    );
    assert!(
        state_names
            .iter()
            .any(|name| name == "main.sync__parallel_1_branch_1_active")
    );
    assert!(
        state_names
            .iter()
            .any(|name| name == "main.sync__parallel_1_branch_1_done")
    );
    assert!(
        state_names
            .iter()
            .any(|name| name == "main.sync__parallel_1_branch_2_active")
    );
    assert!(
        state_names
            .iter()
            .any(|name| name == "main.sync__parallel_1_branch_2_done")
    );
    assert!(
        state_names
            .iter()
            .any(|name| name == "main.sync__parallel_1_join")
    );
}

#[test]
fn parallel_branches_can_start_outputs_same_tick() {
    let plc = r#"
[topology]

device plc_main: plc {
    purpose: "并行双轴同 tick 启动验证",
    ports: [Y0:digital:producer, Y1:digital:producer]
}

device motor_x: motor {
    purpose: "X 轴电机"
}

device motor_y: motor {
    purpose: "Y 轴电机"
}

relation { from: plc_main.Y0, to: motor_x.cmd, via: driven_by }
relation { from: plc_main.Y1, to: motor_y.cmd, via: driven_by }

[constraints]

[tasks]

task main:
    step move_together:
        parallel:
            branch_x:
                action: set motor_x.run on
            branch_y:
                action: set motor_y.run on
    on_complete: goto done

task done:
    step halt:
"#;

    let program = compile_to_runtime(plc, 10);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(0, 2, 0, 0);

    rt.tick(&mut io).expect("tick");

    let edges = io.digital_output_edges();
    assert_eq!(edges.len(), 2, "两个并行分支应在同一 tick 写出");
    assert_eq!(edges[0].tick, Tick(0));
    assert_eq!(edges[1].tick, Tick(0));
    assert!(edges.contains(&sim::DigitalEdge {
        tick: Tick(0),
        id: DigitalOutputId(0),
        value: true,
    }));
    assert!(edges.contains(&sim::DigitalEdge {
        tick: Tick(0),
        id: DigitalOutputId(1),
        value: true,
    }));
}

#[test]
fn race_prefers_first_ready_branch_deterministically() {
    let plc = r#"
[topology]

device plc_main: plc {
    purpose: "race 先完成分支确定性验证",
    ports: [X0:digital:consumer, X1:digital:consumer]
}

device sensor_a: sensor {
    purpose: "A 分支传感器"
}

device sensor_b: sensor {
    purpose: "B 分支传感器"
}

relation { from: sensor_a.out, to: plc_main.X0, via: reports_to }
relation { from: sensor_b.out, to: plc_main.X1, via: reports_to }

[constraints]

[tasks]

task main:
    step choose:
        race:
            branch_a:
                wait: sensor_a == true
                then: goto winner_a
            branch_b:
                wait: sensor_b == true
                then: goto winner_b

task winner_a:
    step hold:

task winner_b:
    step hold:
"#;

    let program = compile_to_runtime(plc, 10);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(2, 0, 0, 0);
    io.schedule_digital_input(Tick(0), DigitalInputId(0), true);
    io.schedule_digital_input(Tick(0), DigitalInputId(1), true);

    rt.tick(&mut io).expect("tick");

    assert_eq!(current_step_name(&rt, &program), "winner_a.hold");
}

#[test]
fn parallel_waits_for_slower_branch_before_join() {
    let plc = r#"
[topology]

device plc_main: plc {
    purpose: "A/B 分支先后完成回归",
    ports: [X0:digital:consumer, X1:digital:consumer]
}

device sensor_a: sensor {
    purpose: "A 分支完成传感器"
}

device sensor_b: sensor {
    purpose: "B 分支完成传感器"
}

relation { from: sensor_a.out, to: plc_main.X0, via: reports_to }
relation { from: sensor_b.out, to: plc_main.X1, via: reports_to }

[constraints]

[tasks]

task main:
    step sync:
        parallel:
            branch_a:
                wait: sensor_a == true
            branch_b:
                wait: sensor_b == true
    on_complete: goto done

task done:
    step halt:
"#;

    let program = compile_to_runtime(plc, 10);
    let mut rt = Runtime::new(&program).expect("runtime init");
    let mut io = sim::SimIo::new(2, 0, 0, 0);
    io.schedule_digital_input(Tick(0), DigitalInputId(0), true);
    io.schedule_digital_input(Tick(1), DigitalInputId(1), true);

    rt.tick(&mut io).expect("tick0");
    assert_eq!(
        current_step_name(&rt, &program),
        "main.sync__parallel_1_branch_2_active"
    );

    rt.tick(&mut io).expect("tick1");
    assert_eq!(current_step_name(&rt, &program), "done.halt");
}
