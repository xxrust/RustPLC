use crate::ast::{
    ActionStatement, BinaryValue, Branch, CausalityConstraint, ComparisonOperator,
    ConditionExpression, ConstraintsSection, DeviceAttributes, DeviceDeclaration, DeviceType,
    DurationValue, GotoDirective, LiteralValue, MeasuredValue, OnCompleteDirective, ParallelBlock,
    PlcProgram, RaceBlock, RaceBranch, SafetyConstraint, SafetyRelation, StateReference,
    StepDeclaration, StepStatement, TaskDeclaration, TasksSection, TimeUnit, TimeoutDirective,
    TimingConstraint, TimingRelation, TimingTarget, TopologySection, WaitStatement,
};
use crate::error::PlcError;
use pest::Parser;
use pest::error::LineColLocation;
use pest::iterators::Pair;

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
    let mut pairs = PlcParser::parse(Rule::plc_file, input).map_err(map_parse_error)?;
    let plc_pair = pairs
        .next()
        .ok_or_else(|| PlcError::parse(1, "未找到可解析的 PLC 程序"))?;

    parse_plc_pair(plc_pair)
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

fn parse_topology_section(pair: Pair<Rule>) -> Result<TopologySection, PlcError> {
    let mut devices = Vec::new();

    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::device_declaration {
            devices.push(parse_device_declaration(entry)?);
        }
    }

    Ok(TopologySection { devices })
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
        "solenoid_valve" => Ok(DeviceType::SolenoidValve),
        "cylinder" => Ok(DeviceType::Cylinder),
        "sensor" => Ok(DeviceType::Sensor),
        "motor" => Ok(DeviceType::Motor),
        other => Err(PlcError::parse(line, format!("未知设备类型: {other}"))),
    }
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
        "connected_to" => {
            attributes.connected_to = Some(expect_identifier(value, "connected_to")?);
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
        "type" => {
            attributes.r#type = Some(expect_identifier_or_string(value, "type")?);
        }
        "detects" => {
            attributes.detects = Some(expect_state_reference(value, "detects")?);
        }
        "debounce" => {
            attributes.debounce = Some(expect_duration(value, "debounce")?);
        }
        "inverted" => {
            attributes.inverted = Some(expect_boolean(value, "inverted")?);
        }
        "rated_speed" => {
            attributes.rated_speed = Some(expect_measured(value, "rated_speed")?);
        }
        "ramp_time" => {
            attributes.ramp_time = Some(expect_duration(value, "ramp_time")?);
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

fn parse_constraints_section(pair: Pair<Rule>) -> Result<ConstraintsSection, PlcError> {
    let mut safety = Vec::new();
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
            Rule::timing_constraint => timing.push(parse_timing_constraint(constraint)?),
            Rule::causality_constraint => causality.push(parse_causality_constraint(constraint)?),
            _ => {}
        }
    }

    Ok(ConstraintsSection {
        safety,
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
            Rule::state_reference if left.is_none() => left = Some(parse_state_reference(part)?),
            Rule::safety_relation => relation = Some(parse_safety_relation(part)?),
            Rule::state_reference => right = Some(parse_state_reference(part)?),
            Rule::reason_clause => reason = Some(parse_reason_clause(part)?),
            _ => {}
        }
    }

    Ok(SafetyConstraint {
        line,
        left: left.ok_or_else(|| PlcError::parse(line, "safety 约束缺少左侧状态"))?,
        relation: relation.ok_or_else(|| PlcError::parse(line, "safety 约束缺少关系符"))?,
        right: right.ok_or_else(|| PlcError::parse(line, "safety 约束缺少右侧状态"))?,
        reason,
    })
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
        Rule::action_statement => Ok(StepStatement::Action(parse_action_statement(pair)?)),
        Rule::wait_statement => Ok(StepStatement::Wait(parse_wait_statement(pair)?)),
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
            let target = action
                .into_inner()
                .next()
                .ok_or_else(|| PlcError::parse(line, "extend 缺少目标设备"))?
                .as_str()
                .to_string();
            Ok(ActionStatement::Extend { target })
        }
        Rule::action_retract => {
            let target = action
                .into_inner()
                .next()
                .ok_or_else(|| PlcError::parse(line, "retract 缺少目标设备"))?
                .as_str()
                .to_string();
            Ok(ActionStatement::Retract { target })
        }
        Rule::action_set => {
            let mut parts = action.into_inner();
            let target = parts
                .next()
                .ok_or_else(|| PlcError::parse(line, "set 缺少目标设备"))?
                .as_str()
                .to_string();
            let value_pair = parts
                .next()
                .ok_or_else(|| PlcError::parse(line, "set 缺少 on/off 值"))?;
            let value = parse_binary_value(value_pair)?;
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

fn parse_binary_value(pair: Pair<Rule>) -> Result<BinaryValue, PlcError> {
    let line = line_of(&pair);
    match pair.as_str() {
        "on" => Ok(BinaryValue::On),
        "off" => Ok(BinaryValue::Off),
        other => Err(PlcError::parse(
            line,
            format!("set 语句的值必须是 on/off，实际为: {other}"),
        )),
    }
}

fn parse_wait_statement(pair: Pair<Rule>) -> Result<WaitStatement, PlcError> {
    let line = line_of(&pair);
    let mut operand = None;
    let mut operator = None;
    let mut value = None;

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::condition_operand => {
                let inner = first_inner(part, line, "wait 左值")?;
                operand = Some(inner.as_str().to_string());
            }
            Rule::comparison_operator => operator = Some(parse_comparison_operator(part)?),
            Rule::condition_value => value = Some(parse_condition_value(part)?),
            _ => {}
        }
    }

    Ok(WaitStatement {
        condition: ConditionExpression {
            left: operand.ok_or_else(|| PlcError::parse(line, "wait 缺少左值"))?,
            operator: operator.ok_or_else(|| PlcError::parse(line, "wait 缺少比较符"))?,
            right: value.ok_or_else(|| PlcError::parse(line, "wait 缺少右值"))?,
        },
    })
}

