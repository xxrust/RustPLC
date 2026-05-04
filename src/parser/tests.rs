#[cfg(test)]
mod tests {
    use super::{parse_constraints, parse_plc, parse_tasks, parse_topology};
    use crate::ast::{
        ActionStatement, AxisAutoResetPolicy, AxisFaultPropagationScope, AxisFaultSeverity,
        AxisStopMode, BinaryOperator, DeviceType, Expression, ExternCallBinding, LiteralValue,
        OnCompleteDirective, PortRole, PortType, StepStatement, VariableType, WaitCondition,
    };

    #[test]
    fn parses_prd_5_3_topology_example() {
        let input = r#"
[topology]
device Y0: digital_output
device X0: digital_input
device valve_A: solenoid_valve { ports: [coil:digital:consumer, out:pneumatic:producer] }
device cyl_A: cylinder { ports: [cmd:pneumatic:consumer, extended:logical:producer] }
device sensor_A: sensor { ports: [sense:logical:consumer, out:digital:producer] }

relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_A.sense, via: detects }
relation { from: sensor_A.out, to: X0.in, via: reports_to }
"#;

        assert!(parse_topology(input).is_ok());
    }

    #[test]
    fn parses_custom_states_attribute_into_ast() {
        let input = r#"
[topology]

device valve_3pos: solenoid_valve {
    states: [extend, neutral, retract]
}

[constraints]

[tasks]

task main:
    step start:
        action: log "ok"
"#;

        let program = parse_plc(input).expect("自定义 states 属性应能解析为 AST");
        let valve = program
            .topology
            .devices
            .iter()
            .find(|device| device.name == "valve_3pos")
            .expect("应包含 valve_3pos 设备");

        let expected = vec![
            "extend".to_string(),
            "neutral".to_string(),
            "retract".to_string(),
        ];
        assert_eq!(
            valve.attributes.custom_states.as_ref(),
            Some(&expected),
            "应解析出自定义 states 列表"
        );
    }

    #[test]
    fn parses_new_relation_fields_and_ports_into_ast() {
        let input = r#"
[topology]

device Y0: digital_output { ports: [out:digital:producer] }
device X0: digital_input { ports: [in:digital:consumer] }
device valve_A: solenoid_valve { ports: [coil:digital:consumer, feedback:logical:producer] }
device sensor_A: sensor { ports: [sense:logical:consumer, out:digital:producer] }
relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.feedback, to: sensor_A.sense, via: detects }
relation { from: sensor_A.out, to: X0.in, via: reports_to }

[constraints]

[tasks]
task main:
    step idle:
        action: log "ok"
