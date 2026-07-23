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
    fn topology_graph_retains_station_protocol_ir() {
        let input = r#"
[topology]
device plc_a: plc { model_ref: openplc_softplc }
device plc_b: plc { model_ref: openplc_softplc }
device cyl_load: cylinder
device cyl_press: cylinder
workpiece part: workpiece_type {
    ingress_sites: [handoff]
}
site handoff: workpiece_location { capacity: 1 }

station st01 { owns: [plc_a, cyl_load], tasks: [load_cycle] }
station st02 { owns: [plc_b, cyl_press], tasks: [press_cycle] }
handshake st01_to_st02 {
    from: st01,
    to: st02,
    request: st01_request,
    allow: st02_allow,
    complete: st01_complete,
    timeout: 5s -> goto fault.timeout
}
transfer_point load_to_press {
    from_station: st01,
    to_station: st02,
    site: handoff,
    handshake: st01_to_st02
}
controller_sync plc_pair_sync {
    controllers: [plc_a, plc_b],
    max_skew: 5ms,
    heartbeat: 100ms
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
        build_state_machine(&program).expect("station protocol should pass semantic gates");
        let expanded = preprocess_program(&program).expect("preprocess should keep station metadata");
        let topology = build_topology_graph(&expanded).expect("topology should build");

        assert_eq!(
            topology.station_protocol.controllers,
            vec!["plc_a".to_string(), "plc_b".to_string()]
        );
        assert_eq!(topology.station_protocol.stations.len(), 2);
        assert_eq!(topology.station_protocol.stations[0].name, "st01");
        assert_eq!(
            topology.station_protocol.stations[0].owns,
            vec!["plc_a".to_string(), "cyl_load".to_string()]
        );
        assert_eq!(topology.station_protocol.handshakes.len(), 1);
        let handshake = &topology.station_protocol.handshakes[0];
        assert_eq!(handshake.name, "st01_to_st02");
        assert_eq!(handshake.from_station, "st01");
        assert_eq!(handshake.to_station, "st02");
        assert_eq!(handshake.timeout_ms, 5000);
        assert_eq!(handshake.timeout_target_task, "fault");
        assert_eq!(handshake.timeout_target_step.as_deref(), Some("timeout"));
        assert_eq!(topology.station_protocol.transfer_points.len(), 1);
        assert_eq!(
            topology.station_protocol.transfer_points[0].handshake,
            "st01_to_st02"
        );
        assert_eq!(topology.station_protocol.controller_syncs.len(), 1);
        let sync = &topology.station_protocol.controller_syncs[0];
        assert_eq!(sync.name, "plc_pair_sync");
        assert_eq!(
            sync.controllers,
            vec!["plc_a".to_string(), "plc_b".to_string()]
        );
        assert_eq!(sync.max_skew_ms, 5);
        assert_eq!(sync.heartbeat_ms, 100);

        let topology_json =
            serde_json::to_string(&topology).expect("topology should serialize");
        assert!(topology_json.contains("station_protocol"));
        assert!(topology_json.contains("st01_to_st02"));
        assert!(topology_json.contains("plc_pair_sync"));
    }

    #[test]
    fn controller_sync_contracts_are_semantically_validated() {
        let input = r#"
[topology]
device plc_a: plc { model_ref: openplc_softplc }
controller_sync bad_sync {
    controllers: [plc_a, plc_missing],
    max_skew: 20ms,
    heartbeat: 10ms
}

[constraints]

[tasks]
task main:
    step idle:
"#;

        let program = parse_plc(input).expect("controller_sync syntax should parse");
        let errors = build_state_machine(&program)
            .expect_err("invalid controller_sync contract should fail semantic validation");
        let rendered = errors
            .iter()
            .map(|error| format!("{error}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("[SEM-233]") && rendered.contains("plc_missing"),
            "expected undefined controller diagnostic, got: {rendered}"
        );
        assert!(
            rendered.contains("[SEM-236]"),
            "expected heartbeat/skew diagnostic, got: {rendered}"
        );
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

    #[test]
    fn rejects_duplicate_source_device_names_before_topology_lowering() {
        let input = r#"
[topology]
device duplicate_sensor: sensor
device duplicate_sensor: sensor

[constraints]

[tasks]
task main:
    step idle:
"#;

        let program = parse_plc(input).expect("duplicate names are syntactically valid");
        let errors = build_topology_graph(&program).expect_err("duplicate devices must fail");
        let rendered = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("duplicate_sensor"), "{rendered}");
        assert!(rendered.contains("duplicate"), "{rendered}");
    }

    #[test]
    fn topology_builder_matches_explicit_preprocessing() {
        let input = r#"
[topology]
device_template single<T> {
    device main: T { purpose: "templated sensor" }
}
device_instance station: single<sensor>

[constraints]

[tasks]
task main:
    step idle:
"#;

        let program = parse_plc(input).expect("template source should parse");
        let direct = build_topology_graph(&program).expect("public builder should preprocess");
        let expanded = preprocess_program(&program).expect("explicit preprocess should succeed");
        let explicit = build_topology_graph(&expanded).expect("expanded program should build");
        let direct_names = direct
            .graph
            .node_weights()
            .map(|device| device.name.clone())
            .collect::<Vec<_>>();
        let explicit_names = explicit
            .graph
            .node_weights()
            .map(|device| device.name.clone())
            .collect::<Vec<_>>();
        assert_eq!(direct_names, explicit_names);
        assert_eq!(direct_names, vec!["station_main"]);
    }

