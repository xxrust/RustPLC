pub fn build_topology_graph(program: &PlcProgram) -> Result<TopologyGraph, Vec<PlcError>> {
    build_topology_from_ast(&program.topology)
}

pub fn build_state_machine(program: &PlcProgram) -> Result<StateMachine, Vec<PlcError>> {
    let mut expanded = preprocess_program(program)?;
    let variable_types = collect_variable_types(&expanded.topology);
    let mut expr_errors = Vec::new();
    validate_expression_actions_in_tasks(&expanded.tasks, &variable_types, &mut expr_errors);
    let extern_signatures =
        collect_extern_function_signatures(&expanded.topology, &mut expr_errors);
    validate_extern_calls_in_tasks(
        &expanded.tasks,
        &extern_signatures,
        &variable_types,
        &mut expr_errors,
    );
    validate_non_pure_extern_concurrency_in_tasks(
        &expanded.tasks,
        &extern_signatures,
        &mut expr_errors,
    );
    let device_kinds = collect_device_kinds(&expanded.topology);
    let cam_table_names = expanded
        .topology
        .cam_tables
        .iter()
        .map(|table| table.name.clone())
        .collect::<HashSet<_>>();
    validate_cam_actions_in_tasks(
        &expanded.tasks,
        &device_kinds,
        &cam_table_names,
        &mut expr_errors,
    );
    validate_axis_motion_actions_in_tasks(&expanded.tasks, &device_kinds, &mut expr_errors);
    device_semantics::validate_task_action_semantics(
        &expanded.tasks,
        &device_kinds,
        &mut expr_errors,
    );
    resolve_axis_motion_parameters_in_tasks(
        &mut expanded.tasks,
        &expanded.topology,
        &mut expr_errors,
    );
    validate_vertical_axis_brake_sequence_in_tasks(
        &expanded.tasks,
        &expanded.topology,
        &mut expr_errors,
    );
    let has_workpiece_context = !expanded.topology.workpiece_types.is_empty()
        || !expanded.topology.workpiece_sites.is_empty()
        || !expanded.topology.workpiece_holders.is_empty()
        || !expanded.topology.workpiece_carriers.is_empty()
        || tasks_use_workpiece_effects(&expanded.tasks);
    if has_workpiece_context {
        let mut workpiece_constraints = ConstraintSet::default();
        let catalog = validate_and_lower_workpiece_topology_v2(
            &expanded.topology,
            &mut workpiece_constraints,
            &mut expr_errors,
        );
        validate_workpiece_effects_in_tasks_v2(
            &expanded.tasks,
            &catalog,
            &workpiece_constraints.workpiece_types,
            &mut expr_errors,
        );
    }
    if !expr_errors.is_empty() {
        return Err(expr_errors);
    }
    let wait_ctx = WaitExpressionContext::for_program(&expanded);
    build_state_machine_from_ast_with_context(&expanded.tasks, &wait_ctx, Some(&device_kinds))
}

#[derive(Debug, Clone)]
struct ExternFunctionSignature {
    line: usize,
    param_types: Vec<AstVariableType>,
    return_types: Vec<AstVariableType>,
    pure: bool,
}

fn collect_variable_types(topology: &TopologySection) -> HashMap<String, AstVariableType> {
    topology
        .variables
        .iter()
        .map(|variable| (variable.name.clone(), variable.var_type.clone()))
        .collect()
}

fn collect_extern_function_signatures(
    topology: &TopologySection,
    errors: &mut Vec<PlcError>,
) -> HashMap<String, ExternFunctionSignature> {
    let mut signatures: HashMap<String, ExternFunctionSignature> = HashMap::new();

    for decl in &topology.extern_functions {
        let line = decl.line.max(1);
        validate_extern_function_contract(decl, errors);
        validate_extern_function_signature_types(decl, errors);
        if let Some(previous) = signatures.get(&decl.name) {
            errors.push(PlcError::duplicate_definition_with_reason(
                line,
                "extern 函数",
                &decl.name,
                format!(
                    "extern 函数 {} 已在第 {} 行声明，请保持函数签名唯一",
                    decl.name, previous.line
                ),
            ));
            continue;
        }

        signatures.insert(
            decl.name.clone(),
            ExternFunctionSignature {
                line,
                param_types: decl
                    .params
                    .iter()
                    .map(|param| param.var_type.clone())
                    .collect(),
                return_types: decl.return_types.clone(),
                pure: decl.contract.pure,
            },
        );
    }

    signatures
}

fn validate_extern_function_contract(
    decl: &AstExternFunctionDeclaration,
    errors: &mut Vec<PlcError>,
) {
    let line = decl.line.max(1);

    if decl.contract.rust_module.trim().is_empty() {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!("extern 函数 {} 的 rust_module 不能为空", decl.name),
            "请为 rust_module 设置非空字符串（例如 \"math::add\"）",
        ));
    }

    if decl.contract.time_bound_us == 0 {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "extern 函数 {} 的 time_bound_us 必须为正整数，当前为 0",
                decl.name
            ),
            "请将 time_bound_us 设置为大于 0 的整数值（单位：微秒）",
        ));
    }
}

fn validate_extern_function_signature_types(
    decl: &AstExternFunctionDeclaration,
    errors: &mut Vec<PlcError>,
) {
    let line = decl.line.max(1);

    for (index, param) in decl.params.iter().enumerate() {
        if !is_phase1_supported_extern_type(&param.var_type) {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!(
                    "extern 函数 {} 参数 #{} 使用了不支持的类型 {}",
                    decl.name,
                    index + 1,
                    ast_variable_type_name(&param.var_type)
                ),
                "Phase 1 仅支持标量类型：bool/int/float",
            ));
        }
    }

    for (index, return_type) in decl.return_types.iter().enumerate() {
        if !is_phase1_supported_extern_type(return_type) {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!(
                    "extern 函数 {} 返回值 #{} 使用了不支持的类型 {}",
                    decl.name,
                    index + 1,
                    ast_variable_type_name(return_type)
                ),
                "Phase 1 仅支持标量类型：bool/int/float",
            ));
        }
    }
}

fn is_phase1_supported_extern_type(var_type: &AstVariableType) -> bool {
    matches!(
        var_type,
        AstVariableType::Float | AstVariableType::Int | AstVariableType::Bool
    )
}

pub fn build_constraint_set(program: &PlcProgram) -> Result<ConstraintSet, Vec<PlcError>> {
    let expanded = preprocess_program(program)?;
    let mut errors = Vec::new();
    validate_vertical_axis_brake_sequence_in_tasks(
        &expanded.tasks,
        &expanded.topology,
        &mut errors,
    );
    match build_constraint_set_from_ast(&expanded.topology, &expanded.constraints, &expanded.tasks)
    {
        Ok(constraints) if errors.is_empty() => Ok(constraints),
        Ok(_) => Err(errors),
        Err(mut constraint_errors) => {
            errors.append(&mut constraint_errors);
            Err(errors)
        }
    }
}

pub fn build_timing_model(program: &PlcProgram) -> Result<TimingModel, Vec<PlcError>> {
    let expanded = preprocess_program(program)?;
    build_timing_model_from_ast(&expanded.topology, &expanded.tasks)
}

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

fn semantic_topology_connections(topology: &TopologySection) -> Vec<TopologyConnection> {
    topology.connections.clone()
}

fn topology_connection_line(topology: &TopologySection, connection: &TopologyConnection) -> usize {
    topology
        .devices
        .iter()
        .find(|device| device.name == connection.to)
        .map(|device| device.line.max(1))
        .or_else(|| {
            topology
                .devices
                .iter()
                .find(|device| device.name == connection.from)
                .map(|device| device.line.max(1))
        })
        .unwrap_or(1)
}

fn topology_connection_context(connection: &TopologyConnection) -> String {
    let relation = topology_relation_name(&connection.relation);
    let from_port = connection.from_port.as_deref().unwrap_or("<missing>");
    let to_port = connection.to_port.as_deref().unwrap_or("<missing>");
    format!(
        "relation {{ from: {}.{}, to: {}.{}, via: {} }}",
        connection.from, from_port, connection.to, to_port, relation
    )
}

fn topology_relation_name(relation: &TopologyRelation) -> &'static str {
    match relation {
        TopologyRelation::DrivenBy => "driven_by",
        TopologyRelation::ReportsTo => "reports_to",
        TopologyRelation::Detects => "detects",
    }
}

fn extract_variable_defs(
    topology: &TopologySection,
    errors: &mut Vec<PlcError>,
) -> Vec<VariableDef> {
    let mut defs = Vec::new();
    let mut seen = HashSet::<String>::new();

    for variable in &topology.variables {
        let line = variable.line.max(1);
        if !seen.insert(variable.name.clone()) {
            errors.push(PlcError::duplicate_definition_with_reason(
                line,
                "变量",
                &variable.name,
                "变量名必须唯一，请重命名重复声明",
            ));
            continue;
        }

        if topology.devices.iter().any(|d| d.name == variable.name)
            || topology.cam_tables.iter().any(|t| t.name == variable.name)
        {
            errors.push(PlcError::duplicate_definition_with_reason(
                line,
                "符号",
                &variable.name,
                "变量名不能与设备名或 cam_table 名相同",
            ));
            continue;
        }

        let (ir_type, initial_value) = match lower_variable_initial_value(variable) {
            Ok(value) => value,
            Err(err) => {
                errors.push(err);
                continue;
            }
        };

        defs.push(VariableDef {
            name: variable.name.clone(),
            var_type: ir_type,
            initial_value,
            index: (defs.len() as u16),
        });
    }

    if defs.len() > RUNTIME_MAX_VARIABLES {
        errors.push(PlcError::semantic_with_reason(
            1,
            format!(
                "变量数量超限：声明 {} 个，最大支持 {} 个",
                defs.len(),
                RUNTIME_MAX_VARIABLES
            ),
            "请减少 variable 声明数量，或在运行时扩容前保持 <= 64".to_string(),
        ));
    }

    defs
}

