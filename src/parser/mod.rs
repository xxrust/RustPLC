use crate::ast::{
    ActionStatement, ActionTarget, AxisAutoResetPolicy, AxisFaultContractDeclaration,
    AxisFaultPropagationScope, AxisFaultRouteDirective, AxisFaultRouteKind, AxisFaultSeverity,
    AxisStopMode, BinaryOperator, Branch, CamPoint, CamTableDeclaration, CamTableMode,
    CausalityConstraint, ComparisonOperator, ConditionExpression, ConstraintsSection,
    DeviceAttributes, DeviceDeclaration, DevicePort, DeviceTags, DeviceType, DurationValue,
    EffectKind, EffectStatement, Expression, ExternCallBinding, ExternFunctionContract,
    ExternFunctionDeclaration, ExternFunctionParameter, GotoDirective, LiteralValue, MeasuredValue,
    OnCompleteDirective, ParallelBlock, PlcProgram, PortRole, PortType, RaceBlock, RaceBranch,
    ResourceClaimConstraint, ResourceClaimSource, SafetyConstraint, SafetyOperand, SafetyRelation,
    SemanticResourceDeclaration, SemanticResourceMode, StateReference, StepDeclaration,
    StepStatement, TaskDeclaration, TasksSection, TimeUnit, TimeoutDirective, TimingConstraint,
    TimingRelation, TimingTarget, TopologyConnection, TopologyRelation, TopologySection,
    VariableDeclaration, VariableType, WaitCondition, WaitStatement, WorkpieceAllowDeclaration,
    WorkpieceCarrierDeclaration, WorkpieceCarrierLayout, WorkpieceDerivationDeclaration,
    WorkpieceHolderDeclaration, WorkpiecePropertyDeclaration, WorkpiecePropertyType,
    WorkpieceSiteDeclaration, WorkpieceSiteKind, WorkpieceTypeDeclaration,
};
use crate::error::PlcError;
use pest::Parser;
use pest::error::LineColLocation;
use pest::iterators::Pair;
use std::collections::HashSet;

#[derive(pest_derive::Parser)]
#[grammar = "parser/plc.pest"]
pub struct PlcParser;

pub fn parse_topology(input: &str) -> Result<(), pest::error::Error<Rule>> {
    PlcParser::parse(Rule::topology_file, input).map(|_| ())
}

pub fn parse_constraints(input: &str) -> Result<(), pest::error::Error<Rule>> {
    PlcParser::parse(Rule::constraints_file, input).map(|_| ())
}

pub fn parse_tasks(input: &str) -> Result<(), pest::error::Error<Rule>> {
    PlcParser::parse(Rule::tasks_file, input).map(|_| ())
}

pub fn parse_plc(input: &str) -> Result<PlcProgram, PlcError> {
    reject_deprecated_connected_to(input)?;
    let mut pairs = PlcParser::parse(Rule::plc_file, input).map_err(map_parse_error)?;
    let plc_pair = pairs
        .next()
        .ok_or_else(|| PlcError::parse(1, "未找到可解析的 PLC 程序"))?;

    let program = parse_plc_pair(plc_pair)?;
    reject_extern_calls_in_expression_context(&program)?;
    Ok(program)
}

fn reject_deprecated_connected_to(input: &str) -> Result<(), PlcError> {
    for (line_idx, line) in input.lines().enumerate() {
        let code = line.split('#').next().unwrap_or(line);
        if let Some(col_idx) = code.find("connected_to") {
            let tail = &code[col_idx + "connected_to".len()..];
            if tail.trim_start().starts_with(':') {
                return Err(PlcError::parse_at(
                    "<input>",
                    line_idx + 1,
                    col_idx + 1,
                    "属性 connected_to 已废弃，请改用 relation { from: Device.Port, to: Device.Port, via: ... }",
                ));
            }
        }
    }

    Ok(())
}

fn parse_plc_pair(pair: Pair<Rule>) -> Result<PlcProgram, PlcError> {
    let mut topology = None;
    let mut constraints = None;
    let mut tasks = None;

    for section in pair.into_inner() {
        match section.as_rule() {
            Rule::topology_section => topology = Some(parse_topology_section(section)?),
            Rule::constraints_section => constraints = Some(parse_constraints_section(section)?),
            Rule::tasks_section => tasks = Some(parse_tasks_section(section)?),
            _ => {}
        }
    }

    Ok(PlcProgram {
        topology: topology.ok_or_else(|| PlcError::parse(1, "缺少 [topology] 段"))?,
        constraints: constraints.ok_or_else(|| PlcError::parse(1, "缺少 [constraints] 段"))?,
        tasks: tasks.ok_or_else(|| PlcError::parse(1, "缺少 [tasks] 段"))?,
    })
}

fn reject_extern_calls_in_expression_context(program: &PlcProgram) -> Result<(), PlcError> {
    let extern_names: HashSet<&str> = program
        .topology
        .extern_functions
        .iter()
        .map(|func| func.name.as_str())
        .collect();

    if extern_names.is_empty() {
        return Ok(());
    }

    for task in &program.tasks.tasks {
        for step in &task.steps {
            reject_extern_calls_in_statements(
                &step.statements,
                &extern_names,
                step.line.max(1),
                &task.name,
                &step.name,
            )?;
        }
    }

    Ok(())
}

fn reject_extern_calls_in_statements(
    statements: &[StepStatement],
    extern_names: &HashSet<&str>,
    line: usize,
    task_name: &str,
    step_name: &str,
) -> Result<(), PlcError> {
    for statement in statements {
        match statement {
            StepStatement::Action(action) => {
                reject_extern_calls_in_action(action, extern_names, line, task_name, step_name)?;
            }
            StepStatement::Effect(_) => {}
            StepStatement::Wait(wait) => {
                reject_extern_calls_in_wait(wait, extern_names, line, task_name, step_name)?;
            }
            StepStatement::IfElse { condition, .. } => {
                reject_extern_calls_in_condition(
                    condition,
                    extern_names,
                    line,
                    task_name,
                    step_name,
                )?;
            }
            StepStatement::Repeat { body, .. } => {
                reject_extern_calls_in_statements(body, extern_names, line, task_name, step_name)?;
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    reject_extern_calls_in_statements(
                        &branch.statements,
                        extern_names,
                        line,
                        task_name,
                        step_name,
                    )?;
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    reject_extern_calls_in_statements(
                        &branch.statements,
                        extern_names,
                        line,
                        task_name,
                        step_name,
                    )?;
                }
            }
            StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }

    Ok(())
}

fn reject_extern_calls_in_action(
    action: &ActionStatement,
    extern_names: &HashSet<&str>,
    line: usize,
    task_name: &str,
    step_name: &str,
) -> Result<(), PlcError> {
    match action {
        ActionStatement::SetAnalogExpr { expr, .. } | ActionStatement::Compute { expr, .. } => {
            reject_extern_calls_in_expression(expr, extern_names, line, task_name, step_name)?;
        }
        ActionStatement::CamPhase { offset, .. } => {
            reject_extern_calls_in_expression(offset, extern_names, line, task_name, step_name)?;
        }
        ActionStatement::Call { args, .. } => {
            for arg in args {
                reject_extern_calls_in_expression(arg, extern_names, line, task_name, step_name)?;
            }
        }
        ActionStatement::Extend { .. }
        | ActionStatement::Retract { .. }
        | ActionStatement::Set { .. }
        | ActionStatement::SetAnalog { .. }
        | ActionStatement::AxisMoveRelative { .. }
        | ActionStatement::AxisMoveAbsolute { .. }
        | ActionStatement::CamEngage { .. }
        | ActionStatement::CamDisengage { .. }
        | ActionStatement::CamSwitch { .. }
        | ActionStatement::Log { .. } => {}
    }

    Ok(())
}

fn reject_extern_calls_in_wait(
    wait: &WaitStatement,
    extern_names: &HashSet<&str>,
    line: usize,
    task_name: &str,
    step_name: &str,
) -> Result<(), PlcError> {
    match &wait.condition {
        WaitCondition::Single(condition) => {
            reject_extern_calls_in_condition(condition, extern_names, line, task_name, step_name)?;
        }
        WaitCondition::And(conditions) | WaitCondition::Or(conditions) => {
            for condition in conditions {
                reject_extern_calls_in_condition(
                    condition,
                    extern_names,
                    line,
                    task_name,
                    step_name,
                )?;
            }
        }
    }

    Ok(())
}

fn reject_extern_calls_in_condition(
    condition: &ConditionExpression,
    extern_names: &HashSet<&str>,
    line: usize,
    task_name: &str,
    step_name: &str,
) -> Result<(), PlcError> {
    if let Some((left, right)) = condition.expression_pair() {
        reject_extern_calls_in_expression(left, extern_names, line, task_name, step_name)?;
        reject_extern_calls_in_expression(right, extern_names, line, task_name, step_name)?;
    }

    Ok(())
}

fn reject_extern_calls_in_expression(
    expr: &Expression,
    extern_names: &HashSet<&str>,
    line: usize,
    task_name: &str,
    step_name: &str,
) -> Result<(), PlcError> {
    match expr {
        Expression::Literal(_) | Expression::Boolean(_) | Expression::Variable(_) => Ok(()),
        Expression::UnaryNeg(inner) => {
            reject_extern_calls_in_expression(inner, extern_names, line, task_name, step_name)
        }
        Expression::UnaryNot(inner) => {
            reject_extern_calls_in_expression(inner, extern_names, line, task_name, step_name)
        }
        Expression::BinaryOp { left, right, .. } => {
            reject_extern_calls_in_expression(left, extern_names, line, task_name, step_name)?;
            reject_extern_calls_in_expression(right, extern_names, line, task_name, step_name)
        }
        Expression::FunctionCall { name, args } => {
            if extern_names.contains(name.as_str()) {
                return Err(PlcError::parse_with_reason(
                    line,
                    format!("extern 函数 {name} 只能在 action: call 中调用"),
                    format!(
                        "请改写为 `action: call {name}(...) -> <binding>`（task: {task_name}, step: {step_name}）"
                    ),
                ));
            }
            for arg in args {
                reject_extern_calls_in_expression(arg, extern_names, line, task_name, step_name)?;
            }
            Ok(())
        }
    }
}

fn parse_topology_section(pair: Pair<Rule>) -> Result<TopologySection, PlcError> {
    let mut devices = Vec::new();
    let mut workpiece_types = Vec::new();
    let mut workpiece_sites = Vec::new();
    let mut workpiece_holders = Vec::new();
    let mut workpiece_carriers = Vec::new();
    let mut semantic_resources = Vec::new();
    let mut explicit_connections = Vec::new();
    let mut variables = Vec::new();
    let mut cam_tables = Vec::new();
    let mut extern_functions = Vec::new();
    let mut axis_fault_contracts = Vec::new();

    for entry in pair.into_inner() {
        match entry.as_rule() {
            Rule::device_declaration => devices.push(parse_device_declaration(entry)?),
            Rule::workpiece_type_declaration => {
                workpiece_types.push(parse_workpiece_type_declaration(entry)?);
            }
            Rule::workpiece_site_declaration => {
                workpiece_sites.push(parse_workpiece_site_declaration(entry)?);
            }
            Rule::workpiece_holder_declaration => {
                workpiece_holders.push(parse_workpiece_holder_declaration(entry)?);
            }
            Rule::workpiece_carrier_declaration => {
                workpiece_carriers.push(parse_workpiece_carrier_declaration(entry)?);
            }
            Rule::semantic_resource_declaration => {
                semantic_resources.push(parse_semantic_resource_declaration(entry)?);
            }
            Rule::relation_declaration => {
                explicit_connections.push(parse_relation_declaration(entry)?);
            }
            Rule::variable_declaration => variables.push(parse_variable_declaration(entry)?),
            Rule::cam_table_declaration => cam_tables.push(parse_cam_table_declaration(entry)?),
            Rule::extern_function_declaration => {
                extern_functions.push(parse_extern_function_declaration(entry)?);
            }
            Rule::axis_fault_contract_declaration => {
                axis_fault_contracts.push(parse_axis_fault_contract_declaration(entry)?);
            }
            _ => {}
        }
    }

    Ok(TopologySection {
        devices,
        workpiece_types,
        workpiece_sites,
        workpiece_holders,
        workpiece_carriers,
        semantic_resources,
        connections: explicit_connections,
        variables,
        cam_tables,
        extern_functions,
        axis_fault_contracts,
    })
}

