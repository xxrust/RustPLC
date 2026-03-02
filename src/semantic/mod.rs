use crate::ast::{
    ActionStatement, BinaryOperator as AstBinaryOperator, CamTableMode, ComparisonOperator,
    ConditionExpression, ConstraintsSection, DeviceAttributes, DeviceDeclaration, DeviceType,
    DurationValue, Expression as AstExpression, ExternCallBinding as AstExternCallBinding,
    ExternFunctionDeclaration as AstExternFunctionDeclaration, GotoDirective, LiteralValue,
    OnCompleteDirective, ParallelBlock, PlcProgram, PortRole, PortType, RaceBlock,
    SafetyConstraint, SafetyOperand, SafetyRelation as AstSafetyRelation, StateReference,
    StepStatement, TaskDeclaration, TasksSection, TimeUnit, TimeoutDirective,
    TimingRelation as AstTimingRelation, TimingTarget, TopologyConnection, TopologyRelation,
    TopologySection, VariableDeclaration, VariableType as AstVariableType, WaitCondition,
    WaitStatement,
};
use crate::error::PlcError;
use crate::ir::{
    ActionKind, ActionRef, ActionTiming, BinaryValue as IrBinaryValue, CamCouplingDef,
    CamInterpolation, CamTableIr, CausalityChain, ConnectionType, ConstraintSet, Device,
    DeviceKind, ExternCallBinding as IrExternCallBinding,
    ExternFunctionContract as IrExternContract, ExternFunctionDef as IrExternFunctionDef,
    ExternFunctionParam as IrExternFunctionParam, MAX_CAM_POINTS, PidLoop as IrPidLoop, SafetyExpr,
    SafetyRelation as IrSafetyRelation, SafetyRule, SplineCoeff, State, StateExpr, StateMachine,
    TimeInterval, TimerOperation, TimerOperationKind, TimingModel,
    TimingRelation as IrTimingRelation, TimingRule, TimingScope, TopologyGraph, TopologyLink,
    Transition, TransitionAction, TransitionGuard, VariableDef, VariableType as IrVariableType,
};
use crate::plc_port::{PlcPortKind, canonical_physical_device_name, parse_plc_port_ref};
use petgraph::graph::NodeIndex;
use runtime_core::MAX_VARIABLES as RUNTIME_MAX_VARIABLES;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

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
    let expanded_topology = expand_plc_controller_devices(&program.topology)?;
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

fn expand_plc_controller_devices(
    topology: &TopologySection,
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
        if plc.attributes.ports.is_empty() {
            errors.push(PlcError::semantic_with_reason(
                plc.line.max(1),
                format!("PLC 设备 {} 必须声明 ports", plc.name),
                "请在 plc 设备上声明端口，例如 ports: [Y0:digital:producer, X0:digital:consumer]",
            ));
            continue;
        }

        let mut seen_ports = BTreeSet::<String>::new();
        for port in &plc.attributes.ports {
            if !seen_ports.insert(port.id.clone()) {
                errors.push(PlcError::duplicate_definition_with_reason(
                    plc.line.max(1),
                    "端口",
                    &format!("{}.{}", plc.name, port.id),
                    "PLC 设备的端口 id 不能重复",
                ));
                continue;
            }

            let Some(port_ref) = parse_plc_port_ref(&port.id) else {
                errors.push(PlcError::semantic_with_reason(
                    plc.line.max(1),
                    format!(
                        "PLC 设备 {} 的端口 {} 不是有效 PLC 通道（支持 X*/Y*/AI*/AO*/DI*/DO*）",
                        plc.name, port.id
                    ),
                    "请将 plc 端口命名为 X0/Y0/AI0/AO0 或 DI0/DO0 形式",
                ));
                continue;
            };

            let expected_type = expected_plc_port_type(port_ref.kind);
            if port.port_type != expected_type {
                errors.push(PlcError::type_mismatch_with_reason(
                    plc.line.max(1),
                    port_type_name(&expected_type),
                    port_type_name(&port.port_type),
                    format!("PLC 端口 {}.{}", plc.name, port.id),
                    "请修正 plc 端口类型，使其与端口编号前缀一致（X/DI=Digital, Y/DO=Digital, AI=Analog, AO=Analog）",
                ));
                continue;
            }

            let expected_role = expected_plc_port_role(port_ref.kind);
            if port.role != expected_role && port.role != PortRole::Bidirectional {
                errors.push(PlcError::type_mismatch_with_reason(
                    plc.line.max(1),
                    port_role_name(&expected_role),
                    port_role_name(&port.role),
                    format!("PLC 端口 {}.{}", plc.name, port.id),
                    "请修正 plc 端口方向：输入端口应为 consumer，输出端口应为 producer",
                ));
                continue;
            }

            let synthetic_name = canonical_physical_device_name(port_ref.kind, port_ref.id);
            let synthetic_type = plc_port_device_type(port_ref.kind);
            if let Some(existing_type) = existing_names.get(&synthetic_name) {
                if *existing_type != synthetic_type {
                    errors.push(PlcError::type_mismatch_with_reason(
                        plc.line.max(1),
                        device_type_name(&synthetic_type),
                        device_type_name(existing_type),
                        format!("PLC 端口 {}.{}", plc.name, port.id),
                        format!(
                            "端口 {} 映射到内部节点 {}，但该节点已被声明为不同类型",
                            port.id, synthetic_name
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
                        format!("PLC 端口 {}.{}", plc.name, port.id),
                        "多个 plc 端口映射到同一内部节点但类型冲突",
                    ));
                    continue;
                }
            } else {
                synthetic_declarations.insert(
                    synthetic_name.clone(),
                    DeviceDeclaration {
                        line: plc.line,
                        name: synthetic_name.clone(),
                        device_type: synthetic_type.clone(),
                        attributes: DeviceAttributes::default(),
                    },
                );
                existing_names.insert(synthetic_name.clone(), synthetic_type.clone());
            }

            port_lookup.insert(
                (plc.name.clone(), port.id.clone()),
                ResolvedPlcEndpoint {
                    name: synthetic_name,
                },
            );
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
        connections: rewritten_connections,
        variables: topology.variables.clone(),
        cam_tables: topology.cam_tables.clone(),
        extern_functions: topology.extern_functions.clone(),
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
        | StepStatement::AllowIndefiniteWait(_) => false,
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
        | StepStatement::AllowIndefiniteWait(_) => false,
    }
}

pub fn build_topology_graph(program: &PlcProgram) -> Result<TopologyGraph, Vec<PlcError>> {
    build_topology_from_ast(&program.topology)
}

pub fn build_state_machine(program: &PlcProgram) -> Result<StateMachine, Vec<PlcError>> {
    let expanded = preprocess_program(program)?;
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
    build_constraint_set_from_ast(&expanded.topology, &expanded.constraints, &expanded.tasks)
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

pub fn build_state_machine_from_ast(tasks: &TasksSection) -> Result<StateMachine, Vec<PlcError>> {
    build_state_machine_from_ast_with_context(tasks, &WaitExpressionContext::default(), None)
}

fn build_state_machine_from_ast_with_context(
    tasks: &TasksSection,
    wait_ctx: &WaitExpressionContext,
    device_kinds: Option<&HashMap<String, DeviceKind>>,
) -> Result<StateMachine, Vec<PlcError>> {
    let mut builder = StateMachineBuilder::default();
    let mut errors = Vec::new();

    if tasks.tasks.is_empty() {
        errors.push(PlcError::semantic(1, "[tasks] 段至少需要一个 task"));
        return Err(errors);
    }

    let mut task_initial_states = HashMap::<String, State>::new();

    for task in &tasks.tasks {
        if task.steps.is_empty() {
            errors.push(PlcError::semantic(
                task.line,
                format!("task {} 至少需要一个 step", task.name),
            ));
            continue;
        }

        let initial_state = State {
            task_name: task.name.clone(),
            step_name: task.steps[0].name.clone(),
        };

        if task_initial_states
            .insert(task.name.clone(), initial_state)
            .is_some()
        {
            errors.push(PlcError::duplicate_definition_with_reason(
                task.line,
                "task",
                &task.name,
                "请确保每个 task 名称唯一",
            ));
        }

        for step in &task.steps {
            builder.add_state(&task.name, &step.name);
        }
    }

    let Some(initial) = tasks.tasks.iter().find_map(|task| {
        task.steps.first().map(|step| State {
            task_name: task.name.clone(),
            step_name: step.name.clone(),
        })
    }) else {
        errors.push(PlcError::semantic(1, "未找到可执行的 task/step 初始状态"));
        return Err(errors);
    };

    let task_defined_steps = collect_task_steps(tasks);

    let mut task_on_complete_targets = HashMap::<String, Option<State>>::new();
    for task in &tasks.tasks {
        let on_complete_target = match &task.on_complete {
            Some(OnCompleteDirective::Goto { target }) => resolve_task_target(
                target,
                &task_initial_states,
                &task_defined_steps,
                &mut errors,
                "on_complete",
            ),
            _ => None,
        };
        task_on_complete_targets.insert(task.name.clone(), on_complete_target);
    }

    for task in &tasks.tasks {
        for (step_index, step) in task.steps.iter().enumerate() {
            validate_set_enum_values(&step.statements, step.line.max(1), &mut errors);
            if let Some(device_kinds) = device_kinds {
                validate_motor_legacy_set_actions(
                    &step.statements,
                    step.line.max(1),
                    device_kinds,
                    &mut errors,
                );
            }
            let from_state = State {
                task_name: task.name.clone(),
                step_name: step.name.clone(),
            };
            let completion_target =
                completion_target_for_step(task, step_index, &task_on_complete_targets);

            let analyzed = analyze_statements(&step.statements, wait_ctx);

            for (block_index, block) in analyzed.parallel_blocks.iter().enumerate() {
                build_parallel_block(
                    &mut builder,
                    task,
                    &step.name,
                    &from_state,
                    block_index,
                    block,
                    completion_target.clone(),
                    &task_initial_states,
                    &task_defined_steps,
                    &mut errors,
                    analyzed.actions.clone(),
                    wait_ctx,
                );
            }

            for (block_index, block) in analyzed.race_blocks.iter().enumerate() {
                build_race_block(
                    &mut builder,
                    task,
                    &step.name,
                    &from_state,
                    block_index,
                    block,
                    completion_target.clone(),
                    &task_initial_states,
                    &task_defined_steps,
                    &mut errors,
                    analyzed.actions.clone(),
                    wait_ctx,
                );
            }

            for goto in &analyzed.gotos {
                if let Some(target) = resolve_task_target(
                    goto,
                    &task_initial_states,
                    &task_defined_steps,
                    &mut errors,
                    "goto",
                ) {
                    builder.add_transition(
                        from_state.clone(),
                        target,
                        TransitionGuard::Always,
                        analyzed.actions.clone(),
                        Vec::new(),
                    );
                }
            }

            for if_else in &analyzed.if_elses {
                let expr = condition_to_expression(&if_else.condition);

                if let Some(then_target) = resolve_task_target(
                    &if_else.then_goto,
                    &task_initial_states,
                    &task_defined_steps,
                    &mut errors,
                    "if/else then goto",
                ) {
                    builder.add_transition(
                        from_state.clone(),
                        then_target,
                        TransitionGuard::Condition {
                            expression: expr.clone(),
                        },
                        analyzed.actions.clone(),
                        Vec::new(),
                    );
                }

                if let Some(else_target) = resolve_task_target(
                    &if_else.else_goto,
                    &task_initial_states,
                    &task_defined_steps,
                    &mut errors,
                    "if/else else goto",
                ) {
                    builder.add_transition(
                        from_state.clone(),
                        else_target,
                        TransitionGuard::Condition {
                            expression: format!("NOT({expr})"),
                        },
                        analyzed.actions.clone(),
                        Vec::new(),
                    );
                }
            }

            for (delay_index, duration_ms) in analyzed.delays_ms.iter().enumerate() {
                if let Some(target) = completion_target.clone() {
                    builder.add_transition(
                        from_state.clone(),
                        target,
                        TransitionGuard::Delay {
                            duration_ms: *duration_ms,
                        },
                        Vec::new(),
                        vec![TimerOperation {
                            timer_name: format!(
                                "{}.{}.delay_{}",
                                task.name,
                                step.name,
                                delay_index + 1
                            ),
                            operation: TimerOperationKind::Start,
                            duration_ms: Some(*duration_ms),
                        }],
                    );
                }
            }

            for (timeout_index, timeout) in analyzed.timeouts.iter().enumerate() {
                if let Some(target) = resolve_task_target(
                    &timeout.target,
                    &task_initial_states,
                    &task_defined_steps,
                    &mut errors,
                    "timeout -> goto",
                ) {
                    let duration_ms = duration_to_ms(timeout);
                    builder.add_transition(
                        from_state.clone(),
                        target,
                        TransitionGuard::Timeout { duration_ms },
                        Vec::new(),
                        vec![TimerOperation {
                            timer_name: format!(
                                "{}.{}.timeout_{}",
                                task.name,
                                step.name,
                                timeout_index + 1
                            ),
                            operation: TimerOperationKind::Start,
                            duration_ms: Some(duration_ms),
                        }],
                    );
                }
            }

            for wait_expression in &analyzed.waits {
                if let Some(target) = completion_target.clone() {
                    builder.add_transition(
                        from_state.clone(),
                        target,
                        TransitionGuard::Condition {
                            expression: wait_expression.clone(),
                        },
                        analyzed.actions.clone(),
                        Vec::new(),
                    );
                }
            }

            let has_control_flow = !analyzed.waits.is_empty()
                || !analyzed.delays_ms.is_empty()
                || !analyzed.gotos.is_empty()
                || !analyzed.if_elses.is_empty()
                || !analyzed.parallel_blocks.is_empty()
                || !analyzed.race_blocks.is_empty();
            if !has_control_flow {
                if let Some(target) = completion_target {
                    builder.add_transition(
                        from_state,
                        target,
                        TransitionGuard::Always,
                        analyzed.actions,
                        Vec::new(),
                    );
                }
            }
        }
    }

    if errors.is_empty() {
        let analog_regions = wait_ctx
            .analog_input_regions
            .iter()
            .map(|(device, regions)| {
                (
                    device.clone(),
                    regions
                        .iter()
                        .map(|(min, max)| {
                            (format_numeric_literal(*min), format_numeric_literal(*max))
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        Ok(StateMachine {
            states: builder.states,
            transitions: builder.transitions,
            initial,
            analog_regions,
        })
    } else {
        Err(errors)
    }
}

fn format_numeric_literal(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.1}")
    } else {
        v.to_string()
    }
}

fn format_numeric_literal_from_literal(literal: &LiteralValue) -> Option<String> {
    match literal {
        LiteralValue::Number(v) => Some(format_numeric_literal(*v)),
        LiteralValue::Measured(measured) => Some(format_numeric_literal(measured.value)),
        LiteralValue::Boolean(_) | LiteralValue::String(_) | LiteralValue::State(_) => None,
    }
}

#[derive(Debug, Clone, Default)]
struct StateMachineBuilder {
    states: Vec<State>,
    transitions: Vec<Transition>,
    seen_states: HashSet<(String, String)>,
}

impl StateMachineBuilder {
    fn add_state(&mut self, task_name: &str, step_name: &str) -> State {
        let key = (task_name.to_string(), step_name.to_string());
        if self.seen_states.insert(key.clone()) {
            self.states.push(State {
                task_name: key.0.clone(),
                step_name: key.1.clone(),
            });
        }

        State {
            task_name: key.0,
            step_name: key.1,
        }
    }

    fn add_transition(
        &mut self,
        from: State,
        to: State,
        guard: TransitionGuard,
        actions: Vec<TransitionAction>,
        timers: Vec<TimerOperation>,
    ) {
        self.transitions.push(Transition {
            from,
            to,
            guard,
            actions,
            timers,
        });
    }
}

#[derive(Debug, Clone, Default)]
struct AnalyzedStatements {
    actions: Vec<TransitionAction>,
    waits: Vec<String>,
    delays_ms: Vec<u64>,
    gotos: Vec<GotoDirective>,
    timeouts: Vec<TimeoutDirective>,
    if_elses: Vec<IfElseSpec>,
    parallel_blocks: Vec<ParallelBlock>,
    race_blocks: Vec<RaceBlock>,
}

#[derive(Debug, Clone)]
struct IfElseSpec {
    condition: ConditionExpression,
    then_goto: GotoDirective,
    else_goto: GotoDirective,
}

#[derive(Debug, Clone, Default)]
struct DeviceTimingProfile {
    response_ms: Option<u64>,
    stroke_ms: Option<u64>,
    retract_ms: Option<u64>,
    ramp_ms: Option<u64>,
}

fn collect_device_kinds(topology: &TopologySection) -> HashMap<String, DeviceKind> {
    topology
        .devices
        .iter()
        .map(|device| {
            (
                device.name.clone(),
                ast_type_to_ir_kind(&device.device_type),
            )
        })
        .collect()
}

fn collect_known_states(
    topology: &TopologySection,
    device_kinds: &HashMap<String, DeviceKind>,
) -> HashMap<String, HashSet<String>> {
    let mut known_states = HashMap::new();

    for device in &topology.devices {
        let Some(kind) = device_kinds.get(&device.name) else {
            continue;
        };

        let mut states = HashSet::new();
        for port in &device.attributes.ports {
            for state in &port.states {
                states.insert(state.clone());
            }
        }

        if let Some(custom_states) = &device.attributes.custom_states {
            if custom_states.len() > 8 {
                eprintln!(
                    "WARNING [semantic] 设备 {} 声明了 {} 个 states（> 8），请确认状态空间规模合理",
                    device.name,
                    custom_states.len()
                );
            }

            for state in custom_states {
                states.insert(state.clone());
            }
        } else {
            for state in default_states_for_kind(kind) {
                states.insert(state.to_string());
            }
        }

        known_states.insert(device.name.clone(), states);
    }

    for device in &topology.devices {
        if let Some(detects) = &device.attributes.detects {
            known_states
                .entry(detects.device.clone())
                .or_default()
                .insert(detects.state.clone());
        }
    }

    known_states
}

fn collect_task_steps(tasks: &TasksSection) -> HashMap<String, HashSet<String>> {
    let mut task_steps = HashMap::new();

    for task in &tasks.tasks {
        let steps = task
            .steps
            .iter()
            .map(|step| step.name.clone())
            .collect::<HashSet<_>>();
        task_steps.insert(task.name.clone(), steps);
    }

    task_steps
}

fn validate_state_reference(
    state: &crate::ast::StateReference,
    line: usize,
    source: &str,
    device_kinds: &HashMap<String, DeviceKind>,
    known_states: &HashMap<String, HashSet<String>>,
    errors: &mut Vec<PlcError>,
) {
    let Some(kind) = device_kinds.get(&state.device) else {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            "设备",
            &state.device,
            format!("{source} 使用前需要先在 [topology] 段定义设备"),
        ));
        return;
    };

    if *kind == DeviceKind::Motor && state.port == "self" {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "{source} 使用了已废弃的电机状态写法 {}.{}",
                state.device, state.state
            ),
            format!(
                "请改用显式端口状态，例如 {}.run.on/off 或 {}.direction.forward/reverse",
                state.device, state.device
            ),
        ));
        return;
    }

    if state.state.is_empty() {
        errors.push(PlcError::semantic(
            line,
            format!("{source} 设备 {} 缺少状态名", state.device),
        ));
        return;
    }

    let Some(allowed_states) = known_states.get(&state.device) else {
        return;
    };

    if !allowed_states.is_empty() && !allowed_states.contains(&state.state) {
        errors.push(PlcError::semantic(
            line,
            format!(
                "{source} 引用了设备 {} 的未定义状态 {}",
                state.device, state.state
            ),
        ));
    }
}

