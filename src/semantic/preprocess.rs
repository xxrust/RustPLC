const CONTROLLER_PROFILES_DIR: &str = "devices/controllers";

#[derive(Debug, Clone)]
struct DeviceNode {
    index: NodeIndex,
    kind: DeviceKind,
}

/// Expand syntax sugar in the AST before semantic lowering.
///
/// Currently this performs compile-time `repeat N:` expansion by rewriting it into `N` sequential
/// steps named with `_1.._N` suffixes.
pub fn preprocess_program(program: &PlcProgram) -> Result<PlcProgram, Vec<PlcError>> {
    preprocess_program_with_library(program, None)
}

/// Like `preprocess_program`, but also injects device-library constraints when a library is provided.
pub fn preprocess_program_with_library(
    program: &PlcProgram,
    device_library: Option<&crate::device_library::DeviceLibrary>,
) -> Result<PlcProgram, Vec<PlcError>> {
    let expanded_tasks = expand_repeat_blocks(&program.tasks)?;
    let used_controller_ports =
        collect_used_controller_port_ids(&program.topology, &program.constraints, &expanded_tasks);
    let expanded_topology =
        expand_plc_controller_devices(&program.topology, &used_controller_ports)?;
    let mut rewritten = program.clone();
    rewritten.tasks = expanded_tasks;
    rewritten.topology = expanded_topology;

    if let Some(library) = device_library {
        if !library.is_empty() {
            inject_device_constraints(&mut rewritten, library)?;
        }
    }

    Ok(rewritten)
}

fn device_type_str(device_type: &DeviceType) -> &'static str {
    match device_type {
        DeviceType::DigitalOutput => "digital_output",
        DeviceType::DigitalInput => "digital_input",
        DeviceType::Plc => "plc",
        DeviceType::SolenoidValve => "solenoid_valve",
        DeviceType::Cylinder => "cylinder",
        DeviceType::Sensor => "sensor",
        DeviceType::Motor => "motor",
        DeviceType::StepperMotor => "stepper_motor",
        DeviceType::Vfd => "vfd",
        DeviceType::ServoDrive => "servo_drive",
        DeviceType::CamCoupling => "cam_coupling",
        DeviceType::AnalogInput => "analog_input",
        DeviceType::AnalogOutput => "analog_output",
        DeviceType::Pid => "pid",
    }
}

