use runtime_core::{Action, Instr, ProcessDeviceActionResult, Runtime, RuntimeError, StepId};
use rust_plc::codegen::st::{generate_st, StCodegenConfig, StCodegenError};
use rust_plc::device_semantics::process::{
    all_process_source_contracts, collect_process_device_source_reports,
};
use rust_plc::ir::TransitionAction;
use rust_plc::parser::parse_plc;
use rust_plc::runtime_bridge::state_machine_to_runtime_program;
use rust_plc::semantic::{
    build_constraint_set, build_state_machine, build_topology_graph, preprocess_program,
};
use rustplc_device_semantics::{ActionResultBucket, DefaultFeedbackPolicy};

#[test]
fn process_device_source_contracts_expose_actions_results_and_default_policy() {
    let contracts = all_process_source_contracts();
    assert_eq!(contracts.len(), 6);

    for contract in contracts {
        assert!(!contract.family.is_empty());
        assert!(!contract.command_ports.is_empty());
        assert!(!contract.required_feedback_ports.is_empty());
        assert!(!contract.actions.is_empty());
        assert_eq!(
            contract.default_feedback_policy,
            DefaultFeedbackPolicy::FeedbackRequiredUnlessExplicitOpenLoop
        );
        for action in contract.actions {
            assert!(!action.name.is_empty());
            assert!(
                action
                    .result_buckets
                    .contains(&ActionResultBucket::Complete),
                "{}.{} should expose a completion bucket",
                contract.family,
                action.name
            );
        }
    }
}

#[test]
fn process_source_report_lists_missing_feedback_ports() {
    let input = r#"
[topology]
device plc_main: plc { model_ref: openplc_softplc }
device oven: heater

[constraints]

[tasks]
task main:
    step idle:
"#;

    let program = parse_plc(input).expect("parse");
    let reports = collect_process_device_source_reports(&program.topology);
    let heater = reports
        .iter()
        .find(|report| report.device == "oven")
        .expect("heater report");

    assert_eq!(heater.family, "heater");
    assert_eq!(heater.missing_feedback_ports, vec!["temperature"]);
    assert_eq!(heater.open_loop_policy, None);
}

#[test]
fn process_feedback_contract_rejects_missing_feedback_before_runtime() {
    let input = r#"
[topology]
device plc_main: plc { model_ref: openplc_softplc }
device oven: heater

relation { from: plc_main.Y0, to: oven.power, via: driven_by }

[constraints]

[tasks]
task main:
    step idle:
"#;

    let program = parse_plc(input).expect("parse");
    let errors = build_state_machine(&program).expect_err("missing heater feedback should fail");
    let rendered = errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        rendered.contains("[PROC-001]")
            && rendered.contains("oven")
            && rendered.contains("temperature")
            && rendered.contains("open_loop_policy"),
        "expected stable process feedback diagnostic, got: {rendered}"
    );
}

#[test]
fn explicit_open_loop_policy_allows_process_device_without_feedback() {
    let input = r#"
[topology]
device plc_main: plc { model_ref: openplc_softplc }
device oven: heater { open_loop_policy: commissioned_low_risk_fixture }

relation { from: plc_main.Y0, to: oven.power, via: driven_by }

[constraints]

[tasks]
task main:
    step idle:
"#;

    let program = parse_plc(input).expect("parse");
    build_state_machine(&program).expect("explicit open-loop policy should satisfy front door");
}

#[test]
fn closed_feedback_paths_satisfy_each_process_family() {
    let input = r#"
[topology]
device plc_main: plc { model_ref: openplc_softplc }
device hand: gripper
device belt: conveyor
device coolant: pump
device oven: heater
device valve: proportional_valve
device camera: vision_sensor
device hand_gripped_sensor: sensor
device hand_released_sensor: sensor
device belt_running_sensor: sensor
device camera_ready_sensor: sensor
device camera_pass_sensor: sensor
device camera_fail_sensor: sensor

relation { from: plc_main.Y0, to: hand.cmd, via: driven_by }
relation { from: hand.gripped, to: hand_gripped_sensor.sense, via: detects }
relation { from: hand_gripped_sensor.out, to: plc_main.X0, via: reports_to }
relation { from: hand.released, to: hand_released_sensor.sense, via: detects }
relation { from: hand_released_sensor.out, to: plc_main.X1, via: reports_to }

relation { from: plc_main.Y1, to: belt.drive, via: driven_by }
relation { from: belt.running, to: belt_running_sensor.sense, via: detects }
relation { from: belt_running_sensor.out, to: plc_main.X2, via: reports_to }

relation { from: plc_main.Y2, to: coolant.drive, via: driven_by }
relation { from: coolant.pressure, to: plc_main.AI0, via: reports_to }
relation { from: coolant.flow, to: plc_main.AI1, via: reports_to }

relation { from: plc_main.Y3, to: oven.power, via: driven_by }
relation { from: oven.temperature, to: plc_main.AI2, via: reports_to }

relation { from: plc_main.AO0, to: valve.cmd, via: driven_by }
relation { from: valve.feedback, to: plc_main.AI3, via: reports_to }

relation { from: plc_main.Y4, to: camera.trigger, via: driven_by }
relation { from: camera.ready, to: camera_ready_sensor.sense, via: detects }
relation { from: camera_ready_sensor.out, to: plc_main.X3, via: reports_to }
relation { from: camera.pass, to: camera_pass_sensor.sense, via: detects }
relation { from: camera_pass_sensor.out, to: plc_main.X4, via: reports_to }
relation { from: camera.fail, to: camera_fail_sensor.sense, via: detects }
relation { from: camera_fail_sensor.out, to: plc_main.X5, via: reports_to }

[constraints]

[tasks]
task main:
    step idle:
"#;

    let program = parse_plc(input).expect("parse");
    build_state_machine(&program).expect("closed process feedback contracts should pass");
}