fn validate_device_reference(
    device_name: &str,
    line: usize,
    source: &str,
    device_kinds: &HashMap<String, DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    if !device_kinds.contains_key(device_name) {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            "设备",
            device_name,
            format!("{source} 约束引用前需要定义该设备"),
        ));
    }
}

fn validate_causality_node_reference(
    node_name: &str,
    line: usize,
    device_kinds: &HashMap<String, DeviceKind>,
    variable_names: &HashSet<String>,
    extern_function_names: &HashSet<String>,
    errors: &mut Vec<PlcError>,
) {
    if device_kinds.contains_key(node_name)
        || variable_names.contains(node_name)
        || extern_function_names.contains(node_name)
    {
        return;
    }

    errors.push(PlcError::undefined_reference_with_reason(
        line,
        "因果节点",
        node_name,
        "causality 链路节点需要先定义为设备、[topology] variable 或 extern function".to_string(),
    ));
}

fn validate_timing_target(
    target: &TimingTarget,
    line: usize,
    task_steps: &HashMap<String, HashSet<String>>,
    errors: &mut Vec<PlcError>,
) {
    match target {
        TimingTarget::Task { task } => {
            if !task_steps.contains_key(task) {
                errors.push(PlcError::undefined_reference_with_reason(
                    line,
                    " task",
                    task,
                    "请先在 [tasks] 段定义该 task".to_string(),
                ));
            }
        }
        TimingTarget::Step { task, step } => {
            let Some(steps) = task_steps.get(task) else {
                errors.push(PlcError::undefined_reference_with_reason(
                    line,
                    " task",
                    task,
                    "请先在 [tasks] 段定义该 task".to_string(),
                ));
                return;
            };

            if !steps.contains(step) {
                errors.push(PlcError::semantic(
                    line,
                    format!("timing 约束引用了未定义 step {task}.{step}"),
                ));
            }
        }
    }
}

fn collect_device_ranges(topology: &TopologySection) -> HashMap<String, (f64, f64)> {
    topology
        .devices
        .iter()
        .filter_map(|device| {
            device.attributes.range.as_ref().map(|r| {
                let (min, max) = if r.min <= r.max {
                    (r.min, r.max)
                } else {
                    (r.max, r.min)
                };
                (device.name.clone(), (min, max))
            })
        })
        .collect()
}

fn collect_device_port_types(
    topology: &TopologySection,
    device_kinds: &HashMap<String, DeviceKind>,
) -> HashMap<String, PortType> {
    let mut out = HashMap::new();

    for device in &topology.devices {
        for port in &device.attributes.ports {
            out.insert(
                format!("{}.{}", device.name, port.id),
                port.port_type.clone(),
            );
        }

        if let Some(kind) = device_kinds.get(&device.name) {
            for port in default_analog_ports_for_kind(kind) {
                out.entry(format!("{}.{}", device.name, port))
                    .or_insert(PortType::Analog);
            }
        }
    }

    out
}

fn collect_device_units(topology: &TopologySection) -> HashMap<String, String> {
    topology
        .devices
        .iter()
        .filter_map(|device| {
            device
                .attributes
                .unit
                .as_ref()
                .map(|unit| (device.name.clone(), unit.clone()))
        })
        .collect()
}

fn default_analog_ports_for_kind(kind: &DeviceKind) -> &'static [&'static str] {
    match kind {
        DeviceKind::CamCoupling => &["following_error", "master_pos", "slave_cmd"],
        DeviceKind::AnalogInput => &["in"],
        DeviceKind::AnalogOutput => &["out"],
        DeviceKind::Pid => &["in", "out"],
        _ => &[],
    }
}

