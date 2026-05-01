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
fn source_topology_gate_rejects_raw_io_devices_before_ir_builders() {
    let input = r#"
[topology]
device plc_main: plc { model_ref: openplc_softplc }
device Y0: digital_output { purpose: "raw coil" }

[constraints]

[tasks]
task main:
    step idle:
"#;

    let program = parse_plc(input).expect("parse");
    let direct_errors =
        validate_source_topology_semantics(&program).expect_err("source gate should reject raw IO");
    let builder_errors =
        build_state_machine(&program).expect_err("state machine builder should run source gate");
    let rendered = direct_errors
        .iter()
        .chain(builder_errors.iter())
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        rendered.contains("SEM-108") && rendered.contains("digital_output"),
        "expected SEM-108 raw IO rejection, got: {rendered}"
    );
}

#[test]
fn source_state_machine_entry_rejects_raw_axis_port_bypass() {
    let input = r#"
[topology]
device plc_main: plc { model_ref: openplc_softplc }
device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: default }

[constraints]

[tasks]
task main:
    step jog:
        action: set axis_x.pulse on
"#;

    let program = parse_plc(input).expect("parse");
    let errors = build_state_machine(&program).expect_err("raw axis pulse should be rejected");
    let rendered = errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        rendered.contains("SEM-110") && rendered.contains("axis_x.pulse"),
        "expected raw axis port bypass rejection, got: {rendered}"
    );
}

#[test]
fn source_state_machine_entry_rejects_raw_controller_port_bypass() {
    let input = r#"
[topology]
device plc_main: plc { model_ref: openplc_softplc }

[constraints]

[tasks]
task main:
    step start:
        action: set plc_main.Y0 on
"#;

    let program = parse_plc(input).expect("parse");
    let errors =
        build_state_machine(&program).expect_err("raw controller output should be rejected");
    let rendered = errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        rendered.contains("SEM-110") && rendered.contains("plc_main.Y0"),
        "expected raw controller port bypass rejection, got: {rendered}"
    );
}

#[test]
fn source_state_machine_entry_rejects_raw_valve_and_process_command_bypass() {
    let input = r#"
[topology]
device plc_main: plc { model_ref: openplc_softplc }
device valve_A: solenoid_valve
device heater_A: heater
device camera_A: vision_sensor

[constraints]

[tasks]
task main:
    step start:
        action: set valve_A.coil on
        action: set heater_A.power on
        action: set camera_A.trigger on
"#;

    let program = parse_plc(input).expect("parse");
    let errors =
        build_state_machine(&program).expect_err("raw device command ports should be rejected");
    let rendered = errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        rendered.contains("SEM-110")
            && rendered.contains("valve_A.coil")
            && rendered.contains("heater_A.power")
            && rendered.contains("camera_A.trigger"),
        "expected raw command port bypass rejections, got: {rendered}"
    );
}

#[test]
fn source_topology_gate_accepts_channel_bindings_not_io_devices() {
    let input = r#"
[topology]
device plc_main: plc { model_ref: openplc_softplc }
device pressure_sensor: sensor { ports: [out:analog:producer] }
device valve: proportional_valve

relation { from: pressure_sensor.out, to: plc_main.AI0, via: reports_to }
relation { from: plc_main.AO0, to: valve.cmd, via: driven_by }

[constraints]

[tasks]
task main:
    step idle:
"#;

    let program = parse_plc(input).expect("parse");
    validate_source_topology_semantics(&program).expect("channel bindings should pass source gate");
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
fn preprocess_with_library_injects_device_defaults_without_overriding_source() {
    let input = r#"
[topology]

device hand: gripper { ports: [cmd:digital:consumer] }
device oven: heater { response_time: 50ms, ports: [power:digital:consumer] }

[constraints]

[tasks]
task main:
    step idle:
        action: log "ok"
"#;

    let program = parse_plc(input).expect("parse");
    let library = DeviceLibrary::load(Path::new("devices")).expect("load device library");
    let expanded = preprocess_program_with_library(&program, Some(&library))
        .expect("device defaults should inject during preprocess");

    let hand = expanded
        .topology
        .devices
        .iter()
        .find(|device| device.name == "hand")
        .expect("hand device");
    let hand_response = hand
        .attributes
        .response_time
        .as_ref()
        .expect("gripper response_time default");
    assert_eq!(hand_response.value, 300);
    assert!(matches!(hand_response.unit, crate::ast::TimeUnit::Ms));
    assert_eq!(
        hand.attributes
            .ports
            .iter()
            .find(|port| port.id == "cmd")
            .map(|port| port.default_state.as_str()),
        Some("hold")
    );

    let oven = expanded
        .topology
        .devices
        .iter()
        .find(|device| device.name == "oven")
        .expect("oven device");
    let oven_response = oven
        .attributes
        .response_time
        .as_ref()
        .expect("authored heater response_time");
    assert_eq!(oven_response.value, 50);
    assert_eq!(
        oven.attributes
            .ports
            .iter()
            .find(|port| port.id == "power")
            .map(|port| port.default_state.as_str()),
        Some("off")
    );
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
fn preprocess_with_library_deduplicates_authored_safety_constraint() {
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

[tasks]
task main:
    step idle:
        action: log "ok"
"#;

    let program = parse_plc(input).expect("parse");
    let library = DeviceLibrary::load(Path::new("devices")).expect("load device library");
    let expanded = preprocess_program_with_library(&program, Some(&library))
        .expect("显式 safety 与设备库 safety 同义时应去重");

    let injected_matches = expanded
        .constraints
        .safety
        .iter()
        .filter(|rule| {
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
            ) && matches!(rule.relation, crate::ast::SafetyRelation::ConflictsWith)
        })
        .collect::<Vec<_>>();

    assert_eq!(
        injected_matches.len(),
        1,
        "同义 authored/device-library safety 只应保留一条"
    );
    assert!(
        injected_matches[0].line > 0,
        "应保留 authored 规则的源码行号，而不是设备库注入的 line=0"
    );
    assert!(
        injected_matches[0].source.is_none(),
        "同义规则去重后应保留 authored 条目，而不是 device:* 注入条目"
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