#[test]
fn raw_io_bypass_rejects_process_command_ports_before_feedback_contracts() {
    let input = r#"
[topology]
device plc_main: plc { model_ref: openplc_softplc }
device oven: heater
device coolant: pump
device belt: conveyor
device camera: vision_sensor

[constraints]

[tasks]
task main:
    step start:
        action: set oven.power on
        action: set coolant.drive on
        action: set belt.drive on
        action: set camera.trigger on
"#;

    let program = parse_plc(input).expect("parse");
    let errors = build_state_machine(&program).expect_err("raw process bypass should fail");
    let rendered = errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        rendered.contains("SEM-110")
            && rendered.contains("oven.power")
            && rendered.contains("coolant.drive")
            && rendered.contains("belt.drive")
            && rendered.contains("camera.trigger")
            && !rendered.contains("PROC-001"),
        "expected raw bypass diagnostics to win, got: {rendered}"
    );
}

#[test]
fn process_device_action_lowers_to_first_class_ir_and_runtime_requires_handler() {
    let input = r#"
[topology]
device plc_main: plc { model_ref: openplc_softplc }
device oven: heater { open_loop_policy: commissioned_low_risk_fixture, response_time: 50ms }

[constraints]

[tasks]
task main:
    step heat:
        action: heater.heat_to(oven, 80)
    step done:
"#;

    let program = parse_plc(input).expect("parse");
    let expanded = preprocess_program(&program).expect("preprocess");
    let topology = build_topology_graph(&expanded).expect("topology");
    let constraints = build_constraint_set(&expanded).expect("constraints");
    let state_machine = build_state_machine(&expanded).expect("state machine");

    let device_action = state_machine
        .transitions
        .iter()
        .flat_map(|transition| transition.actions.iter())
        .find_map(|action| match action {
            TransitionAction::DeviceAction {
                family,
                action_name,
                target,
                result_buckets,
                ..
            } => Some((family, action_name, target, result_buckets)),
            _ => None,
        })
        .expect("process action should lower to IR device action");

    assert_eq!(device_action.0, "heater");
    assert_eq!(device_action.1, "heat_to");
    assert_eq!(device_action.2, "oven");
    assert!(device_action.3.iter().any(|bucket| bucket == "complete"));
    assert!(device_action.3.iter().any(|bucket| bucket == "timeout"));

    let st_errors = generate_st(
        &topology,
        &constraints,
        &state_machine,
        &StCodegenConfig::default(),
    )
    .expect_err("ST backend must not silently lower process device action to raw IO");
    assert!(st_errors.iter().any(|error| matches!(
        error,
        StCodegenError::DeviceActionUnsupported { target, .. } if target == "oven"
    )));

    let runtime_program =
        state_machine_to_runtime_program(&topology, &constraints, &state_machine, 10)
            .expect("runtime bridge should preserve first-class process action");
    let Instr::Action { actions, next } = runtime_program.tasks[0].steps[0].instr else {
        panic!("process action step should lower to runtime action instruction");
    };
    assert_eq!(next, StepId(1));
    let Action::ProcessDeviceAction { command } = actions[0] else {
        panic!("process device action should remain first-class runtime action");
    };
    assert_eq!(command.family, "heater");
    assert_eq!(command.action, "heat_to");
    assert_eq!(command.target, "oven");
    assert_eq!(command.args, &["80"]);

    let mut runtime = Runtime::new(&runtime_program).expect("runtime init");
    let mut io = sim::SimIo::new(0, 0, 0, 0);
    assert_eq!(
        runtime
            .tick(&mut io)
            .expect_err("plain runtime tick must require process handler"),
        RuntimeError::ProcessDeviceActionRequiresHandler {
            family: "heater",
            action: "heat_to",
            target: "oven",
        }
    );

    runtime
        .tick_with_process_device(&mut io, |command| {
            assert_eq!(command.target, "oven");
            ProcessDeviceActionResult::Pending
        })
        .expect("handler pending should keep step active");
    assert_eq!(runtime.location().step, StepId(0));

    runtime
        .tick_with_process_device(&mut io, |_| ProcessDeviceActionResult::Done)
        .expect("handler done should advance");
    assert_eq!(runtime.location().step, StepId(1));
}

#[test]
fn process_device_action_rejects_wrong_family_or_action() {
    let input = r#"
[topology]
device plc_main: plc { model_ref: openplc_softplc }
device oven: heater { open_loop_policy: commissioned_low_risk_fixture }

[constraints]

[tasks]
task main:
    step bad:
        action: gripper.grip(oven)
        action: heater.fly_to(oven)
"#;

    let program = parse_plc(input).expect("parse");
    let errors = build_state_machine(&program).expect_err("bad device actions should fail");
    let rendered = errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("[PROC-002]"));
    assert!(rendered.contains("[PROC-003]"));
}
