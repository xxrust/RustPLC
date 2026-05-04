#[derive(Debug, Clone, Default)]
struct WaitExpressionContext {
    analog_input_regions: HashMap<String, Vec<(f64, f64)>>,
}

impl WaitExpressionContext {
    fn for_program(program: &PlcProgram) -> Self {
        Self {
            analog_input_regions: compute_analog_input_regions(program),
        }
    }
}

fn compute_analog_input_regions(program: &PlcProgram) -> HashMap<String, Vec<(f64, f64)>> {
    let mut values_by_device: HashMap<String, Vec<f64>> = HashMap::new();

    for constraint in &program.constraints.safety {
        for operand in [&constraint.left, &constraint.right] {
            if let SafetyOperand::Threshold { device, value, .. } = operand {
                values_by_device
                    .entry(device.clone())
                    .or_default()
                    .push(*value);
            }
        }
    }

    for task in &program.tasks.tasks {
        for step in &task.steps {
            collect_threshold_values_from_statements(&step.statements, &mut values_by_device);
        }
    }

    let mut regions_by_device = HashMap::new();
    for device in &program.topology.devices {
        if !matches!(device.device_type, DeviceType::AnalogInput) {
            continue;
        }

        let Some(range) = &device.attributes.range else {
            continue;
        };

        let (min, max) = if range.min <= range.max {
            (range.min, range.max)
        } else {
            (range.max, range.min)
        };

        let mut bounds = vec![min, max];
        if let Some(values) = values_by_device.get(&device.name) {
            for value in values {
                if *value >= min && *value <= max {
                    bounds.push(*value);
                }
            }
        }

        bounds.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        bounds.dedup_by(|a, b| (*a - *b).abs() <= f64::EPSILON);

        let mut regions = Vec::new();
        for window in bounds.windows(2) {
            regions.push((window[0], window[1]));
        }
        if regions.is_empty() {
            regions.push((min, max));
        }

        regions_by_device.insert(device.name.clone(), regions);
    }

    regions_by_device
}

fn collect_threshold_values_from_statements(
    statements: &[StepStatement],
    values_by_device: &mut HashMap<String, Vec<f64>>,
) {
    for statement in statements {
        match statement {
            StepStatement::Wait(wait) => {
                let terms: Vec<&ConditionExpression> = match &wait.condition {
                    WaitCondition::Single(condition) => vec![condition],
                    WaitCondition::And(conditions) | WaitCondition::Or(conditions) => {
                        conditions.iter().collect()
                    }
                    WaitCondition::Edge(_) => Vec::new(),
                };

                for condition in terms {
                    if condition.is_expression_compare() {
                        continue;
                    }
                    if let LiteralValue::Number(value) = &condition.right {
                        if let Some(device_name) = wait_operand_device_name(&condition.left) {
                            values_by_device
                                .entry(device_name.to_string())
                                .or_default()
                                .push(*value);
                        }
                    }
                    if let LiteralValue::Measured(measured) = &condition.right {
                        if let Some(device_name) = wait_operand_device_name(&condition.left) {
                            values_by_device
                                .entry(device_name.to_string())
                                .or_default()
                                .push(measured.value);
                        }
                    }
                }
            }
            StepStatement::Repeat { body, .. } => {
                collect_threshold_values_from_statements(body, values_by_device);
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    collect_threshold_values_from_statements(&branch.statements, values_by_device);
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    collect_threshold_values_from_statements(&branch.statements, values_by_device);
                }
            }
            _ => {}
        }
    }
}