fn parse_workpiece_type_declaration(
    pair: Pair<Rule>,
) -> Result<WorkpieceTypeDeclaration, PlcError> {
    let line = line_of(&pair);
    let mut name = None;
    let mut properties = Vec::new();
    let mut normal_terminal_states = Vec::new();
    let mut abnormal_terminal_states = Vec::new();
    let mut ingress_sites = Vec::new();
    let mut normal_egress_sites = Vec::new();
    let mut abnormal_egress_sites = Vec::new();
    let mut allows = Vec::new();
    let mut derived_from = Vec::new();

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::identifier => name = Some(part.as_str().to_string()),
            Rule::workpiece_type_block => {
                for entry in part.into_inner() {
                    if entry.as_rule() != Rule::workpiece_type_entry {
                        continue;
                    }
                    let entry_line = line_of(&entry);
                    let mut inner = entry.into_inner();
                    let field = inner
                        .next()
                        .ok_or_else(|| PlcError::parse(entry_line, "workpiece 字段缺少名称"))?
                        .as_str()
                        .to_string();
                    let value = inner.next().ok_or_else(|| {
                        PlcError::parse(entry_line, format!("workpiece 字段 {field} 缺少值"))
                    })?;
                    let value_inner = first_inner(value, entry_line, "workpiece 字段值")?;
                    match field.as_str() {
                        "properties" => properties = parse_workpiece_property_list(value_inner)?,
                        "normal_terminal_states" => {
                            normal_terminal_states = expect_workpiece_identifier_list(
                                value_inner,
                                "normal_terminal_states",
                            )?
                        }
                        "abnormal_terminal_states" => {
                            abnormal_terminal_states = expect_workpiece_identifier_list(
                                value_inner,
                                "abnormal_terminal_states",
                            )?
                        }
                        "ingress_sites" => {
                            ingress_sites =
                                expect_workpiece_reference_list(value_inner, "ingress_sites")?
                        }
                        "normal_egress_sites" => {
                            normal_egress_sites =
                                expect_workpiece_reference_list(value_inner, "normal_egress_sites")?
                        }
                        "abnormal_egress_sites" => {
                            abnormal_egress_sites = expect_workpiece_reference_list(
                                value_inner,
                                "abnormal_egress_sites",
                            )?
                        }
                        "allows" => {
                            allows = parse_workpiece_allow_rule_list(value_inner, entry_line)?
                        }
                        "derived_from" => {
                            derived_from =
                                parse_workpiece_derivation_rule_list(value_inner, entry_line)?
                        }
                        _ => {
                            return Err(PlcError::parse(
                                entry_line,
                                format!("不支持的 workpiece 字段: {field}"),
                            ));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(WorkpieceTypeDeclaration {
        line,
        name: name.ok_or_else(|| PlcError::parse(line, "workpiece 声明缺少名称"))?,
        properties,
        normal_terminal_states,
        abnormal_terminal_states,
        ingress_sites,
        normal_egress_sites,
        abnormal_egress_sites,
        allows,
        derived_from,
    })
}

fn parse_workpiece_property_list(
    pair: Pair<Rule>,
) -> Result<Vec<WorkpiecePropertyDeclaration>, PlcError> {
    let line = line_of(&pair);
    let list = if pair.as_rule() == Rule::workpiece_property_list {
        pair
    } else {
        first_inner(pair, line, "properties")?
    };
    if list.as_rule() != Rule::workpiece_property_list {
        return Err(PlcError::parse(line, "properties 需要属性列表"));
    }

    list.into_inner()
        .filter(|part| part.as_rule() == Rule::workpiece_property)
        .map(parse_workpiece_property)
        .collect()
}

fn expect_workpiece_reference_list(
    pair: Pair<Rule>,
    field_name: &str,
) -> Result<Vec<String>, PlcError> {
    let line = line_of(&pair);
    if pair.as_rule() != Rule::workpiece_reference_list {
        return Err(PlcError::parse(
            line,
            format!("{field_name} requires a workpiece reference list"),
        ));
    }

    let values = pair
        .into_inner()
        .filter(|part| part.as_rule() == Rule::workpiece_reference)
        .map(|part| part.as_str().to_string())
        .collect::<Vec<_>>();
    if values.is_empty() {
        return Err(PlcError::parse(
            line,
            format!("{field_name} must contain at least one reference"),
        ));
    }
    Ok(values)
}

fn expect_workpiece_identifier_list(
    pair: Pair<Rule>,
    field_name: &str,
) -> Result<Vec<String>, PlcError> {
    if pair.as_rule() == Rule::identifier_list {
        return expect_identifier_list(pair, field_name);
    }
    let line = line_of(&pair);
    if pair.as_rule() != Rule::workpiece_reference_list {
        return Err(PlcError::parse(
            line,
            format!("{field_name} requires an identifier list"),
        ));
    }
    let values = pair
        .into_inner()
        .filter(|part| part.as_rule() == Rule::workpiece_reference)
        .map(|part| part.as_str().to_string())
        .collect::<Vec<_>>();
    if values.iter().any(|value| value.contains(".slot[")) {
        return Err(PlcError::parse(
            line,
            format!("{field_name} only accepts bare identifier entries"),
        ));
    }
    if values.is_empty() {
        return Err(PlcError::parse(
            line,
            format!("{field_name} must contain at least one identifier"),
        ));
    }
    Ok(values)
}

fn parse_workpiece_allow_rule_list(
    pair: Pair<Rule>,
    line: usize,
) -> Result<Vec<WorkpieceAllowDeclaration>, PlcError> {
    if pair.as_rule() != Rule::workpiece_allow_rule_list {
        return Err(PlcError::parse(line, "allows list expected"));
    }
    pair.into_inner()
        .filter(|part| part.as_rule() == Rule::workpiece_allow_rule)
        .map(parse_workpiece_allow_rule)
        .collect()
}

fn parse_workpiece_allow_rule(pair: Pair<Rule>) -> Result<WorkpieceAllowDeclaration, PlcError> {
    let line = line_of(&pair);
    let target = pair
        .into_inner()
        .find(|part| part.as_rule() == Rule::identifier)
        .ok_or_else(|| PlcError::parse(line, "split_into target missing"))?
        .as_str()
        .to_string();
    Ok(WorkpieceAllowDeclaration::SplitInto { target })
}

fn parse_workpiece_derivation_rule_list(
    pair: Pair<Rule>,
    line: usize,
) -> Result<Vec<WorkpieceDerivationDeclaration>, PlcError> {
    if pair.as_rule() == Rule::identifier_list {
        return Ok(pair
            .into_inner()
            .filter(|part| part.as_rule() == Rule::identifier)
            .map(|part| WorkpieceDerivationDeclaration::WorkpieceType {
                workpiece_type: part.as_str().to_string(),
            })
            .collect());
    }
    if pair.as_rule() == Rule::workpiece_reference_list {
        let mut out = Vec::new();
        for part in pair.into_inner() {
            if part.as_rule() != Rule::workpiece_reference {
                continue;
            }
            let raw = part.as_str();
            if raw.contains(".slot[") {
                return Err(PlcError::parse(
                    line,
                    "derived_from only accepts workpiece type names or merge(...)",
                ));
            }
            out.push(WorkpieceDerivationDeclaration::WorkpieceType {
                workpiece_type: raw.to_string(),
            });
        }
        return Ok(out);
    }
    if pair.as_rule() != Rule::workpiece_derivation_rule_list {
        return Err(PlcError::parse(line, "derived_from list expected"));
    }
    pair.into_inner()
        .filter(|part| part.as_rule() == Rule::workpiece_derivation_rule)
        .map(parse_workpiece_derivation_rule)
        .collect()
}

fn parse_workpiece_derivation_rule(
    pair: Pair<Rule>,
) -> Result<WorkpieceDerivationDeclaration, PlcError> {
    let line = line_of(&pair);
    let raw = pair.as_str().trim();
    if raw.starts_with("merge(") && raw.ends_with(')') {
        let inner = &raw["merge(".len()..raw.len() - 1];
        let inputs = inner
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        if inputs.len() < 2 {
            return Err(PlcError::parse(
                line,
                "merge derivation needs at least two inputs",
            ));
        }
        return Ok(WorkpieceDerivationDeclaration::Merge { inputs });
    }

    if raw.is_empty() {
        return Err(PlcError::parse(line, "derived_from source missing"));
    }
    Ok(WorkpieceDerivationDeclaration::WorkpieceType {
        workpiece_type: raw.to_string(),
    })
}

fn parse_workpiece_property(pair: Pair<Rule>) -> Result<WorkpiecePropertyDeclaration, PlcError> {
    let line = line_of(&pair);
    let mut inner = pair.into_inner();
    let name = inner
        .next()
        .ok_or_else(|| PlcError::parse(line, "workpiece property 缺少名称"))?
        .as_str()
        .to_string();
    let property_type = parse_workpiece_property_type(
        inner
            .next()
            .ok_or_else(|| PlcError::parse(line, "workpiece property 缺少类型"))?,
    )?;
    Ok(WorkpiecePropertyDeclaration {
        name,
        property_type,
    })
}

fn parse_workpiece_property_type(pair: Pair<Rule>) -> Result<WorkpiecePropertyType, PlcError> {
    let line = line_of(&pair);
    let raw = pair.as_str().trim();
    if raw == "bool" {
        return Ok(WorkpiecePropertyType::Bool);
    }
    if raw.starts_with("enum(") && raw.ends_with(')') {
        let values = raw[5..raw.len() - 1]
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        if values.is_empty() {
            return Err(PlcError::parse(line, "enum 属性至少需要一个候选值"));
        }
        return Ok(WorkpiecePropertyType::Enum { values });
    }
    Err(PlcError::parse(
        line,
        format!("不支持的 workpiece property 类型: {raw}"),
    ))
}

fn parse_workpiece_site_declaration(
    pair: Pair<Rule>,
) -> Result<WorkpieceSiteDeclaration, PlcError> {
    let line = line_of(&pair);
    let mut name = None;
    let mut kind = None;
    let mut capacity = None;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::identifier => name = Some(part.as_str().to_string()),
            Rule::workpiece_site_kind => kind = Some(parse_workpiece_site_kind(part)?),
            Rule::workpiece_site_block => {
                let raw = part.as_str();
                let value = raw
                    .split(':')
                    .nth(1)
                    .map(str::trim)
                    .ok_or_else(|| PlcError::parse(line, "site capacity 缺少值"))?;
                capacity = Some(parse_u32_from_str(line, value, "capacity")?);
            }
            _ => {}
        }
    }

    Ok(WorkpieceSiteDeclaration {
        line,
        name: name.ok_or_else(|| PlcError::parse(line, "site 声明缺少名称"))?,
        kind: kind.ok_or_else(|| PlcError::parse(line, "site 声明缺少类型"))?,
        capacity: capacity.ok_or_else(|| PlcError::parse(line, "site 声明缺少 capacity"))?,
    })
}

fn parse_workpiece_site_kind(pair: Pair<Rule>) -> Result<WorkpieceSiteKind, PlcError> {
    let line = line_of(&pair);
    match pair.as_str() {
        "workpiece_location" => Ok(WorkpieceSiteKind::WorkpieceLocation),
        "carrier_location" => Ok(WorkpieceSiteKind::CarrierLocation),
        other => Err(PlcError::parse(
            line,
            format!("不支持的 site 类型: {other}"),
        )),
    }
}

fn parse_workpiece_holder_declaration(
    pair: Pair<Rule>,
) -> Result<WorkpieceHolderDeclaration, PlcError> {
    let line = line_of(&pair);
    let mut name = None;
    let mut capacity = None;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::identifier => name = Some(part.as_str().to_string()),
            Rule::workpiece_holder_block => {
                let raw = part.as_str();
                let value = raw
                    .split(':')
                    .nth(1)
                    .map(str::trim)
                    .ok_or_else(|| PlcError::parse(line, "holder capacity 缺少值"))?;
                capacity = Some(parse_u32_from_str(line, value, "capacity")?);
            }
            _ => {}
        }
    }

    Ok(WorkpieceHolderDeclaration {
        line,
        name: name.ok_or_else(|| PlcError::parse(line, "holder 声明缺少名称"))?,
        capacity: capacity.ok_or_else(|| PlcError::parse(line, "holder 声明缺少 capacity"))?,
    })
}

