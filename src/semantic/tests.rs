#[cfg(test)]
mod tests {
    use super::{
        build_constraint_set, build_state_machine, build_timing_model, build_topology_graph,
        preprocess_program, preprocess_program_with_library,
    };
    use crate::device_library::DeviceLibrary;
    use crate::ir::{
        ActionKind, ConnectionType, DeviceKind, SafetyRelation, TaskBlockingState,
        TimerOperationKind, TimingRelation, TimingScope, TransitionGuard,
    };
    use crate::parser::parse_plc;
    use petgraph::visit::EdgeRef;
    use std::path::Path;

    #[test]
    fn preprocess_expands_plc_device_ports_into_internal_io_nodes() {
        let input = r#"
[topology]
device plc_main: plc { model_ref: openplc_softplc }
device valve_A: solenoid_valve { ports: [coil:digital:consumer] }
device start_button: sensor { ports: [out:digital:producer] }

relation { from: plc_main.Y0, to: valve_A.coil, via: driven_by }
relation { from: start_button.out, to: plc_main.X0, via: reports_to }

[constraints]

[tasks]
task main:
    step idle:
"#;

        let program = parse_plc(input).expect("parse");
        let expanded = preprocess_program(&program).expect("preprocess");
        assert!(
            !expanded
                .topology
                .devices
                .iter()
                .any(|d| matches!(d.device_type, crate::ast::DeviceType::Plc)),
            "plc 设备应在 preprocess 后降维"
        );
        assert!(
            expanded.topology.devices.iter().any(|d| d.name == "Y0"),
            "应生成 Y0 内部 IO 节点"
        );
        assert!(
            expanded.topology.devices.iter().any(|d| d.name == "X0"),
            "应生成 X0 内部 IO 节点"
        );

        let y0_edge_exists = expanded.topology.connections.iter().any(|c| {
            c.from == "Y0"
                && c.to == "valve_A"
                && c.from_port.is_none()
                && c.to_port.as_deref() == Some("coil")
        });
        assert!(y0_edge_exists, "plc_main.Y0 应改写为 Y0 -> valve_A.coil");
    }

