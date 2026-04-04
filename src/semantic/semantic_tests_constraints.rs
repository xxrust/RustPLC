    #[test]
    fn builds_constraint_set_and_timing_model_from_prd_5_4_example() {
        let input = r#"
[topology]

device Y0: digital_output
device Y1: digital_output
device motor_ctrl: motor {
    ramp_time: 50ms
}

device valve_A: solenoid_valve {
    response_time: 15ms
}

device valve_B: solenoid_valve {
    response_time: 15ms
}

device cyl_A: cylinder {
    stroke_time: 200ms,
    retract_time: 180ms
}

device cyl_B: cylinder {
    stroke_time: 300ms,
    retract_time: 250ms
}

device sensor_A_ext: sensor
device sensor_B_ext: sensor

relation { from: Y0.out, to: motor_ctrl.cmd, via: driven_by }
relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: Y1.out, to: valve_B.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: valve_B.out, to: cyl_B.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_A_ext.sense, via: detects }
relation { from: cyl_B.extended, to: sensor_B_ext.sense, via: detects }

[constraints]

safety: cyl_A.extended conflicts_with cyl_B.extended
    reason: "A缸和B缸同时伸出会导致机械碰撞"

safety: valve_A.on conflicts_with valve_B.on
    reason: "气源压力不足以同时驱动两个阀"

timing: task.init must_complete_within 5000ms
    reason: "初始化超过5秒视为异常"

timing: task.init.step_extend_A must_complete_within 500ms
    reason: "单步动作不应超过500ms"

causality: Y0 -> valve_A -> cyl_A -> sensor_A_ext
    reason: "Y0 驱动 valve_A 推动 cyl_A 由 sensor_A_ext 检测"

causality: Y1 -> valve_B -> cyl_B -> sensor_B_ext
    reason: "Y1 驱动 valve_B 推动 cyl_B 由 sensor_B_ext 检测"

[tasks]

task init:
    step step_extend_A:
        action: extend cyl_A
    step step_retract_A:
        action: retract cyl_A

task ready:
    step start_motor:
        action: set motor_ctrl.run on