fn validate_analog_actions_in_statements(
    statements: &[StepStatement],
    line: usize,
    device_kinds: &HashMap<String, DeviceKind>,
    device_ranges: &HashMap<String, (f64, f64)>,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::SetAnalog { target, value }) => {
                if let Some(kind) = device_kinds.get(&target.device) {
                    if *kind != DeviceKind::AnalogOutput
                        && *kind != DeviceKind::Motor
                        && *kind != DeviceKind::Vfd
                    {
                        errors.push(PlcError::type_mismatch_with_reason(
                            line,
                            "analog_output / motor / vfd",
                            device_kind_name(kind),
                            format!("set_analog {target}"),
                            "set_analog 只能用于 analog_output、motor 或 vfd 类型设备",
                        ));
                    }
                }
                if let Some((min, max)) = device_ranges.get(&target.device) {
                    if *value < *min || *value > *max {
                        errors.push(PlcError::semantic_with_reason(
                            line,
                            format!("set_analog {target} {value} 超出声明范围 {min}..{max}",),
                            "请确保 set_analog 值在设备声明的 range 范围内",
                        ));
                    }
                }
            }
            StepStatement::Action(ActionStatement::SetAnalogExpr { target, .. }) => {
                if let Some(kind) = device_kinds.get(&target.device) {
                    if *kind != DeviceKind::AnalogOutput
                        && *kind != DeviceKind::Motor
                        && *kind != DeviceKind::Vfd
                    {
                        errors.push(PlcError::type_mismatch_with_reason(
                            line,
                            "analog_output / motor / vfd",
                            device_kind_name(kind),
                            format!("set_analog {target}"),
                            "set_analog 只能用于 analog_output、motor 或 vfd 类型设备",
                        ));
                    }
                }
            }
            StepStatement::Action(ActionStatement::Set { target, .. }) => {
                if let Some(kind) = device_kinds.get(&target.device) {
                    if *kind == DeviceKind::AnalogOutput || *kind == DeviceKind::AnalogInput {
                        errors.push(PlcError::type_mismatch_with_reason(
                            line,
                            "digital_output 或 solenoid_valve 等离散设备",
                            device_kind_name(kind),
                            format!("set {target} on/off"),
                            "模拟量设备请使用 set_analog 指令",
                        ));
                    }
                }
            }
            StepStatement::Repeat { body, .. } => {
                validate_analog_actions_in_statements(
                    body,
                    line,
                    device_kinds,
                    device_ranges,
                    errors,
                );
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_analog_actions_in_statements(
                        &branch.statements,
                        line,
                        device_kinds,
                        device_ranges,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_analog_actions_in_statements(
                        &branch.statements,
                        line,
                        device_kinds,
                        device_ranges,
                        errors,
                    );
                }
            }
            _ => {}
        }
    }
}

fn validate_set_enum_values(statements: &[StepStatement], line: usize, errors: &mut Vec<PlcError>) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::Set { target, value }) => {
                if set_enum_to_binary(value).is_none() {
                    errors.push(PlcError::semantic_with_reason(
                        line,
                        format!("set {target} {value} 使用了不支持的状态值"),
                        "set 状态值仅支持 on/off/forward/reverse/active/idle".to_string(),
                    ));
                }
            }
            StepStatement::Repeat { body, .. } => validate_set_enum_values(body, line, errors),
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_set_enum_values(&branch.statements, line, errors);
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_set_enum_values(&branch.statements, line, errors);
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

fn validate_expression_actions_in_tasks(
    tasks: &TasksSection,
    variable_types: &HashMap<String, AstVariableType>,
    errors: &mut Vec<PlcError>,
) {
    for task in &tasks.tasks {
        for step in &task.steps {
            validate_expression_actions_in_statements(
                &step.statements,
                step.line.max(1),
                variable_types,
                errors,
            );
        }
    }
}

fn validate_extern_calls_in_tasks(
    tasks: &TasksSection,
    extern_signatures: &HashMap<String, ExternFunctionSignature>,
    variable_types: &HashMap<String, AstVariableType>,
    errors: &mut Vec<PlcError>,
) {
    for task in &tasks.tasks {
        for step in &task.steps {
            validate_extern_calls_in_statements(
                &step.statements,
                step.line.max(1),
                extern_signatures,
                variable_types,
                errors,
            );
        }
    }
}

fn validate_non_pure_extern_concurrency_in_tasks(
    tasks: &TasksSection,
    extern_signatures: &HashMap<String, ExternFunctionSignature>,
    errors: &mut Vec<PlcError>,
) {
    for task in &tasks.tasks {
        for step in &task.steps {
            validate_non_pure_extern_concurrency_in_statements(
                &step.statements,
                step.line.max(1),
                extern_signatures,
                errors,
            );
        }
    }
}

fn validate_non_pure_extern_concurrency_in_statements(
    statements: &[StepStatement],
    line: usize,
    extern_signatures: &HashMap<String, ExternFunctionSignature>,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Parallel(block) => {
                let branch_statements = block
                    .branches
                    .iter()
                    .map(|branch| branch.statements.as_slice())
                    .collect::<Vec<_>>();
                validate_non_pure_extern_concurrency_in_branches(
                    &branch_statements,
                    "parallel",
                    line,
                    extern_signatures,
                    errors,
                );

                for branch in &block.branches {
                    validate_non_pure_extern_concurrency_in_statements(
                        &branch.statements,
                        line,
                        extern_signatures,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                let branch_statements = block
                    .branches
                    .iter()
                    .map(|branch| branch.statements.as_slice())
                    .collect::<Vec<_>>();
                validate_non_pure_extern_concurrency_in_branches(
                    &branch_statements,
                    "race",
                    line,
                    extern_signatures,
                    errors,
                );

                for branch in &block.branches {
                    validate_non_pure_extern_concurrency_in_statements(
                        &branch.statements,
                        line,
                        extern_signatures,
                        errors,
                    );
                }
            }
            StepStatement::Repeat { body, .. } => {
                validate_non_pure_extern_concurrency_in_statements(
                    body,
                    line,
                    extern_signatures,
                    errors,
                );
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

fn validate_non_pure_extern_concurrency_in_branches(
    branches: &[&[StepStatement]],
    block_kind: &str,
    line: usize,
    extern_signatures: &HashMap<String, ExternFunctionSignature>,
    errors: &mut Vec<PlcError>,
) {
    let mut first_seen_by_function: HashMap<String, usize> = HashMap::new();

    for (branch_index, statements) in branches.iter().enumerate() {
        let mut calls = HashSet::new();
        collect_non_pure_extern_calls(statements, extern_signatures, &mut calls);
        for function in calls {
            if let Some(first_branch) = first_seen_by_function.get(&function).copied() {
                errors.push(PlcError::semantic_with_reason(
                    line,
                    format!(
                        "non-pure extern 函数 {function} 在 {block_kind} 分支 #{} 与 #{} 中并发调用",
                        first_branch + 1,
                        branch_index + 1
                    ),
                    "请将 pure: false 的 extern 调用改为串行执行，避免在 parallel/race 多分支中重复调用同一函数",
                ));
            } else {
                first_seen_by_function.insert(function, branch_index);
            }
        }
    }
}

fn collect_non_pure_extern_calls(
    statements: &[StepStatement],
    extern_signatures: &HashMap<String, ExternFunctionSignature>,
    out: &mut HashSet<String>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::Call { function, .. }) => {
                if extern_signatures
                    .get(function)
                    .map(|signature| !signature.pure)
                    .unwrap_or(false)
                {
                    out.insert(function.clone());
                }
            }
            StepStatement::Repeat { body, .. } => {
                collect_non_pure_extern_calls(body, extern_signatures, out);
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    collect_non_pure_extern_calls(&branch.statements, extern_signatures, out);
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    collect_non_pure_extern_calls(&branch.statements, extern_signatures, out);
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

fn validate_extern_calls_in_statements(
    statements: &[StepStatement],
    line: usize,
    extern_signatures: &HashMap<String, ExternFunctionSignature>,
    variable_types: &HashMap<String, AstVariableType>,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::Call {
                function,
                args,
                binding,
            }) => {
                validate_extern_call_signature(
                    function,
                    args,
                    binding,
                    line,
                    extern_signatures,
                    variable_types,
                    errors,
                );
            }
            StepStatement::Repeat { body, .. } => validate_extern_calls_in_statements(
                body,
                line,
                extern_signatures,
                variable_types,
                errors,
            ),
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_extern_calls_in_statements(
                        &branch.statements,
                        line,
                        extern_signatures,
                        variable_types,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_extern_calls_in_statements(
                        &branch.statements,
                        line,
                        extern_signatures,
                        variable_types,
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

fn validate_extern_call_signature(
    function: &str,
    args: &[AstExpression],
    binding: &AstExternCallBinding,
    line: usize,
    extern_signatures: &HashMap<String, ExternFunctionSignature>,
    variable_types: &HashMap<String, AstVariableType>,
    errors: &mut Vec<PlcError>,
) {
    let Some(signature) = extern_signatures.get(function) else {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            "extern 函数",
            function,
            format!("action: call {function}(...) 调用前需要先在 [topology] 中声明"),
        ));
        return;
    };

    if args.len() != signature.param_types.len() {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "extern 函数 {function} 参数个数错误：期望 {} 个，实际 {} 个",
                signature.param_types.len(),
                args.len()
            ),
            "请检查 action: call 参数列表与 extern function 声明是否一致".to_string(),
        ));
    }

    for (index, (arg, expected_type)) in args.iter().zip(&signature.param_types).enumerate() {
        let Some(actual_type) = infer_expression_type(arg, variable_types) else {
            continue;
        };
        if actual_type != *expected_type {
            errors.push(PlcError::type_mismatch_with_reason(
                line,
                ast_variable_type_name(expected_type),
                ast_variable_type_name(&actual_type),
                format!("extern 调用 {function} 参数 #{}", index + 1),
                "请将实参与 extern function 声明的参数类型保持一致",
            ));
        }
    }

    let binding_targets: &[String] = match binding {
        AstExternCallBinding::Single(name) => std::slice::from_ref(name),
        AstExternCallBinding::Tuple(names) => names.as_slice(),
    };

    let expected_return_count = signature.return_types.len();
    if binding_targets.len() != expected_return_count {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "extern 函数 {function} 返回值绑定数量错误：期望 {expected_return_count} 个，实际 {} 个",
                binding_targets.len()
            ),
            "请让 -> 绑定变量数量与 extern function 返回类型数量保持一致".to_string(),
        ));
        return;
    }

    for (index, (target, expected_type)) in binding_targets
        .iter()
        .zip(&signature.return_types)
        .enumerate()
    {
        let Some(actual_type) = variable_types.get(target) else {
            errors.push(PlcError::undefined_reference_with_reason(
                line,
                "变量",
                target,
                format!("extern 函数 {function} 返回值绑定目标必须先在 [topology] 中声明"),
            ));
            continue;
        };

        if actual_type != expected_type {
            errors.push(PlcError::type_mismatch_with_reason(
                line,
                ast_variable_type_name(expected_type),
                ast_variable_type_name(actual_type),
                format!("extern 调用 {function} 返回绑定 #{} ({target})", index + 1),
                "请将绑定变量类型与 extern function 返回类型保持一致",
            ));
        }
    }
}

