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
    fn topology_extracts_pid_loop_from_plc_channel_bindings_without_io_devices() {
        let input = r#"
[topology]
device plc_main: plc { model_ref: openplc_softplc }
device pressure_sensor: sensor { ports: [out:analog:producer] }
device valve: proportional_valve
device loop_pressure: pid {
    pv: AI0,
    sp: 0.5raw,
    kp: 2.0,
    ki: 0.4,
    kd: 0.05,
    out: AO0,
    period_ms: 100,
    limit: 0..1
}

relation { from: pressure_sensor.out, to: plc_main.AI0, via: reports_to }
relation { from: plc_main.AO0, to: valve.cmd, via: driven_by }

[constraints]

[tasks]
task main:
    step hold:
"#;

        let program = parse_plc(input).expect("parse");
        validate_source_topology_semantics(&program)
            .expect("pid source should use channel bindings, not IO devices");
        let expanded = preprocess_program(&program).expect("controller ports should expand");
        let topology = build_topology_graph(&expanded).expect("build topology");

        assert_eq!(topology.pid_loops.len(), 1);
        let pid = &topology.pid_loops[0];
        assert_eq!(pid.name, "loop_pressure");
        assert_eq!(pid.pv, "AI0");
        assert_eq!(pid.out, "AO0");
        assert_eq!(pid.period_ms, 100);
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