fn inject_device_constraints(
    program: &mut PlcProgram,
    library: &crate::device_library::DeviceLibrary,
) -> Result<(), Vec<PlcError>> {
    let mut errors = Vec::new();

    for device in &mut program.topology.devices {
        let type_key = device_type_str(&device.device_type);
        let Some(def) = library.get(type_key) else {
            continue;
        };
        validate_device_extra_params(device, type_key, def, &mut errors);
        let declared_port_ids = known_port_ids(device);

        // Inject port states from library into device ports
        for lib_port in &def.interfaces.ports {
            if let Some(existing) = device
                .attributes
                .ports
                .iter_mut()
                .find(|p| p.id == lib_port.name)
            {
                if existing.states.is_empty() {
                    existing.states = lib_port.states.clone();
                }
                if existing.default_state.is_empty() {
                    existing.default_state = lib_port.default_state.clone();
                }
            }
            // Don't auto-register ports not declared in DSL — the library enriches, not overrides
        }

        // Expand device-library safety constraints into AST constraints
        for lib_safety in &def.device_constraints.safety {
            let Some((left_port, _left_state)) = lib_safety.left.split_once('.') else {
                errors.push(PlcError::device_library_invalid_port_ref(
                    &lib_safety.left,
                    &device.name,
                ));
                continue;
            };
            let Some((right_port, _right_state)) = lib_safety.right.split_once('.') else {
                errors.push(PlcError::device_library_invalid_port_ref(
                    &lib_safety.right,
                    &device.name,
                ));
                continue;
            };
            // Library constraints should only apply when the device exposes the referenced ports.
            if !declared_port_ids.contains(left_port) || !declared_port_ids.contains(right_port) {
                continue;
            }
            let left = match expand_port_state_ref(&lib_safety.left, &device.name) {
                Ok(sr) => SafetyOperand::State(sr),
                Err(e) => {
                    errors.push(e);
                    continue;
                }
            };
            let right = match expand_port_state_ref(&lib_safety.right, &device.name) {
                Ok(sr) => SafetyOperand::State(sr),
                Err(e) => {
                    errors.push(e);
                    continue;
                }
            };
            let relation = match lib_safety.relation.as_str() {
                "conflicts_with" => AstSafetyRelation::ConflictsWith,
                "requires" => AstSafetyRelation::Requires,
                other => {
                    errors.push(PlcError::semantic(
                        0,
                        format!("设备库约束关系未知: {other} (设备实例: {})", device.name),
                    ));
                    continue;
                }
            };

            if program.constraints.safety.iter().any(|existing| {
                safety_relations_match(&existing.relation, &relation)
                    && safety_operands_match(&existing.left, &left)
                    && safety_operands_match(&existing.right, &right)
            }) {
                continue;
            }

            program.constraints.safety.push(SafetyConstraint {
                line: 0,
                left,
                relation,
                right,
                reason: lib_safety.reason.clone(),
                source: Some(format!("device:{}", type_key)),
            });
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn safety_relations_match(left: &AstSafetyRelation, right: &AstSafetyRelation) -> bool {
    matches!(
        (left, right),
        (
            AstSafetyRelation::ConflictsWith,
            AstSafetyRelation::ConflictsWith
        ) | (AstSafetyRelation::Requires, AstSafetyRelation::Requires)
    )
}

fn safety_operands_match(left: &SafetyOperand, right: &SafetyOperand) -> bool {
    match (left, right) {
        (SafetyOperand::State(left), SafetyOperand::State(right)) => {
            left.device == right.device && left.port == right.port && left.state == right.state
        }
        (
            SafetyOperand::Threshold {
                device: left_device,
                operator: left_operator,
                value: left_value,
                unit: left_unit,
            },
            SafetyOperand::Threshold {
                device: right_device,
                operator: right_operator,
                value: right_value,
                unit: right_unit,
            },
        ) => {
            left_device == right_device
                && comparison_operators_match(left_operator, right_operator)
                && left_value == right_value
                && left_unit == right_unit
        }
        _ => false,
    }
}

fn comparison_operators_match(left: &ComparisonOperator, right: &ComparisonOperator) -> bool {
    matches!(
        (left, right),
        (ComparisonOperator::Gt, ComparisonOperator::Gt)
            | (ComparisonOperator::Lt, ComparisonOperator::Lt)
            | (ComparisonOperator::Gte, ComparisonOperator::Gte)
            | (ComparisonOperator::Lte, ComparisonOperator::Lte)
            | (ComparisonOperator::Eq, ComparisonOperator::Eq)
            | (ComparisonOperator::Neq, ComparisonOperator::Neq)
    )
}

fn validate_device_extra_params(
    device: &DeviceDeclaration,
    type_key: &str,
    def: &crate::device_library::DeviceDef,
    errors: &mut Vec<PlcError>,
) {
    if def.parameters.is_empty() {
        if !device.attributes.extra_params.is_empty() {
            for param_name in device.attributes.extra_params.keys() {
                errors.push(PlcError::semantic_with_reason(
                    device.line.max(1),
                    format!(
                        "设备 {}({}) 参数 `{param_name}` 未在设备库 parameters 中声明",
                        device.name, type_key
                    ),
                    "请在 devices/<type>.toml 中补充 [[parameters]] 定义，或移除该参数".to_string(),
                ));
            }
        }
        return;
    }

    let mut schema_by_name = HashMap::<String, &crate::device_library::DeviceParameterDef>::new();
    for parameter in &def.parameters {
        schema_by_name.insert(parameter.name.clone(), parameter);
    }

    for (name, raw_value) in &device.attributes.extra_params {
        let Some(schema) = schema_by_name.get(name) else {
            errors.push(PlcError::semantic_with_reason(
                device.line.max(1),
                format!(
                    "设备 {}({}) 参数 `{name}` 未在设备库 parameters 中声明",
                    device.name, type_key
                ),
                "请检查参数名拼写，或在设备库中添加该参数定义".to_string(),
            ));
            continue;
        };

        if let Err(reason) = validate_extra_param_value(raw_value, schema) {
            errors.push(PlcError::semantic_with_reason(
                device.line.max(1),
                format!(
                    "设备 {}({}) 参数 `{name}` 的值 `{raw_value}` 无效",
                    device.name, type_key
                ),
                reason,
            ));
        }
    }

    for parameter in &def.parameters {
        if !parameter.required || device.attributes.extra_params.contains_key(&parameter.name) {
            continue;
        }
        if !parameter.default.trim().is_empty() {
            continue;
        }
        errors.push(PlcError::semantic_with_reason(
            device.line.max(1),
            format!(
                "设备 {}({}) 缺少必填参数 `{}`",
                device.name, type_key, parameter.name
            ),
            "请在设备声明中补充该参数，或为设备库参数提供默认值".to_string(),
        ));
    }
}

fn validate_extra_param_value(
    raw_value: &str,
    schema: &crate::device_library::DeviceParameterDef,
) -> Result<(), String> {
    let kind = schema.parameter_type.trim().to_ascii_lowercase();
    match kind.as_str() {
        "integer" | "int" | "u32" | "i32" => raw_value
            .trim()
            .parse::<i64>()
            .map(|_| ())
            .map_err(|_| "参数类型要求 integer（示例：200）".to_string()),
        "float" | "number" | "ratio" => raw_value
            .trim()
            .parse::<f64>()
            .map(|_| ())
            .or_else(|_| validate_number_with_optional_unit(raw_value, &schema.unit))
            .map_err(|_| "参数类型要求 number（示例：12.5 或 2.2kW）".to_string()),
        "time" => validate_time_param(raw_value),
        "boolean" | "bool" => {
            let normalized = strip_wrapping_quotes(raw_value.trim())
                .to_ascii_lowercase()
                .to_string();
            if normalized == "true" || normalized == "false" {
                Ok(())
            } else {
                Err("参数类型要求 boolean（true/false）".to_string())
            }
        }
        "enum" => {
            let candidate = strip_wrapping_quotes(raw_value.trim());
            if schema.options.iter().any(|option| option == candidate) {
                Ok(())
            } else {
                Err(format!(
                    "参数类型要求 enum，合法值: {}",
                    schema.options.join(", ")
                ))
            }
        }
        "length" | "pressure" => validate_numeric_with_optional_unit(raw_value, &schema.unit),
        other => Err(format!(
            "设备库参数类型 `{other}` 暂不支持；请使用 integer/number/time/boolean/enum/length/pressure"
        )),
    }
}

fn validate_time_param(raw_value: &str) -> Result<(), String> {
    let trimmed = raw_value.trim();
    if trimmed.len() <= 2 {
        return Err("参数类型要求 time（示例：100ms 或 2s）".to_string());
    }

    if let Some(number) = trimmed.strip_suffix("ms") {
        number
            .trim()
            .parse::<f64>()
            .map_err(|_| "time 参数格式错误，应为 <number>ms".to_string())?;
        return Ok(());
    }

    if let Some(number) = trimmed.strip_suffix('s') {
        number
            .trim()
            .parse::<f64>()
            .map_err(|_| "time 参数格式错误，应为 <number>s".to_string())?;
        return Ok(());
    }

    Err("参数类型要求 time（示例：100ms 或 2s）".to_string())
}

fn validate_numeric_with_optional_unit(raw_value: &str, expected_unit: &str) -> Result<(), String> {
    let trimmed = raw_value.trim();
    if trimmed.is_empty() {
        return Err("参数值不能为空".to_string());
    }

    let split_index = trimmed
        .char_indices()
        .find(|(_, ch)| {
            !(ch.is_ascii_digit() || *ch == '.' || *ch == '-' || *ch == '+' || *ch == '_')
        })
        .map(|(idx, _)| idx)
        .unwrap_or(trimmed.len());

    let (number_part, unit_part) = trimmed.split_at(split_index);
    if number_part.trim().is_empty() {
        return Err("参数值缺少数字部分".to_string());
    }

    number_part
        .trim()
        .replace('_', "")
        .parse::<f64>()
        .map_err(|_| "参数值的数字部分解析失败".to_string())?;

    let normalized_unit = unit_part.trim();
    if expected_unit.trim().is_empty() {
        return Ok(());
    }

    if normalized_unit == expected_unit.trim() {
        Ok(())
    } else if normalized_unit.is_empty() {
        Err(format!("参数缺少单位，应为 `{}`", expected_unit.trim()))
    } else {
        Err(format!(
            "参数单位不匹配，期望 `{}`，实际 `{normalized_unit}`",
            expected_unit.trim()
        ))
    }
}

fn validate_number_with_optional_unit(raw_value: &str, expected_unit: &str) -> Result<(), String> {
    let trimmed = raw_value.trim();
    if trimmed.parse::<f64>().is_ok() {
        return Ok(());
    }

    let split_index = trimmed
        .char_indices()
        .find(|(_, ch)| {
            !(ch.is_ascii_digit() || *ch == '.' || *ch == '-' || *ch == '+' || *ch == '_')
        })
        .map(|(idx, _)| idx)
        .unwrap_or(trimmed.len());
    let (number_part, unit_part) = trimmed.split_at(split_index);
    if number_part.trim().is_empty() {
        return Err("参数值缺少数字部分".to_string());
    }

    number_part
        .trim()
        .replace('_', "")
        .parse::<f64>()
        .map_err(|_| "参数值的数字部分解析失败".to_string())?;

    let actual_unit = unit_part.trim();
    let expected_unit = expected_unit.trim();
    if expected_unit.is_empty() {
        return Ok(());
    }
    if actual_unit.is_empty() || actual_unit == expected_unit {
        Ok(())
    } else {
        Err(format!(
            "参数单位不匹配，期望 `{expected_unit}`，实际 `{actual_unit}`"
        ))
    }
}

fn strip_wrapping_quotes(raw: &str) -> &str {
    if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') {
        &raw[1..raw.len() - 1]
    } else {
        raw
    }
}

fn known_port_ids(device: &DeviceDeclaration) -> HashSet<String> {
    if !device.attributes.ports.is_empty() {
        return device
            .attributes
            .ports
            .iter()
            .map(|port| port.id.clone())
            .collect();
    }

    implicit_port_ids_for_device_type(&device.device_type)
        .iter()
        .map(|port| (*port).to_string())
        .collect()
}

fn implicit_port_ids_for_device_type(device_type: &DeviceType) -> &'static [&'static str] {
    match device_type {
        DeviceType::DigitalOutput => &["out"],
        DeviceType::DigitalInput => &["in"],
        DeviceType::Plc => &[],
        DeviceType::SolenoidValve => &["coil", "out"],
        DeviceType::Cylinder => &["cmd", "extended", "retracted"],
        DeviceType::Sensor => &["sense", "out"],
        DeviceType::Motor => &["run", "direction", "running", "fault", "cmd", "on"],
        DeviceType::StepperMotor => &["enable", "direction", "pulse", "fault"],
        DeviceType::Vfd => &["run", "direction", "running", "fault", "freq_arrive"],
        DeviceType::ServoDrive => &[
            "enable",
            "direction",
            "pulse",
            "clear_fault",
            "ready",
            "in_position",
            "fault",
            "zero_speed",
        ],
        DeviceType::CamCoupling => &[
            "engage",
            "in_sync",
            "fault",
            "following_error",
            "master_pos",
            "slave_cmd",
        ],
        DeviceType::AnalogInput => &["in"],
        DeviceType::AnalogOutput => &["out"],
        DeviceType::Pid => &["in", "out"],
    }
}

fn expand_port_state_ref(port_state: &str, instance: &str) -> Result<StateReference, PlcError> {
    let (port, state) = port_state
        .split_once('.')
        .ok_or_else(|| PlcError::device_library_invalid_port_ref(port_state, instance))?;
    Ok(StateReference {
        device: instance.to_string(),
        port: port.to_string(),
        state: state.to_string(),
    })
}

#[derive(Debug, Clone)]
struct ResolvedPlcEndpoint {
    name: String,
}

#[derive(Debug, Clone)]
struct ResolvedControllerPort {
    port: crate::ast::DevicePort,
    analog_range: Option<crate::ast::AnalogRange>,
    unit: Option<String>,
    external: bool,
}

fn collect_used_controller_port_ids(
    topology: &TopologySection,
    constraints: &ConstraintsSection,
    tasks: &TasksSection,
) -> HashSet<String> {
    let plc_names = topology
        .devices
        .iter()
        .filter(|device| matches!(device.device_type, DeviceType::Plc))
        .map(|device| device.name.clone())
        .collect::<HashSet<_>>();
    let mut used = HashSet::new();

    for connection in &topology.connections {
        collect_used_controller_port_from_relation_endpoint(
            &connection.from,
            connection.from_port.as_deref(),
            &plc_names,
            &mut used,
        );
        collect_used_controller_port_from_relation_endpoint(
            &connection.to,
            connection.to_port.as_deref(),
            &plc_names,
            &mut used,
        );
    }

    for rule in &constraints.safety {
        collect_used_controller_port_from_safety_operand(&rule.left, &plc_names, &mut used);
        collect_used_controller_port_from_safety_operand(&rule.right, &plc_names, &mut used);
    }

    for task in &tasks.tasks {
        for step in &task.steps {
            collect_used_controller_port_ids_from_statements(
                &step.statements,
                &plc_names,
                &mut used,
            );
        }
    }

    used
}

fn collect_used_controller_port_from_relation_endpoint(
    device: &str,
    port: Option<&str>,
    plc_names: &HashSet<String>,
    used: &mut HashSet<String>,
) {
    if let Some(port) = port {
        if plc_names.contains(device) {
            collect_used_controller_port_reference(port, plc_names, used);
        }
        return;
    }

    collect_used_controller_port_reference(device, plc_names, used);
}

fn collect_used_controller_port_from_safety_operand(
    operand: &SafetyOperand,
    plc_names: &HashSet<String>,
    used: &mut HashSet<String>,
) {
    match operand {
        SafetyOperand::State(state) => {
            if plc_names.contains(&state.device) {
                collect_used_controller_port_reference(&state.port, plc_names, used);
            } else {
                collect_used_controller_port_reference(&state.device, plc_names, used);
            }
        }
        SafetyOperand::Threshold { device, .. } => {
            collect_used_controller_port_reference(device, plc_names, used);
        }
    }
}

fn collect_used_controller_port_ids_from_statements(
    statements: &[StepStatement],
    plc_names: &HashSet<String>,
    used: &mut HashSet<String>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(action) => {
                collect_used_controller_port_from_action(action, plc_names, used);
            }
            StepStatement::Wait(wait) => {
                let conditions = match &wait.condition {
                    WaitCondition::Single(condition) => vec![condition],
                    WaitCondition::And(conditions) | WaitCondition::Or(conditions) => {
                        conditions.iter().collect::<Vec<_>>()
                    }
                };
                for condition in conditions {
                    if let Some((left_expr, right_expr)) = condition.expression_pair() {
                        collect_used_controller_port_from_expression(left_expr, plc_names, used);
                        collect_used_controller_port_from_expression(right_expr, plc_names, used);
                    } else {
                        collect_used_controller_port_reference(&condition.left, plc_names, used);
                    }
                }
            }
            StepStatement::IfElse { condition, .. } => {
                if let Some((left_expr, right_expr)) = condition.expression_pair() {
                    collect_used_controller_port_from_expression(left_expr, plc_names, used);
                    collect_used_controller_port_from_expression(right_expr, plc_names, used);
                } else {
                    collect_used_controller_port_reference(&condition.left, plc_names, used);
                }
            }
            StepStatement::Repeat { body, .. } => {
                collect_used_controller_port_ids_from_statements(body, plc_names, used);
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    collect_used_controller_port_ids_from_statements(
                        &branch.statements,
                        plc_names,
                        used,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    collect_used_controller_port_ids_from_statements(
                        &branch.statements,
                        plc_names,
                        used,
                    );
                }
            }
            StepStatement::Effect(_)
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
}

fn collect_used_controller_port_from_action(
    action: &ActionStatement,
    plc_names: &HashSet<String>,
    used: &mut HashSet<String>,
) {
    match action {
        ActionStatement::Extend { target, .. }
        | ActionStatement::Retract { target, .. }
        | ActionStatement::Set { target, .. }
        | ActionStatement::SetAnalog { target, .. }
        | ActionStatement::SetAnalogExpr { target, .. }
        | ActionStatement::AxisMoveRelative { target, .. }
        | ActionStatement::AxisMoveAbsolute { target, .. } => {
            if plc_names.contains(&target.device) {
                collect_used_controller_port_reference(&target.port, plc_names, used);
            } else {
                collect_used_controller_port_reference(&target.device, plc_names, used);
            }
        }
        ActionStatement::Compute { target, expr } => {
            collect_used_controller_port_reference(target, plc_names, used);
            collect_used_controller_port_from_expression(expr, plc_names, used);
        }
        ActionStatement::Call { args, binding, .. } => {
            for arg in args {
                collect_used_controller_port_from_expression(arg, plc_names, used);
            }
            match binding {
                AstExternCallBinding::Single(target) => {
                    collect_used_controller_port_reference(target, plc_names, used);
                }
                AstExternCallBinding::Tuple(targets) => {
                    for target in targets {
                        collect_used_controller_port_reference(target, plc_names, used);
                    }
                }
            }
        }
        ActionStatement::CamEngage { target }
        | ActionStatement::CamDisengage { target }
        | ActionStatement::CamSwitch { target, .. } => {
            collect_used_controller_port_reference(target, plc_names, used);
        }
        ActionStatement::CamPhase { target, offset } => {
            collect_used_controller_port_reference(target, plc_names, used);
            collect_used_controller_port_from_expression(offset, plc_names, used);
        }
        ActionStatement::Log { .. } => {}
    }
}

fn collect_used_controller_port_from_expression(
    expr: &AstExpression,
    plc_names: &HashSet<String>,
    used: &mut HashSet<String>,
) {
    match expr {
        AstExpression::Variable(name) => {
            collect_used_controller_port_reference(name, plc_names, used);
        }
        AstExpression::UnaryNeg(inner) | AstExpression::UnaryNot(inner) => {
            collect_used_controller_port_from_expression(inner, plc_names, used);
        }
        AstExpression::BinaryOp { left, right, .. } => {
            collect_used_controller_port_from_expression(left, plc_names, used);
            collect_used_controller_port_from_expression(right, plc_names, used);
        }
        AstExpression::FunctionCall { args, .. } => {
            for arg in args {
                collect_used_controller_port_from_expression(arg, plc_names, used);
            }
        }
        AstExpression::Literal(_) | AstExpression::Boolean(_) => {}
    }
}

fn collect_used_controller_port_reference(
    reference: &str,
    plc_names: &HashSet<String>,
    used: &mut HashSet<String>,
) {
    if let Some(port_ref) = parse_plc_port_ref(reference) {
        used.insert(canonical_physical_device_name(port_ref.kind, port_ref.id));
        return;
    }

    if let Some((device, port)) = reference.split_once('.') {
        if plc_names.contains(device) {
            if let Some(port_ref) = parse_plc_port_ref(port) {
                used.insert(canonical_physical_device_name(port_ref.kind, port_ref.id));
            }
        }
    }
}

fn expand_plc_controller_devices(
    topology: &TopologySection,
    used_controller_ports: &HashSet<String>,
) -> Result<TopologySection, Vec<PlcError>> {
    let mut errors = Vec::<PlcError>::new();
    let mut rewritten_devices = topology
        .devices
        .iter()
        .filter(|device| !matches!(device.device_type, DeviceType::Plc))
        .cloned()
        .collect::<Vec<_>>();
    let mut existing_names = rewritten_devices
        .iter()
        .map(|device| (device.name.clone(), device.device_type.clone()))
        .collect::<HashMap<_, _>>();

    let plc_devices = topology
        .devices
        .iter()
        .filter(|device| matches!(device.device_type, DeviceType::Plc))
        .collect::<Vec<_>>();
    if plc_devices.is_empty() {
        return Ok(topology.clone());
    }

    let mut port_lookup = HashMap::<(String, String), ResolvedPlcEndpoint>::new();
    let mut synthetic_declarations = HashMap::<String, DeviceDeclaration>::new();

    for plc in plc_devices {
        let plc_ports = match resolve_plc_ports(plc) {
            Ok(ports) => ports,
            Err(mut local_errors) => {
                errors.append(&mut local_errors);
                continue;
            }
        };
        let mut seen_ports = BTreeSet::<String>::new();
        for port in &plc_ports {
            if !seen_ports.insert(port.port.id.clone()) {
                errors.push(PlcError::duplicate_definition_with_reason(
                    plc.line.max(1),
                    "端口",
                    &format!("{}.{}", plc.name, port.port.id),
                    "PLC 设备的端口 id 不能重复",
                ));
                continue;
            }

            let Some(port_ref) = parse_plc_port_ref(&port.port.id) else {
                errors.push(PlcError::semantic_with_reason(
                    plc.line.max(1),
                    format!(
                        "PLC 设备 {} 的端口 {} 不是有效 PLC 通道（支持 X*/Y*/AI*/AO*/DI*/DO*）",
                        plc.name, port.port.id
                    ),
                    "请将 plc 端口命名为 X0/Y0/AI0/AO0 或 DI0/DO0 形式",
                ));
                continue;
            };

            let expected_type = expected_plc_port_type(port_ref.kind);
            if port.port.port_type != expected_type {
                errors.push(PlcError::type_mismatch_with_reason(
                    plc.line.max(1),
                    port_type_name(&expected_type),
                    port_type_name(&port.port.port_type),
                    format!("PLC 端口 {}.{}", plc.name, port.port.id),
                    "请修正 plc 端口类型，使其与端口编号前缀一致（X/DI=Digital, Y/DO=Digital, AI=Analog, AO=Analog）",
                ));
                continue;
            }

            let expected_role = expected_plc_port_role(port_ref.kind);
            if port.port.role != expected_role && port.port.role != PortRole::Bidirectional {
                errors.push(PlcError::type_mismatch_with_reason(
                    plc.line.max(1),
                    port_role_name(&expected_role),
                    port_role_name(&port.port.role),
                    format!("PLC 端口 {}.{}", plc.name, port.port.id),
                    "请修正 plc 端口方向：输入端口应为 consumer，输出端口应为 producer",
                ));
                continue;
            }

            let synthetic_name = canonical_physical_device_name(port_ref.kind, port_ref.id);
            let synthetic_type = plc_port_device_type(port_ref.kind);
            port_lookup.insert(
                (plc.name.clone(), port.port.id.clone()),
                ResolvedPlcEndpoint {
                    name: synthetic_name.clone(),
                },
            );
            if !used_controller_ports.contains(&synthetic_name) {
                continue;
            }
            if let Some(existing_type) = existing_names.get(&synthetic_name) {
                if *existing_type != synthetic_type {
                    errors.push(PlcError::type_mismatch_with_reason(
                        plc.line.max(1),
                        device_type_name(&synthetic_type),
                        device_type_name(existing_type),
                        format!("PLC 端口 {}.{}", plc.name, port.port.id),
                        format!(
                            "端口 {} 映射到内部节点 {}，但该节点已被声明为不同类型",
                            port.port.id, synthetic_name
                        ),
                    ));
                    continue;
                }
            } else if let Some(existing_decl) = synthetic_declarations.get(&synthetic_name) {
                if existing_decl.device_type != synthetic_type {
                    errors.push(PlcError::type_mismatch_with_reason(
                        plc.line.max(1),
                        device_type_name(&synthetic_type),
                        device_type_name(&existing_decl.device_type),
                        format!("PLC 端口 {}.{}", plc.name, port.port.id),
                        "多个 plc 端口映射到同一内部节点但类型冲突",
                    ));
                    continue;
                }
            } else {
                let mut attributes = DeviceAttributes::default();
                if matches!(
                    synthetic_type,
                    DeviceType::AnalogInput | DeviceType::AnalogOutput
                ) {
                    attributes.range = port
                        .analog_range
                        .clone()
                        .or(Some(crate::ast::AnalogRange { min: 0.0, max: 1.0 }));
                    attributes.unit = port.unit.clone().or(Some("raw".to_string()));
                }
                attributes.external = port.external.then_some(true);
                synthetic_declarations.insert(
                    synthetic_name.clone(),
                    DeviceDeclaration {
                        line: plc.line,
                        name: synthetic_name.clone(),
                        device_type: synthetic_type.clone(),
                        attributes,
                    },
                );
                existing_names.insert(synthetic_name.clone(), synthetic_type.clone());
            }
        }
    }

    let mut rewritten_connections = Vec::<TopologyConnection>::new();
    for connection in &topology.connections {
        let Some(rewritten) =
            rewrite_plc_connection(connection, &port_lookup, topology, &mut errors)
        else {
            continue;
        };
        rewritten_connections.push(rewritten);
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    let mut synthetic = synthetic_declarations.into_values().collect::<Vec<_>>();
    synthetic.sort_by(|a, b| a.name.cmp(&b.name));
    rewritten_devices.extend(synthetic);

    Ok(TopologySection {
        devices: rewritten_devices,
        workpiece_types: topology.workpiece_types.clone(),
        workpiece_sites: topology.workpiece_sites.clone(),
        workpiece_holders: topology.workpiece_holders.clone(),
        workpiece_carriers: topology.workpiece_carriers.clone(),
        semantic_resources: topology.semantic_resources.clone(),
        connections: rewritten_connections,
        variables: topology.variables.clone(),
        cam_tables: topology.cam_tables.clone(),
        extern_functions: topology.extern_functions.clone(),
        axis_fault_contracts: topology.axis_fault_contracts.clone(),
    })
}

fn resolve_plc_ports(
    plc: &DeviceDeclaration,
) -> Result<Vec<ResolvedControllerPort>, Vec<PlcError>> {
    if !plc.attributes.ports.is_empty() {
        return Err(vec![PlcError::semantic_with_reason(
            plc.line.max(1),
            format!(
                "controller device {} may not declare inline ports",
                plc.name
            ),
            format!(
                "move the controller IO inventory into {CONTROLLER_PROFILES_DIR}/<profile>.toml and reference it with model_ref"
            ),
        )]);
    }

    let Some(profile_id) = plc.attributes.model_ref.as_deref() else {
        return Err(vec![PlcError::semantic_with_reason(
            plc.line.max(1),
            format!("controller device {} is missing model_ref", plc.name),
            "declare a controller profile, for example `model_ref: openplc_softplc`".to_string(),
        )]);
    };

    let profile = load_controller_profile(profile_id).map_err(|err| vec![err])?;
    controller_profile_to_ports(plc, profile_id, &profile)
}

fn load_controller_profile(profile_id: &str) -> Result<DeviceDef, PlcError> {
    let path = Path::new(CONTROLLER_PROFILES_DIR).join(format!("{profile_id}.toml"));
    let content = fs::read_to_string(&path).map_err(|err| {
        PlcError::semantic_with_reason(
            1,
            format!("failed to load controller profile `{profile_id}`"),
            format!(
                "define the profile at {CONTROLLER_PROFILES_DIR}/{}.toml or fix model_ref ({})",
                profile_id, err
            ),
        )
    })?;

    let profile: DeviceDef = toml::from_str(&content).map_err(|err| {
        PlcError::semantic_with_reason(
            1,
            format!("failed to parse controller profile `{profile_id}`"),
            format!("fix the TOML structure in {} ({})", path.display(), err),
        )
    })?;

    if profile.identity.device_type.trim() != "plc" {
        return Err(PlcError::semantic_with_reason(
            1,
            format!(
                "controller profile `{profile_id}` must use type `plc`, got `{}`",
                profile.identity.device_type
            ),
            "set `[identity].type = \"plc\"` in the controller profile".to_string(),
        ));
    }

    Ok(profile)
}

fn controller_profile_to_ports(
    plc: &DeviceDeclaration,
    profile_id: &str,
    profile: &DeviceDef,
) -> Result<Vec<ResolvedControllerPort>, Vec<PlcError>> {
    if profile.interfaces.ports.is_empty() {
        return Err(vec![PlcError::semantic_with_reason(
            plc.line.max(1),
            format!("controller profile `{profile_id}` declares no ports"),
            "declare controller IO ports in `[[interfaces.ports]]`".to_string(),
        )]);
    }

    let mut errors = Vec::new();
    let mut ports = Vec::new();
    for port in &profile.interfaces.ports {
        match controller_profile_port_to_ast(plc, profile_id, port) {
            Ok(ast_port) => ports.push(ast_port),
            Err(err) => errors.push(err),
        }
    }

    if errors.is_empty() {
        Ok(ports)
    } else {
        Err(errors)
    }
}

fn controller_profile_port_to_ast(
    plc: &DeviceDeclaration,
    profile_id: &str,
    port: &PortDef,
) -> Result<ResolvedControllerPort, PlcError> {
    let Some(port_ref) = parse_plc_port_ref(&port.name) else {
        return Err(PlcError::semantic_with_reason(
            plc.line.max(1),
            format!(
                "controller profile `{profile_id}` contains invalid controller port `{}`",
                port.name
            ),
            "use controller port ids like X0, Y0, AI0, AO0, DI0, or DO0".to_string(),
        ));
    };

    let port_type = match port.port_type.trim() {
        "digital" => PortType::Digital,
        "analog" => PortType::Analog,
        "pneumatic" => PortType::Pneumatic,
        "logical" => PortType::Logical,
        "generic" => PortType::Generic,
        other => {
            return Err(PlcError::semantic_with_reason(
                plc.line.max(1),
                format!(
                    "controller profile `{profile_id}` uses unsupported port_type `{other}` on `{}`",
                    port.name
                ),
                "use one of: digital, analog, logical, generic".to_string(),
            ));
        }
    };

    let role = match port.direction.trim() {
        "input" => PortRole::Consumer,
        "output" => PortRole::Producer,
        "bidirectional" => PortRole::Bidirectional,
        other => {
            return Err(PlcError::semantic_with_reason(
                plc.line.max(1),
                format!(
                    "controller profile `{profile_id}` uses unsupported direction `{other}` on `{}`",
                    port.name
                ),
                "use one of: input, output, bidirectional".to_string(),
            ));
        }
    };

    let expected_type = expected_plc_port_type(port_ref.kind);
    if port_type != expected_type {
        return Err(PlcError::semantic_with_reason(
            plc.line.max(1),
            format!(
                "controller profile `{profile_id}` assigns the wrong type to `{}`",
                port.name
            ),
            format!(
                "use port type `{}` for controller port `{}`",
                port_type_name(&expected_type),
                port.name
            ),
        ));
    }

    let expected_role = expected_plc_port_role(port_ref.kind);
    if role != expected_role && role != PortRole::Bidirectional {
        return Err(PlcError::semantic_with_reason(
            plc.line.max(1),
            format!(
                "controller profile `{profile_id}` assigns the wrong direction to `{}`",
                port.name
            ),
            format!(
                "use direction `{}` for controller port `{}`",
                port_role_name(&expected_role),
                port.name
            ),
        ));
    }

    Ok(ResolvedControllerPort {
        port: crate::ast::DevicePort {
            id: port.name.clone(),
            port_type,
            role,
            states: port.states.clone(),
            default_state: port.default_state.clone(),
        },
        analog_range: match (port.range_min, port.range_max) {
            (Some(min), Some(max)) => Some(crate::ast::AnalogRange { min, max }),
            _ => None,
        },
        unit: port.unit.clone(),
        external: port.external,
    })
}
fn rewrite_plc_connection(
    connection: &TopologyConnection,
    port_lookup: &HashMap<(String, String), ResolvedPlcEndpoint>,
    topology: &TopologySection,
    errors: &mut Vec<PlcError>,
) -> Option<TopologyConnection> {
    let mut rewritten = connection.clone();
    let line = topology_connection_line(topology, connection);

    if let Some(endpoint) = resolve_plc_side(connection, true, port_lookup, line, errors)? {
        rewritten.from = endpoint.name;
        rewritten.from_port = None;
    }
    if let Some(endpoint) = resolve_plc_side(connection, false, port_lookup, line, errors)? {
        rewritten.to = endpoint.name;
        rewritten.to_port = None;
    }

    Some(rewritten)
}

fn resolve_plc_side(
    connection: &TopologyConnection,
    source_side: bool,
    port_lookup: &HashMap<(String, String), ResolvedPlcEndpoint>,
    line: usize,
    errors: &mut Vec<PlcError>,
) -> Option<Option<ResolvedPlcEndpoint>> {
    let device_name = if source_side {
        &connection.from
    } else {
        &connection.to
    };

    let Some(requested_port) = (if source_side {
        connection.from_port.as_ref()
    } else {
        connection.to_port.as_ref()
    }) else {
        if port_lookup
            .keys()
            .any(|(plc_name, _)| plc_name == device_name)
        {
            errors.push(PlcError::semantic_with_reason(
                line.max(1),
                format!(
                    "连接 {} -> {} 使用了 PLC 设备 {} 但未指定端口",
                    connection.from, connection.to, device_name
                ),
                "请使用 Device.Port 形式，例如 plc_main.Y0 或 plc_main.X0",
            ));
            return None;
        }
        return Some(None);
    };

    if let Some(endpoint) = port_lookup.get(&(device_name.clone(), requested_port.clone())) {
        return Some(Some(endpoint.clone()));
    }

    if port_lookup
        .keys()
        .any(|(plc_name, _)| plc_name == device_name)
    {
        errors.push(PlcError::semantic_with_reason(
            line.max(1),
            format!(
                "PLC 设备 {} 上未找到端口 {}（连接 {} -> {}）",
                device_name, requested_port, connection.from, connection.to
            ),
            "请检查 relation 引用的端口名，确保与 plc 设备 ports 声明一致",
        ));
        return None;
    }

    Some(None)
}

fn expected_plc_port_type(kind: PlcPortKind) -> PortType {
    match kind {
        PlcPortKind::DigitalInput | PlcPortKind::DigitalOutput => PortType::Digital,
        PlcPortKind::AnalogInput | PlcPortKind::AnalogOutput => PortType::Analog,
    }
}

fn expected_plc_port_role(kind: PlcPortKind) -> PortRole {
    match kind {
        PlcPortKind::DigitalInput | PlcPortKind::AnalogInput => PortRole::Consumer,
        PlcPortKind::DigitalOutput | PlcPortKind::AnalogOutput => PortRole::Producer,
    }
}

fn plc_port_device_type(kind: PlcPortKind) -> DeviceType {
    match kind {
        PlcPortKind::DigitalInput => DeviceType::DigitalInput,
        PlcPortKind::DigitalOutput => DeviceType::DigitalOutput,
        PlcPortKind::AnalogInput => DeviceType::AnalogInput,
        PlcPortKind::AnalogOutput => DeviceType::AnalogOutput,
    }
}

fn device_type_name(device_type: &DeviceType) -> &'static str {
    match device_type {
        DeviceType::DigitalOutput => "digital_output",
        DeviceType::DigitalInput => "digital_input",
        DeviceType::Plc => "plc",
        DeviceType::SolenoidValve => "solenoid_valve",
        DeviceType::Cylinder => "cylinder",
        DeviceType::Sensor => "sensor",
        DeviceType::Motor => "motor",
        DeviceType::StepperMotor => "stepper_motor",
        DeviceType::Vfd => "vfd",
        DeviceType::ServoDrive => "servo_drive",
        DeviceType::CamCoupling => "cam_coupling",
        DeviceType::AnalogInput => "analog_input",
        DeviceType::AnalogOutput => "analog_output",
        DeviceType::Pid => "pid",
    }
}

fn port_type_name(port_type: &PortType) -> &'static str {
    match port_type {
        PortType::Digital => "digital",
        PortType::Analog => "analog",
        PortType::Pneumatic => "pneumatic",
        PortType::Logical => "logical",
        PortType::Generic => "generic",
    }
}

