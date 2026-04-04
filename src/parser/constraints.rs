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

