fn parse_tasks_section(pair: Pair<Rule>) -> Result<TasksSection, PlcError> {
    let mut task_templates = Vec::new();
    let mut task_instances = Vec::new();
    let mut tasks = Vec::new();

    for item in pair.into_inner() {
        match item.as_rule() {
            Rule::task_template_declaration => {
                task_templates.push(parse_task_template_declaration(item)?)
            }
            Rule::task_instance_declaration => {
                task_instances.push(parse_task_instance_declaration(item)?)
            }
            Rule::task_declaration => tasks.push(parse_task_declaration(item)?),
            _ => {}
        }
    }

    Ok(TasksSection {
        task_templates,
        task_instances,
        tasks,
    })
}

fn parse_task_template_declaration(pair: Pair<Rule>) -> Result<TaskTemplateDeclaration, PlcError> {
    let line = line_of(&pair);
    let mut name = None;
    let mut params = Vec::new();
    let mut tasks = Vec::new();

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::identifier if name.is_none() => name = Some(part.as_str().to_string()),
            Rule::template_param_list => {
                params = part
                    .into_inner()
                    .filter(|item| item.as_rule() == Rule::identifier)
                    .map(|item| item.as_str().to_string())
                    .collect();
            }
            Rule::task_declaration => tasks.push(parse_task_declaration(part)?),
            _ => {}
        }
    }

    Ok(TaskTemplateDeclaration {
        line,
        name: name.ok_or_else(|| PlcError::parse(line, "task_template missing name"))?,
        params,
        tasks,
    })
}

fn parse_task_instance_declaration(pair: Pair<Rule>) -> Result<TaskInstanceDeclaration, PlcError> {
    let line = line_of(&pair);
    let mut identifiers = Vec::new();

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::identifier => identifiers.push(part.as_str().to_string()),
            Rule::template_identifier_arg_list => identifiers.extend(
                part.into_inner()
                    .filter(|item| item.as_rule() == Rule::identifier)
                    .map(|item| item.as_str().to_string()),
            ),
            _ => {}
        }
    }

    if identifiers.len() < 2 {
        return Err(PlcError::parse(
            line,
            "task_instance requires instance name and template name",
        ));
    }

    let name = identifiers.remove(0);
    let template = identifiers.remove(0);

    Ok(TaskInstanceDeclaration {
        line,
        name,
        template,
        args: identifiers,
    })
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
        Rule::match_statement => parse_match_statement(pair),
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

fn parse_match_statement(pair: Pair<Rule>) -> Result<StepStatement, PlcError> {
    let line = line_of(&pair);
    let mut selector = None;
    let mut cases = Vec::new();
    let mut default = None;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::condition_operand => {
                let inner = first_inner(part, line, "match selector")?;
                selector = Some(inner.as_str().to_string());
            }
            Rule::match_case => cases.push(parse_match_case(part)?),
            Rule::match_default => {
                let goto = part
                    .into_inner()
                    .find(|item| item.as_rule() == Rule::goto_statement)
                    .ok_or_else(|| PlcError::parse(line, "match default missing goto"))?;
                default = Some(parse_goto_statement(goto)?);
            }
            _ => {}
        }
    }

    if cases.len() != 1 {
        return Err(PlcError::parse(
            line,
            "match currently supports exactly one case plus default",
        ));
    }

    let case = cases
        .into_iter()
        .next()
        .ok_or_else(|| PlcError::parse(line, "match requires one case"))?;

    Ok(StepStatement::IfElse {
        condition: ConditionExpression::legacy(
            selector.ok_or_else(|| PlcError::parse(line, "match missing selector"))?,
            ComparisonOperator::Eq,
            case.pattern,
        ),
        then_goto: case.target,
        else_goto: default.ok_or_else(|| PlcError::parse(line, "match missing default branch"))?,
    })
}

struct ParsedMatchCase {
    pattern: LiteralValue,
    target: GotoDirective,
}

