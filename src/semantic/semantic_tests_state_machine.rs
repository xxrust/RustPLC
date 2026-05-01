    #[test]
    fn builds_state_machine_from_prd_5_5_1_sequence_example() {
        let input = r#"
[topology]
device cyl_A: cylinder
device cyl_B: cylinder
device sensor_A_ext: sensor
device sensor_A_ret: sensor
device sensor_B_ext: sensor
device sensor_B_ret: sensor
device start_button: sensor

[constraints]

[tasks]

task init:
    step extend_A:
        action: extend cyl_A
        wait: sensor_A_ext == true
        timeout: 600ms -> goto fault_handler

    step retract_A:
        action: retract cyl_A
        wait: sensor_A_ret == true
        timeout: 500ms -> goto fault_handler

    step extend_B:
        action: extend cyl_B
        wait: sensor_B_ext == true
        timeout: 800ms -> goto fault_handler

    step retract_B:
        action: retract cyl_B
        wait: sensor_B_ret == true
        timeout: 700ms -> goto fault_handler

    on_complete: goto ready

task fault_handler:
    step safe_position:
        action: retract cyl_A
    on_complete: goto ready

task ready:
    step wait_start:
        wait: start_button == true
        allow_indefinite_wait: true
    on_complete: goto init
"#;

        let program = parse_plc(input).expect("PRD 5.5.1 示例应能成功解析为 AST");
        let state_machine = build_state_machine(&program).expect("应能从 5.5.1 示例构建状态机");

        assert!(
            state_machine
                .states
                .iter()
                .any(|state| state.task_name == "init" && state.step_name == "extend_A")
        );
        assert!(
            state_machine
                .states
                .iter()
                .any(|state| state.task_name == "init" && state.step_name == "retract_B")
        );

        let has_wait_transition = state_machine.transitions.iter().any(|transition| {
            transition.from.task_name == "init"
                && transition.from.step_name == "extend_A"
                && transition.to.task_name == "init"
                && transition.to.step_name == "retract_A"
                && matches!(
                    transition.guard,
                    TransitionGuard::Condition { ref expression }
                        if expression == "sensor_A_ext == true"
                )
        });
        assert!(has_wait_transition, "应存在 wait 条件驱动的顺序转移");

        let has_timeout_transition = state_machine.transitions.iter().any(|transition| {
            transition.from.task_name == "init"
                && transition.from.step_name == "extend_A"
                && transition.to.task_name == "fault_handler"
                && transition.to.step_name == "safe_position"
                && matches!(
                    transition.guard,
                    TransitionGuard::Timeout { duration_ms } if duration_ms == 600
                )
        });
        assert!(has_timeout_transition, "timeout 应创建带定时守卫的跳转");

        let has_on_complete_goto = state_machine.transitions.iter().any(|transition| {
            transition.from.task_name == "init"
                && transition.from.step_name == "retract_B"
                && transition.to.task_name == "ready"
                && transition.to.step_name == "wait_start"
                && matches!(
                    transition.guard,
                    TransitionGuard::Condition { ref expression }
                        if expression == "sensor_B_ret == true"
                )
        });
        assert!(
            has_on_complete_goto,
            "最后一步应能够通过 on_complete 跳转到 ready"
        );
    }

    #[test]
    fn lowers_delay_statement_into_bounded_transition_to_next_step() {
        let input = r#"
[topology]

[constraints]

[tasks]

task init:
    step warmup:
        delay: 2000ms
    step work:
        action: log "start"
"#;

        let program = parse_plc(input).expect("测试输入应能成功解析为 AST");
        let state_machine = build_state_machine(&program).expect("delay 应能降级为状态机转移");

        let delay_transition = state_machine
            .transitions
            .iter()
            .find(|transition| {
                transition.from.task_name == "init"
                    && transition.from.step_name == "warmup"
                    && transition.to.task_name == "init"
                    && transition.to.step_name == "work"
                    && matches!(transition.guard, TransitionGuard::Delay { duration_ms } if duration_ms == 2000)
            })
            .expect("delay 应生成到下一个 step 的有界等待转移");

        assert!(
            delay_transition.actions.is_empty(),
            "delay 转移不应重复执行动作"
        );
        assert_eq!(delay_transition.timers.len(), 1);
        assert_eq!(
            delay_transition.timers[0].operation,
            TimerOperationKind::Start
        );
        assert_eq!(delay_transition.timers[0].duration_ms, Some(2000));
    }

    #[test]
    fn keeps_timeout_as_protective_upper_bound_when_delay_and_timeout_coexist() {
        let input = r#"
[topology]

[constraints]

[tasks]

task init:
    step wait_heat:
        delay: 300ms
        timeout: 1200ms -> goto fault_handler
    step run:
        action: log "running"

task fault_handler:
    step safe_stop:
        action: log "fault"
"#;

        let program = parse_plc(input).expect("测试输入应能成功解析为 AST");
        let state_machine = build_state_machine(&program).expect("delay + timeout 应可共存");

        let has_delay_to_next = state_machine.transitions.iter().any(|transition| {
            transition.from.task_name == "init"
                && transition.from.step_name == "wait_heat"
                && transition.to.task_name == "init"
                && transition.to.step_name == "run"
                && matches!(transition.guard, TransitionGuard::Delay { duration_ms } if duration_ms == 300)
        });
        assert!(has_delay_to_next, "delay 应指向当前 task 的下一个 step");

        let has_timeout_escape = state_machine.transitions.iter().any(|transition| {
            transition.from.task_name == "init"
                && transition.from.step_name == "wait_heat"
                && transition.to.task_name == "fault_handler"
                && transition.to.step_name == "safe_stop"
                && matches!(transition.guard, TransitionGuard::Timeout { duration_ms } if duration_ms == 1200)
        });
        assert!(has_timeout_escape, "timeout 应保留为保护性上界跳转");
    }

    #[test]
    fn builds_state_machine_race_branches_from_prd_9_example() {
        let input = r#"
[topology]

[constraints]

[tasks]

task search:
    step start_motor:
        action: set motor_ctrl.run on
    step detect:
        race:
            branch_A:
                wait: sensor_A == true
                then: goto process_A
            branch_B:
                wait: sensor_B == true
                then: goto process_B
        timeout: 800ms -> goto motor_fault

task process_A:
    step stop_motor:
        action: set motor_ctrl.run off
    on_complete: goto ready

task process_B:
    step stop_motor:
        action: set motor_ctrl.run off
    on_complete: goto ready

task motor_fault:
    step emergency_stop:
        action: set motor_ctrl.run off
    on_complete: goto ready

task ready:
    step wait_start:
        wait: start_button == true
        allow_indefinite_wait: true
    on_complete: goto search
"#;

        let program = parse_plc(input).expect("PRD 9 示例应能成功解析为 AST");
        let state_machine = build_state_machine(&program).expect("应能构建 race 状态机");

        assert!(state_machine.states.iter().any(
            |state| state.task_name == "search" && state.step_name == "detect__race_1_decision"
        ));
        assert!(state_machine.states.iter().any(
            |state| state.task_name == "search" && state.step_name == "detect__race_1_branch_1"
        ));

        let has_branch_a_transition = state_machine.transitions.iter().any(|transition| {
            transition.from.task_name == "search"
                && transition.from.step_name == "detect__race_1_branch_1"
                && transition.to.task_name == "process_A"
                && transition.to.step_name == "stop_motor"
                && matches!(
                    transition.guard,
                    TransitionGuard::Condition { ref expression }
                        if expression == "sensor_A == true"
                )
        });
        assert!(has_branch_a_transition, "race 分支 A 应创建条件跳转");

        let has_branch_b_transition = state_machine.transitions.iter().any(|transition| {
            transition.from.task_name == "search"
                && transition.from.step_name == "detect__race_1_branch_2"
                && transition.to.task_name == "process_B"
                && transition.to.step_name == "stop_motor"
                && matches!(
                    transition.guard,
                    TransitionGuard::Condition { ref expression }
                        if expression == "sensor_B == true"
                )
        });
        assert!(has_branch_b_transition, "race 分支 B 应创建条件跳转");

        let has_timeout_transition = state_machine.transitions.iter().any(|transition| {
            transition.from.task_name == "search"
                && transition.from.step_name == "detect"
                && transition.to.task_name == "motor_fault"
                && transition.to.step_name == "emergency_stop"
                && matches!(
                    transition.guard,
                    TransitionGuard::Timeout { duration_ms } if duration_ms == 800
                )
        });
        assert!(
            has_timeout_transition,
            "race 所在 step 应保留 timeout 守卫跳转"
        );
    }

    #[test]
    fn reports_undefined_goto_target_with_line_number() {
        let input = r#"
[topology]

[constraints]

[tasks]

task init:
    step start:
        goto missing_task
"#;

        let program = parse_plc(input).expect("测试输入应能成功解析为 AST");
        let errors = build_state_machine(&program).expect_err("未定义 goto 目标应返回语义错误");

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].line(), 10);
        assert!(
            errors[0].to_string().contains("未定义 task missing_task"),
            "错误消息应包含未定义 task 名称"
        );
    }

    #[test]
    fn rejects_goto_to_synthetic_parallel_step() {
        let input = r#"
[topology]

[constraints]

[tasks]

task main:
    step start:
        parallel:
            branch_A:
                action: log "A"
    step jump:
        goto main.start__parallel_1_fork
"#;

        let program = parse_plc(input).expect("测试输入应能成功解析为 AST");
        let errors = build_state_machine(&program).expect_err("跳转到合成 step 应报语义错误");

        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("不允许跳转到 parallel/race 内部合成 step"),
            "应提示不允许跳转到合成 step"
        );
    }

    #[test]
    fn expands_repeat_block_into_sequential_steps_with_suffixes() {
        let input = r#"
[topology]

[constraints]

[tasks]

task init:
    step glue_cycle:
        repeat 3:
            action: log "tick"
"#;

        let program = parse_plc(input).expect("repeat 示例应能成功解析为 AST");
        let state_machine = build_state_machine(&program).expect("repeat 应在语义阶段展开");

        for suffix in ["glue_cycle_1", "glue_cycle_2", "glue_cycle_3"] {
            assert!(
                state_machine
                    .states
                    .iter()
                    .any(|state| { state.task_name == "init" && state.step_name == suffix }),
                "repeat 展开后应包含 step {suffix}"
            );
        }

        let has_1_to_2 = state_machine.transitions.iter().any(|transition| {
            transition.from.task_name == "init"
                && transition.from.step_name == "glue_cycle_1"
                && transition.to.task_name == "init"
                && transition.to.step_name == "glue_cycle_2"
                && matches!(transition.guard, TransitionGuard::Always)
        });
        assert!(has_1_to_2, "glue_cycle_1 应顺序链接到 glue_cycle_2");

        let has_2_to_3 = state_machine.transitions.iter().any(|transition| {
            transition.from.task_name == "init"
                && transition.from.step_name == "glue_cycle_2"
                && transition.to.task_name == "init"
                && transition.to.step_name == "glue_cycle_3"
                && matches!(transition.guard, TransitionGuard::Always)
        });
        assert!(has_2_to_3, "glue_cycle_2 应顺序链接到 glue_cycle_3");
    }

    #[test]
    fn reports_semantic_error_for_repeat_count_zero_or_one() {
        for count in [0, 1] {
            let input = format!(
                r#"
[topology]

[constraints]

[tasks]

task init:
    step glue_cycle:
        repeat {count}:
            action: log "tick"
"#
            );

            let program = parse_plc(&input).expect("repeat 语法应能解析");
            let errors = build_state_machine(&program).expect_err("repeat 0/1 应报语义错误");
            let joined = errors
                .iter()
                .map(|err| err.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                joined.contains("repeat 次数必须在 2..=100 之间"),
                "应包含 repeat 次数范围错误提示"
            );
        }
    }

    #[test]
    fn reports_semantic_error_for_repeat_count_over_limit() {
        let input = r#"
[topology]

[constraints]

[tasks]

task init:
    step glue_cycle:
        repeat 101:
            action: log "tick"
"#;

        let program = parse_plc(input).expect("repeat 语法应能解析");
        let errors = build_state_machine(&program).expect_err("repeat > 100 应报语义错误");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("repeat 次数超过上限 100"),
            "应包含 repeat 次数上限错误提示"
        );
    }

    #[test]
    fn reports_semantic_error_for_nested_repeat_blocks() {
        let input = r#"
[topology]

[constraints]

[tasks]

task init:
    step glue_cycle:
        repeat 2:
            repeat 2:
                action: log "tick"
"#;

        let program = parse_plc(input).expect("嵌套 repeat 语法应能解析");
        let errors = build_state_machine(&program).expect_err("嵌套 repeat 应报语义错误");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("不允许嵌套 repeat"),
            "应包含嵌套 repeat 错误提示"
        );
    }

    #[test]
    fn lowers_topology_variables_into_ir_defs() {
        let input = r#"
[topology]
device plc_main: plc
variable master_pos: float = 0.5
variable cycle_count: int = 2
variable cam_active: bool = true

[constraints]

[tasks]
task main:
    step idle:
        action: log "ok"
"#;

        let program = parse_plc(input).expect("变量示例应能解析");
        let topology = build_topology_graph(&program).expect("变量示例应能构建拓扑");

        assert_eq!(topology.variables.len(), 3);
        assert_eq!(topology.variables[0].name, "master_pos");
        assert!(matches!(
            topology.variables[0].var_type,
            crate::ir::VariableType::Float
        ));
        assert_eq!(topology.variables[0].initial_value, 0.5);
        assert_eq!(topology.variables[0].index, 0);
        assert!(matches!(
            topology.variables[1].var_type,
            crate::ir::VariableType::Int
        ));
        assert_eq!(topology.variables[1].initial_value, 2.0);
        assert!(matches!(
            topology.variables[2].var_type,
            crate::ir::VariableType::Bool
        ));
        assert_eq!(topology.variables[2].initial_value, 1.0);
    }

    #[test]
    fn lowers_cam_tables_into_ir_defs() {
        let input = r#"
[topology]
cam_table linear_cam: periodic [
    (0, 0),
    (180, 50),
    (360, 0),
]
cam_table shear_profile: oneshot [
    (0, 0),
    (30, 20),
    (60, 45),
    (90, 20),
]

[constraints]

[tasks]
task main:
    step idle:
        action: log "ok"
"#;

        let program = parse_plc(input).expect("cam_table 示例应能解析");
        let topology = build_topology_graph(&program).expect("cam_table 示例应能构建拓扑");

        assert_eq!(topology.cam_tables.len(), 2);
        assert_eq!(topology.cam_tables[0].name, "linear_cam");
        assert!(topology.cam_tables[0].periodic);
        assert_eq!(topology.cam_tables[0].num_points, 3);
        assert_eq!(topology.cam_tables[0].spline_coeffs.len(), 2);
        assert!(
            topology.cam_tables[0]
                .spline_coeffs
                .iter()
                .any(|coeff| coeff.c.abs() > 1e-6 || coeff.d.abs() > 1e-6),
            "periodic 曲线应生成非零二/三次项系数"
        );
        assert_eq!(topology.cam_tables[1].name, "shear_profile");
        assert!(!topology.cam_tables[1].periodic);
        assert!(
            topology.cam_tables[1]
                .spline_coeffs
                .iter()
                .any(|coeff| coeff.c.abs() > 1e-6 || coeff.d.abs() > 1e-6),
            "oneshot 曲线应生成非零二/三次项系数"
        );
    }

    #[test]
    fn periodic_cam_table_coeffs_are_c2_continuous_on_boundaries() {
        let input = r#"
[topology]
cam_table smooth_periodic: periodic [
    (0, 0),
    (90, 40),
    (180, 10),
    (270, 50),
    (360, 0),
]

[constraints]

[tasks]
task main:
    step idle:
        action: log "ok"
"#;

        let program = parse_plc(input).expect("cam_table 示例应能解析");
        let topology = build_topology_graph(&program).expect("cam_table 示例应能构建拓扑");
        let table = topology
            .cam_tables
            .iter()
            .find(|table| table.name == "smooth_periodic")
            .expect("应包含 smooth_periodic");
        let eval = |coeff: &crate::ir::SplineCoeff, dx: f32| {
            coeff.a + dx * (coeff.b + dx * (coeff.c + dx * coeff.d))
        };
        let d1 = |coeff: &crate::ir::SplineCoeff, dx: f32| {
            coeff.b + dx * (2.0 * coeff.c + 3.0 * coeff.d * dx)
        };
        let d2 = |coeff: &crate::ir::SplineCoeff, dx: f32| 2.0 * coeff.c + 6.0 * coeff.d * dx;

        let pos_tol = 1e-3f32;
        let d1_tol = 1e-3f32;
        let d2_tol = 2e-3f32;
        let last_segment = table.num_points.saturating_sub(2);

        for boundary in 1..table.num_points.saturating_sub(1) {
            let left = &table.spline_coeffs[boundary - 1];
            let right = &table.spline_coeffs[boundary];
            let dx_left = table.master_positions[boundary] - table.master_positions[boundary - 1];

            assert!(
                (eval(left, dx_left) - eval(right, 0.0)).abs() <= pos_tol,
                "boundary {boundary} position continuity failed"
            );
            assert!(
                (d1(left, dx_left) - d1(right, 0.0)).abs() <= d1_tol,
                "boundary {boundary} first-derivative continuity failed"
            );
            assert!(
                (d2(left, dx_left) - d2(right, 0.0)).abs() <= d2_tol,
                "boundary {boundary} second-derivative continuity failed"
            );
        }

        let left = &table.spline_coeffs[last_segment];
        let right = &table.spline_coeffs[0];
        let dx_left =
            table.master_positions[last_segment + 1] - table.master_positions[last_segment];
        assert!(
            (eval(left, dx_left) - eval(right, 0.0)).abs() <= pos_tol,
            "periodic boundary position continuity failed"
        );
        assert!(
            (d1(left, dx_left) - d1(right, 0.0)).abs() <= d1_tol,
            "periodic boundary first-derivative continuity failed"
        );
        assert!(
            (d2(left, dx_left) - d2(right, 0.0)).abs() <= d2_tol,
            "periodic boundary second-derivative continuity failed"
        );
    }

    #[test]
    fn rejects_invalid_cam_table_shapes() {
        let input = r#"
[topology]
device linear_cam: sensor
cam_table linear_cam: periodic [
    (0, 0),
    (360, 0),
]
cam_table bad_profile: periodic [
    (0, 0),
    (120, 40),
    (90, 40),
    (360, 10),
]

[constraints]

[tasks]
task main:
    step idle:
        action: log "ok"
"#;

        let program = parse_plc(input).expect("示例语法应可解析");
        let errors = build_topology_graph(&program).expect_err("无效 cam_table 应报错");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("cam_table 名称不能与 device/variable 重名"));
        assert!(joined.contains("master 坐标必须严格递增"));
    }

    #[test]
    fn lowers_cam_coupling_defs_and_links() {
        let input = r#"
[topology]
device AI0: analog_input { range: 0..360 }
device AO0: analog_output { range: 0..360 }
device cam_xy: cam_coupling {
    master: AI0,
    slave: AO0,
    table: linear_cam,
    interpolation: linear,
    gear_ratio: 2.0,
    phase_offset: 3.0,
    following_error_limit: 1.5,
}
cam_table linear_cam: periodic [
    (0, 0),
    (360, 0),
]

[constraints]

[tasks]
task main:
    step idle:
        action: log "ok"
"#;

        let program = parse_plc(input).expect("cam_coupling 示例应能解析");
        let topology = build_topology_graph(&program).expect("cam_coupling 示例应能构建拓扑");
        assert_eq!(topology.cam_couplings.len(), 1);
        let cam = &topology.cam_couplings[0];
        assert_eq!(cam.name, "cam_xy");
        assert_eq!(cam.master, "AI0");
        assert_eq!(cam.slave, "AO0");
        assert_eq!(cam.table, "linear_cam");
        assert!(matches!(
            cam.interpolation,
            crate::ir::CamInterpolation::Linear
        ));
    }

    #[test]
    fn rejects_invalid_cam_actions() {
        let input = r#"
[topology]
device AI0: analog_input { range: 0..360 }
device AO0: analog_output { range: 0..360 }
device cam_xy: cam_coupling { master: AI0, slave: AO0, table: t0 }
device motor_x: motor
cam_table t0: periodic [
    (0, 0),
    (360, 0),
]

[constraints]

[tasks]
task main:
    step run:
        action: cam_switch cam_xy missing_table
        action: cam_engage motor_x
"#;

        let program = parse_plc(input).expect("示例语法应可解析");
        let errors = build_state_machine(&program).expect_err("无效 cam action 应报错");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("cam_switch 的目标表需要先在 [topology] 中声明"));
        assert!(joined.contains("cam 动作仅支持作用于 cam_coupling 设备"));
    }

    #[test]
    fn rejects_raw_motor_drive_write_when_process_device_exists() {
        let input = r#"
[topology]
device plc_main: plc { model_ref: openplc_softplc }
device drive_motor: motor
device conveyor_main: conveyor

[constraints]

[tasks]
task main:
    step run:
        action: set drive_motor.run on
"#;

        let program = parse_plc(input).expect("source should parse");
        let errors = build_state_machine(&program).expect_err("raw drive provider write fails");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("SEM-110"));
        assert!(joined.contains("drive capability provider"));
    }

    #[test]
    fn rejects_double_solenoid_coils_energized_together() {
        let input = r#"
[topology]
device valve_shift: solenoid_valve {
    ports: [coil_A:digital:consumer, coil_B:digital:consumer, out:pneumatic:producer]
}

[constraints]

[tasks]
task main:
    step illegal:
        action: set valve_shift.coil_A on
        action: set valve_shift.coil_B on
"#;

        let program = parse_plc(input).expect("source should parse");
        let errors = build_state_machine(&program).expect_err("coil conflict fails");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("mutually exclusive coils"));
        assert!(joined.contains("coil_A"));
        assert!(joined.contains("coil_B"));
    }

    #[test]
    fn rejects_variable_initial_value_type_mismatch() {
        let input = r#"
[topology]
device plc_main: plc
variable cycle_count: int = true

[constraints]

[tasks]
task main:
    step idle:
        action: log "ok"
"#;

        let program = parse_plc(input).expect("示例语法应可解析");
        let errors = build_topology_graph(&program).expect_err("错误变量初值应被拒绝");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("int 初值应为整数"),
            "应提示 int 初值类型错误"
        );
    }

    #[test]
    fn rejects_variable_name_colliding_with_device() {
        let input = r#"
[topology]
device cam_xy: plc
variable cam_xy: bool = false

[constraints]

[tasks]
task main:
    step idle:
        action: log "ok"
"#;

        let program = parse_plc(input).expect("示例语法应可解析");
        let errors = build_topology_graph(&program).expect_err("变量与设备重名应报错");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("变量名不能与设备名或 cam_table 名相同"),
            "应提示符号重名冲突"
        );
    }

    #[test]
    fn rejects_unknown_builtin_function_in_expression() {
        let input = r#"
[topology]
device ao0: analog_output { range: 0..100 }
variable x: float = 1.0

[constraints]

[tasks]
task main:
    step run:
        action: set_analog ao0 foo(x)
"#;

        let program = parse_plc(input).expect("示例语法应可解析");
        let errors = build_state_machine(&program).expect_err("未知函数应报语义错误");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("不支持的内置函数"), "应提示不支持的函数名");
    }

    #[test]
    fn rejects_undefined_variable_in_expression_condition() {
        let input = r#"
[topology]
variable known: float = 1.0

[constraints]

[tasks]
task main:
    step wait_expr:
        wait: known + missing > 0.0
"#;

        let program = parse_plc(input).expect("示例语法应可解析");
        let errors = build_state_machine(&program).expect_err("条件表达式中未知变量应报错");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("表达式变量必须先在 [topology] 中使用 variable 声明"),
            "应报告表达式条件中的未知变量"
        );
    }

    #[test]
    fn rejects_builtin_function_with_wrong_arity() {
        let input = r#"
[topology]
device ao0: analog_output { range: 0..100 }
variable x: float = 1.0

[constraints]

[tasks]
task main:
    step run:
        action: set_analog ao0 clamp(x, 0)
"#;

        let program = parse_plc(input).expect("示例语法应可解析");
        let errors = build_state_machine(&program).expect_err("函数参数个数错误应报语义错误");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("参数个数错误"), "应提示函数参数个数错误");
    }

    #[test]
    fn lowers_extern_function_metadata_into_ir_topology() {
        let input = r#"
[topology]
extern function add(a: float, b: float) -> float {
    rust_module: "math::add"
    pure: true
    time_bound_us: 100
}

[constraints]

[tasks]
task main:
    step idle:
"#;

        let program = parse_plc(input).expect("extern function 示例应能解析");
        let topology = build_topology_graph(&program).expect("应能构建包含 extern 的拓扑");

        assert_eq!(topology.extern_functions.len(), 1);
        let add = &topology.extern_functions[0];
        assert_eq!(add.name, "add");
        assert_eq!(add.params.len(), 2);
        assert!(matches!(
            add.return_types.as_slice(),
            [crate::ir::VariableType::Float]
        ));
        assert_eq!(add.contract.rust_module, "math::add");
        assert!(add.contract.pure);
        assert_eq!(add.contract.time_bound_us, 100);
    }

    #[test]
    fn lowers_action_call_into_ir_transition_action() {
        let input = r#"
[topology]
variable temperature: float = 0.0
variable lo: float = 0.0
variable hi: float = 0.0
extern function split(v: float) -> (float, float) {
    rust_module: "math::split"
    pure: true
    time_bound_us: 120
}

[constraints]

[tasks]
task main:
    step run:
        action: call split(temperature) -> (lo, hi)
    on_complete: goto done

task done:
    step idle:
"#;

        let program = parse_plc(input).expect("extern call 示例应能解析");
        let sm = build_state_machine(&program).expect("extern call 应能 lowering 到 IR");

        let action = sm
            .transitions
            .iter()
            .flat_map(|transition| transition.actions.iter())
            .find_map(|action| match action {
                crate::ir::TransitionAction::CallExtern {
                    function,
                    args_raw,
                    binding,
                } => Some((function, args_raw, binding)),
                _ => None,
            })
            .expect("状态机 transition 中应包含 call_extern 动作");

        assert_eq!(action.0, "split");
        assert_eq!(action.1, &vec!["temperature".to_string()]);
        assert!(matches!(
            action.2,
            crate::ir::ExternCallBinding::Tuple(names)
                if names == &vec!["lo".to_string(), "hi".to_string()]
        ));
    }

    #[test]
    fn validates_station_protocol_and_blocks_cross_station_device_writes() {
        let input = r#"
[topology]
device cyl_load: cylinder
device cyl_press: cylinder
site handoff: workpiece_location { capacity: 1 }

station st01 { owns: [cyl_load], tasks: [load_cycle] }
station st02 { owns: [cyl_press], tasks: [press_cycle] }
handshake st01_to_st02 {
    from: st01,
    to: st02,
    request: st01_request,
    allow: st02_allow,
    complete: st01_complete,
    timeout: 5000ms -> goto fault.timeout
}
transfer_point load_to_press {
    from_station: st01,
    to_station: st02,
    site: handoff,
    handshake: st01_to_st02
}

[constraints]

[tasks]
task load_cycle:
    step bad:
        action: extend cyl_press
task press_cycle:
    step idle:
task fault:
    step timeout:
"#;

        let program = parse_plc(input).expect("station protocol should parse");
        let errors = build_state_machine(&program).expect_err("cross-station write should fail");
        let rendered = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("[SEM-204]")
                && rendered.contains("load_cycle")
                && rendered.contains("cyl_press"),
            "expected station ownership diagnostic, got: {rendered}"
        );
    }

    #[test]
    fn rejects_invalid_station_handshake_and_transfer_point_contracts() {
        let input = r#"
[topology]
device cyl_load: cylinder
site handoff: workpiece_location { capacity: 2 }

station st01 { owns: [cyl_load], tasks: [load_cycle] }
handshake bad_hs {
    from: st01,
    to: missing_station,
    request: hs_request,
    allow: hs_allow,
    complete: hs_done,
    timeout: 5000ms -> goto missing_task
}
transfer_point bad_tp {
    from_station: st01,
    to_station: missing_station,
    site: handoff,
    handshake: bad_hs
}

[constraints]

[tasks]
task load_cycle:
    step idle:
"#;

        let program = parse_plc(input).expect("station protocol should parse");
        let errors = build_state_machine(&program).expect_err("invalid protocol should fail");
        let rendered = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("[SEM-211]"), "{rendered}");
        assert!(rendered.contains("[SEM-214]"), "{rendered}");
        assert!(rendered.contains("[SEM-221]"), "{rendered}");
        assert!(rendered.contains("[SEM-223]"), "{rendered}");
    }

