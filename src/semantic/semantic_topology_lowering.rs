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
            device_semantics::motor::validate_legacy_set_actions(
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