fn parse_workpiece_carrier_declaration(
    pair: Pair<Rule>,
) -> Result<WorkpieceCarrierDeclaration, PlcError> {
    let line = line_of(&pair);
    let mut name = None;
    let mut slots = None;
    let mut layout = None;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::identifier => name = Some(part.as_str().to_string()),
            Rule::workpiece_carrier_block => {
                for entry in part.into_inner() {
                    if entry.as_rule() != Rule::workpiece_carrier_entry {
                        continue;
                    }
                    let entry_line = line_of(&entry);
                    let mut inner = entry.into_inner();
                    let field = inner
                        .next()
                        .ok_or_else(|| PlcError::parse(entry_line, "carrier field name missing"))?
                        .as_str()
                        .to_string();
                    let value = inner.next().ok_or_else(|| {
                        PlcError::parse(entry_line, "carrier field value missing")
                    })?;
                    match field.as_str() {
                        "slots" => {
                            slots = Some(parse_u32_from_str(entry_line, value.as_str(), "slots")?)
                        }
                        "layout" => {
                            layout = Some(parse_workpiece_carrier_layout(first_inner(
                                value, entry_line, "layout",
                            )?)?)
                        }
                        _ => {
                            return Err(PlcError::parse(
                                entry_line,
                                format!("unsupported carrier field: {field}"),
                            ));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let layout = match (slots, layout) {
        (Some(count), None) => WorkpieceCarrierLayout::Slots { count },
        (None, Some(layout)) => layout,
        (Some(_), Some(_)) => {
            return Err(PlcError::parse(
                line,
                "workpiece_carrier cannot declare both slots and layout",
            ));
        }
        (None, None) => {
            return Err(PlcError::parse(
                line,
                "workpiece_carrier requires slots or layout",
            ));
        }
    };

    Ok(WorkpieceCarrierDeclaration {
        line,
        name: name.ok_or_else(|| PlcError::parse(line, "carrier name missing"))?,
        layout,
    })
}

fn parse_workpiece_carrier_layout(pair: Pair<Rule>) -> Result<WorkpieceCarrierLayout, PlcError> {
    let line = line_of(&pair);
    if pair.as_rule() != Rule::workpiece_carrier_layout {
        return Err(PlcError::parse(
            line,
            "layout requires grid(rows: m, cols: n)",
        ));
    }

    let raw = pair.as_str().trim();
    let inner = raw
        .strip_prefix("grid(")
        .and_then(|rest| rest.strip_suffix(')'))
        .ok_or_else(|| PlcError::parse(line, "unsupported carrier layout"))?;
    let mut rows = None;
    let mut cols = None;
    for item in inner.split(',') {
        let Some((key, value)) = item.split_once(':') else {
            return Err(PlcError::parse(
                line,
                format!("invalid layout item: {item}"),
            ));
        };
        match key.trim() {
            "rows" => rows = Some(parse_u32_from_str(line, value, "rows")?),
            "cols" => cols = Some(parse_u32_from_str(line, value, "cols")?),
            other => {
                return Err(PlcError::parse(
                    line,
                    format!("unsupported grid dimension: {other}"),
                ));
            }
        }
    }

    Ok(WorkpieceCarrierLayout::Grid {
        rows: rows.ok_or_else(|| PlcError::parse(line, "grid rows missing"))?,
        cols: cols.ok_or_else(|| PlcError::parse(line, "grid cols missing"))?,
    })
}

fn parse_semantic_resource_declaration(
    pair: Pair<Rule>,
) -> Result<SemanticResourceDeclaration, PlcError> {
    let line = line_of(&pair);
    let mut name = None;
    let mut mode = None;
    let mut purpose = None;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::identifier => name = Some(part.as_str().to_string()),
            Rule::semantic_resource_block => {
                for entry in part.into_inner() {
                    if entry.as_rule() != Rule::semantic_resource_entry {
                        continue;
                    }
                    let entry_line = line_of(&entry);
                    let mut inner = entry.into_inner();
                    let field = inner
                        .next()
                        .ok_or_else(|| {
                            PlcError::parse(entry_line, "semantic_resource 字段缺少名称")
                        })?
                        .as_str()
                        .to_string();
                    let value = inner.next().ok_or_else(|| {
                        PlcError::parse(
                            entry_line,
                            format!("semantic_resource 字段 {field} 缺少值"),
                        )
                    })?;
                    match field.as_str() {
                        "mode" => {
                            if mode.is_some() {
                                return Err(PlcError::parse(
                                    entry_line,
                                    "semantic_resource 字段 mode 重复声明",
                                ));
                            }
                            mode = Some(parse_semantic_resource_mode(value)?);
                        }
                        "purpose" => {
                            if purpose.is_some() {
                                return Err(PlcError::parse(
                                    entry_line,
                                    "semantic_resource 字段 purpose 重复声明",
                                ));
                            }
                            purpose = Some(parse_string_literal(value)?);
                        }
                        _ => {
                            return Err(PlcError::parse(
                                entry_line,
                                format!("不支持的 semantic_resource 字段: {field}"),
                            ));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(SemanticResourceDeclaration {
        line,
        name: name.ok_or_else(|| PlcError::parse(line, "resource 声明缺少名称"))?,
        mode: mode.ok_or_else(|| PlcError::parse(line, "resource 声明缺少 mode"))?,
        purpose,
    })
}

fn parse_semantic_resource_mode(pair: Pair<Rule>) -> Result<SemanticResourceMode, PlcError> {
    let line = line_of(&pair);
    match pair.as_str() {
        "exclusive" => Ok(SemanticResourceMode::Exclusive),
        other => Err(PlcError::parse(
            line,
            format!("不支持的 semantic_resource mode: {other}"),
        )),
    }
}

fn parse_axis_fault_contract_declaration(
    pair: Pair<Rule>,
) -> Result<AxisFaultContractDeclaration, PlcError> {
    let line = line_of(&pair);
    let col = col_of(&pair);
    let mut name = None;
    let mut axis = None;
    let mut severity = None;
    let mut stop_mode = None;
    let mut auto_reset_policy = None;
    let mut manual_ack_required = None;
    let mut propagation_scope = None;
    let mut propagation_targets = Vec::new();

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::identifier => name = Some(part.as_str().to_string()),
            Rule::axis_fault_contract_block => {
                let parsed = parse_axis_fault_contract_block(
                    part,
                    name.as_deref().unwrap_or("<unknown>"),
                    line,
                    col,
                )?;
                axis = Some(parsed.axis);
                severity = Some(parsed.severity);
                stop_mode = Some(parsed.stop_mode);
                auto_reset_policy = Some(parsed.auto_reset_policy);
                manual_ack_required = Some(parsed.manual_ack_required);
                propagation_scope = Some(parsed.propagation_scope);
                propagation_targets = parsed.propagation_targets;
            }
            _ => {}
        }
    }

    Ok(AxisFaultContractDeclaration {
        line,
        name: name.ok_or_else(|| PlcError::parse(line, "axis_fault_contract 声明缺少名称"))?,
        axis: axis.ok_or_else(|| PlcError::parse(line, "axis_fault_contract 缺少 axis 字段"))?,
        severity: severity
            .ok_or_else(|| PlcError::parse(line, "axis_fault_contract 缺少 severity 字段"))?,
        stop_mode: stop_mode
            .ok_or_else(|| PlcError::parse(line, "axis_fault_contract 缺少 stop_mode 字段"))?,
        auto_reset_policy: auto_reset_policy.ok_or_else(|| {
            PlcError::parse(line, "axis_fault_contract 缺少 auto_reset_policy 字段")
        })?,
        manual_ack_required: manual_ack_required.ok_or_else(|| {
            PlcError::parse(line, "axis_fault_contract 缺少 manual_ack_required 字段")
        })?,
        propagation_scope: propagation_scope.ok_or_else(|| {
            PlcError::parse(line, "axis_fault_contract 缺少 propagation_scope 字段")
        })?,
        propagation_targets,
    })
}

struct ParsedAxisFaultContract {
    axis: String,
    severity: AxisFaultSeverity,
    stop_mode: AxisStopMode,
    auto_reset_policy: AxisAutoResetPolicy,
    manual_ack_required: bool,
    propagation_scope: AxisFaultPropagationScope,
    propagation_targets: Vec<String>,
}

fn parse_axis_fault_contract_block(
    pair: Pair<Rule>,
    contract_name: &str,
    declaration_line: usize,
    declaration_col: usize,
) -> Result<ParsedAxisFaultContract, PlcError> {
    let mut axis = None;
    let mut severity = None;
    let mut stop_mode = None;
    let mut auto_reset_policy = None;
    let mut manual_ack_required = None;
    let mut propagation_scope = None;
    let mut propagation_targets: Option<Vec<String>> = None;

    for entry in pair.into_inner() {
        if entry.as_rule() != Rule::axis_fault_contract_entry {
            continue;
        }

        let line = line_of(&entry);
        let col = col_of(&entry);
        let mut inner = entry.into_inner();
        let field = inner
            .next()
            .ok_or_else(|| PlcError::parse(line, "axis_fault_contract 字段缺少名称"))?
            .as_str()
            .to_string();
        let value_wrapper = inner.next().ok_or_else(|| {
            PlcError::parse(line, format!("axis_fault_contract 字段 {field} 缺少值"))
        })?;
        let value = first_inner(value_wrapper, line, "axis_fault_contract 字段值")?;

        match field.as_str() {
            "axis" => {
                if axis.is_some() {
                    return Err(PlcError::parse_at(
                        "<input>",
                        line,
                        col,
                        "axis_fault_contract 字段 axis 重复声明",
                    ));
                }
                axis = Some(expect_identifier(value, "axis")?);
            }
            "severity" => {
                if severity.is_some() {
                    return Err(PlcError::parse_at(
                        "<input>",
                        line,
                        col,
                        "axis_fault_contract 字段 severity 重复声明",
                    ));
                }
                severity = Some(parse_axis_fault_severity(value)?);
            }
            "stop_mode" => {
                if stop_mode.is_some() {
                    return Err(PlcError::parse_at(
                        "<input>",
                        line,
                        col,
                        "axis_fault_contract 字段 stop_mode 重复声明",
                    ));
                }
                stop_mode = Some(parse_axis_stop_mode(value)?);
            }
            "auto_reset_policy" => {
                if auto_reset_policy.is_some() {
                    return Err(PlcError::parse_at(
                        "<input>",
                        line,
                        col,
                        "axis_fault_contract 字段 auto_reset_policy 重复声明",
                    ));
                }
                auto_reset_policy = Some(parse_axis_auto_reset_policy(value)?);
            }
            "manual_ack_required" => {
                if manual_ack_required.is_some() {
                    return Err(PlcError::parse_at(
                        "<input>",
                        line,
                        col,
                        "axis_fault_contract 字段 manual_ack_required 重复声明",
                    ));
                }
                manual_ack_required = Some(expect_boolean(value, "manual_ack_required")?);
            }
            "propagation_scope" => {
                if propagation_scope.is_some() {
                    return Err(PlcError::parse_at(
                        "<input>",
                        line,
                        col,
                        "axis_fault_contract 字段 propagation_scope 重复声明",
                    ));
                }
                propagation_scope = Some(parse_axis_fault_propagation_scope(value)?);
            }
            "propagation_targets" => {
                if propagation_targets.is_some() {
                    return Err(PlcError::parse_at(
                        "<input>",
                        line,
                        col,
                        "axis_fault_contract 字段 propagation_targets 重复声明",
                    ));
                }
                propagation_targets = Some(expect_identifier_list(value, "propagation_targets")?);
            }
            _ => {
                return Err(PlcError::parse_at(
                    "<input>",
                    line,
                    col,
                    format!("不支持的 axis_fault_contract 字段: {field}"),
                ));
            }
        }
    }

    let parsed = ParsedAxisFaultContract {
        axis: axis.ok_or_else(|| {
            PlcError::parse_at(
                "<input>",
                declaration_line,
                declaration_col,
                format!("axis_fault_contract {contract_name} 缺少必填字段 axis"),
            )
        })?,
        severity: severity.ok_or_else(|| {
            PlcError::parse_at(
                "<input>",
                declaration_line,
                declaration_col,
                format!("axis_fault_contract {contract_name} 缺少必填字段 severity"),
            )
        })?,
        stop_mode: stop_mode.ok_or_else(|| {
            PlcError::parse_at(
                "<input>",
                declaration_line,
                declaration_col,
                format!("axis_fault_contract {contract_name} 缺少必填字段 stop_mode"),
            )
        })?,
        auto_reset_policy: auto_reset_policy.ok_or_else(|| {
            PlcError::parse_at(
                "<input>",
                declaration_line,
                declaration_col,
                format!("axis_fault_contract {contract_name} 缺少必填字段 auto_reset_policy"),
            )
        })?,
        manual_ack_required: manual_ack_required.ok_or_else(|| {
            PlcError::parse_at(
                "<input>",
                declaration_line,
                declaration_col,
                format!("axis_fault_contract {contract_name} 缺少必填字段 manual_ack_required"),
            )
        })?,
        propagation_scope: propagation_scope.ok_or_else(|| {
            PlcError::parse_at(
                "<input>",
                declaration_line,
                declaration_col,
                format!("axis_fault_contract {contract_name} 缺少必填字段 propagation_scope"),
            )
        })?,
        propagation_targets: propagation_targets.unwrap_or_default(),
    };

    if parsed.propagation_scope == AxisFaultPropagationScope::Custom
        && parsed.propagation_targets.is_empty()
    {
        return Err(PlcError::parse_at(
            "<input>",
            declaration_line,
            declaration_col,
            format!(
                "axis_fault_contract {contract_name} 在 propagation_scope=custom 时必须提供 propagation_targets"
            ),
        ));
    }

    if parsed.propagation_scope != AxisFaultPropagationScope::Custom
        && !parsed.propagation_targets.is_empty()
    {
        return Err(PlcError::parse_at(
            "<input>",
            declaration_line,
            declaration_col,
            format!(
                "axis_fault_contract {contract_name} 仅在 propagation_scope=custom 时允许 propagation_targets"
            ),
        ));
    }

    Ok(parsed)
}

fn parse_axis_fault_severity(pair: Pair<Rule>) -> Result<AxisFaultSeverity, PlcError> {
    let line = line_of(&pair);
    let raw = expect_identifier(pair, "severity")?;
    match raw.as_str() {
        "recoverable" => Ok(AxisFaultSeverity::Recoverable),
        "non_recoverable" => Ok(AxisFaultSeverity::NonRecoverable),
        "safety" => Ok(AxisFaultSeverity::Safety),
        _ => Err(PlcError::parse(
            line,
            format!(
                "axis_fault_contract.severity 不支持 `{raw}`，仅支持 recoverable/non_recoverable/safety"
            ),
        )),
    }
}

fn parse_axis_stop_mode(pair: Pair<Rule>) -> Result<AxisStopMode, PlcError> {
    let line = line_of(&pair);
    let raw = expect_identifier(pair, "stop_mode")?;
    match raw.as_str() {
        "controlled" => Ok(AxisStopMode::Controlled),
        "quick" => Ok(AxisStopMode::Quick),
        "immediate" => Ok(AxisStopMode::Immediate),
        _ => Err(PlcError::parse(
            line,
            format!(
                "axis_fault_contract.stop_mode 不支持 `{raw}`，仅支持 controlled/quick/immediate"
            ),
        )),
    }
}

fn parse_axis_auto_reset_policy(pair: Pair<Rule>) -> Result<AxisAutoResetPolicy, PlcError> {
    let line = line_of(&pair);
    let raw = expect_identifier(pair, "auto_reset_policy")?;
    match raw.as_str() {
        "never" => Ok(AxisAutoResetPolicy::Never),
        "on_clear" => Ok(AxisAutoResetPolicy::OnClear),
        "immediate" => Ok(AxisAutoResetPolicy::Immediate),
        _ => Err(PlcError::parse(
            line,
            format!(
                "axis_fault_contract.auto_reset_policy 不支持 `{raw}`，仅支持 never/on_clear/immediate"
            ),
        )),
    }
}

fn parse_axis_fault_propagation_scope(
    pair: Pair<Rule>,
) -> Result<AxisFaultPropagationScope, PlcError> {
    let line = line_of(&pair);
    let raw = expect_identifier(pair, "propagation_scope")?;
    match raw.as_str() {
        "self" => Ok(AxisFaultPropagationScope::SelfOnly),
        "group" => Ok(AxisFaultPropagationScope::Group),
        "all" => Ok(AxisFaultPropagationScope::All),
        "followers" => Ok(AxisFaultPropagationScope::Followers),
        "custom" => Ok(AxisFaultPropagationScope::Custom),
        _ => Err(PlcError::parse(
            line,
            format!(
                "axis_fault_contract.propagation_scope 不支持 `{raw}`，仅支持 self/group/all/followers/custom"
            ),
        )),
    }
}

fn parse_relation_declaration(pair: Pair<Rule>) -> Result<TopologyConnection, PlcError> {
    let line = line_of(&pair);
    let col = col_of(&pair);
    let mut from = None::<(String, Option<String>)>;
    let mut to = None::<(String, Option<String>)>;
    let mut relation = None::<TopologyRelation>;

    for field in pair.into_inner() {
        if field.as_rule() != Rule::relation_field {
            continue;
        }

        let field_line = line_of(&field);
        let mut inner = field.into_inner();
        let field_name = inner
            .next()
            .ok_or_else(|| PlcError::parse(field_line, "relation 字段缺少名称"))?
            .as_str()
            .to_string();
        let value_wrapper = inner
            .next()
            .ok_or_else(|| PlcError::parse(field_line, format!("relation.{field_name} 缺少值")))?;
        let value = first_inner(value_wrapper, field_line, "relation 字段值")?;

        match field_name.as_str() {
            "from" => {
                if from.is_some() {
                    return Err(PlcError::parse(field_line, "relation.from 重复声明"));
                }
                from = Some(expect_relation_endpoint(value, "relation.from")?);
            }
            "to" => {
                if to.is_some() {
                    return Err(PlcError::parse(field_line, "relation.to 重复声明"));
                }
                to = Some(expect_relation_endpoint(value, "relation.to")?);
            }
            "via" => {
                if relation.is_some() {
                    return Err(PlcError::parse(field_line, "relation.via 重复声明"));
                }
                relation = Some(expect_topology_relation(value, "relation.via")?);
            }
            _ => {
                return Err(PlcError::parse(
                    field_line,
                    format!("不支持的 relation 字段: {field_name}"),
                ));
            }
        }
    }

    let (from_device, from_port) = from.ok_or_else(|| {
        PlcError::parse_at(
            "<input>",
            line,
            col,
            "relation 缺少 from 字段（需要 Device 或 Device.Port）",
        )
    })?;
    let (to_device, to_port) = to.ok_or_else(|| {
        PlcError::parse_at(
            "<input>",
            line,
            col,
            "relation 缺少 to 字段（需要 Device 或 Device.Port）",
        )
    })?;
    let relation = relation.ok_or_else(|| {
        PlcError::parse_at(
            "<input>",
            line,
            col,
            "relation 缺少 via 字段（driven_by/reports_to/detects）",
        )
    })?;

    Ok(TopologyConnection {
        from: from_device,
        to: to_device,
        relation,
        from_port,
        to_port,
        signal: None,
    })
}

fn parse_device_declaration(pair: Pair<Rule>) -> Result<DeviceDeclaration, PlcError> {
    let line = line_of(&pair);
    let mut name = None;
    let mut device_type = None;
    let mut attributes = DeviceAttributes::default();

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::identifier => name = Some(part.as_str().to_string()),
            Rule::device_type => device_type = Some(parse_device_type(part)?),
            Rule::attribute_block => attributes = parse_attribute_block(part)?,
            _ => {}
        }
    }

    Ok(DeviceDeclaration {
        line,
        name: name.ok_or_else(|| PlcError::parse(line, "device 声明缺少名称"))?,
        device_type: device_type.ok_or_else(|| PlcError::parse(line, "device 声明缺少类型"))?,
        attributes,
    })
}

fn parse_device_type(pair: Pair<Rule>) -> Result<DeviceType, PlcError> {
    let line = line_of(&pair);
    match pair.as_str() {
        "digital_output" => Ok(DeviceType::DigitalOutput),
        "digital_input" => Ok(DeviceType::DigitalInput),
        "plc" => Ok(DeviceType::Plc),
        "solenoid_valve" => Ok(DeviceType::SolenoidValve),
        "cylinder" => Ok(DeviceType::Cylinder),
        "sensor" => Ok(DeviceType::Sensor),
        "stepper_motor" => Ok(DeviceType::StepperMotor),
        "vfd" => Ok(DeviceType::Vfd),
        "servo_drive" => Ok(DeviceType::ServoDrive),
        "cam_coupling" => Ok(DeviceType::CamCoupling),
        "motor" => Ok(DeviceType::Motor),
        "analog_input" => Ok(DeviceType::AnalogInput),
        "analog_output" => Ok(DeviceType::AnalogOutput),
        "pid" => Ok(DeviceType::Pid),
        other => Err(PlcError::parse(line, format!("未知设备类型: {other}"))),
    }
}

fn parse_variable_declaration(pair: Pair<Rule>) -> Result<VariableDeclaration, PlcError> {
    let line = line_of(&pair);
    let mut name = None;
    let mut var_type = None;
    let mut initial_value = None;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::identifier => name = Some(part.as_str().to_string()),
            Rule::variable_type => var_type = Some(parse_variable_type(part)?),
            Rule::variable_initializer => {
                let init = first_inner(part, line, "variable 初始值")?;
                initial_value = Some(init.as_str().to_string());
            }
            _ => {}
        }
    }

    Ok(VariableDeclaration {
        line,
        name: name.ok_or_else(|| PlcError::parse(line, "variable 声明缺少名称"))?,
        var_type: var_type.ok_or_else(|| PlcError::parse(line, "variable 声明缺少类型"))?,
        initial_value: initial_value
            .ok_or_else(|| PlcError::parse(line, "variable 声明缺少初始值"))?,
    })
}

fn parse_variable_type(pair: Pair<Rule>) -> Result<VariableType, PlcError> {
    let line = line_of(&pair);
    match pair.as_str() {
        "float" => Ok(VariableType::Float),
        "int" => Ok(VariableType::Int),
        "bool" => Ok(VariableType::Bool),
        other => Err(PlcError::parse(line, format!("不支持的变量类型: {other}"))),
    }
}

fn parse_extern_function_declaration(
    pair: Pair<Rule>,
) -> Result<ExternFunctionDeclaration, PlcError> {
    let line = line_of(&pair);
    let col = col_of(&pair);
    let mut name = None;
    let mut params = Vec::new();
    let mut return_types = None;
    let mut contract = None;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::identifier => name = Some(part.as_str().to_string()),
            Rule::extern_param_list => params = parse_extern_param_list(part)?,
            Rule::extern_return_spec => return_types = Some(parse_extern_return_spec(part)?),
            Rule::extern_contract_block => {
                contract = Some(parse_extern_contract_block(
                    part,
                    name.as_deref().unwrap_or("<unknown>"),
                    line,
                    col,
                )?);
            }
            _ => {}
        }
    }

    Ok(ExternFunctionDeclaration {
        line,
        name: name.ok_or_else(|| PlcError::parse(line, "extern function 声明缺少名称"))?,
        params,
        return_types: return_types
            .ok_or_else(|| PlcError::parse(line, "extern function 声明缺少返回类型"))?,
        contract: contract
            .ok_or_else(|| PlcError::parse(line, "extern function 声明缺少 contract"))?,
    })
}

fn parse_extern_param_list(pair: Pair<Rule>) -> Result<Vec<ExternFunctionParameter>, PlcError> {
    let mut params = Vec::new();
    for param in pair.into_inner() {
        if param.as_rule() == Rule::extern_param {
            params.push(parse_extern_param(param)?);
        }
    }
    Ok(params)
}

fn parse_extern_param(pair: Pair<Rule>) -> Result<ExternFunctionParameter, PlcError> {
    let line = line_of(&pair);
    let mut name = None;
    let mut var_type = None;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::identifier => name = Some(part.as_str().to_string()),
            Rule::variable_type => var_type = Some(parse_variable_type(part)?),
            _ => {}
        }
    }

    Ok(ExternFunctionParameter {
        name: name.ok_or_else(|| PlcError::parse(line, "extern 参数缺少名称"))?,
        var_type: var_type.ok_or_else(|| PlcError::parse(line, "extern 参数缺少类型"))?,
    })
}

fn parse_extern_return_spec(pair: Pair<Rule>) -> Result<Vec<VariableType>, PlcError> {
    let line = line_of(&pair);
    let mut return_types = Vec::new();

    for part in pair.into_inner() {
        if part.as_rule() == Rule::variable_type {
            return_types.push(parse_variable_type(part)?);
        }
    }

    if return_types.is_empty() {
        return Err(PlcError::parse(line, "extern function 返回类型不能为空"));
    }

    Ok(return_types)
}

fn parse_extern_contract_block(
    pair: Pair<Rule>,
    function_name: &str,
    declaration_line: usize,
    declaration_col: usize,
) -> Result<ExternFunctionContract, PlcError> {
    let mut rust_module = None;
    let mut pure = None;
    let mut time_bound_us = None;

    for entry in pair.into_inner() {
        if entry.as_rule() != Rule::extern_contract_entry {
            continue;
        }

        let line = line_of(&entry);
        let col = col_of(&entry);
        let mut inner = entry.into_inner();
        let field = inner
            .next()
            .ok_or_else(|| PlcError::parse(line, "extern contract 字段缺少名称"))?
            .as_str()
            .to_string();
        let value_wrapper = inner
            .next()
            .ok_or_else(|| PlcError::parse(line, format!("extern contract 字段 {field} 缺少值")))?;
        let value = first_inner(value_wrapper, line, "extern contract 字段值")?;

        match field.as_str() {
            "rust_module" => {
                if rust_module.is_some() {
                    return Err(PlcError::parse_at(
                        "<input>",
                        line,
                        col,
                        "extern contract 字段 rust_module 重复声明",
                    ));
                }
                rust_module = Some(expect_string(value, "rust_module")?);
            }
            "pure" => {
                if pure.is_some() {
                    return Err(PlcError::parse_at(
                        "<input>",
                        line,
                        col,
                        "extern contract 字段 pure 重复声明",
                    ));
                }
                pure = Some(expect_boolean(value, "pure")?);
            }
            "time_bound_us" => {
                if time_bound_us.is_some() {
                    return Err(PlcError::parse_at(
                        "<input>",
                        line,
                        col,
                        "extern contract 字段 time_bound_us 重复声明",
                    ));
                }
                time_bound_us = Some(expect_u64(value, "time_bound_us")?);
            }
            _ => {
                return Err(PlcError::parse_at(
                    "<input>",
                    line,
                    col,
                    format!("不支持的 extern contract 字段: {field}"),
                ));
            }
        }
    }

    let rust_module = rust_module.ok_or_else(|| {
        PlcError::parse_at(
            "<input>",
            declaration_line,
            declaration_col,
            format!("extern function {function_name} 缺少必填 contract 字段 rust_module"),
        )
    })?;
    let pure = pure.ok_or_else(|| {
        PlcError::parse_at(
            "<input>",
            declaration_line,
            declaration_col,
            format!("extern function {function_name} 缺少必填 contract 字段 pure"),
        )
    })?;
    let time_bound_us = time_bound_us.ok_or_else(|| {
        PlcError::parse_at(
            "<input>",
            declaration_line,
            declaration_col,
            format!("extern function {function_name} 缺少必填 contract 字段 time_bound_us"),
        )
    })?;

    Ok(ExternFunctionContract {
        rust_module,
        pure,
        time_bound_us,
    })
}

fn parse_cam_table_declaration(pair: Pair<Rule>) -> Result<CamTableDeclaration, PlcError> {
    let line = line_of(&pair);
    let mut name = None;
    let mut mode = None;
    let mut points = Vec::new();

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::identifier => name = Some(part.as_str().to_string()),
            Rule::cam_table_mode => mode = Some(parse_cam_table_mode(part)?),
            Rule::cam_point_list => points = parse_cam_point_list(part)?,
            _ => {}
        }
    }

    Ok(CamTableDeclaration {
        line,
        name: name.ok_or_else(|| PlcError::parse(line, "cam_table 声明缺少名称"))?,
        mode: mode.ok_or_else(|| PlcError::parse(line, "cam_table 声明缺少 mode"))?,
        points,
    })
}

fn parse_cam_table_mode(pair: Pair<Rule>) -> Result<CamTableMode, PlcError> {
    let line = line_of(&pair);
    match pair.as_str() {
        "periodic" => Ok(CamTableMode::Periodic),
        "oneshot" => Ok(CamTableMode::Oneshot),
        other => Err(PlcError::parse(
            line,
            format!("不支持的 cam_table mode: {other}"),
        )),
    }
}

