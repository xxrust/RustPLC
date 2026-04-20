#[cfg(test)]
mod tests {
    use super::{
        SafetyConfig, SafetyModel, SafetyProofLevel, SafetyRuleStatusKind, analog_state_for_value,
        initial_concrete_state, verify_safety, verify_safety_with_config,
    };
    use crate::ir::{SafetyExpr, SafetyRelation, SafetyRule, StateExpr};
    use crate::parser::parse_plc;
    use crate::semantic::{build_constraint_set, build_state_machine};

    #[test]
    fn proves_sequential_cylinder_sequence_without_parallel_conflict() {
        let source = r#"
[topology]

device Y0: digital_output
device Y1: digital_output

device valve_A: solenoid_valve {
    response_time: 20ms
}

device valve_B: solenoid_valve {
    response_time: 20ms
}

device cyl_A: cylinder {
    stroke_time: 300ms
    retract_time: 300ms
}

device cyl_B: cylinder {
    stroke_time: 300ms
    retract_time: 300ms
}

relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: Y1.out, to: valve_B.coil, via: driven_by }
relation { from: valve_B.out, to: cyl_B.cmd, via: driven_by }

[constraints]

safety: cyl_A.extended conflicts_with cyl_B.extended

[tasks]

task init:
    step extend_A:
        action: extend cyl_A
    step retract_A:
        action: retract cyl_A
    step extend_B:
        action: extend cyl_B
    step retract_B:
        action: retract cyl_B
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let report = verify_safety(&program, &constraints, &state_machine)
            .expect("顺序双气缸逻辑不应违反互斥约束");

        assert!(
            matches!(
                report.level,
                SafetyProofLevel::Complete | SafetyProofLevel::Bounded
            ),
            "验证结果应返回有效级别"
        );
        assert!(report.explored_depth >= state_machine.states.len());
    }

    #[test]
    fn reports_conflict_for_parallel_extend_actions() {
        let source = r#"
[topology]

device Y0: digital_output
device Y1: digital_output

device valve_A: solenoid_valve
device valve_B: solenoid_valve

device cyl_A: cylinder {
    stroke_time: 200ms
    retract_time: 200ms
}

device cyl_B: cylinder {
    stroke_time: 200ms
    retract_time: 200ms
}

relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: Y1.out, to: valve_B.coil, via: driven_by }
relation { from: valve_B.out, to: cyl_B.cmd, via: driven_by }

[constraints]

safety: cyl_A.extended conflicts_with cyl_B.extended

[tasks]

task parallel_demo:
    step move_together:
        parallel:
            branch_A:
                action: extend cyl_A
            branch_B:
                action: extend cyl_B
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let errors = verify_safety(&program, &constraints, &state_machine)
            .expect_err("并行伸出冲突气缸时应触发 safety 错误");

        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("conflicts_with")),
            "错误应包含冲突约束说明"
        );
        assert!(errors.iter().all(|error| error.line > 0), "错误应携带行号");
    }

    #[test]
    fn uses_scc_size_plus_one_as_default_depth_floor() {
        let source = r#"
[topology]

device Y0: digital_output
device valve_A: solenoid_valve
device cyl_A: cylinder {
    stroke_time: 100ms
    retract_time: 100ms
}

device Y1: digital_output
device valve_B: solenoid_valve
device cyl_B: cylinder {
    stroke_time: 100ms
    retract_time: 100ms
}

relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: Y1.out, to: valve_B.coil, via: driven_by }
relation { from: valve_B.out, to: cyl_B.cmd, via: driven_by }

[constraints]

safety: cyl_A.extended conflicts_with cyl_B.extended

[tasks]

task init:
    step a:
        action: retract cyl_A
    on_complete: goto loop

task loop:
    step b:
        action: retract cyl_B
    on_complete: goto init
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let report = verify_safety(&program, &constraints, &state_machine)
            .expect("不含冲突动作时 safety 应通过");

        assert!(
            report.explored_depth >= 3,
            "SCC(2节点) 场景默认深度应至少为 |SCC|+1=3"
        );
    }

    #[test]
    fn warns_when_bmc_max_depth_caps_default_search_depth() {
        let source = r#"
[topology]

device Y0: digital_output
device valve_A: solenoid_valve
device cyl_A: cylinder {
    stroke_time: 100ms
    retract_time: 100ms
}

device Y1: digital_output
device valve_B: solenoid_valve
device cyl_B: cylinder {
    stroke_time: 100ms
    retract_time: 100ms
}

relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: Y1.out, to: valve_B.coil, via: driven_by }
relation { from: valve_B.out, to: cyl_B.cmd, via: driven_by }

