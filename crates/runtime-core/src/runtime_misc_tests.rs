    #[test]
    fn pid_output_is_bounded_and_first_order_step_response_converges() {
        static STEPS: [Step<'static>; 1] = [Step {
            name: "halt",
            instr: Instr::Halt,
        }];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static PID: [PidConfig; 1] = [PidConfig {
            pv: AnalogInputId(0),
            out: AnalogOutputId(0),
            sp: 1.0,
            kp: 2.0,
            ki: 0.8,
            kd: 0.0,
            dt_s: 0.1,
            period_ticks: 1,
            limit_min: 0.0,
            limit_max: 1.0,
            anti_windup: AntiWindup::ConditionalIntegration,
        }];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &PID,
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

        // Simple first-order plant model: y[k+1] = y[k] + alpha*(u[k]-y[k]).
        let alpha = 0.2_f32;
        let mut pv_hist = std::vec::Vec::new();
        let mut u_hist = std::vec::Vec::new();

        for _ in 0..80 {
            rt.tick(&mut io).unwrap();
            let u = io.ao[0];
            io.ai[0] = io.ai[0] + alpha * (u - io.ai[0]);
            pv_hist.push(io.ai[0]);
            u_hist.push(u);
        }

        assert!(
            u_hist.iter().all(|u| *u >= 0.0 && *u <= 1.0),
            "PID output must stay in configured clamp range"
        );
        let initial_err = (1.0 - pv_hist[0]).abs();
        let final_err = (1.0 - pv_hist[pv_hist.len() - 1]).abs();
        assert!(
            final_err < initial_err,
            "step response should move toward setpoint (initial_err={initial_err}, final_err={final_err})"
        );
        assert!(
            pv_hist[pv_hist.len() - 1] > 0.8,
            "first-order response should converge near setpoint under this tuning"
        );
    }

    #[test]
    fn eval_expr_supports_builtin_math_functions() {
        let mut vars = [0.0f32; MAX_VARIABLES];
        vars[0] = -4.0;

        let mut ops = [ExprOp::PushLiteral(0.0); MAX_EXPR_OPS];
        ops[0] = ExprOp::PushVariable(0);
        ops[1] = ExprOp::CallAbs;
        ops[2] = ExprOp::PushLiteral(2.0);
        ops[3] = ExprOp::CallPow;
        ops[4] = ExprOp::PushLiteral(0.0);
        ops[5] = ExprOp::PushLiteral(9.0);
        ops[6] = ExprOp::CallClamp;
        let expr = ExprProgram { ops, len: 7 };
        let out = eval_expr(&expr, &vars);
        assert!(
            (out - 9.0).abs() < 1e-6,
            "clamp(pow(abs(x),2),0,9) should evaluate to 9"
        );

        let mut ops2 = [ExprOp::PushLiteral(0.0); MAX_EXPR_OPS];
        ops2[0] = ExprOp::PushLiteral(3.0);
        ops2[1] = ExprOp::PushLiteral(2.0);
        ops2[2] = ExprOp::CallFmod;
        ops2[3] = ExprOp::PushLiteral(0.0);
        ops2[4] = ExprOp::CallSin;
        ops2[5] = ExprOp::CallCos;
        ops2[6] = ExprOp::CallMax;
        let expr2 = ExprProgram { ops: ops2, len: 7 };
        let out2 = eval_expr(&expr2, &vars);
        assert!(
            (out2 - 1.0).abs() < 1e-6,
            "max(fmod(3,2), cos(sin(0))) should evaluate to 1"
        );
    }

    #[test]
    fn eval_expr_supports_boolean_and_comparison_operators() {
        // NOT(a) OR (b AND x > 0)
        let mut vars = [0.0f32; MAX_VARIABLES];
        vars[0] = 0.0; // a = false
        vars[1] = 1.0; // b = true
        vars[2] = 0.5; // x = 0.5

        let mut ops = [ExprOp::PushLiteral(0.0); MAX_EXPR_OPS];
        ops[0] = ExprOp::PushVariable(0);
        ops[1] = ExprOp::BoolNot;
        ops[2] = ExprOp::PushVariable(1);
        ops[3] = ExprOp::PushVariable(2);
        ops[4] = ExprOp::PushLiteral(0.0);
        ops[5] = ExprOp::CmpGt;
        ops[6] = ExprOp::BoolAnd;
        ops[7] = ExprOp::BoolOr;
        let expr = ExprProgram { ops, len: 8 };
        let out = eval_expr(&expr, &vars);
        assert!(
            (out - 1.0).abs() < 1e-6,
            "NOT(false) OR (true AND 0.5 > 0) should evaluate to true"
        );

        vars[0] = 1.0; // a = true
        vars[1] = 0.0; // b = false
        vars[2] = -0.5; // x = -0.5
        let out2 = eval_expr(&expr, &vars);
        assert!(
            (out2 - 0.0).abs() < 1e-6,
            "NOT(true) OR (false AND -0.5 > 0) should evaluate to false"
        );
    }

    #[test]
    fn runtime_loads_variable_initial_values() {
        static STEPS: [Step<'static>; 1] = [Step {
            name: "halt",
            instr: Instr::Halt,
        }];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static VARS: [f32; 3] = [1.5, 2.0, 0.0];
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

        let rt = Runtime::new(&PROGRAM).expect("runtime init should succeed");
        assert_eq!(rt.variables()[0], 1.5);
        assert_eq!(rt.variables()[1], 2.0);
        assert_eq!(rt.variables()[2], 0.0);
        assert_eq!(
            rt.variables()[3],
            0.0,
            "uninitialized variable slots should stay zero"
        );
    }

    #[test]
    fn runtime_rejects_too_many_variables() {
        static STEPS: [Step<'static>; 1] = [Step {
            name: "halt",
            instr: Instr::Halt,
        }];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static VARS: [f32; MAX_VARIABLES + 1] = [0.0; MAX_VARIABLES + 1];
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

        let err = match Runtime::new(&PROGRAM) {
            Ok(_) => panic!("too many variables should fail"),
            Err(err) => err,
        };
        assert_eq!(
            err,
            RuntimeError::TooManyVariables {
                configured: MAX_VARIABLES + 1,
                max: MAX_VARIABLES,
            }
        );
    }

    #[test]
    fn runtime_rejects_too_many_cam_couplings_at_init() {
        static STEPS: [Step<'static>; 1] = [Step {
            name: "halt",
            instr: Instr::Halt,
        }];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];

        let cam_tables: &'static [CamTableData] = Box::leak(
            vec![build_cam_table(false, &[(0.0, 0.0), (360.0, 360.0)])].into_boxed_slice(),
        );
        let cam_configs: &'static [CamCouplingConfig] = Box::leak(
            vec![
                CamCouplingConfig {
                    master_input: AnalogInputId(0),
                    slave_output: AnalogOutputId(0),
                    table_index: 0,
                    interpolation: CamInterpolation::Linear,
                    gear_ratio: 1.0,
                    initial_phase_offset: 0.0,
                    following_error_limit: 1.0,
                    slave_feedback: AnalogInputId(1),
                };
                MAX_CAM_COUPLINGS + 1
            ]
            .into_boxed_slice(),
        );
        let program = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs,
            cam_tables,
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &[],
            workpiece_sites: &[],
            workpiece_holders: &[],
        };

        let err = match Runtime::new(&program) {
            Ok(_) => panic!("too many cam couplings should fail"),
            Err(err) => err,
        };
        assert_eq!(
            err,
            RuntimeError::TooManyCamCouplings {
                configured: MAX_CAM_COUPLINGS + 1,
                max: MAX_CAM_COUPLINGS,
            }
        );
    }

    #[test]
    fn runtime_rejects_invalid_initial_cam_table_index() {
        static STEPS: [Step<'static>; 1] = [Step {
            name: "halt",
            instr: Instr::Halt,
        }];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];

        let cam_tables: &'static [CamTableData] = Box::leak(
            vec![build_cam_table(false, &[(0.0, 0.0), (360.0, 360.0)])].into_boxed_slice(),
        );
        let cam_configs: &'static [CamCouplingConfig] = Box::leak(
            vec![CamCouplingConfig {
                master_input: AnalogInputId(0),
                slave_output: AnalogOutputId(0),
                table_index: 1,
                interpolation: CamInterpolation::Linear,
                gear_ratio: 1.0,
                initial_phase_offset: 0.0,
                following_error_limit: 1.0,
                slave_feedback: AnalogInputId(1),
            }]
            .into_boxed_slice(),
        );
        let program = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs,
            cam_tables,
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &[],
            workpiece_sites: &[],
            workpiece_holders: &[],
        };

        let err = match Runtime::new(&program) {
            Ok(_) => panic!("invalid initial table_index should fail"),
            Err(err) => err,
        };
        assert_eq!(
            err,
            RuntimeError::InvalidCamTableIndex {
                cam_index: 0,
                table_index: 1,
            }
        );
    }

    #[test]
    fn pid_conditional_integration_prevents_windup_after_saturation() {
        let cfg = PidConfig {
            pv: AnalogInputId(0),
            out: AnalogOutputId(0),
            sp: 10.0,
            kp: 0.0,
            ki: 1.0,
            kd: 0.0,
            dt_s: 0.1,
            period_ticks: 1,
            limit_min: 0.0,
            limit_max: 1.0,
            anti_windup: AntiWindup::ConditionalIntegration,
        };
        let mut state = PidState::default();

        // Large positive error; I-term-only controller hits clamp and should stop integrating.
        for _ in 0..20 {
            let _ = pid_step(&cfg, &mut state, 0.0);
        }

        // With conditional integration and ki=1.0, integrator should clamp near limit_max.
        assert!(
            (state.integral - 1.0).abs() < 1e-6,
            "integrator should clamp once output saturates (integral={})",
            state.integral
        );
    }

    #[test]
    fn cam_action_rejects_invalid_index() {
        static ACTIONS: [Action; 1] = [Action::CamDisengage { cam_index: 1 }];
        static STEPS: [Step<'static>; 1] = [Step {
            name: "bad_cam",
            instr: Instr::Action {
                actions: &ACTIONS,
                next: StepId(0),
            },
        }];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];

        let cam_tables: &'static [CamTableData] = Box::leak(
            vec![build_cam_table(false, &[(0.0, 0.0), (360.0, 360.0)])].into_boxed_slice(),
        );
        let cam_configs: &'static [CamCouplingConfig] = Box::leak(
            vec![CamCouplingConfig {
                master_input: AnalogInputId(0),
                slave_output: AnalogOutputId(0),
                table_index: 0,
                interpolation: CamInterpolation::Linear,
                gear_ratio: 1.0,
                initial_phase_offset: 0.0,
                following_error_limit: 1.0,
                slave_feedback: AnalogInputId(1),
            }]
            .into_boxed_slice(),
        );
        let program = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs,
            cam_tables,
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &[],
            workpiece_sites: &[],
            workpiece_holders: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&program).expect("runtime init");
        let err = rt.tick(&mut io).expect_err("invalid cam_index should fail");
        assert_eq!(err, RuntimeError::InvalidCamIndex { cam_index: 1 });
    }

    #[test]
    fn cam_phase_rejects_invalid_index() {
        static PHASE_EXPR: ExprProgram = ExprProgram {
            ops: [ExprOp::PushLiteral(5.0); MAX_EXPR_OPS],
            len: 1,
        };
        static ACTIONS: [Action; 1] = [Action::CamPhase {
            cam_index: 2,
            offset_expr: PHASE_EXPR,
        }];
        static STEPS: [Step<'static>; 1] = [Step {
            name: "bad_phase",
            instr: Instr::Action {
                actions: &ACTIONS,
                next: StepId(0),
            },
        }];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];

        let cam_tables: &'static [CamTableData] = Box::leak(
            vec![build_cam_table(false, &[(0.0, 0.0), (360.0, 360.0)])].into_boxed_slice(),
        );
        let cam_configs: &'static [CamCouplingConfig] = Box::leak(
            vec![CamCouplingConfig {
                master_input: AnalogInputId(0),
                slave_output: AnalogOutputId(0),
                table_index: 0,
                interpolation: CamInterpolation::Linear,
                gear_ratio: 1.0,
                initial_phase_offset: 0.0,
                following_error_limit: 1.0,
                slave_feedback: AnalogInputId(1),
            }]
            .into_boxed_slice(),
        );
        let program = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs,
            cam_tables,
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &[],
            workpiece_sites: &[],
            workpiece_holders: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&program).expect("runtime init");
        let err = rt.tick(&mut io).expect_err("invalid cam_index should fail");
        assert_eq!(err, RuntimeError::InvalidCamIndex { cam_index: 2 });
    }

    #[test]
    fn cam_switch_rejects_invalid_table_index() {
        static ACTIONS: [Action; 1] = [Action::CamSwitch {
            cam_index: 0,
            table_index: 9,
        }];
        static STEPS: [Step<'static>; 1] = [Step {
            name: "bad_table",
            instr: Instr::Action {
                actions: &ACTIONS,
                next: StepId(0),
            },
        }];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];

        let cam_tables: &'static [CamTableData] = Box::leak(
            vec![build_cam_table(false, &[(0.0, 0.0), (360.0, 360.0)])].into_boxed_slice(),
        );
        let cam_configs: &'static [CamCouplingConfig] = Box::leak(
            vec![CamCouplingConfig {
                master_input: AnalogInputId(0),
                slave_output: AnalogOutputId(0),
                table_index: 0,
                interpolation: CamInterpolation::Linear,
                gear_ratio: 1.0,
                initial_phase_offset: 0.0,
                following_error_limit: 1.0,
                slave_feedback: AnalogInputId(1),
            }]
            .into_boxed_slice(),
        );
        let program = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs,
            cam_tables,
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &[],
            workpiece_sites: &[],
            workpiece_holders: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&program).expect("runtime init");
        let err = rt
            .tick(&mut io)
            .expect_err("invalid table_index should fail");
        assert_eq!(
            err,
            RuntimeError::InvalidCamTableIndex {
                cam_index: 0,
                table_index: 9,
            }
        );
    }

    #[test]
    fn cam_switch_keeps_continuity_with_ratio_phase_and_decay() {
        static ENGAGE: [Action; 1] = [Action::CamEngage { cam_index: 0 }];
        static SWITCH: [Action; 1] = [Action::CamSwitch {
            cam_index: 0,
            table_index: 1,
        }];
        static STEPS: [Step<'static>; 4] = [
            Step {
                name: "engage",
                instr: Instr::Action {
                    actions: &ENGAGE,
                    next: StepId(1),
                },
            },
            Step {
                name: "settle_one_tick",
                instr: Instr::Delay {
                    ticks: 1,
                    next: StepId(2),
                },
            },
            Step {
                name: "switch",
                instr: Instr::Action {
                    actions: &SWITCH,
                    next: StepId(3),
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

        let cam_tables: &'static [CamTableData] = Box::leak(
            vec![
                build_cam_table(true, &[(0.0, 0.0), (180.0, 180.0), (360.0, 0.0)]),
                build_cam_table(true, &[(0.0, 50.0), (180.0, 100.0), (360.0, 50.0)]),
            ]
            .into_boxed_slice(),
        );
        let cam_configs: &'static [CamCouplingConfig] = Box::leak(
            vec![CamCouplingConfig {
                master_input: AnalogInputId(0),
                slave_output: AnalogOutputId(0),
                table_index: 0,
                interpolation: CamInterpolation::Linear,
                gear_ratio: 2.0,
                initial_phase_offset: 30.0,
                following_error_limit: 9999.0,
                slave_feedback: AnalogInputId(1),
            }]
            .into_boxed_slice(),
        );
        let program = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs,
            cam_tables,
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &[],
            workpiece_sites: &[],
            workpiece_holders: &[],
        };

        let mut io = MemIo::new();
        io.ai[0] = 45.0;
        io.ai[1] = 0.0;

        let mut rt = Runtime::new(&program).expect("runtime init");
        rt.tick(&mut io).expect("tick0 engage");
        rt.tick(&mut io).expect("tick1 switch");
        let before_switch = io.ao[0];

        rt.tick(&mut io).expect("tick2 apply switch offset");
        let after_switch = io.ao[0];
        assert!(
            (after_switch - before_switch).abs() < 1e-4,
            "switch output should stay continuous, before={before_switch}, after={after_switch}"
        );
        assert_eq!(
            rt.cam_states()[0].switch_decay_ticks,
            99,
            "switch should enter decay tracking"
        );

        let adjusted_master = io.ai[0] * 2.0 + 30.0;
        let switched_base = linear_interpolate(&cam_tables[1], adjusted_master);
        assert!(
            (after_switch - switched_base).abs() > 1e-3,
            "switch offset compensation should remain active on the first tick"
        );

        rt.tick(&mut io).expect("tick3 decay continues");
        assert_eq!(rt.cam_states()[0].switch_decay_ticks, 98);
        assert!(
            (io.ao[0] - after_switch).abs() > 1e-4,
            "output should change while switch decay progresses"
        );
    }

    #[test]
    fn cam_wait_and_phase_actions_work_with_runtime_state() {
        static ENGAGE: [Action; 1] = [Action::CamEngage { cam_index: 0 }];
        static PHASE_EXPR: ExprProgram = ExprProgram {
            ops: [ExprOp::PushLiteral(10.0); MAX_EXPR_OPS],
            len: 1,
        };
        static PHASE: [Action; 1] = [Action::CamPhase {
            cam_index: 0,
            offset_expr: PHASE_EXPR,
        }];
        static STEPS: [Step<'static>; 5] = [
            Step {
                name: "engage",
                instr: Instr::Action {
                    actions: &ENGAGE,
                    next: StepId(1),
                },
            },
            Step {
                name: "wait_engaged",
                instr: Instr::WaitCamDigital {
                    cam_index: 0,
                    field: CamDigitalField::Engage,
                    equals: true,
                    next: StepId(2),
                    timeout: None,
                },
            },
            Step {
                name: "phase",
                instr: Instr::Action {
                    actions: &PHASE,
                    next: StepId(3),
                },
            },
            Step {
                name: "wait_master",
                instr: Instr::WaitCamAnalog {
                    cam_index: 0,
                    field: CamAnalogField::MasterPos,
                    op: CompareOp::Gt,
                    value: 5.0,
                    next: StepId(4),
                    timeout: None,
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

        let cam_tables: &'static [CamTableData] = Box::leak(
            vec![build_cam_table(false, &[(0.0, 0.0), (360.0, 360.0)])].into_boxed_slice(),
        );
        let cam_configs: &'static [CamCouplingConfig] = Box::leak(
            vec![CamCouplingConfig {
                master_input: AnalogInputId(0),
                slave_output: AnalogOutputId(0),
                table_index: 0,
                interpolation: CamInterpolation::Linear,
                gear_ratio: 1.0,
                initial_phase_offset: 0.0,
                following_error_limit: 1000.0,
                slave_feedback: AnalogInputId(1),
            }]
            .into_boxed_slice(),
        );
        let program = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs,
            cam_tables,
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &[],
            workpiece_sites: &[],
            workpiece_holders: &[],
        };

        let mut io = MemIo::new();
        io.ai[0] = 20.0;
        io.ai[1] = 0.0;
        let mut rt = Runtime::new(&program).expect("runtime init");

        rt.tick(&mut io)
            .expect("tick0 should progress to wait_master");
        assert_eq!(rt.location().step, StepId(3));

        rt.tick(&mut io).expect("tick1 should satisfy wait_master");
        assert_eq!(rt.location().step, StepId(4));
        assert!(
            (io.ao[0] - 30.0).abs() < 1e-5,
            "phase offset should shift cam output"
        );
    }

    #[test]
    fn cam_fault_disengages_when_following_error_exceeds_limit() {
        static ENGAGE: [Action; 1] = [Action::CamEngage { cam_index: 0 }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "engage",
                instr: Instr::Action {
                    actions: &ENGAGE,
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

        let cam_tables: &'static [CamTableData] = Box::leak(
            vec![build_cam_table(false, &[(0.0, 0.0), (360.0, 360.0)])].into_boxed_slice(),
        );
        let cam_configs: &'static [CamCouplingConfig] = Box::leak(
            vec![CamCouplingConfig {
                master_input: AnalogInputId(0),
                slave_output: AnalogOutputId(0),
                table_index: 0,
                interpolation: CamInterpolation::Linear,
                gear_ratio: 1.0,
                initial_phase_offset: 0.0,
                following_error_limit: 1.0,
                slave_feedback: AnalogInputId(1),
            }]
            .into_boxed_slice(),
        );
        let program = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs,
            cam_tables,
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &[],
            workpiece_sites: &[],
            workpiece_holders: &[],
        };

        let mut io = MemIo::new();
        io.ai[0] = 180.0;
        io.ai[1] = 0.0;
        let mut rt = Runtime::new(&program).expect("runtime init");

        rt.tick(&mut io).expect("tick0 engage");
        rt.tick(&mut io).expect("tick1 update cam and detect fault");

        let cam = rt.cam_states()[0];
        assert!(cam.fault, "following error should raise fault");
        assert!(!cam.engaged, "fault should disengage cam");
    }