fn infer_expression_type(
    expr: &AstExpression,
    variable_types: &HashMap<String, AstVariableType>,
) -> Option<AstVariableType> {
    match expr {
        AstExpression::Literal(_) => Some(AstVariableType::Float),
        AstExpression::Boolean(_) => Some(AstVariableType::Bool),
        AstExpression::Variable(name) => variable_types.get(name).cloned(),
        AstExpression::UnaryNeg(inner) => match infer_expression_type(inner, variable_types)? {
            AstVariableType::Bool => None,
            AstVariableType::Int => Some(AstVariableType::Int),
            AstVariableType::Float => Some(AstVariableType::Float),
        },
        AstExpression::UnaryNot(inner) => match infer_expression_type(inner, variable_types)? {
            AstVariableType::Bool => Some(AstVariableType::Bool),
            _ => None,
        },
        AstExpression::BinaryOp { op, left, right } => {
            let left_type = infer_expression_type(left, variable_types)?;
            let right_type = infer_expression_type(right, variable_types)?;
            match op {
                AstBinaryOperator::Add
                | AstBinaryOperator::Sub
                | AstBinaryOperator::Mul
                | AstBinaryOperator::Div
                | AstBinaryOperator::Mod => match (left_type, right_type) {
                    (AstVariableType::Bool, _) | (_, AstVariableType::Bool) => None,
                    (AstVariableType::Float, _) | (_, AstVariableType::Float) => {
                        Some(AstVariableType::Float)
                    }
                    (AstVariableType::Int, AstVariableType::Int) => Some(AstVariableType::Int),
                },
                AstBinaryOperator::Eq | AstBinaryOperator::Neq => match (left_type, right_type) {
                    (AstVariableType::Bool, AstVariableType::Bool) => Some(AstVariableType::Bool),
                    (AstVariableType::Bool, _) | (_, AstVariableType::Bool) => None,
                    _ => Some(AstVariableType::Bool),
                },
                AstBinaryOperator::Gt
                | AstBinaryOperator::Lt
                | AstBinaryOperator::Gte
                | AstBinaryOperator::Lte => match (left_type, right_type) {
                    (AstVariableType::Bool, _) | (_, AstVariableType::Bool) => None,
                    _ => Some(AstVariableType::Bool),
                },
                AstBinaryOperator::And | AstBinaryOperator::Or => {
                    if left_type == AstVariableType::Bool && right_type == AstVariableType::Bool {
                        Some(AstVariableType::Bool)
                    } else {
                        None
                    }
                }
            }
        }
        AstExpression::FunctionCall { .. } => Some(AstVariableType::Float),
    }
}

fn validate_expression_actions_in_statements(
    statements: &[StepStatement],
    line: usize,
    variable_types: &HashMap<String, AstVariableType>,
    errors: &mut Vec<PlcError>,
) {
    let variables = variable_types.keys().cloned().collect::<HashSet<_>>();
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::Compute { target, expr }) => {
                if !variables.contains(target) {
                    errors.push(PlcError::undefined_reference_with_reason(
                        line,
                        "变量",
                        target,
                        "compute 目标变量必须先在 [topology] 中使用 variable 声明".to_string(),
                    ));
                }
                validate_expression_variables(expr, line, &variables, errors);
                if let Some(target_type) = variable_types.get(target) {
                    match infer_expression_type(expr, variable_types) {
                        Some(actual_type)
                            if !expression_type_assignable_to(&actual_type, target_type) =>
                        {
                            errors.push(PlcError::type_mismatch_with_reason(
                                line,
                                ast_variable_type_name(target_type),
                                ast_variable_type_name(&actual_type),
                                format!("compute {target}"),
                                "compute 表达式类型必须与目标变量类型一致".to_string(),
                            ));
                        }
                        None => errors.push(PlcError::semantic_with_reason(
                            line,
                            format!("compute {target} 表达式类型不合法"),
                            "请检查布尔/比较/算术表达式是否符合类型规则".to_string(),
                        )),
                        _ => {}
                    }
                }
            }
            StepStatement::Action(ActionStatement::SetAnalogExpr { expr, .. }) => {
                validate_expression_variables(expr, line, &variables, errors);
                match infer_expression_type(expr, variable_types) {
                    Some(AstVariableType::Bool) => {
                        errors.push(PlcError::type_mismatch_with_reason(
                            line,
                            "float/int",
                            "bool",
                            "set_analog expression".to_string(),
                            "set_analog 表达式必须是数值类型".to_string(),
                        ))
                    }
                    None => errors.push(PlcError::semantic_with_reason(
                        line,
                        "set_analog 表达式类型不合法".to_string(),
                        "请检查布尔/比较/算术表达式是否符合类型规则".to_string(),
                    )),
                    _ => {}
                }
            }
            StepStatement::Action(ActionStatement::Call { args, .. }) => {
                for arg in args {
                    validate_expression_variables(arg, line, &variables, errors);
                }
            }
            StepStatement::Action(ActionStatement::CamPhase { offset, .. }) => {
                validate_expression_variables(offset, line, &variables, errors);
                match infer_expression_type(offset, variable_types) {
                    Some(AstVariableType::Bool) => {
                        errors.push(PlcError::type_mismatch_with_reason(
                            line,
                            "float/int",
                            "bool",
                            "cam_phase offset".to_string(),
                            "cam_phase 偏移表达式必须是数值类型".to_string(),
                        ))
                    }
                    None => errors.push(PlcError::semantic_with_reason(
                        line,
                        "cam_phase 偏移表达式类型不合法".to_string(),
                        "请检查布尔/比较/算术表达式是否符合类型规则".to_string(),
                    )),
                    _ => {}
                }
            }
            StepStatement::Wait(wait) => {
                for condition in wait_condition_terms(&wait.condition) {
                    if let Some((left, right)) = condition.expression_pair() {
                        validate_expression_variables(left, line, &variables, errors);
                        validate_expression_variables(right, line, &variables, errors);
                    }
                }
            }
            StepStatement::IfElse { condition, .. } => {
                if let Some((left, right)) = condition.expression_pair() {
                    validate_expression_variables(left, line, &variables, errors);
                    validate_expression_variables(right, line, &variables, errors);
                }
            }
            StepStatement::Repeat { body, .. } => {
                validate_expression_actions_in_statements(body, line, variable_types, errors);
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_expression_actions_in_statements(
                        &branch.statements,
                        line,
                        variable_types,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_expression_actions_in_statements(
                        &branch.statements,
                        line,
                        variable_types,
                        errors,
                    );
                }
            }
            StepStatement::Action(_)
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
}

fn validate_cam_actions_in_tasks(
    tasks: &TasksSection,
    device_kinds: &HashMap<String, DeviceKind>,
    cam_table_names: &HashSet<String>,
    errors: &mut Vec<PlcError>,
) {
    for task in &tasks.tasks {
        for step in &task.steps {
            validate_cam_actions_in_statements(
                &step.statements,
                step.line.max(1),
                device_kinds,
                cam_table_names,
                errors,
            );
        }
    }
}

fn validate_cam_actions_in_statements(
    statements: &[StepStatement],
    line: usize,
    device_kinds: &HashMap<String, DeviceKind>,
    cam_table_names: &HashSet<String>,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::CamEngage { target })
            | StepStatement::Action(ActionStatement::CamDisengage { target })
            | StepStatement::Action(ActionStatement::CamPhase { target, .. }) => {
                match device_kinds.get(target) {
                    Some(DeviceKind::CamCoupling) => {}
                    Some(kind) => errors.push(PlcError::type_mismatch_with_reason(
                        line,
                        "cam_coupling",
                        device_kind_name(kind),
                        format!("cam action {target}"),
                        "cam 动作仅支持作用于 cam_coupling 设备",
                    )),
                    None => errors.push(PlcError::undefined_reference_with_reason(
                        line,
                        "设备",
                        target,
                        "cam 动作引用前需要在 [topology] 中定义 cam_coupling 设备".to_string(),
                    )),
                }
            }
            StepStatement::Action(ActionStatement::CamSwitch { target, new_table }) => {
                match device_kinds.get(target) {
                    Some(DeviceKind::CamCoupling) => {}
                    Some(kind) => errors.push(PlcError::type_mismatch_with_reason(
                        line,
                        "cam_coupling",
                        device_kind_name(kind),
                        format!("cam_switch {target}"),
                        "cam_switch 仅支持作用于 cam_coupling 设备",
                    )),
                    None => errors.push(PlcError::undefined_reference_with_reason(
                        line,
                        "设备",
                        target,
                        "cam_switch 引用前需要定义 cam_coupling 设备".to_string(),
                    )),
                }
                if !cam_table_names.contains(new_table) {
                    errors.push(PlcError::undefined_reference_with_reason(
                        line,
                        "cam_table",
                        new_table,
                        "cam_switch 的目标表需要先在 [topology] 中声明".to_string(),
                    ));
                }
            }
            StepStatement::Repeat { body, .. } => validate_cam_actions_in_statements(
                body,
                line,
                device_kinds,
                cam_table_names,
                errors,
            ),
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_cam_actions_in_statements(
                        &branch.statements,
                        line,
                        device_kinds,
                        cam_table_names,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_cam_actions_in_statements(
                        &branch.statements,
                        line,
                        device_kinds,
                        cam_table_names,
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

fn validate_expression_variables(
    expr: &AstExpression,
    line: usize,
    variables: &HashSet<String>,
    errors: &mut Vec<PlcError>,
) {
    match expr {
        AstExpression::Literal(_) => {}
        AstExpression::Boolean(_) => {}
        AstExpression::Variable(name) => {
            if !variables.contains(name) {
                errors.push(PlcError::undefined_reference_with_reason(
                    line,
                    "变量",
                    name,
                    "表达式变量必须先在 [topology] 中使用 variable 声明".to_string(),
                ));
            }
        }
        AstExpression::UnaryNeg(inner) => {
            validate_expression_variables(inner, line, variables, errors)
        }
        AstExpression::UnaryNot(inner) => {
            validate_expression_variables(inner, line, variables, errors)
        }
        AstExpression::BinaryOp { left, right, .. } => {
            validate_expression_variables(left, line, variables, errors);
            validate_expression_variables(right, line, variables, errors);
        }
        AstExpression::FunctionCall { args, .. } => {
            for arg in args {
                validate_expression_variables(arg, line, variables, errors);
            }
            validate_builtin_function_call(expr, line, errors);
        }
    }
}

fn validate_builtin_function_call(expr: &AstExpression, line: usize, errors: &mut Vec<PlcError>) {
    let AstExpression::FunctionCall { name, args } = expr else {
        return;
    };

    let expected_arity = match name.as_str() {
        "abs" | "sin" | "cos" | "sqrt" => 1,
        "min" | "max" | "pow" | "fmod" => 2,
        "clamp" => 3,
        _ => {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!("不支持的内置函数: {name}"),
                "支持函数: abs/min/max/sin/cos/sqrt/pow/fmod/clamp".to_string(),
            ));
            return;
        }
    };

    if args.len() != expected_arity {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "函数 {name} 参数个数错误：期望 {expected_arity} 个，实际 {} 个",
                args.len()
            ),
            "请检查函数调用参数数量".to_string(),
        ));
    }
}

fn expression_type_assignable_to(
    expression_type: &AstVariableType,
    target_type: &AstVariableType,
) -> bool {
    matches!(
        (expression_type, target_type),
        (AstVariableType::Bool, AstVariableType::Bool)
            | (AstVariableType::Int, AstVariableType::Int)
            | (AstVariableType::Int, AstVariableType::Float)
            | (AstVariableType::Float, AstVariableType::Float)
    )
}

