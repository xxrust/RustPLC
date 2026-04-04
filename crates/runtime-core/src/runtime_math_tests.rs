    #[test]
    fn analog_wait_satisfies_when_value_enters_selected_region() {
        static RANGES: [AnalogRange; 1] = [AnalogRange {
            min: 80.0,
            max: 100.0,
        }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "wait_ai0_region",
                instr: Instr::WaitAnalog {
                    id: AnalogInputId(0),
                    ranges: &RANGES,
                    next: StepId(1),
                    timeout: None,
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
        io.ai[0] = 90.0;
        let mut rt = Runtime::new(&PROGRAM).unwrap();
        let mut events: std::vec::Vec<TraceEvent> = std::vec::Vec::new();

        rt.tick_with_trace(&mut io, |e| events.push(e)).unwrap();
        assert_eq!(
            events,
            std::vec![TraceEvent {
                tick: Tick(0),
                task: 0,
                from: StepId(0),
                to: StepId(1),
                reason: TransitionReason::WaitSatisfied,
            }]
        );
    }

    #[test]
    fn linear_interpolate_handles_periodic_wrap_and_oneshot_clamp() {
        let periodic = build_cam_table(true, &[(0.0, 0.0), (100.0, 100.0), (200.0, 0.0)]);
        let wrapped_neg = linear_interpolate(&periodic, -50.0);
        let wrapped_over = linear_interpolate(&periodic, 250.0);
        assert!(
            (wrapped_neg - 50.0).abs() < 1e-5,
            "periodic wrap(-50) should resolve to 50, got {wrapped_neg}"
        );
        assert!(
            (wrapped_over - 50.0).abs() < 1e-5,
            "periodic wrap(250) should resolve to 50, got {wrapped_over}"
        );

        let oneshot = build_cam_table(false, &[(0.0, 0.0), (100.0, 100.0)]);
        assert_eq!(
            linear_interpolate(&oneshot, -10.0),
            0.0,
            "oneshot should clamp on the left edge"
        );
        assert_eq!(
            linear_interpolate(&oneshot, 150.0),
            100.0,
            "oneshot should clamp on the right edge"
        );
    }

    #[test]
    fn binary_search_interval_covers_boundaries_exact_hits_and_inner_points() {
        let table = build_cam_table(false, &[(0.0, 0.0), (100.0, 40.0), (200.0, 100.0)]);
        assert_eq!(
            binary_search_interval(&table, 0.0),
            0,
            "lower boundary should map to the first segment"
        );
        assert_eq!(
            binary_search_interval(&table, 40.0),
            0,
            "midpoint should map to the matching segment"
        );
        assert_eq!(
            binary_search_interval(&table, 100.0),
            1,
            "exact midpoint hit should advance to the right segment"
        );
        assert_eq!(
            binary_search_interval(&table, 200.0),
            1,
            "upper boundary should clamp to the last segment"
        );
    }

    #[test]
    fn linear_interpolate_matches_known_midpoint_precision() {
        let table = build_cam_table(false, &[(0.0, 0.0), (10.0, 20.0)]);
        let y = linear_interpolate(&table, 5.0);
        assert!(
            (y - 10.0).abs() < 1e-6,
            "linear interpolation midpoint error should stay below 1e-6, got {y}"
        );
    }

    #[test]
    fn cubic_interpolate_evaluates_horner_polynomial() {
        let mut table = build_cam_table(false, &[(0.0, 0.0), (10.0, 10.0)]);
        table.coeffs[0] = SplineCoeff {
            a: 1.0,
            b: 2.0,
            c: 3.0,
            d: 4.0,
        };
        let out = cubic_interpolate(&table, 2.0);
        assert!(
            (out - 49.0).abs() < 1e-6,
            "Horner polynomial should evaluate to 49, got {out}"
        );
    }

    #[test]
    fn cubic_derivative_matches_central_difference() {
        let mut table = build_cam_table(false, &[(0.0, 0.0), (10.0, 0.0)]);
        table.coeffs[0] = SplineCoeff {
            a: 0.5,
            b: 1.2,
            c: -0.3,
            d: 0.08,
        };

        let x = 3.0f32;
        let h = 1e-3f32;
        let analytical = cubic_derivative(&table, x);
        let finite_diff =
            (cubic_interpolate(&table, x + h) - cubic_interpolate(&table, x - h)) / (2.0 * h);

        assert!(
            (analytical - finite_diff).abs() < 1e-3,
            "cubic_derivative should match the finite difference estimate, analytical={analytical}, finite_diff={finite_diff}"
        );
    }

    #[test]
    fn wait_expr_satisfies_and_supports_timeout() {
        const fn lit_expr(value: f32) -> ExprProgram {
            let mut ops = [ExprOp::PushLiteral(0.0); MAX_EXPR_OPS];
            ops[0] = ExprOp::PushLiteral(value);
            ExprProgram { ops, len: 1 }
        }
        const fn add_var_and_lit(var_idx: u16, value: f32) -> ExprProgram {
            let mut ops = [ExprOp::PushLiteral(0.0); MAX_EXPR_OPS];
            ops[0] = ExprOp::PushVariable(var_idx);
            ops[1] = ExprOp::PushLiteral(value);
            ops[2] = ExprOp::Add;
            ExprProgram { ops, len: 3 }
        }

        static STEPS: [Step<'static>; 4] = [
            Step {
                name: "wait_expr_ok",
                instr: Instr::WaitExpr {
                    left: add_var_and_lit(0, 1.0),
                    op: CompareOp::Gt,
                    right: lit_expr(1.5),
                    next: StepId(1),
                    timeout: None,
                },
            },
            Step {
                name: "wait_expr_timeout",
                instr: Instr::WaitExpr {
                    left: lit_expr(0.0),
                    op: CompareOp::Eq,
                    right: lit_expr(1.0),
                    next: StepId(3),
                    timeout: Some(Timeout {
                        after_ticks: 1,
                        target: StepId(2),
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
        static VARS: [f32; 1] = [1.0];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &VARS,
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

        rt.tick_with_trace(&mut io, |e| events.push(e)).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].to, StepId(1));
        assert_eq!(events[0].reason, TransitionReason::WaitSatisfied);

        rt.tick_with_trace(&mut io, |e| events.push(e)).unwrap();
        rt.tick_with_trace(&mut io, |e| events.push(e)).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].to, StepId(2));
        assert_eq!(events[1].reason, TransitionReason::Timeout);
    }

    #[test]
    fn log_action_emits_log_event_without_touching_io() {
        static ACTIONS: [Action; 1] = [Action::Log {
            message_id: 7,
            message: "fault timeout",
        }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "log_once",
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

        let mut logs = std::vec::Vec::new();
        let mut traces = std::vec::Vec::new();
        rt.tick_with_trace_and_logs(&mut io, |e| traces.push(e), |l| logs.push(l))
            .unwrap();

        assert_eq!(io.do_[0], false, "log action should not modify outputs");
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].tick, Tick(0));
        assert_eq!(logs[0].step, StepId(0));
        assert_eq!(logs[0].message_id, 7);
        assert_eq!(logs[0].message, "fault timeout");
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].reason, TransitionReason::Action);
    }