fn parse_cam_point_list(pair: Pair<Rule>) -> Result<Vec<CamPoint>, PlcError> {
    let mut points = Vec::new();
    for part in pair.into_inner() {
        if part.as_rule() == Rule::cam_point {
            points.push(parse_cam_point(part)?);
        }
    }
    Ok(points)
}

fn parse_cam_point(pair: Pair<Rule>) -> Result<CamPoint, PlcError> {
    let line = line_of(&pair);
    let mut numbers = Vec::new();
    for part in pair.into_inner() {
        if part.as_rule() == Rule::number {
            let parsed = part
                .as_str()
                .parse::<f64>()
                .map_err(|_| PlcError::parse(line, "cam_table 点位数值解析失败"))?;
            numbers.push(parsed);
        }
    }

    if numbers.len() != 2 {
        return Err(PlcError::parse(
            line,
            "cam_table 点位必须为 (master, slave)",
        ));
    }

    Ok(CamPoint {
        master: numbers[0],
        slave: numbers[1],
    })
}

fn parse_attribute_block(pair: Pair<Rule>) -> Result<DeviceAttributes, PlcError> {
    let mut attributes = DeviceAttributes::default();

    for attr in pair.into_inner() {
        if attr.as_rule() == Rule::attribute {
            apply_attribute(&mut attributes, attr)?;
        }
    }

    Ok(attributes)
}

fn apply_attribute(attributes: &mut DeviceAttributes, pair: Pair<Rule>) -> Result<(), PlcError> {
    let line = line_of(&pair);
    let col = col_of(&pair);
    let mut inner = pair.into_inner();

    let attr_name = inner
        .next()
        .ok_or_else(|| PlcError::parse(line, "属性缺少名称"))?
        .as_str()
        .to_string();
    let value_wrapper = inner
        .next()
        .ok_or_else(|| PlcError::parse(line, format!("属性 {attr_name} 缺少值")))?;
    let value = first_inner(value_wrapper, line, "属性值")?;

    match attr_name.as_str() {
        "driven_by" => {
            return Err(legacy_topology_attribute_error(line, col, "driven_by"));
        }
        "reports_to" => {
            return Err(legacy_topology_attribute_error(line, col, "reports_to"));
        }
        "purpose" => {
            attributes.purpose = Some(expect_string(value, "purpose")?);
        }
        "response_time" => {
            attributes.response_time = Some(expect_duration(value, "response_time")?);
        }
        "stroke_time" => {
            attributes.stroke_time = Some(expect_duration(value, "stroke_time")?);
        }
        "retract_time" => {
            attributes.retract_time = Some(expect_duration(value, "retract_time")?);
        }
        "stroke" => {
            attributes.stroke = Some(expect_measured(value, "stroke")?);
        }
        "subtype" => {
            attributes.subtype = Some(expect_identifier_or_string(value, "subtype")?);
        }
        "type" => {
            return Err(PlcError::parse_at(
                "<input>",
                line,
                col,
                "属性 type 已移除，请使用 subtype".to_string(),
            ));
        }
        "detects" => {
            return Err(legacy_topology_attribute_error(line, col, "detects"));
        }
        "debounce" => {
            attributes.debounce = Some(expect_duration(value, "debounce")?);
        }
        "inverted" => {
            attributes.inverted = Some(expect_boolean(value, "inverted")?);
        }
        "external" => {
            attributes.external = Some(expect_boolean(value, "external")?);
        }
        "rated_speed" => {
            attributes.rated_speed = Some(expect_measured(value, "rated_speed")?);
        }
        "ramp_time" => {
            attributes.ramp_time = Some(expect_duration(value, "ramp_time")?);
        }
        "model_ref" => {
            attributes.model_ref = Some(expect_identifier_or_string(value, "model_ref")?);
        }
        "config_ref" => {
            attributes.config_ref = Some(expect_identifier_or_string(value, "config_ref")?);
        }
        "motion_param_set" => {
            attributes.motion_param_set =
                Some(expect_identifier_or_string(value, "motion_param_set")?);
        }
        "states" => {
            attributes.custom_states = Some(expect_identifier_list(value, "states")?);
        }
        "ports" => {
            attributes.ports = expect_port_list(value, "ports")?;
        }
        "tags" => {
            attributes.tags = expect_tags(value, "tags")?;
        }
        "range" => {
            attributes.range = Some(parse_range_value(value)?);
        }
        "unit" => {
            attributes.unit = Some(expect_string(value, "unit")?);
        }
        "pv" => {
            attributes.pv = Some(expect_identifier(value, "pv")?);
        }
        "sp" => {
            attributes.sp = Some(expect_pid_setpoint(value, "sp")?);
        }
        "kp" => {
            attributes.kp = Some(expect_number(value, "kp")?);
        }
        "ki" => {
            attributes.ki = Some(expect_number(value, "ki")?);
        }
        "kd" => {
            attributes.kd = Some(expect_number(value, "kd")?);
        }
        "out" => {
            attributes.out = Some(expect_identifier(value, "out")?);
        }
        "period_ms" => {
            attributes.period_ms = Some(expect_u64(value, "period_ms")?);
        }
        "limit" => {
            attributes.limit = Some(parse_range_value(value)?);
        }
        "master" => {
            attributes.master = Some(expect_identifier(value, "master")?);
        }
        "slave" => {
            attributes.slave = Some(expect_identifier(value, "slave")?);
        }
        "table" => {
            attributes.table = Some(expect_identifier(value, "table")?);
        }
        "interpolation" => {
            attributes.interpolation = Some(expect_identifier(value, "interpolation")?);
        }
        "gear_ratio" => {
            attributes.gear_ratio = Some(expect_number(value, "gear_ratio")?);
        }
        "phase_offset" => {
            attributes.phase_offset = Some(expect_number(value, "phase_offset")?);
        }
        "following_error_limit" => {
            attributes.following_error_limit = Some(expect_number(value, "following_error_limit")?);
        }
        "slave_feedback" => {
            attributes.slave_feedback = Some(expect_identifier(value, "slave_feedback")?);
        }
        "steps_per_rev"
        | "max_speed"
        | "accel_time"
        | "decel_time"
        | "encoder_resolution"
        | "electronic_gear_num"
        | "electronic_gear_den"
        | "positioning_window"
        | "rated_power"
        | "rated_freq"
        | "microstep"
        | "gear_num"
        | "gear_den"
        | "lead_screw"
        | "position_unit"
        | "max_acceleration" => {
            attributes
                .extra_params
                .insert(attr_name.to_string(), value.as_str().to_string());
        }
        _ => {
            return Err(PlcError::parse(
                line,
                format!("不支持的属性名: {attr_name}"),
            ));
        }
    }

    Ok(())
}

fn legacy_topology_attribute_error(line: usize, col: usize, attr_name: &str) -> PlcError {
    PlcError::parse_at(
        "<input>",
        line,
        col,
        format!(
            "属性 {attr_name} 已废弃，请改用 relation {{ from: Device.Port, to: Device.Port, via: {attr_name} }}"
        ),
    )
}

fn parse_constraints_section(pair: Pair<Rule>) -> Result<ConstraintsSection, PlcError> {
    let mut safety = Vec::new();
    let mut claims = Vec::new();
    let mut timing = Vec::new();
    let mut causality = Vec::new();

    for item in pair.into_inner() {
        if item.as_rule() != Rule::constraint_declaration {
            continue;
        }

        let line = line_of(&item);
        let constraint = first_inner(item, line, "约束声明")?;
        match constraint.as_rule() {
            Rule::safety_constraint => safety.push(parse_safety_constraint(constraint)?),
            Rule::resource_claim_constraint => {
                claims.push(parse_resource_claim_constraint(constraint)?)
            }
            Rule::timing_constraint => timing.push(parse_timing_constraint(constraint)?),
            Rule::causality_constraint => causality.push(parse_causality_constraint(constraint)?),
            _ => {}
        }
    }

    Ok(ConstraintsSection {
        safety,
        claims,
        timing,
        causality,
    })
}

fn parse_safety_constraint(pair: Pair<Rule>) -> Result<SafetyConstraint, PlcError> {
    let line = line_of(&pair);
    let mut left = None;
    let mut relation = None;
    let mut right = None;
    let mut reason = None;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::safety_operand if left.is_none() => left = Some(parse_safety_operand(part)?),
            Rule::safety_relation => relation = Some(parse_safety_relation(part)?),
            Rule::safety_operand => right = Some(parse_safety_operand(part)?),
            Rule::reason_clause => reason = Some(parse_reason_clause(part)?),
            _ => {}
        }
    }

    Ok(SafetyConstraint {
        line,
        left: left.ok_or_else(|| PlcError::parse(line, "safety 约束缺少左侧操作数"))?,
        relation: relation.ok_or_else(|| PlcError::parse(line, "safety 约束缺少关系符"))?,
        right: right.ok_or_else(|| PlcError::parse(line, "safety 约束缺少右侧操作数"))?,
        reason,
        source: None,
    })
}

fn parse_resource_claim_constraint(pair: Pair<Rule>) -> Result<ResourceClaimConstraint, PlcError> {
    let line = line_of(&pair);
    let mut source = None;
    let mut resource = None;
    let mut reason = None;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::resource_claim_source => {
                source = Some(parse_resource_claim_source(part)?);
            }
            Rule::identifier => {
                if resource.is_none() {
                    resource = Some(part.as_str().to_string());
                }
            }
            Rule::reason_clause => {
                reason = Some(parse_reason_clause(part)?);
            }
            _ => {}
        }
    }

    Ok(ResourceClaimConstraint {
        line,
        source: source.ok_or_else(|| PlcError::parse(line, "claim 缺少来源"))?,
        resource: resource.ok_or_else(|| PlcError::parse(line, "claim 缺少 resource"))?,
        reason,
    })
}

fn parse_resource_claim_source(pair: Pair<Rule>) -> Result<ResourceClaimSource, PlcError> {
    let line = line_of(&pair);
    let mut inner = pair.into_inner();
    let first = inner
        .next()
        .ok_or_else(|| PlcError::parse(line, "claim source 缺少内容"))?;
    match first.as_rule() {
        Rule::state_reference => Ok(ResourceClaimSource::State(parse_state_reference(first)?)),
        Rule::identifier => Ok(ResourceClaimSource::ActionTag {
            tag: first.as_str().to_string(),
        }),
        _ => Err(PlcError::parse(
            line,
            format!("不支持的 claim source: {:?}", first.as_rule()),
        )),
    }
}

fn parse_safety_operand(pair: Pair<Rule>) -> Result<SafetyOperand, PlcError> {
    let line = line_of(&pair);
    let inner = first_inner(pair, line, "safety 操作数")?;
    match inner.as_rule() {
        Rule::analog_condition => {
            let mut device = None;
            let mut operator = None;
            let mut value = None;
            let mut unit = None;
            for part in inner.into_inner() {
                match part.as_rule() {
                    Rule::identifier | Rule::state_reference => {
                        device = Some(part.as_str().to_string())
                    }
                    Rule::comparison_operator => operator = Some(parse_comparison_operator(part)?),
                    Rule::number => {
                        value =
                            Some(part.as_str().parse::<f64>().map_err(|_| {
                                PlcError::parse(line, "analog_condition 数值解析失败")
                            })?);
                    }
                    Rule::measured_value => {
                        let measured = parse_measured_value(part)?;
                        value = Some(measured.value);
                        unit = Some(measured.unit);
                    }
                    _ => {}
                }
            }
            Ok(SafetyOperand::Threshold {
                device: device
                    .ok_or_else(|| PlcError::parse(line, "analog_condition 缺少设备名"))?,
                operator: operator
                    .ok_or_else(|| PlcError::parse(line, "analog_condition 缺少比较符"))?,
                value: value.ok_or_else(|| PlcError::parse(line, "analog_condition 缺少阈值"))?,
                unit,
            })
        }
        Rule::state_reference => Ok(SafetyOperand::State(parse_state_reference(inner)?)),
        rule => Err(PlcError::parse(
            line,
            format!("不支持的 safety 操作数类型: {rule:?}"),
        )),
    }
}

fn parse_safety_relation(pair: Pair<Rule>) -> Result<SafetyRelation, PlcError> {
    let line = line_of(&pair);
    match pair.as_str() {
        "conflicts_with" => Ok(SafetyRelation::ConflictsWith),
        "requires" => Ok(SafetyRelation::Requires),
        other => Err(PlcError::parse(line, format!("未知 safety 关系: {other}"))),
    }
}

fn parse_timing_constraint(pair: Pair<Rule>) -> Result<TimingConstraint, PlcError> {
    let line = line_of(&pair);
    let mut target = None;
    let mut relation = None;
    let mut duration = None;
    let mut reason = None;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::timing_scope => target = Some(parse_timing_scope(part)?),
            Rule::timing_relation => relation = Some(parse_timing_relation(part)?),
            Rule::duration_value => duration = Some(parse_duration_value(part)?),
            Rule::reason_clause => reason = Some(parse_reason_clause(part)?),
            _ => {}
        }
    }

    Ok(TimingConstraint {
        line,
        target: target.ok_or_else(|| PlcError::parse(line, "timing 约束缺少作用域"))?,
        relation: relation.ok_or_else(|| PlcError::parse(line, "timing 约束缺少关系符"))?,
        duration: duration.ok_or_else(|| PlcError::parse(line, "timing 约束缺少时长"))?,
        reason,
    })
}

fn parse_timing_scope(pair: Pair<Rule>) -> Result<TimingTarget, PlcError> {
    let line = line_of(&pair);
    let identifiers: Vec<String> = pair
        .into_inner()
        .filter(|item| item.as_rule() == Rule::identifier)
        .map(|item| item.as_str().to_string())
        .collect();

    match identifiers.as_slice() {
        [task] => Ok(TimingTarget::Task { task: task.clone() }),
        [task, step] => Ok(TimingTarget::Step {
            task: task.clone(),
            step: step.clone(),
        }),
        _ => Err(PlcError::parse(
            line,
            "timing 作用域格式错误，应为 task.<name> 或 task.<name>.<step>",
        )),
    }
}

fn parse_timing_relation(pair: Pair<Rule>) -> Result<TimingRelation, PlcError> {
    let line = line_of(&pair);
    match pair.as_str() {
        "must_complete_within" => Ok(TimingRelation::MustCompleteWithin),
        "must_complete_within_worst_case" => Ok(TimingRelation::MustCompleteWithinWorstCase),
        "must_start_after" => Ok(TimingRelation::MustStartAfter),
        other => Err(PlcError::parse(line, format!("未知 timing 关系: {other}"))),
    }
}

fn parse_causality_constraint(pair: Pair<Rule>) -> Result<CausalityConstraint, PlcError> {
    let line = line_of(&pair);
    let mut chain = None;
    let mut reason = None;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::causality_chain => {
                let nodes: Vec<StateReference> = part
                    .into_inner()
                    .filter(|item| item.as_rule() == Rule::identifier)
                    .map(|item| StateReference {
                        device: item.as_str().to_string(),
                        port: String::new(),
                        // Causality declarations are device-level chains, so state is intentionally empty.
                        state: String::new(),
                    })
                    .collect();
                chain = Some(nodes);
            }
            Rule::reason_clause => reason = Some(parse_reason_clause(part)?),
            _ => {}
        }
    }

    let chain = chain.ok_or_else(|| PlcError::parse(line, "causality 约束缺少链路"))?;
    if chain.len() < 2 {
        return Err(PlcError::parse(line, "causality 链路至少需要两个设备节点"));
    }

    Ok(CausalityConstraint {
        line,
        chain,
        reason,
    })
}

fn parse_reason_clause(pair: Pair<Rule>) -> Result<String, PlcError> {
    let line = line_of(&pair);
    let value = pair
        .into_inner()
        .next()
        .ok_or_else(|| PlcError::parse(line, "reason 缺少字符串值"))?;
    parse_string_literal(value)
}

fn parse_tasks_section(pair: Pair<Rule>) -> Result<TasksSection, PlcError> {
    let mut tasks = Vec::new();

    for item in pair.into_inner() {
        if item.as_rule() == Rule::task_declaration {
            tasks.push(parse_task_declaration(item)?);
        }
    }

    Ok(TasksSection { tasks })
}

fn parse_task_declaration(pair: Pair<Rule>) -> Result<TaskDeclaration, PlcError> {
    let line = line_of(&pair);
    let mut name = None;
    let mut steps = Vec::new();
    let mut on_complete_line = None;
    let mut on_complete = None;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::identifier => name = Some(part.as_str().to_string()),
            Rule::step_declaration => steps.push(parse_step_declaration(part)?),
            Rule::on_complete_statement => {
                on_complete_line = Some(line_of(&part));
                on_complete = Some(parse_on_complete_statement(part)?);
            }
            _ => {}
        }
    }

    Ok(TaskDeclaration {
        line,
        name: name.ok_or_else(|| PlcError::parse(line, "task 声明缺少名称"))?,
        steps,
        on_complete_line,
        on_complete,
    })
}

fn parse_step_declaration(pair: Pair<Rule>) -> Result<StepDeclaration, PlcError> {
    let line = line_of(&pair);
    let mut name = None;
    let mut statements = Vec::new();

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::identifier => name = Some(part.as_str().to_string()),
            Rule::step_statement => statements.push(parse_step_statement_wrapper(part)?),
            _ => {}
        }
    }

    Ok(StepDeclaration {
        line,
        name: name.ok_or_else(|| PlcError::parse(line, "step 声明缺少名称"))?,
        statements,
    })
}

fn parse_step_statement_wrapper(pair: Pair<Rule>) -> Result<StepStatement, PlcError> {
    let line = line_of(&pair);
    let statement = first_inner(pair, line, "step 语句")?;
    parse_step_statement(statement)
}