fn extract_cam_table_defs(
    topology: &TopologySection,
    errors: &mut Vec<PlcError>,
) -> Vec<CamTableIr> {
    let mut defs = Vec::new();
    let mut seen = HashSet::<String>::new();
    let variable_names = topology
        .variables
        .iter()
        .map(|v| v.name.as_str())
        .collect::<HashSet<_>>();
    let device_names = topology
        .devices
        .iter()
        .map(|d| d.name.as_str())
        .collect::<HashSet<_>>();

    for table in &topology.cam_tables {
        let line = table.line.max(1);
        if !seen.insert(table.name.clone()) {
            errors.push(PlcError::duplicate_definition_with_reason(
                line,
                "cam_table",
                &table.name,
                "cam_table 名称必须唯一，请重命名重复声明",
            ));
            continue;
        }

        if variable_names.contains(table.name.as_str())
            || device_names.contains(table.name.as_str())
        {
            errors.push(PlcError::duplicate_definition_with_reason(
                line,
                "符号",
                &table.name,
                "cam_table 名称不能与 device/variable 重名",
            ));
            continue;
        }

        if table.points.len() < 2 {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!("cam_table {} 至少需要 2 个点", table.name),
                "请至少保留起点和终点坐标".to_string(),
            ));
            continue;
        }
        if table.points.len() > MAX_CAM_POINTS {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!(
                    "cam_table {} 点数超限：{} > {}",
                    table.name,
                    table.points.len(),
                    MAX_CAM_POINTS
                ),
                "请减少点数量或分拆为多个表".to_string(),
            ));
            continue;
        }

        let mut master_positions = Vec::with_capacity(table.points.len());
        let mut slave_positions = Vec::with_capacity(table.points.len());
        let mut monotonic_ok = true;
        for point in &table.points {
            master_positions.push(point.master as f32);
            slave_positions.push(point.slave as f32);
        }
        for i in 1..master_positions.len() {
            if master_positions[i] <= master_positions[i - 1] {
                monotonic_ok = false;
                break;
            }
        }
        if !monotonic_ok {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!("cam_table {} 的 master 坐标必须严格递增", table.name),
                "请确保后一个点的 master 值始终大于前一个点".to_string(),
            ));
            continue;
        }

        if matches!(table.mode, CamTableMode::Periodic) {
            let first = slave_positions.first().copied().unwrap_or(0.0);
            let last = slave_positions.last().copied().unwrap_or(0.0);
            if (first - last).abs() > f32::EPSILON {
                errors.push(PlcError::semantic_with_reason(
                    line,
                    format!("周期 cam_table {} 要求首尾 slave 值相等", table.name),
                    "periodic 模式下请保持首尾点从轴值一致".to_string(),
                ));
                continue;
            }
        }

        defs.push(CamTableIr {
            name: table.name.clone(),
            periodic: matches!(table.mode, CamTableMode::Periodic),
            num_points: master_positions.len(),
            spline_coeffs: compute_spline_coeffs(
                &master_positions,
                &slave_positions,
                matches!(table.mode, CamTableMode::Periodic),
            ),
            master_positions,
            slave_positions,
        });
    }

    defs
}

fn extract_cam_coupling_defs(
    topology: &TopologySection,
    cam_table_names: &HashSet<String>,
    errors: &mut Vec<PlcError>,
) -> Vec<CamCouplingDef> {
    let device_names = topology
        .devices
        .iter()
        .map(|d| d.name.as_str())
        .collect::<HashSet<_>>();

    let mut defs = Vec::new();
    for device in &topology.devices {
        if !matches!(device.device_type, DeviceType::CamCoupling) {
            continue;
        }
        let line = device.line.max(1);
        let attrs = &device.attributes;

        let Some(master) = attrs.master.as_ref() else {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!("cam_coupling {} 缺少 master 属性", device.name),
                "请配置主轴设备名称，例如 master: encoder_main".to_string(),
            ));
            continue;
        };
        let Some(slave) = attrs.slave.as_ref() else {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!("cam_coupling {} 缺少 slave 属性", device.name),
                "请配置从轴设备名称，例如 slave: servo_x".to_string(),
            ));
            continue;
        };
        let Some(table) = attrs.table.as_ref() else {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!("cam_coupling {} 缺少 table 属性", device.name),
                "请配置凸轮表名称，例如 table: linear_cam".to_string(),
            ));
            continue;
        };

        if !device_names.contains(master.as_str()) {
            errors.push(PlcError::undefined_reference_with_reason(
                line,
                "设备",
                master,
                format!("cam_coupling {} 的 master 引用了未定义设备", device.name),
            ));
            continue;
        }
        if !device_names.contains(slave.as_str()) {
            errors.push(PlcError::undefined_reference_with_reason(
                line,
                "设备",
                slave,
                format!("cam_coupling {} 的 slave 引用了未定义设备", device.name),
            ));
            continue;
        }
        if !cam_table_names.contains(table) {
            errors.push(PlcError::undefined_reference_with_reason(
                line,
                "cam_table",
                table,
                format!("cam_coupling {} 的 table 引用了未定义表", device.name),
            ));
            continue;
        }

        let interpolation = match attrs.interpolation.as_deref().unwrap_or("cubic_spline") {
            "linear" => CamInterpolation::Linear,
            "cubic_spline" => CamInterpolation::CubicSpline,
            other => {
                errors.push(PlcError::semantic_with_reason(
                    line,
                    format!(
                        "cam_coupling {} 的 interpolation 不支持 {}",
                        device.name, other
                    ),
                    "支持值: linear / cubic_spline".to_string(),
                ));
                continue;
            }
        };

        let slave_feedback = attrs
            .slave_feedback
            .clone()
            .unwrap_or_else(|| slave.clone());
        if !device_names.contains(slave_feedback.as_str()) {
            errors.push(PlcError::undefined_reference_with_reason(
                line,
                "设备",
                &slave_feedback,
                format!(
                    "cam_coupling {} 的 slave_feedback 引用了未定义设备",
                    device.name
                ),
            ));
            continue;
        }

        defs.push(CamCouplingDef {
            name: device.name.clone(),
            master: master.clone(),
            slave: slave.clone(),
            table: table.clone(),
            interpolation,
            gear_ratio: attrs.gear_ratio.unwrap_or(1.0) as f32,
            phase_offset: attrs.phase_offset.unwrap_or(0.0) as f32,
            following_error_limit: attrs.following_error_limit.unwrap_or(1.0) as f32,
            slave_feedback,
        });
    }

    defs
}

fn compute_spline_coeffs(master: &[f32], slave: &[f32], periodic: bool) -> Vec<SplineCoeff> {
    let n = master.len();
    if n < 2 || slave.len() != n {
        return Vec::new();
    }
    if n == 2 {
        return compute_linear_spline_coeffs(master, slave);
    }
    if periodic {
        return compute_periodic_spline_coeffs(master, slave)
            .unwrap_or_else(|| compute_linear_spline_coeffs(master, slave));
    }

    let mut h = vec![0.0f32; n - 1];
    for i in 0..(n - 1) {
        let dx = master[i + 1] - master[i];
        if dx <= 0.0 {
            return compute_linear_spline_coeffs(master, slave);
        }
        h[i] = dx;
    }

    let mut alpha = vec![0.0f32; n];
    for i in 1..(n - 1) {
        alpha[i] =
            3.0 / h[i] * (slave[i + 1] - slave[i]) - 3.0 / h[i - 1] * (slave[i] - slave[i - 1]);
    }

    let mut l = vec![0.0f32; n];
    let mut mu = vec![0.0f32; n];
    let mut z = vec![0.0f32; n];
    let mut c = vec![0.0f32; n];

    l[0] = 1.0;
    for i in 1..(n - 1) {
        l[i] = 2.0 * (master[i + 1] - master[i - 1]) - h[i - 1] * mu[i - 1];
        if l[i] == 0.0 {
            return compute_linear_spline_coeffs(master, slave);
        }
        mu[i] = h[i] / l[i];
        z[i] = (alpha[i] - h[i - 1] * z[i - 1]) / l[i];
    }
    l[n - 1] = 1.0;

    let mut coeffs = vec![
        SplineCoeff {
            a: 0.0,
            b: 0.0,
            c: 0.0,
            d: 0.0,
        };
        n - 1
    ];
    for j in (0..(n - 1)).rev() {
        c[j] = z[j] - mu[j] * c[j + 1];
        let b = (slave[j + 1] - slave[j]) / h[j] - h[j] * (c[j + 1] + 2.0 * c[j]) / 3.0;
        let d = (c[j + 1] - c[j]) / (3.0 * h[j]);
        coeffs[j] = SplineCoeff {
            a: slave[j],
            b,
            c: c[j],
            d,
        };
    }

    coeffs
}

fn compute_periodic_spline_coeffs(master: &[f32], slave: &[f32]) -> Option<Vec<SplineCoeff>> {
    let n = master.len();
    if n < 3 || slave.len() != n {
        return None;
    }

    let unique = n - 1;
    if unique < 2 {
        return None;
    }

    let mut h = vec![0.0f64; unique];
    for i in 0..unique {
        let dx = (master[i + 1] - master[i]) as f64;
        if dx <= 0.0 {
            return None;
        }
        h[i] = dx;
    }

    let mut matrix = vec![vec![0.0f64; unique]; unique];
    let mut rhs = vec![0.0f64; unique];
    for i in 0..unique {
        let prev = if i == 0 { unique - 1 } else { i - 1 };
        let next = (i + 1) % unique;

        let h_prev = h[prev];
        let h_curr = h[i];
        matrix[i][prev] = h_prev;
        matrix[i][i] = 2.0 * (h_prev + h_curr);
        matrix[i][next] = h_curr;

        let y_prev = slave[prev] as f64;
        let y_curr = slave[i] as f64;
        let y_next = if i + 1 < unique {
            slave[i + 1] as f64
        } else {
            slave[0] as f64
        };
        rhs[i] = 6.0 * ((y_next - y_curr) / h_curr - (y_curr - y_prev) / h_prev);
    }

    let second = solve_linear_system(matrix, rhs)?;
    if second.len() != unique {
        return None;
    }

    let mut coeffs = Vec::with_capacity(unique);
    for i in 0..unique {
        let dx = (master[i + 1] - master[i]) as f64;
        let y0 = slave[i] as f64;
        let y1 = slave[i + 1] as f64;
        let m0 = second[i];
        let m1 = if i + 1 < unique {
            second[i + 1]
        } else {
            second[0]
        };
        let b = (y1 - y0) / dx - dx * (2.0 * m0 + m1) / 6.0;
        let c = m0 / 2.0;
        let d = (m1 - m0) / (6.0 * dx);
        coeffs.push(SplineCoeff {
            a: y0 as f32,
            b: b as f32,
            c: c as f32,
            d: d as f32,
        });
    }

    Some(coeffs)
}

fn solve_linear_system(mut matrix: Vec<Vec<f64>>, mut rhs: Vec<f64>) -> Option<Vec<f64>> {
    let n = rhs.len();
    if matrix.len() != n || matrix.iter().any(|row| row.len() != n) {
        return None;
    }

    for col in 0..n {
        let mut pivot = col;
        for row in (col + 1)..n {
            if matrix[row][col].abs() > matrix[pivot][col].abs() {
                pivot = row;
            }
        }
        if matrix[pivot][col].abs() <= f64::EPSILON {
            return None;
        }
        if pivot != col {
            matrix.swap(pivot, col);
            rhs.swap(pivot, col);
        }

        let pivot_value = matrix[col][col];
        for row in (col + 1)..n {
            let factor = matrix[row][col] / pivot_value;
            if factor.abs() <= f64::EPSILON {
                continue;
            }
            matrix[row][col] = 0.0;
            for c in (col + 1)..n {
                matrix[row][c] -= factor * matrix[col][c];
            }
            rhs[row] -= factor * rhs[col];
        }
    }

    let mut solution = vec![0.0f64; n];
    for row in (0..n).rev() {
        let mut value = rhs[row];
        for col in (row + 1)..n {
            value -= matrix[row][col] * solution[col];
        }
        let denom = matrix[row][row];
        if denom.abs() <= f64::EPSILON {
            return None;
        }
        solution[row] = value / denom;
    }

    Some(solution)
}

fn compute_linear_spline_coeffs(master: &[f32], slave: &[f32]) -> Vec<SplineCoeff> {
    if master.len() < 2 || slave.len() < 2 {
        return Vec::new();
    }
    let mut coeffs = Vec::with_capacity(master.len().saturating_sub(1));
    for i in 0..(master.len() - 1) {
        let dx = master[i + 1] - master[i];
        let slope = if dx == 0.0 {
            0.0
        } else {
            (slave[i + 1] - slave[i]) / dx
        };
        coeffs.push(SplineCoeff {
            a: slave[i],
            b: slope,
            c: 0.0,
            d: 0.0,
        });
    }
    coeffs
}