fn validate_motor_legacy_set_actions(
    statements: &[StepStatement],
    line: usize,
    device_kinds: &HashMap<String, DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::Set { target, value })
                if target.port == "self"
                    && matches!(device_kinds.get(&target.device), Some(DeviceKind::Motor)) =>
            {
                errors.push(PlcError::semantic_with_reason(
                    line,
                    format!("set {} {value} 旧写法已废弃", target.device),
                    format!(
                        "请改用显式端口写法：set {}.run on/off 或 set {}.direction forward/reverse",
                        target.device, target.device
                    ),
                ));
            }
            StepStatement::Repeat { body, .. } => {
                validate_motor_legacy_set_actions(body, line, device_kinds, errors)
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_motor_legacy_set_actions(
                        &branch.statements,
                        line,
                        device_kinds,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_motor_legacy_set_actions(
                        &branch.statements,
                        line,
                        device_kinds,
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

fn validate_wait_device_references_in_statements(
    statements: &[StepStatement],
    line: usize,
    device_kinds: &HashMap<String, DeviceKind>,
    device_port_types: &HashMap<String, PortType>,
    device_ranges: &HashMap<String, (f64, f64)>,
    device_units: &HashMap<String, String>,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Wait(wait) => {
                let should_validate_references =
                    matches!(wait.condition, WaitCondition::And(_) | WaitCondition::Or(_));
                for condition in wait_condition_terms(&wait.condition) {
                    if condition.is_expression_compare() {
                        continue;
                    }
                    validate_motor_legacy_wait_operand(&condition.left, line, device_kinds, errors);
                    if should_validate_references {
                        validate_wait_operand_device(
                            &condition.left,
                            line,
                            "wait 条件左值",
                            device_kinds,
                            errors,
                        );
                        if let LiteralValue::State(state) = &condition.right {
                            validate_device_reference(
                                &state.device,
                                line,
                                "wait 条件右值",
                                device_kinds,
                                errors,
                            );
                        }
                    }
                    if let Some((value, unit)) = threshold_literal_value_and_unit(&condition.right)
                    {
                        if wait_operand_device_name(&condition.left).is_some() {
                            validate_analog_threshold_comparison(
                                &condition.left,
                                value,
                                unit,
                                line,
                                "wait 条件阈值比较",
                                device_kinds,
                                device_port_types,
                                device_ranges,
                                device_units,
                                errors,
                            );
                        }
                    }
                }
            }
            StepStatement::IfElse { .. } => {}
            StepStatement::Repeat { body, .. } => {
                validate_wait_device_references_in_statements(
                    body,
                    line,
                    device_kinds,
                    device_port_types,
                    device_ranges,
                    device_units,
                    errors,
                );
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_wait_device_references_in_statements(
                        &branch.statements,
                        line,
                        device_kinds,
                        device_port_types,
                        device_ranges,
                        device_units,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_wait_device_references_in_statements(
                        &branch.statements,
                        line,
                        device_kinds,
                        device_port_types,
                        device_ranges,
                        device_units,
                        errors,
                    );
                }
            }
            StepStatement::Action(_)
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
}

fn wait_condition_terms(condition: &WaitCondition) -> Vec<&ConditionExpression> {
    match condition {
        WaitCondition::Single(term) => vec![term],
        WaitCondition::And(terms) | WaitCondition::Or(terms) => terms.iter().collect(),
    }
}

fn validate_wait_operand_device(
    operand: &str,
    line: usize,
    source: &str,
    device_kinds: &HashMap<String, DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    if let Some(candidate) = wait_operand_device_name(operand) {
        validate_device_reference(candidate, line, source, device_kinds, errors);
    }
}

fn validate_motor_legacy_wait_operand(
    operand: &str,
    line: usize,
    device_kinds: &HashMap<String, DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    let mut parts = operand.split('.');
    let Some(device) = parts.next() else {
        return;
    };
    let Some(state) = parts.next() else {
        return;
    };
    if parts.next().is_some() {
        return;
    }

    if matches!(device_kinds.get(device), Some(DeviceKind::Motor)) {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!("wait 条件使用了已废弃的电机状态写法 {device}.{state}"),
            format!(
                "请改用显式端口状态，例如 {device}.run.on/off 或 {device}.direction.forward/reverse"
            ),
        ));
    }
}

fn wait_operand_device_name(operand: &str) -> Option<&str> {
    let candidate = operand.split('.').next().unwrap_or(operand).trim();

    if candidate.is_empty() {
        None
    } else {
        Some(candidate)
    }
}

fn parse_threshold_target(target: &str) -> Option<(&str, Option<&str>)> {
    let mut parts = target.split('.');
    let device = parts.next()?.trim();
    if device.is_empty() {
        return None;
    }
    let Some(port) = parts.next() else {
        return Some((device, None));
    };
    if parts.next().is_some() {
        return None;
    }
    let port = port.trim();
    if port.is_empty() {
        return None;
    }
    Some((device, Some(port)))
}

fn map_safety_relation(relation: &AstSafetyRelation) -> IrSafetyRelation {
    match relation {
        AstSafetyRelation::ConflictsWith => IrSafetyRelation::ConflictsWith,
        AstSafetyRelation::Requires => IrSafetyRelation::Requires,
    }
}

fn validate_safety_operand(
    operand: &SafetyOperand,
    line: usize,
    source: &str,
    device_kinds: &HashMap<String, DeviceKind>,
    known_states: &HashMap<String, HashSet<String>>,
    device_port_types: &HashMap<String, PortType>,
    device_ranges: &HashMap<String, (f64, f64)>,
    device_units: &HashMap<String, String>,
    errors: &mut Vec<PlcError>,
) {
    match operand {
        SafetyOperand::State(state_ref) => {
            validate_state_reference(state_ref, line, source, device_kinds, known_states, errors);
        }
        SafetyOperand::Threshold {
            device,
            value,
            unit,
            ..
        } => {
            if let Some(device_name) = wait_operand_device_name(device) {
                validate_device_reference(device_name, line, source, device_kinds, errors);
            }
            validate_analog_threshold_comparison(
                device,
                *value,
                unit.as_deref(),
                line,
                "safety 阈值比较",
                device_kinds,
                device_port_types,
                device_ranges,
                device_units,
                errors,
            );
        }
    }
}

fn validate_analog_threshold_comparison(
    target: &str,
    value: f64,
    value_unit: Option<&str>,
    line: usize,
    source: &str,
    device_kinds: &HashMap<String, DeviceKind>,
    device_port_types: &HashMap<String, PortType>,
    device_ranges: &HashMap<String, (f64, f64)>,
    device_units: &HashMap<String, String>,
    errors: &mut Vec<PlcError>,
) {
    let Some((device, port)) = parse_threshold_target(target) else {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!("{source} 目标 {target} 格式非法"),
            "阈值比较目标仅支持 device 或 device.port".to_string(),
        ));
        return;
    };

    let Some(kind) = device_kinds.get(device) else {
        return;
    };

    let range_key = if let Some(port_name) = port {
        let key = format!("{device}.{port_name}");
        let is_analog = device_port_types
            .get(&key)
            .is_some_and(|port_type| matches!(port_type, PortType::Analog));
        if !is_analog {
            errors.push(PlcError::type_mismatch_with_reason(
                line,
                "analog 端口",
                format!("{}.{}", device_kind_name(kind), port_name),
                format!("{source} {target}"),
                "阈值比较仅支持模拟量端口（如 cam_xy.following_error）",
            ));
            return;
        }
        Some(key)
    } else {
        if *kind != DeviceKind::AnalogInput {
            errors.push(PlcError::type_mismatch_with_reason(
                line,
                "analog_input",
                device_kind_name(kind),
                format!("{source} {target}"),
                "阈值比较仅支持 analog_input 设备，或 device.port 形式的模拟量端口",
            ));
            return;
        }
        Some(device.to_string())
    };

    let range = range_key
        .as_ref()
        .and_then(|key| device_ranges.get(key))
        .copied();

    if port.is_none() {
        let Some((min, max)) = range else {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!("模拟量输入 {device} 缺少 range，无法进行阈值比较"),
                "请在 [topology] 段为该设备声明 range: min..max",
            ));
            return;
        };

        if value < min || value > max {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!("阈值 {value} 超出 {device} 的 range {min}..{max}"),
                "请调整阈值或更新 range 范围",
            ));
        }
    }

    if let Some(expected_unit) = device_units.get(device) {
        if let Some(got_unit) = value_unit
            && got_unit != expected_unit
        {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!(
                    "阈值单位不一致：{device} 声明 unit=\"{expected_unit}\"，但比较值使用单位 \"{got_unit}\"",
                ),
                "请统一单位（修改 unit 或比较值单位），或移除比较值的单位后缀",
            ));
        }
    }
}

fn map_safety_operand(operand: &SafetyOperand) -> SafetyExpr {
    match operand {
        SafetyOperand::State(state_ref) => SafetyExpr::State(StateExpr {
            device: state_ref.device.clone(),
            port: state_ref.port.clone(),
            state: state_ref.state.clone(),
        }),
        SafetyOperand::Threshold {
            device,
            operator,
            value,
            unit: _,
        } => SafetyExpr::Threshold {
            device: device.clone(),
            operator: comparison_operator_to_string(operator).to_string(),
            value: value.to_string(),
        },
    }
}

fn comparison_operator_to_string(op: &ComparisonOperator) -> &'static str {
    match op {
        ComparisonOperator::Eq => "==",
        ComparisonOperator::Neq => "!=",
        ComparisonOperator::Gt => ">",
        ComparisonOperator::Lt => "<",
        ComparisonOperator::Gte => ">=",
        ComparisonOperator::Lte => "<=",
    }
}

fn map_timing_scope(target: &TimingTarget) -> TimingScope {
    match target {
        TimingTarget::Task { task } => TimingScope::Task { task: task.clone() },
        TimingTarget::Step { task, step } => TimingScope::Step {
            task: task.clone(),
            step: step.clone(),
        },
    }
}

fn map_timing_relation(relation: &AstTimingRelation) -> IrTimingRelation {
    match relation {
        AstTimingRelation::MustCompleteWithin => IrTimingRelation::MustCompleteWithin,
        AstTimingRelation::MustCompleteWithinWorstCase => {
            IrTimingRelation::MustCompleteWithinWorstCase
        }
        AstTimingRelation::MustStartAfter => IrTimingRelation::MustStartAfter,
    }
}

fn collect_device_timing_profiles(
    topology: &TopologySection,
) -> HashMap<String, DeviceTimingProfile> {
    topology
        .devices
        .iter()
        .map(|device| {
            (
                device.name.clone(),
                DeviceTimingProfile {
                    response_ms: device
                        .attributes
                        .response_time
                        .as_ref()
                        .map(duration_value_to_ms),
                    stroke_ms: device
                        .attributes
                        .stroke_time
                        .as_ref()
                        .map(duration_value_to_ms),
                    retract_ms: device
                        .attributes
                        .retract_time
                        .as_ref()
                        .map(duration_value_to_ms),
                    ramp_ms: device
                        .attributes
                        .ramp_time
                        .as_ref()
                        .map(duration_value_to_ms),
                },
            )
        })
        .collect()
}

fn collect_actions(statements: &[StepStatement], actions: &mut Vec<ActionStatement>) {
    for statement in statements {
        match statement {
            StepStatement::Action(action) => actions.push(action.clone()),
            StepStatement::IfElse { .. } => {}
            StepStatement::Repeat { body, .. } => collect_actions(body, actions),
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    collect_actions(&branch.statements, actions);
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    collect_actions(&branch.statements, actions);
                }
            }
            StepStatement::Wait(_)
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
}