"#;

        let program = parse_plc(input).expect("PRD 5.4 示例应能成功解析为 AST");
        let constraints = build_constraint_set(&program).expect("应能构建约束集合");
        let timing_model = build_timing_model(&program).expect("应能构建设备时序模型");

        assert_eq!(constraints.safety.len(), 2);
        assert_eq!(constraints.timing.len(), 2);
        assert_eq!(constraints.causality.len(), 2);

        assert!(matches!(
            constraints.safety[0].relation,
            SafetyRelation::ConflictsWith
        ));
        match &constraints.safety[0].left {
            crate::ir::SafetyExpr::State(expr) => {
                assert_eq!(expr.device, "cyl_A");
                assert_eq!(expr.state, "extended");
            }
            other => panic!("期望 State 变体，实际为: {other:?}"),
        }

        assert!(matches!(
            constraints.timing[0].scope,
            TimingScope::Task { ref task } if task == "init"
        ));
        assert!(matches!(
            constraints.timing[0].relation,
            TimingRelation::MustCompleteWithin
        ));
        assert_eq!(constraints.timing[0].duration_ms, 5000);

        assert!(matches!(
            constraints.timing[1].scope,
            TimingScope::Step { ref task, ref step } if task == "init" && step == "step_extend_A"
        ));
        assert_eq!(constraints.causality[0].devices.len(), 4);
        assert_eq!(constraints.causality[0].devices[0], "Y0");
        assert_eq!(constraints.causality[0].devices[3], "sensor_A_ext");

        let extend_key = "init.step_extend_A.extend.cyl_A";
        let retract_key = "init.step_retract_A.retract.cyl_A";
        let motor_key = "ready.start_motor.set.motor_ctrl";

        assert_eq!(timing_model.intervals[extend_key].interval.min_ms, 200);
        assert_eq!(timing_model.intervals[extend_key].interval.max_ms, 200);
        assert_eq!(timing_model.intervals[retract_key].interval.min_ms, 180);
        assert_eq!(timing_model.intervals[motor_key].interval.min_ms, 50);
    }

    #[test]
    fn builds_constraint_set_with_must_complete_within_worst_case_relation() {
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

timing: task.init must_complete_within_worst_case 1000ms

[tasks]

task init:
    step start:
        action: log "ok"
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let constraints = build_constraint_set(&program).expect("应能构建约束集合");

        assert_eq!(constraints.timing.len(), 1);
        assert!(matches!(
            constraints.timing[0].relation,
            TimingRelation::MustCompleteWithinWorstCase
        ));
        assert_eq!(constraints.timing[0].duration_ms, 1000);
    }

    #[test]
    fn reports_constraint_reference_errors_for_undefined_device_state_and_task() {
        let input = r#"
[topology]

device cyl_A: cylinder {
    stroke_time: 200ms,
    retract_time: 180ms
}

[constraints]

safety: cyl_A.invalid_state conflicts_with missing_device.on
timing: task.unknown must_complete_within 100ms
causality: cyl_A -> missing_device

[tasks]

task init:
    step start:
        action: extend cyl_A
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let errors = build_constraint_set(&program).expect_err("未定义引用应报错");

        assert_eq!(errors.len(), 4);
        assert!(
            errors
                .iter()
                .any(|err| err.to_string().contains("未定义状态 invalid_state")),
            "应报告未定义状态"
        );
        assert!(
            errors
                .iter()
                .any(|err| err.to_string().contains("未定义设备 missing_device")),
            "应报告未定义设备"
        );
        assert!(
            errors
                .iter()
                .any(|err| err.to_string().contains("未定义 task unknown")),
            "应报告未定义 task"
        );
    }

    #[test]
    fn allows_causality_nodes_for_extern_functions_and_variables() {
        let input = r#"
[topology]

device pressure_in: analog_input { range: 0..10 }
variable normalized: float = 0.0
extern function normalize(v: float) -> float {
    rust_module: "math::normalize"
    pure: true
    time_bound_us: 100
}

[constraints]

causality: pressure_in -> normalize -> normalized

[tasks]

task main:
    step run:
        action: call normalize(pressure_in) -> normalized
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let constraints = build_constraint_set(&program)
            .expect("causality 约束应允许引用 extern 函数和 topology 变量");

        assert_eq!(constraints.causality.len(), 1);
        assert_eq!(
            constraints.causality[0].devices,
            vec!["pressure_in", "normalize", "normalized"]
        );
    }

    #[test]
    fn reports_undefined_device_in_and_or_wait_conditions() {
        let input = r#"
[topology]

device sensor_A: sensor
device sensor_C: sensor

[constraints]

[tasks]

task main:
    step wait_combo:
        wait: sensor_A == true AND sensor_B == true
        wait: sensor_C == true OR sensor_D == true
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let errors = build_constraint_set(&program).expect_err("AND/OR wait 的未定义设备应报错");

        let rendered = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("未定义设备 sensor_B"),
            "应报告 AND 子条件中的未定义设备"
        );
        assert!(
            rendered.contains("未定义设备 sensor_D"),
            "应报告 OR 子条件中的未定义设备"
        );
    }

    #[test]
    fn reports_invalid_analog_thresholds_in_safety() {
        let input = r#"
[topology]

device pressure_ok: analog_input { range: 0..10 }
device pressure_missing: analog_input
device button: digital_input

[constraints]

safety: pressure_ok > 11 conflicts_with button.on
safety: pressure_missing > 5 conflicts_with button.on
safety: button > 1 conflicts_with button.on

[tasks]

task main:
    step start:
        wait: button == true
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let errors = build_constraint_set(&program).expect_err("无效阈值比较应报错");

        let rendered = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("pressure_ok") && rendered.contains("超出"),
            "应报告阈值超出范围"
        );
        assert!(
            rendered.contains("pressure_missing") && rendered.contains("缺少 range"),
            "应报告缺少 range 的模拟量输入"
        );
        assert!(
            rendered.contains("期望 analog_input"),
            "应报告非 analog_input 的阈值比较"
        );
    }

    #[test]
    fn reports_invalid_analog_thresholds_in_wait_conditions() {
        let input = r#"
[topology]

device temp_ok: analog_input { range: 0..100 }
device temp_missing: analog_input
device start_button: digital_input

[constraints]

[tasks]

task main:
    step check:
        wait: temp_ok > 120
        wait: temp_missing < 10
        wait: start_button > 1
        wait: start_button == true
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let errors = build_constraint_set(&program).expect_err("无效 wait 阈值比较应报错");

        let rendered = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("temp_ok") && rendered.contains("超出"),
            "应报告 wait 阈值超出范围"
        );
        assert!(
            rendered.contains("temp_missing") && rendered.contains("缺少 range"),
            "应报告 wait 条件缺少 range 的模拟量输入"
        );
        assert!(
            rendered.contains("期望 analog_input"),
            "应报告 wait 条件使用非 analog_input 设备"
        );
    }

    #[test]
    fn reports_unit_mismatch_for_analog_thresholds() {
        let input = r#"
[topology]

device pressure: analog_input { range: 0..10, unit: "bar" }
device button: digital_input

[constraints]

safety: pressure > 5psi conflicts_with button.on

[tasks]

task main:
    step check:
        wait: pressure > 5psi
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let errors = build_constraint_set(&program).expect_err("单位不一致应报错");
        let rendered = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("单位不一致") && rendered.contains("bar") && rendered.contains("psi"),
            "应报告阈值比较单位不一致"
        );
    }

    #[test]
    fn accepts_unit_matched_analog_thresholds() {
        let input = r#"
[topology]

device pressure: analog_input { range: 0..10, unit: "bar" }
device button: digital_input

[constraints]

safety: pressure > 5bar conflicts_with button.on

[tasks]

task main:
    step check:
        wait: pressure > 5bar
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let constraints = build_constraint_set(&program).expect("单位一致的阈值比较应通过语义检查");
        assert_eq!(constraints.safety.len(), 1);
    }

    #[test]
    fn accepts_cam_following_error_threshold_with_device_port_target() {
        let input = r#"
[topology]

device encoder_main: analog_input { range: 0..360 }
device servo_cmd: analog_output { range: 0..360 }
device cam_xy: cam_coupling {
    master: encoder_main,
    slave: servo_cmd,
    table: cam_a,
}
cam_table cam_a: periodic [
    (0, 0),
    (180, 100),
    (360, 0),
]

[constraints]

safety: cam_xy.fault.on conflicts_with cam_xy.engage.on
safety: cam_xy.following_error > 2 conflicts_with cam_xy.in_sync.on

[tasks]

task main:
    step s1:
        action: cam_engage cam_xy
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let constraints = build_constraint_set(&program).expect("cam 端口阈值应通过约束构建");
        assert_eq!(constraints.safety.len(), 2);
        assert!(matches!(
            constraints.safety[1].left,
            crate::ir::SafetyExpr::Threshold { .. }
        ));
    }

    #[test]
    fn rejects_non_analog_cam_port_threshold_target() {
        let input = r#"
[topology]

device encoder_main: analog_input { range: 0..360 }
device servo_cmd: analog_output { range: 0..360 }
device cam_xy: cam_coupling {
    master: encoder_main,
    slave: servo_cmd,
    table: cam_a,
}
cam_table cam_a: periodic [
    (0, 0),
    (180, 100),
    (360, 0),
]

[constraints]

safety: cam_xy.engage > 1 conflicts_with cam_xy.fault.on

[tasks]

task main:
    step s1:
        action: cam_engage cam_xy
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let errors = build_constraint_set(&program).expect_err("数字阈值不应作用于数字端口");
        let rendered = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("analog 端口"),
            "应报告阈值目标必须是模拟量端口"
        );
    }

    #[test]
    fn rejects_non_whitelisted_set_enum_value_before_lowering() {
        let input = r#"
[topology]

device Y0: digital_output

[constraints]

[tasks]

task main:
    step run:
        action: set Y0 diagonal
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let constraints_errors =
            build_constraint_set(&program).expect_err("非法 set 枚举值应在约束构建阶段报错");
        let rendered_constraints = constraints_errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered_constraints.contains("on/off/forward/reverse/active/idle"),
            "应报告 set 枚举值白名单错误"
        );

        let state_machine_errors =
            build_state_machine(&program).expect_err("非法 set 枚举值应在 lowering 前被拦截");
        let rendered_state_machine = state_machine_errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered_state_machine.contains("on/off/forward/reverse/active/idle"),
            "状态机构建阶段也应返回同样的白名单错误"
        );
    }

    #[test]
    fn maps_set_enum_values_to_binary_ir_values() {
        let input = r#"
[topology]

device motor_dir: digital_output

[constraints]

[tasks]

task drive:
    step forward:
        action: set motor_dir forward
    step reverse:
        action: set motor_dir reverse
    step active:
        action: set motor_dir active
    step idle:
        action: set motor_dir idle
    on_complete: goto done

task done:
    step halt:
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let state_machine = build_state_machine(&program).expect("枚举状态应能成功 lowering");

        let forward_is_on = state_machine.transitions.iter().any(|transition| {
            transition.from.task_name == "drive"
                && transition.from.step_name == "forward"
                && transition.actions.iter().any(|action| {
                    matches!(
                        action,
                        crate::ir::TransitionAction::Set {
                            value: crate::ir::BinaryValue::On,
                            ..
                        }
                    )
                })
        });
        let reverse_is_off = state_machine.transitions.iter().any(|transition| {
            transition.from.task_name == "drive"
                && transition.from.step_name == "reverse"
                && transition.actions.iter().any(|action| {
                    matches!(
                        action,
                        crate::ir::TransitionAction::Set {
                            value: crate::ir::BinaryValue::Off,
                            ..
                        }
                    )
                })
        });
        let active_is_on = state_machine.transitions.iter().any(|transition| {
            transition.from.task_name == "drive"
                && transition.from.step_name == "active"
                && transition.actions.iter().any(|action| {
                    matches!(
                        action,
                        crate::ir::TransitionAction::Set {
                            value: crate::ir::BinaryValue::On,
                            ..
                        }
                    )
                })
        });
        let idle_is_off = state_machine.transitions.iter().any(|transition| {
            transition.from.task_name == "drive"
                && transition.from.step_name == "idle"
                && transition.actions.iter().any(|action| {
                    matches!(
                        action,
                        crate::ir::TransitionAction::Set {
                            value: crate::ir::BinaryValue::Off,
                            ..
                        }
                    )
                })
        });

        assert!(forward_is_on, "forward 应映射为 IR on");
        assert!(reverse_is_off, "reverse 应映射为 IR off");
        assert!(active_is_on, "active 应映射为 IR on");
        assert!(idle_is_off, "idle 应映射为 IR off");
    }

    #[test]
    fn rejects_legacy_motor_shorthand_in_action_and_state_refs() {
        let input = r#"
[topology]

device motor_x: motor
device alarm: sensor

[constraints]

safety: motor_x.on conflicts_with alarm.on

[tasks]

task main:
    step run:
        action: set motor_x on
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");

        let constraints_errors =
            build_constraint_set(&program).expect_err("legacy motor 状态引用应被拒绝");
        let rendered_constraints = constraints_errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered_constraints.contains("显式端口"),
            "应提示迁移到显式端口写法"
        );

        let state_machine_errors =
            build_state_machine(&program).expect_err("legacy motor set 写法应被拒绝");
        let rendered_state_machine = state_machine_errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered_state_machine.contains("set motor_x on 旧写法已废弃"),
            "应提示 set motor_x on 已废弃"
        );
    }

    #[test]
    fn supports_new_motor_family_device_types_in_topology_ir() {
        let input = r#"
[topology]

device stepper_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default }
device vfd_main: vfd
device servo_y: servo_drive { model_ref: servo_generic, config_ref: servo_default }