[constraints]

safety: cyl_A.extended conflicts_with cyl_B.extended

[tasks]

task init:
    step one:
        action: retract cyl_A
    step two:
        action: retract cyl_B
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let report = verify_safety_with_config(
            &program,
            &constraints,
            &state_machine,
            SafetyConfig {
                bmc_max_depth: Some(1),
            },
        )
        .expect("应返回有界验证结果");

        assert_eq!(report.explored_depth, 1);
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("bmc_max_depth=1")),
            "当用户上限截断默认展开深度时应输出警告"
        );
    }

    #[test]
    fn warns_when_bmc_limit_is_lower_than_scc_requirement() {
        let source = r#"
[topology]

device Y0: digital_output
device valve_A: solenoid_valve
device cyl_A: cylinder {
    stroke_time: 100ms
    retract_time: 100ms
}

device Y1: digital_output
device valve_B: solenoid_valve
device cyl_B: cylinder {
    stroke_time: 100ms
    retract_time: 100ms
}

relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: Y1.out, to: valve_B.coil, via: driven_by }
relation { from: valve_B.out, to: cyl_B.cmd, via: driven_by }

[constraints]

safety: cyl_A.extended conflicts_with cyl_B.extended

[tasks]

task init:
    step a:
        action: retract cyl_A
    on_complete: goto loop