fn action_to_timing(
    task_name: &str,
    step_name: &str,
    line: usize,
    action: &ActionStatement,
    profiles: &HashMap<String, DeviceTimingProfile>,
    errors: &mut Vec<PlcError>,
) -> Option<ActionTiming> {
    let (action_kind, target) = match action {
        ActionStatement::Extend { target } => (ActionKind::Extend, Some(target.device.as_str())),
        ActionStatement::Retract { target } => (ActionKind::Retract, Some(target.device.as_str())),
        ActionStatement::Set { target, .. } => (ActionKind::Set, Some(target.device.as_str())),
        ActionStatement::SetAnalog { target, .. } => {
            (ActionKind::SetAnalog, Some(target.device.as_str()))
        }
        ActionStatement::SetAnalogExpr { target, .. } => {
            (ActionKind::SetAnalogExpr, Some(target.device.as_str()))
        }
        ActionStatement::Compute { .. } => (ActionKind::Compute, None),
        ActionStatement::Call { .. } => return None,
        ActionStatement::CamEngage { target } => (ActionKind::CamEngage, Some(target.as_str())),
        ActionStatement::CamDisengage { target } => {
            (ActionKind::CamDisengage, Some(target.as_str()))
        }
        ActionStatement::CamSwitch { target, .. } => (ActionKind::CamSwitch, Some(target.as_str())),
        ActionStatement::CamPhase { target, .. } => (ActionKind::CamPhase, Some(target.as_str())),
        ActionStatement::Log { .. } => (ActionKind::Log, None),
    };

    let Some(target) = target else {
        return None;
    };

    let Some(profile) = profiles.get(target) else {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            "设备",
            target,
            "请先在 [topology] 段定义该设备并补充物理参数".to_string(),
        ));
        return None;
    };

    let duration_ms = match action_kind {
        ActionKind::Extend => profile
            .stroke_ms
            .or(profile.response_ms)
            .or(profile.ramp_ms),
        ActionKind::Retract => profile
            .retract_ms
            .or(profile.response_ms)
            .or(profile.ramp_ms),
        ActionKind::Set | ActionKind::SetAnalog | ActionKind::SetAnalogExpr => {
            profile.ramp_ms.or(profile.response_ms)
        }
        ActionKind::CamEngage
        | ActionKind::CamDisengage
        | ActionKind::CamSwitch
        | ActionKind::CamPhase
        | ActionKind::CallExtern
        | ActionKind::Compute
        | ActionKind::Log => None,
    }?;

    Some(ActionTiming {
        action: ActionRef {
            task_name: task_name.to_string(),
            step_name: step_name.to_string(),
            action_kind,
            target: Some(target.to_string()),
        },
        interval: TimeInterval {
            min_ms: duration_ms,
            max_ms: duration_ms,
        },
    })
}

fn insert_action_timing(intervals: &mut BTreeMap<String, ActionTiming>, timing: ActionTiming) {
    let action_name = action_kind_name(&timing.action.action_kind);
    let target = timing.action.target.as_deref().unwrap_or("_");
    let base_key = format!(
        "{}.{}.{}.{}",
        timing.action.task_name, timing.action.step_name, action_name, target
    );

    if !intervals.contains_key(&base_key) {
        intervals.insert(base_key, timing);
        return;
    }

    let mut duplicate_index = 2usize;
    loop {
        let key = format!("{base_key}.{duplicate_index}");
        if !intervals.contains_key(&key) {
            intervals.insert(key, timing);
            return;
        }
        duplicate_index += 1;
    }
}

fn action_kind_name(action_kind: &ActionKind) -> &'static str {
    match action_kind {
        ActionKind::Extend => "extend",
        ActionKind::Retract => "retract",
        ActionKind::Set => "set",
        ActionKind::SetAnalog => "set_analog",
        ActionKind::SetAnalogExpr => "set_analog_expr",
        ActionKind::Compute => "compute",
        ActionKind::CallExtern => "call_extern",
        ActionKind::CamEngage => "cam_engage",
        ActionKind::CamDisengage => "cam_disengage",
        ActionKind::CamSwitch => "cam_switch",
        ActionKind::CamPhase => "cam_phase",
        ActionKind::Log => "log",
    }
}

fn default_states_for_kind(kind: &DeviceKind) -> &'static [&'static str] {
    match kind {
        DeviceKind::Cylinder => &["extended", "retracted"],
        DeviceKind::DigitalOutput
        | DeviceKind::DigitalInput
        | DeviceKind::SolenoidValve
        | DeviceKind::Sensor
        | DeviceKind::Motor
        | DeviceKind::StepperMotor
        | DeviceKind::Vfd
        | DeviceKind::ServoDrive
        | DeviceKind::CamCoupling => &["on", "off", "forward", "reverse", "active", "idle"],
        DeviceKind::AnalogInput | DeviceKind::AnalogOutput | DeviceKind::Pid | DeviceKind::Plc => {
            &[]
        }
    }
}

fn completion_target_for_step(
    task: &TaskDeclaration,
    step_index: usize,
    task_on_complete_targets: &HashMap<String, Option<State>>,
) -> Option<State> {
    if step_index + 1 < task.steps.len() {
        return Some(State {
            task_name: task.name.clone(),
            step_name: task.steps[step_index + 1].name.clone(),
        });
    }

    task_on_complete_targets
        .get(&task.name)
        .cloned()
        .unwrap_or(None)
}

fn analyze_statements(
    statements: &[StepStatement],
    wait_ctx: &WaitExpressionContext,
) -> AnalyzedStatements {
    let mut analyzed = AnalyzedStatements::default();

    for statement in statements {
        match statement {
            StepStatement::Action(action) => {
                if let Some(mapped) = action_to_transition_action(action) {
                    analyzed.actions.push(mapped);
                }
            }
            StepStatement::Wait(wait) => {
                analyzed
                    .waits
                    .push(wait_to_guard_expression(wait, wait_ctx));
            }
            StepStatement::IfElse {
                condition,
                then_goto,
                else_goto,
            } => analyzed.if_elses.push(IfElseSpec {
                condition: condition.clone(),
                then_goto: then_goto.clone(),
                else_goto: else_goto.clone(),
            }),
            StepStatement::Delay { duration_ms } => analyzed.delays_ms.push(*duration_ms),
            StepStatement::Repeat { .. } => {}
            StepStatement::Timeout(timeout) => analyzed.timeouts.push(timeout.clone()),
            StepStatement::Goto(goto) => analyzed.gotos.push(goto.clone()),
            StepStatement::Parallel(block) => analyzed.parallel_blocks.push(block.clone()),
            StepStatement::Race(block) => analyzed.race_blocks.push(block.clone()),
            StepStatement::AllowIndefiniteWait(_) => {}
        }
    }

    analyzed
}

fn build_parallel_block(
    builder: &mut StateMachineBuilder,
    task: &TaskDeclaration,
    step_name: &str,
    source_state: &State,
    block_index: usize,
    block: &ParallelBlock,
    completion_target: Option<State>,
    task_initial_states: &HashMap<String, State>,
    task_defined_steps: &HashMap<String, HashSet<String>>,
    errors: &mut Vec<PlcError>,
    parent_actions: Vec<TransitionAction>,
    wait_ctx: &WaitExpressionContext,
) {
    let fork_state_name = format!("{step_name}__parallel_{}_fork", block_index + 1);
    let join_state_name = format!("{step_name}__parallel_{}_join", block_index + 1);

    let fork_state = builder.add_state(&task.name, &fork_state_name);
    let join_state = builder.add_state(&task.name, &join_state_name);

    builder.add_transition(
        source_state.clone(),
        fork_state.clone(),
        TransitionGuard::Always,
        parent_actions,
        Vec::new(),
    );

    for (branch_index, branch) in block.branches.iter().enumerate() {
        let branch_state_name = format!(
            "{step_name}__parallel_{}_branch_{}",
            block_index + 1,
            branch_index + 1
        );
        let branch_state = builder.add_state(&task.name, &branch_state_name);

        builder.add_transition(
            fork_state.clone(),
            branch_state.clone(),
            TransitionGuard::Always,
            Vec::new(),
            Vec::new(),
        );

        let analyzed = analyze_statements(&branch.statements, wait_ctx);

        for goto in &analyzed.gotos {
            if let Some(target) = resolve_task_target(
                goto,
                task_initial_states,
                task_defined_steps,
                errors,
                "goto",
            ) {
                builder.add_transition(
                    branch_state.clone(),
                    target,
                    TransitionGuard::Always,
                    analyzed.actions.clone(),
                    Vec::new(),
                );
            }
        }

        for if_else in &analyzed.if_elses {
            let expr = condition_to_expression(&if_else.condition);

            if let Some(then_target) = resolve_task_target(
                &if_else.then_goto,
                task_initial_states,
                task_defined_steps,
                errors,
                "if/else then goto",
            ) {
                builder.add_transition(
                    branch_state.clone(),
                    then_target,
                    TransitionGuard::Condition {
                        expression: expr.clone(),
                    },
                    analyzed.actions.clone(),
                    Vec::new(),
                );
            }

            if let Some(else_target) = resolve_task_target(
                &if_else.else_goto,
                task_initial_states,
                task_defined_steps,
                errors,
                "if/else else goto",
            ) {
                builder.add_transition(
                    branch_state.clone(),
                    else_target,
                    TransitionGuard::Condition {
                        expression: format!("NOT({expr})"),
                    },
                    analyzed.actions.clone(),
                    Vec::new(),
                );
            }
        }

        for (delay_index, duration_ms) in analyzed.delays_ms.iter().enumerate() {
            builder.add_transition(
                branch_state.clone(),
                join_state.clone(),
                TransitionGuard::Delay {
                    duration_ms: *duration_ms,
                },
                Vec::new(),
                vec![TimerOperation {
                    timer_name: format!(
                        "{}.{}.parallel_{}_branch_{}.delay_{}",
                        task.name,
                        step_name,
                        block_index + 1,
                        branch_index + 1,
                        delay_index + 1
                    ),
                    operation: TimerOperationKind::Start,
                    duration_ms: Some(*duration_ms),
                }],
            );
        }

        for (timeout_index, timeout) in analyzed.timeouts.iter().enumerate() {
            if let Some(target) = resolve_task_target(
                &timeout.target,
                task_initial_states,
                task_defined_steps,
                errors,
                "timeout -> goto",
            ) {
                let duration_ms = duration_to_ms(timeout);
                builder.add_transition(
                    branch_state.clone(),
                    target,
                    TransitionGuard::Timeout { duration_ms },
                    Vec::new(),
                    vec![TimerOperation {
                        timer_name: format!(
                            "{}.{}.parallel_{}_branch_{}.timeout_{}",
                            task.name,
                            step_name,
                            block_index + 1,
                            branch_index + 1,
                            timeout_index + 1
                        ),
                        operation: TimerOperationKind::Start,
                        duration_ms: Some(duration_ms),
                    }],
                );
            }
        }

        for wait_expression in &analyzed.waits {
            builder.add_transition(
                branch_state.clone(),
                join_state.clone(),
                TransitionGuard::Condition {
                    expression: wait_expression.clone(),
                },
                analyzed.actions.clone(),
                Vec::new(),
            );
        }

        for (nested_parallel_index, nested_parallel) in analyzed.parallel_blocks.iter().enumerate()
        {
            build_parallel_block(
                builder,
                task,
                &format!(
                    "{step_name}__parallel_{}_branch_{}",
                    block_index + 1,
                    branch_index + 1
                ),
                &branch_state,
                nested_parallel_index,
                nested_parallel,
                Some(join_state.clone()),
                task_initial_states,
                task_defined_steps,
                errors,
                analyzed.actions.clone(),
                wait_ctx,
            );
        }

        for (nested_race_index, nested_race) in analyzed.race_blocks.iter().enumerate() {
            build_race_block(
                builder,
                task,
                &format!(
                    "{step_name}__parallel_{}_branch_{}",
                    block_index + 1,
                    branch_index + 1
                ),
                &branch_state,
                nested_race_index,
                nested_race,
                Some(join_state.clone()),
                task_initial_states,
                task_defined_steps,
                errors,
                analyzed.actions.clone(),
                wait_ctx,
            );
        }

        let has_control_flow = !analyzed.waits.is_empty()
            || !analyzed.delays_ms.is_empty()
            || !analyzed.gotos.is_empty()
            || !analyzed.if_elses.is_empty()
            || !analyzed.parallel_blocks.is_empty()
            || !analyzed.race_blocks.is_empty();
        if !has_control_flow {
            builder.add_transition(
                branch_state,
                join_state.clone(),
                TransitionGuard::Always,
                analyzed.actions,
                Vec::new(),
            );
        }
    }

    if let Some(target) = completion_target {
        builder.add_transition(
            join_state,
            target,
            TransitionGuard::Always,
            Vec::new(),
            Vec::new(),
        );
    }
}