fn port_role_name(role: &PortRole) -> &'static str {
    match role {
        PortRole::Producer => "producer",
        PortRole::Consumer => "consumer",
        PortRole::Bidirectional => "bidirectional",
    }
}

fn expand_repeat_blocks(tasks: &TasksSection) -> Result<TasksSection, Vec<PlcError>> {
    let mut rewritten_tasks = Vec::new();
    let mut errors = Vec::new();

    for task in &tasks.tasks {
        let mut expanded_steps = Vec::new();

        for step in &task.steps {
            let top_level_repeat_indices = step
                .statements
                .iter()
                .enumerate()
                .filter_map(|(idx, statement)| match statement {
                    StepStatement::Repeat { .. } => Some(idx),
                    _ => None,
                })
                .collect::<Vec<_>>();

            // Reject repeat blocks that appear in nested statement contexts (e.g., parallel/race).
            for statement in &step.statements {
                if contains_nested_repeat(statement) {
                    errors.push(PlcError::semantic(
                        step.line.max(1),
                        format!(
                            "step {}.{} 的 repeat 只能写在 step 顶层，不能嵌套在 parallel/race 等块内",
                            task.name, step.name
                        ),
                    ));
                    break;
                }
            }

            match top_level_repeat_indices.len() {
                0 => expanded_steps.push(step.clone()),
                1 => {
                    let repeat_index = top_level_repeat_indices[0];
                    let (prefix, repeat_statement, suffix) = split_repeat_step(step, repeat_index);

                    let StepStatement::Repeat { count, body } = repeat_statement else {
                        // split_repeat_step guarantees this index points at a repeat.
                        expanded_steps.push(step.clone());
                        continue;
                    };

                    if *count <= 1 {
                        errors.push(PlcError::semantic(
                            step.line.max(1),
                            format!(
                                "repeat 次数必须在 2..=100 之间，当前为 {count}（step {}.{}）",
                                task.name, step.name
                            ),
                        ));
                        continue;
                    }

                    if *count > 100 {
                        errors.push(PlcError::semantic(
                            step.line.max(1),
                            format!(
                                "repeat 次数超过上限 100，当前为 {count}（step {}.{}）",
                                task.name, step.name
                            ),
                        ));
                        continue;
                    }

                    if body.iter().any(statement_contains_repeat) {
                        errors.push(PlcError::semantic(
                            step.line.max(1),
                            format!(
                                "repeat 块内不允许嵌套 repeat（step {}.{}）",
                                task.name, step.name
                            ),
                        ));
                        continue;
                    }

                    for iteration in 1..=(*count as usize) {
                        let mut statements = Vec::new();
                        if iteration == 1 {
                            statements.extend_from_slice(prefix);
                        }
                        statements.extend(body.clone());
                        if iteration == *count as usize {
                            statements.extend_from_slice(suffix);
                        }

                        expanded_steps.push(crate::ast::StepDeclaration {
                            line: step.line,
                            name: format!("{}_{}", step.name, iteration),
                            statements,
                        });
                    }
                }
                _ => {
                    errors.push(PlcError::semantic(
                        step.line.max(1),
                        format!(
                            "step {}.{} 同时包含多个 repeat 块，当前版本只支持一个 repeat",
                            task.name, step.name
                        ),
                    ));
                }
            }
        }

        // Ensure step names remain unique inside the task after expansion.
        let mut seen = HashSet::<String>::new();
        for step in &expanded_steps {
            if !seen.insert(step.name.clone()) {
                errors.push(PlcError::duplicate_definition_with_reason(
                    step.line.max(1),
                    "step",
                    &format!("{}.{}", task.name, step.name),
                    "repeat 展开后产生了重复 step 名称，请重命名原始 step 或调整 repeat 使用方式",
                ));
            }
        }

        let mut rewritten_task = task.clone();
        rewritten_task.steps = expanded_steps;
        rewritten_tasks.push(rewritten_task);
    }

    if errors.is_empty() {
        Ok(TasksSection {
            tasks: rewritten_tasks,
        })
    } else {
        Err(errors)
    }
}