fn lower_variable_initial_value(
    variable: &VariableDeclaration,
) -> Result<(IrVariableType, f32), PlcError> {
    let line = variable.line.max(1);
    let raw = variable.initial_value.trim();
    match variable.var_type {
        AstVariableType::Float => {
            let value = raw.parse::<f32>().map_err(|_| {
                PlcError::type_mismatch_with_reason(
                    line,
                    "float",
                    raw,
                    format!("variable {}", variable.name),
                    "float 初值应为数字字面量（如 0.0）",
                )
            })?;
            Ok((IrVariableType::Float, value))
        }
        AstVariableType::Int => {
            let value = raw.parse::<i32>().map_err(|_| {
                PlcError::type_mismatch_with_reason(
                    line,
                    "int",
                    raw,
                    format!("variable {}", variable.name),
                    "int 初值应为整数（如 0）",
                )
            })?;
            Ok((IrVariableType::Int, value as f32))
        }
        AstVariableType::Bool => {
            let value = match raw {
                "true" => 1.0,
                "false" => 0.0,
                _ => {
                    return Err(PlcError::type_mismatch_with_reason(
                        line,
                        "bool",
                        raw,
                        format!("variable {}", variable.name),
                        "bool 初值应为 true 或 false",
                    ));
                }
            };
            Ok((IrVariableType::Bool, value))
        }
    }
}

fn extract_extern_function_defs(topology: &TopologySection) -> Vec<IrExternFunctionDef> {
    topology
        .extern_functions
        .iter()
        .map(lower_extern_function_def)
        .collect()
}

fn lower_extern_function_def(decl: &AstExternFunctionDeclaration) -> IrExternFunctionDef {
    IrExternFunctionDef {
        name: decl.name.clone(),
        params: decl
            .params
            .iter()
            .map(|param| IrExternFunctionParam {
                name: param.name.clone(),
                var_type: ast_variable_type_to_ir(&param.var_type),
            })
            .collect(),
        return_types: decl
            .return_types
            .iter()
            .map(ast_variable_type_to_ir)
            .collect(),
        contract: IrExternContract {
            rust_module: decl.contract.rust_module.clone(),
            pure: decl.contract.pure,
            time_bound_us: decl.contract.time_bound_us,
        },
    }
}

fn extract_axis_fault_contract_defs(
    topology: &TopologySection,
    device_nodes: &HashMap<String, DeviceNode>,
    errors: &mut Vec<PlcError>,
) -> Vec<IrAxisFaultContractDef> {
    let mut defs = Vec::new();
    let mut seen_names = HashSet::<String>::new();
    let mut seen_axis = HashMap::<String, String>::new();
    let axis_device_names = collect_axis_device_names(topology);
    let axis_device_name_set = axis_device_names
        .iter()
        .cloned()
        .collect::<HashSet<String>>();
    let axis_functional_groups = collect_axis_functional_groups(topology, &axis_device_name_set);
    let axis_followers_by_master = collect_axis_followers(topology, &axis_device_name_set);

    for contract in &topology.axis_fault_contracts {
        let line = contract.line.max(1);
        if !seen_names.insert(contract.name.clone()) {
            errors.push(PlcError::duplicate_definition_with_reason(
                line,
                "axis_fault_contract",
                &contract.name,
                "axis_fault_contract 名称必须唯一，请重命名重复声明",
            ));
            continue;
        }

        let Some(axis_node) = device_nodes.get(&contract.axis) else {
            errors.push(PlcError::undefined_reference_with_reason(
                line,
                "轴设备",
                &contract.axis,
                format!(
                    "axis_fault_contract {} 的 axis 字段引用了未定义设备",
                    contract.name
                ),
            ));
            continue;
        };

        if !matches!(
            axis_node.kind,
            DeviceKind::StepperMotor | DeviceKind::ServoDrive
        ) {
            errors.push(PlcError::type_mismatch_with_reason(
                line,
                "stepper_motor 或 servo_drive",
                device_kind_name(&axis_node.kind),
                format!("axis_fault_contract {}.axis", contract.name),
                "axis_fault_contract 只能绑定到轴设备（stepper_motor/servo_drive）",
            ));
            continue;
        }

        if let Some(previous_contract) = seen_axis.get(&contract.axis) {
            errors.push(PlcError::duplicate_definition_with_reason(
                line,
                "axis_fault_contract",
                &contract.name,
                format!(
                    "轴设备 {} 已被 axis_fault_contract {} 绑定，同一轴仅允许一个 contract",
                    contract.axis, previous_contract
                ),
            ));
            continue;
        }

        if matches!(
            contract.propagation_scope,
            AstAxisFaultPropagationScope::Custom
        ) {
            let mut has_invalid_target = false;
            for target in &contract.propagation_targets {
                let Some(target_node) = device_nodes.get(target) else {
                    errors.push(PlcError::undefined_reference_with_reason(
                        line,
                        "轴设备",
                        target,
                        format!(
                            "axis_fault_contract {} 的 propagation_targets 引用了未定义设备",
                            contract.name
                        ),
                    ));
                    has_invalid_target = true;
                    continue;
                };

                if !matches!(
                    target_node.kind,
                    DeviceKind::StepperMotor | DeviceKind::ServoDrive
                ) {
                    errors.push(PlcError::type_mismatch_with_reason(
                        line,
                        "stepper_motor 或 servo_drive",
                        device_kind_name(&target_node.kind),
                        format!("axis_fault_contract {}.propagation_targets", contract.name),
                        "propagation_targets 只能包含轴设备（stepper_motor/servo_drive）",
                    ));
                    has_invalid_target = true;
                }
            }

            if has_invalid_target {
                continue;
            }
        }

        seen_axis.insert(contract.axis.clone(), contract.name.clone());
        let resolved_targets = resolve_axis_fault_propagation_targets(
            contract,
            &axis_device_names,
            &axis_functional_groups,
            &axis_followers_by_master,
        );
        defs.push(lower_axis_fault_contract_def(contract, resolved_targets));
    }

    defs
}

fn collect_axis_device_names(topology: &TopologySection) -> Vec<String> {
    topology
        .devices
        .iter()
        .filter(|device| {
            matches!(
                device.device_type,
                DeviceType::StepperMotor | DeviceType::ServoDrive
            )
        })
        .map(|device| device.name.clone())
        .collect()
}

fn collect_axis_functional_groups(
    topology: &TopologySection,
    axis_device_name_set: &HashSet<String>,
) -> HashMap<String, HashSet<String>> {
    let mut groups = HashMap::new();
    for device in &topology.devices {
        if !axis_device_name_set.contains(&device.name) {
            continue;
        }

        groups.insert(
            device.name.clone(),
            device
                .attributes
                .tags
                .functional_group
                .iter()
                .cloned()
                .collect(),
        );
    }
    groups
}

fn collect_axis_followers(
    topology: &TopologySection,
    axis_device_name_set: &HashSet<String>,
) -> HashMap<String, Vec<String>> {
    let mut followers_by_master = HashMap::<String, Vec<String>>::new();

    for device in &topology.devices {
        if !matches!(device.device_type, DeviceType::CamCoupling) {
            continue;
        }

        let Some(master) = device.attributes.master.as_ref() else {
            continue;
        };
        let Some(slave) = device.attributes.slave.as_ref() else {
            continue;
        };
        if !axis_device_name_set.contains(master) || !axis_device_name_set.contains(slave) {
            continue;
        }

        followers_by_master
            .entry(master.clone())
            .or_default()
            .push(slave.clone());
    }

    for followers in followers_by_master.values_mut() {
        followers.sort();
        followers.dedup();
    }

    followers_by_master
}