fn build_race_block(
    builder: &mut StateMachineBuilder,
    task: &TaskDeclaration,
    step_name: &str,
    source_state: &State,
    block_index: usize,
    block: &RaceBlock,
    completion_target: Option<State>,
    task_initial_states: &HashMap<String, State>,
    task_defined_steps: &HashMap<String, HashSet<String>>,
    errors: &mut Vec<PlcError>,
    parent_actions: Vec<TransitionAction>,
    wait_ctx: &WaitExpressionContext,
) {
    let decision_state_name = format!("{step_name}__race_{}_decision", block_index + 1);
    let decision_state = builder.add_state(&task.name, &decision_state_name);

    builder.add_transition(
        source_state.clone(),
        decision_state.clone(),
        TransitionGuard::Always,
        parent_actions,
        Vec::new(),
    );

    for (branch_index, branch) in block.branches.iter().enumerate() {
        let branch_state_name = format!(
            "{step_name}__race_{}_branch_{}",
            block_index + 1,
            branch_index + 1
        );
        let branch_state = builder.add_state(&task.name, &branch_state_name);

        builder.add_transition(
            decision_state.clone(),
            branch_state.clone(),
            TransitionGuard::Always,
            Vec::new(),
            Vec::new(),
        );

        let analyzed = analyze_statements(&branch.statements, wait_ctx);
        let branch_completion_target = branch
            .then_goto
            .as_ref()
            .and_then(|goto| {
                resolve_task_target(
                    goto,
                    task_initial_states,
                    task_defined_steps,
                    errors,
                    "race then goto",
                )
            })
            .or_else(|| completion_target.clone());

        for goto in &analyzed.gotos {
            if let Some(target) = resolve_task_target(
                goto,
                task_initial_states,
                task_defined_steps,
                errors,
                "goto",
            ) {
                builder.add_transition(
                    branch_state.clone(),
                    target,
                    TransitionGuard::Always,
                    analyzed.actions.clone(),
                    Vec::new(),
                );
            }
        }

        for if_else in &analyzed.if_elses {
            let expr = condition_to_expression(&if_else.condition);

            if let Some(then_target) = resolve_task_target(
                &if_else.then_goto,
                task_initial_states,
                task_defined_steps,
                errors,
                "if/else then goto",
            ) {
                builder.add_transition(
                    branch_state.clone(),
                    then_target,
                    TransitionGuard::Condition {
                        expression: expr.clone(),
                    },
                    analyzed.actions.clone(),
                    Vec::new(),
                );
            }

            if let Some(else_target) = resolve_task_target(
                &if_else.else_goto,
                task_initial_states,
                task_defined_steps,
                errors,
                "if/else else goto",
            ) {
                builder.add_transition(
                    branch_state.clone(),
                    else_target,
                    TransitionGuard::Condition {
                        expression: format!("NOT({expr})"),
                    },
                    analyzed.actions.clone(),
                    Vec::new(),
                );
            }
        }

        for (delay_index, duration_ms) in analyzed.delays_ms.iter().enumerate() {
            if let Some(target) = branch_completion_target.clone() {
                builder.add_transition(
                    branch_state.clone(),
                    target,
                    TransitionGuard::Delay {
                        duration_ms: *duration_ms,
                    },
                    Vec::new(),
                    vec![TimerOperation {
                        timer_name: format!(
                            "{}.{}.race_{}_branch_{}.delay_{}",
                            task.name,
                            step_name,
                            block_index + 1,
                            branch_index + 1,
                            delay_index + 1
                        ),
                        operation: TimerOperationKind::Start,
                        duration_ms: Some(*duration_ms),
                    }],
                );
            }
        }

        for (timeout_index, timeout) in analyzed.timeouts.iter().enumerate() {
            if let Some(target) = resolve_task_target(
                &timeout.target,
                task_initial_states,
                task_defined_steps,
                errors,
                "timeout -> goto",
            ) {
                let duration_ms = duration_to_ms(timeout);
                builder.add_transition(
                    branch_state.clone(),
                    target,
                    TransitionGuard::Timeout { duration_ms },
                    Vec::new(),
                    vec![TimerOperation {
                        timer_name: format!(
                            "{}.{}.race_{}_branch_{}.timeout_{}",
                            task.name,
                            step_name,
                            block_index + 1,
                            branch_index + 1,
                            timeout_index + 1
                        ),
                        operation: TimerOperationKind::Start,
                        duration_ms: Some(duration_ms),
                    }],
                );
            }
        }

        for wait_expression in &analyzed.waits {
            if let Some(target) = branch_completion_target.clone() {
                builder.add_transition(
                    branch_state.clone(),
                    target,
                    TransitionGuard::Condition {
                        expression: wait_expression.clone(),
                    },
                    analyzed.actions.clone(),
                    Vec::new(),
                );
            }
        }

        for (nested_parallel_index, nested_parallel) in analyzed.parallel_blocks.iter().enumerate()
        {
            build_parallel_block(
                builder,
                task,
                &format!(
                    "{step_name}__race_{}_branch_{}",
                    block_index + 1,
                    branch_index + 1
                ),
                &branch_state,
                nested_parallel_index,
                nested_parallel,
                branch_completion_target.clone(),
                task_initial_states,
                task_defined_steps,
                errors,
                analyzed.actions.clone(),
                wait_ctx,
            );
        }

        for (nested_race_index, nested_race) in analyzed.race_blocks.iter().enumerate() {
            build_race_block(
                builder,
                task,
                &format!(
                    "{step_name}__race_{}_branch_{}",
                    block_index + 1,
                    branch_index + 1
                ),
                &branch_state,
                nested_race_index,
                nested_race,
                branch_completion_target.clone(),
                task_initial_states,
                task_defined_steps,
                errors,
                analyzed.actions.clone(),
                wait_ctx,
            );
        }

        let has_control_flow = !analyzed.waits.is_empty()
            || !analyzed.delays_ms.is_empty()
            || !analyzed.gotos.is_empty()
            || !analyzed.if_elses.is_empty()
            || !analyzed.parallel_blocks.is_empty()
            || !analyzed.race_blocks.is_empty();
        if !has_control_flow {
            if let Some(target) = branch_completion_target {
                builder.add_transition(
                    branch_state,
                    target,
                    TransitionGuard::Always,
                    analyzed.actions,
                    Vec::new(),
                );
            }
        }
    }
}

fn resolve_task_target(
    target: &GotoDirective,
    task_initial_states: &HashMap<String, State>,
    task_defined_steps: &HashMap<String, HashSet<String>>,
    errors: &mut Vec<PlcError>,
    source: &str,
) -> Option<State> {
    let line = target.line.max(1);
    let Some(initial_state) = task_initial_states.get(&target.task) else {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            " task",
            &target.task,
            format!("{source} 目标必须是已定义 task 名称"),
        ));
        return None;
    };

    let Some(step) = &target.step else {
        return Some(initial_state.clone());
    };

    let Some(steps) = task_defined_steps.get(&target.task) else {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            " task",
            &target.task,
            format!("{source} 目标必须是已定义 task 名称"),
        ));
        return None;
    };

    if !steps.contains(step) {
        let synthetic_hint = step.contains("__parallel_") || step.contains("__race_");
        if synthetic_hint {
            errors.push(PlcError::semantic(
                line,
                format!(
                    "{source} 不允许跳转到 parallel/race 内部合成 step {}.{step}",
                    target.task
                ),
            ));
        } else {
            errors.push(PlcError::semantic(
                line,
                format!("{source} 引用了未定义 step {}.{step}", target.task),
            ));
        }
        return None;
    }

    Some(State {
        task_name: target.task.clone(),
        step_name: step.clone(),
    })
}

fn action_to_transition_action(action: &ActionStatement) -> Option<TransitionAction> {
    match action {
        ActionStatement::Extend { target } => Some(TransitionAction::Extend {
            target: target.device.clone(),
            port: target.port.clone(),
        }),
        ActionStatement::Retract { target } => Some(TransitionAction::Retract {
            target: target.device.clone(),
            port: target.port.clone(),
        }),
        ActionStatement::Set { target, value } => Some(TransitionAction::Set {
            target: target.device.clone(),
            port: target.port.clone(),
            value: set_enum_to_binary(value)?,
        }),
        ActionStatement::SetAnalog { target, value } => Some(TransitionAction::SetAnalog {
            target: target.device.clone(),
            port: target.port.clone(),
            value_raw: value.to_string(),
        }),
        ActionStatement::SetAnalogExpr { target, expr } => Some(TransitionAction::SetAnalogExpr {
            target: target.device.clone(),
            port: target.port.clone(),
            expr_raw: expression_to_raw(expr),
        }),
        ActionStatement::Compute { target, expr } => Some(TransitionAction::Compute {
            target: target.clone(),
            expr_raw: expression_to_raw(expr),
        }),
        ActionStatement::Call {
            function,
            args,
            binding,
        } => Some(TransitionAction::CallExtern {
            function: function.clone(),
            args_raw: args.iter().map(expression_to_raw).collect(),
            binding: lower_extern_call_binding(binding),
        }),
        ActionStatement::CamEngage { target } => Some(TransitionAction::CamEngage {
            target: target.clone(),
        }),
        ActionStatement::CamDisengage { target } => Some(TransitionAction::CamDisengage {
            target: target.clone(),
        }),
        ActionStatement::CamSwitch { target, new_table } => Some(TransitionAction::CamSwitch {
            target: target.clone(),
            new_table: new_table.clone(),
        }),
        ActionStatement::CamPhase { target, offset } => Some(TransitionAction::CamPhase {
            target: target.clone(),
            offset_expr_raw: expression_to_raw(offset),
        }),
        ActionStatement::Log { message } => Some(TransitionAction::Log {
            message: message.clone(),
        }),
    }
}

fn lower_extern_call_binding(binding: &AstExternCallBinding) -> IrExternCallBinding {
    match binding {
        AstExternCallBinding::Single(name) => IrExternCallBinding::Single(name.clone()),
        AstExternCallBinding::Tuple(names) => IrExternCallBinding::Tuple(names.clone()),
    }
}

