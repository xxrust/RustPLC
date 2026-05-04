    #[test]
    fn runtime_initializes_independent_task_contexts() {
        static TASK0_STEPS: [Step<'static>; 2] = [
            Step {
                name: "idle",
                instr: Instr::Halt,
            },
            Step {
                name: "entry",
                instr: Instr::Halt,
            },
        ];
        static TASK1_STEPS: [Step<'static>; 1] = [Step {
            name: "wait",
            instr: Instr::WaitDigital {
                id: DigitalInputId(0),
                equals: true,
                next: StepId(0),
                timeout: None,
            },
        }];
        static TASKS: [Task<'static>; 2] = [
            Task {
                name: "loader",
                steps: &TASK0_STEPS,
                entry: StepId(1),
            },
            Task {
                name: "unloader",
                steps: &TASK1_STEPS,
                entry: StepId(0),
            },
        ];
        static PROGRAM: Program<'static> = Program {
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
        };

        let rt = Runtime::new(&PROGRAM).expect("runtime init should succeed");
        assert_eq!(rt.active_task_count(), 2);
        assert_eq!(
            rt.location(),
            Location {
                task: 0,
                step: StepId(1),
            }
        );

        let task0 = rt.task_context(0).expect("task0 context");
        assert_eq!(task0.current_step, StepId(1));
        assert_eq!(task0.step_entered_at, None);
        assert_eq!(task0.wait_state, TaskWaitState::Ready);
        assert_eq!(task0.timeout_state, TaskTimeoutState::Inactive);
        assert_eq!(task0.pending_action_state, TaskPendingActionState::Idle);

        let task1 = rt.task_context(1).expect("task1 context");
        assert_eq!(task1.current_step, StepId(0));
        assert_eq!(task1.step_entered_at, None);
        assert_eq!(task1.wait_state, TaskWaitState::Ready);
        assert_eq!(task1.timeout_state, TaskTimeoutState::Inactive);
        assert_eq!(task1.pending_action_state, TaskPendingActionState::Idle);
    }

    #[test]
    fn runtime_tick_keeps_blocked_task_isolated_while_advancing_other_tasks() {
        static TASK0_STEPS: [Step<'static>; 2] = [
            Step {
                name: "wait_part",
                instr: Instr::WaitDigital {
                    id: DigitalInputId(0),
                    equals: true,
                    next: StepId(1),
                    timeout: Some(Timeout {
                        after_ticks: 3,
                        target: StepId(1),
                    }),
                },
            },
            Step {
                name: "halt",
                instr: Instr::Halt,
            },
        ];
        static TASK1_STEPS: [Step<'static>; 3] = [
            Step {
                name: "prepare_output",
                instr: Instr::Action {
                    actions: &[Action::SetDigital {
                        id: DigitalOutputId(0),
                        value: true,
                    }],
                    next: StepId(1),
                },
            },
            Step {
                name: "to_halt",
                instr: Instr::Goto { target: StepId(2) },
            },
            Step {
                name: "halt",
                instr: Instr::Halt,
            },
        ];
        static TASKS: [Task<'static>; 2] = [
            Task {
                name: "loader",
                steps: &TASK0_STEPS,
                entry: StepId(0),
            },
            Task {
                name: "background",
                steps: &TASK1_STEPS,
                entry: StepId(0),
            },
        ];
        static PROGRAM: Program<'static> = Program {
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
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init should succeed");
        let mut events: std::vec::Vec<TraceEvent> = std::vec::Vec::new();

        rt.tick_with_trace(&mut io, |e| events.push(e))
            .expect("tick should evaluate all tasks");

        let loader = rt.task_context(0).expect("loader context");
        assert_eq!(loader.current_step, StepId(0));
        assert_eq!(loader.step_entered_at, Some(Tick(0)));
        assert_eq!(loader.wait_state, TaskWaitState::WaitCondition);
        assert_eq!(
            loader.timeout_state,
            TaskTimeoutState::Armed {
                after_ticks: 3,
                target: StepId(1),
            }
        );
        assert_eq!(loader.pending_action_state, TaskPendingActionState::Idle);

        let background = rt.task_context(1).expect("background context");
        assert_eq!(background.current_step, StepId(2));
        assert_eq!(background.step_entered_at, Some(Tick(0)));
        assert_eq!(background.wait_state, TaskWaitState::Ready);
        assert_eq!(background.timeout_state, TaskTimeoutState::Inactive);
        assert_eq!(
            background.pending_action_state,
            TaskPendingActionState::Idle
        );
        assert!(io.do_[0]);
        assert_eq!(
            events,
            std::vec![
                TraceEvent {
                    tick: Tick(0),
                    task: 1,
                    from: StepId(0),
                    to: StepId(1),
                    reason: TransitionReason::Action,
                },
                TraceEvent {
                    tick: Tick(0),
                    task: 1,
                    from: StepId(1),
                    to: StepId(2),
                    reason: TransitionReason::Goto,
                },
            ]
        );
        assert_eq!(rt.location().task, 0);

        io.di[0] = true;
        rt.tick(&mut io)
            .expect("tick should satisfy wait and transition");

        let loader = rt.task_context(0).expect("loader context");
        assert_eq!(loader.current_step, StepId(1));
        assert_eq!(loader.step_entered_at, Some(Tick(1)));
        assert_eq!(loader.wait_state, TaskWaitState::Ready);
        assert_eq!(loader.timeout_state, TaskTimeoutState::Inactive);
    }

    #[test]
    fn digital_edge_wait_samples_initial_level_without_firing() {
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "wait_start_edge",
                instr: Instr::WaitDigitalEdge {
                    id: DigitalInputId(0),
                    edge: EdgeKind::Rising,
                    next: StepId(1),
                    timeout: None,
                },
            },
            Step {
                name: "halt",
                instr: Instr::Halt,
            },
        ];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "ready",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static PROGRAM: Program<'static> = Program {
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
        };

        let mut io = MemIo::new();
        io.di[0] = true;
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init should succeed");

        rt.tick(&mut io).expect("first tick should only sample baseline");
        assert_eq!(rt.task_context(0).unwrap().current_step, StepId(0));

        io.di[0] = false;
        rt.tick(&mut io).expect("falling level should not satisfy rising edge");
        assert_eq!(rt.task_context(0).unwrap().current_step, StepId(0));

        io.di[0] = true;
        rt.tick(&mut io).expect("false-to-true should satisfy rising edge");
        assert_eq!(rt.task_context(0).unwrap().current_step, StepId(1));
    }

    #[test]
    fn digital_falling_edge_wait_requires_true_to_false_transition() {
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "wait_stop_edge",
                instr: Instr::WaitDigitalEdge {
                    id: DigitalInputId(0),
                    edge: EdgeKind::Falling,
                    next: StepId(1),
                    timeout: None,
                },
            },
            Step {
                name: "halt",
                instr: Instr::Halt,
            },
        ];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "ready",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static PROGRAM: Program<'static> = Program {
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
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init should succeed");

        rt.tick(&mut io).expect("first low sample is baseline only");
        assert_eq!(rt.task_context(0).unwrap().current_step, StepId(0));

        io.di[0] = true;
        rt.tick(&mut io).expect("rising level should not satisfy falling edge");
        assert_eq!(rt.task_context(0).unwrap().current_step, StepId(0));

        io.di[0] = false;
        rt.tick(&mut io).expect("true-to-false should satisfy falling edge");
        assert_eq!(rt.task_context(0).unwrap().current_step, StepId(1));
    }

    #[test]
    fn digital_edge_wait_is_consumed_once_per_task_tick() {
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "wait_start_edge",
                instr: Instr::WaitDigitalEdge {
                    id: DigitalInputId(0),
                    edge: EdgeKind::Rising,
                    next: StepId(1),
                    timeout: None,
                },
            },
            Step {
                name: "return_to_wait",
                instr: Instr::Goto { target: StepId(0) },
            },
        ];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "ready",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static PROGRAM: Program<'static> = Program {
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
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init should succeed");
        let mut events: std::vec::Vec<TraceEvent> = std::vec::Vec::new();

        rt.tick(&mut io).expect("initial low sample is baseline");
        io.di[0] = true;
        rt.tick_with_trace(&mut io, |event| events.push(event))
            .expect("rising edge should be consumed once");

        assert_eq!(rt.task_context(0).unwrap().current_step, StepId(0));
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].reason, TransitionReason::WaitSatisfied);
        assert_eq!(events[1].reason, TransitionReason::Goto);
    }

    #[test]
    fn runtime_tick_schedules_tasks_in_fixed_index_order() {
        static TASK0_ACTIONS: [Action; 1] = [Action::Log {
            message_id: 10,
            message: "task0",
        }];
        static TASK1_ACTIONS: [Action; 1] = [Action::Log {
            message_id: 20,
            message: "task1",
        }];
        static TASK0_STEPS: [Step<'static>; 2] = [
            Step {
                name: "emit_log",
                instr: Instr::Action {
                    actions: &TASK0_ACTIONS,
                    next: StepId(1),
                },
            },
            Step {
                name: "halt",
                instr: Instr::Halt,
            },
        ];
        static TASK1_STEPS: [Step<'static>; 2] = [
            Step {
                name: "emit_log",
                instr: Instr::Action {
                    actions: &TASK1_ACTIONS,
                    next: StepId(1),
                },
            },
            Step {
                name: "halt",
                instr: Instr::Halt,
            },
        ];
        static TASKS: [Task<'static>; 2] = [
            Task {
                name: "first",
                steps: &TASK0_STEPS,
                entry: StepId(0),
            },
            Task {
                name: "second",
                steps: &TASK1_STEPS,
                entry: StepId(0),
            },
        ];
        static PROGRAM: Program<'static> = Program {
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
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init should succeed");
        let mut events: std::vec::Vec<TraceEvent> = std::vec::Vec::new();
        let mut logs: std::vec::Vec<LogEvent> = std::vec::Vec::new();

        rt.tick_with_trace_and_logs(&mut io, |e| events.push(e), |l| logs.push(l))
            .expect("tick should process both tasks");

        assert_eq!(
            events,
            std::vec![
                TraceEvent {
                    tick: Tick(0),
                    task: 0,
                    from: StepId(0),
                    to: StepId(1),
                    reason: TransitionReason::Action,
                },
                TraceEvent {
                    tick: Tick(0),
                    task: 1,
                    from: StepId(0),
                    to: StepId(1),
                    reason: TransitionReason::Action,
                },
            ]
        );
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].task, 0);
        assert_eq!(logs[0].message_id, 10);
        assert_eq!(logs[1].task, 1);
        assert_eq!(logs[1].message_id, 20);
    }

    #[test]
    fn per_task_transition_budget_allows_two_active_tasks_to_chain_under_cap() {
        let task0_steps = build_goto_chain_steps(40);
        let task1_steps = build_goto_chain_steps(40);
        let tasks = Box::leak(
            vec![
                Task {
                    name: "task0",
                    steps: task0_steps,
                    entry: StepId(0),
                },
                Task {
                    name: "task1",
                    steps: task1_steps,
                    entry: StepId(0),
                },
            ]
            .into_boxed_slice(),
        );
        let program = Program {
            tasks,
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
        };

        let mut io = MemIo::new();
        let mut runtime = Runtime::new(&program).expect("runtime init should succeed");
        runtime
            .tick(&mut io)
            .expect("per-task budget should allow both tasks under cap");

        assert_eq!(
            runtime.task_context(0).expect("task0 context").current_step,
            StepId(39)
        );
        assert_eq!(
            runtime.task_context(1).expect("task1 context").current_step,
            StepId(39)
        );
        assert_eq!(io.tick(), Tick(1));
    }

    #[test]
    fn per_task_transition_budget_error_reports_context_for_multi_task_runtime() {
        let task0_steps = leak_steps(vec![Step {
            name: "loop",
            instr: Instr::Goto { target: StepId(0) },
        }]);
        let task1_steps = leak_steps(vec![Step {
            name: "halt",
            instr: Instr::Halt,
        }]);
        let tasks = Box::leak(
            vec![
                Task {
                    name: "task0",
                    steps: task0_steps,
                    entry: StepId(0),
                },
                Task {
                    name: "task1",
                    steps: task1_steps,
                    entry: StepId(0),
                },
            ]
            .into_boxed_slice(),
        );
        let program = Program {
            tasks,
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
        };

        let mut io = MemIo::new();
        let mut runtime = Runtime::new(&program).expect("runtime init should succeed");
        let error = runtime
            .tick(&mut io)
            .expect_err("infinite same-tick chain should hit per-task budget");
        assert_eq!(
            error,
            RuntimeError::TooManyTransitionsInOneTick {
                task: 0,
                attempted: MAX_TRANSITIONS_PER_TASK_PER_TICK + 1,
                per_task_cap: MAX_TRANSITIONS_PER_TASK_PER_TICK,
                active_tasks: 2,
            }
        );
    }

    #[test]
    fn step_completion_rules_cover_immediate_delay_wait_and_pending_paths() {
        static TASK0_STEPS: [Step<'static>; 2] = [
            Step {
                name: "immediate_set",
                instr: Instr::Action {
                    actions: &[Action::SetDigital {
                        id: DigitalOutputId(0),
                        value: true,
                    }],
                    next: StepId(1),
                },
            },
            Step {
                name: "halt",
                instr: Instr::Halt,
            },
        ];
        static TASK1_STEPS: [Step<'static>; 2] = [
            Step {
                name: "delay_two_ticks",
                instr: Instr::Delay {
                    ticks: 2,
                    next: StepId(1),
                },
            },
            Step {
                name: "halt",
                instr: Instr::Halt,
            },
        ];
        static TASK2_STEPS: [Step<'static>; 2] = [
            Step {
                name: "wait_di0_true",
                instr: Instr::WaitDigital {
                    id: DigitalInputId(0),
                    equals: true,
                    next: StepId(1),
                    timeout: None,
                },
            },
            Step {
                name: "halt",
                instr: Instr::Halt,
            },
        ];
        static TASKS: [Task<'static>; 3] = [
            Task {
                name: "immediate_task",
                steps: &TASK0_STEPS,
                entry: StepId(0),
            },
            Task {
                name: "delay_task",
                steps: &TASK1_STEPS,
                entry: StepId(0),
            },
            Task {
                name: "wait_task",
                steps: &TASK2_STEPS,
                entry: StepId(0),
            },
        ];
        static PROGRAM: Program<'static> = Program {
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
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init should succeed");
        rt.tick(&mut io)
            .expect("tick should evaluate immediate/delay/wait paths");

        let immediate = rt.task_context(0).expect("immediate task");
        assert_eq!(immediate.current_step, StepId(1));
        assert_eq!(immediate.wait_state, TaskWaitState::Ready);
        assert!(
            io.do_[0],
            "immediate action should commit output before completion"
        );

        let delay = rt.task_context(1).expect("delay task");
        assert_eq!(delay.current_step, StepId(0));
        assert_eq!(delay.wait_state, TaskWaitState::Delay);

        let wait = rt.task_context(2).expect("wait task");
        assert_eq!(wait.current_step, StepId(0));
        assert_eq!(wait.wait_state, TaskWaitState::WaitCondition);

        assert_eq!(
            Runtime::action_completion_decision(StepId(9), ActionCompletionState::Pending),
            StepCompletionDecision::StayOnStep
        );
        assert_eq!(
            Runtime::action_completion_decision(StepId(9), ActionCompletionState::Completed),
            StepCompletionDecision::ContinueWith {
                target: StepId(9),
                reason: TransitionReason::Action,
            }
        );
    }

    #[test]
    fn delay_boundary_and_goto_chain_happen_on_expected_tick() {
        static STEPS: [Step<'static>; 3] = [
            Step {
                name: "delay2",
                instr: Instr::Delay {
                    ticks: 2,
                    next: StepId(1),
                },
            },
            Step {
                name: "goto2",
                instr: Instr::Goto { target: StepId(2) },
            },
            Step {
                name: "halt",
                instr: Instr::Halt,
            },
        ];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static PROGRAM: Program<'static> = Program {
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
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).unwrap();

        let mut events: std::vec::Vec<TraceEvent> = std::vec::Vec::new();
        rt.tick_with_trace(&mut io, |e| events.push(e)).unwrap(); // tick 0
        rt.tick_with_trace(&mut io, |e| events.push(e)).unwrap(); // tick 1
        rt.tick_with_trace(&mut io, |e| events.push(e)).unwrap(); // tick 2 (delay completes + goto)

        assert_eq!(
            events,
            std::vec![
                TraceEvent {
                    tick: Tick(2),
                    task: 0,
                    from: StepId(0),
                    to: StepId(1),
                    reason: TransitionReason::DelayElapsed,
                },
                TraceEvent {
                    tick: Tick(2),
                    task: 0,
                    from: StepId(1),
                    to: StepId(2),
                    reason: TransitionReason::Goto,
                },
            ]
        );
    }

    #[test]
    fn wait_timeout_fires_when_elapsed_reaches_after_ticks() {
        static STEPS: [Step<'static>; 3] = [
            Step {
                name: "wait_di0_true_tmo2",
                instr: Instr::WaitDigital {
                    id: DigitalInputId(0),
                    equals: true,
                    next: StepId(2),
                    timeout: Some(Timeout {
                        after_ticks: 2,
                        target: StepId(1),
                    }),
                },
            },
            Step {
                name: "timed_out",
                instr: Instr::Halt,
            },
            Step {
                name: "ok",
                instr: Instr::Halt,
            },
        ];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static PROGRAM: Program<'static> = Program {
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
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).unwrap();

        let mut events: std::vec::Vec<TraceEvent> = std::vec::Vec::new();
        rt.tick_with_trace(&mut io, |e| events.push(e)).unwrap(); // tick 0
        rt.tick_with_trace(&mut io, |e| events.push(e)).unwrap(); // tick 1
        rt.tick_with_trace(&mut io, |e| events.push(e)).unwrap(); // tick 2 -> timeout

        assert_eq!(
            events,
            std::vec![TraceEvent {
                tick: Tick(2),
                task: 0,
                from: StepId(0),
                to: StepId(1),
                reason: TransitionReason::Timeout,
            }]
        );
    }

    #[test]
    fn timeout_zero_is_immediate_on_entry_tick() {
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "wait_di0_true_tmo0",
                instr: Instr::WaitDigital {
                    id: DigitalInputId(0),
                    equals: true,
                    next: StepId(1),
                    timeout: Some(Timeout {
                        after_ticks: 0,
                        target: StepId(1),
                    }),
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
        static PROGRAM: Program<'static> = Program {
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
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).unwrap();

        let mut events: std::vec::Vec<TraceEvent> = std::vec::Vec::new();
        rt.tick_with_trace(&mut io, |e| events.push(e)).unwrap(); // tick 0 -> immediate timeout

        assert_eq!(
            events,
            std::vec![TraceEvent {
                tick: Tick(0),
                task: 0,
                from: StepId(0),
                to: StepId(1),
                reason: TransitionReason::Timeout,
            }]
        );
    }