task loop:
    step b:
        action: retract cyl_B
    on_complete: goto init
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let report = verify_safety_with_config(
            &program,
            &constraints,
            &state_machine,
            SafetyConfig {
                bmc_max_depth: Some(2),
            },
        )
        .expect("应返回有界验证结果");

        assert_eq!(report.explored_depth, 2);
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("SCC")),
            "bmc_max_depth 小于 |SCC|+1 时应输出 SCC 截断警告"
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("WARNING: Safety 在深度 2 内未发现反例")),
            "截断后应输出有界验证警告"
        );
    }

    #[test]
    fn reports_requires_violation_when_press_extends_without_clamp() {
        let source = r#"
[topology]

device Y0: digital_output
device Y1: digital_output

device valve_clamp: solenoid_valve
device valve_press: solenoid_valve

device cyl_clamp: cylinder {
    stroke_time: 120ms
    retract_time: 120ms
}

device cyl_press: cylinder {
    stroke_time: 140ms
    retract_time: 140ms
}

relation { from: Y0.out, to: valve_clamp.coil, via: driven_by }
relation { from: valve_clamp.out, to: cyl_clamp.cmd, via: driven_by }
relation { from: Y1.out, to: valve_press.coil, via: driven_by }
relation { from: valve_press.out, to: cyl_press.cmd, via: driven_by }

[constraints]

safety: cyl_press.extended requires cyl_clamp.extended

[tasks]

task press:
    step press_down:
        action: extend cyl_press
    step retract_press:
        action: retract cyl_press
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let errors = verify_safety(&program, &constraints, &state_machine)
            .expect_err("未夹紧即下压时应触发 requires 违反");

        assert!(
            errors
                .iter()
                .any(|error| error.constraint.contains("requires")),
            "错误应包含 requires 约束文本"
        );
        assert!(
            errors
                .iter()
                .any(|error| error.reason.contains("未满足") || error.reason.contains("不为真")),
            "错误原因应说明 requires 前置条件未满足"
        );
        assert!(
            errors
                .iter()
                .any(|error| !error.violation_path.is_empty() && error.line > 0),
            "requires 错误应包含路径和位置"
        );
    }

    #[test]
    fn passes_requires_constraint_when_clamp_precedes_press() {
        let source = r#"
[topology]

device Y0: digital_output
device Y1: digital_output

device valve_clamp: solenoid_valve
device valve_press: solenoid_valve

device cyl_clamp: cylinder {
    stroke_time: 120ms
    retract_time: 120ms
}

device cyl_press: cylinder {
    stroke_time: 140ms
    retract_time: 140ms
}

relation { from: Y0.out, to: valve_clamp.coil, via: driven_by }
relation { from: valve_clamp.out, to: cyl_clamp.cmd, via: driven_by }
relation { from: Y1.out, to: valve_press.coil, via: driven_by }
relation { from: valve_press.out, to: cyl_press.cmd, via: driven_by }

[constraints]

safety: cyl_press.extended requires cyl_clamp.extended

[tasks]

task press:
    step clamp:
        action: extend cyl_clamp
    step press_down:
        action: extend cyl_press
    step retract_press:
        action: retract cyl_press
    step release_clamp:
        action: retract cyl_clamp
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let report = verify_safety(&program, &constraints, &state_machine)
            .expect("先夹紧后下压应满足 requires 约束");

        assert!(
            matches!(
                report.level,
                SafetyProofLevel::Complete | SafetyProofLevel::Bounded
            ),
            "requires 满足场景应通过 safety"
        );
    }

    #[test]
    fn reports_conflict_when_independent_tasks_overlap_on_conflicting_outputs() {
        let source = r#"
[topology]

device out_a: digital_output
device out_b: digital_output

[constraints]

safety: out_a.on conflicts_with out_b.on

[tasks]

task load:
    step set_a:
        action: set out_a on
    step hold_a:
        action: log "load"

task unload:
    step set_b:
        action: set out_b on
    step hold_b:
        action: log "unload"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let errors = verify_safety(&program, &constraints, &state_machine)
            .expect_err("独立 task 并发命中冲突资源时应触发 safety 失败");
        assert!(
            errors.iter().any(|error| error
                .constraint
                .contains("out_a.on conflicts_with out_b.on")),
            "错误应包含跨 task 冲突约束文本"
        );
    }

    #[test]
    fn reports_requires_violation_when_independent_tasks_overlap_without_prerequisite() {
        let source = r#"
[topology]

device clamp: digital_output
device press: digital_output

[constraints]

safety: press.on requires clamp.on

[tasks]

task clamp_task:
    step idle:
        action: log "clamp idle"

task press_task:
    step press_down:
        action: set press on
    step hold:
        action: log "press hold"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let errors = verify_safety(&program, &constraints, &state_machine)
            .expect_err("并发 task 中 prerequisite 缺失时应触发 requires 失败");
        assert!(
            errors
                .iter()
                .any(|error| error.constraint.contains("press.on requires clamp.on")),
            "错误应包含 requires 约束文本"
        );
    }

    #[test]
    fn passes_when_independent_tasks_operate_on_disjoint_resources() {
        let source = r#"
[topology]

device out_a: digital_output
device out_b: digital_output
device out_c: digital_output

[constraints]

safety: out_a.on conflicts_with out_b.on

[tasks]

task load:
    step set_a:
        action: set out_a on
    step hold_a:
        action: log "load"

task inspect:
    step set_c:
        action: set out_c on
    step hold_c:
        action: log "inspect"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let report = verify_safety(&program, &constraints, &state_machine)
            .expect("并发 task 操作互不冲突资源时应通过 safety");
        assert!(
            matches!(
                report.level,
                SafetyProofLevel::Complete | SafetyProofLevel::Bounded
            ),
            "应返回有效 proof level"
        );
    }

    #[test]
    fn models_pending_action_status_in_concurrent_global_state() {
        let source = r#"
[topology]

device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default, motion_param_set: stepper_default_fast }
device out_a: digital_output
device AI0: analog_input { range: 0..10 }

[constraints]

safety: AI0 > 6 conflicts_with AI0 < 2

[tasks]

task motion:
    step move:
        action: axis.move_relative(axis_x, distance: 5, speed: 10)
            timeout: 100ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
    step done:
        action: log "done"

task io:
    step set_a:
        action: set out_a on
    step hold:
        action: log "hold"