"#;

        let program = parse_plc(input).expect("应支持 relation + ports 新语法");
        let valve = program
            .topology
            .devices
            .iter()
            .find(|device| device.name == "valve_A")
            .expect("应包含 valve_A");
        let sensor = program
            .topology
            .devices
            .iter()
            .find(|device| device.name == "sensor_A")
            .expect("应包含 sensor_A");

        assert!(valve.attributes.driven_by.is_none());
        assert!(sensor.attributes.reports_to.is_none());
        assert!(sensor.attributes.detects.is_none());
        assert_eq!(valve.attributes.ports.len(), 2);
        assert_eq!(valve.attributes.ports[0].id, "coil");
        assert_eq!(valve.attributes.ports[0].port_type, PortType::Digital);
        assert_eq!(valve.attributes.ports[0].role, PortRole::Consumer);
        assert_eq!(program.topology.connections.len(), 3);
        assert_eq!(
            program.topology.connections[0].relation,
            crate::ast::TopologyRelation::DrivenBy
        );
        assert_eq!(
            program.topology.connections[0].from_port.as_deref(),
            Some("out")
        );
        assert_eq!(
            program.topology.connections[0].to_port.as_deref(),
            Some("coil")
        );
        assert_eq!(
            program.topology.connections[1].relation,
            crate::ast::TopologyRelation::Detects
        );
        assert_eq!(
            program.topology.connections[1].from_port.as_deref(),
            Some("feedback")
        );
        assert_eq!(
            program.topology.connections[1].to_port.as_deref(),
            Some("sense")
        );
        assert_eq!(
            program.topology.connections[2].relation,
            crate::ast::TopologyRelation::ReportsTo
        );
        assert_eq!(program.topology.connections[2].signal.as_deref(), None);
        assert_eq!(
            program.topology.connections[2].from_port.as_deref(),
            Some("out")
        );
        assert_eq!(
            program.topology.connections[2].to_port.as_deref(),
            Some("in")
        );
    }

    #[test]
    fn parses_explicit_relation_blocks_into_topology_connections() {
        let input = r#"
[topology]
device Y0: digital_output { ports: [out:digital:producer] }
device valve_A: solenoid_valve { ports: [coil:digital:consumer] }

relation {
    from: Y0.out,
    to: valve_A.coil,
    via: driven_by
}

[constraints]

[tasks]
task main:
    step idle:
"#;

        let program = parse_plc(input).expect("relation DSL 应写入 topology.connections");
        assert_eq!(program.topology.connections.len(), 1);

        let relation = &program.topology.connections[0];
        assert_eq!(relation.from, "Y0");
        assert_eq!(relation.to, "valve_A");
        assert_eq!(relation.from_port.as_deref(), Some("out"));
        assert_eq!(relation.to_port.as_deref(), Some("coil"));
        assert_eq!(relation.relation, crate::ast::TopologyRelation::DrivenBy);
    }

    #[test]
    fn parses_relation_with_plc_io_shorthand_endpoints() {
        let input = r#"
[topology]
device Y0: digital_output
device X0: digital_input
device valve_A: solenoid_valve { ports: [coil:digital:consumer, out:pneumatic:producer] }
device sensor_A: sensor

relation { from: Y0, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: sensor_A.sense, via: detects }
relation { from: sensor_A.out, to: X0, via: reports_to }

[constraints]

[tasks]
task main:
    step idle:
"#;

        let program = parse_plc(input).expect("relation 应支持 PLC IO 简写端点");
        assert_eq!(program.topology.connections.len(), 3);
        assert_eq!(program.topology.connections[0].from, "Y0");
        assert_eq!(program.topology.connections[0].from_port, None);
        assert_eq!(
            program.topology.connections[0].to_port.as_deref(),
            Some("coil")
        );
        assert_eq!(program.topology.connections[2].to, "X0");
        assert_eq!(program.topology.connections[2].to_port, None);
    }

    #[test]
    fn parses_plc_controller_device_type_and_model_ref() {
        let input = r#"
[topology]
device plc_main: plc { model_ref: openplc_softplc }
device valve_A: solenoid_valve { ports: [coil:digital:consumer] }
relation { from: plc_main.Y0, to: valve_A.coil, via: driven_by }

[constraints]

[tasks]
task main:
    step idle:
"#;

        let program = parse_plc(input).expect("plc 设备应能解析");
        let plc = program
            .topology
            .devices
            .iter()
            .find(|d| d.name == "plc_main")
            .expect("应包含 plc_main");
        assert!(matches!(plc.device_type, DeviceType::Plc));
        assert_eq!(plc.attributes.model_ref.as_deref(), Some("openplc_softplc"));
        assert_eq!(
            program.topology.connections[0].from_port.as_deref(),
            Some("Y0")
        );
    }

    #[test]
    fn rejects_relation_when_required_fields_are_missing() {
        let cases = [
            (
                "missing_from",
                "relation { to: valve_A.coil, via: driven_by }",
                "relation 缺少 from 字段",
            ),
            (
                "missing_to",
                "relation { from: Y0.out, via: driven_by }",
                "relation 缺少 to 字段",
            ),
            (
                "missing_via",
                "relation { from: Y0.out, to: valve_A.coil }",
                "relation 缺少 via 字段",
            ),
        ];

        for (case_name, relation_block, expected_error) in cases {
            let input = format!(
                r#"
[topology]
device Y0: digital_output {{ ports: [out:digital:producer] }}
device valve_A: solenoid_valve {{ ports: [coil:digital:consumer] }}

{relation_block}

[constraints]

[tasks]
task main:
    step idle:
"#
            );

            let err = parse_plc(&input).expect_err(case_name);
            assert!(
                err.to_string().contains(expected_error),
                "{case_name} 应返回 `{expected_error}`，实际: {err}"
            );
        }
    }

    #[test]
    fn rejects_legacy_topology_attributes_with_migration_hint() {
        let cases = [
            ("driven_by", "driven_by: Y0", "via: driven_by"),
            ("reports_to", "reports_to: X0", "via: reports_to"),
            ("detects", "detects: valve_A.on", "via: detects"),
        ];

        for (name, legacy_attr, hint) in cases {
            let input = format!(
                r#"
[topology]
device Y0: digital_output
device X0: digital_input
device valve_A: solenoid_valve {{ {legacy_attr} }}

[constraints]

[tasks]
task main:
    step idle:
"#
            );

            let err = parse_plc(&input).expect_err(name);
            assert!(
                err.to_string().contains("已废弃"),
                "{name} 应提示旧写法已废弃，实际: {err}"
            );
            assert!(
                err.to_string().contains(hint),
                "{name} 应提示迁移到 relation.via，实际: {err}"
            );
        }
    }

    #[test]
    fn parses_subtype_attribute_into_ast() {
        let input = r#"
[topology]
device start_button: digital_input { subtype: "push_button" }

[constraints]

[tasks]
task main:
    step idle:
"#;

        let program = parse_plc(input).expect("subtype should parse");
        let start_button = program
            .topology
            .devices
            .iter()
            .find(|device| device.name == "start_button")
            .expect("should include start_button");
        assert_eq!(
            start_button.attributes.subtype.as_deref(),
            Some("push_button")
        );
    }

    #[test]
    fn rejects_removed_type_attribute_with_hint() {
        let input = r#"
[topology]
device legacy_limit: digital_input { type: "limit_switch" }

[constraints]

[tasks]
task main:
    step idle:
"#;

        let err = parse_plc(input).expect_err("legacy type attribute should be rejected");
        assert!(
            err.to_string().contains("属性 type 已移除"),
            "应提示 type 已移除，实际: {err}"
        );
    }

    #[test]
    fn rejects_connected_to_with_migration_hint() {
        let input = r#"
[topology]
device Y0: digital_output
device valve_A: solenoid_valve { connected_to: Y0 }

[constraints]

[tasks]
task main:
    step idle:
        action: log "ok"
"#;

        let err = parse_plc(input).expect_err("connected_to 应被明确禁止");
        assert_eq!(err.line(), 4);
        assert!(
            err.to_string().contains("relation { from: Device.Port"),
            "迁移提示应建议使用 relation + Device.Port，实际: {err}"
        );
    }

    #[test]
    fn parses_multidimensional_tags_into_ast() {
        let input = r#"
[topology]

device valve_A: solenoid_valve {
    tags: {
        functional_group: [clamp, press],
        danger_level: [high],
        location_group: ["line_a/cell_2/station_7"]
    }
}

[constraints]

[tasks]
task main:
    step idle:
        action: log "ok"
"#;

        let program = parse_plc(input).expect("应支持多维 tags 语法");
        let valve = program
            .topology
            .devices
            .iter()
            .find(|device| device.name == "valve_A")
            .expect("应包含 valve_A");

        assert_eq!(
            valve.attributes.tags.functional_group,
            vec!["clamp".to_string(), "press".to_string()]
        );
        assert_eq!(valve.attributes.tags.danger_level, vec!["high".to_string()]);
        assert_eq!(
            valve.attributes.tags.location_group,
            vec!["line_a/cell_2/station_7".to_string()]
        );
    }

    #[test]
    fn parses_external_attribute_into_ast() {
        let input = r#"
[topology]

device X1: digital_input {
    external: true
}

device pressure_in: analog_input {
    range: 0..10,
    external: true
}

[constraints]

[tasks]

task main:
    step start:
        action: log "ok"
"#;

        let program = parse_plc(input).expect("external 属性应能解析为 AST");
        let digital = program
            .topology
            .devices
            .iter()
            .find(|device| device.name == "X1")
            .expect("应包含 X1 设备");
        let analog = program
            .topology
            .devices
            .iter()
            .find(|device| device.name == "pressure_in")
            .expect("应包含 pressure_in 设备");

        assert_eq!(
            digital.attributes.external,
            Some(true),
            "digital_input external 应解析为 true"
        );
        assert_eq!(
            analog.attributes.external,
            Some(true),
            "analog_input external 应解析为 true"
        );
    }

    #[test]
    fn parses_extern_function_declarations_into_ast() {
        let input = r#"
[topology]
extern function add(a: float, b: float) -> float {
    rust_module: "math::basic"
    pure: true
    time_bound_us: 10
}

extern function split(v: float) -> (float, float) {
    rust_module: "math::split"
    pure: true
    time_bound_us: 15
}

[constraints]

[tasks]
task main:
    step idle:
"#;

        let program = parse_plc(input).expect("extern function 声明应能解析到 AST");
        assert_eq!(program.topology.extern_functions.len(), 2);

        let add = &program.topology.extern_functions[0];
        assert_eq!(add.name, "add");
        assert_eq!(add.params.len(), 2);
        assert_eq!(add.params[0].name, "a");
        assert_eq!(add.params[0].var_type, VariableType::Float);
        assert_eq!(add.params[1].name, "b");
        assert_eq!(add.params[1].var_type, VariableType::Float);
        assert_eq!(add.return_types, vec![VariableType::Float]);
        assert_eq!(add.contract.rust_module, "math::basic");
        assert!(add.contract.pure);
        assert_eq!(add.contract.time_bound_us, 10);

        let split = &program.topology.extern_functions[1];
        assert_eq!(split.name, "split");
        assert_eq!(split.params.len(), 1);
        assert_eq!(split.params[0].name, "v");
        assert_eq!(split.params[0].var_type, VariableType::Float);
        assert_eq!(
            split.return_types,
            vec![VariableType::Float, VariableType::Float]
        );
        assert_eq!(split.contract.rust_module, "math::split");
        assert!(split.contract.pure);
        assert_eq!(split.contract.time_bound_us, 15);
    }

    #[test]
    fn rejects_extern_declaration_missing_required_contract_fields() {
        let input = r#"
[topology]
extern function add(a: float, b: float) -> float {
    pure: true
    time_bound_us: 10
}

[constraints]

[tasks]
task main:
    step idle:
"#;

        let err = parse_plc(input).expect_err("缺少 rust_module 时应返回错误");
        assert_eq!(err.line(), 3, "错误应定位到 extern 声明行");
        assert!(
            err.to_string()
                .contains("缺少必填 contract 字段 rust_module"),
            "错误信息应明确缺失字段，实际: {err}"
        );
    }

    #[test]
    fn parses_axis_fault_contract_declaration_into_ast() {
        let input = r#"
[topology]
device axis_x: stepper_motor {
    purpose: "transport"
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
"#;

        let program = parse_plc(input).expect("axis_fault_contract 声明应能解析到 AST");
        assert_eq!(program.topology.axis_fault_contracts.len(), 1);
        let contract = &program.topology.axis_fault_contracts[0];
        assert_eq!(contract.name, "axis_x_fault");
        assert_eq!(contract.axis, "axis_x");
        assert_eq!(contract.severity, AxisFaultSeverity::Safety);
        assert_eq!(contract.stop_mode, AxisStopMode::Immediate);
        assert_eq!(contract.auto_reset_policy, AxisAutoResetPolicy::Never);
        assert!(contract.manual_ack_required);
        assert_eq!(
            contract.propagation_scope,
            AxisFaultPropagationScope::SelfOnly
        );
        assert!(contract.propagation_targets.is_empty());
    }

    #[test]
    fn rejects_axis_fault_contract_missing_required_fields() {
        let input = r#"
[topology]
device axis_x: stepper_motor {
    purpose: "transport"
}
axis_fault_contract axis_x_fault {
    axis: axis_x
    stop_mode: quick
    auto_reset_policy: on_clear
    manual_ack_required: false
    propagation_scope: self
}

[constraints]

[tasks]
task main:
    step idle:
"#;

        let err = parse_plc(input).expect_err("缺少 severity 字段时应返回错误");
        assert!(
            err.to_string().contains("缺少必填字段 severity"),
            "错误信息应明确缺失字段，实际: {err}"
        );
    }

    #[test]
    fn parses_axis_fault_contract_custom_propagation_targets() {
        let input = r#"
[topology]
device axis_x: stepper_motor {
    purpose: "transport"
}
device axis_y: servo_drive {
    purpose: "transport"
}
axis_fault_contract axis_x_fault {
    axis: axis_x
    severity: safety
    stop_mode: immediate
    auto_reset_policy: never
    manual_ack_required: true
    propagation_scope: custom
    propagation_targets: [axis_y]
}

[constraints]

[tasks]
task main:
    step idle:
"#;

        let program = parse_plc(input).expect("custom propagation should parse");
        let contract = &program.topology.axis_fault_contracts[0];
        assert_eq!(
            contract.propagation_scope,
            AxisFaultPropagationScope::Custom
        );
        assert_eq!(contract.propagation_targets, vec!["axis_y".to_string()]);
    }

    #[test]
    fn rejects_axis_fault_contract_custom_scope_without_targets() {
        let input = r#"
[topology]
device axis_x: stepper_motor {
    purpose: "transport"
}
axis_fault_contract axis_x_fault {
    axis: axis_x
    severity: recoverable
    stop_mode: controlled
    auto_reset_policy: on_clear
    manual_ack_required: false
    propagation_scope: custom
}

[constraints]

[tasks]
task main:
    step idle:
"#;

        let err = parse_plc(input).expect_err("custom scope without targets should fail");
        assert!(
            err.to_string().contains("必须提供 propagation_targets"),
            "error should mention missing propagation_targets, got: {err}"
        );
    }

    #[test]
    fn rejects_axis_fault_contract_non_custom_scope_with_targets() {
        let input = r#"
[topology]
device axis_x: stepper_motor {
    purpose: "transport"
}
device axis_y: servo_drive {
    purpose: "transport"
}
axis_fault_contract axis_x_fault {
    axis: axis_x
    severity: recoverable
    stop_mode: controlled
    auto_reset_policy: on_clear
    manual_ack_required: false
    propagation_scope: followers
    propagation_targets: [axis_y]
}

[constraints]

[tasks]
task main:
    step idle:
"#;

        let err = parse_plc(input).expect_err("non-custom scope with targets should fail");
        assert!(
            err.to_string()
                .contains("仅在 propagation_scope=custom 时允许 propagation_targets"),
            "error should mention invalid propagation_targets usage, got: {err}"
        );
    }

    #[test]
    fn parses_extern_call_actions_with_single_and_tuple_bindings() {
        let input = r#"
[topology]
extern function add(a: float, b: float) -> float {
    rust_module: "math::basic"
    pure: true
    time_bound_us: 10
}
extern function split(v: float) -> (float, float) {
    rust_module: "math::split"
    pure: true
    time_bound_us: 15
}
variable x: float = 1.0
variable y: float = 2.0
variable sum: float = 0.0
variable lo: float = 0.0
variable hi: float = 0.0

[constraints]

[tasks]
task main:
    step run:
        action: call add(x, y) -> sum
        action: call split(sum) -> (lo, hi)
"#;

        let program = parse_plc(input).expect("extern call action 应能解析");
        let statements = &program.tasks.tasks[0].steps[0].statements;
        assert_eq!(statements.len(), 2);

        match &statements[0] {
            StepStatement::Action(ActionStatement::Call {
                function,
                args,
                binding,
            }) => {
                assert_eq!(function, "add");
                assert_eq!(args.len(), 2);
                assert!(matches!(&args[0], Expression::Variable(name) if name == "x"));
                assert!(matches!(&args[1], Expression::Variable(name) if name == "y"));
                assert!(matches!(binding, ExternCallBinding::Single(name) if name == "sum"));
            }
            other => panic!("第一个 action 应为 extern call，实际: {other:?}"),
        }

        match &statements[1] {
            StepStatement::Action(ActionStatement::Call {
                function,
                args,
                binding,
            }) => {
                assert_eq!(function, "split");
                assert_eq!(args.len(), 1);
                assert!(matches!(&args[0], Expression::Variable(name) if name == "sum"));
                assert!(matches!(
                    binding,
                    ExternCallBinding::Tuple(names) if names == &vec!["lo".to_string(), "hi".to_string()]
                ));
            }
            other => panic!("第二个 action 应为 tuple extern call，实际: {other:?}"),
        }
    }

    #[test]
    fn rejects_extern_calls_in_expression_context() {
        let input = r#"
[topology]
extern function add(a: float, b: float) -> float {
    rust_module: "math::basic"
    pure: true
    time_bound_us: 10
}
variable x: float = 1.0
variable y: float = 2.0
variable out: float = 0.0

[constraints]

[tasks]
task main:
    step run:
        action: compute out = add(x, y)
"#;

        let err = parse_plc(input).expect_err("extern 函数在表达式上下文中应被拒绝");
        assert!(
            err.to_string().contains("只能在 action: call 中调用"),
            "错误信息应提示 extern 调用上下文限制，实际: {err}"
        );
    }

    #[test]
    fn parses_pid_device_declaration_minimal_fields() {
        let input = r#"
[topology]

device AI0: analog_input { range: 0..100, unit: "bar" }
device AO0: analog_output { range: 0..100, unit: "%" }
device loop_pressure: pid {
    pv: AI0,
    sp: 50bar,
    kp: 2.0,
    ki: 0.3,
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

        let program = parse_plc(input).expect("PID 设备声明应能解析");
        let pid = program
            .topology
            .devices
            .iter()
            .find(|device| device.name == "loop_pressure")
            .expect("应包含 loop_pressure PID");
        assert!(matches!(pid.device_type, crate::ast::DeviceType::Pid));
        assert_eq!(pid.attributes.pv.as_deref(), Some("AI0"));
        assert_eq!(pid.attributes.out.as_deref(), Some("AO0"));
        assert_eq!(pid.attributes.period_ms, Some(100));
        assert_eq!(pid.attributes.kp, Some(2.0));
        assert_eq!(pid.attributes.ki, Some(0.3));
        assert_eq!(pid.attributes.kd, Some(0.05));
        match pid.attributes.sp.as_ref() {
            Some(LiteralValue::Measured(measured)) => {
                assert!((measured.value - 50.0).abs() < f64::EPSILON);
                assert_eq!(measured.unit, "bar");
            }
            other => panic!("sp 应解析为 measured literal, got {other:?}"),
        }
    }

    #[test]
    fn parses_all_topology_device_types_and_property_shapes() {
        let input = r#"
[topology]

device Y3: digital_output
device X5: digital_input

device estop: digital_input {
    debounce: 10ms,
    inverted: true
}

device spindle_valve: solenoid_valve {
    response_time: 25ms,
    subtype: "3/2"
}

device spindle_cyl: cylinder {
    stroke_time: 120ms,
    retract_time: 110ms,
    stroke: 80mm,
    subtype: compact
}

device spindle_sensor: sensor {
    subtype: optical
}

device spindle_motor: motor {
    rated_speed: 60rpm,
    ramp_time: 300ms
}

device axis_stepper: stepper_motor {
    steps_per_rev: 200,
    max_speed: 1200,
    accel_time: 80ms,
    decel_time: 90ms
}

device feed_vfd: vfd {
    rated_power: 2.2,
    rated_freq: 50
}

device pick_servo: servo_drive {
    encoder_resolution: 131072,
    electronic_gear_num: 10,
    electronic_gear_den: 1,
    positioning_window: 5
}

device flow_valve: proportional_valve
device hand: gripper
device belt: conveyor
device coolant_pump: pump
device oven_heater: heater
device camera: vision_sensor
"#;

        assert!(parse_topology(input).is_ok());
    }

    #[test]
    fn stores_motor_extension_attributes_into_extra_params() {
        let input = r#"
[topology]

device axis: stepper_motor {
    steps_per_rev: 200,
    max_speed: 1200,
    accel_time: 80ms,
    microstep: 16,
    gear_num: 5,
    gear_den: 2,
    lead_screw: 5.0,
    position_unit: mm,
    max_acceleration: 2500
}

[constraints]

[tasks]

task main:
    step idle:
"#;

        let program = parse_plc(input).expect("应能解析 motor 扩展参数");
        let axis = program
            .topology
            .devices
            .iter()
            .find(|device| device.name == "axis")
            .expect("应包含 axis 设备");

        assert_eq!(
            axis.attributes.extra_params.get("steps_per_rev"),
            Some(&"200".to_string())
        );
        assert_eq!(
            axis.attributes.extra_params.get("max_speed"),
            Some(&"1200".to_string())
        );
        assert_eq!(
            axis.attributes.extra_params.get("accel_time"),
            Some(&"80ms".to_string())
        );
        assert_eq!(
            axis.attributes.extra_params.get("microstep"),
            Some(&"16".to_string())
        );
        assert_eq!(
            axis.attributes.extra_params.get("gear_num"),
            Some(&"5".to_string())
        );
        assert_eq!(
            axis.attributes.extra_params.get("gear_den"),
            Some(&"2".to_string())
        );
        assert_eq!(
            axis.attributes.extra_params.get("lead_screw"),
            Some(&"5.0".to_string())
        );
        assert_eq!(
            axis.attributes.extra_params.get("position_unit"),
            Some(&"mm".to_string())
        );
        assert_eq!(
            axis.attributes.extra_params.get("max_acceleration"),
            Some(&"2500".to_string())
        );
    }

    #[test]
    fn parses_axis_motion_param_set_reference() {
        let input = r#"
[topology]

device axis_x: stepper_motor {
    model_ref: stepper_generic,
    config_ref: stepper_default,
    motion_param_set: stepper_pick
}

[constraints]

[tasks]

task main:
    step idle:
"#;

        let program = parse_plc(input).expect("应能解析 motion_param_set 引用");
        let axis = program
            .topology
            .devices
            .iter()
            .find(|device| device.name == "axis_x")
            .expect("应包含 axis_x 设备");

        assert_eq!(
            axis.attributes.model_ref.as_deref(),
            Some("stepper_generic")
        );
        assert_eq!(
            axis.attributes.config_ref.as_deref(),
            Some("stepper_default")
        );
        assert_eq!(
            axis.attributes.motion_param_set.as_deref(),
            Some("stepper_pick")
        );
    }

    #[test]
    fn rejects_misspelled_axis_parameter_name() {
        let input = r#"
[topology]

device axis: stepper_motor {
    microstepp: 16
}

[constraints]

[tasks]

task main:
    step idle:
"#;

        let err = parse_plc(input).expect_err("非法参数名应被解析器拒绝");
        let msg = err.to_string();
        assert!(
            msg.contains("expected attribute")
                || msg.contains("attribute_name")
                || msg.contains("不支持的属性名"),
            "应提示 attribute/attribute_name/属性名错误，实际: {msg}"
        );
    }

    #[test]
    fn parses_prd_5_4_constraints_example() {
        let input = r#"
[constraints]

# ===== 状态互斥 (Safety) =====
safety: cyl_A.extended conflicts_with cyl_B.extended
    reason: "A缸和B缸同时伸出会导致机械碰撞"

safety: valve_A.on conflicts_with valve_B.on
    reason: "气源压力不足以同时驱动两个阀"

# ===== 时序约束 (Timing) =====
timing: task.init must_complete_within 5000ms
    reason: "初始化超过5秒视为异常"

timing: task.init.step_extend_A must_complete_within 500ms
    reason: "单步动作不应超过500ms"

# ===== 因果链声明 (Causality) =====
causality: Y0 -> valve_A -> cyl_A -> sensor_A_ext
    reason: "Y0 驱动 valve_A 推动 cyl_A 由 sensor_A_ext 检测"

causality: Y1 -> valve_B -> cyl_B -> sensor_B_ext
    reason: "Y1 驱动 valve_B 推动 cyl_B 由 sensor_B_ext 检测"
"#;

        assert!(parse_constraints(input).is_ok());
    }

    #[test]
    fn parses_requires_and_must_start_after_constraints() {
        let input = r#"
[constraints]

safety: sensor_A_ext.on requires valve_A.on
timing: task.ready must_start_after 120ms
causality: X0 -> relay_A -> valve_A
"#;

        assert!(parse_constraints(input).is_ok());
    }

    #[test]
    fn parses_must_complete_within_worst_case_constraints() {
        let input = r#"
[constraints]

timing: task.ready must_complete_within_worst_case 120ms
"#;

        assert!(parse_constraints(input).is_ok());
    }

    #[test]
    fn parses_prd_5_5_1_basic_sequence_tasks_example() {
        let input = r#"
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
"#;

        assert!(parse_tasks(input).is_ok());
    }

    #[test]
    fn parses_prd_5_5_2_wait_and_jump_tasks_example() {
        let input = r#"
[tasks]

task ready:
    step wait_start:
        wait: start_button == true
        allow_indefinite_wait: true
    on_complete: goto main_cycle
"#;

        assert!(parse_tasks(input).is_ok());
    }

    #[test]
    fn parses_delay_statement_into_ast_milliseconds() {
        let input = r#"
[topology]
device Y0: digital_output
device X0: digital_input
device valve_A: solenoid_valve
device cyl_A: cylinder { stroke_time: 200ms, retract_time: 180ms }
device sensor_A_ext: sensor

[constraints]
causality: Y0 -> valve_A -> cyl_A -> sensor_A_ext

[tasks]
task init:
    step settle:
        delay: 2000ms
        delay: 0ms
        wait: sensor_A_ext == true
"#;

        let ast = parse_plc(input).expect("包含 delay 的 PLC 应能构建 AST");
        let statements = &ast.tasks.tasks[0].steps[0].statements;

        assert!(matches!(
            statements.first(),
            Some(StepStatement::Delay { duration_ms: 2000 })
        ));
        assert!(matches!(
            statements.get(1),
            Some(StepStatement::Delay { duration_ms: 0 })
        ));
    }

    #[test]
    fn parses_repeat_block_into_ast() {
        let input = r#"
[topology]
device Y0: digital_output
device X0: digital_input
device valve_glue: solenoid_valve
device cyl_glue: cylinder { stroke_time: 200ms, retract_time: 180ms }
device sensor_glue_ext: sensor

[constraints]
causality: Y0 -> valve_glue -> cyl_glue -> sensor_glue_ext

[tasks]
task glue:
    step glue_cycle:
        repeat 3:
            action: extend cyl_glue
            wait: sensor_glue_ext == true
            timeout: 300ms -> goto fault_handler
"#;

        let ast = parse_plc(input).expect("包含 repeat 的 PLC 应能构建 AST");
        let statements = &ast.tasks.tasks[0].steps[0].statements;

        let repeat = statements.first().expect("repeat 语句应位于 step 首条语句");
        match repeat {
            StepStatement::Repeat { count, body } => {
                assert_eq!(*count, 3);
                assert!(matches!(body.first(), Some(StepStatement::Action(_))));
                assert!(matches!(body.get(1), Some(StepStatement::Wait(_))));
                assert!(matches!(body.get(2), Some(StepStatement::Timeout(_))));
            }
            other => panic!("期望 repeat 语句，实际为: {other:?}"),
        }
    }

    #[test]
    fn parses_repeat_zero_count_in_syntax_stage() {
        let input = r#"
[topology]
device Y0: digital_output
device valve_glue: solenoid_valve
device cyl_glue: cylinder { stroke_time: 200ms, retract_time: 180ms }

[constraints]

[tasks]
task glue:
    step glue_cycle:
        repeat 0:
            action: extend cyl_glue
"#;

        let ast = parse_plc(input).expect("repeat 0 在语法阶段应可解析");
        assert!(matches!(
            ast.tasks.tasks[0].steps[0].statements.first(),
            Some(StepStatement::Repeat { count: 0, .. })
        ));
    }

    #[test]
    fn parses_prd_5_5_3_fault_handler_tasks_example() {
        let input = r#"
[tasks]

task fault_handler:
    step safe_position:
        action: retract cyl_A
        action: retract cyl_B
    step alarm:
        action: set alarm_light on
        action: log "动作超时，已执行安全复位"
    on_complete: goto ready
"#;

        assert!(parse_tasks(input).is_ok());
    }

    #[test]
    fn parses_prd_5_5_4_parallel_tasks_example() {
        let input = r#"
[tasks]

task parallel_demo:
    step move_together:
        parallel:
            branch_A:
                action: extend cyl_A
                wait: sensor_A_ext == true
                timeout: 600ms -> goto fault_handler
            branch_B:
                action: extend cyl_B
                wait: sensor_B_ext == true
                timeout: 800ms -> goto fault_handler
    on_complete: goto next_task
"#;

        assert!(parse_tasks(input).is_ok());
    }

    #[test]
    fn keeps_task_on_complete_after_terminal_parallel_step() {
        let input = r#"
[topology]

[constraints]

[tasks]

task cycle:
    step do_parallel:
        parallel:
            branch_A:
                delay: 10ms
            branch_B:
                delay: 20ms
    on_complete: goto ready

task ready:
    step idle:
        action: log "idle"
"#;

        let ast = parse_plc(input).expect("并行末尾 step 后的 on_complete 应可解析");
        let cycle = ast
            .tasks
            .tasks
            .iter()
            .find(|task| task.name == "cycle")
            .expect("应存在 cycle task");
        assert!(
            matches!(cycle.on_complete, Some(OnCompleteDirective::Goto { .. })),
            "on_complete: goto 不应被并行分支吞掉"
        );

        let StepStatement::Parallel(block) = &cycle.steps[0].statements[0] else {
            panic!("cycle.do_parallel 首条语句应为 parallel");
        };
        assert_eq!(block.branches.len(), 2, "parallel 分支数量应保持为 2");
    }

    #[test]
    fn parses_prd_5_5_5_race_tasks_example() {
        let input = r#"
[tasks]

task search_position:
    step start_motor:
        action: set motor on
    step detect:
        race:
            branch_A:
                wait: sensor_A == true
                then: goto process_A
            branch_B:
                wait: sensor_B == true
                then: goto process_B
        timeout: 2000ms -> goto fault_handler
    on_complete: unreachable
"#;

        assert!(parse_tasks(input).is_ok());
    }

    #[test]
    fn parses_set_with_enum_like_state_value() {
        let input = r#"
[tasks]

task drive:
    step start:
        action: set stepper_x.direction forward
"#;

        assert!(parse_tasks(input).is_ok());
    }

    #[test]
    fn parses_prd_6_3_full_example_into_ast() {
        let input = r#"
[topology]

device Y0: digital_output
device Y1: digital_output
device X0: digital_input
device X1: digital_input
device X2: digital_input
device X3: digital_input
device X4: digital_input

device start_button: digital_input {
    debounce: 20ms
}

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
device sensor_A_ext: sensor
device sensor_A_ret: sensor
device sensor_B_ext: sensor
device sensor_B_ret: sensor

[constraints]

safety: cyl_A.extended conflicts_with cyl_B.extended
    reason: "A缸和B缸不能同时伸出"

causality: Y0 -> valve_A -> cyl_A -> sensor_A_ext
causality: Y1 -> valve_B -> cyl_B -> sensor_B_ext

[tasks]

task init:
    step extend_A:
        action: extend cyl_A
        wait: sensor_A_ext == true
        timeout: 500ms -> goto fault_handler
    step retract_A:
        action: retract cyl_A
        wait: sensor_A_ret == true
        timeout: 500ms -> goto fault_handler
    step extend_B:
        action: extend cyl_B
        wait: sensor_B_ext == true
        timeout: 500ms -> goto fault_handler
    step retract_B:
        action: retract cyl_B
        wait: sensor_B_ret == true
        timeout: 500ms -> goto fault_handler
    on_complete: goto ready

task fault_handler:
    step safe:
        action: retract cyl_A
        action: retract cyl_B
    step alarm:
        action: log "动作超时报警"
    on_complete: goto ready

task ready:
    step wait_start:
        wait: start_button == true
        allow_indefinite_wait: true
    on_complete: goto init
"#;

        let ast = parse_plc(input).expect("PRD 6.3 示例应能成功构建 AST");

        assert_eq!(ast.topology.devices.len(), 16);
        assert_eq!(ast.constraints.safety.len(), 1);
        assert_eq!(ast.constraints.causality.len(), 2);
        assert_eq!(ast.tasks.tasks.len(), 3);

        let init_task = ast
            .tasks
            .tasks
            .iter()
            .find(|task| task.name == "init")
            .expect("应包含 init task");
        assert_eq!(init_task.steps.len(), 4);
        assert!(matches!(
            init_task.on_complete,
            Some(OnCompleteDirective::Goto { ref target })
                if target.task == "ready" && target.step.is_none()
        ));

        assert!(matches!(
            init_task.steps[0].statements.first(),
            Some(StepStatement::Action(ActionStatement::Extend { target, .. })) if target.device == "cyl_A"
        ));
    }

    #[test]
    fn parses_prd_9_half_rotation_example_into_ast() {
        let input = r#"
[topology]

device Y0: digital_output                # 电机控制
device X0: digital_input                 # 传感器A
device X1: digital_input                 # 传感器B
device X2: digital_input                 # 启动按钮

device start_button: digital_input {     # 启动按钮
    debounce: 20ms
}

device motor_ctrl: motor {
    rated_speed: 60rpm
    ramp_time: 50ms                      # 启动到额定转速时间
}

device sensor_A: sensor {
    subtype: proximity
}

device sensor_B: sensor {
    subtype: proximity
}

[constraints]

# 半圈旋转时间: 60rpm = 1圈/秒, 半圈 = 500ms, 加上启动时间
timing: task.search.step_detect must_complete_within 800ms
    reason: "半圈旋转加启动不应超过800ms"

causality: Y0 -> motor_ctrl -> sensor_A
    reason: "电机旋转应能被传感器A检测"
causality: Y0 -> motor_ctrl -> sensor_B
    reason: "电机旋转应能被传感器B检测"

[tasks]

task search:
    step start_motor:
        action: set motor_ctrl on
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
        action: set motor_ctrl off
    step do_work_A:
        action: log "工件在A位置，执行A工艺"
        # ... A 工艺的具体步骤
    on_complete: goto ready

task process_B:
    step stop_motor:
        action: set motor_ctrl off
    step do_work_B:
        action: log "工件在B位置，执行B工艺"
        # ... B 工艺的具体步骤
    on_complete: goto ready

task motor_fault:
    step emergency_stop:
        action: set motor_ctrl off
    step alarm:
        action: log "电机旋转超时: 半圈内未检测到任何传感器信号"
        action: log "请检查: 电机是否旋转 / 传感器A,B是否正常 / 工件是否到位"
    on_complete: goto ready

task ready:
    step wait_start:
        wait: start_button == true
        allow_indefinite_wait: true
    on_complete: goto search
"#;

        let ast = parse_plc(input).expect("PRD 9 示例应能成功构建 AST");

        assert_eq!(ast.topology.devices.len(), 8);
        assert_eq!(ast.constraints.timing.len(), 1);
        assert_eq!(ast.constraints.causality.len(), 2);
        assert_eq!(ast.tasks.tasks.len(), 5);

        let search_task = ast
            .tasks
            .tasks
            .iter()
            .find(|task| task.name == "search")
            .expect("应包含 search task");
        assert_eq!(search_task.steps.len(), 2);

        let detect_step = search_task
            .steps
            .iter()
            .find(|step| step.name == "detect")
            .expect("search 任务应包含 detect step");

        assert!(detect_step
            .statements
            .iter()
            .any(|stmt| matches!(stmt, StepStatement::Race(_))));
        assert!(detect_step
            .statements
            .iter()
            .any(|stmt| matches!(stmt, StepStatement::Timeout(_))));

        let ready_task = ast
            .tasks
            .tasks
            .iter()
            .find(|task| task.name == "ready")
            .expect("应包含 ready task");
        assert!(matches!(
            ready_task.on_complete,
            Some(OnCompleteDirective::Goto { ref target })
                if target.task == "search" && target.step.is_none()
        ));
    }

    #[test]
    fn parse_plc_reports_line_number_for_syntax_errors() {
        let bad_input = r#"
[topology]
device Y0: digital_output

[constraints]
safety: cyl_A.extended conflicts_with

[tasks]
"#;

        let err = parse_plc(bad_input).expect_err("错误输入应返回解析错误");
        assert!(err.line() >= 6);
    }

    #[test]
    fn parses_variable_declarations_in_topology() {
        let input = r#"
[topology]
device plc_main: plc
variable master_pos: float = 0.0
variable cycle_count: int = 0
variable cam_active: bool = false

[constraints]

[tasks]
task main:
    step idle:
        action: log "ok"
"#;

        let ast = parse_plc(input).expect("变量声明应能解析");
        assert_eq!(ast.topology.variables.len(), 3);
        assert_eq!(ast.topology.variables[0].name, "master_pos");
        assert!(matches!(
            ast.topology.variables[0].var_type,
            crate::ast::VariableType::Float
        ));
        assert_eq!(ast.topology.variables[0].initial_value, "0.0");
        assert!(matches!(
            ast.topology.variables[1].var_type,
            crate::ast::VariableType::Int
        ));
        assert_eq!(ast.topology.variables[1].initial_value, "0");
        assert!(matches!(
            ast.topology.variables[2].var_type,
            crate::ast::VariableType::Bool
        ));
        assert_eq!(ast.topology.variables[2].initial_value, "false");
    }

    #[test]
    fn parses_cam_table_declarations_in_topology() {
        let input = r#"
[topology]
cam_table linear_cam: periodic [
    (0, 0),
    (90, 50),
    (180, 50),
    (360, 0),
]
cam_table shear_profile: oneshot [
    (0, 0),
    (30, 5),
    (60, 45),
]

[constraints]

[tasks]
task main:
    step idle:
        action: log "ok"
"#;

        let ast = parse_plc(input).expect("cam_table 声明应能解析");
        assert_eq!(ast.topology.cam_tables.len(), 2);
        assert_eq!(ast.topology.cam_tables[0].name, "linear_cam");
        assert!(matches!(
            ast.topology.cam_tables[0].mode,
            crate::ast::CamTableMode::Periodic
        ));
        assert_eq!(ast.topology.cam_tables[0].points.len(), 4);
        assert!((ast.topology.cam_tables[0].points[1].master - 90.0).abs() < f64::EPSILON);
        assert!((ast.topology.cam_tables[0].points[1].slave - 50.0).abs() < f64::EPSILON);
        assert!(matches!(
            ast.topology.cam_tables[1].mode,
            crate::ast::CamTableMode::Oneshot
        ));
    }

    #[test]
    fn parses_cam_coupling_device_with_attributes() {
        let input = r#"
[topology]
device encoder_main: analog_input
device servo_x: servo_drive
device cam_xy: cam_coupling {
    master: encoder_main,
    slave: servo_x,
    table: linear_cam,
    interpolation: cubic_spline,
    gear_ratio: 1.5,
    phase_offset: 10.0,
    following_error_limit: 2.0,
    slave_feedback: servo_x,
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

        let ast = parse_plc(input).expect("cam_coupling 声明应能解析");
        let cam = ast
            .topology
            .devices
            .iter()
            .find(|d| d.name == "cam_xy")
            .expect("应包含 cam_xy");
        assert!(matches!(cam.device_type, DeviceType::CamCoupling));
        assert_eq!(cam.attributes.master.as_deref(), Some("encoder_main"));
        assert_eq!(cam.attributes.slave.as_deref(), Some("servo_x"));
        assert_eq!(cam.attributes.table.as_deref(), Some("linear_cam"));
        assert_eq!(
            cam.attributes.interpolation.as_deref(),
            Some("cubic_spline")
        );
    }

    #[test]
    fn parses_cam_action_statements() {
        let input = r#"
[topology]
device cam_xy: cam_coupling
cam_table t0: periodic [
    (0, 0),
    (360, 0),
]
cam_table t1: periodic [
    (0, 0),
    (360, 0),
]
variable phase: float = 12.5

[constraints]

[tasks]
task main:
    step run:
        action: cam_engage cam_xy
        action: cam_switch cam_xy t1
        action: cam_phase cam_xy phase + 1.0
        action: cam_disengage cam_xy
"#;

        let ast = parse_plc(input).expect("cam actions 应能解析");
        let step = &ast.tasks.tasks[0].steps[0];
        assert!(matches!(
            step.statements[0],
            StepStatement::Action(ActionStatement::CamEngage { .. })
        ));
        assert!(matches!(
            step.statements[1],
            StepStatement::Action(ActionStatement::CamSwitch { .. })
        ));
        assert!(matches!(
            step.statements[2],
            StepStatement::Action(ActionStatement::CamPhase { .. })
        ));
        assert!(matches!(
            step.statements[3],
            StepStatement::Action(ActionStatement::CamDisengage { .. })
        ));
    }

    #[test]
    fn parses_axis_move_actions_with_fault_branches_into_ast() {
        let input = r#"
[topology]
device axis_x: stepper_motor

[constraints]

[tasks]
task motion:
    step start:
        action: axis.move_relative(axis_x, distance: 10, params: stepper_default_fast, speed: 2)
            timeout: 500ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
        action: axis.move_absolute(axis_x, position: 120, speed: 5, acc: 20, dec: 20)
            timeout: 800ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
task fault:
    step timeout:
    step reject:
    step motion_fault:
    step safety_fault:
"#;

        let ast = parse_plc(input).expect("axis move 语句应能解析");
        let step = &ast.tasks.tasks[0].steps[0];
        assert_eq!(step.statements.len(), 2);

        match &step.statements[0] {
            StepStatement::Action(ActionStatement::AxisMoveRelative {
                target,
                params,
                distance,
                speed,
                acceleration,
                deceleration,
                timeout,
                on_reject,
                on_motion_fault,
                on_safety_fault,
                on_reject_routes,
                on_motion_fault_routes,
                on_safety_fault_routes,
                semantic_tag: _,
            }) => {
                assert_eq!(target.device, "axis_x");
                assert_eq!(params.as_deref(), Some("stepper_default_fast"));
                assert!((*distance - 10.0).abs() < f64::EPSILON);
                assert_eq!(*speed, Some(2.0));
                assert_eq!(*acceleration, None);
                assert_eq!(*deceleration, None);
                assert_eq!(timeout.as_ref().map(|v| v.duration.value), Some(500));
                assert_eq!(
                    timeout.as_ref().map(|v| v.target.task.as_str()),
                    Some("fault")
                );
                assert_eq!(
                    timeout.as_ref().and_then(|v| v.target.step.as_deref()),
                    Some("timeout")
                );
                assert_eq!(on_reject.as_ref().map(|v| v.task.as_str()), Some("fault"));
                assert_eq!(
                    on_reject.as_ref().and_then(|v| v.step.as_deref()),
                    Some("reject")
                );
                assert_eq!(
                    on_motion_fault.as_ref().and_then(|v| v.step.as_deref()),
                    Some("motion_fault")
                );
                assert_eq!(
                    on_safety_fault.as_ref().and_then(|v| v.step.as_deref()),
                    Some("safety_fault")
                );
                assert!(on_reject_routes.is_empty());
                assert!(on_motion_fault_routes.is_empty());
                assert!(on_safety_fault_routes.is_empty());
            }
            other => panic!("期望 AxisMoveRelative，实际: {other:?}"),
        }

        assert!(matches!(
            &step.statements[1],
            StepStatement::Action(ActionStatement::AxisMoveAbsolute { .. })
        ));
    }

    #[test]
    fn parses_cylinder_motion_action_with_fault_branches_into_ast() {
        let input = r#"
[topology]
device cyl_A: cylinder

[constraints]

[tasks]
task motion:
    step start:
        action: extend cyl_A
            timeout: 500ms -> goto fault.timeout
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
task fault:
    step timeout:
    step motion_fault:
    step safety_fault:
"#;

        let ast = parse_plc(input).expect("cylinder motion 带故障分支语句应能解析");
        let step = &ast.tasks.tasks[0].steps[0];
        assert_eq!(step.statements.len(), 1);
        match &step.statements[0] {
            StepStatement::Action(ActionStatement::Extend {
                target,
                timeout,
                on_motion_fault,
                on_safety_fault,
            }) => {
                assert_eq!(target.device, "cyl_A");
                assert_eq!(
                    timeout.as_ref().map(|t| t.target.task.as_str()),
                    Some("fault")
                );
                assert_eq!(
                    on_motion_fault.as_ref().and_then(|v| v.step.as_deref()),
                    Some("motion_fault")
                );
                assert_eq!(
                    on_safety_fault.as_ref().and_then(|v| v.step.as_deref()),
                    Some("safety_fault")
                );
            }
            other => panic!("expected extend action with fault branches, got {other:?}"),
        }
    }

    #[test]
    fn parses_semantic_resource_claims_and_axis_semantic_tag() {
        let input = r#"
[topology]
device axis_x: stepper_motor
device cyl_feed: cylinder

resource slide_pick_zone: semantic_resource {
    mode: exclusive
    purpose: "slide pick area"
}

[constraints]

claim: cyl_feed.extended occupies slide_pick_zone
claim: action_tag arm_pick_to_slide occupies slide_pick_zone

[tasks]
task motion:
    step start:
        action: axis.move_relative(axis_x, distance: 10, speed: 2)
            timeout: 500ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
            semantic_tag: arm_pick_to_slide
task fault:
    step timeout:
    step reject:
    step motion_fault:
    step safety_fault:
"#;

        let ast = parse_plc(input).expect("fixture should parse");
        assert_eq!(ast.topology.semantic_resources.len(), 1);
        assert_eq!(ast.topology.semantic_resources[0].name, "slide_pick_zone");
        assert_eq!(ast.constraints.claims.len(), 2);

        match &ast.tasks.tasks[0].steps[0].statements[0] {
            StepStatement::Action(ActionStatement::AxisMoveRelative { semantic_tag, .. }) => {
                assert_eq!(semantic_tag.as_deref(), Some("arm_pick_to_slide"));
            }
            other => panic!("expected AxisMoveRelative, got {other:?}"),
        }
    }

    #[test]
    fn parses_axis_move_when_fault_branch_is_missing() {
        let input = r#"
[topology]
device axis_x: stepper_motor

[constraints]

[tasks]
task motion:
    step start:
        action: axis.move_relative(axis_x, distance: 10, speed: 2)
            timeout: 500ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
task fault:
    step timeout:
    step reject:
    step motion_fault:
"#;

        let ast = parse_plc(input).expect("缺失分支应在语义阶段校验，不应在 parser 阶段失败");
        let step = &ast.tasks.tasks[0].steps[0];
        match &step.statements[0] {
            StepStatement::Action(ActionStatement::AxisMoveRelative {
                timeout,
                on_reject,
                on_motion_fault,
                on_safety_fault,
                ..
            }) => {
                assert!(timeout.is_some());
                assert!(on_reject.is_some());
                assert!(on_motion_fault.is_some());
                assert!(on_safety_fault.is_none());
            }
            other => panic!("期望 AxisMoveRelative，实际: {other:?}"),
        }
    }

    #[test]
    fn parses_axis_move_with_refined_fault_routes() {
        let input = r#"
[topology]
device axis_x: stepper_motor

[constraints]

[tasks]
task motion:
    step start:
        action: axis.move_relative(axis_x, distance: 10, speed: 2)
            timeout: 500ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_default
            on_motion_fault(kind: vendor) -> fault.motion_vendor
            on_motion_fault(code: 17) -> fault.motion_code_17
            on_safety_fault -> fault.safety_default
task fault:
    step timeout:
    step reject:
    step motion_default:
    step motion_vendor:
    step motion_code_17:
    step safety_default:
"#;

        let ast = parse_plc(input).expect("细分 axis fault routes 应能解析");
        let step = &ast.tasks.tasks[0].steps[0];
        match &step.statements[0] {
            StepStatement::Action(ActionStatement::AxisMoveRelative {
                on_reject,
                on_motion_fault,
                on_motion_fault_routes,
                on_safety_fault,
                ..
            }) => {
                assert_eq!(
                    on_reject.as_ref().and_then(|v| v.step.as_deref()),
                    Some("reject")
                );
                assert_eq!(
                    on_motion_fault.as_ref().and_then(|v| v.step.as_deref()),
                    Some("motion_default")
                );
                assert_eq!(
                    on_safety_fault.as_ref().and_then(|v| v.step.as_deref()),
                    Some("safety_default")
                );
                assert_eq!(on_motion_fault_routes.len(), 2);

                assert_eq!(
                    on_motion_fault_routes[0].kind,
                    Some(crate::ast::AxisFaultRouteKind::Vendor)
                );
                assert_eq!(on_motion_fault_routes[0].code, None);
                assert_eq!(
                    on_motion_fault_routes[0].target.step.as_deref(),
                    Some("motion_vendor")
                );

                assert_eq!(on_motion_fault_routes[1].kind, None);
                assert_eq!(on_motion_fault_routes[1].code, Some(17));
                assert_eq!(
                    on_motion_fault_routes[1].target.step.as_deref(),
                    Some("motion_code_17")
                );
            }
            other => panic!("期望 AxisMoveRelative，实际: {other:?}"),
        }
    }

    #[test]
    fn rejects_axis_move_duplicate_primary_fault_bucket_branch() {
        let input = r#"
[topology]
device axis_x: stepper_motor

[constraints]

[tasks]
task motion:
    step start:
        action: axis.move_relative(axis_x, distance: 10, speed: 2)
            timeout: 500ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_default
            on_motion_fault -> fault.motion_other
            on_safety_fault -> fault.safety_default
task fault:
    step timeout:
    step reject:
    step motion_default:
    step motion_other:
    step safety_default:
"#;

        let err = parse_plc(input).expect_err("重复主桶分支应在 parser 阶段失败");
        assert!(err.to_string().contains("on_motion_fault 主桶分支重复声明"));
    }

    #[test]
    fn parses_axis_move_with_params_reference_and_partial_overrides() {
        let input = r#"
[topology]
device axis_x: stepper_motor

[constraints]

[tasks]
task motion:
    step start:
        action: axis.move_relative(axis_x, distance: 10, params: stepper_default_fast, speed: 2)
            timeout: 500ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
task fault:
    step timeout:
    step reject:
    step motion_fault:
    step safety_fault:
"#;

        let ast = parse_plc(input).expect("params + override 语法应能解析");
        let step = &ast.tasks.tasks[0].steps[0];
        match &step.statements[0] {
            StepStatement::Action(ActionStatement::AxisMoveRelative {
                params,
                speed,
                acceleration,
                deceleration,
                ..
            }) => {
                assert_eq!(params.as_deref(), Some("stepper_default_fast"));
                assert_eq!(*speed, Some(2.0));
                assert_eq!(*acceleration, None);
                assert_eq!(*deceleration, None);
            }
            other => panic!("期望 AxisMoveRelative，实际: {other:?}"),
        }
    }

    #[test]
    fn rejects_axis_move_with_unknown_argument_field() {
        let input = r#"
[topology]
device axis_x: stepper_motor

[constraints]

[tasks]
task motion:
    step start:
        action: axis.move_relative(axis_x, distance: 10, speed: 2, jerk: 1)
            timeout: 500ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
task fault:
    step timeout:
    step reject:
    step motion_fault:
    step safety_fault:
"#;

        let err = parse_plc(input).expect_err("未知 axis.move 字段应在 parser 阶段失败");
        let message = err.to_string();
        assert!(
            message.contains("[AXIS-013]"),
            "应包含稳定错误码 [AXIS-013]，实际: {message}"
        );
        assert!(
            message.contains("jerk"),
            "应包含未知字段名 jerk，实际: {message}"
        );
    }

    #[test]
    fn rejects_axis_move_with_alias_argument_field_using_stable_code() {
        let input = r#"
[topology]
device axis_x: stepper_motor

[constraints]

[tasks]
task motion:
    step start:
        action: axis.move_absolute(axis_x, position: 100, vel: 5)
            timeout: 500ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
task fault:
    step timeout:
    step reject:
    step motion_fault:
    step safety_fault:
"#;

        let err = parse_plc(input).expect_err("别名字段 vel 应在 parser 阶段失败");
        let message = err.to_string();
        assert!(message.contains("[AXIS-013]"));
        assert!(message.contains("vel"));
    }

    #[test]
    fn parses_compute_and_set_analog_expression_actions() {
        let input = r#"
[topology]
device ao0: analog_output { range: 0..100 }
variable x: float = 1.0
variable y: float = 2.0

[constraints]

[tasks]
task main:
    step calc:
        action: compute x = x + y * 2
        action: set_analog ao0 x + 1
        action: compute y = clamp(abs(x), 0, 10)
"#;

        let ast = parse_plc(input).expect("表达式 action 应能解析");
        let step = &ast.tasks.tasks[0].steps[0];
        assert_eq!(step.statements.len(), 3);

        match &step.statements[0] {
            StepStatement::Action(ActionStatement::Compute { target, expr }) => {
                assert_eq!(target, "x");
                match expr {
                    Expression::BinaryOp {
                        op: BinaryOperator::Add,
                        ..
                    } => {}
                    other => panic!("compute 表达式应为加法根节点，实际: {other:?}"),
                }
            }
            other => panic!("期望 compute action，实际: {other:?}"),
        }

        match &step.statements[1] {
            StepStatement::Action(ActionStatement::SetAnalogExpr { target, expr }) => {
                assert_eq!(target.device, "ao0");
                match expr {
                    Expression::BinaryOp {
                        op: BinaryOperator::Add,
                        ..
                    } => {}
                    other => panic!("set_analog 表达式应为加法根节点，实际: {other:?}"),
                }
            }
            other => panic!("期望 set_analog_expr action，实际: {other:?}"),
        }

        match &step.statements[2] {
            StepStatement::Action(ActionStatement::Compute { target, expr }) => {
                assert_eq!(target, "y");
                match expr {
                    Expression::FunctionCall { name, args } => {
                        assert_eq!(name, "clamp");
                        assert_eq!(args.len(), 3);
                    }
                    other => panic!("期望 clamp 函数调用，实际: {other:?}"),
                }
            }
            other => panic!("期望 compute(clamp) action，实际: {other:?}"),
        }
    }

    #[test]
    fn parses_compute_boolean_literals_into_expression_literals() {
        let input = r#"
[topology]
variable flag: bool = false

[constraints]

[tasks]
task main:
    step calc:
        action: compute flag = true
        action: compute flag = false
"#;

        let ast = parse_plc(input).expect("boolean compute literals should parse");
        let step = &ast.tasks.tasks[0].steps[0];
        assert_eq!(step.statements.len(), 2);

        match &step.statements[0] {
            StepStatement::Action(ActionStatement::Compute { target, expr }) => {
                assert_eq!(target, "flag");
                assert!(matches!(expr, Expression::Boolean(true)));
            }
            other => panic!("expected compute action, got {other:?}"),
        }

        match &step.statements[1] {
            StepStatement::Action(ActionStatement::Compute { target, expr }) => {
                assert_eq!(target, "flag");
                assert!(matches!(expr, Expression::Boolean(false)));
            }
            other => panic!("expected compute action, got {other:?}"),
        }
    }

    #[test]
    fn parses_compute_boolean_expression_with_logical_and_comparison_ops() {
        let input = r#"
[topology]
variable flag: bool = false
variable a: bool = false
variable b: bool = true
variable x: float = 0.0

[constraints]

[tasks]
task main:
    step calc:
        action: compute flag = NOT a OR (b AND x > 0)
"#;

        let ast = parse_plc(input).expect("boolean expression compute should parse");
        let step = &ast.tasks.tasks[0].steps[0];
        let StepStatement::Action(ActionStatement::Compute { expr, .. }) = &step.statements[0]
        else {
            panic!("expected compute action");
        };

        let Expression::BinaryOp { op, left, right } = expr else {
            panic!("top-level expression should be binary OR");
        };
        assert!(matches!(op, BinaryOperator::Or));
        assert!(matches!(left.as_ref(), Expression::UnaryNot(_)));
        let Expression::BinaryOp {
            op: right_op,
            left: and_left,
            right: and_right,
        } = right.as_ref()
        else {
            panic!("right side should be binary AND");
        };
        assert!(matches!(right_op, BinaryOperator::And));
        assert!(matches!(and_left.as_ref(), Expression::Variable(name) if name == "b"));
        assert!(matches!(
            and_right.as_ref(),
            Expression::BinaryOp {
                op: BinaryOperator::Gt,
                ..
            }
        ));
    }

    #[test]
    fn parses_wait_and_or_conditions_and_rejects_mixed() {
        let and_input = r#"
[topology]
device sensor_A: sensor
device sensor_B: sensor

[constraints]

[tasks]
task main:
    step wait_all:
        wait: sensor_A == true AND sensor_B == true
"#;

        let ast = parse_plc(and_input).expect("AND wait 应能解析");
        let step = &ast.tasks.tasks[0].steps[0];
        match &step.statements[0] {
            StepStatement::Wait(wait) => match &wait.condition {
                WaitCondition::And(conditions) => {
                    assert_eq!(conditions.len(), 2);
                    assert_eq!(conditions[0].left, "sensor_A");
                    assert_eq!(conditions[1].left, "sensor_B");
                }
                other => panic!("期望 And 条件，实际为: {other:?}"),
            },
            other => panic!("期望 wait 语句，实际为: {other:?}"),
        }

        let or_input = r#"
[topology]
device sensor_A: sensor
device sensor_B: sensor

[constraints]

[tasks]
task main:
    step wait_any:
        wait: sensor_A == true OR sensor_B == true
"#;

        let ast = parse_plc(or_input).expect("OR wait 应能解析");
        let step = &ast.tasks.tasks[0].steps[0];
        match &step.statements[0] {
            StepStatement::Wait(wait) => match &wait.condition {
                WaitCondition::Or(conditions) => {
                    assert_eq!(conditions.len(), 2);
                    assert_eq!(conditions[0].left, "sensor_A");
                    assert_eq!(conditions[1].left, "sensor_B");
                }
                other => panic!("期望 Or 条件，实际为: {other:?}"),
            },
            other => panic!("期望 wait 语句，实际为: {other:?}"),
        }

        let single_input = r#"
[topology]
device sensor_A: sensor

[constraints]

[tasks]
task main:
    step wait_one:
        wait: sensor_A == true
"#;

        let ast = parse_plc(single_input).expect("单条件 wait 应能解析");
        let step = &ast.tasks.tasks[0].steps[0];
        match &step.statements[0] {
            StepStatement::Wait(wait) => assert!(
                matches!(wait.condition, WaitCondition::Single(_)),
                "单条件 wait 应降级为 Single 变体"
            ),
            other => panic!("期望 wait 语句，实际为: {other:?}"),
        }

        let mixed_input = r#"
[topology]
device sensor_A: sensor
device sensor_B: sensor
device sensor_C: sensor

[constraints]

[tasks]
task main:
    step wait_mixed:
        wait: sensor_A == true AND sensor_B == true OR sensor_C == true
"#;

        let err = parse_plc(mixed_input).expect_err("混用 AND/OR 应被拒绝");
        assert!(
            err.to_string().contains("混用 AND/OR"),
            "应提示 AND/OR 混用错误"
        );
    }

    #[test]
    fn parses_edge_wait_conditions() {
        let input = r#"
[topology]
device start_button: sensor
device reset_button: sensor

[constraints]

[tasks]
task main:
    step wait_start:
        wait: rising_edge(start_button)

    step wait_reset:
        wait: falling_edge(reset_button)
"#;

        let ast = parse_plc(input).expect("edge wait should parse");
        let first = &ast.tasks.tasks[0].steps[0].statements[0];
        match first {
            StepStatement::Wait(wait) => match &wait.condition {
                WaitCondition::Edge(edge) => {
                    assert_eq!(edge.edge, crate::ast::EdgeKind::Rising);
                    assert_eq!(edge.operand, "start_button");
                }
                other => panic!("expected rising edge wait, got {other:?}"),
            },
            other => panic!("expected wait statement, got {other:?}"),
        }

        let second = &ast.tasks.tasks[0].steps[1].statements[0];
        match second {
            StepStatement::Wait(wait) => match &wait.condition {
                WaitCondition::Edge(edge) => {
                    assert_eq!(edge.edge, crate::ast::EdgeKind::Falling);
                    assert_eq!(edge.operand, "reset_button");
                }
                other => panic!("expected falling edge wait, got {other:?}"),
            },
            other => panic!("expected wait statement, got {other:?}"),
        }
    }

    #[test]
    fn parses_expression_conditions_in_wait_and_if() {
        let input = r#"
[topology]
variable master_pos: float = 0.0
variable slave_pos: float = 0.0

[constraints]

[tasks]
task main:
    step s1:
        wait: abs(master_pos - slave_pos) < 0.5
        if: (master_pos + 1.0) >= (slave_pos * 2.0)
            goto done
        else:
            goto main.s1

task done:
    step halt:
"#;

        let ast = parse_plc(input).expect("表达式条件应能解析");
        let statements = &ast.tasks.tasks[0].steps[0].statements;
        match &statements[0] {
            StepStatement::Wait(wait) => match &wait.condition {
                WaitCondition::Single(condition) => {
                    assert!(
                        condition.expression_pair().is_some(),
                        "wait 条件应为表达式比较"
                    );
                }
                other => panic!("期望单条件 wait，实际: {other:?}"),
            },
            other => panic!("期望 wait 语句，实际: {other:?}"),
        }

        match &statements[1] {
            StepStatement::IfElse { condition, .. } => {
                assert!(
                    condition.expression_pair().is_some(),
                    "if 条件应为表达式比较"
                );
            }
            other => panic!("期望 if/else 语句，实际: {other:?}"),
        }
    }

    #[test]
    fn parses_if_else_statement_into_ast() {
        let input = r#"
[topology]
device switch_A: digital_input

[constraints]

[tasks]

task main:
    step choose:
        if: switch_A == true
            goto grind_coarse
        else:
            goto grind_fine
"#;

        let program = parse_plc(input).expect("if/else 示例应能解析为 AST");
        let step = &program.tasks.tasks[0].steps[0];
        let statement = step.statements.first().expect("step 应包含语句");

        match statement {
            StepStatement::IfElse {
                condition,
                then_goto,
                else_goto,
            } => {
                assert_eq!(condition.left, "switch_A");
                assert_eq!(then_goto.task, "grind_coarse");
                assert!(then_goto.step.is_none());
                assert_eq!(else_goto.task, "grind_fine");
                assert!(else_goto.step.is_none());
            }
            other => panic!("期望 IfElse 语句，实际为: {other:?}"),
        }
    }

    #[test]
    fn parses_goto_task_step_statement_into_ast() {
        let input = r#"
[topology]

[constraints]

[tasks]

task cycle:
    step press_down:
        action: log "press"

task main:
    step start:
        goto cycle.press_down
"#;

        let program = parse_plc(input).expect("goto task.step 示例应能解析");
        let step = &program
            .tasks
            .tasks
            .iter()
            .find(|task| task.name == "main")
            .expect("应包含 main task")
            .steps[0];

        match step.statements.first() {
            Some(StepStatement::Goto(goto)) => {
                assert_eq!(goto.task, "cycle");
                assert_eq!(goto.step.as_deref(), Some("press_down"));
            }
            other => panic!("期望 goto 语句，实际为: {other:?}"),
        }
    }

    #[test]
    fn rejects_if_without_else_branch() {
        let input = r#"
[topology]
device switch_A: digital_input

[constraints]

[tasks]

task main:
    step choose:
        if: switch_A == true
            goto grind_coarse
"#;

        assert!(parse_plc(input).is_err(), "缺少 else 分支时应报解析错误");
    }

    #[test]
    fn parses_station_handshake_and_transfer_point_into_topology() {
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
    step idle:
task press_cycle:
    step idle:
task fault:
    step timeout:
"#;

        let program = parse_plc(input).expect("station protocol should parse");
        assert_eq!(program.topology.stations.len(), 2);
        assert_eq!(program.topology.handshakes.len(), 1);
        assert_eq!(program.topology.transfer_points.len(), 1);
        assert_eq!(program.topology.stations[0].owns, vec!["cyl_load"]);
        assert_eq!(program.topology.handshakes[0].timeout.target.task, "fault");
        assert_eq!(
            program.topology.transfer_points[0].handshake,
            "st01_to_st02"
        );
    }
}
