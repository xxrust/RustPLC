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
        ActionStatement::DeviceAction { args, .. } => {
            for arg in args {
                reject_extern_calls_in_expression(arg, extern_names, line, task_name, step_name)?;
            }
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
        "proportional_valve" => Ok(DeviceType::ProportionalValve),
        "gripper" => Ok(DeviceType::Gripper),
        "conveyor" => Ok(DeviceType::Conveyor),
        "pump" => Ok(DeviceType::Pump),
        "heater" => Ok(DeviceType::Heater),
        "vision_sensor" => Ok(DeviceType::VisionSensor),
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
        "open_loop_policy" => {
            attributes.open_loop_policy =
                Some(expect_identifier_or_string(value, "open_loop_policy")?);
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