fn parse_step_statement(pair: Pair<Rule>) -> Result<StepStatement, PlcError> {
    match pair.as_rule() {
        Rule::axis_move_statement => Ok(StepStatement::Action(parse_axis_move_statement(pair)?)),
        Rule::cylinder_motion_statement => Ok(StepStatement::Action(
            parse_cylinder_motion_statement(pair)?,
        )),
        Rule::action_statement => Ok(StepStatement::Action(parse_action_statement(pair)?)),
        Rule::effect_statement => Ok(StepStatement::Effect(parse_effect_statement_v2(pair)?)),
        Rule::wait_statement => Ok(StepStatement::Wait(parse_wait_statement(pair)?)),
        Rule::if_else_statement => Ok(parse_if_else_statement(pair)?),
        Rule::delay_statement => Ok(StepStatement::Delay {
            duration_ms: parse_delay_statement(pair)?,
        }),
        Rule::repeat_block => {
            let (count, body) = parse_repeat_block(pair)?;
            Ok(StepStatement::Repeat { count, body })
        }
        Rule::timeout_statement => Ok(StepStatement::Timeout(parse_timeout_statement(pair)?)),
        Rule::goto_statement => Ok(StepStatement::Goto(parse_goto_statement(pair)?)),
        Rule::parallel_statement => Ok(StepStatement::Parallel(parse_parallel_block(pair)?)),
        Rule::race_statement => Ok(StepStatement::Race(parse_race_block(pair)?)),
        Rule::allow_indefinite_wait_statement => Ok(StepStatement::AllowIndefiniteWait(
            parse_allow_indefinite_wait(pair)?,
        )),
        rule => Err(PlcError::parse(
            line_of(&pair),
            format!("不支持的 step 语句: {rule:?}"),
        )),
    }
}

#[allow(dead_code)]
fn parse_effect_statement(pair: Pair<Rule>) -> Result<EffectStatement, PlcError> {
    let line = line_of(&pair);
    let command_wrapper = first_inner(pair, line, "effect 语句")?;
    let command = first_inner(command_wrapper, line, "effect 具体语义")?;

    let kind = match command.as_rule() {
        Rule::effect_acquire => {
            let parts = command.as_str().split_whitespace().collect::<Vec<_>>();
            let holder = parts
                .get(2)
                .ok_or_else(|| PlcError::parse(line, "acquire 缺少 holder"))?
                .to_string();
            let from = parts
                .get(4)
                .ok_or_else(|| PlcError::parse(line, "acquire 缺少来源"))?
                .to_string();
            EffectKind::Acquire { holder, from }
        }
        Rule::effect_transfer => {
            let parts = command.as_str().split_whitespace().collect::<Vec<_>>();
            let from = parts
                .get(2)
                .ok_or_else(|| PlcError::parse(line, "transfer 缺少 from"))?
                .to_string();
            let to = parts
                .get(4)
                .ok_or_else(|| PlcError::parse(line, "transfer 缺少 to"))?
                .to_string();
            EffectKind::Transfer { from, to }
        }
        Rule::effect_finish => {
            let parts = command.as_str().split_whitespace().collect::<Vec<_>>();
            let at = parts
                .get(3)
                .ok_or_else(|| PlcError::parse(line, "finish 缺少 site"))?
                .to_string();
            let terminal_state = parts
                .get(5)
                .ok_or_else(|| PlcError::parse(line, "finish 缺少 terminal state"))?
                .to_string();
            EffectKind::Finish { at, terminal_state }
        }
        rule => {
            return Err(PlcError::parse(
                line,
                format!("不支持的 effect 语句: {rule:?}"),
            ));
        }
    };

    Ok(EffectStatement { line, kind })
}

fn parse_effect_statement_v2(pair: Pair<Rule>) -> Result<EffectStatement, PlcError> {
    let line = line_of(&pair);
    let command_wrapper = first_inner(pair, line, "effect")?;
    let command = first_inner(command_wrapper, line, "effect command")?;

    let kind = match command.as_rule() {
        Rule::effect_acquire => {
            let mut inner = command.into_inner();
            let holder = inner
                .next()
                .ok_or_else(|| PlcError::parse(line, "acquire holder missing"))?
                .as_str()
                .to_string();
            let from = inner
                .next()
                .ok_or_else(|| PlcError::parse(line, "acquire source missing"))?
                .as_str()
                .to_string();
            EffectKind::Acquire { holder, from }
        }
        Rule::effect_transfer => {
            let mut inner = command.into_inner();
            let from = inner
                .next()
                .ok_or_else(|| PlcError::parse(line, "transfer source missing"))?
                .as_str()
                .to_string();
            let to = inner
                .next()
                .ok_or_else(|| PlcError::parse(line, "transfer target missing"))?
                .as_str()
                .to_string();
            EffectKind::Transfer { from, to }
        }
        Rule::effect_finish => {
            let mut inner = command.into_inner();
            let at = inner
                .next()
                .ok_or_else(|| PlcError::parse(line, "finish site missing"))?
                .as_str()
                .to_string();
            let terminal_state = inner
                .next()
                .ok_or_else(|| PlcError::parse(line, "finish terminal state missing"))?
                .as_str()
                .to_string();
            EffectKind::Finish { at, terminal_state }
        }
        Rule::effect_mount => {
            let mut inner = command.into_inner();
            let workpiece_type = inner
                .next()
                .ok_or_else(|| PlcError::parse(line, "mount workpiece type missing"))?
                .as_str()
                .to_string();
            let slot = inner
                .next()
                .ok_or_else(|| PlcError::parse(line, "mount slot missing"))?
                .as_str()
                .to_string();
            EffectKind::Mount {
                workpiece_type,
                slot,
            }
        }
        Rule::effect_unmount => {
            let mut inner = command.into_inner();
            let workpiece_type = inner
                .next()
                .ok_or_else(|| PlcError::parse(line, "unmount workpiece type missing"))?
                .as_str()
                .to_string();
            let slot = inner
                .next()
                .ok_or_else(|| PlcError::parse(line, "unmount slot missing"))?
                .as_str()
                .to_string();
            let to = inner
                .next()
                .ok_or_else(|| PlcError::parse(line, "unmount target missing"))?
                .as_str()
                .to_string();
            EffectKind::Unmount {
                workpiece_type,
                slot,
                to,
            }
        }
        Rule::effect_split => {
            let mut inner = command.into_inner();
            let source_type = inner
                .next()
                .ok_or_else(|| PlcError::parse(line, "split source type missing"))?
                .as_str()
                .to_string();
            let target_type = inner
                .next()
                .ok_or_else(|| PlcError::parse(line, "split target type missing"))?
                .as_str()
                .to_string();
            let count = inner
                .next()
                .ok_or_else(|| PlcError::parse(line, "split count missing"))?
                .as_str()
                .parse::<u32>()
                .map_err(|_| PlcError::parse(line, "split count must be an unsigned integer"))?;
            EffectKind::Split {
                source_type,
                target_type,
                count,
                consumed: true,
            }
        }
        Rule::effect_merge => {
            let mut inner = command.into_inner();
            let inputs = inner
                .next()
                .ok_or_else(|| PlcError::parse(line, "merge inputs missing"))?
                .into_inner()
                .filter(|part| part.as_rule() == Rule::identifier)
                .map(|part| part.as_str().to_string())
                .collect::<Vec<_>>();
            let target_type = inner
                .next()
                .ok_or_else(|| PlcError::parse(line, "merge target type missing"))?
                .as_str()
                .to_string();
            EffectKind::Merge {
                inputs,
                target_type,
                consumed_inputs: true,
            }
        }
        Rule::effect_transform_carrier => {
            let mut inner = command.into_inner();
            let carrier = inner
                .next()
                .ok_or_else(|| PlcError::parse(line, "transform carrier missing"))?
                .as_str()
                .to_string();
            let frame = inner
                .next()
                .ok_or_else(|| PlcError::parse(line, "transform frame missing"))?
                .as_str()
                .to_string();
            EffectKind::TransformCarrier { carrier, frame }
        }
        rule => {
            return Err(PlcError::parse(
                line,
                format!("unsupported effect statement: {rule:?}"),
            ));
        }
    };

    Ok(EffectStatement { line, kind })
}

fn parse_if_else_statement(pair: Pair<Rule>) -> Result<StepStatement, PlcError> {
    let line = line_of(&pair);
    let mut condition = None;
    let mut then_goto = None;
    let mut else_goto = None;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::simple_condition => condition = Some(parse_simple_condition(part)?),
            Rule::goto_statement if then_goto.is_none() => {
                then_goto = Some(parse_goto_statement(part)?)
            }
            Rule::goto_statement => else_goto = Some(parse_goto_statement(part)?),
            _ => {}
        }
    }

    Ok(StepStatement::IfElse {
        condition: condition.ok_or_else(|| PlcError::parse(line, "if 缺少条件表达式"))?,
        then_goto: then_goto.ok_or_else(|| PlcError::parse(line, "if 缺少 goto 分支"))?,
        else_goto: else_goto.ok_or_else(|| PlcError::parse(line, "else 缺少 goto 分支"))?,
    })
}

fn parse_repeat_block(pair: Pair<Rule>) -> Result<(u64, Vec<StepStatement>), PlcError> {
    let line = line_of(&pair);
    let mut count = None;
    let mut body = Vec::new();

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::integer => {
                let parsed = part
                    .as_str()
                    .parse::<u64>()
                    .map_err(|_| PlcError::parse(line, "repeat 次数必须是非负整数"))?;
                count = Some(parsed);
            }
            Rule::step_statement => body.push(parse_step_statement_wrapper(part)?),
            _ => {}
        }
    }

    let count = count.ok_or_else(|| PlcError::parse(line, "repeat 缺少次数"))?;
    if body.is_empty() {
        return Err(PlcError::parse(line, "repeat 块至少需要一条语句"));
    }

    Ok((count, body))
}

fn parse_delay_statement(pair: Pair<Rule>) -> Result<u64, PlcError> {
    let line = line_of(&pair);
    let duration_pair = pair
        .into_inner()
        .next()
        .ok_or_else(|| PlcError::parse(line, "delay 缺少时长"))?;
    let duration = parse_duration_value(duration_pair)?;

    Ok(duration_value_to_ms(&duration))
}

fn parse_action_statement(pair: Pair<Rule>) -> Result<ActionStatement, PlcError> {
    let line = line_of(&pair);
    let action_command = pair
        .into_inner()
        .next()
        .ok_or_else(|| PlcError::parse(line, "action 缺少具体命令"))?;
    let action = first_inner(action_command, line, "action 命令")?;

    match action.as_rule() {
        Rule::action_extend => {
            let target_pair = action
                .into_inner()
                .next()
                .ok_or_else(|| PlcError::parse(line, "extend 缺少目标设备"))?;
            Ok(ActionStatement::Extend {
                target: parse_action_target(target_pair)?,
                timeout: None,
                on_motion_fault: None,
                on_safety_fault: None,
            })
        }
        Rule::action_retract => {
            let target_pair = action
                .into_inner()
                .next()
                .ok_or_else(|| PlcError::parse(line, "retract 缺少目标设备"))?;
            Ok(ActionStatement::Retract {
                target: parse_action_target(target_pair)?,
                timeout: None,
                on_motion_fault: None,
                on_safety_fault: None,
            })
        }
        Rule::action_set_analog => {
            let mut parts = action.into_inner();
            let target_pair = parts
                .next()
                .ok_or_else(|| PlcError::parse(line, "set_analog 缺少目标设备"))?;
            let target = parse_action_target(target_pair)?;
            let value_pair = parts
                .next()
                .ok_or_else(|| PlcError::parse(line, "set_analog 缺少数值"))?;
            let value = value_pair
                .as_str()
                .parse::<f64>()
                .map_err(|_| PlcError::parse(line, "set_analog 数值解析失败"))?;
            Ok(ActionStatement::SetAnalog { target, value })
        }
        Rule::action_set_analog_expr => {
            let mut parts = action.into_inner();
            let target_pair = parts
                .next()
                .ok_or_else(|| PlcError::parse(line, "set_analog 缺少目标设备"))?;
            let target = parse_action_target(target_pair)?;
            let expr_pair = parts
                .next()
                .ok_or_else(|| PlcError::parse(line, "set_analog 缺少表达式"))?;
            let expr = parse_expression(expr_pair)?;
            Ok(ActionStatement::SetAnalogExpr { target, expr })
        }
        Rule::compute_statement => {
            let mut parts = action.into_inner();
            let target = parts
                .next()
                .ok_or_else(|| PlcError::parse(line, "compute 缺少目标变量"))?
                .as_str()
                .to_string();
            let expr_pair = parts
                .next()
                .ok_or_else(|| PlcError::parse(line, "compute 缺少表达式"))?;
            let expr = parse_expression(expr_pair)?;
            Ok(ActionStatement::Compute { target, expr })
        }
        Rule::action_call_extern => {
            let mut parts = action.into_inner();
            let function = parts
                .next()
                .ok_or_else(|| PlcError::parse(line, "call 缺少函数名"))?
                .as_str()
                .to_string();
            let mut args = Vec::new();
            let mut binding = None;

            for part in parts {
                match part.as_rule() {
                    Rule::extern_call_args => {
                        for item in part.into_inner() {
                            if item.as_rule() == Rule::expression {
                                args.push(parse_expression(item)?);
                            }
                        }
                    }
                    Rule::extern_call_binding => {
                        binding = Some(parse_extern_call_binding(part)?);
                    }
                    _ => {}
                }
            }

            Ok(ActionStatement::Call {
                function,
                args,
                binding: binding.ok_or_else(|| PlcError::parse(line, "call 缺少返回绑定"))?,
            })
        }
        Rule::action_cam_engage => {
            let target = action
                .into_inner()
                .next()
                .ok_or_else(|| PlcError::parse(line, "cam_engage 缺少目标设备"))?
                .as_str()
                .to_string();
            Ok(ActionStatement::CamEngage { target })
        }
        Rule::action_cam_disengage => {
            let target = action
                .into_inner()
                .next()
                .ok_or_else(|| PlcError::parse(line, "cam_disengage 缺少目标设备"))?
                .as_str()
                .to_string();
            Ok(ActionStatement::CamDisengage { target })
        }
        Rule::action_cam_switch => {
            let mut parts = action.into_inner();
            let target = parts
                .next()
                .ok_or_else(|| PlcError::parse(line, "cam_switch 缺少目标设备"))?
                .as_str()
                .to_string();
            let new_table = parts
                .next()
                .ok_or_else(|| PlcError::parse(line, "cam_switch 缺少新表名"))?
                .as_str()
                .to_string();
            Ok(ActionStatement::CamSwitch { target, new_table })
        }
        Rule::action_cam_phase => {
            let mut parts = action.into_inner();
            let target = parts
                .next()
                .ok_or_else(|| PlcError::parse(line, "cam_phase 缺少目标设备"))?
                .as_str()
                .to_string();
            let expr_pair = parts
                .next()
                .ok_or_else(|| PlcError::parse(line, "cam_phase 缺少偏移表达式"))?;
            let offset = parse_expression(expr_pair)?;
            Ok(ActionStatement::CamPhase { target, offset })
        }
        Rule::action_set => {
            let mut parts = action.into_inner();
            let target_pair = parts
                .next()
                .ok_or_else(|| PlcError::parse(line, "set 缺少目标设备"))?;
            let target = parse_action_target(target_pair)?;
            let value_pair = parts
                .next()
                .ok_or_else(|| PlcError::parse(line, "set 缺少状态值"))?;
            let value = parse_state_value(value_pair)?;
            Ok(ActionStatement::Set { target, value })
        }
        Rule::action_log => {
            let message_pair = action
                .into_inner()
                .next()
                .ok_or_else(|| PlcError::parse(line, "log 缺少消息字符串"))?;
            let message = parse_string_literal(message_pair)?;
            Ok(ActionStatement::Log { message })
        }
        rule => Err(PlcError::parse(
            line,
            format!("不支持的 action 命令: {rule:?}"),
        )),
    }
}

fn parse_cylinder_motion_statement(pair: Pair<Rule>) -> Result<ActionStatement, PlcError> {
    let line = line_of(&pair);
    let mut target = None::<ActionTarget>;
    let mut timeout = None::<TimeoutDirective>;
    let mut on_motion_fault = None::<GotoDirective>;
    let mut on_safety_fault = None::<GotoDirective>;
    let mut is_extend = None::<bool>;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::action_extend => {
                let target_pair = part
                    .into_inner()
                    .next()
                    .ok_or_else(|| PlcError::parse(line, "extend 缂哄皯鐩爣璁惧"))?;
                target = Some(parse_action_target(target_pair)?);
                is_extend = Some(true);
            }
            Rule::action_retract => {
                let target_pair = part
                    .into_inner()
                    .next()
                    .ok_or_else(|| PlcError::parse(line, "retract 缂哄皯鐩爣璁惧"))?;
                target = Some(parse_action_target(target_pair)?);
                is_extend = Some(false);
            }
            Rule::timeout_statement => {
                if timeout.is_some() {
                    return Err(PlcError::parse(line, "cylinder motion timeout 閲嶅澹版槑"));
                }
                timeout = Some(parse_timeout_statement(part)?);
            }
            Rule::cylinder_on_motion_fault_branch => {
                let target_pair = first_inner(part, line, "cylinder on_motion_fault target")?;
                if on_motion_fault.is_some() {
                    return Err(PlcError::parse(line, "on_motion_fault 主桶分支重复声明"));
                }
                on_motion_fault = Some(parse_axis_branch_target(target_pair, "on_motion_fault")?);
            }
            Rule::cylinder_on_safety_fault_branch => {
                let target_pair = first_inner(part, line, "cylinder on_safety_fault target")?;
                if on_safety_fault.is_some() {
                    return Err(PlcError::parse(line, "on_safety_fault 主桶分支重复声明"));
                }
                on_safety_fault = Some(parse_axis_branch_target(target_pair, "on_safety_fault")?);
            }
            _ => {}
        }
    }

    let target = target.ok_or_else(|| PlcError::parse(line, "cylinder motion 缂哄皯鐩爣璁惧"))?;
    match is_extend {
        Some(true) => Ok(ActionStatement::Extend {
            target,
            timeout,
            on_motion_fault,
            on_safety_fault,
        }),
        Some(false) => Ok(ActionStatement::Retract {
            target,
            timeout,
            on_motion_fault,
            on_safety_fault,
        }),
        None => Err(PlcError::parse(
            line,
            "cylinder motion 璇彞蹇呴』鏄 extend 鎴 retract",
        )),
    }
}