task fault:
    step timeout:
        action: log "timeout"
    step reject:
        action: log "reject"
    step motion_fault:
        action: log "motion"
    step safety_fault:
        action: log "safety"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");
        let model = SafetyModel::from_inputs(&program, &constraints, &state_machine);
        let concrete = initial_concrete_state(&model);

        assert!(
            concrete.task_pending.iter().any(|pending| *pending),
            "并发全局状态应携带 task 级 pending action 标记"
        );
    }

    #[test]
    fn reports_rule_statuses_and_coverage_for_all_bound_rules() {
        let source = r#"
[topology]

device out_a: digital_output
device out_b: digital_output

[constraints]

safety: out_a.on conflicts_with out_b.on
safety: out_a.on requires out_a.on

[tasks]

task main:
    step s1:
        action: log "tick"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let report = verify_safety(&program, &constraints, &state_machine)
            .expect("无动作变更场景不应违反安全约束");

        assert_eq!(report.coverage.total_rules, 2);
        assert_eq!(report.coverage.bound_rules, 2);
        assert_eq!(report.coverage.degraded_rules, 0);
        assert_eq!(report.coverage.skipped_rules, 0);
        assert_eq!(report.rule_statuses.len(), 2);
        assert!(
            report
                .rule_statuses
                .iter()
                .all(|status| matches!(status.status, SafetyRuleStatusKind::Bound))
        );
        assert!(report.rule_statuses.iter().all(|s| s.reason.is_none()));
    }

    #[test]
    fn reports_rule_statuses_and_coverage_with_skipped_rule() {
        let source = r#"
[topology]

device out_a: digital_output
device out_b: digital_output

[constraints]

safety: out_a.on conflicts_with out_b.on

[tasks]

task main:
    step s1:
        action: log "tick"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let mut constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        constraints.safety.push(SafetyRule {
            left: SafetyExpr::State(StateExpr {
                device: "unknown_device".to_string(),
                state: "on".to_string(),
                port: String::new(),
            }),
            relation: SafetyRelation::ConflictsWith,
            right: SafetyExpr::State(StateExpr {
                device: "out_a".to_string(),
                state: "on".to_string(),
                port: String::new(),
            }),
            reason: None,
            source: None,
        });

        let report = verify_safety(&program, &constraints, &state_machine)
            .expect("跳过绑定失败规则时应仍返回可用安全报告");

        assert_eq!(report.coverage.total_rules, 2);
        assert_eq!(report.coverage.bound_rules, 1);
        assert_eq!(report.coverage.skipped_rules, 1);
        assert!(
            report
                .rule_statuses
                .iter()
                .any(|status| matches!(status.status, SafetyRuleStatusKind::Skipped))
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("已跳过")),
            "跳过规则时应输出可读告警"
        );
    }

    #[test]
    fn reports_rule_statuses_and_coverage_when_all_rules_skipped() {
        let source = r#"
[topology]

device out_a: digital_output

[constraints]

[tasks]

task main:
    step s1:
        action: log "tick"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let mut constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        constraints.safety.push(SafetyRule {
            left: SafetyExpr::State(StateExpr {
                device: "unknown_device".to_string(),
                state: "on".to_string(),
                port: String::new(),
            }),
            relation: SafetyRelation::ConflictsWith,
            right: SafetyExpr::State(StateExpr {
                device: "out_a".to_string(),
                state: "on".to_string(),
                port: String::new(),
            }),
            reason: None,
            source: None,
        });
        constraints.safety.push(SafetyRule {
            left: SafetyExpr::State(StateExpr {
                device: "unknown_device_2".to_string(),
                state: "on".to_string(),
                port: String::new(),
            }),
            relation: SafetyRelation::Requires,
            right: SafetyExpr::State(StateExpr {
                device: "out_a".to_string(),
                state: "on".to_string(),
                port: String::new(),
            }),
            reason: None,
            source: None,
        });

        let report = verify_safety(&program, &constraints, &state_machine)
            .expect("全部规则跳过时仍应返回可用安全报告");

        assert_eq!(report.coverage.total_rules, 2);
        assert_eq!(report.coverage.bound_rules, 0);
        assert_eq!(report.coverage.degraded_rules, 0);
        assert_eq!(report.coverage.skipped_rules, 2);
        assert!(
            report
                .rule_statuses
                .iter()
                .all(|status| matches!(status.status, SafetyRuleStatusKind::Skipped))
        );
    }

    #[test]
    fn handles_and_or_wait_guards_in_bmc_state_exploration() {
        let source = r#"
[topology]

device Y0: digital_output
device Y1: digital_output
device X0: digital_input
device X1: digital_input

device valve_A: solenoid_valve
device valve_B: solenoid_valve

device cyl_A: cylinder {
    stroke_time: 120ms
    retract_time: 120ms
}

device cyl_B: cylinder {
    stroke_time: 120ms
    retract_time: 120ms
}

device sensor_A_ext: sensor
device sensor_B_ext: sensor

relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: Y1.out, to: valve_B.coil, via: driven_by }
relation { from: valve_B.out, to: cyl_B.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_A_ext.sense, via: detects }
relation { from: sensor_A_ext.out, to: X0.in, via: reports_to }
relation { from: cyl_B.extended, to: sensor_B_ext.sense, via: detects }
relation { from: sensor_B_ext.out, to: X1.in, via: reports_to }

