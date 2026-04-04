    #[test]
    fn lowers_axis_move_actions_into_ir_transition_actions() {
        let input = r#"
[topology]
device axis_x: stepper_motor {
    model_ref: stepper_generic
    config_ref: stepper_default
    motion_param_set: stepper_default_fast
}

[constraints]

[tasks]
task motion:
    step run:
        action: axis.move_relative(axis_x, distance: 10, speed: 2)
            timeout: 500ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
        action: axis.move_absolute(axis_x, position: 120, speed: 5)
            timeout: 800ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
    on_complete: goto done

task fault:
    step timeout:
    step reject:
    step motion_fault:
    step safety_fault:

task done:
    step idle:
"#;

        let program = parse_plc(input).expect("axis move 示例应能解析");
        let sm = build_state_machine(&program).expect("axis move 应能 lowering 到 IR");

        let actions = sm
            .transitions
            .iter()
            .flat_map(|transition| transition.actions.iter())
            .collect::<Vec<_>>();

        let relative = actions
            .iter()
            .find_map(|action| match action {
                crate::ir::TransitionAction::AxisMoveRelative {
                    target,
                    distance_raw,
                    speed_raw,
                    timeout,
                    on_reject,
                    on_motion_fault,
                    on_safety_fault,
                    ..
                } => Some((
                    target,
                    distance_raw,
                    speed_raw,
                    timeout,
                    on_reject,
                    on_motion_fault,
                    on_safety_fault,
                )),
                _ => None,
            })
            .expect("应包含 axis_move_relative 动作");
        assert_eq!(relative.0, "axis_x");
        assert_eq!(relative.1, "10");
        assert_eq!(relative.2, "2");
        assert_eq!(relative.3.duration_ms, 500);
        assert_eq!(relative.3.target_task, "fault");
        assert_eq!(relative.3.target_step.as_deref(), Some("timeout"));
        assert_eq!(relative.4.target_task, "fault");
        assert_eq!(relative.4.target_step.as_deref(), Some("reject"));
        assert!(relative.4.error_code.is_none());
        assert_eq!(relative.5.target_step.as_deref(), Some("motion_fault"));
        assert_eq!(relative.6.target_step.as_deref(), Some("safety_fault"));

        let absolute = actions
            .iter()
            .find_map(|action| match action {
                crate::ir::TransitionAction::AxisMoveAbsolute {
                    target,
                    position_raw,
                    speed_raw,
                    timeout,
                    ..
                } => Some((target, position_raw, speed_raw, timeout)),
                _ => None,
            })
            .expect("应包含 axis_move_absolute 动作");
        assert_eq!(absolute.0, "axis_x");
        assert_eq!(absolute.1, "120");
        assert_eq!(absolute.2, "5");
        assert_eq!(absolute.3.duration_ms, 800);
        assert_eq!(absolute.3.target_step.as_deref(), Some("timeout"));
    }

    #[test]
    fn lowers_axis_move_refined_fault_routes_into_ir_transition_actions() {
        let input = r#"
[topology]
device axis_x: stepper_motor {
    model_ref: stepper_generic
    config_ref: stepper_default
    motion_param_set: stepper_default_fast
}

[constraints]

[tasks]
task motion:
    step run:
        action: axis.move_relative(axis_x, distance: 10, speed: 2)
            timeout: 500ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_default
            on_motion_fault(kind: vendor) -> fault.motion_vendor
            on_motion_fault(code: 17) -> fault.motion_code_17
            on_safety_fault -> fault.safety_fault
    on_complete: goto done

task fault:
    step timeout:
    step reject:
    step motion_default:
    step motion_vendor:
    step motion_code_17:
    step safety_fault:

task done:
    step idle:
"#;

        let program = parse_plc(input).expect("axis refined routes 示例应能解析");
        let sm = build_state_machine(&program).expect("axis refined routes 应能 lowering 到 IR");

        let action = sm
            .transitions
            .iter()
            .flat_map(|transition| transition.actions.iter())
            .find_map(|action| match action {
                crate::ir::TransitionAction::AxisMoveRelative {
                    on_motion_fault,
                    on_motion_fault_routes,
                    ..
                } => Some((on_motion_fault, on_motion_fault_routes)),
                _ => None,
            })
            .expect("应包含 axis_move_relative 动作");

        assert_eq!(action.0.target_step.as_deref(), Some("motion_default"));
        assert_eq!(action.1.len(), 2);
        assert_eq!(
            action.1[0].kind,
            Some(crate::ir::AxisFaultRouteKind::Vendor)
        );
        assert_eq!(action.1[0].code, None);
        assert_eq!(action.1[0].target_step.as_deref(), Some("motion_vendor"));

        assert_eq!(action.1[1].kind, None);
        assert_eq!(action.1[1].code, Some(17));
        assert_eq!(action.1[1].target_step.as_deref(), Some("motion_code_17"));
    }

    #[test]
    fn keeps_axis_move_absolute_homing_guard_when_not_statically_proven() {
        let input = r#"
[topology]
device axis_x: stepper_motor {
    model_ref: stepper_generic
    config_ref: stepper_default
    motion_param_set: stepper_default_fast
}

[constraints]

[tasks]
task motion:
    step run:
        action: axis.move_absolute(axis_x, position: 120, speed: 5)
            timeout: 800ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
    on_complete: goto done
task fault:
    step timeout:
    step reject:
    step motion_fault:
    step safety_fault:
task done:
    step idle:
"#;

        let program = parse_plc(input).expect("axis absolute 示例应能解析");
        let sm = build_state_machine(&program).expect("axis absolute 应能 lowering 到 IR");
        let require_homed = sm
            .transitions
            .iter()
            .flat_map(|transition| transition.actions.iter())
            .find_map(|action| match action {
                crate::ir::TransitionAction::AxisMoveAbsolute { require_homed, .. } => {
                    Some(*require_homed)
                }
                _ => None,
            })
            .expect("应包含 axis_move_absolute 动作");

        assert!(require_homed, "未被静态证明时应保留 runtime homing guard");
    }

    #[test]
    fn elides_axis_move_absolute_homing_guard_after_proven_relative() {
        let input = r#"
[topology]
device axis_x: stepper_motor {
    model_ref: stepper_generic
    config_ref: stepper_default
    motion_param_set: stepper_default_fast
}

[constraints]

[tasks]
task motion:
    step home:
        action: axis.move_relative(axis_x, distance: 10, speed: 2)
            timeout: 500ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
    step run:
        action: axis.move_absolute(axis_x, position: 120, speed: 5)
            timeout: 800ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
    on_complete: goto done
task fault:
    step timeout:
    step reject:
    step motion_fault:
    step safety_fault:
task done:
    step idle:
"#;

        let program = parse_plc(input).expect("axis relative+absolute 示例应能解析");
        let sm = build_state_machine(&program).expect("应能 lowering 到 IR");
        let require_homed = sm
            .transitions
            .iter()
            .filter(|transition| transition.from.step_name == "run")
            .flat_map(|transition| transition.actions.iter())
            .find_map(|action| match action {
                crate::ir::TransitionAction::AxisMoveAbsolute { require_homed, .. } => {
                    Some(*require_homed)
                }
                _ => None,
            })
            .expect("run step 应包含 axis_move_absolute 动作");

        assert!(!require_homed, "可静态证明时应消解 runtime homing guard");
    }

    #[test]
    fn lowers_compute_boolean_literals_to_numeric_ir_expression() {
        let input = r#"
[topology]
variable flag: bool = false

[constraints]

[tasks]
task main:
    step run:
        action: compute flag = true
        action: compute flag = false
    on_complete: goto done

task done:
    step idle:
"#;

        let program = parse_plc(input).expect("bool compute 示例应能解析");
        let sm = build_state_machine(&program).expect("bool compute 应能 lowering 到 IR");

        let compute_exprs = sm
            .transitions
            .iter()
            .flat_map(|transition| transition.actions.iter())
            .filter_map(|action| match action {
                crate::ir::TransitionAction::Compute { target, expr_raw } if target == "flag" => {
                    Some(expr_raw.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        assert!(
            compute_exprs.iter().any(|expr| expr == "true"),
            "true 应保留为布尔表达式字面量 true，实际: {compute_exprs:?}"
        );
        assert!(
            compute_exprs.iter().any(|expr| expr == "false"),
            "false 应保留为布尔表达式字面量 false，实际: {compute_exprs:?}"
        );
    }

    #[test]
    fn rejects_compute_type_mismatch_between_bool_target_and_numeric_expression() {
        let input = r#"
[topology]
variable flag: bool = false
variable x: float = 1.0

[constraints]

[tasks]
task main:
    step run:
        action: compute flag = x + 1
"#;

        let program = parse_plc(input).expect("示例语法应可解析");
        let errors = build_state_machine(&program).expect_err("bool 目标 + 数值表达式应报错");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("compute 表达式类型必须与目标变量类型一致"),
            "应报告 compute 目标/表达式类型不匹配"
        );
    }

    #[test]
    fn lowers_compute_boolean_logical_expression() {
        let input = r#"
[topology]
variable flag: bool = false
variable a: bool = false
variable b: bool = true
variable x: float = 0.0

[constraints]

[tasks]
task main:
    step run:
        action: compute flag = NOT a OR (b AND x > 0)
    on_complete: goto done

task done:
    step idle:
"#;

        let program = parse_plc(input).expect("示例语法应可解析");
        let sm = build_state_machine(&program).expect("合法布尔表达式应能 lowering");
        let compute_expr = sm
            .transitions
            .iter()
            .flat_map(|transition| transition.actions.iter())
            .find_map(|action| match action {
                crate::ir::TransitionAction::Compute { target, expr_raw } if target == "flag" => {
                    Some(expr_raw.clone())
                }
                _ => None,
            })
            .expect("应包含 compute flag 动作");
        assert!(compute_expr.contains("NOT"), "应保留 NOT");
        assert!(compute_expr.contains("OR"), "应保留 OR");
        assert!(compute_expr.contains("AND"), "应保留 AND");
        assert!(compute_expr.contains(">"), "应保留比较运算");
    }

    #[test]
    fn rejects_extern_call_with_wrong_argument_count_and_reports_line() {
        let input = "[topology]
variable lhs: float = 1.0
variable rhs: float = 2.0
variable out: float = 0.0
extern function add(a: float, b: float) -> float {
    rust_module: \"math::add\"
    pure: true
    time_bound_us: 100
}

[constraints]

[tasks]
task main:
    step run:
        action: call add(lhs) -> out
";

        let program = parse_plc(input).expect("示例语法应可解析");
        let errors = build_state_machine(&program).expect_err("参数个数错误应报语义错误");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("extern 函数 add 参数个数错误"),
            "应提示 extern 函数名和参数个数错误，实际: {joined}"
        );
        assert!(
            errors.iter().any(|err| err.line() == 15),
            "错误应定位到调用所在 step 行"
        );
    }

    #[test]
    fn rejects_extern_call_with_argument_type_mismatch() {
        let input = "[topology]
variable enabled: bool = true
variable rhs: float = 2.0
variable out: float = 0.0
extern function add(a: float, b: float) -> float {
    rust_module: \"math::add\"
    pure: true
    time_bound_us: 100
}

[constraints]

[tasks]
task main:
    step run:
        action: call add(enabled, rhs) -> out
";

        let program = parse_plc(input).expect("示例语法应可解析");
        let errors = build_state_machine(&program).expect_err("参数类型不匹配应报语义错误");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("extern 调用 add 参数 #1 类型不匹配"),
            "应提示 extern 函数参数类型不匹配，实际: {joined}"
        );
    }

    #[test]
    fn rejects_extern_call_with_return_binding_arity_mismatch() {
        let input = "[topology]
variable value: float = 1.0
variable out: float = 0.0
extern function split(v: float) -> (float, float) {
    rust_module: \"math::split\"
    pure: true
    time_bound_us: 100
}

[constraints]

[tasks]
task main:
    step run:
        action: call split(value) -> out
";

        let program = parse_plc(input).expect("示例语法应可解析");
        let errors = build_state_machine(&program).expect_err("返回绑定数量不匹配应报语义错误");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("extern 函数 split 返回值绑定数量错误"),
            "应提示 extern 函数返回值绑定数量错误，实际: {joined}"
        );
    }

    #[test]
    fn rejects_extern_call_with_return_binding_type_mismatch() {
        let input = "[topology]
variable trigger: bool = true
variable out: float = 0.0
extern function is_ready(trigger: bool) -> bool {
    rust_module: \"logic::is_ready\"
    pure: true
    time_bound_us: 80
}

[constraints]

[tasks]
task main:
    step run:
        action: call is_ready(trigger) -> out
";

        let program = parse_plc(input).expect("示例语法应可解析");
        let errors = build_state_machine(&program).expect_err("返回绑定类型不匹配应报语义错误");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("extern 调用 is_ready 返回绑定 #1 (out) 类型不匹配"),
            "应提示 extern 函数返回绑定类型不匹配，实际: {joined}"
        );
    }

    #[test]
    fn rejects_duplicate_extern_function_names_during_semantic_analysis() {
        let input = "[topology]
extern function add(a: float, b: float) -> float {
    rust_module: \"math::add\"
    pure: true
    time_bound_us: 100
}
extern function add(v: float) -> float {
    rust_module: \"math::add_alt\"
    pure: true
    time_bound_us: 120
}

[constraints]

[tasks]
task main:
    step run:
        action: log \"ok\"
";

        let program = parse_plc(input).expect("示例语法应可解析");
        let errors = build_state_machine(&program).expect_err("重复 extern 函数名应报语义错误");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("重复定义extern 函数 add"),
            "应报告重复 extern 函数定义，实际: {joined}"
        );
        assert!(
            errors.iter().any(|err| err.line() == 7),
            "错误应定位到重复声明所在行"
        );
    }

    #[test]
    fn rejects_extern_function_with_zero_time_bound() {
        let input = "[topology]
extern function add(a: float, b: float) -> float {
    rust_module: \"math::add\"
    pure: true
    time_bound_us: 0
}

[constraints]

[tasks]
task main:
    step run:
        action: log \"ok\"
";

        let program = parse_plc(input).expect("示例语法应可解析");
        let errors = build_state_machine(&program).expect_err("time_bound_us 为 0 应报语义错误");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("time_bound_us 必须为正整数"),
            "应提示 time_bound_us 需大于 0，实际: {joined}"
        );
        assert!(
            errors.iter().any(|err| err.line() == 2),
            "错误应定位到 extern 声明所在行"
        );
    }

    #[test]
    fn rejects_extern_function_with_empty_rust_module() {
        let input = "[topology]
extern function add(a: float, b: float) -> float {
    rust_module: \"   \"
    pure: true
    time_bound_us: 10
}

[constraints]

[tasks]
task main:
    step run:
        action: log \"ok\"
";

        let program = parse_plc(input).expect("示例语法应可解析");
        let errors = build_state_machine(&program).expect_err("空 rust_module 应报语义错误");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("rust_module 不能为空"),
            "应提示 rust_module 不能为空，实际: {joined}"
        );
        assert!(
            errors.iter().any(|err| err.line() == 2),
            "错误应定位到 extern 声明所在行"
        );
    }

    #[test]
    fn rejects_non_pure_extern_used_in_parallel_branches() {
        let input = "[topology]
variable e1: float = 0.1
variable e2: float = 0.2
variable kp: float = 1.0
variable ki: float = 0.1
variable kd: float = 0.01
variable dt: float = 0.1
variable out1: float = 0.0
variable out2: float = 0.0
extern function pid_update(error: float, kp: float, ki: float, kd: float, dt: float) -> float {
    rust_module: \"control::pid\"
    pure: false
    time_bound_us: 200
}

[constraints]

[tasks]
task main:
    step run:
        parallel:
            branch_a:
                action: call pid_update(e1, kp, ki, kd, dt) -> out1
            branch_b:
                action: call pid_update(e2, kp, ki, kd, dt) -> out2
";

        let program = parse_plc(input).expect("示例语法应可解析");
        let errors =
            build_state_machine(&program).expect_err("parallel 多分支 non-pure extern 应报错");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("non-pure extern 函数 pid_update"),
            "应报告 non-pure extern 并发调用风险，实际: {joined}"
        );
    }

    #[test]
    fn allows_pure_extern_used_in_parallel_branches() {
        let input = "[topology]
variable x: float = 1.0
variable y: float = 2.0
variable out1: float = 0.0
variable out2: float = 0.0
extern function add(a: float, b: float) -> float {
    rust_module: \"math::add\"
    pure: true
    time_bound_us: 50
}

[constraints]

[tasks]
task main:
    step run:
        parallel:
            branch_a:
                action: call add(x, y) -> out1
            branch_b:
                action: call add(y, x) -> out2
";

        let program = parse_plc(input).expect("示例语法应可解析");
        build_state_machine(&program).expect("pure extern 在并行分支中应允许");
    }

    #[test]
    fn rejects_axis_move_missing_branches_with_axis_rule_codes() {
        let input = "[topology]
device axis_x: stepper_motor {
    model_ref: stepper_generic
    config_ref: stepper_default
    motion_param_set: stepper_default_fast
}

[constraints]

[tasks]
task main:
    step move:
        action: axis.move_relative(axis_x, distance: 10, speed: 2)
";

        let program = parse_plc(input).expect("axis move 语法应可解析");
        let errors = build_state_machine(&program).expect_err("缺失分支应触发 AXIS 语义错误");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(joined.contains("[AXIS-001]"));
        assert!(joined.contains("[AXIS-002]"));
        assert!(joined.contains("[AXIS-003]"));
        assert!(joined.contains("[AXIS-004]"));
        assert!(joined.contains("step 'move'"));
        assert!(joined.contains("timeout: <duration> -> <task.step>"));
        assert!(errors.iter().all(|err| err.line() > 0));
    }

    #[test]
    fn rejects_axis_move_refined_route_without_primary_bucket() {
        let input = "[topology]
device axis_x: stepper_motor {
    model_ref: stepper_generic
    config_ref: stepper_default
    motion_param_set: stepper_default_fast
}

[constraints]

[tasks]
task motion:
    step run:
        action: axis.move_relative(axis_x, distance: 10, speed: 2)
            timeout: 500ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault(kind: vendor) -> fault.motion_vendor
            on_safety_fault -> fault.safety_fault
task fault:
    step timeout:
    step reject:
    step motion_vendor:
    step safety_fault:
";

        let program = parse_plc(input).expect("语法应可解析");
        let errors = build_state_machine(&program).expect_err("缺失主桶分支应触发 AXIS-003");
        let joined = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("[AXIS-003]"));
    }

    #[test]
    fn rejects_axis_move_refined_route_with_incompatible_bucket_kind() {
        let input = "[topology]
device axis_x: stepper_motor {
    model_ref: stepper_generic
    config_ref: stepper_default
    motion_param_set: stepper_default_fast
}

[constraints]

[tasks]
task motion:
    step run:
        action: axis.move_relative(axis_x, distance: 10, speed: 2)
            timeout: 500ms -> fault.timeout
            on_reject -> fault.reject
            on_reject(kind: safety) -> fault.bad_reject_route
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
task fault:
    step timeout:
    step reject:
    step bad_reject_route:
    step motion_fault:
    step safety_fault:
";

        let program = parse_plc(input).expect("语法应可解析");
        let errors = build_state_machine(&program).expect_err("不兼容 kind 应触发 AXIS-010");
        let joined = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("[AXIS-010]"));
    }

    #[test]
    fn rejects_axis_move_target_that_is_not_stepper_or_servo() {
        let input = "[topology]
device conveyor_motor: motor

[constraints]

[tasks]
task motion:
    step run:
        action: axis.move_absolute(conveyor_motor, position: 120, speed: 5)
            timeout: 800ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
task fault:
    step timeout:
    step reject:
    step motion_fault:
    step safety_fault:
";

        let program = parse_plc(input).expect("axis move 语法应可解析");
        let errors = build_state_machine(&program).expect_err("非法轴目标应触发 AXIS-005");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(joined.contains("[AXIS-005]"));
        assert!(joined.contains("axis target 'conveyor_motor'"));
        assert!(joined.contains("step 'run'"));
        assert!(errors.iter().any(|err| err.line() > 0));
    }

    #[test]
    fn accepts_axis_move_with_complete_branches_on_stepper_target() {
        let input = "[topology]
device axis_x: stepper_motor {
    model_ref: stepper_generic
    config_ref: stepper_default
    motion_param_set: stepper_default_fast
}

[constraints]

[tasks]
task motion:
    step run:
        action: axis.move_relative(axis_x, distance: 10, speed: 2)
            timeout: 500ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
task fault:
    step timeout:
    step reject:
    step motion_fault:
    step safety_fault:
";

        let program = parse_plc(input).expect("axis move 语法应可解析");
        build_state_machine(&program).expect("完整 axis move 分支 + 合法目标应通过语义校验");
    }

    #[test]
    fn resolves_axis_move_params_reference_and_applies_inline_override_priority() {
        let input = "[topology]
device axis_x: stepper_motor {
    model_ref: stepper_generic
    config_ref: stepper_default
}

[constraints]

[tasks]
task motion:
    step run:
        action: axis.move_relative(axis_x, distance: 10, params: stepper_default_fast, speed: 1800)
            timeout: 500ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
    on_complete: goto done
task fault:
    step timeout:
    step reject:
    step motion_fault:
    step safety_fault:
task done:
    step idle:
";

        let program = parse_plc(input).expect("axis move 语法应可解析");
        let sm = build_state_machine(&program).expect("params 引用 + 覆盖应能通过语义校验");
        let speed_raw = sm
            .transitions
            .iter()
            .flat_map(|transition| transition.actions.iter())
            .find_map(|action| match action {
                crate::ir::TransitionAction::AxisMoveRelative { speed_raw, .. } => {
                    Some(speed_raw.as_str())
                }
                _ => None,
            })
            .expect("应包含 axis_move_relative 动作");
        assert_eq!(speed_raw, "1800");
    }

    #[test]
    fn rejects_axis_move_missing_acc_or_dec_without_params_reference() {
        let input = "[topology]
device axis_x: stepper_motor {
    model_ref: stepper_generic
    config_ref: stepper_default
}

[constraints]

[tasks]
task motion:
    step run:
        action: axis.move_relative(axis_x, distance: 10, speed: 1200)
            timeout: 500ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
task fault:
    step timeout:
    step reject:
    step motion_fault:
    step safety_fault:
";

        let program = parse_plc(input).expect("axis move 语法应可解析");
        let errors =
            build_state_machine(&program).expect_err("缺少 acc/dec 且无 params 应触发 AXIS-007");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("[AXIS-007]"));
    }

    #[test]
    fn rejects_axis_move_when_effective_params_exceed_axis_profile_limits() {
        let input = "[topology]
device axis_x: stepper_motor {
    model_ref: stepper_generic
    config_ref: stepper_default
}

[constraints]

[tasks]
task motion:
    step run:
        action: axis.move_relative(axis_x, distance: 10, params: stepper_default_fast, acc: 12000)
            timeout: 500ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
task fault:
    step timeout:
    step reject:
    step motion_fault:
    step safety_fault:
";

        let program = parse_plc(input).expect("axis move 语法应可解析");
        let errors = build_state_machine(&program).expect_err("超出 profile 上限应触发 AXIS-009");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("[AXIS-009]"));
    }

    #[test]
    fn rejects_axis_move_when_effective_params_are_non_positive() {
        let input = "[topology]
device axis_x: stepper_motor {
    model_ref: stepper_generic
    config_ref: stepper_default
}

[constraints]

[tasks]
task motion:
    step run:
        action: axis.move_relative(axis_x, distance: 10, speed: 1200, acc: 0, dec: 100)
            timeout: 500ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
task fault:
    step timeout:
    step reject:
    step motion_fault:
    step safety_fault:
";

        let program = parse_plc(input).expect("axis move 语法应可解析");
        let errors = build_state_machine(&program).expect_err("非正参数应触发 AXIS-008");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("[AXIS-008]"));
    }

    #[test]
    fn rejects_axis_move_absolute_when_position_exceeds_soft_limits() {
        let input = "[topology]
device axis_x: stepper_motor {
    model_ref: stepper_generic
    config_ref: stepper_soft_limited
}

[constraints]

[tasks]
task motion:
    step run:
        action: axis.move_absolute(axis_x, position: 800, speed: 1200, acc: 1000, dec: 1000)
            timeout: 500ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
task fault:
    step timeout:
    step reject:
    step motion_fault:
    step safety_fault:
";

        let program = parse_plc(input).expect("axis move 语法应可解析");
        let errors =
            build_state_machine(&program).expect_err("超出 soft limit 的绝对运动应触发 AXIS-011");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("[AXIS-011]"));
    }

    #[test]
    fn rejects_vertical_axis_disable_without_brake_confirmation() {
        let input = "[topology]
device axis_z: stepper_motor {
    model_ref: stepper_generic
    config_ref: stepper_vertical_brake
}

[constraints]

[tasks]
task fault:
    step stop_now:
        action: set axis_z.enable off
";

        let program = parse_plc(input).expect("垂直轴示例语法应可解析");
        let errors =
            build_state_machine(&program).expect_err("未确认抱闸直接 disable 应触发 AXIS-012");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("[AXIS-012]"));
        assert!(joined.contains("brake_engage_confirmed"));
    }

    #[test]
    fn accepts_vertical_axis_disable_after_brake_confirmation() {
        let input = "[topology]
device axis_z: stepper_motor {
    model_ref: stepper_generic
    config_ref: stepper_vertical_brake
}

[constraints]

[tasks]
task fault:
    step safe_stop:
        action: set axis_z.brake_cmd on
        wait: axis_z.brake_engaged == true
        action: set axis_z.enable off
";

        let program = parse_plc(input).expect("垂直轴示例语法应可解析");
        build_state_machine(&program).expect("先抱闸确认再 disable 应通过语义校验");
    }

    #[test]
    fn accepts_phase1_scalar_types_in_extern_signatures() {
        let input = "[topology]
variable state: bool = true
variable count: int = 1
variable next_state: bool = false
variable next_count: int = 0
extern function step_logic(flag: bool, value: int) -> (bool, int) {
    rust_module: \"logic::step\"
    pure: true
    time_bound_us: 20
}

[constraints]

[tasks]
task main:
    step run:
        action: call step_logic(state, count) -> (next_state, next_count)
";

        let program = parse_plc(input).expect("示例语法应可解析");
        build_state_machine(&program).expect("Phase 1 标量类型签名应通过语义检查");
    }

    #[test]
    fn lowers_axis_fault_contracts_into_topology_ir() {
        let input = "[topology]
device axis_x: stepper_motor {
    model_ref: stepper_generic
    config_ref: stepper_default
}
axis_fault_contract axis_x_fault {
    axis: axis_x
    severity: safety
    stop_mode: immediate
    auto_reset_policy: never
    manual_ack_required: true
    propagation_scope: self
}

[constraints]

[tasks]
task main:
    step idle:
";

        let program = parse_plc(input).expect("axis_fault_contract 语法应可解析");
        let topology = build_topology_graph(&program).expect("axis_fault_contract 应能降级到 IR");
        assert_eq!(topology.axis_fault_contracts.len(), 1);
        let contract = &topology.axis_fault_contracts[0];
        assert_eq!(contract.name, "axis_x_fault");
        assert_eq!(contract.axis, "axis_x");
        assert!(matches!(
            contract.severity,
            crate::ir::AxisFaultSeverity::Safety
        ));
        assert!(matches!(
            contract.stop_mode,
            crate::ir::AxisStopMode::Immediate
        ));
        assert!(matches!(
            contract.auto_reset_policy,
            crate::ir::AxisAutoResetPolicy::Never
        ));
        assert!(contract.manual_ack_required);
        assert!(matches!(
            contract.propagation_scope,
            crate::ir::AxisFaultPropagationScope::SelfOnly
        ));
        assert_eq!(contract.propagation_targets, vec!["axis_x".to_string()]);
    }

    #[test]
    fn rejects_axis_fault_contract_when_axis_is_not_motion_device() {
        let input = "[topology]
device valve_a: solenoid_valve
axis_fault_contract valve_fault {
    axis: valve_a
    severity: recoverable
    stop_mode: quick
    auto_reset_policy: on_clear
    manual_ack_required: false
    propagation_scope: self
}

[constraints]

[tasks]
task main:
    step idle:
";

        let program = parse_plc(input).expect("示例语法应可解析");
        let errors =
            build_topology_graph(&program).expect_err("非轴设备绑定 axis_fault_contract 应报错");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("axis_fault_contract 只能绑定到轴设备"));
    }

    #[test]
    fn lowers_axis_fault_followers_scope_into_resolved_targets() {
        let input = "[topology]
device axis_master: servo_drive {
    model_ref: servo_generic
    config_ref: servo_default
}
device axis_follower: servo_drive {
    model_ref: servo_generic
    config_ref: servo_default
}
device cam_link: cam_coupling {
    master: axis_master
    slave: axis_follower
    table: servo_cam_profile
}
cam_table servo_cam_profile: oneshot[(0.0, 0.0), (180.0, 180.0)]
axis_fault_contract master_fault {
    axis: axis_master
    severity: recoverable
    stop_mode: controlled
    auto_reset_policy: on_clear
    manual_ack_required: false
    propagation_scope: followers
}

[constraints]

[tasks]
task main:
    step idle:
";

        let program = parse_plc(input).expect("followers scope fixture should parse");
        let topology = build_topology_graph(&program).expect("followers scope should lower");
        let contract = topology
            .axis_fault_contracts
            .iter()
            .find(|contract| contract.name == "master_fault")
            .expect("contract should exist");
        assert!(matches!(
            contract.propagation_scope,
            crate::ir::AxisFaultPropagationScope::Followers
        ));
        assert_eq!(
            contract.propagation_targets,
            vec!["axis_master".to_string(), "axis_follower".to_string()]
        );
    }

    #[test]
    fn rejects_axis_fault_contract_custom_targets_with_non_axis_device() {
        let input = "[topology]
device axis_x: stepper_motor {
    model_ref: stepper_generic
    config_ref: stepper_default
}
device valve_a: solenoid_valve
axis_fault_contract axis_fault {
    axis: axis_x
    severity: recoverable
    stop_mode: controlled
    auto_reset_policy: never
    manual_ack_required: false
    propagation_scope: custom
    propagation_targets: [valve_a]
}

[constraints]

[tasks]
task main:
    step idle:
";

        let program = parse_plc(input).expect("custom targets fixture should parse");
        let errors =
            build_topology_graph(&program).expect_err("non-axis propagation target should fail");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("propagation_targets 只能包含轴设备"));
    }

    #[test]
    fn state_machine_populates_task_execution_contexts_with_timer_and_pending_metadata() {
        let input = "[topology]
variable input_value: float = 1.0
variable output_value: float = 0.0
extern function calc(value: float) -> float {
    rust_module: \"math::calc\"
    pure: false
    time_bound_us: 50
}

[constraints]

[tasks]
task loader:
    step invoke:
        action: call calc(input_value) -> output_value
        delay: 20ms
    on_complete: goto timeout

task watcher:
    step await_output:
        wait: output_value > 0.5
        timeout: 30ms -> goto timeout

task timeout:
    step halt:
";

        let program = parse_plc(input).expect("fixture should parse");
        let sm = build_state_machine(&program).expect("state machine should be built");

        assert_eq!(sm.task_contexts.len(), 3);

        let loader_ctx = sm
            .task_contexts
            .iter()
            .find(|ctx| ctx.task_name == "loader")
            .expect("loader context should exist");
        assert_eq!(loader_ctx.entry_state.step_name, "invoke");
        assert!(matches!(
            loader_ctx.blocking_state,
            TaskBlockingState::Ready
        ));
        assert!(
            loader_ctx
                .timers
                .iter()
                .any(|timer| timer.timer_name == "loader.invoke.delay_1")
        );
        assert!(loader_ctx.pending_actions.iter().any(|action| {
            action.action_kind == ActionKind::CallExtern
                && action.target.as_deref() == Some("calc")
                && action.source_state.task_name == "loader"
                && action.source_state.step_name == "invoke"
        }));

        let watcher_ctx = sm
            .task_contexts
            .iter()
            .find(|ctx| ctx.task_name == "watcher")
            .expect("watcher context should exist");
        assert_eq!(watcher_ctx.entry_state.step_name, "await_output");
        assert!(
            watcher_ctx
                .timers
                .iter()
                .any(|timer| timer.timer_name == "watcher.await_output.timeout_1")
        );
    }

    #[test]
    fn semantic_resource_duplicate_name_reports_sri_001() {
        let source = r#"
[topology]
resource slide_zone: semantic_resource { mode: exclusive }
resource slide_zone: semantic_resource { mode: exclusive }

[constraints]

[tasks]
task main:
    step idle:
"#;

        let program = parse_plc(source).expect("fixture should parse");
        let errors = build_constraint_set(&program).expect_err("duplicate resource should fail");
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("[SRI-001]"))
        );
    }

    #[test]
    fn semantic_resource_unknown_claim_target_reports_sri_002() {
        let source = r#"
[topology]
device cyl_feed: cylinder

[constraints]
claim: cyl_feed.extended occupies slide_zone

[tasks]
task main:
    step idle:
"#;

        let program = parse_plc(source).expect("fixture should parse");
        let errors =
            build_constraint_set(&program).expect_err("unknown resource claim should fail");
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("[SRI-002]"))
        );
    }

    #[test]
    fn semantic_resource_unknown_action_tag_reports_sri_003() {
        let source = r#"
[topology]
resource slide_zone: semantic_resource { mode: exclusive }

[constraints]
claim: action_tag arm_pick_to_slide occupies slide_zone

[tasks]
task main:
    step idle:
"#;

        let program = parse_plc(source).expect("fixture should parse");
        let errors =
            build_constraint_set(&program).expect_err("unknown action tag claim should fail");
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("[SRI-003]"))
        );
    }