    #[test]
    fn preprocess_rejects_inline_plc_port_inventory() {
        let input = r#"
[topology]
device plc_main: plc { ports: [Y0:digital:producer, X0:digital:consumer] }

[constraints]

[tasks]
task main:
    step idle:
"#;

        let program = parse_plc(input).expect("parse");
        let errors = preprocess_program(&program).expect_err("inline plc ports should be rejected");
        let rendered = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            rendered.contains("inline ports") && rendered.contains("model_ref"),
            "expected inline port inventory rejection, got: {rendered}"
        );
    }

    #[test]
    fn preprocess_rejects_plc_missing_model_ref() {
        let input = r#"
[topology]
device plc_main: plc

[constraints]

[tasks]
task main:
    step idle:
"#;

        let program = parse_plc(input).expect("parse");
        let errors = preprocess_program(&program).expect_err("plc without model_ref should fail");
        let rendered = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            rendered.contains("missing model_ref"),
            "expected missing model_ref error, got: {rendered}"
        );
    }

    #[test]
    fn preprocess_with_library_rejects_unknown_motor_extension_param_for_device_type() {
        let input = r#"
[topology]

device axis_x: stepper_motor {
    rated_power: 2.2kW,
    steps_per_rev: 200
}

[constraints]

[tasks]
task main:
    step idle:
"#;

        let program = parse_plc(input).expect("parse");
        let library = DeviceLibrary::load(Path::new("devices")).expect("load device library");

        let errors = preprocess_program_with_library(&program, Some(&library))
            .expect_err("stepper_motor 不应接受 rated_power 参数");
        let rendered = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            rendered.contains("rated_power") && rendered.contains("未在设备库 parameters 中声明"),
            "应报告参数未在设备库声明，实际: {rendered}"
        );
    }

    #[test]
    fn preprocess_with_library_rejects_invalid_typed_motor_extension_param() {
        let input = r#"
[topology]

device axis_x: stepper_motor {
    steps_per_rev: 200.5,
    accel_time: 120ms
}

[constraints]

[tasks]
task main:
    step idle:
"#;

        let program = parse_plc(input).expect("parse");
        let library = DeviceLibrary::load(Path::new("devices")).expect("load device library");

        let errors = preprocess_program_with_library(&program, Some(&library))
            .expect_err("steps_per_rev 应是 integer");
        let rendered = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            rendered.contains("steps_per_rev") && rendered.contains("integer"),
            "应报告参数类型错误，实际: {rendered}"
        );
    }

    #[test]
    fn preprocess_with_library_accepts_valid_typed_motor_extension_params() {
        let input = r#"
[topology]

device axis_x: stepper_motor {
    steps_per_rev: 200,
    max_speed: 5000,
    accel_time: 120ms,
    decel_time: 120ms,
    microstep: 16,
    gear_num: 5,
    gear_den: 2,
    lead_screw: 5mm,
    position_unit: mm,
    max_acceleration: 12000pps
}

device servo_x: servo_drive {
    microstep: 8,
    gear_num: 10,
    gear_den: 1,
    lead_screw: 2mm,
    position_unit: mm,
    max_acceleration: 3000rpm
}

[constraints]

[tasks]
task main:
    step idle:
"#;

        let program = parse_plc(input).expect("parse");
        let library = DeviceLibrary::load(Path::new("devices")).expect("load device library");
        preprocess_program_with_library(&program, Some(&library))
            .expect("合法参数应通过设备库类型校验");
    }

    #[test]
    fn preprocess_with_library_rejects_invalid_axis_param_type_with_line_context() {
        let input = r#"
[topology]

device axis_x: stepper_motor {
    microstep: 1.5,
    lead_screw: 5mm
}

[constraints]

[tasks]
task main:
    step idle:
"#;

        let program = parse_plc(input).expect("parse");
        let library = DeviceLibrary::load(Path::new("devices")).expect("load device library");
        let errors = preprocess_program_with_library(&program, Some(&library))
            .expect_err("microstep 应是 integer");
        let rendered = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            rendered.contains("microstep") && rendered.contains("integer"),
            "应报告 axis 参数类型错误，实际: {rendered}"
        );
        assert!(
            errors.iter().any(|e| e.line() > 0),
            "参数诊断应携带有效行号，实际: {rendered}"
        );
    }

    #[test]
    fn preprocess_with_library_rejects_invalid_axis_param_unit_and_enum() {
        let input = r#"
[topology]

device axis_x: stepper_motor {
    lead_screw: 5inch,
    position_unit: turns
}

[constraints]

[tasks]
task main:
    step idle:
"#;

        let program = parse_plc(input).expect("parse");
        let library = DeviceLibrary::load(Path::new("devices")).expect("load device library");
        let errors = preprocess_program_with_library(&program, Some(&library))
            .expect_err("lead_screw 单位和 position_unit 枚举值均应被拒绝");
        let rendered = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            rendered.contains("lead_screw") && rendered.contains("参数单位不匹配"),
            "应报告 axis 参数单位错误，实际: {rendered}"
        );
        assert!(
            rendered.contains("position_unit") && rendered.contains("参数类型要求 enum"),
            "应报告 axis 参数枚举错误，实际: {rendered}"
        );
        assert!(
            errors.iter().any(|e| e.line() > 0),
            "参数诊断应携带有效行号，实际: {rendered}"
        );
    }

    #[test]
    fn preprocess_with_library_accepts_number_params_with_expected_unit_suffix() {
        let input = r#"
[topology]

device motor_main: motor {
    rated_power: 2.2kW,
    rated_freq: 50Hz,
    accel_time: 0.8s
}

[constraints]

[tasks]
task main:
    step idle:
"#;

        let program = parse_plc(input).expect("parse");
        let library = DeviceLibrary::load(Path::new("devices")).expect("load device library");
        preprocess_program_with_library(&program, Some(&library))
            .expect("带单位后缀的 number 参数应通过校验");
    }

    #[test]
    fn preprocess_with_library_injects_cam_fault_interlock_constraint() {
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

[tasks]
task main:
    step idle:
        action: log "ok"
"#;

        let program = parse_plc(input).expect("parse");
        let library = DeviceLibrary::load(Path::new("devices")).expect("load device library");
        let expanded = preprocess_program_with_library(&program, Some(&library))
            .expect("cam device-library constraints should inject");

        assert!(
            expanded.constraints.safety.iter().any(|rule| {
                matches!(
                    (&rule.left, &rule.right),
                    (
                        crate::ast::SafetyOperand::State(left),
                        crate::ast::SafetyOperand::State(right)
                    ) if left.device == "cam_xy"
                        && left.port == "fault"
                        && left.state == "on"
                        && right.device == "cam_xy"
                        && right.port == "engage"
                        && right.state == "on"
                )
            }),
            "应注入 cam_coupling.toml 的 fault.on conflicts_with engage.on 约束"
        );
    }

    #[test]
    fn preprocess_rejects_plc_endpoint_without_explicit_port() {
        let input = r#"
[topology]
device plc_main: plc { model_ref: openplc_softplc }
device valve_A: solenoid_valve { ports: [coil:digital:consumer] }
relation { from: plc_main, to: valve_A.coil, via: driven_by }

[constraints]

[tasks]
task main:
    step idle:
"#;
        let program = parse_plc(input).expect("parse");
        let errors = preprocess_program(&program).expect_err("应报错");
        assert!(
            errors.iter().any(|e| e.to_string().contains("未指定端口")),
            "应提示 PLC 端点必须显式指定端口"
        );
    }

    #[test]
    fn builds_topology_graph_from_prd_5_3_topology() {
        let input = r#"
[topology]

# ===== controller ports =====
device Y0: digital_output
device Y1: digital_output
device Y2: digital_output
device X0: digital_input
device X1: digital_input
device X2: digital_input
device X3: digital_input
device X4: digital_input

# ===== operator panel =====
device start_button: sensor {
    debounce: 20ms
}

device alarm_light: motor

# ===== solenoid valves =====
device valve_A: solenoid_valve {
    subtype: "5/2",
    response_time: 15ms
}

device valve_B: solenoid_valve {
    subtype: "5/2",
    response_time: 15ms
}

# ===== cylinders =====
device cyl_A: cylinder {
    subtype: double_acting,
    stroke: 100mm,
    stroke_time: 200ms,
    retract_time: 180ms
}

device cyl_B: cylinder {
    subtype: double_acting,
    stroke: 150mm,
    stroke_time: 300ms,
    retract_time: 250ms
}

# ===== sensors =====
device sensor_A_ext: sensor {
    subtype: magnetic
}

device sensor_A_ret: sensor {
    subtype: magnetic
}

device sensor_B_ext: sensor {
    subtype: magnetic
}

device sensor_B_ret: sensor {
    subtype: magnetic
}

relation { from: start_button.out, to: X4.in, via: reports_to }
relation { from: Y2.out, to: alarm_light.cmd, via: driven_by }
relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_A_ext.sense, via: detects }
relation { from: sensor_A_ext.out, to: X0.in, via: reports_to }
relation { from: cyl_A.retracted, to: sensor_A_ret.sense, via: detects }
relation { from: sensor_A_ret.out, to: X1.in, via: reports_to }
relation { from: Y1.out, to: valve_B.coil, via: driven_by }
relation { from: valve_B.out, to: cyl_B.cmd, via: driven_by }
relation { from: cyl_B.extended, to: sensor_B_ext.sense, via: detects }
relation { from: sensor_B_ext.out, to: X2.in, via: reports_to }
relation { from: cyl_B.retracted, to: sensor_B_ret.sense, via: detects }
relation { from: sensor_B_ret.out, to: X3.in, via: reports_to }