[constraints]

[tasks]

task main:
    step idle:
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let topology = build_topology_graph(&program).expect("应能构建拓扑图");

        let kinds = topology
            .graph
            .node_indices()
            .map(|idx| {
                (
                    topology.graph[idx].name.clone(),
                    topology.graph[idx].kind.clone(),
                )
            })
            .collect::<std::collections::HashMap<_, _>>();

        assert!(matches!(
            kinds.get("stepper_x"),
            Some(DeviceKind::StepperMotor)
        ));
        assert!(matches!(kinds.get("vfd_main"), Some(DeviceKind::Vfd)));
        assert!(matches!(kinds.get("servo_y"), Some(DeviceKind::ServoDrive)));
    }

    #[test]
    fn maps_analog_wait_conditions_to_region_predicates() {
        let input = r#"
[topology]

device AI0: analog_input { range: 0..10 }

[constraints]

[tasks]

task main:
    step wait_pressure:
        wait: AI0 > 5
    step done:
        action: log "ok"
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let state_machine = build_state_machine(&program).expect("应能构建状态机");

        let has_region_guard = state_machine.transitions.iter().any(|transition| {
            matches!(
                transition.guard,
                TransitionGuard::Condition { ref expression }
                    if expression.contains("AI0") && expression.contains("region_")
            )
        });
        assert!(has_region_guard, "模拟量 wait 应映射为 region 谓词表达式");
    }

    #[test]
    fn lowers_expression_wait_conditions_to_guard_expression() {
        let input = r#"
[topology]
variable master_pos: float = 0.0
variable slave_pos: float = 0.0

[constraints]

[tasks]
task main:
    step wait_sync:
        wait: abs(master_pos - slave_pos) < 0.5
    step done:
        action: log "ok"
"#;

        let program = parse_plc(input).expect("表达式 wait 示例应能解析");
        let state_machine = build_state_machine(&program).expect("表达式 wait 应能构建状态机");
        let has_expr_guard = state_machine.transitions.iter().any(|transition| {
            matches!(
                transition.guard,
                TransitionGuard::Condition { ref expression }
                    if expression.contains("abs(") && expression.contains("< 0.5")
            )
        });
        assert!(has_expr_guard, "表达式 wait 应保留为 guard 表达式");
    }