[constraints]

safety: cyl_A.extended conflicts_with cyl_B.extended

[tasks]

task main:
    step move_a:
        action: extend cyl_A
        wait: sensor_A_ext == true AND sensor_B_ext == true
    step return_a:
        action: retract cyl_A
        wait: sensor_A_ext == true OR sensor_B_ext == true
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let report = verify_safety(&program, &constraints, &state_machine)
            .expect("AND/OR wait 守卫不应导致 safety BMC 崩溃或误报");

        assert!(
            matches!(
                report.level,
                SafetyProofLevel::Complete | SafetyProofLevel::Bounded
            ),
            "含 AND/OR wait 的场景应得到有效 safety 结论"
        );
    }

    #[test]
    fn models_analog_threshold_rules_with_region_abstraction() {
        let source = r#"
[topology]

device pressure_sensor: analog_input { range: 0..100, unit: "bar" }

device Y0: digital_output
device valve_A: solenoid_valve
device cyl_A: cylinder {
    stroke_time: 120ms
    retract_time: 120ms
}

relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }

[constraints]

safety: pressure_sensor > 50 conflicts_with pressure_sensor < 10

[tasks]

task demo:
    step extend:
        action: extend cyl_A
    step retract:
        action: retract cyl_A
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let report = verify_safety(&program, &constraints, &state_machine)
            .expect("含模拟量阈值规则的场景应返回可用 safety 结果");

        assert!(
            report
                .warnings
                .iter()
                .all(|warning| !warning.contains("阈值") && !warning.contains("未建模")),
            "阈值规则已纳入离散抽象时不应输出跳过告警"
        );
        assert!(
            matches!(
                report.level,
                SafetyProofLevel::Complete | SafetyProofLevel::Bounded
            ),
            "阈值规则纳入建模后应产生有效证明等级"
        );
    }

    #[test]
    fn reports_analog_threshold_split_points_and_hit_intervals() {
        let source = r#"
[topology]

device pressure_sensor: analog_input { range: 0..100, unit: "bar" }

[constraints]

safety: pressure_sensor > 50 conflicts_with pressure_sensor < 10

[tasks]

task main:
    step s1:
        action: log "tick"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let report = verify_safety(&program, &constraints, &state_machine)
            .expect("含模拟量阈值规则的场景应返回可用 safety 结果");

        assert_eq!(report.rule_statuses.len(), 1);
        let status = &report.rule_statuses[0];
        assert!(matches!(status.status, SafetyRuleStatusKind::Degraded));
        assert!(
            status.reason.as_deref().unwrap_or("").contains("区间离散"),
            "阈值抽象应标注为降级原因"
        );
        assert_eq!(status.analog_thresholds.len(), 2);
        for detail in &status.analog_thresholds {
            assert_eq!(detail.split_points, vec![0.0, 10.0, 50.0, 100.0]);
            assert_eq!(detail.total_intervals, 3);
            assert_eq!(detail.hit_intervals, 1);
        }
    }

    #[test]
    fn detects_conflict_for_overlapping_analog_thresholds() {
        let source = r#"
[topology]

device AI0: analog_input { range: 0..100 }

[constraints]

safety: AI0 > 50 conflicts_with AI0 > 60

[tasks]

task main:
    step s1:
        action: log "tick"
    step s2:
        action: log "tick"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let errors = verify_safety(&program, &constraints, &state_machine)
            .expect_err("重叠的模拟量阈值应触发冲突");

        assert!(
            errors
                .iter()
                .any(|error| error.constraint.contains("AI0 > 50")),
            "错误应包含模拟量阈值冲突描述"
        );
    }

    #[test]
    fn detects_cross_port_conflict_for_stepper_enable_and_pulse() {
        let source = r#"
[topology]

device axis_x: stepper_motor

[constraints]

safety: axis_x.enable.off conflicts_with axis_x.pulse.active

[tasks]

task main:
    step disable_axis:
        action: set axis_x.enable off
    step pulse_axis:
        action: set axis_x.pulse active
    step done:
        action: log "done"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let errors = verify_safety(&program, &constraints, &state_machine)
            .expect_err("enable.off 与 pulse.active 组合应触发冲突");

        assert!(
            errors
                .iter()
                .any(|error| error.constraint.contains("axis_x.enable.off")),
            "错误应包含端口化安全约束文本"
        );
        assert!(
            errors
                .iter()
                .any(|error| error.constraint.contains("axis_x.pulse.active")),
            "错误应包含 pulse 端口状态"
        );
    }

    #[test]
    fn axis_move_matches_stepper_enable_pulse_interlock() {
        let source = r#"
[topology]

device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default, motion_param_set: stepper_default_fast }