fn resolve_axis_fault_propagation_targets(
    contract: &AstAxisFaultContractDeclaration,
    axis_device_names: &[String],
    axis_functional_groups: &HashMap<String, HashSet<String>>,
    axis_followers_by_master: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    let mut targets = vec![contract.axis.clone()];
    match contract.propagation_scope {
        AstAxisFaultPropagationScope::SelfOnly => {}
        AstAxisFaultPropagationScope::Custom => {
            for target in &contract.propagation_targets {
                push_unique_target(&mut targets, target);
            }
        }
        AstAxisFaultPropagationScope::All => {
            let mut others = axis_device_names
                .iter()
                .filter(|name| *name != &contract.axis)
                .cloned()
                .collect::<Vec<_>>();
            others.sort();
            for target in others {
                push_unique_target(&mut targets, &target);
            }
        }
        AstAxisFaultPropagationScope::Group => {
            let source_groups = axis_functional_groups
                .get(&contract.axis)
                .cloned()
                .unwrap_or_default();
            if !source_groups.is_empty() {
                let mut grouped_axes = axis_device_names
                    .iter()
                    .filter(|name| {
                        axis_functional_groups
                            .get(*name)
                            .map(|groups| !groups.is_disjoint(&source_groups))
                            .unwrap_or(false)
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                grouped_axes.sort();
                for target in grouped_axes {
                    push_unique_target(&mut targets, &target);
                }
            }
        }
        AstAxisFaultPropagationScope::Followers => {
            let mut queue = vec![contract.axis.clone()];
            let mut cursor = 0usize;
            while cursor < queue.len() {
                let master = &queue[cursor];
                cursor += 1;
                if let Some(followers) = axis_followers_by_master.get(master) {
                    for follower in followers {
                        if !targets.iter().any(|existing| existing == follower) {
                            targets.push(follower.clone());
                            queue.push(follower.clone());
                        }
                    }
                }
            }
        }
    }

    targets
}

fn push_unique_target(targets: &mut Vec<String>, candidate: &str) {
    if targets.iter().any(|existing| existing == candidate) {
        return;
    }
    targets.push(candidate.to_string());
}

fn lower_axis_fault_contract_def(
    contract: &AstAxisFaultContractDeclaration,
    propagation_targets: Vec<String>,
) -> IrAxisFaultContractDef {
    IrAxisFaultContractDef {
        name: contract.name.clone(),
        axis: contract.axis.clone(),
        severity: lower_axis_fault_severity(&contract.severity),
        stop_mode: lower_axis_stop_mode(&contract.stop_mode),
        auto_reset_policy: lower_axis_auto_reset_policy(&contract.auto_reset_policy),
        manual_ack_required: contract.manual_ack_required,
        propagation_scope: lower_axis_fault_propagation_scope(&contract.propagation_scope),
        propagation_targets,
    }
}

fn lower_axis_fault_severity(severity: &AstAxisFaultSeverity) -> IrAxisFaultSeverity {
    match severity {
        AstAxisFaultSeverity::Recoverable => IrAxisFaultSeverity::Recoverable,
        AstAxisFaultSeverity::NonRecoverable => IrAxisFaultSeverity::NonRecoverable,
        AstAxisFaultSeverity::Safety => IrAxisFaultSeverity::Safety,
    }
}

fn lower_axis_stop_mode(stop_mode: &AstAxisStopMode) -> IrAxisStopMode {
    match stop_mode {
        AstAxisStopMode::Controlled => IrAxisStopMode::Controlled,
        AstAxisStopMode::Quick => IrAxisStopMode::Quick,
        AstAxisStopMode::Immediate => IrAxisStopMode::Immediate,
    }
}

fn lower_axis_auto_reset_policy(policy: &AstAxisAutoResetPolicy) -> IrAxisAutoResetPolicy {
    match policy {
        AstAxisAutoResetPolicy::Never => IrAxisAutoResetPolicy::Never,
        AstAxisAutoResetPolicy::OnClear => IrAxisAutoResetPolicy::OnClear,
        AstAxisAutoResetPolicy::Immediate => IrAxisAutoResetPolicy::Immediate,
    }
}

fn lower_axis_fault_propagation_scope(
    scope: &AstAxisFaultPropagationScope,
) -> IrAxisFaultPropagationScope {
    match scope {
        AstAxisFaultPropagationScope::SelfOnly => IrAxisFaultPropagationScope::SelfOnly,
        AstAxisFaultPropagationScope::Group => IrAxisFaultPropagationScope::Group,
        AstAxisFaultPropagationScope::All => IrAxisFaultPropagationScope::All,
        AstAxisFaultPropagationScope::Followers => IrAxisFaultPropagationScope::Followers,
        AstAxisFaultPropagationScope::Custom => IrAxisFaultPropagationScope::Custom,
    }
}

fn ast_variable_type_to_ir(var_type: &AstVariableType) -> IrVariableType {
    match var_type {
        AstVariableType::Float => IrVariableType::Float,
        AstVariableType::Int => IrVariableType::Int,
        AstVariableType::Bool => IrVariableType::Bool,
    }
}

fn ast_variable_type_name(var_type: &AstVariableType) -> &'static str {
    match var_type {
        AstVariableType::Float => "float",
        AstVariableType::Int => "int",
        AstVariableType::Bool => "bool",
    }
}

fn extract_pid_loops(topology: &TopologySection, errors: &mut Vec<PlcError>) -> Vec<IrPidLoop> {
    let device_ranges = collect_device_ranges(topology);
    let device_units = collect_device_units(topology);
    let analog_inputs = topology
        .devices
        .iter()
        .filter(|d| matches!(d.device_type, DeviceType::AnalogInput))
        .map(|d| d.name.as_str())
        .collect::<HashSet<_>>();
    let analog_outputs = topology
        .devices
        .iter()
        .filter(|d| matches!(d.device_type, DeviceType::AnalogOutput))
        .map(|d| d.name.as_str())
        .collect::<HashSet<_>>();

    let mut pid_loops = Vec::new();
    for device in &topology.devices {
        if !matches!(device.device_type, DeviceType::Pid) {
            continue;
        }
        let line = device.line.max(1);
        let Some(pv) = device.attributes.pv.as_ref() else {
            errors.push(PlcError::semantic(
                line,
                format!("PID {} 缺少 pv 属性", device.name),
            ));
            continue;
        };
        let Some(sp) = device.attributes.sp.as_ref() else {
            errors.push(PlcError::semantic(
                line,
                format!("PID {} 缺少 sp 属性", device.name),
            ));
            continue;
        };
        let Some(sp_numeric) = format_numeric_literal_from_literal(sp) else {
            errors.push(PlcError::semantic(
                line,
                format!("PID {} 的 sp 必须是 number 或 measured_value", device.name),
            ));
            continue;
        };
        let Some(kp) = device.attributes.kp else {
            errors.push(PlcError::semantic(
                line,
                format!("PID {} 缺少 kp 属性", device.name),
            ));
            continue;
        };
        let Some(ki) = device.attributes.ki else {
            errors.push(PlcError::semantic(
                line,
                format!("PID {} 缺少 ki 属性", device.name),
            ));
            continue;
        };
        let Some(kd) = device.attributes.kd else {
            errors.push(PlcError::semantic(
                line,
                format!("PID {} 缺少 kd 属性", device.name),
            ));
            continue;
        };
        let Some(out) = device.attributes.out.as_ref() else {
            errors.push(PlcError::semantic(
                line,
                format!("PID {} 缺少 out 属性", device.name),
            ));
            continue;
        };
        let Some(period_ms) = device.attributes.period_ms else {
            errors.push(PlcError::semantic(
                line,
                format!("PID {} 缺少 period_ms 属性", device.name),
            ));
            continue;
        };
        let Some(limit) = device.attributes.limit.as_ref() else {
            errors.push(PlcError::semantic(
                line,
                format!("PID {} 缺少 limit 属性", device.name),
            ));
            continue;
        };
        if period_ms == 0 {
            errors.push(PlcError::semantic(
                line,
                format!("PID {} 的 period_ms 必须 > 0", device.name),
            ));
        }
        if !analog_inputs.contains(pv.as_str()) {
            errors.push(PlcError::semantic(
                line,
                format!("PID {} 的 pv={} 不是 analog_input", device.name, pv),
            ));
        }
        if !analog_outputs.contains(out.as_str()) {
            errors.push(PlcError::semantic(
                line,
                format!("PID {} 的 out={} 不是 analog_output", device.name, out),
            ));
        }

        let (limit_min, limit_max) = if limit.min <= limit.max {
            (limit.min, limit.max)
        } else {
            (limit.max, limit.min)
        };

        if let Some((out_min, out_max)) = device_ranges.get(out).copied() {
            if limit_min < out_min || limit_max > out_max {
                errors.push(PlcError::semantic_with_reason(
                    line,
                    format!(
                        "PID {} 的 limit {}..{} 超出了输出 {} 的 range {}..{}",
                        device.name, limit_min, limit_max, out, out_min, out_max
                    ),
                    "请将 limit 约束在 analog_output 的 range 之内（或调整输出 range）",
                ));
            }
        }

        // If pv declares a unit and sp is measured, require them to match.
        if let Some(pv_unit) = device_units.get(pv) {
            if let LiteralValue::Measured(measured) = sp {
                if measured.unit != *pv_unit {
                    errors.push(PlcError::semantic_with_reason(
                        line,
                        format!(
                            "PID {} 的 sp 单位 {} 与 pv={} 单位 {} 不一致",
                            device.name, measured.unit, pv, pv_unit
                        ),
                        "请确保 sp 与 pv 使用相同 unit（或调整 pv 的 unit）",
                    ));
                }
            }
        }

        pid_loops.push(IrPidLoop {
            name: device.name.clone(),
            pv: pv.clone(),
            sp: sp_numeric,
            kp: format_numeric_literal(kp),
            ki: format_numeric_literal(ki),
            kd: format_numeric_literal(kd),
            out: out.clone(),
            period_ms,
            limit_min: format_numeric_literal(limit_min),
            limit_max: format_numeric_literal(limit_max),
            anti_windup: "conditional_integration".to_string(),
        });
    }
    pid_loops
}

pub fn build_constraint_set_from_ast(
    topology: &TopologySection,
    constraints: &ConstraintsSection,
    tasks: &TasksSection,
) -> Result<ConstraintSet, Vec<PlcError>> {
    let mut errors = Vec::new();
    let mut constraint_set = ConstraintSet::default();
    let workpiece_catalog =
        validate_and_lower_workpiece_topology_v2(topology, &mut constraint_set, &mut errors);

    let device_kinds = collect_device_kinds(topology);
    let known_states = collect_known_states(topology, &device_kinds);
    let task_steps = collect_task_steps(tasks);
    let device_port_types = collect_device_port_types(topology, &device_kinds);
    let device_ranges = collect_device_ranges(topology);
    let device_units = collect_device_units(topology);
    let variable_names = topology
        .variables
        .iter()
        .map(|var| var.name.clone())
        .collect::<HashSet<_>>();
    let extern_function_names = topology
        .extern_functions
        .iter()
        .map(|func| func.name.clone())
        .collect::<HashSet<_>>();
    let declared_action_tags = collect_declared_action_tags(tasks);
    let mut seen_resource_names = HashSet::<String>::new();

    for resource in &topology.semantic_resources {
        if !seen_resource_names.insert(resource.name.clone()) {
            errors.push(PlcError::semantic_with_reason(
                resource.line.max(1),
                format!(
                    "[SRI-001] semantic resource '{}' is declared more than once.",
                    resource.name
                ),
                "请合并重复的 resource 声明，或重命名其中一个资源".to_string(),
            ));
            continue;
        }

        constraint_set.semantic_resources.push(IrSemanticResource {
            name: resource.name.clone(),
            mode: map_semantic_resource_mode(&resource.mode),
            purpose: resource.purpose.clone(),
        });
    }

    let declared_resource_names = constraint_set
        .semantic_resources
        .iter()
        .map(|resource| resource.name.clone())
        .collect::<HashSet<_>>();

    if tasks_use_workpiece_effects(tasks) {
        validate_workpiece_effects_in_tasks_v2(
            tasks,
            &workpiece_catalog,
            &constraint_set.workpiece_types,
            &mut errors,
        );
    }

    for safety in &constraints.safety {
        validate_safety_operand(
            &safety.left,
            safety.line,
            "safety 左侧",
            &device_kinds,
            &known_states,
            &device_port_types,
            &device_ranges,
            &device_units,
            &mut errors,
        );
        validate_safety_operand(
            &safety.right,
            safety.line,
            "safety 右侧",
            &device_kinds,
            &known_states,
            &device_port_types,
            &device_ranges,
            &device_units,
            &mut errors,
        );

        constraint_set.safety.push(SafetyRule {
            left: map_safety_operand(&safety.left),
            relation: map_safety_relation(&safety.relation),
            right: map_safety_operand(&safety.right),
            reason: safety.reason.clone(),
            source: safety.source.clone(),
        });
    }

    for claim in &constraints.claims {
        match &claim.source {
            AstResourceClaimSource::State(state_ref) => {
                validate_state_reference(
                    state_ref,
                    claim.line,
                    "claim source",
                    &device_kinds,
                    &known_states,
                    &mut errors,
                );
            }
            AstResourceClaimSource::ActionTag { tag } => {
                if !declared_action_tags.contains(tag) {
                    errors.push(PlcError::semantic_with_reason(
                        claim.line.max(1),
                        format!(
                            "[SRI-003] action_tag '{tag}' is not used by any supported action."
                        ),
                        "请在受支持的长时动作上显式声明 semantic_tag，或删除该 claim".to_string(),
                    ));
                }
            }
        }

        if !declared_resource_names.contains(&claim.resource) {
            errors.push(PlcError::semantic_with_reason(
                claim.line.max(1),
                format!(
                    "[SRI-002] claim references unknown semantic resource '{}'.",
                    claim.resource
                ),
                "请先在 [topology] 中声明对应的 resource".to_string(),
            ));
        }

        constraint_set.resource_claims.push(IrResourceClaimRule {
            source: map_resource_claim_source(&claim.source),
            resource: claim.resource.clone(),
            reason: claim.reason.clone(),
        });
    }

    for timing in &constraints.timing {
        validate_timing_target(&timing.target, timing.line, &task_steps, &mut errors);

        constraint_set.timing.push(TimingRule {
            scope: map_timing_scope(&timing.target),
            relation: map_timing_relation(&timing.relation),
            duration_ms: duration_value_to_ms(&timing.duration),
            reason: timing.reason.clone(),
        });
    }

    for causality in &constraints.causality {
        for node in &causality.chain {
            validate_causality_node_reference(
                &node.device,
                causality.line,
                &device_kinds,
                &variable_names,
                &extern_function_names,
                &mut errors,
            );
        }

        constraint_set.causality.push(CausalityChain {
            devices: causality
                .chain
                .iter()
                .map(|node| node.device.clone())
                .collect(),
            reason: causality.reason.clone(),
        });
    }

    for task in &tasks.tasks {
        for step in &task.steps {
            validate_wait_device_references_in_statements(
                &step.statements,
                step.line.max(1),
                &device_kinds,
                &device_port_types,
                &device_ranges,
                &device_units,
                &mut errors,
            );
            validate_analog_actions_in_statements(
                &step.statements,
                step.line.max(1),
                &device_kinds,
                &device_ranges,
                &mut errors,
            );
            validate_set_enum_values(&step.statements, step.line.max(1), &mut errors);
            validate_motor_legacy_set_actions(
                &step.statements,
                step.line.max(1),
                &device_kinds,
                &mut errors,
            );
        }
    }

    if errors.is_empty() {
        Ok(constraint_set)
    } else {
        Err(errors)
    }
}

#[derive(Debug, Clone, Default)]
struct WorkpieceCatalog {
    site_kinds: HashMap<String, AstWorkpieceSiteKind>,
    holders: HashSet<String>,
    carriers: HashMap<String, CarrierShape>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CarrierShape {
    dimensions: Vec<u32>,
}

#[allow(dead_code)]
fn validate_and_lower_workpiece_topology(
    topology: &TopologySection,
    constraint_set: &mut ConstraintSet,
    errors: &mut Vec<PlcError>,
) -> WorkpieceCatalog {
    let mut catalog = WorkpieceCatalog::default();
    let mut seen_workpiece_types = HashSet::<String>::new();
    let mut seen_places = HashSet::<String>::new();

    for site in &topology.workpiece_sites {
        if !seen_places.insert(site.name.clone()) {
            errors.push(PlcError::semantic_with_reason(
                site.line.max(1),
                format!("workpiece site '{}' 重复声明", site.name),
                "请删除重复的 site/location 声明，或改名以保持引用唯一".to_string(),
            ));
            continue;
        }
        catalog
            .site_kinds
            .insert(site.name.clone(), site.kind.clone());
        constraint_set.workpiece_sites.push(IrWorkpieceSiteDef {
            name: site.name.clone(),
            kind: map_workpiece_site_kind(&site.kind),
            capacity: site.capacity,
        });
    }

    for holder in &topology.workpiece_holders {
        if !seen_places.insert(holder.name.clone()) {
            errors.push(PlcError::semantic_with_reason(
                holder.line.max(1),
                format!("workpiece endpoint '{}' 重复声明", holder.name),
                "holder 与 site/location 不能重名，否则 effect 引用会失去唯一性".to_string(),
            ));
            continue;
        }
        catalog.holders.insert(holder.name.clone());
        constraint_set.workpiece_holders.push(IrWorkpieceHolderDef {
            name: holder.name.clone(),
            capacity: holder.capacity,
        });
    }

    for workpiece in &topology.workpiece_types {
        if !seen_workpiece_types.insert(workpiece.name.clone()) {
            errors.push(PlcError::semantic_with_reason(
                workpiece.line.max(1),
                format!("workpiece type '{}' 重复声明", workpiece.name),
                "请合并重复的 workpiece type 声明，或改名".to_string(),
            ));
            continue;
        }

        validate_workpiece_type_declaration(workpiece, &catalog, errors);
        constraint_set.workpiece_types.push(IrWorkpieceTypeDef {
            name: workpiece.name.clone(),
            properties: workpiece
                .properties
                .iter()
                .map(|property| IrWorkpiecePropertyDef {
                    name: property.name.clone(),
                    property_type: map_workpiece_property_type(&property.property_type),
                })
                .collect(),
            normal_terminal_states: workpiece.normal_terminal_states.clone(),
            abnormal_terminal_states: workpiece.abnormal_terminal_states.clone(),
            ingress_sites: workpiece.ingress_sites.clone(),
            normal_egress_sites: workpiece.normal_egress_sites.clone(),
            abnormal_egress_sites: workpiece.abnormal_egress_sites.clone(),
            allows: vec![],
            derived_from: vec![],
        });
    }

    catalog
}

fn map_workpiece_site_kind(kind: &AstWorkpieceSiteKind) -> IrWorkpieceSiteKind {
    match kind {
        AstWorkpieceSiteKind::WorkpieceLocation => IrWorkpieceSiteKind::WorkpieceLocation,
        AstWorkpieceSiteKind::CarrierLocation => IrWorkpieceSiteKind::CarrierLocation,
    }
}

fn map_workpiece_property_type(kind: &AstWorkpiecePropertyType) -> IrWorkpiecePropertyTypeDef {
    match kind {
        AstWorkpiecePropertyType::Bool => IrWorkpiecePropertyTypeDef::Bool,
        AstWorkpiecePropertyType::Enum { values } => IrWorkpiecePropertyTypeDef::Enum {
            values: values.clone(),
        },
    }
}

fn validate_workpiece_type_declaration(
    workpiece: &AstWorkpieceTypeDeclaration,
    catalog: &WorkpieceCatalog,
    errors: &mut Vec<PlcError>,
) {
    let mut seen_properties = HashSet::<String>::new();
    for property in &workpiece.properties {
        if !seen_properties.insert(property.name.clone()) {
            errors.push(PlcError::semantic_with_reason(
                workpiece.line.max(1),
                format!(
                    "workpiece type '{}' 的属性 '{}' 重复声明",
                    workpiece.name, property.name
                ),
                "同一个 workpiece type 内，属性名必须唯一".to_string(),
            ));
        }
        if let AstWorkpiecePropertyType::Enum { values } = &property.property_type {
            if values.is_empty() {
                errors.push(PlcError::semantic_with_reason(
                    workpiece.line.max(1),
                    format!(
                        "workpiece type '{}' 的枚举属性 '{}' 为空",
                        workpiece.name, property.name
                    ),
                    "enum 属性至少需要一个候选值".to_string(),
                ));
            }
        }
    }
    for property in &workpiece.properties {
        let AstWorkpiecePropertyType::Enum { values } = &property.property_type else {
            continue;
        };
        let mut seen_values = HashSet::<String>::new();
        for value in values {
            if seen_values.insert(value.clone()) {
                continue;
            }
            errors.push(PlcError::semantic_with_reason(
                workpiece.line.max(1),
                format!(
                    "workpiece type '{}' enum property '{}' repeats value '{}'",
                    workpiece.name, property.name, value
                ),
                "remove the duplicate enum value".to_string(),
            ));
        }
    }

    validate_terminal_egress_pair(
        workpiece.line.max(1),
        &workpiece.name,
        "normal",
        &workpiece.normal_terminal_states,
        &workpiece.normal_egress_sites,
        errors,
    );
    validate_terminal_egress_pair(
        workpiece.line.max(1),
        &workpiece.name,
        "abnormal",
        &workpiece.abnormal_terminal_states,
        &workpiece.abnormal_egress_sites,
        errors,
    );
    validate_unique_workpiece_entries(
        workpiece.line.max(1),
        &workpiece.name,
        "normal terminal state",
        &workpiece.normal_terminal_states,
        errors,
    );
    validate_unique_workpiece_entries(
        workpiece.line.max(1),
        &workpiece.name,
        "abnormal terminal state",
        &workpiece.abnormal_terminal_states,
        errors,
    );
    validate_reserved_terminal_state_entries(
        workpiece.line.max(1),
        &workpiece.name,
        "normal",
        &workpiece.normal_terminal_states,
        errors,
    );
    validate_reserved_terminal_state_entries(
        workpiece.line.max(1),
        &workpiece.name,
        "abnormal",
        &workpiece.abnormal_terminal_states,
        errors,
    );
    validate_unique_workpiece_entries(
        workpiece.line.max(1),
        &workpiece.name,
        "ingress site",
        &workpiece.ingress_sites,
        errors,
    );
    validate_unique_workpiece_entries(
        workpiece.line.max(1),
        &workpiece.name,
        "normal egress site",
        &workpiece.normal_egress_sites,
        errors,
    );
    validate_unique_workpiece_entries(
        workpiece.line.max(1),
        &workpiece.name,
        "abnormal egress site",
        &workpiece.abnormal_egress_sites,
        errors,
    );
    validate_disjoint_workpiece_entries(
        workpiece.line.max(1),
        &workpiece.name,
        "terminal state",
        "normal",
        &workpiece.normal_terminal_states,
        "abnormal",
        &workpiece.abnormal_terminal_states,
        errors,
    );
    validate_disjoint_workpiece_entries(
        workpiece.line.max(1),
        &workpiece.name,
        "egress site",
        "normal",
        &workpiece.normal_egress_sites,
        "abnormal",
        &workpiece.abnormal_egress_sites,
        errors,
    );

    for site in workpiece
        .ingress_sites
        .iter()
        .chain(workpiece.normal_egress_sites.iter())
        .chain(workpiece.abnormal_egress_sites.iter())
    {
        validate_workpiece_location_reference(workpiece.line.max(1), site, catalog, errors);
    }
}

fn validate_terminal_egress_pair(
    line: usize,
    workpiece_name: &str,
    category: &str,
    terminal_states: &[String],
    egress_sites: &[String],
    errors: &mut Vec<PlcError>,
) {
    if terminal_states.is_empty() != egress_sites.is_empty() {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "workpiece type '{}' 的 {} terminal states 与 egress sites 必须成对声明",
                workpiece_name, category
            ),
            "如果声明了一侧，另一侧也必须同时存在".to_string(),
        ));
    }
}