fn parse_match_case(pair: Pair<Rule>) -> Result<ParsedMatchCase, PlcError> {
    let line = line_of(&pair);
    let mut pattern = None;
    let mut target = None;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::match_pattern => pattern = Some(parse_match_pattern(part)?),
            Rule::goto_statement => target = Some(parse_goto_statement(part)?),
            _ => {}
        }
    }

    Ok(ParsedMatchCase {
        pattern: pattern.ok_or_else(|| PlcError::parse(line, "match case missing pattern"))?,
        target: target.ok_or_else(|| PlcError::parse(line, "match case missing goto"))?,
    })
}

fn parse_match_pattern(pair: Pair<Rule>) -> Result<LiteralValue, PlcError> {
    let line = line_of(&pair);
    let value = first_inner(pair, line, "match pattern")?;

    match value.as_rule() {
        Rule::boolean_value => Ok(LiteralValue::Boolean(value.as_str() == "true")),
        Rule::number => {
            let parsed = parse_finite_f64(value.as_str(), line, "match pattern")
                .map_err(|_| PlcError::parse(line, "match number pattern parse failed"))?;
            Ok(LiteralValue::Number(parsed))
        }
        Rule::string_literal => Ok(LiteralValue::String(parse_string_literal(value)?)),
        Rule::identifier => Ok(LiteralValue::String(value.as_str().to_string())),
        rule => Err(PlcError::parse(
            line,
            format!("unsupported match pattern: {rule:?}"),
        )),
    }
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
            let value = parse_finite_f64(value_pair.as_str(), line, "set_analog")
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
        Rule::device_action => {
            let mut parts = action.into_inner();
            let family = parts
                .next()
                .ok_or_else(|| PlcError::parse(line, "device action 缺少设备族"))?
                .as_str()
                .to_string();
            let action_name = parts
                .next()
                .ok_or_else(|| PlcError::parse(line, "device action 缺少动作名"))?
                .as_str()
                .to_string();
            let target_pair = parts
                .next()
                .ok_or_else(|| PlcError::parse(line, "device action 缺少目标设备"))?;
            let target = parse_action_target(target_pair)?;
            let mut args = Vec::new();
            for part in parts {
                if part.as_rule() == Rule::device_action_arg {
                    let expr_pair = part
                        .into_inner()
                        .next()
                        .ok_or_else(|| PlcError::parse(line, "device action 参数缺少表达式"))?;
                    args.push(parse_expression(expr_pair)?);
                }
            }
            Ok(ActionStatement::DeviceAction {
                family,
                action_name,
                target,
                args,
            })
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

    parse_finite_f64(number.as_str(), line, &format!("axis.move {field_name}"))
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

    if let Some(edge_pair) = condition_pair
        .clone()
        .into_inner()
        .find(|part| part.as_rule() == Rule::edge_condition)
    {
        return Ok(WaitStatement {
            condition: WaitCondition::Edge(parse_edge_condition(edge_pair)?),
        });
    }

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

fn parse_edge_condition(pair: Pair<Rule>) -> Result<crate::ast::EdgeCondition, PlcError> {
    let line = line_of(&pair);
    let mut edge = None;
    let mut operand = None;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::edge_kind => {
                edge = Some(match part.as_str() {
                    "rising_edge" => crate::ast::EdgeKind::Rising,
                    "falling_edge" => crate::ast::EdgeKind::Falling,
                    other => {
                        return Err(PlcError::parse(
                            line,
                            format!("未知边沿触发类型: {other}"),
                        ));
                    }
                });
            }
            Rule::condition_operand => {
                let inner = first_inner(part, line, "边沿触发操作数")?;
                operand = Some(inner.as_str().to_string());
            }
            _ => {}
        }
    }

    Ok(crate::ast::EdgeCondition {
        edge: edge.ok_or_else(|| PlcError::parse(line, "边沿触发缺少类型"))?,
        operand: operand.ok_or_else(|| PlcError::parse(line, "边沿触发缺少操作数"))?,
    })
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
            let parsed = parse_finite_f64(value.as_str(), line, "literal")
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

const MAX_EXPRESSION_DEPTH: usize = 128;
const MAX_EXPRESSION_NODES: usize = 4096;

#[derive(Default)]
struct ExpressionParseBudget {
    nodes: usize,
}

fn parse_expression(pair: Pair<Rule>) -> Result<Expression, PlcError> {
    let mut budget = ExpressionParseBudget::default();
    parse_expression_with_budget(pair, 0, &mut budget)
}

fn parse_expression_with_budget(
    pair: Pair<Rule>,
    depth: usize,
    budget: &mut ExpressionParseBudget,
) -> Result<Expression, PlcError> {
    let line = line_of(&pair);
    if depth > MAX_EXPRESSION_DEPTH {
        return Err(PlcError::parse_with_reason(
            line,
            format!("expression depth exceeds limit {MAX_EXPRESSION_DEPTH}"),
            "split the expression into intermediate variables",
        ));
    }
    budget.nodes = budget.nodes.saturating_add(1);
    if budget.nodes > MAX_EXPRESSION_NODES {
        return Err(PlcError::parse_with_reason(
            line,
            format!("expression node budget exceeds limit {MAX_EXPRESSION_NODES}"),
            "split the expression into smaller compute statements",
        ));
    }
    match pair.as_rule() {
        Rule::expression | Rule::expr_or | Rule::expr_and | Rule::expr_add | Rule::expr_mul => {
            let mut inner = pair.into_inner();
            let first = inner
                .next()
                .ok_or_else(|| PlcError::parse(line, "表达式为空"))?;
            let mut expr = parse_expression_with_budget(first, depth + 1, budget)?;
            while let Some(op) = inner.next() {
                let rhs_pair = inner
                    .next()
                    .ok_or_else(|| PlcError::parse(line, "表达式缺少右操作数"))?;
                let rhs = parse_expression_with_budget(rhs_pair, depth + 1, budget)?;
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
            let mut expr = parse_expression_with_budget(first, depth + 1, budget)?;
            if let Some(op) = inner.next() {
                let rhs_pair = inner
                    .next()
                    .ok_or_else(|| PlcError::parse(line, "比较表达式缺少右操作数"))?;
                let rhs = parse_expression_with_budget(rhs_pair, depth + 1, budget)?;
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
            let mut expr = parse_expression_with_budget(inner_pair, depth + 1, budget)?;
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
            parse_expression_with_budget(inner, depth + 1, budget)
        }
        Rule::expr_func_call => parse_function_call_expression(pair, depth, budget),
        Rule::expr_literal => match pair.as_str() {
            "true" => Ok(Expression::Boolean(true)),
            "false" => Ok(Expression::Boolean(false)),
            raw => {
                let parsed = parse_finite_f64(raw, line, "expression literal")
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

fn parse_function_call_expression(
    pair: Pair<Rule>,
    depth: usize,
    budget: &mut ExpressionParseBudget,
) -> Result<Expression, PlcError> {
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
            args.push(parse_expression_with_budget(item, depth + 1, budget)?);
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

    let value = parse_finite_f64(value_raw, line, "duration")
        .map_err(|_| PlcError::parse(line, format!("时间值解析失败: {raw}")))?;

    if value < 0.0 || value.fract() != 0.0 || value >= u64::MAX as f64 {
        return Err(PlcError::parse(
            line,
            format!("时间值必须为可表示的非负整数: {raw}"),
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

    let value = parse_finite_f64(&raw[..idx], line, "measured value")
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
        let min = parse_finite_f64(min_str, line, "range minimum")
            .map_err(|_| PlcError::parse(line, format!("range 最小值解析失败: {min_str}")))?;
        let max = parse_finite_f64(max_str, line, "range maximum")
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
        parse_finite_f64(pair.as_str(), line, &format!("attribute {field_name}")).map_err(|_| {
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
            let value = parse_finite_f64(
                pair.as_str(),
                line,
                &format!("attribute {field_name}"),
            )
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