[constraints]

[tasks]
"#;

        let program = parse_plc(input).expect("PRD 5.3 示例应能成功解析为 AST");
        let topology = build_topology_graph(&program).expect("PRD 5.3 示例应能成功构建拓扑图");

        assert_eq!(topology.graph.node_count(), 18);
        assert_eq!(topology.graph.edge_count(), 14);

        let has_pneumatic_edge = topology.graph.edge_references().any(|edge| {
            let source = &topology.graph[edge.source()].name;
            let target = &topology.graph[edge.target()].name;
            source == "valve_A" && target == "cyl_A" && edge.weight() == &ConnectionType::Pneumatic
        });
        assert!(has_pneumatic_edge, "应包含 valve_A -> cyl_A 气路连接");

        let has_electrical_edge = topology.graph.edge_references().any(|edge| {
            let source = &topology.graph[edge.source()].name;
            let target = &topology.graph[edge.target()].name;
            source == "Y0" && target == "valve_A" && edge.weight() == &ConnectionType::Electrical
        });
        assert!(has_electrical_edge, "应包含 Y0 -> valve_A 电气连接");

        let has_detects_edge = topology.graph.edge_references().any(|edge| {
            let source = &topology.graph[edge.source()].name;
            let target = &topology.graph[edge.target()].name;
            source == "cyl_A"
                && target == "sensor_A_ext"
                && edge.weight() == &ConnectionType::Logical
        });
        assert!(has_detects_edge, "应包含 cyl_A -> sensor_A_ext 检测连接");
    }

    #[test]
    fn topology_extracts_pid_loop_with_conditional_integration_strategy() {
        let input = r#"
[topology]
device AI0: analog_input { range: 0..100, unit: "bar" }
device AO0: analog_output { range: 0..100, unit: "%" }
device loop_pressure: pid {
    pv: AI0,
    sp: 50bar,
    kp: 2.0,
    ki: 0.4,
    kd: 0.05,
    out: AO0,
    period_ms: 100,
    limit: 0..100
}

[constraints]

[tasks]
task main:
    step hold:
"#;

        let program = parse_plc(input).expect("parse");
        let topology = build_topology_graph(&program).expect("build topology");
        assert_eq!(topology.pid_loops.len(), 1);
        let pid = &topology.pid_loops[0];
        assert_eq!(pid.name, "loop_pressure");
        assert_eq!(pid.pv, "AI0");
        assert_eq!(pid.out, "AO0");
        assert_eq!(pid.period_ms, 100);
        assert_eq!(pid.anti_windup, "conditional_integration");
    }

    #[test]
    fn reports_error_when_connected_to_references_undefined_device() {
        let input = r#"
[topology]
device Y0: digital_output

device valve_A: solenoid_valve {
    response_time: 15ms
}
relation { from: Y9.out, to: valve_A.coil, via: driven_by }

[constraints]

[tasks]
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let errors = build_topology_graph(&program).expect_err("未定义 connected_to 引用应报错");

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].line(), 5);
        assert!(
            errors[0].to_string().contains("未定义设备 Y9"),
            "错误消息应包含未定义设备名"
        );
    }

    #[test]
    fn reports_error_when_connection_types_are_incompatible() {
        let input = r#"
[topology]
device cyl_A: cylinder { stroke_time: 200ms, retract_time: 180ms }

device valve_A: solenoid_valve {
    response_time: 15ms
}

device sensor_bad: sensor

device Y0: digital_output

relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_bad.sense, via: driven_by }