fn split_repeat_step(
    step: &crate::ast::StepDeclaration,
    repeat_index: usize,
) -> (&[StepStatement], &StepStatement, &[StepStatement]) {
    let prefix = &step.statements[..repeat_index];
    let repeat_statement = &step.statements[repeat_index];
    let suffix = &step.statements[repeat_index + 1..];
    (prefix, repeat_statement, suffix)
}

fn contains_nested_repeat(statement: &StepStatement) -> bool {
    match statement {
        // Top-level repeats are handled separately; nested repeats are rejected.
        StepStatement::Repeat { .. } => false,
        StepStatement::Parallel(block) => block
            .branches
            .iter()
            .any(|branch| branch.statements.iter().any(statement_contains_repeat)),
        StepStatement::Race(block) => block
            .branches
            .iter()
            .any(|branch| branch.statements.iter().any(statement_contains_repeat)),
        StepStatement::Action(_)
        | StepStatement::Wait(_)
        | StepStatement::IfElse { .. }
        | StepStatement::Delay { .. }
        | StepStatement::Timeout(_)
        | StepStatement::Goto(_)
        | StepStatement::AllowIndefiniteWait(_)
        | StepStatement::Effect(_) => false,
    }
}

fn statement_contains_repeat(statement: &StepStatement) -> bool {
    match statement {
        StepStatement::Repeat { .. } => true,
        StepStatement::Parallel(block) => block
            .branches
            .iter()
            .any(|branch| branch.statements.iter().any(statement_contains_repeat)),
        StepStatement::Race(block) => block
            .branches
            .iter()
            .any(|branch| branch.statements.iter().any(statement_contains_repeat)),
        StepStatement::Action(_)
        | StepStatement::Wait(_)
        | StepStatement::IfElse { .. }
        | StepStatement::Delay { .. }
        | StepStatement::Timeout(_)
        | StepStatement::Goto(_)
        | StepStatement::AllowIndefiniteWait(_)
        | StepStatement::Effect(_) => false,
    }
}