[constraints]

safety: axis_x.enable.off conflicts_with axis_x.pulse.active

[tasks]

task main:
    step move:
        action: axis.move_relative(axis_x, distance: 5, speed: 10)
            timeout: 100ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
    step done:
        action: log "done"
task fault:
    step timeout:
        action: log "timeout"
    step reject:
        action: log "reject"
    step motion_fault:
        action: log "motion"
    step safety_fault:
        action: log "safety"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let errors = verify_safety(&program, &constraints, &state_machine)
            .expect_err("axis move 应命中互锁约束");

        assert!(
            errors
                .iter()
                .any(|error| error.constraint.contains("axis_x.enable.off")),
            "错误应包含 enable 端口约束"
        );
        assert!(
            errors
                .iter()
                .any(|error| error.constraint.contains("axis_x.pulse.active")),
            "错误应包含 pulse.active 端口约束"
        );
    }

    #[test]
    fn axis_move_passes_enable_pulse_interlock_after_enable_on() {
        let source = r#"
[topology]

device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default, motion_param_set: stepper_default_fast }

[constraints]

safety: axis_x.enable.off conflicts_with axis_x.pulse.active

[tasks]

task main:
    step enable_axis:
        action: set axis_x.enable on
    step move:
        action: axis.move_relative(axis_x, distance: 5, speed: 10)
            timeout: 100ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
    step done:
        action: log "done"