[constraints]

[tasks]
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let errors = build_topology_graph(&program).expect_err("不兼容连接类型应报错");

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].line(), 9);
        assert!(
            errors[0].to_string().contains("sensor") && errors[0].to_string().contains("cylinder"),
            "错误消息应包含不兼容的设备类型"
        );
    }

    #[test]
    fn supports_mimo_edges_in_producer_to_consumer_direction() {
        let input = r#"
[topology]
device Y0: digital_output
device X0: digital_input
device valve_A: solenoid_valve
device valve_B: solenoid_valve
device sensor_A: sensor
device sensor_B: sensor
relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: Y0.out, to: valve_B.coil, via: driven_by }
relation { from: valve_A.out, to: sensor_A.sense, via: detects }
relation { from: valve_A.out, to: sensor_B.sense, via: detects }
relation { from: sensor_A.out, to: X0.in, via: reports_to }
relation { from: sensor_B.out, to: X0.in, via: reports_to }

[constraints]

[tasks]
"#;

        let program = parse_plc(input).expect("parse");
        let topology = build_topology_graph(&program).expect("build topology");

        let edge_exists = |from: &str, to: &str| {
            topology.graph.edge_references().any(|edge| {
                topology.graph[edge.source()].name == from
                    && topology.graph[edge.target()].name == to
            })
        };

        assert!(edge_exists("Y0", "valve_A"), "应支持一对多：Y0 -> valve_A");
        assert!(edge_exists("Y0", "valve_B"), "应支持一对多：Y0 -> valve_B");
        assert!(
            edge_exists("sensor_A", "X0"),
            "应支持多生产者汇聚到同一输入"
        );
        assert!(
            edge_exists("sensor_B", "X0"),
            "应支持多生产者汇聚到同一输入"
        );
        assert!(
            edge_exists("valve_A", "sensor_A"),
            "应支持多入：valve_A -> sensor_A"
        );
        assert!(
            edge_exists("valve_A", "sensor_B"),
            "应支持多入：valve_A -> sensor_B"
        );
    }

    #[test]
    fn reports_direction_error_for_invalid_reports_to_target() {
        let input = r#"
[topology]
device Y0: digital_output
device valve_A: solenoid_valve
device sensor_bad: sensor
relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: sensor_bad.out, to: valve_A.coil, via: reports_to }

[constraints]

[tasks]
"#;

        let program = parse_plc(input).expect("parse");
        let errors = build_topology_graph(&program).expect_err("reports_to 指向非 consumer 应报错");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].line(), 4);
        assert!(
            errors[0].to_string().contains("reports_to")
                && errors[0].to_string().contains("producer -> consumer"),
            "错误提示应说明 reports_to 的方向约束，实际: {}",
            errors[0]
        );
    }

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
}