fn parse_axis_move_statement(pair: Pair<Rule>) -> Result<ActionStatement, PlcError> {
    let line = line_of(&pair);
    let mut target = None::<ActionTarget>;
    let mut distance = None::<f64>;
    let mut position = None::<f64>;
    let mut speed = None::<f64>;
    let mut acceleration = None::<f64>;
    let mut deceleration = None::<f64>;
    let mut params = None::<String>;
    let mut timeout = None::<TimeoutDirective>;
    let mut on_reject = None::<GotoDirective>;
    let mut on_motion_fault = None::<GotoDirective>;
    let mut on_safety_fault = None::<GotoDirective>;
    let mut on_reject_routes = Vec::<AxisFaultRouteDirective>::new();
    let mut on_motion_fault_routes = Vec::<AxisFaultRouteDirective>::new();
    let mut on_safety_fault_routes = Vec::<AxisFaultRouteDirective>::new();
    let mut semantic_tag = None::<String>;
    let mut is_relative = None::<bool>;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::axis_move_relative_action => {
                let parsed = parse_axis_move_relative_action(part)?;
                target = Some(parsed.target);
                distance = Some(parsed.distance);
                speed = parsed.speed;
                acceleration = parsed.acceleration;
                deceleration = parsed.deceleration;
                params = parsed.params;
                is_relative = Some(true);
            }
            Rule::axis_move_absolute_action => {
                let parsed = parse_axis_move_absolute_action(part)?;
                target = Some(parsed.target);
                position = Some(parsed.position);
                speed = parsed.speed;
                acceleration = parsed.acceleration;
                deceleration = parsed.deceleration;
                params = parsed.params;
                is_relative = Some(false);
            }
            Rule::axis_timeout_branch => {
                timeout = Some(parse_axis_timeout_branch(part)?);
            }
            Rule::axis_on_reject_branch => {
                push_axis_fault_branch(
                    parse_axis_fault_branch(part, "on_reject")?,
                    line,
                    "on_reject",
                    &mut on_reject,
                    &mut on_reject_routes,
                )?;
            }
            Rule::axis_on_motion_fault_branch => {
                push_axis_fault_branch(
                    parse_axis_fault_branch(part, "on_motion_fault")?,
                    line,
                    "on_motion_fault",
                    &mut on_motion_fault,
                    &mut on_motion_fault_routes,
                )?;
            }
            Rule::axis_on_safety_fault_branch => {
                push_axis_fault_branch(
                    parse_axis_fault_branch(part, "on_safety_fault")?,
                    line,
                    "on_safety_fault",
                    &mut on_safety_fault,
                    &mut on_safety_fault_routes,
                )?;
            }
            Rule::axis_semantic_tag_branch => {
                if semantic_tag.is_some() {
                    return Err(PlcError::parse(line, "semantic_tag 重复声明"));
                }
                semantic_tag = Some(parse_axis_semantic_tag_branch(part)?);
            }
            _ => {}
        }
    }

    let target = target.ok_or_else(|| PlcError::parse(line, "axis.move 缺少目标设备"))?;

    match is_relative {
        Some(true) => Ok(ActionStatement::AxisMoveRelative {
            target,
            params,
            distance: distance
                .ok_or_else(|| PlcError::parse(line, "axis.move_relative 缺少 distance 参数"))?,
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
            semantic_tag,
        }),
        Some(false) => Ok(ActionStatement::AxisMoveAbsolute {
            target,
            params,
            position: position
                .ok_or_else(|| PlcError::parse(line, "axis.move_absolute 缺少 position 参数"))?,
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
            semantic_tag,
        }),
        None => Err(PlcError::parse(
            line,
            "axis.move 语句必须是 axis.move_relative 或 axis.move_absolute",
        )),
    }
}

fn parse_axis_semantic_tag_branch(pair: Pair<Rule>) -> Result<String, PlcError> {
    let line = line_of(&pair);
    pair.into_inner()
        .find(|part| part.as_rule() == Rule::identifier)
        .map(|part| part.as_str().to_string())
        .ok_or_else(|| PlcError::parse(line, "semantic_tag 缺少标记名"))
}

fn push_axis_fault_branch(
    branch: ParsedAxisFaultBranch,
    line: usize,
    branch_name: &str,
    primary: &mut Option<GotoDirective>,
    routes: &mut Vec<AxisFaultRouteDirective>,
) -> Result<(), PlcError> {
    let ParsedAxisFaultBranch { target, kind, code } = branch;
    if kind.is_none() && code.is_none() {
        if primary.is_some() {
            return Err(PlcError::parse(
                line,
                format!("{branch_name} 主桶分支重复声明"),
            ));
        }
        *primary = Some(target);
        return Ok(());
    }

    routes.push(AxisFaultRouteDirective {
        line: target.line,
        kind,
        code,
        target,
    });
    Ok(())
}

#[derive(Debug)]
struct ParsedAxisMoveRelative {
    target: ActionTarget,
    distance: f64,
    speed: Option<f64>,
    acceleration: Option<f64>,
    deceleration: Option<f64>,
    params: Option<String>,
}

#[derive(Debug)]
struct ParsedAxisMoveAbsolute {
    target: ActionTarget,
    position: f64,
    speed: Option<f64>,
    acceleration: Option<f64>,
    deceleration: Option<f64>,
    params: Option<String>,
}

fn parse_axis_move_relative_action(pair: Pair<Rule>) -> Result<ParsedAxisMoveRelative, PlcError> {
    let line = line_of(&pair);
    let mut target = None;
    let mut distance = None::<f64>;
    let mut speed = None::<f64>;
    let mut acceleration = None::<f64>;
    let mut deceleration = None::<f64>;
    let mut params = None::<String>;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::action_target => target = Some(parse_action_target(part)?),
            Rule::axis_move_relative_arg => {
                let arg = first_inner(part, line, "axis.move_relative 参数")?;
                match arg.as_rule() {
                    Rule::axis_move_distance_arg => {
                        if distance.is_some() {
                            return Err(PlcError::parse(
                                line,
                                "axis.move_relative 参数 distance 重复声明",
                            ));
                        }
                        distance = Some(parse_axis_move_numeric_arg(arg, "distance")?);
                    }
                    Rule::axis_move_speed_arg => {
                        if speed.is_some() {
                            return Err(PlcError::parse(
                                line,
                                "axis.move_relative 参数 speed 重复声明",
                            ));
                        }
                        speed = Some(parse_axis_move_numeric_arg(arg, "speed")?);
                    }
                    Rule::axis_move_acc_arg => {
                        if acceleration.is_some() {
                            return Err(PlcError::parse(
                                line,
                                "axis.move_relative 参数 acc 重复声明",
                            ));
                        }
                        acceleration = Some(parse_axis_move_numeric_arg(arg, "acc")?);
                    }
                    Rule::axis_move_dec_arg => {
                        if deceleration.is_some() {
                            return Err(PlcError::parse(
                                line,
                                "axis.move_relative 参数 dec 重复声明",
                            ));
                        }
                        deceleration = Some(parse_axis_move_numeric_arg(arg, "dec")?);
                    }
                    Rule::axis_move_params_arg => {
                        if params.is_some() {
                            return Err(PlcError::parse(
                                line,
                                "axis.move_relative 参数 params 重复声明",
                            ));
                        }
                        params = Some(parse_axis_move_params_arg(arg)?);
                    }
                    Rule::axis_move_unknown_arg => {
                        return Err(axis_move_unknown_arg_error(arg, "axis.move_relative"));
                    }
                    _ => {}
                }
            }
            Rule::axis_move_distance_arg => {
                if distance.is_some() {
                    return Err(PlcError::parse(
                        line,
                        "axis.move_relative 参数 distance 重复声明",
                    ));
                }
                distance = Some(parse_axis_move_numeric_arg(part, "distance")?);
            }
            Rule::axis_move_speed_arg => {
                if speed.is_some() {
                    return Err(PlcError::parse(
                        line,
                        "axis.move_relative 参数 speed 重复声明",
                    ));
                }
                speed = Some(parse_axis_move_numeric_arg(part, "speed")?);
            }
            Rule::axis_move_acc_arg => {
                if acceleration.is_some() {
                    return Err(PlcError::parse(
                        line,
                        "axis.move_relative 参数 acc 重复声明",
                    ));
                }
                acceleration = Some(parse_axis_move_numeric_arg(part, "acc")?);
            }
            Rule::axis_move_dec_arg => {
                if deceleration.is_some() {
                    return Err(PlcError::parse(
                        line,
                        "axis.move_relative 参数 dec 重复声明",
                    ));
                }
                deceleration = Some(parse_axis_move_numeric_arg(part, "dec")?);
            }
            Rule::axis_move_params_arg => {
                if params.is_some() {
                    return Err(PlcError::parse(
                        line,
                        "axis.move_relative 参数 params 重复声明",
                    ));
                }
                params = Some(parse_axis_move_params_arg(part)?);
            }
            Rule::axis_move_unknown_arg => {
                return Err(axis_move_unknown_arg_error(part, "axis.move_relative"));
            }
            _ => {}
        }
    }

    Ok(ParsedAxisMoveRelative {
        target: target.ok_or_else(|| PlcError::parse(line, "axis.move_relative 缺少目标设备"))?,
        distance: distance
            .ok_or_else(|| PlcError::parse(line, "axis.move_relative 缺少 distance 参数"))?,
        speed,
        acceleration,
        deceleration,
        params,
    })
}

fn parse_axis_move_absolute_action(pair: Pair<Rule>) -> Result<ParsedAxisMoveAbsolute, PlcError> {
    let line = line_of(&pair);
    let mut target = None;
    let mut position = None::<f64>;
    let mut speed = None::<f64>;
    let mut acceleration = None::<f64>;
    let mut deceleration = None::<f64>;
    let mut params = None::<String>;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::action_target => target = Some(parse_action_target(part)?),
            Rule::axis_move_absolute_arg => {
                let arg = first_inner(part, line, "axis.move_absolute 参数")?;
                match arg.as_rule() {
                    Rule::axis_move_position_arg => {
                        if position.is_some() {
                            return Err(PlcError::parse(
                                line,
                                "axis.move_absolute 参数 position 重复声明",
                            ));
                        }
                        position = Some(parse_axis_move_numeric_arg(arg, "position")?);
                    }
                    Rule::axis_move_speed_arg => {
                        if speed.is_some() {
                            return Err(PlcError::parse(
                                line,
                                "axis.move_absolute 参数 speed 重复声明",
                            ));
                        }
                        speed = Some(parse_axis_move_numeric_arg(arg, "speed")?);
                    }
                    Rule::axis_move_acc_arg => {
                        if acceleration.is_some() {
                            return Err(PlcError::parse(
                                line,
                                "axis.move_absolute 参数 acc 重复声明",
                            ));
                        }
                        acceleration = Some(parse_axis_move_numeric_arg(arg, "acc")?);
                    }
                    Rule::axis_move_dec_arg => {
                        if deceleration.is_some() {
                            return Err(PlcError::parse(
                                line,
                                "axis.move_absolute 参数 dec 重复声明",
                            ));
                        }
                        deceleration = Some(parse_axis_move_numeric_arg(arg, "dec")?);
                    }
                    Rule::axis_move_params_arg => {
                        if params.is_some() {
                            return Err(PlcError::parse(
                                line,
                                "axis.move_absolute 参数 params 重复声明",
                            ));
                        }
                        params = Some(parse_axis_move_params_arg(arg)?);
                    }
                    Rule::axis_move_unknown_arg => {
                        return Err(axis_move_unknown_arg_error(arg, "axis.move_absolute"));
                    }
                    _ => {}
                }
            }
            Rule::axis_move_position_arg => {
                if position.is_some() {
                    return Err(PlcError::parse(
                        line,
                        "axis.move_absolute 参数 position 重复声明",
                    ));
                }
                position = Some(parse_axis_move_numeric_arg(part, "position")?);
            }
            Rule::axis_move_speed_arg => {
                if speed.is_some() {
                    return Err(PlcError::parse(
                        line,
                        "axis.move_absolute 参数 speed 重复声明",
                    ));
                }
                speed = Some(parse_axis_move_numeric_arg(part, "speed")?);
            }
            Rule::axis_move_acc_arg => {
                if acceleration.is_some() {
                    return Err(PlcError::parse(
                        line,
                        "axis.move_absolute 参数 acc 重复声明",
                    ));
                }
                acceleration = Some(parse_axis_move_numeric_arg(part, "acc")?);
            }
            Rule::axis_move_dec_arg => {
                if deceleration.is_some() {
                    return Err(PlcError::parse(
                        line,
                        "axis.move_absolute 参数 dec 重复声明",
                    ));
                }
                deceleration = Some(parse_axis_move_numeric_arg(part, "dec")?);
            }
            Rule::axis_move_params_arg => {
                if params.is_some() {
                    return Err(PlcError::parse(
                        line,
                        "axis.move_absolute 参数 params 重复声明",
                    ));
                }
                params = Some(parse_axis_move_params_arg(part)?);
            }
            Rule::axis_move_unknown_arg => {
                return Err(axis_move_unknown_arg_error(part, "axis.move_absolute"));
            }
            _ => {}
        }
    }

    Ok(ParsedAxisMoveAbsolute {
        target: target.ok_or_else(|| PlcError::parse(line, "axis.move_absolute 缺少目标设备"))?,
        position: position
            .ok_or_else(|| PlcError::parse(line, "axis.move_absolute 缺少 position 参数"))?,
        speed,
        acceleration,
        deceleration,
        params,
    })
}

fn parse_axis_move_numeric_arg(pair: Pair<Rule>, field_name: &str) -> Result<f64, PlcError> {
    let line = line_of(&pair);
    let number = pair
        .into_inner()
        .find(|part| part.as_rule() == Rule::number)
        .ok_or_else(|| PlcError::parse(line, format!("axis.move 缺少 {field_name} 数值")))?;

    number
        .as_str()
        .parse::<f64>()
        .map_err(|_| PlcError::parse(line, format!("axis.move 参数 {field_name} 数值解析失败")))
}

fn axis_move_unknown_arg_error(pair: Pair<Rule>, action_name: &str) -> PlcError {
    let line = line_of(&pair);
    let field_name = pair
        .into_inner()
        .find(|part| part.as_rule() == Rule::identifier)
        .map(|part| part.as_str().to_string())
        .unwrap_or_else(|| "<unknown>".to_string());

    let whitelist = match action_name {
        "axis.move_relative" => "distance/speed/acc/dec/params",
        "axis.move_absolute" => "position/speed/acc/dec/params",
        _ => "",
    };

    PlcError::parse_with_reason(
        line,
        format!("[AXIS-013] {action_name} 参数字段 '{field_name}' 不在白名单中。"),
        format!("请仅使用 {action_name} 允许字段: {whitelist}；不支持别名（如 vel/jerk）。"),
    )
}

fn parse_axis_move_params_arg(pair: Pair<Rule>) -> Result<String, PlcError> {
    let line = line_of(&pair);
    let identifier = pair
        .into_inner()
        .find(|part| part.as_rule() == Rule::identifier)
        .ok_or_else(|| PlcError::parse(line, "axis.move params 缺少参数集名称"))?;
    Ok(identifier.as_str().to_string())
}

fn parse_axis_timeout_branch(pair: Pair<Rule>) -> Result<TimeoutDirective, PlcError> {
    let line = line_of(&pair);
    let mut duration = None;
    let mut target = None;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::duration_value => duration = Some(parse_duration_value(part)?),
            Rule::axis_branch_target => target = Some(parse_axis_branch_target(part, "timeout")?),
            _ => {}
        }
    }

    Ok(TimeoutDirective {
        duration: duration.ok_or_else(|| PlcError::parse(line, "axis timeout 缺少时长"))?,
        target: target.ok_or_else(|| PlcError::parse(line, "axis timeout 缺少跳转目标"))?,
    })
}

#[derive(Debug)]
struct ParsedAxisFaultBranch {
    target: GotoDirective,
    kind: Option<AxisFaultRouteKind>,
    code: Option<i32>,
}

fn parse_axis_fault_branch(
    pair: Pair<Rule>,
    branch_name: &str,
) -> Result<ParsedAxisFaultBranch, PlcError> {
    let line = line_of(&pair);
    let mut target = None::<GotoDirective>;
    let mut kind = None::<AxisFaultRouteKind>;
    let mut code = None::<i32>;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::axis_fault_route_matcher => {
                for matcher in part.into_inner() {
                    let matcher = if matcher.as_rule() == Rule::axis_fault_route_matcher_entry {
                        first_inner(matcher, line, "axis fault route matcher")?
                    } else {
                        matcher
                    };
                    match matcher.as_rule() {
                        Rule::axis_fault_route_kind_entry => {
                            if kind.is_some() {
                                return Err(PlcError::parse(
                                    line,
                                    format!("{branch_name} matcher kind 重复声明"),
                                ));
                            }
                            kind = Some(parse_axis_fault_route_kind_entry(matcher, branch_name)?);
                        }
                        Rule::axis_fault_route_code_entry => {
                            if code.is_some() {
                                return Err(PlcError::parse(
                                    line,
                                    format!("{branch_name} matcher code 重复声明"),
                                ));
                            }
                            code = Some(parse_axis_fault_route_code_entry(matcher, branch_name)?);
                        }
                        _ => {}
                    }
                }
            }
            Rule::axis_branch_target => {
                target = Some(parse_axis_branch_target(part, branch_name)?);
            }
            _ => {}
        }
    }

    Ok(ParsedAxisFaultBranch {
        target: target
            .ok_or_else(|| PlcError::parse(line, format!("{branch_name} 缺少跳转目标")))?,
        kind,
        code,
    })
}