fn parse_comparison_operator(pair: Pair<Rule>) -> Result<ComparisonOperator, PlcError> {
    let line = line_of(&pair);
    match pair.as_str() {
        "==" => Ok(ComparisonOperator::Eq),
        "!=" => Ok(ComparisonOperator::Neq),
        other => Err(PlcError::parse(line, format!("不支持的比较符: {other}"))),
    }
}

fn parse_condition_value(pair: Pair<Rule>) -> Result<LiteralValue, PlcError> {
    let line = line_of(&pair);
    let value = first_inner(pair, line, "wait 右值")?;

    match value.as_rule() {
        Rule::boolean_value => Ok(LiteralValue::Boolean(value.as_str() == "true")),
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
    let step = pair
        .into_inner()
        .next()
        .ok_or_else(|| PlcError::parse(line, "goto 缺少目标 step"))?
        .as_str()
        .to_string();

    Ok(GotoDirective { line, step })
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
        Ok(OnCompleteDirective::Goto { step: goto.step })
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
    let (device, state) = raw
        .split_once('.')
        .ok_or_else(|| PlcError::parse(line, format!("状态引用格式错误: {raw}")))?;

    Ok(StateReference {
        device: device.to_string(),
        state: state.to_string(),
    })
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

fn expect_state_reference(pair: Pair<Rule>, field_name: &str) -> Result<StateReference, PlcError> {
    let line = line_of(&pair);
    if pair.as_rule() == Rule::state_reference {
        parse_state_reference(pair)
    } else {
        Err(PlcError::parse(
            line,
            format!("属性 {field_name} 需要状态引用（如 cyl_A.extended）"),
        ))
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
    use crate::ast::{ActionStatement, OnCompleteDirective, StepStatement};

    #[test]
    fn parses_prd_5_3_topology_example() {
        let input = r#"
[topology]

# ===== controller ports =====
device Y0: digital_output               # digital output port
device Y1: digital_output
device Y2: digital_output               # alarm light output
device X0: digital_input                # digital input port
device X1: digital_input
device X2: digital_input
device X3: digital_input
device X4: digital_input                # start button

# ===== operator panel =====
device start_button: digital_input {
    connected_to: X4,
    debounce: 20ms
}

device alarm_light: digital_output {
    connected_to: Y2
}

# ===== solenoid valves =====
device valve_A: solenoid_valve {
    type: "5/2",
    connected_to: Y0,
    response_time: 15ms
}

device valve_B: solenoid_valve {
    type: "5/2",
    connected_to: Y1,
    response_time: 15ms
}

# ===== cylinders =====
device cyl_A: cylinder {
    type: double_acting,
    connected_to: valve_A,
    stroke: 100mm,
    stroke_time: 200ms,
    retract_time: 180ms
}

device cyl_B: cylinder {
    type: double_acting,
    connected_to: valve_B,
    stroke: 150mm,
    stroke_time: 300ms,
    retract_time: 250ms
}

# ===== sensors =====
device sensor_A_ext: sensor {
    type: magnetic,
    connected_to: X0,
    detects: cyl_A.extended
}

device sensor_A_ret: sensor {
    type: magnetic,
    connected_to: X1,
    detects: cyl_A.retracted
}

device sensor_B_ext: sensor {
    type: magnetic,
    connected_to: X2,
    detects: cyl_B.extended
}

device sensor_B_ret: sensor {
    type: magnetic,
    connected_to: X3,
    detects: cyl_B.retracted
}
"#;

        assert!(parse_topology(input).is_ok());
    }

    #[test]
    fn parses_all_topology_device_types_and_property_shapes() {
        let input = r#"
[topology]

device Y3: digital_output
device X5: digital_input

device estop: digital_input {
    connected_to: X5,
    debounce: 10ms,
    inverted: true
}

device spindle_valve: solenoid_valve {
    connected_to: Y3,
    response_time: 25ms,
    type: "3/2"
}

device spindle_cyl: cylinder {
    connected_to: spindle_valve,
    stroke_time: 120ms,
    retract_time: 110ms,
    stroke: 80mm,
    type: compact
}

device spindle_sensor: sensor {
    connected_to: X5,
    detects: spindle_cyl.extended,
    type: optical
}

device spindle_motor: motor {
    connected_to: Y3,
    rated_speed: 60rpm,
    ramp_time: 300ms
}
"#;

        assert!(parse_topology(input).is_ok());
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
device valve_A: solenoid_valve { connected_to: Y0 }
device cyl_A: cylinder { connected_to: valve_A, stroke_time: 200ms, retract_time: 180ms }
device sensor_A_ext: sensor { connected_to: X0, detects: cyl_A.extended }

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
device valve_glue: solenoid_valve { connected_to: Y0 }
device cyl_glue: cylinder { connected_to: valve_glue, stroke_time: 200ms, retract_time: 180ms }
device sensor_glue_ext: sensor { connected_to: X0, detects: cyl_glue.extended }

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
device valve_glue: solenoid_valve { connected_to: Y0 }
device cyl_glue: cylinder { connected_to: valve_glue, stroke_time: 200ms, retract_time: 180ms }

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
    connected_to: X4
    debounce: 20ms
}

device valve_A: solenoid_valve {
    connected_to: Y0
    response_time: 20ms
}
device valve_B: solenoid_valve {
    connected_to: Y1
    response_time: 20ms
}
device cyl_A: cylinder {
    connected_to: valve_A
    stroke_time: 300ms
    retract_time: 300ms
}
device cyl_B: cylinder {
    connected_to: valve_B
    stroke_time: 300ms
    retract_time: 300ms
}
device sensor_A_ext: sensor { connected_to: X0, detects: cyl_A.extended }
device sensor_A_ret: sensor { connected_to: X1, detects: cyl_A.retracted }
device sensor_B_ext: sensor { connected_to: X2, detects: cyl_B.extended }
device sensor_B_ret: sensor { connected_to: X3, detects: cyl_B.retracted }

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
            Some(OnCompleteDirective::Goto { ref step }) if step == "ready"
        ));

        assert!(matches!(
            init_task.steps[0].statements.first(),
            Some(StepStatement::Action(ActionStatement::Extend { target })) if target == "cyl_A"
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
    connected_to: X2
    debounce: 20ms
}

device motor_ctrl: motor {
    connected_to: Y0
    rated_speed: 60rpm
    ramp_time: 50ms                      # 启动到额定转速时间
}

device sensor_A: sensor {
    type: proximity
    connected_to: X0
    detects: motor_ctrl.position_A       # 检测A位置
}

device sensor_B: sensor {
    type: proximity
    connected_to: X1
    detects: motor_ctrl.position_B       # 检测B位置
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
            Some(OnCompleteDirective::Goto { ref step }) if step == "search"
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
}