fn validate_unique_workpiece_entries(
    line: usize,
    workpiece_name: &str,
    entry_kind: &str,
    entries: &[String],
    errors: &mut Vec<PlcError>,
) {
    let mut seen = HashSet::<String>::new();
    for entry in entries {
        if seen.insert(entry.clone()) {
            continue;
        }
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "workpiece type '{}' repeats {} '{}'",
                workpiece_name, entry_kind, entry
            ),
            format!("remove the duplicate {} entry", entry_kind),
        ));
    }
}

fn validate_disjoint_workpiece_entries(
    line: usize,
    workpiece_name: &str,
    entry_kind: &str,
    left_label: &str,
    left_entries: &[String],
    right_label: &str,
    right_entries: &[String],
    errors: &mut Vec<PlcError>,
) {
    let right = right_entries.iter().cloned().collect::<HashSet<_>>();
    for entry in left_entries {
        if !right.contains(entry) {
            continue;
        }
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "workpiece type '{}' declares {} '{}' in both {} and {} categories",
                workpiece_name, entry_kind, entry, left_label, right_label
            ),
            format!(
                "keep each {} in exactly one of {} or {}",
                entry_kind, left_label, right_label
            ),
        ));
    }
}

fn validate_reserved_terminal_state_entries(
    line: usize,
    workpiece_name: &str,
    category: &str,
    entries: &[String],
    errors: &mut Vec<PlcError>,
) {
    for entry in entries {
        if entry != "consumed" {
            continue;
        }
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "workpiece type '{}' cannot declare reserved terminal state 'consumed' in {} category",
                workpiece_name, category
            ),
            "model in-process consumption via split/merge effects instead of terminal egress"
                .to_string(),
        ));
    }
}