fn parse_axis_fault_route_kind_entry(
    pair: Pair<Rule>,
    branch_name: &str,
) -> Result<AxisFaultRouteKind, PlcError> {
    let line = line_of(&pair);
    let value = pair
        .into_inner()
        .find(|part| part.as_rule() == Rule::axis_fault_route_kind)
        .ok_or_else(|| PlcError::parse(line, format!("{branch_name} matcher kind 缺少值")))?;

    match value.as_str() {
        "reject" => Ok(AxisFaultRouteKind::Reject),
        "motion" => Ok(AxisFaultRouteKind::Motion),
        "safety" => Ok(AxisFaultRouteKind::Safety),
        "vendor" => Ok(AxisFaultRouteKind::Vendor),
        other => Err(PlcError::parse(
            line,
            format!("{branch_name} matcher kind 不支持值: {other}"),
        )),
    }
}

fn parse_axis_fault_route_code_entry(pair: Pair<Rule>, branch_name: &str) -> Result<i32, PlcError> {
    let line = line_of(&pair);
    let raw = pair
        .into_inner()
        .find(|part| part.as_rule() == Rule::integer)
        .ok_or_else(|| PlcError::parse(line, format!("{branch_name} matcher code 缺少值")))?
        .as_str();

    raw.parse::<i32>().map_err(|_| {
        PlcError::parse(
            line,
            format!("{branch_name} matcher code 无法解析为 32 位整数: {raw}"),
        )
    })
}

fn parse_axis_branch_target(
    pair: Pair<Rule>,
    branch_name: &str,
) -> Result<GotoDirective, PlcError> {
    let line = line_of(&pair);
    let mut identifiers = pair
        .into_inner()
        .filter(|part| matches!(part.as_rule(), Rule::identifier));

    let task = identifiers
        .next()
        .ok_or_else(|| PlcError::parse(line, format!("{branch_name} 缺少目标 task")))?
        .as_str()
        .to_string();
    let step = identifiers.next().map(|part| part.as_str().to_string());

    Ok(GotoDirective { line, task, step })
}

fn parse_extern_call_binding(pair: Pair<Rule>) -> Result<ExternCallBinding, PlcError> {
    let line = line_of(&pair);
    let is_tuple = pair.as_str().trim_start().starts_with('(');
    let names: Vec<String> = pair
        .into_inner()
        .filter(|item| item.as_rule() == Rule::identifier)
        .map(|item| item.as_str().to_string())
        .collect();

    if names.is_empty() {
        return Err(PlcError::parse(line, "call 返回绑定至少需要一个变量名"));
    }

    if is_tuple {
        Ok(ExternCallBinding::Tuple(names))
    } else {
        Ok(ExternCallBinding::Single(
            names
                .into_iter()
                .next()
                .ok_or_else(|| PlcError::parse(line, "call 返回绑定缺少变量名"))?,
        ))
    }
}

fn parse_state_value(pair: Pair<Rule>) -> Result<String, PlcError> {
    let line = line_of(&pair);
    if pair.as_rule() != Rule::state_value {
        return Err(PlcError::parse(line, "set 语句的状态值格式错误"));
    }

    Ok(pair.as_str().to_string())
}

fn parse_wait_statement(pair: Pair<Rule>) -> Result<WaitStatement, PlcError> {
    let line = line_of(&pair);
    let condition_pair = pair
        .into_inner()
        .find(|part| part.as_rule() == Rule::wait_condition)
        .ok_or_else(|| PlcError::parse(line, "wait 缺少条件表达式"))?;

    let mut conditions = Vec::new();
    let mut relation = None::<&str>;

    for part in condition_pair.into_inner() {
        match part.as_rule() {
            Rule::simple_condition => conditions.push(parse_simple_condition(part)?),
            Rule::logical_operator => {
                let current = part.as_str();
                if let Some(existing) = relation {
                    if existing != current {
                        return Err(PlcError::parse(
                            line,
                            "wait 条件不支持混用 AND/OR，请统一使用 AND 或 OR",
                        ));
                    }
                } else {
                    relation = Some(current);
                }
            }
            _ => {}
        }
    }

    let condition = if conditions.is_empty() {
        return Err(PlcError::parse(line, "wait 缺少条件表达式"));
    } else if let Some(op) = relation {
        if op == "AND" {
            WaitCondition::And(conditions)
        } else {
            WaitCondition::Or(conditions)
        }
    } else {
        WaitCondition::Single(
            conditions
                .into_iter()
                .next()
                .ok_or_else(|| PlcError::parse(line, "wait 缺少条件表达式"))?,
        )
    };

    Ok(WaitStatement { condition })
}

fn parse_simple_condition(pair: Pair<Rule>) -> Result<ConditionExpression, PlcError> {
    let line = line_of(&pair);
    let mut legacy_operand = None;
    let mut legacy_value = None;
    let mut expr_left = None;
    let mut expr_right = None;
    let mut operator = None;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::condition_operand => {
                let inner = first_inner(part, line, "wait 左值")?;
                legacy_operand = Some(inner.as_str().to_string());
            }
            Rule::comparison_operator => operator = Some(parse_comparison_operator(part)?),
            Rule::condition_value => legacy_value = Some(parse_condition_value(part)?),
            Rule::condition_expr => {
                let mut inner = part.into_inner();
                let left_pair = inner
                    .next()
                    .ok_or_else(|| PlcError::parse(line, "wait 条件缺少左侧表达式"))?;
                let op_pair = inner
                    .next()
                    .ok_or_else(|| PlcError::parse(line, "wait 条件缺少比较符"))?;
                let right_pair = inner
                    .next()
                    .ok_or_else(|| PlcError::parse(line, "wait 条件缺少右侧表达式"))?;
                expr_left = Some(parse_expression(left_pair)?);
                operator = Some(parse_comparison_operator(op_pair)?);
                expr_right = Some(parse_expression(right_pair)?);
            }
            _ => {}
        }
    }

    if let (Some(left), Some(op), Some(right)) = (legacy_operand, operator.clone(), legacy_value) {
        return Ok(ConditionExpression::legacy(left, op, right));
    }

    if let (Some(left_expr), Some(op), Some(right_expr)) = (expr_left, operator, expr_right) {
        let left_raw = expression_to_raw(&left_expr);
        let right_raw = expression_to_raw(&right_expr);
        return Ok(ConditionExpression {
            left: left_raw,
            operator: op,
            right: LiteralValue::String(right_raw),
            left_expr: Some(left_expr),
            right_expr: Some(right_expr),
        });
    }

    Err(PlcError::parse(line, "wait 缺少完整条件表达式"))
}

fn parse_comparison_operator(pair: Pair<Rule>) -> Result<ComparisonOperator, PlcError> {
    let line = line_of(&pair);
    match pair.as_str() {
        "==" => Ok(ComparisonOperator::Eq),
        "!=" => Ok(ComparisonOperator::Neq),
        ">" => Ok(ComparisonOperator::Gt),
        "<" => Ok(ComparisonOperator::Lt),
        ">=" => Ok(ComparisonOperator::Gte),
        "<=" => Ok(ComparisonOperator::Lte),
        other => Err(PlcError::parse(line, format!("不支持的比较符: {other}"))),
    }
}

fn parse_condition_value(pair: Pair<Rule>) -> Result<LiteralValue, PlcError> {
    let line = line_of(&pair);
    let value = first_inner(pair, line, "wait 右值")?;

    match value.as_rule() {
        Rule::boolean_value => Ok(LiteralValue::Boolean(value.as_str() == "true")),
        Rule::measured_value => Ok(LiteralValue::Measured(parse_measured_value(value)?)),
        Rule::number => {
            let parsed = value
                .as_str()
                .parse::<f64>()
                .map_err(|_| PlcError::parse(line, "数字字面量解析失败"))?;
            Ok(LiteralValue::Number(parsed))
        }
        Rule::string_literal => Ok(LiteralValue::String(parse_string_literal(value)?)),
        Rule::state_reference => Ok(LiteralValue::State(parse_state_reference(value)?)),
        Rule::identifier => Ok(LiteralValue::String(value.as_str().to_string())),
        rule => Err(PlcError::parse(
            line,
            format!("不支持的 wait 右值类型: {rule:?}"),
        )),
    }
}

fn parse_expression(pair: Pair<Rule>) -> Result<Expression, PlcError> {
    let line = line_of(&pair);
    match pair.as_rule() {
        Rule::expression | Rule::expr_or | Rule::expr_and | Rule::expr_add | Rule::expr_mul => {
            let mut inner = pair.into_inner();
            let first = inner
                .next()
                .ok_or_else(|| PlcError::parse(line, "表达式为空"))?;
            let mut expr = parse_expression(first)?;
            while let Some(op) = inner.next() {
                let rhs_pair = inner
                    .next()
                    .ok_or_else(|| PlcError::parse(line, "表达式缺少右操作数"))?;
                let rhs = parse_expression(rhs_pair)?;
                expr = Expression::BinaryOp {
                    op: parse_binary_operator(op)?,
                    left: Box::new(expr),
                    right: Box::new(rhs),
                };
            }
            Ok(expr)
        }
        Rule::expr_cmp => {
            let mut inner = pair.into_inner();
            let first = inner
                .next()
                .ok_or_else(|| PlcError::parse(line, "比较表达式为空"))?;
            let mut expr = parse_expression(first)?;
            if let Some(op) = inner.next() {
                let rhs_pair = inner
                    .next()
                    .ok_or_else(|| PlcError::parse(line, "比较表达式缺少右操作数"))?;
                let rhs = parse_expression(rhs_pair)?;
                expr = Expression::BinaryOp {
                    op: parse_binary_operator(op)?,
                    left: Box::new(expr),
                    right: Box::new(rhs),
                };
            }
            Ok(expr)
        }
        Rule::expr_unary => {
            let raw = pair.as_str().trim_start();
            let mut inner = pair.into_inner();
            let mut not_count = 0usize;
            while let Some(next) = inner.peek() {
                if next.as_rule() == Rule::expr_not_op {
                    not_count += 1;
                    inner.next();
                } else {
                    break;
                }
            }
            let inner_pair = inner
                .next()
                .ok_or_else(|| PlcError::parse(line, "一元表达式为空"))?;
            let mut expr = parse_expression(inner_pair)?;
            for _ in 0..not_count {
                expr = Expression::UnaryNot(Box::new(expr));
            }
            if raw.starts_with('-') {
                Ok(Expression::UnaryNeg(Box::new(expr)))
            } else {
                Ok(expr)
            }
        }
        Rule::expr_atom => {
            let inner = pair
                .into_inner()
                .next()
                .ok_or_else(|| PlcError::parse(line, "表达式原子为空"))?;
            parse_expression(inner)
        }
        Rule::expr_func_call => parse_function_call_expression(pair),
        Rule::expr_literal => match pair.as_str() {
            "true" => Ok(Expression::Boolean(true)),
            "false" => Ok(Expression::Boolean(false)),
            raw => {
                let parsed = raw
                    .parse::<f64>()
                    .map_err(|_| PlcError::parse(line, "数字字面量解析失败"))?;
                Ok(Expression::Literal(parsed))
            }
        },
        Rule::expr_variable => Ok(Expression::Variable(pair.as_str().to_string())),
        rule => Err(PlcError::parse(
            line,
            format!("不支持的表达式节点: {rule:?}"),
        )),
    }
}

fn parse_function_call_expression(pair: Pair<Rule>) -> Result<Expression, PlcError> {
    let line = line_of(&pair);
    let mut inner = pair.into_inner();
    let name = inner
        .next()
        .ok_or_else(|| PlcError::parse(line, "函数调用缺少函数名"))?
        .as_str()
        .to_string();
    let mut args = Vec::new();
    for item in inner {
        if item.as_rule() == Rule::expression {
            args.push(parse_expression(item)?);
        }
    }
    Ok(Expression::FunctionCall { name, args })
}

fn parse_binary_operator(pair: Pair<Rule>) -> Result<BinaryOperator, PlcError> {
    let line = line_of(&pair);
    match pair.as_str() {
        "+" => Ok(BinaryOperator::Add),
        "-" => Ok(BinaryOperator::Sub),
        "*" => Ok(BinaryOperator::Mul),
        "/" => Ok(BinaryOperator::Div),
        "%" => Ok(BinaryOperator::Mod),
        "==" => Ok(BinaryOperator::Eq),
        "!=" => Ok(BinaryOperator::Neq),
        ">" => Ok(BinaryOperator::Gt),
        "<" => Ok(BinaryOperator::Lt),
        ">=" => Ok(BinaryOperator::Gte),
        "<=" => Ok(BinaryOperator::Lte),
        "AND" | "and" | "&&" => Ok(BinaryOperator::And),
        "OR" | "or" | "||" => Ok(BinaryOperator::Or),
        other => Err(PlcError::parse(
            line,
            format!("不支持的二元运算符: {other}"),
        )),
    }
}

fn expression_to_raw(expr: &Expression) -> String {
    match expr {
        Expression::Literal(value) => value.to_string(),
        Expression::Boolean(value) => value.to_string(),
        Expression::Variable(name) => name.clone(),
        Expression::UnaryNeg(inner) => format!("-({})", expression_to_raw(inner)),
        Expression::UnaryNot(inner) => format!("NOT({})", expression_to_raw(inner)),
        Expression::BinaryOp { op, left, right } => format!(
            "({} {} {})",
            expression_to_raw(left),
            match op {
                BinaryOperator::Add => "+",
                BinaryOperator::Sub => "-",
                BinaryOperator::Mul => "*",
                BinaryOperator::Div => "/",
                BinaryOperator::Mod => "%",
                BinaryOperator::Eq => "==",
                BinaryOperator::Neq => "!=",
                BinaryOperator::Gt => ">",
                BinaryOperator::Lt => "<",
                BinaryOperator::Gte => ">=",
                BinaryOperator::Lte => "<=",
                BinaryOperator::And => "AND",
                BinaryOperator::Or => "OR",
            },
            expression_to_raw(right)
        ),
        Expression::FunctionCall { name, args } => format!(
            "{}({})",
            name,
            args.iter()
                .map(expression_to_raw)
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

fn parse_timeout_statement(pair: Pair<Rule>) -> Result<TimeoutDirective, PlcError> {
    let line = line_of(&pair);
    let mut duration = None;
    let mut target = None;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::duration_value => duration = Some(parse_duration_value(part)?),
            Rule::goto_statement => target = Some(parse_goto_statement(part)?),
            _ => {}
        }
    }

    Ok(TimeoutDirective {
        duration: duration.ok_or_else(|| PlcError::parse(line, "timeout 缺少时长"))?,
        target: target.ok_or_else(|| PlcError::parse(line, "timeout 缺少 goto 目标"))?,
    })
}

fn parse_goto_statement(pair: Pair<Rule>) -> Result<GotoDirective, PlcError> {
    let line = line_of(&pair);
    let target = pair
        .into_inner()
        .next()
        .ok_or_else(|| PlcError::parse(line, "goto 缺少目标"))?;

    let mut identifiers = target
        .into_inner()
        .filter(|part| matches!(part.as_rule(), Rule::identifier));

    let task = identifiers
        .next()
        .ok_or_else(|| PlcError::parse(line, "goto 缺少目标 task"))?
        .as_str()
        .to_string();
    let step = identifiers.next().map(|part| part.as_str().to_string());

    Ok(GotoDirective { line, task, step })
}

fn parse_parallel_block(pair: Pair<Rule>) -> Result<ParallelBlock, PlcError> {
    let mut branches = Vec::new();

    for part in pair.into_inner() {
        if part.as_rule() == Rule::parallel_branch {
            branches.push(parse_parallel_branch(part)?);
        }
    }

    Ok(ParallelBlock { branches })
}

fn parse_parallel_branch(pair: Pair<Rule>) -> Result<Branch, PlcError> {
    let line = line_of(&pair);
    let mut statements = Vec::new();

    for part in pair.into_inner() {
        if part.as_rule() == Rule::parallel_branch_statement {
            let wrapped = first_inner(part, line, "parallel 分支语句")?;
            statements.push(parse_step_statement(wrapped)?);
        }
    }

    if statements.is_empty() {
        return Err(PlcError::parse(line, "parallel 分支至少需要一条语句"));
    }

    Ok(Branch { statements })
}

fn parse_race_block(pair: Pair<Rule>) -> Result<RaceBlock, PlcError> {
    let mut branches = Vec::new();

    for part in pair.into_inner() {
        if part.as_rule() == Rule::race_branch {
            branches.push(parse_race_branch(part)?);
        }
    }

    Ok(RaceBlock { branches })
}

fn parse_race_branch(pair: Pair<Rule>) -> Result<RaceBranch, PlcError> {
    let line = line_of(&pair);
    let mut statements = Vec::new();
    let mut then_goto = None;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::race_branch_statement => {
                let wrapped = first_inner(part, line, "race 分支语句")?;
                statements.push(parse_step_statement(wrapped)?);
            }
            Rule::then_goto_statement => {
                let goto_pair = part
                    .into_inner()
                    .next()
                    .ok_or_else(|| PlcError::parse(line, "then 缺少 goto 目标"))?;
                then_goto = Some(parse_goto_statement(goto_pair)?);
            }
            _ => {}
        }
    }

    if statements.is_empty() {
        return Err(PlcError::parse(line, "race 分支至少需要一条语句"));
    }

    Ok(RaceBranch {
        statements,
        then_goto,
    })
}