task fault:
    step timeout:
        action: log "timeout"
    step reject:
        action: log "reject"
    step motion_fault:
        action: log "motion"
    step safety_fault:
        action: log "safety"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let report = verify_safety(&program, &constraints, &state_machine)
            .expect("先 enable 再 axis move 应满足互锁约束");
        assert!(
            report
                .rule_statuses
                .iter()
                .any(|status| status.rule.contains("axis_x.enable.off")
                    && status.rule.contains("axis_x.pulse.active")),
            "规则状态应包含 axis 互锁约束"
        );
    }

    #[test]
    fn rejects_vertical_axis_disable_without_brake_confirmation_preflight() {
        let source = r#"
[topology]
device axis_z: stepper_motor { model_ref: stepper_generic, config_ref: stepper_vertical_brake }

[constraints]

[tasks]
task fault:
    step stop_now:
        action: set axis_z.enable off
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let diagnostics = verify_safety(
            &program,
            &crate::ir::ConstraintSet::default(),
            &crate::ir::StateMachine::default(),
        )
        .expect_err("未确认抱闸直接 disable 应触发 safety 预检失败");

        let rendered = diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("[AXIS-012]"));
        assert!(rendered.contains("brake_engage_confirmed"));
    }

    #[test]
    fn accepts_vertical_axis_disable_after_brake_confirmation_preflight() {
        let source = r#"
[topology]
device axis_z: stepper_motor { model_ref: stepper_generic, config_ref: stepper_vertical_brake }

[constraints]

[tasks]
task fault:
    step safe_stop:
        action: set axis_z.brake_cmd on
        wait: axis_z.brake_engaged == true
        action: set axis_z.enable off
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        verify_safety(
            &program,
            &crate::ir::ConstraintSet::default(),
            &crate::ir::StateMachine::default(),
        )
        .expect("先抱闸确认再 disable 应通过 safety 预检");
    }

    #[test]
    fn models_cam_following_error_threshold_on_port_domain() {
        let source = r#"
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

safety: cam_xy.following_error > 2 conflicts_with cam_xy.in_sync.on

[tasks]

task main:
    step run:
        action: cam_engage cam_xy
        wait: cam_xy.in_sync == true
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");
        let report = verify_safety(&program, &constraints, &state_machine)
            .expect("cam following_error 阈值应可建模并返回 safety 结果");

        assert_eq!(report.rule_statuses.len(), 1);
        assert_eq!(report.rule_statuses[0].analog_thresholds.len(), 1);
        let detail = &report.rule_statuses[0].analog_thresholds[0];
        assert_eq!(detail.device, "cam_xy.following_error");
        assert!(
            detail.split_points.contains(&2.0),
            "阈值分割点应包含 following_error 阈值"
        );
    }

    #[test]
    fn validates_cam_fault_interlock_rule_on_cam_ports() {
        let source = r#"
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

safety: cam_xy.fault.on conflicts_with cam_xy.engage.off

[tasks]

task main:
    step force_fault:
        action: set cam_xy.fault on
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        let report = verify_safety(&program, &constraints, &state_machine)
            .expect("cam fault 互锁规则应完成绑定并参与验证");
        assert_eq!(report.rule_statuses.len(), 1);
        assert!(
            report.rule_statuses[0].rule.contains("cam_xy.fault.on")
                && report.rule_statuses[0].rule.contains("cam_xy.engage.off"),
            "规则文本应包含 cam fault 互锁约束"
        );
    }

    #[test]
    fn respects_compute_driven_boolean_condition_in_safety_reachability() {
        let source = r#"
[topology]

variable choose_a: bool = false
device out_a: digital_output
device out_b: digital_output

[constraints]

safety: out_a.on conflicts_with out_b.on

[tasks]

task main:
    step seed:
        action: compute choose_a = true
    step maybe_a:
        if: choose_a == true goto set_a.run else: goto main.maybe_b
    step maybe_b:
        if: choose_a == false goto set_b.run else: goto main.done
    step done:
        action: log "done"

task set_a:
    step run:
        action: set out_a on
    on_complete: goto main.maybe_b

task set_b:
    step run:
        action: set out_b on
    on_complete: goto main.done
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        verify_safety(&program, &constraints, &state_machine)
            .expect("compute 写入的布尔变量应裁剪 if 分支，避免双输出冲突假阳性");
    }

    #[test]
    fn respects_compute_arithmetic_condition_in_safety_reachability() {
        let source = r#"
[topology]

variable count: float = 0.0
device out_a: digital_output
device out_b: digital_output

[constraints]

safety: out_a.on conflicts_with out_b.on

[tasks]

task main:
    step inc:
        action: compute count = count + 1.0
    step maybe_a:
        if: count > 0.0 goto set_a.run else: goto main.maybe_b
    step maybe_b:
        if: count <= 0.0 goto set_b.run else: goto main.done
    step done:
        action: log "done"

task set_a:
    step run:
        action: set out_a on
    on_complete: goto main.maybe_b

task set_b:
    step run:
        action: set out_b on
    on_complete: goto main.done
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        verify_safety(&program, &constraints, &state_machine)
            .expect("compute 算术结果应进入 condition 求值，避免 count>0 路径误放大");
    }

    #[test]
    fn respects_sequential_compute_effect_order_in_safety_reachability() {
        let source = r#"
[topology]

variable a: float = 0.0
variable b: float = 0.0
device out_a: digital_output
device out_b: digital_output

[constraints]

safety: out_a.on conflicts_with out_b.on

[tasks]

task main:
    step seed:
        action: compute a = 1.0
        action: compute b = a + 1.0
    step arm_a:
        action: set out_a on
    step maybe_b:
        if: b <= 1.5 goto set_b.run else: goto main.done
    step done:
        action: log "done"

task set_b:
    step run:
        action: set out_b on
    on_complete: goto main.done
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        verify_safety(&program, &constraints, &state_machine)
            .expect("同一 transition 内 compute 应按源码顺序生效，避免后续表达式继续读取旧变量值");
    }

    #[test]
    fn respects_runtime_supported_function_guards_in_safety_reachability() {
        let source = r#"
[topology]

device out_a: digital_output
device out_b: digital_output

[constraints]

safety: out_a.on conflicts_with out_b.on

[tasks]

task main:
    step arm_a:
        action: set out_a on
    step maybe_b:
        if: abs(-1.0) < 0.5 goto set_b.run else: goto main.done
    step done:
        action: log "done"

task set_b:
    step run:
        action: set out_b on
    on_complete: goto main.done
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        verify_safety(&program, &constraints, &state_machine)
            .expect("runtime 已支持的函数 guard 应进入 safety 求值，避免 unsupported guard 继续放大假路径");
    }

    #[test]
    fn respects_pure_extern_results_in_safety_reachability() {
        let source = r#"
[topology]

extern function add(a: float, b: float) -> float {
    rust_module: "math::basic"
    pure: true
    time_bound_us: 1000
}

variable sum: float = 0.0
device out_a: digital_output
device out_b: digital_output

[constraints]

safety: out_a.on conflicts_with out_b.on

[tasks]

task main:
    step seed:
        action: call add(1.0, 2.0) -> sum
    step arm_a:
        action: set out_a on
    step maybe_b:
        if: sum < 2.5 goto set_b.run else: goto main.done
    step done:
        action: log "done"

task set_b:
    step run:
        action: set out_b on
    on_complete: goto main.done
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");

        verify_safety(&program, &constraints, &state_machine)
            .expect("pure extern 返回值应进入 safety 变量状态，避免把已确定的分支继续放大");
    }

    #[test]
    fn maps_set_analog_to_region_state() {
        let source = r#"
[topology]

device AO0: analog_output { range: 0..10 }
device Y0: digital_output
device valve: solenoid_valve
relation { from: Y0.out, to: valve.coil, via: driven_by }
resource analog_band: semantic_resource { mode: exclusive }

[constraints]

claim: AO0.region_0 occupies analog_band

[tasks]

task main:
    step set:
        action: set_analog AO0 7.5
    step done:
        action: log "done"
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");
        let model = SafetyModel::from_inputs(&program, &constraints, &state_machine);

        let device_id = *model
            .device_index
            .get(&("AO0".to_string(), "self".to_string()))
            .expect("AO0 应注册为设备");
        let target_state =
            analog_state_for_value(&model.devices, device_id, "7.5").expect("应找到区间状态");

        let has_effect = model
            .edges
            .iter()
            .any(|edge| edge.effects.get(&device_id).copied() == Some(target_state));

        assert!(has_effect, "set_analog 应映射到对应区间状态");
    }

    #[test]
    fn tracks_only_constraint_relevant_device_domains() {
        let source = r#"
[topology]

device mode_auto: sensor
device mode_manual: sensor
device irrelevant_sensor: sensor
device irrelevant_output: digital_output

[constraints]

safety: mode_auto.on conflicts_with mode_manual.on

[tasks]

task main:
    step idle:
        action: set irrelevant_output on
"#;

        let program = parse_plc(source).expect("测试程序应能解析");
        let constraints = build_constraint_set(&program).expect("约束应能构建");
        let state_machine = build_state_machine(&program).expect("状态机应能构建");
        let model = SafetyModel::from_inputs(&program, &constraints, &state_machine);

        assert!(
            model
                .device_index
                .contains_key(&("mode_auto".to_string(), "self".to_string())),
            "安全约束引用的设备应进入 safety model"
        );
        assert!(
            model
                .device_index
                .contains_key(&("mode_manual".to_string(), "self".to_string())),
            "安全约束引用的设备应进入 safety model"
        );
        assert!(
            !model
                .device_index
                .contains_key(&("irrelevant_sensor".to_string(), "self".to_string())),
            "与 safety/resource claim 无关的设备不应膨胀 safety 状态空间"
        );
        assert!(
            !model
                .device_index
                .contains_key(&("irrelevant_output".to_string(), "self".to_string())),
            "仅被普通 action 使用、但不参与 safety/resource claim 的设备不应被跟踪"
        );
    }
}