fn validate_unique_workpiece_rule_entries(
    line: usize,
    workpiece_name: &str,
    rule_kind: &str,
    entries: &[String],
    errors: &mut Vec<PlcError>,
) {
    let mut seen = HashSet::<String>::new();
    for entry in entries {
        if seen.insert(entry.clone()) {
            continue;
        }
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "workpiece type '{}' repeats {} rule '{}'",
                workpiece_name, rule_kind, entry
            ),
            format!("remove the duplicate {} rule", rule_kind),
        ));
    }
}

fn validate_workpiece_location_reference(
    line: usize,
    site: &str,
    catalog: &WorkpieceCatalog,
    errors: &mut Vec<PlcError>,
) {
    if site.contains(".slot[") {
        validate_workpiece_contract_reference_v2(line, site, catalog, errors);
        return;
    }
    match catalog.site_kinds.get(site) {
        Some(AstWorkpieceSiteKind::WorkpieceLocation) => {}
        Some(AstWorkpieceSiteKind::CarrierLocation) => errors.push(PlcError::semantic_with_reason(
            line,
            format!("工件契约引用了 carrier_location '{}'", site),
            "Phase 1 的 ingress/egress 只能引用 workpiece_location".to_string(),
        )),
        None => errors.push(PlcError::undefined_reference_with_reason(
            line,
            "workpiece_location",
            site,
            "请先在 [topology] 中声明对应的 location".to_string(),
        )),
    }
}

fn tasks_use_workpiece_effects(tasks: &TasksSection) -> bool {
    tasks
        .tasks
        .iter()
        .flat_map(|task| task.steps.iter())
        .any(|step| statements_use_workpiece_effects(&step.statements))
}

fn statements_use_workpiece_effects(statements: &[StepStatement]) -> bool {
    statements.iter().any(|statement| match statement {
        StepStatement::Effect(_) => true,
        StepStatement::Repeat { body, .. } => statements_use_workpiece_effects(body),
        StepStatement::Parallel(block) => block
            .branches
            .iter()
            .any(|branch| statements_use_workpiece_effects(&branch.statements)),
        StepStatement::Race(block) => block
            .branches
            .iter()
            .any(|branch| statements_use_workpiece_effects(&branch.statements)),
        StepStatement::Action(_)
        | StepStatement::Wait(_)
        | StepStatement::IfElse { .. }
        | StepStatement::Delay { .. }
        | StepStatement::Timeout(_)
        | StepStatement::Goto(_)
        | StepStatement::AllowIndefiniteWait(_) => false,
    })
}

#[allow(dead_code)]
fn validate_workpiece_effects_in_tasks(
    tasks: &TasksSection,
    catalog: &WorkpieceCatalog,
    workpiece_types: &[IrWorkpieceTypeDef],
    errors: &mut Vec<PlcError>,
) {
    if workpiece_types.is_empty() {
        errors.push(PlcError::semantic_with_reason(
            1,
            "检测到 effect 语句，但 [topology] 未声明任何 workpiece type".to_string(),
            "Phase 1 的工件 effect 需要至少一个 workpiece type 契约".to_string(),
        ));
        return;
    }

    if workpiece_types.len() != 1 {
        errors.push(PlcError::semantic_with_reason(
            1,
            format!(
                "当前声明了 {} 个 workpiece type，但 Phase 1 effect 只支持单工件类型",
                workpiece_types.len()
            ),
            "请先收敛到一个 workpiece type，再使用 transfer/acquire/finish effect".to_string(),
        ));
        return;
    }

    let workpiece = &workpiece_types[0];
    for task in &tasks.tasks {
        for step in &task.steps {
            validate_workpiece_effects_in_statements(&step.statements, catalog, workpiece, errors);
        }
    }
}

#[allow(dead_code)]
fn validate_workpiece_effects_in_statements(
    statements: &[StepStatement],
    catalog: &WorkpieceCatalog,
    workpiece: &IrWorkpieceTypeDef,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Effect(effect) => match &effect.kind {
                AstEffectKind::Acquire { holder, from } => {
                    validate_holder_reference(effect.line.max(1), holder, catalog, errors);
                    validate_workpiece_location_reference(
                        effect.line.max(1),
                        from,
                        catalog,
                        errors,
                    );
                }
                AstEffectKind::Transfer { from, to } => {
                    validate_workpiece_endpoint_reference(
                        effect.line.max(1),
                        from,
                        catalog,
                        errors,
                    );
                    validate_workpiece_endpoint_reference(effect.line.max(1), to, catalog, errors);
                }
                AstEffectKind::Finish { at, terminal_state } => {
                    validate_workpiece_location_reference(effect.line.max(1), at, catalog, errors);
                    let normal = workpiece
                        .normal_terminal_states
                        .iter()
                        .any(|state| state == terminal_state);
                    let abnormal = workpiece
                        .abnormal_terminal_states
                        .iter()
                        .any(|state| state == terminal_state);
                    if !normal && !abnormal {
                        errors.push(PlcError::semantic_with_reason(
                            effect.line.max(1),
                            format!(
                                "terminal state '{}' 未在 workpiece type '{}' 中声明",
                                terminal_state, workpiece.name
                            ),
                            "finish effect 只能使用已声明的 normal/abnormal terminal state"
                                .to_string(),
                        ));
                    }
                    if normal && !workpiece.normal_egress_sites.iter().any(|site| site == at) {
                        errors.push(PlcError::semantic_with_reason(
                            effect.line.max(1),
                            format!(
                                "finish at '{}' as '{}' 不满足 normal egress 契约",
                                at, terminal_state
                            ),
                            "normal terminal state 只能落在 normal_egress_sites".to_string(),
                        ));
                    }
                    if abnormal
                        && !workpiece
                            .abnormal_egress_sites
                            .iter()
                            .any(|site| site == at)
                    {
                        errors.push(PlcError::semantic_with_reason(
                            effect.line.max(1),
                            format!(
                                "finish at '{}' as '{}' 不满足 abnormal egress 契约",
                                at, terminal_state
                            ),
                            "abnormal terminal state 只能落在 abnormal_egress_sites".to_string(),
                        ));
                    }
                }
                _ => {}
            },
            StepStatement::Repeat { body, .. } => {
                validate_workpiece_effects_in_statements(body, catalog, workpiece, errors)
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_workpiece_effects_in_statements(
                        &branch.statements,
                        catalog,
                        workpiece,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_workpiece_effects_in_statements(
                        &branch.statements,
                        catalog,
                        workpiece,
                        errors,
                    );
                }
            }
            StepStatement::Action(_)
            | StepStatement::Wait(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
}

fn validate_holder_reference(
    line: usize,
    holder: &str,
    catalog: &WorkpieceCatalog,
    errors: &mut Vec<PlcError>,
) {
    if !catalog.holders.contains(holder) {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            "workpiece_holder",
            holder,
            "请先在 [topology] 中声明对应的 holder".to_string(),
        ));
    }
}

#[allow(dead_code)]
fn validate_workpiece_endpoint_reference(
    line: usize,
    endpoint: &str,
    catalog: &WorkpieceCatalog,
    errors: &mut Vec<PlcError>,
) {
    if catalog.holders.contains(endpoint) {
        return;
    }
    validate_workpiece_location_reference(line, endpoint, catalog, errors);
}

fn validate_and_lower_workpiece_topology_v2(
    topology: &TopologySection,
    constraint_set: &mut ConstraintSet,
    errors: &mut Vec<PlcError>,
) -> WorkpieceCatalog {
    let mut catalog = WorkpieceCatalog::default();
    let mut seen_workpiece_types = HashSet::<String>::new();
    let mut seen_endpoints = HashSet::<String>::new();
    let declared_type_names = topology
        .workpiece_types
        .iter()
        .map(|workpiece| workpiece.name.clone())
        .collect::<HashSet<_>>();

    for site in &topology.workpiece_sites {
        if !seen_endpoints.insert(site.name.clone()) {
            errors.push(PlcError::semantic_with_reason(
                site.line.max(1),
                format!(
                    "workpiece endpoint '{}' is declared more than once",
                    site.name
                ),
                "rename the duplicate workpiece site".to_string(),
            ));
            continue;
        }
        catalog
            .site_kinds
            .insert(site.name.clone(), site.kind.clone());
        constraint_set.workpiece_sites.push(IrWorkpieceSiteDef {
            name: site.name.clone(),
            kind: map_workpiece_site_kind(&site.kind),
            capacity: site.capacity,
        });
    }

    for holder in &topology.workpiece_holders {
        if !seen_endpoints.insert(holder.name.clone()) {
            errors.push(PlcError::semantic_with_reason(
                holder.line.max(1),
                format!(
                    "workpiece endpoint '{}' is declared more than once",
                    holder.name
                ),
                "rename the duplicate workpiece holder".to_string(),
            ));
            continue;
        }
        catalog.holders.insert(holder.name.clone());
        constraint_set.workpiece_holders.push(IrWorkpieceHolderDef {
            name: holder.name.clone(),
            capacity: holder.capacity,
        });
    }

    for carrier in &topology.workpiece_carriers {
        if !seen_endpoints.insert(carrier.name.clone()) {
            errors.push(PlcError::semantic_with_reason(
                carrier.line.max(1),
                format!(
                    "workpiece endpoint '{}' is declared more than once",
                    carrier.name
                ),
                "rename the duplicate workpiece carrier".to_string(),
            ));
            continue;
        }
        let shape = carrier_shape_from_ast(&carrier.layout);
        catalog.carriers.insert(carrier.name.clone(), shape.clone());
        constraint_set
            .workpiece_carriers
            .push(IrWorkpieceCarrierDef {
                name: carrier.name.clone(),
                layout: map_workpiece_carrier_layout(&carrier.layout),
            });
    }

    for workpiece in &topology.workpiece_types {
        if !seen_workpiece_types.insert(workpiece.name.clone()) {
            errors.push(PlcError::semantic_with_reason(
                workpiece.line.max(1),
                format!(
                    "workpiece type '{}' is declared more than once",
                    workpiece.name
                ),
                "merge or rename the duplicate workpiece type".to_string(),
            ));
            continue;
        }

        validate_workpiece_type_declaration_v2(workpiece, &catalog, &declared_type_names, errors);
        constraint_set.workpiece_types.push(IrWorkpieceTypeDef {
            name: workpiece.name.clone(),
            properties: workpiece
                .properties
                .iter()
                .map(|property| IrWorkpiecePropertyDef {
                    name: property.name.clone(),
                    property_type: map_workpiece_property_type(&property.property_type),
                })
                .collect(),
            normal_terminal_states: workpiece.normal_terminal_states.clone(),
            abnormal_terminal_states: workpiece.abnormal_terminal_states.clone(),
            ingress_sites: workpiece.ingress_sites.clone(),
            normal_egress_sites: workpiece.normal_egress_sites.clone(),
            abnormal_egress_sites: workpiece.abnormal_egress_sites.clone(),
            allows: workpiece.allows.iter().map(map_workpiece_allow).collect(),
            derived_from: workpiece
                .derived_from
                .iter()
                .map(map_workpiece_derivation)
                .collect(),
        });
    }

    validate_workpiece_type_contract_alignment(&topology.workpiece_types, errors);

    catalog
}