fn parse_allow_indefinite_wait(pair: Pair<Rule>) -> Result<bool, PlcError> {
    let line = line_of(&pair);
    let value = pair
        .into_inner()
        .next()
        .ok_or_else(|| PlcError::parse(line, "allow_indefinite_wait 缺少布尔值"))?;

    if value.as_str() == "true" {
        Ok(true)
    } else if value.as_str() == "false" {
        Ok(false)
    } else {
        Err(PlcError::parse(
            line,
            format!(
                "allow_indefinite_wait 需要 true/false，实际为: {}",
                value.as_str()
            ),
        ))
    }
}

fn parse_on_complete_statement(pair: Pair<Rule>) -> Result<OnCompleteDirective, PlcError> {
    let line = line_of(&pair);
    let raw = pair.as_str().to_string();
    if let Some(part) = pair.into_inner().next() {
        let goto = parse_goto_statement(part)?;
        Ok(OnCompleteDirective::Goto { target: goto })
    } else {
        if raw.contains("unreachable") {
            Ok(OnCompleteDirective::Unreachable)
        } else {
            Err(PlcError::parse(
                line,
                "on_complete 缺少 goto 或 unreachable",
            ))
        }
    }
}

fn parse_state_reference(pair: Pair<Rule>) -> Result<StateReference, PlcError> {
    let line = line_of(&pair);
    let raw = pair.as_str();
    let parts: Vec<&str> = raw.splitn(3, '.').collect();

    match parts.as_slice() {
        [device, state] => Ok(StateReference {
            device: device.to_string(),
            port: "self".to_string(),
            state: state.to_string(),
        }),
        [device, port, state] => Ok(StateReference {
            device: device.to_string(),
            port: port.to_string(),
            state: state.to_string(),
        }),
        _ => Err(PlcError::parse(line, format!("状态引用格式错误: {raw}"))),
    }
}

fn parse_action_target(pair: Pair<Rule>) -> Result<ActionTarget, PlcError> {
    let raw = pair.as_str();
    if let Some((device, port)) = raw.split_once('.') {
        Ok(ActionTarget {
            device: device.to_string(),
            port: port.to_string(),
        })
    } else {
        Ok(ActionTarget {
            device: raw.to_string(),
            port: "self".to_string(),
        })
    }
}

fn parse_duration_value(pair: Pair<Rule>) -> Result<DurationValue, PlcError> {
    let line = line_of(&pair);
    let raw = pair.as_str();

    let (value_raw, unit) = if let Some(value) = raw.strip_suffix("ms") {
        (value, TimeUnit::Ms)
    } else if let Some(value) = raw.strip_suffix('s') {
        (value, TimeUnit::S)
    } else {
        return Err(PlcError::parse(line, format!("不支持的时间单位: {raw}")));
    };

    let value = value_raw
        .parse::<f64>()
        .map_err(|_| PlcError::parse(line, format!("时间值解析失败: {raw}")))?;

    if value < 0.0 || value.fract() != 0.0 {
        return Err(PlcError::parse(
            line,
            format!("时间值必须为非负整数: {raw}"),
        ));
    }

    Ok(DurationValue {
        value: value as u64,
        unit,
    })
}

fn duration_value_to_ms(duration: &DurationValue) -> u64 {
    match duration.unit {
        TimeUnit::Ms => duration.value,
        TimeUnit::S => duration.value.saturating_mul(1000),
    }
}

fn parse_measured_value(pair: Pair<Rule>) -> Result<MeasuredValue, PlcError> {
    let line = line_of(&pair);
    let raw = pair.as_str();
    let idx = raw
        .find(|c: char| c.is_ascii_alphabetic())
        .ok_or_else(|| PlcError::parse(line, format!("带单位数值格式错误: {raw}")))?;

    let value = raw[..idx]
        .parse::<f64>()
        .map_err(|_| PlcError::parse(line, format!("数值解析失败: {raw}")))?;

    Ok(MeasuredValue {
        value,
        unit: raw[idx..].to_string(),
    })
}

fn parse_string_literal(pair: Pair<Rule>) -> Result<String, PlcError> {
    let line = line_of(&pair);
    let raw = pair.as_str();

    if raw.len() < 2 || !raw.starts_with('"') || !raw.ends_with('"') {
        return Err(PlcError::parse(
            line,
            format!("字符串字面量格式错误: {raw}"),
        ));
    }

    Ok(raw[1..raw.len() - 1].replace("\\\"", "\""))
}

fn parse_range_value(pair: Pair<Rule>) -> Result<crate::ast::AnalogRange, PlcError> {
    let line = line_of(&pair);
    if pair.as_rule() == Rule::range_value {
        let raw = pair.as_str();
        let (min_str, max_str) = raw
            .split_once("..")
            .ok_or_else(|| PlcError::parse(line, format!("range 格式错误: {raw}")))?;
        let min = min_str
            .parse::<f64>()
            .map_err(|_| PlcError::parse(line, format!("range 最小值解析失败: {min_str}")))?;
        let max = max_str
            .parse::<f64>()
            .map_err(|_| PlcError::parse(line, format!("range 最大值解析失败: {max_str}")))?;
        Ok(crate::ast::AnalogRange { min, max })
    } else {
        Err(PlcError::parse(
            line,
            format!(
                "属性 range 需要范围值（如 0..100），实际为: {:?}",
                pair.as_rule()
            ),
        ))
    }
}

fn expect_number(pair: Pair<Rule>, field_name: &str) -> Result<f64, PlcError> {
    let line = line_of(&pair);
    if matches!(pair.as_rule(), Rule::number | Rule::integer) {
        pair.as_str().parse::<f64>().map_err(|_| {
            PlcError::parse(
                line,
                format!("属性 {field_name} 数值解析失败: {}", pair.as_str()),
            )
        })
    } else {
        Err(PlcError::parse(
            line,
            format!("属性 {field_name} 需要数字值"),
        ))
    }
}

fn expect_u64(pair: Pair<Rule>, field_name: &str) -> Result<u64, PlcError> {
    let line = line_of(&pair);
    if matches!(pair.as_rule(), Rule::integer | Rule::number) {
        let raw = pair.as_str();
        if raw.contains('.') {
            return Err(PlcError::parse(
                line,
                format!("属性 {field_name} 需要整数值，实际为: {raw}"),
            ));
        }
        raw.parse::<u64>()
            .map_err(|_| PlcError::parse(line, format!("属性 {field_name} 整数解析失败: {raw}")))
    } else {
        Err(PlcError::parse(
            line,
            format!("属性 {field_name} 需要整数值"),
        ))
    }
}

fn expect_pid_setpoint(pair: Pair<Rule>, field_name: &str) -> Result<LiteralValue, PlcError> {
    let line = line_of(&pair);
    match pair.as_rule() {
        Rule::number | Rule::integer => {
            let value = pair
                .as_str()
                .parse::<f64>()
                .map_err(|_| PlcError::parse(line, format!("属性 {field_name} 数值解析失败")))?;
            Ok(LiteralValue::Number(value))
        }
        Rule::measured_value => {
            let measured = parse_measured_value(pair)?;
            Ok(LiteralValue::Measured(measured))
        }
        _ => Err(PlcError::parse(
            line,
            format!("属性 {field_name} 需要 number 或 measured_value"),
        )),
    }
}

fn expect_string(pair: Pair<Rule>, field_name: &str) -> Result<String, PlcError> {
    let line = line_of(&pair);
    if pair.as_rule() == Rule::string_literal {
        parse_string_literal(pair)
    } else {
        Err(PlcError::parse(
            line,
            format!("属性 {field_name} 需要字符串值"),
        ))
    }
}

fn expect_identifier(pair: Pair<Rule>, field_name: &str) -> Result<String, PlcError> {
    let line = line_of(&pair);
    if pair.as_rule() == Rule::identifier {
        Ok(pair.as_str().to_string())
    } else {
        Err(PlcError::parse(
            line,
            format!("属性 {field_name} 需要标识符"),
        ))
    }
}

fn expect_identifier_or_string(pair: Pair<Rule>, field_name: &str) -> Result<String, PlcError> {
    let line = line_of(&pair);
    match pair.as_rule() {
        Rule::identifier => Ok(pair.as_str().to_string()),
        Rule::string_literal => parse_string_literal(pair),
        _ => Err(PlcError::parse(
            line,
            format!("属性 {field_name} 需要标识符或字符串"),
        )),
    }
}

fn expect_duration(pair: Pair<Rule>, field_name: &str) -> Result<DurationValue, PlcError> {
    let line = line_of(&pair);
    if pair.as_rule() == Rule::duration_value {
        parse_duration_value(pair)
    } else {
        Err(PlcError::parse(
            line,
            format!("属性 {field_name} 需要时间值（如 20ms）"),
        ))
    }
}

fn expect_measured(pair: Pair<Rule>, field_name: &str) -> Result<MeasuredValue, PlcError> {
    let line = line_of(&pair);
    if pair.as_rule() == Rule::measured_value {
        parse_measured_value(pair)
    } else {
        Err(PlcError::parse(
            line,
            format!("属性 {field_name} 需要带单位数值（如 100mm）"),
        ))
    }
}

fn expect_boolean(pair: Pair<Rule>, field_name: &str) -> Result<bool, PlcError> {
    let line = line_of(&pair);
    if pair.as_rule() == Rule::boolean_value {
        Ok(pair.as_str() == "true")
    } else {
        Err(PlcError::parse(
            line,
            format!("属性 {field_name} 需要布尔值 true/false"),
        ))
    }
}

fn expect_relation_endpoint(
    pair: Pair<Rule>,
    field_name: &str,
) -> Result<(String, Option<String>), PlcError> {
    let line = line_of(&pair);
    match pair.as_rule() {
        Rule::port_reference => {
            let (device, port) = pair.as_str().split_once('.').ok_or_else(|| {
                PlcError::parse(line, format!("属性 {field_name} 端口引用格式错误"))
            })?;
            Ok((device.to_string(), Some(port.to_string())))
        }
        Rule::identifier => Ok((pair.as_str().to_string(), None)),
        _ => Err(PlcError::parse(
            line,
            format!("属性 {field_name} 需要端点引用（如 device 或 device.port）"),
        )),
    }
}

fn expect_topology_relation(
    pair: Pair<Rule>,
    field_name: &str,
) -> Result<TopologyRelation, PlcError> {
    let line = line_of(&pair);
    if pair.as_rule() != Rule::topology_relation {
        return Err(PlcError::parse(
            line,
            format!("属性 {field_name} 需要拓扑关系（driven_by/reports_to/detects）"),
        ));
    }

    match pair.as_str() {
        "driven_by" => Ok(TopologyRelation::DrivenBy),
        "reports_to" => Ok(TopologyRelation::ReportsTo),
        "detects" => Ok(TopologyRelation::Detects),
        other => Err(PlcError::parse(
            line,
            format!("不支持的 relation 类型: {other}"),
        )),
    }
}

fn expect_identifier_list(pair: Pair<Rule>, field_name: &str) -> Result<Vec<String>, PlcError> {
    let line = line_of(&pair);
    if pair.as_rule() != Rule::identifier_list {
        return Err(PlcError::parse(
            line,
            format!("属性 {field_name} 需要标识符列表（如 [extend, neutral, retract]）"),
        ));
    }

    let values = pair
        .into_inner()
        .filter(|part| matches!(part.as_rule(), Rule::identifier))
        .map(|part| part.as_str().to_string())
        .collect::<Vec<_>>();

    if values.is_empty() {
        return Err(PlcError::parse(
            line,
            format!("属性 {field_name} 至少需要一个状态"),
        ));
    }

    Ok(values)
}

fn parse_u32_from_str(line: usize, raw: &str, field_name: &str) -> Result<u32, PlcError> {
    let cleaned = raw
        .trim()
        .trim_end_matches('}')
        .trim_end_matches(',')
        .trim();
    cleaned.parse::<u32>().map_err(|_| {
        PlcError::parse(
            line,
            format!("{field_name} 需要无符号整数，当前为: {cleaned}"),
        )
    })
}

fn expect_port_list(pair: Pair<Rule>, field_name: &str) -> Result<Vec<DevicePort>, PlcError> {
    let line = line_of(&pair);
    if pair.as_rule() != Rule::port_list {
        return Err(PlcError::parse(
            line,
            format!(
                "属性 {field_name} 需要端口列表（如 [in:digital:consumer, out:digital:producer]）"
            ),
        ));
    }

    let ports = pair
        .into_inner()
        .filter(|part| part.as_rule() == Rule::port_definition)
        .map(parse_port_definition)
        .collect::<Result<Vec<_>, _>>()?;

    if ports.is_empty() {
        return Err(PlcError::parse(
            line,
            format!("属性 {field_name} 至少需要一个端口定义"),
        ));
    }

    Ok(ports)
}

fn expect_tags(pair: Pair<Rule>, field_name: &str) -> Result<DeviceTags, PlcError> {
    let line = line_of(&pair);
    if pair.as_rule() != Rule::tags_value {
        return Err(PlcError::parse(
            line,
            format!(
                "属性 {field_name} 需要标签对象（如 {{ functional_group: [clamp], danger_level: [high] }}）"
            ),
        ));
    }

    let mut tags = DeviceTags::default();
    for dimension in pair
        .into_inner()
        .filter(|part| part.as_rule() == Rule::tag_dimension)
    {
        let dimension_line = line_of(&dimension);
        let mut key = None::<String>;
        let mut values = None::<Vec<String>>;

        for part in dimension.into_inner() {
            match part.as_rule() {
                Rule::tag_dimension_name => key = Some(part.as_str().to_string()),
                Rule::tag_value_list => values = Some(parse_tag_values(part)?),
                _ => {}
            }
        }

        let key = key.ok_or_else(|| PlcError::parse(dimension_line, "tags 维度缺少名称"))?;
        let values =
            values.ok_or_else(|| PlcError::parse(dimension_line, "tags 维度缺少值列表"))?;
        if values.is_empty() {
            return Err(PlcError::parse(
                dimension_line,
                format!("tags.{key} 至少需要一个标签值"),
            ));
        }

        match key.as_str() {
            "functional_group" => tags.functional_group = values,
            "danger_level" => tags.danger_level = values,
            "location_group" => tags.location_group = values,
            _ => {
                return Err(PlcError::parse(
                    dimension_line,
                    format!("不支持的 tags 维度: {key}"),
                ));
            }
        }
    }

    if tags.is_empty() {
        return Err(PlcError::parse(
            line,
            format!("属性 {field_name} 至少需要一个标签维度"),
        ));
    }

    Ok(tags)
}

fn parse_tag_values(pair: Pair<Rule>) -> Result<Vec<String>, PlcError> {
    let line = line_of(&pair);
    if pair.as_rule() != Rule::tag_value_list {
        return Err(PlcError::parse(line, "tags 维度值必须是列表"));
    }

    let mut values = Vec::new();
    for item in pair
        .into_inner()
        .filter(|part| part.as_rule() == Rule::tag_value)
    {
        let value = first_inner(item, line, "tags 标签值")?;
        match value.as_rule() {
            Rule::identifier => values.push(value.as_str().to_string()),
            Rule::string_literal => values.push(parse_string_literal(value)?),
            _ => {
                return Err(PlcError::parse(line, "tags 标签值仅支持标识符或字符串"));
            }
        }
    }

    Ok(values)
}

fn parse_port_definition(pair: Pair<Rule>) -> Result<DevicePort, PlcError> {
    let line = line_of(&pair);
    let mut port_id = None;
    let mut port_type = None;
    let mut role = None;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::identifier => port_id = Some(part.as_str().to_string()),
            Rule::port_type => port_type = Some(parse_port_type(part)?),
            Rule::port_role => role = Some(parse_port_role(part)?),
            _ => {}
        }
    }

    Ok(DevicePort {
        id: port_id.ok_or_else(|| PlcError::parse(line, "端口定义缺少 id"))?,
        port_type: port_type.ok_or_else(|| PlcError::parse(line, "端口定义缺少 type"))?,
        role: role.ok_or_else(|| PlcError::parse(line, "端口定义缺少 role"))?,
        states: Vec::new(),
        default_state: String::new(),
    })
}

fn parse_port_type(pair: Pair<Rule>) -> Result<PortType, PlcError> {
    let line = line_of(&pair);
    match pair.as_str() {
        "digital" => Ok(PortType::Digital),
        "analog" => Ok(PortType::Analog),
        "pneumatic" => Ok(PortType::Pneumatic),
        "logical" => Ok(PortType::Logical),
        "generic" => Ok(PortType::Generic),
        other => Err(PlcError::parse(line, format!("不支持的端口类型: {other}"))),
    }
}

fn parse_port_role(pair: Pair<Rule>) -> Result<PortRole, PlcError> {
    let line = line_of(&pair);
    match pair.as_str() {
        "producer" => Ok(PortRole::Producer),
        "consumer" => Ok(PortRole::Consumer),
        "bidirectional" => Ok(PortRole::Bidirectional),
        other => Err(PlcError::parse(line, format!("不支持的端口角色: {other}"))),
    }
}

fn first_inner<'a>(
    pair: Pair<'a, Rule>,
    line: usize,
    context: &str,
) -> Result<Pair<'a, Rule>, PlcError> {
    pair.into_inner()
        .next()
        .ok_or_else(|| PlcError::parse(line, format!("{context} 缺少内部结构")))
}

fn line_of(pair: &Pair<Rule>) -> usize {
    pair.as_span().start_pos().line_col().0
}

fn col_of(pair: &Pair<Rule>) -> usize {
    pair.as_span().start_pos().line_col().1
}

fn map_parse_error(err: pest::error::Error<Rule>) -> PlcError {
    let (line, col) = match err.line_col {
        LineColLocation::Pos((line, col)) => (line, col),
        LineColLocation::Span((line, col), _) => (line, col),
    };

    PlcError::parse_at("<input>", line, col, format!("语法解析失败: {err}"))
}

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

        assert!(
            detect_step
                .statements
                .iter()
                .any(|stmt| matches!(stmt, StepStatement::Race(_)))
        );
        assert!(
            detect_step
                .statements
                .iter()
                .any(|stmt| matches!(stmt, StepStatement::Timeout(_)))
        );

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
}