pub fn build_topology_from_ast(topology: &TopologySection) -> Result<TopologyGraph, Vec<PlcError>> {
    let mut topology_graph = TopologyGraph::new();
    let mut device_nodes = HashMap::<String, DeviceNode>::new();
    let mut errors = Vec::new();
    let pid_loops = extract_pid_loops(topology, &mut errors);
    let variable_defs = extract_variable_defs(topology, &mut errors);
    let cam_table_defs = extract_cam_table_defs(topology, &mut errors);
    let extern_function_defs = extract_extern_function_defs(topology);
    let cam_table_names = cam_table_defs
        .iter()
        .map(|table| table.name.clone())
        .collect::<HashSet<_>>();

    for device in &topology.devices {
        let kind = ast_type_to_ir_kind(&device.device_type);
        let index = topology_graph.add_device(Device {
            name: device.name.clone(),
            kind: kind.clone(),
        });

        device_nodes.insert(device.name.clone(), DeviceNode { index, kind });

        // Analog devices must declare range
        if matches!(
            device.device_type,
            DeviceType::AnalogInput | DeviceType::AnalogOutput
        ) && device.attributes.range.is_none()
        {
            errors.push(PlcError::semantic_with_reason(
                device.line,
                format!("模拟量设备 {} 必须声明 range 属性", device.name),
                "请添加 range: min..max 属性，例如 range: 0..100",
            ));
        }
    }

    match resolve_axis_profiles(&topology.devices) {
        Ok(profiles) => {
            topology_graph.axis_profiles = profiles;
        }
        Err(mut axis_errors) => errors.append(&mut axis_errors),
    }

    let axis_fault_contract_defs =
        extract_axis_fault_contract_defs(topology, &device_nodes, &mut errors);

    for connection in semantic_topology_connections(topology) {
        let line = topology_connection_line(topology, &connection);
        let context = topology_connection_context(&connection);

        let Some(from_node) = device_nodes.get(&connection.from) else {
            errors.push(PlcError::undefined_reference_with_reason(
                line,
                "设备",
                &connection.from,
                format!("{context} 引用了该名称，请先定义后再连接"),
            ));
            continue;
        };

        let Some(to_node) = device_nodes.get(&connection.to) else {
            errors.push(PlcError::undefined_reference_with_reason(
                line,
                "设备",
                &connection.to,
                format!("{context} 指向了未定义设备，请先定义后再连接"),
            ));
            continue;
        };

        let Some(connection_type) =
            connection_type_for_relation(&connection.relation, &from_node.kind, &to_node.kind)
        else {
            errors.push(PlcError::type_mismatch_with_reason(
                line,
                format!("{} 可连接的 consumer 设备", device_kind_name(&from_node.kind)),
                device_kind_name(&to_node.kind),
                context,
                format!(
                    "`{}` 关系要求 producer -> consumer，当前为 {}({}) -> {}({})，请调整设备类型或连接方向",
                    topology_relation_name(&connection.relation),
                    connection.from,
                    device_kind_name(&from_node.kind),
                    connection.to,
                    device_kind_name(&to_node.kind)
                ),
            ));
            continue;
        };

        topology_graph.add_connection(from_node.index, to_node.index, connection_type.clone());
        topology_graph.links.push(TopologyLink {
            from: connection.from.clone(),
            to: connection.to.clone(),
            from_port: connection.from_port.clone(),
            to_port: connection.to_port.clone(),
            kind: connection_type,
        });
    }

    let cam_couplings = extract_cam_coupling_defs(topology, &cam_table_names, &mut errors);
    for coupling in &cam_couplings {
        let Some(cam_node) = device_nodes.get(&coupling.name) else {
            continue;
        };
        if let Some(master_node) = device_nodes.get(&coupling.master) {
            topology_graph.add_connection(
                master_node.index,
                cam_node.index,
                ConnectionType::Analog,
            );
            topology_graph.links.push(TopologyLink {
                from: coupling.master.clone(),
                to: coupling.name.clone(),
                from_port: None,
                to_port: Some("master_pos".to_string()),
                kind: ConnectionType::Analog,
            });
        }
        if let Some(slave_node) = device_nodes.get(&coupling.slave) {
            topology_graph.add_connection(cam_node.index, slave_node.index, ConnectionType::Analog);
            topology_graph.links.push(TopologyLink {
                from: coupling.name.clone(),
                to: coupling.slave.clone(),
                from_port: Some("slave_cmd".to_string()),
                to_port: None,
                kind: ConnectionType::Analog,
            });
        }
    }

    topology_graph.pid_loops = pid_loops;
    topology_graph.variables = variable_defs;
    topology_graph.cam_tables = cam_table_defs;
    topology_graph.cam_couplings = cam_couplings;
    topology_graph.extern_functions = extern_function_defs;
    topology_graph.axis_fault_contracts = axis_fault_contract_defs;

    if errors.is_empty() {
        Ok(topology_graph)
    } else {
        Err(errors)
    }
}