fn validate_workpiece_type_contract_alignment(
    workpieces: &[AstWorkpieceTypeDeclaration],
    errors: &mut Vec<PlcError>,
) {
    let index = workpieces
        .iter()
        .map(|workpiece| (workpiece.name.clone(), workpiece))
        .collect::<HashMap<_, _>>();

    for workpiece in workpieces {
        for allow in &workpiece.allows {
            let AstWorkpieceAllowDeclaration::SplitInto { target } = allow;
            let Some(target_def) = index.get(target) else {
                continue;
            };
            let has_counterpart = target_def.derived_from.iter().any(|rule| {
                matches!(
                    rule,
                    AstWorkpieceDerivationDeclaration::WorkpieceType { workpiece_type }
                        if workpiece_type == &workpiece.name
                )
            });
            if !has_counterpart {
                errors.push(PlcError::semantic_with_reason(
                    workpiece.line.max(1),
                    format!(
                        "workpiece type '{}' declares split_into({}), but target type '{}' is missing derived_from({})",
                        workpiece.name, target, target, workpiece.name
                    ),
                    "declare the matching derived_from(...) on the split target workpiece type"
                        .to_string(),
                ));
            }
        }

        for derivation in &workpiece.derived_from {
            let AstWorkpieceDerivationDeclaration::WorkpieceType { workpiece_type } = derivation
            else {
                continue;
            };
            let Some(source_def) = index.get(workpiece_type) else {
                continue;
            };
            let has_counterpart = source_def.allows.iter().any(|allow| {
                matches!(
                    allow,
                    AstWorkpieceAllowDeclaration::SplitInto { target }
                        if target == &workpiece.name
                )
            });
            if !has_counterpart {
                errors.push(PlcError::semantic_with_reason(
                    workpiece.line.max(1),
                    format!(
                        "workpiece type '{}' declares derived_from({}), but source type '{}' is missing split_into({})",
                        workpiece.name, workpiece_type, workpiece_type, workpiece.name
                    ),
                    "declare the matching split_into(...) on the source workpiece type"
                        .to_string(),
                ));
            }
        }
    }
}

fn carrier_shape_from_ast(layout: &AstWorkpieceCarrierLayout) -> CarrierShape {
    match layout {
        AstWorkpieceCarrierLayout::Slots { count } => CarrierShape {
            dimensions: vec![*count],
        },
        AstWorkpieceCarrierLayout::Grid { rows, cols } => CarrierShape {
            dimensions: vec![*rows, *cols],
        },
    }
}

fn map_workpiece_carrier_layout(layout: &AstWorkpieceCarrierLayout) -> IrWorkpieceCarrierLayoutDef {
    match layout {
        AstWorkpieceCarrierLayout::Slots { count } => {
            IrWorkpieceCarrierLayoutDef::Slots { count: *count }
        }
        AstWorkpieceCarrierLayout::Grid { rows, cols } => IrWorkpieceCarrierLayoutDef::Grid {
            rows: *rows,
            cols: *cols,
        },
    }
}

fn map_workpiece_allow(allow: &AstWorkpieceAllowDeclaration) -> IrWorkpieceAllowDef {
    match allow {
        AstWorkpieceAllowDeclaration::SplitInto { target } => IrWorkpieceAllowDef::SplitInto {
            target: target.clone(),
        },
    }
}

fn map_workpiece_derivation(
    derivation: &AstWorkpieceDerivationDeclaration,
) -> IrWorkpieceDerivationDef {
    match derivation {
        AstWorkpieceDerivationDeclaration::WorkpieceType { workpiece_type } => {
            IrWorkpieceDerivationDef::WorkpieceType {
                workpiece_type: workpiece_type.clone(),
            }
        }
        AstWorkpieceDerivationDeclaration::Merge { inputs } => IrWorkpieceDerivationDef::Merge {
            inputs: inputs.clone(),
        },
    }
}

fn validate_workpiece_type_declaration_v2(
    workpiece: &AstWorkpieceTypeDeclaration,
    catalog: &WorkpieceCatalog,
    declared_type_names: &HashSet<String>,
    errors: &mut Vec<PlcError>,
) {
    validate_workpiece_type_declaration(workpiece, catalog, errors);

    for allow in &workpiece.allows {
        match allow {
            AstWorkpieceAllowDeclaration::SplitInto { target } => {
                if !declared_type_names.contains(target) {
                    errors.push(PlcError::undefined_reference_with_reason(
                        workpiece.line.max(1),
                        "workpiece_type",
                        target,
                        "declare the target workpiece type before using split_into".to_string(),
                    ));
                }
            }
        }
    }
    let allow_rules = workpiece
        .allows
        .iter()
        .map(|allow| match allow {
            AstWorkpieceAllowDeclaration::SplitInto { target } => {
                format!("split_into({target})")
            }
        })
        .collect::<Vec<_>>();
    validate_unique_workpiece_rule_entries(
        workpiece.line.max(1),
        &workpiece.name,
        "split_into",
        &allow_rules,
        errors,
    );

    for derivation in &workpiece.derived_from {
        match derivation {
            AstWorkpieceDerivationDeclaration::WorkpieceType { workpiece_type } => {
                if !declared_type_names.contains(workpiece_type) {
                    errors.push(PlcError::undefined_reference_with_reason(
                        workpiece.line.max(1),
                        "workpiece_type",
                        workpiece_type,
                        "declare the source workpiece type before using derived_from".to_string(),
                    ));
                }
            }
            AstWorkpieceDerivationDeclaration::Merge { inputs } => {
                if inputs.len() < 2 {
                    errors.push(PlcError::semantic_with_reason(
                        workpiece.line.max(1),
                        format!(
                            "workpiece type '{}' merge derivation needs at least two inputs",
                            workpiece.name
                        ),
                        "declare two or more source workpiece types in merge(...)".to_string(),
                    ));
                }
                for input in inputs {
                    if !declared_type_names.contains(input) {
                        errors.push(PlcError::undefined_reference_with_reason(
                            workpiece.line.max(1),
                            "workpiece_type",
                            input,
                            "declare each merge input workpiece type first".to_string(),
                        ));
                    }
                }
            }
        }
    }
    let derivation_rules = workpiece
        .derived_from
        .iter()
        .map(|rule| match rule {
            AstWorkpieceDerivationDeclaration::WorkpieceType { workpiece_type } => {
                format!("derived_from({workpiece_type})")
            }
            AstWorkpieceDerivationDeclaration::Merge { inputs } => {
                let mut normalized = inputs.clone();
                normalized.sort();
                format!("merge({})", normalized.join(", "))
            }
        })
        .collect::<Vec<_>>();
    validate_unique_workpiece_rule_entries(
        workpiece.line.max(1),
        &workpiece.name,
        "derived_from",
        &derivation_rules,
        errors,
    );
    validate_unambiguous_workpiece_merge_derivations(workpiece, errors);

    for site in workpiece
        .ingress_sites
        .iter()
        .chain(workpiece.normal_egress_sites.iter())
        .chain(workpiece.abnormal_egress_sites.iter())
    {
        validate_workpiece_contract_reference_v2(workpiece.line.max(1), site, catalog, errors);
    }
}

fn validate_unambiguous_workpiece_merge_derivations(
    workpiece: &AstWorkpieceTypeDeclaration,
    errors: &mut Vec<PlcError>,
) {
    let mut seen_by_arity = HashMap::<usize, String>::new();
    for derivation in &workpiece.derived_from {
        let AstWorkpieceDerivationDeclaration::Merge { inputs } = derivation else {
            continue;
        };
        let mut normalized = inputs.clone();
        normalized.sort();
        let rule = format!("merge({})", normalized.join(", "));
        match seen_by_arity.entry(inputs.len()) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(rule);
            }
            std::collections::hash_map::Entry::Occupied(entry) => {
                if entry.get() == &rule {
                    continue;
                }
                errors.push(PlcError::semantic_with_reason(
                    workpiece.line.max(1),
                    format!(
                        "workpiece type '{}' declares multiple merge(...) derivations with {} inputs",
                        workpiece.name,
                        inputs.len()
                    ),
                    "keep at most one merge(...) derivation per input arity in WPM v1"
                        .to_string(),
                ));
            }
        }
    }
}

fn validate_workpiece_contract_reference_v2(
    line: usize,
    endpoint: &str,
    catalog: &WorkpieceCatalog,
    errors: &mut Vec<PlcError>,
) {
    if let Some((carrier, selectors)) = parse_workpiece_slot_reference(endpoint) {
        validate_carrier_slot_reference(line, &carrier, &selectors, true, catalog, errors);
        return;
    }

    match catalog.site_kinds.get(endpoint) {
        Some(AstWorkpieceSiteKind::WorkpieceLocation) => {}
        Some(AstWorkpieceSiteKind::CarrierLocation) => errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "carrier_location '{}' cannot be used as a workpiece ingress/egress",
                endpoint
            ),
            "use a concrete carrier slot or a workpiece_location".to_string(),
        )),
        None => errors.push(PlcError::undefined_reference_with_reason(
            line,
            "workpiece_endpoint",
            endpoint,
            "declare the location or carrier slot in [topology]".to_string(),
        )),
    }
}

fn validate_workpiece_place_reference_v2(
    line: usize,
    endpoint: &str,
    catalog: &WorkpieceCatalog,
    errors: &mut Vec<PlcError>,
) {
    if let Some((carrier, selectors)) = parse_workpiece_slot_reference(endpoint) {
        validate_carrier_slot_reference(line, &carrier, &selectors, false, catalog, errors);
        return;
    }
    validate_workpiece_location_reference(line, endpoint, catalog, errors);
}

fn validate_workpiece_endpoint_reference_v2(
    line: usize,
    endpoint: &str,
    catalog: &WorkpieceCatalog,
    errors: &mut Vec<PlcError>,
) {
    if catalog.holders.contains(endpoint) {
        return;
    }
    validate_workpiece_place_reference_v2(line, endpoint, catalog, errors);
}

fn validate_carrier_slot_reference(
    line: usize,
    carrier: &str,
    selectors: &[String],
    allow_wildcards: bool,
    catalog: &WorkpieceCatalog,
    errors: &mut Vec<PlcError>,
) {
    let Some(shape) = catalog.carriers.get(carrier) else {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            "workpiece_carrier",
            carrier,
            "declare the carrier before using carrier.slot[...]".to_string(),
        ));
        return;
    };

    if selectors.len() != shape.dimensions.len() {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "carrier '{}' expects {} slot dimensions, but '{}' provides {}",
                carrier,
                shape.dimensions.len(),
                format_slot_reference(carrier, selectors),
                selectors.len()
            ),
            "match the slot index arity to the carrier declaration".to_string(),
        ));
        return;
    }

    for (idx, selector) in selectors.iter().enumerate() {
        if selector == "*" {
            if !allow_wildcards {
                errors.push(PlcError::semantic_with_reason(
                    line,
                    format!(
                        "wildcard slot reference '{}' is not allowed in runtime effects",
                        format_slot_reference(carrier, selectors)
                    ),
                    "use a concrete slot index in effect statements".to_string(),
                ));
            }
            continue;
        }
        if let Ok(value) = selector.parse::<u32>() {
            if value >= shape.dimensions[idx] {
                errors.push(PlcError::semantic_with_reason(
                    line,
                    format!(
                        "slot index {} is out of range for carrier '{}' dimension {}",
                        value, carrier, idx
                    ),
                    "keep slot indices within the declared carrier bounds".to_string(),
                ));
            }
        }
    }
}