fn expression_to_raw(expr: &AstExpression) -> String {
    match expr {
        AstExpression::Literal(v) => v.to_string(),
        AstExpression::Boolean(v) => v.to_string(),
        AstExpression::Variable(name) => name.clone(),
        AstExpression::UnaryNeg(inner) => format!("-({})", expression_to_raw(inner)),
        AstExpression::UnaryNot(inner) => format!("NOT({})", expression_to_raw(inner)),
        AstExpression::BinaryOp { op, left, right } => format!(
            "({} {} {})",
            expression_to_raw(left),
            binary_operator_to_raw(*op),
            expression_to_raw(right)
        ),
        AstExpression::FunctionCall { name, args } => format!(
            "{}({})",
            name,
            args.iter()
                .map(expression_to_raw)
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

fn binary_operator_to_raw(op: AstBinaryOperator) -> &'static str {
    match op {
        AstBinaryOperator::Add => "+",
        AstBinaryOperator::Sub => "-",
        AstBinaryOperator::Mul => "*",
        AstBinaryOperator::Div => "/",
        AstBinaryOperator::Mod => "%",
        AstBinaryOperator::Eq => "==",
        AstBinaryOperator::Neq => "!=",
        AstBinaryOperator::Gt => ">",
        AstBinaryOperator::Lt => "<",
        AstBinaryOperator::Gte => ">=",
        AstBinaryOperator::Lte => "<=",
        AstBinaryOperator::And => "AND",
        AstBinaryOperator::Or => "OR",
    }
}

fn set_enum_to_binary(value: &str) -> Option<IrBinaryValue> {
    match value {
        "on" | "forward" | "active" => Some(IrBinaryValue::On),
        "off" | "reverse" | "idle" => Some(IrBinaryValue::Off),
        _ => None,
    }
}

fn wait_to_guard_expression(wait: &WaitStatement, wait_ctx: &WaitExpressionContext) -> String {
    wait_condition_to_expression(&wait.condition, wait_ctx)
}

fn wait_condition_to_expression(
    condition: &WaitCondition,
    wait_ctx: &WaitExpressionContext,
) -> String {
    match condition {
        WaitCondition::Single(single) => wait_term_to_expression(single, wait_ctx),
        WaitCondition::And(conditions) => conditions
            .iter()
            .map(|condition| wait_term_to_expression(condition, wait_ctx))
            .collect::<Vec<_>>()
            .join(" AND "),
        WaitCondition::Or(conditions) => conditions
            .iter()
            .map(|condition| wait_term_to_expression(condition, wait_ctx))
            .collect::<Vec<_>>()
            .join(" OR "),
    }
}

fn analog_region_state_name(index: usize) -> String {
    format!("region_{index}")
}

#[derive(Clone, Copy)]
enum ComparisonOp {
    Eq,
    Neq,
    Gt,
    Lt,
    Gte,
    Lte,
}

fn comparison_op_from_ast(op: &ComparisonOperator) -> ComparisonOp {
    match op {
        ComparisonOperator::Eq => ComparisonOp::Eq,
        ComparisonOperator::Neq => ComparisonOp::Neq,
        ComparisonOperator::Gt => ComparisonOp::Gt,
        ComparisonOperator::Lt => ComparisonOp::Lt,
        ComparisonOperator::Gte => ComparisonOp::Gte,
        ComparisonOperator::Lte => ComparisonOp::Lte,
    }
}

fn region_intersects(op: ComparisonOp, value: f64, min: f64, max: f64) -> bool {
    match op {
        ComparisonOp::Eq => value >= min && value <= max,
        ComparisonOp::Neq => !(min == max && value == min),
        ComparisonOp::Gt => max > value,
        // For analog waits we need the selected region set to be a *sufficient* condition
        // (otherwise a wait may be satisfied even when the numeric predicate is false).
        //
        // Using intersection semantics for / becomes a tautology when regions overlap
        // at the split point (e.g. [0..T] and [T..MAX]), because both regions intersect.
        // So for non-strict comparisons we pick regions that are entirely within the predicate.
        ComparisonOp::Gte => min >= value,
        ComparisonOp::Lt => min < value,
        ComparisonOp::Lte => max <= value,
    }
}

fn wait_term_to_expression(
    condition: &ConditionExpression,
    wait_ctx: &WaitExpressionContext,
) -> String {
    if condition.is_expression_compare() {
        return condition_to_expression(condition);
    }

    if let Some((value, _unit)) = threshold_literal_value_and_unit(&condition.right) {
        if let Some(device_name) = wait_operand_device_name(&condition.left) {
            if let Some(regions) = wait_ctx.analog_input_regions.get(device_name) {
                let op = comparison_op_from_ast(&condition.operator);
                let mut matching = Vec::new();

                for (index, (min, max)) in regions.iter().enumerate() {
                    if region_intersects(op, value, *min, *max) {
                        matching.push(index);
                    }
                }

                if !matching.is_empty() {
                    let rendered = matching
                        .into_iter()
                        .map(analog_region_state_name)
                        .collect::<Vec<_>>()
                        .join(", ");
                    return format!("{device_name} in {{{rendered}}}");
                }
            }
        }
    }

    condition_to_expression(condition)
}

fn condition_to_expression(condition: &ConditionExpression) -> String {
    if let Some((left, right)) = condition.expression_pair() {
        return format!(
            "{} {} {}",
            expression_to_raw(left),
            match condition.operator {
                ComparisonOperator::Eq => "==",
                ComparisonOperator::Neq => "!=",
                ComparisonOperator::Gt => ">",
                ComparisonOperator::Lt => "<",
                ComparisonOperator::Gte => ">=",
                ComparisonOperator::Lte => "<=",
            },
            expression_to_raw(right)
        );
    }

    let operator = match condition.operator {
        ComparisonOperator::Eq => "==",
        ComparisonOperator::Neq => "!=",
        ComparisonOperator::Gt => ">",
        ComparisonOperator::Lt => "<",
        ComparisonOperator::Gte => ">=",
        ComparisonOperator::Lte => "<=",
    };

    format!(
        "{} {} {}",
        condition.left,
        operator,
        literal_to_expression(&condition.right)
    )
}

fn literal_to_expression(literal: &LiteralValue) -> String {
    match literal {
        LiteralValue::Boolean(value) => value.to_string(),
        LiteralValue::Number(value) => value.to_string(),
        LiteralValue::Measured(measured) => format!("{}{}", measured.value, measured.unit),
        LiteralValue::String(value) => format!("\"{}\"", value),
        LiteralValue::State(state) => format!("{}.{}", state.device, state.state),
    }
}

fn threshold_literal_value_and_unit(literal: &LiteralValue) -> Option<(f64, Option<&str>)> {
    match literal {
        LiteralValue::Number(value) => Some((*value, None)),
        LiteralValue::Measured(measured) => Some((measured.value, Some(measured.unit.as_str()))),
        LiteralValue::Boolean(_) | LiteralValue::String(_) | LiteralValue::State(_) => None,
    }
}

fn duration_to_ms(timeout: &TimeoutDirective) -> u64 {
    duration_value_to_ms(&timeout.duration)
}

fn duration_value_to_ms(duration: &DurationValue) -> u64 {
    match duration.unit {
        TimeUnit::Ms => duration.value,
        TimeUnit::S => duration.value.saturating_mul(1000),
    }
}

fn ast_type_to_ir_kind(device_type: &DeviceType) -> DeviceKind {
    match device_type {
        DeviceType::DigitalOutput => DeviceKind::DigitalOutput,
        DeviceType::DigitalInput => DeviceKind::DigitalInput,
        DeviceType::Plc => DeviceKind::Plc,
        DeviceType::SolenoidValve => DeviceKind::SolenoidValve,
        DeviceType::Cylinder => DeviceKind::Cylinder,
        DeviceType::Sensor => DeviceKind::Sensor,
        DeviceType::Motor => DeviceKind::Motor,
        DeviceType::StepperMotor => DeviceKind::StepperMotor,
        DeviceType::Vfd => DeviceKind::Vfd,
        DeviceType::ServoDrive => DeviceKind::ServoDrive,
        DeviceType::CamCoupling => DeviceKind::CamCoupling,
        DeviceType::AnalogInput => DeviceKind::AnalogInput,
        DeviceType::AnalogOutput => DeviceKind::AnalogOutput,
        DeviceType::Pid => DeviceKind::Pid,
    }
}

fn connection_type_for_relation(
    relation: &TopologyRelation,
    from: &DeviceKind,
    to: &DeviceKind,
) -> Option<ConnectionType> {
    match relation {
        TopologyRelation::DrivenBy => driven_by_connection_type_for(from, to),
        TopologyRelation::ReportsTo => reports_to_connection_type_for(from, to),
        TopologyRelation::Detects => detects_connection_type_for(from, to),
    }
}

fn driven_by_connection_type_for(from: &DeviceKind, to: &DeviceKind) -> Option<ConnectionType> {
    match (from, to) {
        (DeviceKind::DigitalOutput, DeviceKind::SolenoidValve)
        | (DeviceKind::DigitalOutput, DeviceKind::Motor)
        | (DeviceKind::DigitalOutput, DeviceKind::StepperMotor)
        | (DeviceKind::DigitalOutput, DeviceKind::Vfd)
        | (DeviceKind::DigitalOutput, DeviceKind::ServoDrive)
        | (DeviceKind::DigitalOutput, DeviceKind::CamCoupling) => Some(ConnectionType::Electrical),
        (DeviceKind::SolenoidValve, DeviceKind::Cylinder) => Some(ConnectionType::Pneumatic),
        (DeviceKind::AnalogOutput, DeviceKind::Motor)
        | (DeviceKind::AnalogOutput, DeviceKind::Vfd) => Some(ConnectionType::Analog),
        (DeviceKind::AnalogOutput, DeviceKind::CamCoupling) => Some(ConnectionType::Analog),
        _ => None,
    }
}

fn reports_to_connection_type_for(from: &DeviceKind, to: &DeviceKind) -> Option<ConnectionType> {
    match (from, to) {
        (DeviceKind::Sensor, DeviceKind::DigitalInput) => Some(ConnectionType::Logical),
        (DeviceKind::Sensor, DeviceKind::AnalogInput) => Some(ConnectionType::Analog),
        _ => None,
    }
}

fn detects_connection_type_for(from: &DeviceKind, to: &DeviceKind) -> Option<ConnectionType> {
    match (from, to) {
        (DeviceKind::Cylinder, DeviceKind::Sensor)
        | (DeviceKind::Motor, DeviceKind::Sensor)
        | (DeviceKind::StepperMotor, DeviceKind::Sensor)
        | (DeviceKind::Vfd, DeviceKind::Sensor)
        | (DeviceKind::ServoDrive, DeviceKind::Sensor)
        | (DeviceKind::CamCoupling, DeviceKind::Sensor)
        | (DeviceKind::SolenoidValve, DeviceKind::Sensor) => Some(ConnectionType::Logical),
        _ => None,
    }
}

fn device_kind_name(kind: &DeviceKind) -> &'static str {
    match kind {
        DeviceKind::DigitalOutput => "digital_output",
        DeviceKind::DigitalInput => "digital_input",
        DeviceKind::Plc => "plc",
        DeviceKind::SolenoidValve => "solenoid_valve",
        DeviceKind::Cylinder => "cylinder",
        DeviceKind::Sensor => "sensor",
        DeviceKind::Motor => "motor",
        DeviceKind::StepperMotor => "stepper_motor",
        DeviceKind::Vfd => "vfd",
        DeviceKind::ServoDrive => "servo_drive",
        DeviceKind::CamCoupling => "cam_coupling",
        DeviceKind::AnalogInput => "analog_input",
        DeviceKind::AnalogOutput => "analog_output",
        DeviceKind::Pid => "pid",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_constraint_set, build_state_machine, build_timing_model, build_topology_graph,
        preprocess_program, preprocess_program_with_library,
    };
    use crate::device_library::DeviceLibrary;
    use crate::ir::{
        ConnectionType, DeviceKind, SafetyRelation, TimerOperationKind, TimingRelation,
        TimingScope, TransitionGuard,
    };
    use crate::parser::parse_plc;
    use petgraph::visit::EdgeRef;
    use std::path::Path;

    #[test]
    fn preprocess_expands_plc_device_ports_into_internal_io_nodes() {
        let input = r#"
[topology]
device plc_main: plc { ports: [Y0:digital:producer, X0:digital:consumer] }
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
    decel_time: 120ms
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
device plc_main: plc { ports: [Y0:digital:producer] }
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

device stepper_x: stepper_motor
device vfd_main: vfd
device servo_y: servo_drive

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
}
