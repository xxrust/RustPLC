    #[test]
    fn axis_move_requires_handler_when_using_plain_tick() {
        static ACTIONS: [Action; 1] = [Action::AxisMove {
            command: AxisMotionCommand {
                target: "axis_x",
                port: "self",
                kind: AxisMoveKind::Relative,
                value: 10.0,
                speed: 2.0,
                acceleration: 2.0,
                deceleration: 2.0,
                require_homed: false,
                semantic_tag: None,
                timeout: None,
                fault_routing: None,
            },
        }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "axis_run",
                instr: Instr::Action {
                    actions: &ACTIONS,
                    next: StepId(1),
                },
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
        let err = rt
            .tick(&mut io)
            .expect_err("missing axis handler should fail");
        assert_eq!(
            err,
            RuntimeError::AxisMotionRequiresHandler { target: "axis_x" }
        );
    }

    #[test]
    fn axis_move_handler_done_transitions_successfully() {
        static ACTIONS: [Action; 1] = [Action::AxisMove {
            command: AxisMotionCommand {
                target: "axis_x",
                port: "self",
                kind: AxisMoveKind::Absolute,
                value: 120.0,
                speed: 5.0,
                acceleration: 5.0,
                deceleration: 5.0,
                require_homed: false,
                semantic_tag: None,
                timeout: None,
                fault_routing: None,
            },
        }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "axis_run",
                instr: Instr::Action {
                    actions: &ACTIONS,
                    next: StepId(1),
                },
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
        rt.tick_with_axis(&mut io, |command| {
            assert_eq!(command.target, "axis_x");
            assert_eq!(command.kind, AxisMoveKind::Absolute);
            AxisMotionResult::Done
        })
        .expect("axis handler done should continue execution");
        assert_eq!(rt.location().step, StepId(1));
        assert_eq!(io.tick(), Tick(1));
    }

    #[test]
    fn axis_move_pending_blocks_and_polls_without_replaying_prior_actions() {
        static ACTIONS: [Action; 2] = [
            Action::Log {
                message_id: 41,
                message: "axis dispatch",
            },
            Action::AxisMove {
                command: AxisMotionCommand {
                    target: "axis_x",
                    port: "self",
                    kind: AxisMoveKind::Relative,
                    value: 10.0,
                    speed: 2.0,
                    acceleration: 2.0,
                    deceleration: 2.0,
                    require_homed: false,
                    semantic_tag: None,
                    timeout: None,
                    fault_routing: None,
                },
            },
        ];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "axis_run",
                instr: Instr::Action {
                    actions: &ACTIONS,
                    next: StepId(1),
                },
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
        let mut motion_calls = 0usize;
        let mut logs = std::vec::Vec::new();

        rt.tick_with_axis_and_logs(
            &mut io,
            |event| logs.push(event),
            |_| {
                motion_calls += 1;
                AxisMotionResult::Pending
            },
        )
        .expect("pending axis should keep step active");

        assert_eq!(motion_calls, 1);
        assert_eq!(rt.location().step, StepId(0));
        assert_eq!(
            rt.task_context(0)
                .expect("task context")
                .pending_action_state,
            TaskPendingActionState::AxisMotion {
                target: "axis_x",
                action_index: 1,
                semantic_tag: None,
            }
        );
        assert_eq!(
            logs.len(),
            1,
            "dispatch log should fire only once on first entry"
        );

        rt.tick_with_axis_and_logs(
            &mut io,
            |event| logs.push(event),
            |_| {
                motion_calls += 1;
                AxisMotionResult::Done
            },
        )
        .expect("done on polling tick should complete step");

        assert_eq!(motion_calls, 2);
        assert_eq!(rt.location().step, StepId(1));
        assert_eq!(
            rt.task_context(0)
                .expect("task context")
                .pending_action_state,
            TaskPendingActionState::Idle
        );
        assert_eq!(
            logs.len(),
            1,
            "pending polling tick must not replay pre-axis actions"
        );
    }

    #[test]
    fn axis_move_pending_then_fault_clears_pending_state_and_surfaces_error() {
        static ACTIONS: [Action; 1] = [Action::AxisMove {
            command: AxisMotionCommand {
                target: "axis_x",
                port: "self",
                kind: AxisMoveKind::Relative,
                value: 10.0,
                speed: 2.0,
                acceleration: 2.0,
                deceleration: 2.0,
                require_homed: false,
                semantic_tag: None,
                timeout: None,
                fault_routing: None,
            },
        }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "axis_run",
                instr: Instr::Action {
                    actions: &ACTIONS,
                    next: StepId(1),
                },
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

        rt.tick_with_axis(&mut io, |_| AxisMotionResult::Pending)
            .expect("first tick should start pending axis move");
        assert_eq!(rt.location().step, StepId(0));
        assert_eq!(
            rt.task_context(0)
                .expect("task context")
                .pending_action_state,
            TaskPendingActionState::AxisMotion {
                target: "axis_x",
                action_index: 0,
                semantic_tag: None,
            }
        );

        let err = rt
            .tick_with_axis(&mut io, |_| AxisMotionResult::motion_fault(77))
            .expect_err("polling tick fault should be surfaced");
        assert_eq!(
            err,
            RuntimeError::AxisFault {
                target: "axis_x",
                fault: AxisFault::motion(77),
            }
        );
        assert_eq!(
            rt.task_context(0)
                .expect("task context")
                .pending_action_state,
            TaskPendingActionState::Idle
        );
        assert_eq!(
            rt.location().step,
            StepId(0),
            "faulted pending action should not advance success path"
        );
    }

    #[test]
    fn axis_move_absolute_requires_homing_predicate() {
        static ACTIONS: [Action; 1] = [Action::AxisMove {
            command: AxisMotionCommand {
                target: "axis_x",
                port: "self",
                kind: AxisMoveKind::Absolute,
                value: 120.0,
                speed: 5.0,
                acceleration: 5.0,
                deceleration: 5.0,
                require_homed: true,
                semantic_tag: None,
                timeout: None,
                fault_routing: None,
            },
        }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "axis_run",
                instr: Instr::Action {
                    actions: &ACTIONS,
                    next: StepId(1),
                },
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
        let mut invoked = false;
        let err = rt
            .tick_with_axis(&mut io, |_| {
                invoked = true;
                AxisMotionResult::Done
            })
            .expect_err("absolute move should fail while the axis is not homed");
        assert_eq!(err, RuntimeError::AxisNotHomed { target: "axis_x" });
        assert!(
            !invoked,
            "runtime homing guard should short-circuit handler"
        );
    }

    #[test]
    fn axis_move_relative_sets_homing_predicate_for_absolute() {
        static ACTIONS_REL: [Action; 1] = [Action::AxisMove {
            command: AxisMotionCommand {
                target: "axis_x",
                port: "self",
                kind: AxisMoveKind::Relative,
                value: 5.0,
                speed: 1.0,
                acceleration: 1.0,
                deceleration: 1.0,
                require_homed: false,
                semantic_tag: None,
                timeout: None,
                fault_routing: None,
            },
        }];
        static ACTIONS_ABS: [Action; 1] = [Action::AxisMove {
            command: AxisMotionCommand {
                target: "axis_x",
                port: "self",
                kind: AxisMoveKind::Absolute,
                value: 120.0,
                speed: 5.0,
                acceleration: 5.0,
                deceleration: 5.0,
                require_homed: true,
                semantic_tag: None,
                timeout: None,
                fault_routing: None,
            },
        }];
        static STEPS: [Step<'static>; 3] = [
            Step {
                name: "home",
                instr: Instr::Action {
                    actions: &ACTIONS_REL,
                    next: StepId(1),
                },
            },
            Step {
                name: "move_abs",
                instr: Instr::Action {
                    actions: &ACTIONS_ABS,
                    next: StepId(2),
                },
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
        rt.tick_with_axis(&mut io, |_| AxisMotionResult::Done)
            .expect("relative motion should mark the axis as homed");
        rt.tick_with_axis(&mut io, |_| AxisMotionResult::Done)
            .expect("absolute motion should run after homing");
        assert_eq!(rt.location().step, StepId(2));
    }

    #[test]
    fn axis_move_fault_invalidates_homing_predicate() {
        static ACTIONS_REL: [Action; 1] = [Action::AxisMove {
            command: AxisMotionCommand {
                target: "axis_x",
                port: "self",
                kind: AxisMoveKind::Relative,
                value: 5.0,
                speed: 1.0,
                acceleration: 1.0,
                deceleration: 1.0,
                require_homed: false,
                semantic_tag: None,
                timeout: None,
                fault_routing: None,
            },
        }];
        static ACTIONS_ABS: [Action; 1] = [Action::AxisMove {
            command: AxisMotionCommand {
                target: "axis_x",
                port: "self",
                kind: AxisMoveKind::Absolute,
                value: 120.0,
                speed: 5.0,
                acceleration: 5.0,
                deceleration: 5.0,
                require_homed: true,
                semantic_tag: None,
                timeout: None,
                fault_routing: None,
            },
        }];
        static STEPS: [Step<'static>; 3] = [
            Step {
                name: "home",
                instr: Instr::Action {
                    actions: &ACTIONS_REL,
                    next: StepId(1),
                },
            },
            Step {
                name: "move_abs",
                instr: Instr::Action {
                    actions: &ACTIONS_ABS,
                    next: StepId(2),
                },
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
        let mut call_count = 0;
        let err = rt
            .tick_with_axis(&mut io, |command| {
                call_count += 1;
                if command.kind == AxisMoveKind::Relative {
                    AxisMotionResult::Done
                } else {
                    AxisMotionResult::motion_fault(77)
                }
            })
            .expect_err("fault should stop absolute move and clear homing");
        assert_eq!(
            call_count, 2,
            "single tick should execute relative then absolute"
        );
        assert_eq!(
            err,
            RuntimeError::AxisFault {
                target: "axis_x",
                fault: AxisFault::motion(77),
            }
        );

        let mut invoked = false;
        let err = rt
            .tick_with_axis(&mut io, |_| {
                invoked = true;
                AxisMotionResult::Done
            })
            .expect_err("after fault absolute move should be rejected until re-homed");
        assert_eq!(err, RuntimeError::AxisNotHomed { target: "axis_x" });
        assert!(!invoked, "homing guard should trigger before the handler");
    }

    #[test]
    fn axis_move_handler_reject_returns_classified_error() {
        static ACTIONS: [Action; 1] = [Action::AxisMove {
            command: AxisMotionCommand {
                target: "axis_x",
                port: "self",
                kind: AxisMoveKind::Relative,
                value: 5.0,
                speed: 1.0,
                acceleration: 1.0,
                deceleration: 1.0,
                require_homed: false,
                semantic_tag: None,
                timeout: None,
                fault_routing: None,
            },
        }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "axis_run",
                instr: Instr::Action {
                    actions: &ACTIONS,
                    next: StepId(1),
                },
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
        let err = rt
            .tick_with_axis(&mut io, |_| AxisMotionResult::reject(11))
            .expect_err("reject fault should be classified");
        assert_eq!(
            err,
            RuntimeError::AxisFault {
                target: "axis_x",
                fault: AxisFault::reject(11),
            }
        );
    }

    #[test]
    fn axis_move_handler_motion_fault_returns_classified_error() {
        static ACTIONS: [Action; 1] = [Action::AxisMove {
            command: AxisMotionCommand {
                target: "axis_x",
                port: "self",
                kind: AxisMoveKind::Relative,
                value: 5.0,
                speed: 1.0,
                acceleration: 1.0,
                deceleration: 1.0,
                require_homed: false,
                semantic_tag: None,
                timeout: None,
                fault_routing: None,
            },
        }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "axis_run",
                instr: Instr::Action {
                    actions: &ACTIONS,
                    next: StepId(1),
                },
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
        let err = rt
            .tick_with_axis(&mut io, |_| AxisMotionResult::motion_fault(21))
            .expect_err("motion fault should be classified");
        assert_eq!(
            err,
            RuntimeError::AxisFault {
                target: "axis_x",
                fault: AxisFault::motion(21),
            }
        );
    }

    #[test]
    fn axis_move_handler_safety_fault_returns_classified_error() {
        static ACTIONS: [Action; 1] = [Action::AxisMove {
            command: AxisMotionCommand {
                target: "axis_x",
                port: "self",
                kind: AxisMoveKind::Relative,
                value: 5.0,
                speed: 1.0,
                acceleration: 1.0,
                deceleration: 1.0,
                require_homed: false,
                semantic_tag: None,
                timeout: None,
                fault_routing: None,
            },
        }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "axis_run",
                instr: Instr::Action {
                    actions: &ACTIONS,
                    next: StepId(1),
                },
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
        let err = rt
            .tick_with_axis(&mut io, |_| AxisMotionResult::safety_fault(31))
            .expect_err("safety fault should be classified");
        assert_eq!(
            err,
            RuntimeError::AxisFault {
                target: "axis_x",
                fault: AxisFault::safety(31),
            }
        );
    }

    #[test]
    fn axis_fault_policy_applies_mode_specific_stop_transitions() {
        static ACTIONS: [Action; 1] = [Action::AxisMove {
            command: AxisMotionCommand {
                target: "axis_x",
                port: "self",
                kind: AxisMoveKind::Relative,
                value: 5.0,
                speed: 1.0,
                acceleration: 1.0,
                deceleration: 1.0,
                require_homed: false,
                semantic_tag: None,
                timeout: None,
                fault_routing: None,
            },
        }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "axis_run",
                instr: Instr::Action {
                    actions: &ACTIONS,
                    next: StepId(1),
                },
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

        let cases = [
            (
                AxisFaultSeverity::Recoverable,
                AxisStopMode::Controlled,
                AxisMotionResult::reject(101),
            ),
            (
                AxisFaultSeverity::NonRecoverable,
                AxisStopMode::Quick,
                AxisMotionResult::motion_fault(102),
            ),
            (
                AxisFaultSeverity::Safety,
                AxisStopMode::Immediate,
                AxisMotionResult::safety_fault(103),
            ),
        ];

        for (severity, stop_mode, axis_result) in cases {
            let policies = [AxisFaultPolicy {
                axis: "axis_x",
                severity,
                stop_mode,
                auto_reset_policy: AxisAutoResetPolicy::Never,
                manual_ack_required: true,
                propagation_scope: AxisFaultPropagationScope::SelfOnly,
                propagation_targets: &["axis_x"],
            }];
            let program = Program {
                tasks: &TASKS,
                pid_loops: &[],
                var_init: &[],
                cam_configs: &[],
                cam_tables: &[],
                axis_fault_policies: &policies,
                semantic_resources: &[],
                resource_claims: &[],
                workpiece_types: &[],
                workpiece_sites: &[],
                workpiece_holders: &[],
            };

            let expected_fault = match axis_result {
                AxisMotionResult::Fault(fault) => fault,
                AxisMotionResult::Pending => panic!("test case must carry fault result"),
                AxisMotionResult::Done => panic!("test case must carry fault result"),
            };

            let mut io = MemIo::new();
            let mut rt = Runtime::new(&program).expect("runtime init");
            assert_eq!(rt.axis_stop_state(), AxisStopState::Running);

            let mut logs = std::vec::Vec::new();
            let err = rt
                .tick_with_axis_and_logs(&mut io, |event| logs.push(event), |_| axis_result)
                .expect_err("fault result should be surfaced");

            assert_eq!(
                err,
                RuntimeError::AxisFault {
                    target: "axis_x",
                    fault: expected_fault,
                }
            );
            assert_eq!(rt.axis_stop_state(), AxisStopState::Stopped);
            assert_eq!(logs.len(), 3);
            assert_eq!(logs[0].message, AXIS_FAULT_POLICY_LOG_MESSAGE);
            assert_eq!(
                logs[0].message_id,
                axis_fault_policy_log_message_id(
                    severity,
                    stop_mode,
                    AxisAutoResetPolicy::Never,
                    true,
                    expected_fault.kind,
                )
            );
            assert_eq!(logs[1].message, AXIS_STOP_TRANSITION_ENTER_LOG_MESSAGE);
            assert_eq!(
                logs[1].message_id,
                axis_stop_transition_log_message_id(stop_mode, AxisStopTransitionPhase::Enter)
            );
            assert_eq!(logs[2].message, AXIS_STOP_TRANSITION_COMPLETED_LOG_MESSAGE);
            assert_eq!(
                logs[2].message_id,
                axis_stop_transition_log_message_id(stop_mode, AxisStopTransitionPhase::Completed)
            );
        }
    }

    #[test]
    fn axis_fault_routing_resolves_vendor_match_and_primary_bucket_fallback() {
        static REJECT_ROUTES: [AxisFaultRouteRule; 1] = [AxisFaultRouteRule {
            kind: Some(AxisFaultRouteKind::Vendor),
            code: Some(1201),
            target: StepId(11),
        }];
        static MOTION_ROUTES: [AxisFaultRouteRule; 2] = [
            AxisFaultRouteRule {
                kind: Some(AxisFaultRouteKind::Vendor),
                code: None,
                target: StepId(21),
            },
            AxisFaultRouteRule {
                kind: Some(AxisFaultRouteKind::Vendor),
                code: Some(2202),
                target: StepId(22),
            },
        ];
        static SAFETY_ROUTES: [AxisFaultRouteRule; 0] = [];

        let routing = AxisFaultRouting {
            on_reject: StepId(1),
            on_motion_fault: StepId(2),
            on_safety_fault: StepId(3),
            on_reject_routes: &REJECT_ROUTES,
            on_motion_fault_routes: &MOTION_ROUTES,
            on_safety_fault_routes: &SAFETY_ROUTES,
        };

        assert_eq!(routing.resolve_target(AxisFault::reject(99)), StepId(1));
        assert_eq!(routing.resolve_target(AxisFault::motion(77)), StepId(2));
        assert_eq!(routing.resolve_target(AxisFault::safety(88)), StepId(3));
        assert_eq!(
            routing.resolve_target(AxisFault::new(
                AxisFaultKind::Vendor {
                    category: AxisFaultCategory::Recoverable,
                    vendor_code: 1201,
                },
                1201,
            )),
            StepId(11)
        );
        assert_eq!(
            routing.resolve_target(AxisFault::new(
                AxisFaultKind::Vendor {
                    category: AxisFaultCategory::NonRecoverable,
                    vendor_code: 2202,
                },
                2202,
            )),
            StepId(21),
            "first matching route should win inside the same fault bucket"
        );
    }

    #[test]
    fn axis_fault_policy_propagates_targets_within_same_tick() {
        static ACTIONS: [Action; 1] = [Action::AxisMove {
            command: AxisMotionCommand {
                target: "axis_x",
                port: "self",
                kind: AxisMoveKind::Relative,
                value: 5.0,
                speed: 1.0,
                acceleration: 1.0,
                deceleration: 1.0,
                require_homed: false,
                semantic_tag: None,
                timeout: None,
                fault_routing: None,
            },
        }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "axis_run",
                instr: Instr::Action {
                    actions: &ACTIONS,
                    next: StepId(1),
                },
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

        let policies = [AxisFaultPolicy {
            axis: "axis_x",
            severity: AxisFaultSeverity::Safety,
            stop_mode: AxisStopMode::Immediate,
            auto_reset_policy: AxisAutoResetPolicy::Never,
            manual_ack_required: true,
            propagation_scope: AxisFaultPropagationScope::Followers,
            propagation_targets: &["axis_x", "axis_y"],
        }];
        let program = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &policies,
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &[],
            workpiece_sites: &[],
            workpiece_holders: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&program).expect("runtime init");
        let mut on_event = |_| {};
        let mut on_log = |_| {};
        let mut on_extern_call = |_function: &'static str,
                                  _args: &[f32],
                                  _results: &mut [f32]|
         -> Result<usize, ()> { Err(()) };
        let mut map_extern_error_code = |_function: &'static str, _error: &()| 0.0;
        let mut on_axis_motion =
            |_command: AxisMotionCommand| Ok(AxisMotionResult::safety_fault(55));
        let mut applied_targets = std::vec::Vec::new();

        let err = rt.tick_with_trace_and_logs_impl(
            &mut io,
            &mut on_event,
            &mut on_log,
            &mut on_extern_call,
            None,
            &mut map_extern_error_code,
            &mut on_axis_motion,
            &mut |command: AxisMotionCommand, _fault: AxisFault| {
                applied_targets.push(command.target)
            },
        );

        assert!(matches!(
            err,
            Err(RuntimeTickError::Core(RuntimeError::AxisFault {
                target: "axis_x",
                fault,
            })) if fault == AxisFault::safety(55)
        ));
        assert_eq!(applied_targets, vec!["axis_x", "axis_y"]);
    }

    #[test]
    fn axis_fault_vendor_slot_preserves_category_and_vendor_code() {
        let fault = AxisFault::new(
            AxisFaultKind::Vendor {
                category: AxisFaultCategory::NonRecoverable,
                vendor_code: 9001,
            },
            77,
        );

        assert_eq!(fault.category, AxisFaultCategory::NonRecoverable);
        assert_eq!(fault.vendor_code, Some(9001));
        assert_eq!(fault.error_code, 77);
    }