fn parse_workpiece_slot_reference(raw: &str) -> Option<(String, Vec<String>)> {
    let (carrier, rest) = raw.split_once(".slot[")?;
    let selectors = rest.strip_suffix(']')?;
    let parts = selectors
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if carrier.is_empty() || parts.is_empty() {
        return None;
    }
    Some((carrier.to_string(), parts))
}

fn format_slot_reference(carrier: &str, selectors: &[String]) -> String {
    format!("{}.slot[{}]", carrier, selectors.join(", "))
}

fn validate_workpiece_effects_in_tasks_v2(
    tasks: &TasksSection,
    catalog: &WorkpieceCatalog,
    workpiece_types: &[IrWorkpieceTypeDef],
    errors: &mut Vec<PlcError>,
) {
    if workpiece_types.is_empty() {
        errors.push(PlcError::semantic_with_reason(
            1,
            "workpiece effects require at least one workpiece type".to_string(),
            "declare a workpiece type in [topology] before using effect statements".to_string(),
        ));
        return;
    }

    for task in &tasks.tasks {
        for step in &task.steps {
            validate_workpiece_effects_in_statements_v2(
                &step.statements,
                catalog,
                workpiece_types,
                errors,
            );
        }
    }
}

fn validate_workpiece_effects_in_statements_v2(
    statements: &[StepStatement],
    catalog: &WorkpieceCatalog,
    workpiece_types: &[IrWorkpieceTypeDef],
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Effect(effect) => match &effect.kind {
                AstEffectKind::Acquire { holder, from } => {
                    validate_holder_reference(effect.line.max(1), holder, catalog, errors);
                    validate_workpiece_place_reference_v2(
                        effect.line.max(1),
                        from,
                        catalog,
                        errors,
                    );
                    if workpiece_types.len() != 1 {
                        errors.push(PlcError::semantic_with_reason(
                            effect.line.max(1),
                            "acquire/transfer/finish effects remain single-type in this phase"
                                .to_string(),
                            "use one workpiece type per flow when relying on untyped transfer effects"
                                .to_string(),
                        ));
                    }
                }
                AstEffectKind::Transfer { from, to } => {
                    validate_workpiece_endpoint_reference_v2(
                        effect.line.max(1),
                        from,
                        catalog,
                        errors,
                    );
                    validate_workpiece_endpoint_reference_v2(
                        effect.line.max(1),
                        to,
                        catalog,
                        errors,
                    );
                    if workpiece_types.len() != 1 {
                        errors.push(PlcError::semantic_with_reason(
                            effect.line.max(1),
                            "acquire/transfer/finish effects remain single-type in this phase"
                                .to_string(),
                            "use one workpiece type per flow when relying on untyped transfer effects"
                                .to_string(),
                        ));
                    }
                }
                AstEffectKind::Finish { at, terminal_state } => {
                    validate_workpiece_place_reference_v2(effect.line.max(1), at, catalog, errors);
                    if workpiece_types.len() != 1 {
                        errors.push(PlcError::semantic_with_reason(
                            effect.line.max(1),
                            "finish remains single-type in this phase".to_string(),
                            "use one workpiece type per flow when relying on untyped finish"
                                .to_string(),
                        ));
                        continue;
                    }
                    let workpiece = &workpiece_types[0];
                    let normal = workpiece
                        .normal_terminal_states
                        .iter()
                        .any(|state| state == terminal_state);
                    let abnormal = workpiece
                        .abnormal_terminal_states
                        .iter()
                        .any(|state| state == terminal_state);
                    if !normal && !abnormal {
                        errors.push(PlcError::semantic_with_reason(
                            effect.line.max(1),
                            format!(
                                "terminal state '{}' is not declared on workpiece type '{}'",
                                terminal_state, workpiece.name
                            ),
                            "declare the terminal state before using finish".to_string(),
                        ));
                    }
                    let candidates = if normal {
                        &workpiece.normal_egress_sites
                    } else {
                        &workpiece.abnormal_egress_sites
                    };
                    if (normal || abnormal)
                        && !candidates
                            .iter()
                            .any(|candidate| workpiece_endpoint_matches_pattern(at, candidate))
                    {
                        errors.push(PlcError::semantic_with_reason(
                            effect.line.max(1),
                            format!("finish endpoint '{}' does not satisfy the declared egress contract", at),
                            "finish at a declared egress location or carrier slot".to_string(),
                        ));
                    }
                }
                AstEffectKind::Mount {
                    workpiece_type,
                    slot,
                } => {
                    validate_declared_workpiece_type(
                        effect.line.max(1),
                        workpiece_type,
                        workpiece_types,
                        errors,
                    );
                    validate_workpiece_place_reference_v2(
                        effect.line.max(1),
                        slot,
                        catalog,
                        errors,
                    );
                }
                AstEffectKind::Unmount {
                    workpiece_type,
                    slot,
                    to,
                } => {
                    validate_declared_workpiece_type(
                        effect.line.max(1),
                        workpiece_type,
                        workpiece_types,
                        errors,
                    );
                    validate_workpiece_place_reference_v2(
                        effect.line.max(1),
                        slot,
                        catalog,
                        errors,
                    );
                    validate_workpiece_place_reference_v2(effect.line.max(1), to, catalog, errors);
                }
                AstEffectKind::Split {
                    source_type,
                    target_type,
                    count,
                    consumed: _,
                } => {
                    validate_declared_workpiece_type(
                        effect.line.max(1),
                        source_type,
                        workpiece_types,
                        errors,
                    );
                    validate_declared_workpiece_type(
                        effect.line.max(1),
                        target_type,
                        workpiece_types,
                        errors,
                    );
                    if *count == 0 {
                        errors.push(PlcError::semantic_with_reason(
                            effect.line.max(1),
                            "split count must be greater than zero".to_string(),
                            "use a finite positive count for split".to_string(),
                        ));
                    }
                    if let Some(source_def) = find_workpiece_type(workpiece_types, source_type) {
                        let allowed = source_def.allows.iter().any(|allow| {
                            matches!(allow, IrWorkpieceAllowDef::SplitInto { target } if target == target_type)
                        });
                        if !allowed {
                            errors.push(PlcError::semantic_with_reason(
                                effect.line.max(1),
                                format!(
                                    "workpiece type '{}' does not allow split_into({})",
                                    source_type, target_type
                                ),
                                "declare split_into(...) on the source workpiece type".to_string(),
                            ));
                        }
                    }
                    if let Some(target_def) = find_workpiece_type(workpiece_types, target_type) {
                        let derived = target_def.derived_from.iter().any(|rule| {
                            matches!(
                                rule,
                                IrWorkpieceDerivationDef::WorkpieceType { workpiece_type } if workpiece_type == source_type
                            )
                        });
                        if !derived {
                            errors.push(PlcError::semantic_with_reason(
                                effect.line.max(1),
                                format!(
                                    "workpiece type '{}' is not derived_from '{}'",
                                    target_type, source_type
                                ),
                                "declare derived_from on the split output workpiece type"
                                    .to_string(),
                            ));
                        }
                    }
                }
                AstEffectKind::Merge {
                    inputs,
                    target_type,
                    consumed_inputs: _,
                } => {
                    validate_declared_workpiece_type(
                        effect.line.max(1),
                        target_type,
                        workpiece_types,
                        errors,
                    );
                    if inputs.len() < 2 {
                        errors.push(PlcError::semantic_with_reason(
                            effect.line.max(1),
                            "merge requires at least two inputs".to_string(),
                            "list two or more explicit merge inputs".to_string(),
                        ));
                    }
                    if let Some(target_def) = find_workpiece_type(workpiece_types, target_type) {
                        let matches_merge = target_def.derived_from.iter().any(|rule| {
                            matches!(rule, IrWorkpieceDerivationDef::Merge { inputs: expected } if expected.len() == inputs.len())
                        });
                        if !matches_merge {
                            errors.push(PlcError::semantic_with_reason(
                                effect.line.max(1),
                                format!(
                                    "workpiece type '{}' has no merge(...) derivation matching {} inputs",
                                    target_type,
                                    inputs.len()
                                ),
                                "declare a merge(...) derivation on the target workpiece type".to_string(),
                            ));
                        }
                    }
                }
                AstEffectKind::TransformCarrier { carrier, .. } => {
                    if !catalog.carriers.contains_key(carrier) {
                        errors.push(PlcError::undefined_reference_with_reason(
                            effect.line.max(1),
                            "workpiece_carrier",
                            carrier,
                            "declare the carrier before transforming it".to_string(),
                        ));
                    }
                }
            },
            StepStatement::Repeat { body, .. } => {
                validate_workpiece_effects_in_statements_v2(body, catalog, workpiece_types, errors)
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_workpiece_effects_in_statements_v2(
                        &branch.statements,
                        catalog,
                        workpiece_types,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_workpiece_effects_in_statements_v2(
                        &branch.statements,
                        catalog,
                        workpiece_types,
                        errors,
                    );
                }
            }
            StepStatement::Action(_)
            | StepStatement::Wait(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
}

fn validate_declared_workpiece_type(
    line: usize,
    name: &str,
    workpiece_types: &[IrWorkpieceTypeDef],
    errors: &mut Vec<PlcError>,
) {
    if find_workpiece_type(workpiece_types, name).is_none() {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            "workpiece_type",
            name,
            "declare the workpiece type in [topology] first".to_string(),
        ));
    }
}

fn find_workpiece_type<'a>(
    workpiece_types: &'a [IrWorkpieceTypeDef],
    name: &str,
) -> Option<&'a IrWorkpieceTypeDef> {
    workpiece_types
        .iter()
        .find(|workpiece| workpiece.name == name)
}

fn workpiece_endpoint_matches_pattern(endpoint: &str, pattern: &str) -> bool {
    if endpoint == pattern {
        return true;
    }
    let Some((endpoint_carrier, endpoint_selectors)) = parse_workpiece_slot_reference(endpoint)
    else {
        return false;
    };
    let Some((pattern_carrier, pattern_selectors)) = parse_workpiece_slot_reference(pattern) else {
        return false;
    };
    if endpoint_carrier != pattern_carrier || endpoint_selectors.len() != pattern_selectors.len() {
        return false;
    }
    endpoint_selectors
        .iter()
        .zip(pattern_selectors.iter())
        .all(|(value, pattern)| pattern == "*" || value == pattern)
}

pub fn build_timing_model_from_ast(
    topology: &TopologySection,
    tasks: &TasksSection,
) -> Result<TimingModel, Vec<PlcError>> {
    let device_profiles = collect_device_timing_profiles(topology);
    let mut intervals = BTreeMap::new();
    let mut errors = Vec::new();

    for task in &tasks.tasks {
        for step in &task.steps {
            let mut actions = Vec::new();
            collect_actions(&step.statements, &mut actions);

            for action in actions {
                if let Some(action_timing) = action_to_timing(
                    &task.name,
                    &step.name,
                    step.line,
                    &action,
                    &device_profiles,
                    &mut errors,
                ) {
                    insert_action_timing(&mut intervals, action_timing);
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(TimingModel { intervals })
    } else {
        Err(errors)
    }
}

