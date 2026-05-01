fn process_device_program() -> Program<'static> {
    static ARGS: [&str; 1] = ["80"];
    static RESULT_BUCKETS: [&str; 5] = [
        "complete",
        "timeout",
        "reject",
        "motion_fault",
        "safety_fault",
    ];
    static ACTIONS: [Action; 1] = [Action::ProcessDeviceAction {
        command: ProcessDeviceActionCommand {
            family: "heater",
            action: "heat_to",
            target: "oven",
            port: "self",
            args: &ARGS,
            result_buckets: &RESULT_BUCKETS,
        },
    }];
    static STEPS: [Step<'static>; 2] = [
        Step {
            name: "heat",
            instr: Instr::Action {
                actions: &ACTIONS,
                next: StepId(1),
            },
        },
        Step {
            name: "done",
            instr: Instr::Halt,
        },
    ];
    static TASKS: [Task<'static>; 1] = [Task {
        name: "main",
        steps: &STEPS,
        entry: StepId(0),
    }];
    Program {
        tasks: &TASKS,
        pid_loops: &[],
        var_init: &[],
        cam_configs: &[],
        cam_tables: &[],
        axis_fault_policies: &[],
        semantic_resources: &[],
        resource_claims: &[],
        workpiece_types: &[],
        workpiece_sites: &[],
        workpiece_holders: &[],
    }
}

#[test]
fn process_device_action_requires_handler_when_using_plain_tick() {
    let program = process_device_program();
    let mut io = MemIo::new();
    let mut rt = Runtime::new(&program).unwrap();

    let err = rt
        .tick(&mut io)
        .expect_err("missing process device handler should fail");

    assert_eq!(
        err,
        RuntimeError::ProcessDeviceActionRequiresHandler {
            family: "heater",
            action: "heat_to",
            target: "oven",
        }
    );
}

#[test]
fn process_device_action_pending_then_done_transitions_once() {
    let program = process_device_program();
    let mut io = MemIo::new();
    let mut rt = Runtime::new(&program).unwrap();
    let mut calls = 0usize;

    rt.tick_with_process_device(&mut io, |command| {
        calls += 1;
        assert_eq!(command.family, "heater");
        assert_eq!(command.action, "heat_to");
        assert_eq!(command.target, "oven");
        assert_eq!(command.args, &["80"]);
        ProcessDeviceActionResult::Pending
    })
    .expect("pending process action should block step");

    assert_eq!(calls, 1);
    assert_eq!(rt.location().step, StepId(0));
    assert!(matches!(
        rt.task_context(0).unwrap().pending_action_state,
        TaskPendingActionState::ProcessDeviceAction {
            family: "heater",
            action: "heat_to",
            target: "oven",
            action_index: 0,
        }
    ));

    rt.tick_with_process_device(&mut io, |_| {
        calls += 1;
        ProcessDeviceActionResult::Done
    })
    .expect("done process action should advance step");

    assert_eq!(calls, 2);
    assert_eq!(rt.location().step, StepId(1));
}

#[test]
fn process_device_action_fault_is_explicit_runtime_error() {
    let program = process_device_program();
    let mut io = MemIo::new();
    let mut rt = Runtime::new(&program).unwrap();

    let err = rt
        .tick_with_process_device(&mut io, |_| ProcessDeviceActionResult::motion_fault(17))
        .expect_err("fault result should not be silently completed");

    assert_eq!(
        err,
        RuntimeError::ProcessDeviceActionFault {
            family: "heater",
            action: "heat_to",
            target: "oven",
            fault: ProcessDeviceActionFault {
                kind: ProcessDeviceActionFaultKind::MotionFault,
                code: 17,
            },
        }
    );
}
