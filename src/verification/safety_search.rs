fn build_depth_plan(model: &SafetyModel, config: &SafetyConfig) -> DepthPlan {
    let target_depth = model.suggested_depth;
    let mut warnings = Vec::new();
    let mut truncated = false;

    let effective_depth = if let Some(user_limit) = config.bmc_max_depth {
        if user_limit < target_depth {
            truncated = true;
            let reason = if model.max_scc_depth > 0 && user_limit < model.max_scc_depth {
                format!(
                    "WARNING: bmc_max_depth={} 小于 SCC 建议深度 {}，Safety 搜索将截断至 {}（有界验证）",
                    user_limit, model.max_scc_depth, user_limit
                )
            } else {
                format!(
                    "WARNING: bmc_max_depth={} 小于建议展开深度 {}，Safety 搜索将截断至 {}（有界验证）",
                    user_limit, target_depth, user_limit
                )
            };
            warnings.push(reason);
            user_limit
        } else {
            user_limit
        }
    } else {
        target_depth
    };

    DepthPlan {
        effective_depth: effective_depth.max(1),
        warnings,
        truncated,
    }
}

fn bind_safety_expr_rule_with_reason(
    model: &SafetyModel,
    rule: &crate::ir::SafetyRule,
) -> Result<RuleBinding, String> {
    let (left_device, left_states) =
        safety_expr_states_with_reason(model, &rule.left).map_err(|r| format!("左侧：{r}"))?;
    let (right_device, right_states) =
        safety_expr_states_with_reason(model, &rule.right).map_err(|r| format!("右侧：{r}"))?;

    Ok(RuleBinding {
        relation: rule.relation.clone(),
        left_device,
        left_states,
        right_device,
        right_states,
    })
}

fn safety_expr_states_with_reason(
    model: &SafetyModel,
    expr: &SafetyExpr,
) -> Result<(usize, Vec<usize>), String> {
    match expr {
        SafetyExpr::State(state_expr) => {
            let device_id = lookup_device_domain_id(
                &model.device_index,
                &state_expr.device,
                &state_expr.port,
                false,
            )
            .ok_or_else(|| {
                if state_expr.port == "self" {
                    format!("未知设备 {}", state_expr.device)
                } else {
                    format!("未知设备端口 {}.{}", state_expr.device, state_expr.port)
                }
            })?;
            let state_id = model.device_state_index[device_id]
                .get(&state_expr.state)
                .copied()
                .ok_or_else(|| {
                    if state_expr.port == "self" {
                        format!("设备 {} 不存在状态 {}", state_expr.device, state_expr.state)
                    } else {
                        format!(
                            "设备端口 {}.{} 不存在状态 {}",
                            state_expr.device, state_expr.port, state_expr.state
                        )
                    }
                })?;
            Ok((device_id, vec![state_id]))
        }
        SafetyExpr::Threshold {
            device,
            operator,
            value,
        } => {
            let (device_name, port_name) = split_threshold_target(device);
            let device_id =
                lookup_device_domain_id(&model.device_index, device_name, port_name, false)
                    .ok_or_else(|| {
                        if port_name == "self" {
                            format!("未知设备 {device_name}")
                        } else {
                            format!("未知设备端口 {device_name}.{port_name}")
                        }
                    })?;
            let domain = model
                .devices
                .get(device_id)
                .ok_or_else(|| format!("内部错误：设备 {device_name} 未注册"))?;
            if !domain.is_analog {
                return Err(format!("设备 {device} 非模拟量设备，无法进行阈值建模"));
            }
            if domain.region_bounds.is_none() {
                return Err(format!("设备 {device} 缺少 range，无法进行区间离散建模"));
            }
            if comparison_op_from_str(operator).is_none() {
                return Err(format!("不支持的比较运算符 {operator}"));
            }
            if value.parse::<f64>().is_err() {
                return Err(format!("阈值值无法解析为数字：{value}"));
            }
            let states =
                threshold_states_for_expr(model, device_id, operator, value).ok_or_else(|| {
                    format!("无法将阈值表达式映射到离散区间：{device} {operator} {value}")
                })?;
            Ok((device_id, states))
        }
    }
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

fn comparison_op_from_str(op: &str) -> Option<ComparisonOp> {
    match op {
        "==" => Some(ComparisonOp::Eq),
        "!=" => Some(ComparisonOp::Neq),
        ">" => Some(ComparisonOp::Gt),
        "<" => Some(ComparisonOp::Lt),
        ">=" => Some(ComparisonOp::Gte),
        "<=" => Some(ComparisonOp::Lte),
        _ => None,
    }
}

fn threshold_states_for_expr(
    model: &SafetyModel,
    device_id: usize,
    operator: &str,
    value: &str,
) -> Option<Vec<usize>> {
    let domain = model.devices.get(device_id)?;
    let bounds = domain.region_bounds.as_ref()?;
    let op = comparison_op_from_str(operator)?;
    let value = value.parse::<f64>().ok()?;

    let mut states = Vec::new();
    for (index, (min, max)) in bounds.iter().enumerate() {
        if region_intersects(op, value, *min, *max) {
            states.push(index);
        }
    }

    Some(states)
}

fn region_intersects(op: ComparisonOp, value: f64, min: f64, max: f64) -> bool {
    match op {
        ComparisonOp::Eq => value >= min && value <= max,
        ComparisonOp::Neq => !(min == max && value == min),
        ComparisonOp::Gt => max > value,
        ComparisonOp::Gte => max >= value,
        ComparisonOp::Lt => min < value,
        ComparisonOp::Lte => min <= value,
    }
}

fn safety_expr_text(expr: &SafetyExpr) -> String {
    match expr {
        SafetyExpr::State(state_expr) => {
            if state_expr.port == "self" {
                format!("{}.{}", state_expr.device, state_expr.state)
            } else {
                format!(
                    "{}.{}.{}",
                    state_expr.device, state_expr.port, state_expr.state
                )
            }
        }
        SafetyExpr::Threshold {
            device,
            operator,
            value,
        } => format!("{device} {operator} {value}"),
    }
}

fn safety_rule_has_threshold(rule: &crate::ir::SafetyRule) -> bool {
    matches!(rule.left, SafetyExpr::Threshold { .. })
        || matches!(rule.right, SafetyExpr::Threshold { .. })
}

fn collect_analog_threshold_details(
    model: &SafetyModel,
    rule: &crate::ir::SafetyRule,
) -> Vec<SafetyAnalogThresholdDetail> {
    let mut out = Vec::new();
    for expr in [&rule.left, &rule.right] {
        let SafetyExpr::Threshold {
            device,
            operator,
            value,
        } = expr
        else {
            continue;
        };

        let (device_name, port_name) = split_threshold_target(device);
        let Some(device_id) =
            lookup_device_domain_id(&model.device_index, device_name, port_name, false)
        else {
            continue;
        };
        let Some(domain) = model.devices.get(device_id) else {
            continue;
        };
        if !domain.is_analog {
            continue;
        }
        let Some(bounds) = domain.region_bounds.as_ref() else {
            continue;
        };
        let split_points = split_points_from_region_bounds(bounds);
        let hit_intervals = threshold_states_for_expr(model, device_id, operator, value)
            .map(|states| states.len())
            .unwrap_or(0);
        out.push(SafetyAnalogThresholdDetail {
            expr: safety_expr_text(expr),
            device: device.clone(),
            operator: operator.clone(),
            value: value.clone(),
            split_points,
            hit_intervals,
            total_intervals: bounds.len(),
        });
    }
    out
}

fn split_threshold_target(device_ref: &str) -> (&str, &str) {
    let mut parts = device_ref.split('.');
    let Some(device) = parts.next() else {
        return (device_ref, "self");
    };
    let Some(port) = parts.next() else {
        return (device_ref, "self");
    };
    if parts.next().is_some() {
        return (device_ref, "self");
    }
    (device, port)
}

fn split_points_from_region_bounds(bounds: &[(f64, f64)]) -> Vec<f64> {
    if bounds.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(bounds.len() + 1);
    out.push(bounds[0].0);
    for (_, max) in bounds {
        out.push(*max);
    }
    out
}

fn explore_state_space(model: &SafetyModel, max_depth: usize) -> SearchSpace {
    let initial_state = initial_concrete_state(model);
    let mut nodes = vec![SearchNode {
        state: initial_state.clone(),
        depth: 0,
        parent: None,
        via_edge: None,
    }];
    let mut queue = VecDeque::from([0usize]);
    let mut shortest_depth = HashMap::<ConcreteState, usize>::new();
    shortest_depth.insert(initial_state, 0);

    let mut fully_explored = true;

    while let Some(node_id) = queue.pop_front() {
        let node_depth = nodes[node_id].depth;
        let node_state = nodes[node_id].state.clone();

        for (task_slot, &control_state) in node_state.task_states.iter().enumerate() {
            let outgoing = materialized_successors(model, &node_state, task_slot, control_state);
            if node_depth == max_depth {
                for (candidate, _) in outgoing {
                    if !shortest_depth.contains_key(&candidate) {
                        fully_explored = false;
                    }
                }
                continue;
            }

            for (next_state, via_step) in outgoing {
                let next_depth = node_depth + 1;

                if shortest_depth
                    .get(&next_state)
                    .is_some_and(|depth| *depth <= next_depth)
                {
                    continue;
                }

                shortest_depth.insert(next_state.clone(), next_depth);
                let next_id = nodes.len();
                nodes.push(SearchNode {
                    state: next_state,
                    depth: next_depth,
                    parent: Some(node_id),
                    via_edge: Some(via_step),
                });
                queue.push_back(next_id);
            }
        }
    }

    SearchSpace {
        nodes,
        fully_explored,
    }
}

fn materialized_successors(
    model: &SafetyModel,
    current: &ConcreteState,
    task_slot: usize,
    control_state: usize,
) -> Vec<(ConcreteState, TransitionStep)> {
    let mut results = Vec::new();
    let mut queue = VecDeque::from([(control_state, current.clone())]);
    let mut visited = HashSet::<usize>::from([control_state]);

    while let Some((state_id, state)) = queue.pop_front() {
        let Some(outgoing) = model.outgoing.get(state_id) else {
            continue;
        };

        for &edge_id in outgoing {
            let edge = &model.edges[edge_id];
            if !guard_allows_edge(model, &state, &edge.guard) {
                continue;
            }
            let next_state = apply_edge(model, edge, &state, task_slot);
            let next_control_state = next_state
                .task_states
                .get(task_slot)
                .copied()
                .unwrap_or(edge.to);

            if edge_is_material(model, edge, next_control_state) {
                results.push((next_state, TransitionStep { task_slot, edge_id }));
                continue;
            }

            if visited.insert(next_control_state) {
                queue.push_back((next_control_state, next_state));
            }
        }
    }

    results
}

fn edge_is_material(model: &SafetyModel, edge: &ModelEdge, next_control_state: usize) -> bool {
    if !edge.effects.is_empty()
        || !edge.variable_effects.is_empty()
        || !edge.analog_expr_effects.is_empty()
    {
        return true;
    }

    model
        .pending_action_tags
        .get(&next_control_state)
        .is_some_and(|tags| !tags.is_empty())
}

fn analyze_rule(
    model: &SafetyModel,
    search_space: &SearchSpace,
    rule: RuleBinding,
) -> SearchOutcome {
    for (node_id, node) in search_space.nodes.iter().enumerate() {
        if violates_rule(&node.state, &rule) {
            let path = render_path(model, &search_space.nodes, node_id, &rule);
            return SearchOutcome {
                counterexample: Some(Counterexample { path }),
                fully_explored: search_space.fully_explored,
            };
        }
    }

    SearchOutcome {
        counterexample: None,
        fully_explored: search_space.fully_explored,
    }
}

fn check_semantic_resource_interlocks(
    program: &PlcProgram,
    constraints: &ConstraintSet,
    model: &SafetyModel,
    search_space: &SearchSpace,
) -> Vec<SafetyDiagnostic> {
    if constraints.semantic_resources.is_empty() || constraints.resource_claims.is_empty() {
        return Vec::new();
    }

    let Some(counterexample) =
        find_semantic_resource_counterexample(model, constraints, search_space)
    else {
        return Vec::new();
    };

    let line = counterexample
        .holders
        .iter()
        .filter_map(|holder| {
            program
                .constraints
                .claims
                .get(holder.claim_index)
                .map(|claim| claim.line.max(1))
        })
        .min()
        .unwrap_or(1);
    let holders_text = counterexample
        .holders
        .iter()
        .map(|holder| holder.description.clone())
        .collect::<Vec<_>>()
        .join(", ");

    vec![SafetyDiagnostic {
        line,
        constraint: format!(
            "semantic_resource {} exclusive",
            counterexample.resource_name
        ),
        reason: format!(
            "semantic resource '{}' is occupied simultaneously by {}",
            counterexample.resource_name, holders_text
        ),
        violation_path: counterexample.path,
        suggestion: format!(
            "请让这些 claim 不在同一可达状态同时成立，或拆分资源 `{}`",
            counterexample.resource_name
        ),
    }]
}

fn find_semantic_resource_counterexample(
    model: &SafetyModel,
    constraints: &ConstraintSet,
    search_space: &SearchSpace,
) -> Option<SemanticResourceCounterexample> {
    for (node_id, node) in search_space.nodes.iter().enumerate() {
        if let Some((resource_name, holders)) =
            semantic_resource_conflict_in_state(model, constraints, &node.state)
        {
            let path = render_semantic_resource_path(
                model,
                &search_space.nodes,
                node_id,
                &resource_name,
                &holders,
            );
            return Some(SemanticResourceCounterexample {
                resource_name,
                holders,
                path,
            });
        }
    }

    None
}

fn semantic_resource_conflict_in_state(
    model: &SafetyModel,
    constraints: &ConstraintSet,
    state: &ConcreteState,
) -> Option<(String, Vec<SemanticResourceHolder>)> {
    for resource in &constraints.semantic_resources {
        if !matches!(resource.mode, crate::ir::SemanticResourceMode::Exclusive) {
            continue;
        }

        let mut holders = Vec::new();
        for (claim_index, claim) in constraints.resource_claims.iter().enumerate() {
            if claim.resource != resource.name {
                continue;
            }
            holders.extend(active_semantic_resource_holders(
                model,
                state,
                claim_index,
                &claim.source,
            ));
            if holders.len() > 1 {
                return Some((resource.name.clone(), holders));
            }
        }
    }
    None
}

fn active_semantic_resource_holders(
    model: &SafetyModel,
    state: &ConcreteState,
    claim_index: usize,
    source: &crate::ir::ResourceClaimSource,
) -> Vec<SemanticResourceHolder> {
    match source {
        crate::ir::ResourceClaimSource::State(state_expr) => {
            if state_claim_matches(model, state, state_expr) {
                vec![SemanticResourceHolder {
                    claim_index,
                    description: render_state_expr_text(state_expr),
                }]
            } else {
                Vec::new()
            }
        }
        crate::ir::ResourceClaimSource::ActionTag { tag } => {
            let mut holders = Vec::new();
            for (task_slot, state_id) in state.task_states.iter().enumerate() {
                if !state.task_pending.get(task_slot).copied().unwrap_or(false) {
                    continue;
                }
                let Some(tags) = model.pending_action_tags.get(state_id) else {
                    continue;
                };
                if !tags.iter().any(|candidate| candidate == tag) {
                    continue;
                }
                let task_name = model
                    .active_task_names
                    .get(task_slot)
                    .cloned()
                    .unwrap_or_else(|| format!("task_{task_slot}"));
                holders.push(SemanticResourceHolder {
                    claim_index,
                    description: format!("action_tag {} (task={})", tag, task_name),
                });
            }
            holders
        }
    }
}

fn state_claim_matches(model: &SafetyModel, state: &ConcreteState, state_expr: &StateExpr) -> bool {
    let Some(device_id) = lookup_device_domain_id(
        &model.device_index,
        &state_expr.device,
        &state_expr.port,
        false,
    ) else {
        return false;
    };
    let Some(expected_state) = model.device_state_index[device_id]
        .get(&state_expr.state)
        .copied()
    else {
        return false;
    };
    state
        .device_states
        .get(device_id)
        .copied()
        .is_some_and(|actual| actual == expected_state)
}

fn render_state_expr_text(state_expr: &StateExpr) -> String {
    if state_expr.port == "self" {
        format!("{}.{}", state_expr.device, state_expr.state)
    } else {
        format!(
            "{}.{}.{}",
            state_expr.device, state_expr.port, state_expr.state
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExprToken<'a> {
    Number(&'a str),
    Bool(bool),
    Ident(&'a str),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,
    NotEq,
    Gt,
    Lt,
    Ge,
    Le,
    And,
    Or,
    Not,
    LParen,
    RParen,
}

fn guard_allows_edge(model: &SafetyModel, state: &ConcreteState, guard: &ModelGuard) -> bool {
    match guard {
        ModelGuard::Always | ModelGuard::Timeout | ModelGuard::Delay | ModelGuard::Unsupported => {
            true
        }
        ModelGuard::AnalogRegions {
            device_id,
            allowed_states,
        } => state
            .device_states
            .get(*device_id)
            .copied()
            .is_some_and(|actual| allowed_states.contains(&actual)),
        ModelGuard::DeviceState {
            device_id,
            expected_state,
            equals,
        } => state
            .device_states
            .get(*device_id)
            .copied()
            .is_some_and(|actual| (actual == *expected_state) == *equals),
        ModelGuard::VariableBool {
            variable_id,
            equals,
        } => state
            .variable_values
            .get(*variable_id)
            .copied()
            .and_then(SafetyValue::as_bool)
            .is_some_and(|actual| actual == *equals),
        ModelGuard::Expr(expr) => eval_model_expr(model, state, expr)
            .and_then(SafetyValue::as_bool)
            .unwrap_or(true),
    }
}

fn eval_model_expr(
    _model: &SafetyModel,
    state: &ConcreteState,
    expr: &ModelExpr,
) -> Option<SafetyValue> {
    match expr {
        ModelExpr::Literal(value) => Some(*value),
        ModelExpr::Variable(variable_id) => state.variable_values.get(*variable_id).copied(),
        ModelExpr::UnaryNeg(inner) => Some(SafetyValue::number(
            -eval_model_expr(_model, state, inner)?.as_f32(),
        )),
        ModelExpr::UnaryNot(inner) => {
            let value = eval_model_expr(_model, state, inner)?.as_bool()?;
            Some(SafetyValue::bool(!value))
        }
        ModelExpr::Binary { op, left, right } => {
            let left = eval_model_expr(_model, state, left)?;
            let right = eval_model_expr(_model, state, right)?;
            match op {
                ModelBinaryOp::Add => Some(SafetyValue::number(left.as_f32() + right.as_f32())),
                ModelBinaryOp::Sub => Some(SafetyValue::number(left.as_f32() - right.as_f32())),
                ModelBinaryOp::Mul => Some(SafetyValue::number(left.as_f32() * right.as_f32())),
                ModelBinaryOp::Div => {
                    let rhs = right.as_f32();
                    if rhs.abs() <= f32::EPSILON {
                        None
                    } else {
                        Some(SafetyValue::number(left.as_f32() / rhs))
                    }
                }
                ModelBinaryOp::Mod => {
                    let rhs = right.as_f32();
                    if rhs.abs() <= f32::EPSILON {
                        None
                    } else {
                        Some(SafetyValue::number(left.as_f32() % rhs))
                    }
                }
                ModelBinaryOp::Eq => {
                    Some(SafetyValue::bool(compare_safety_values_eq(left, right)?))
                }
                ModelBinaryOp::Neq => {
                    Some(SafetyValue::bool(!compare_safety_values_eq(left, right)?))
                }
                ModelBinaryOp::Gt => Some(SafetyValue::bool(left.as_f32() > right.as_f32())),
                ModelBinaryOp::Lt => Some(SafetyValue::bool(left.as_f32() < right.as_f32())),
                ModelBinaryOp::Gte => Some(SafetyValue::bool(left.as_f32() >= right.as_f32())),
                ModelBinaryOp::Lte => Some(SafetyValue::bool(left.as_f32() <= right.as_f32())),
                ModelBinaryOp::And => Some(SafetyValue::bool(left.as_bool()? && right.as_bool()?)),
                ModelBinaryOp::Or => Some(SafetyValue::bool(left.as_bool()? || right.as_bool()?)),
            }
        }
    }
}

fn compare_safety_values_eq(left: SafetyValue, right: SafetyValue) -> Option<bool> {
    match (left, right) {
        (SafetyValue::Bool(left), SafetyValue::Bool(right)) => Some(left == right),
        (left, right) => Some((left.as_f32() - right.as_f32()).abs() <= f32::EPSILON),
    }
}

fn render_safety_value(value: SafetyValue) -> String {
    match value {
        SafetyValue::Bool(value) => value.to_string(),
        SafetyValue::Number(bits) => {
            let value = f32::from_bits(bits);
            if value.fract().abs() <= f32::EPSILON {
                format!("{}", value as i64)
            } else {
                value.to_string()
            }
        }
    }
}

fn coerce_safety_value_for_type(value: SafetyValue, var_type: &AstVariableType) -> SafetyValue {
    match var_type {
        AstVariableType::Bool => SafetyValue::bool(value.as_bool().unwrap_or(false)),
        AstVariableType::Float => SafetyValue::number(value.as_f32()),
        AstVariableType::Int => SafetyValue::number(value.as_f32().trunc()),
    }
}

fn parse_model_analog_region_guard(expr: &str) -> Option<(String, Vec<usize>)> {
    let (lhs, rhs) = expr.split_once(" in ")?;
    let device = lhs.trim();
    if device.is_empty() || device.contains('.') {
        return None;
    }
    let rhs = rhs.trim();
    let rhs = rhs.strip_prefix('{')?.strip_suffix('}')?.trim();
    if rhs.is_empty() {
        return None;
    }

    let mut out = Vec::new();
    for token in rhs.split(',') {
        let token = token.trim();
        let idx = token.strip_prefix("region_")?.parse::<usize>().ok()?;
        out.push(idx);
    }
    Some((device.to_string(), out))
}

fn parse_model_bool_guard(expr: &str) -> Option<(String, bool)> {
    let parts = expr.split_whitespace().collect::<Vec<_>>();
    if parts.len() != 3 {
        return None;
    }
    let lhs = parts[0].trim();
    let op = parts[1].trim();
    let rhs = parts[2].trim();
    if lhs.is_empty() || !(op == "==" || op == "!=") {
        return None;
    }
    let rhs_value = match rhs {
        "true" => true,
        "false" => false,
        _ => return None,
    };
    let equals = if op == "==" { rhs_value } else { !rhs_value };
    Some((lhs.to_string(), equals))
}

fn compile_model_expr(raw: &str, variable_index: &HashMap<String, usize>) -> Option<ModelExpr> {
    let tokens = tokenize_model_expr(raw).ok()?;
    let mut parser = ModelExprParser {
        tokens,
        pos: 0,
        variable_index,
    };
    let expr = parser.parse_expression().ok()?;
    if parser.pos == parser.tokens.len() {
        Some(expr)
    } else {
        None
    }
}

fn tokenize_model_expr(raw: &str) -> Result<Vec<ExprToken<'_>>, ()> {
    let mut out = Vec::new();
    let bytes = raw.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let ch = bytes[i] as char;
        if ch.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        if ch.is_ascii_digit() || ch == '.' {
            let start = i;
            i += 1;
            while i < bytes.len() {
                let current = bytes[i] as char;
                if current.is_ascii_digit() || current == '.' {
                    i += 1;
                } else {
                    break;
                }
            }
            out.push(ExprToken::Number(&raw[start..i]));
            continue;
        }

        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = i;
            i += 1;
            while i < bytes.len() {
                let current = bytes[i] as char;
                if current.is_ascii_alphanumeric() || current == '_' {
                    i += 1;
                } else {
                    break;
                }
            }
            let word = &raw[start..i];
            match word.to_ascii_lowercase().as_str() {
                "true" => out.push(ExprToken::Bool(true)),
                "false" => out.push(ExprToken::Bool(false)),
                "and" => out.push(ExprToken::And),
                "or" => out.push(ExprToken::Or),
                "not" => out.push(ExprToken::Not),
                _ => out.push(ExprToken::Ident(word)),
            }
            continue;
        }

        if i + 1 < bytes.len() {
            match &raw[i..i + 2] {
                "==" => {
                    out.push(ExprToken::EqEq);
                    i += 2;
                    continue;
                }
                "!=" | "<>" => {
                    out.push(ExprToken::NotEq);
                    i += 2;
                    continue;
                }
                ">=" => {
                    out.push(ExprToken::Ge);
                    i += 2;
                    continue;
                }
                "<=" => {
                    out.push(ExprToken::Le);
                    i += 2;
                    continue;
                }
                "&&" => {
                    out.push(ExprToken::And);
                    i += 2;
                    continue;
                }
                "||" => {
                    out.push(ExprToken::Or);
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }

        match ch {
            '(' => out.push(ExprToken::LParen),
            ')' => out.push(ExprToken::RParen),
            '+' => out.push(ExprToken::Plus),
            '-' => out.push(ExprToken::Minus),
            '*' => out.push(ExprToken::Star),
            '/' => out.push(ExprToken::Slash),
            '%' => out.push(ExprToken::Percent),
            '>' => out.push(ExprToken::Gt),
            '<' => out.push(ExprToken::Lt),
            '!' => out.push(ExprToken::Not),
            _ => return Err(()),
        }
        i += 1;
    }

    Ok(out)
}

struct ModelExprParser<'a> {
    tokens: Vec<ExprToken<'a>>,
    pos: usize,
    variable_index: &'a HashMap<String, usize>,
}

impl<'a> ModelExprParser<'a> {
    fn parse_expression(&mut self) -> Result<ModelExpr, ()> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<ModelExpr, ()> {
        let mut expr = self.parse_and()?;
        while self.consume_if(ExprToken::Or) {
            let right = self.parse_and()?;
            expr = ModelExpr::Binary {
                op: ModelBinaryOp::Or,
                left: Box::new(expr),
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_and(&mut self) -> Result<ModelExpr, ()> {
        let mut expr = self.parse_comparison()?;
        while self.consume_if(ExprToken::And) {
            let right = self.parse_comparison()?;
            expr = ModelExpr::Binary {
                op: ModelBinaryOp::And,
                left: Box::new(expr),
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_comparison(&mut self) -> Result<ModelExpr, ()> {
        let mut expr = self.parse_additive()?;
        loop {
            let op = if self.consume_if(ExprToken::EqEq) {
                Some(ModelBinaryOp::Eq)
            } else if self.consume_if(ExprToken::NotEq) {
                Some(ModelBinaryOp::Neq)
            } else if self.consume_if(ExprToken::Ge) {
                Some(ModelBinaryOp::Gte)
            } else if self.consume_if(ExprToken::Le) {
                Some(ModelBinaryOp::Lte)
            } else if self.consume_if(ExprToken::Gt) {
                Some(ModelBinaryOp::Gt)
            } else if self.consume_if(ExprToken::Lt) {
                Some(ModelBinaryOp::Lt)
            } else {
                None
            };
            let Some(op) = op else {
                break;
            };
            let right = self.parse_additive()?;
            expr = ModelExpr::Binary {
                op,
                left: Box::new(expr),
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_additive(&mut self) -> Result<ModelExpr, ()> {
        let mut expr = self.parse_multiplicative()?;
        loop {
            let op = if self.consume_if(ExprToken::Plus) {
                Some(ModelBinaryOp::Add)
            } else if self.consume_if(ExprToken::Minus) {
                Some(ModelBinaryOp::Sub)
            } else {
                None
            };
            let Some(op) = op else {
                break;
            };
            let right = self.parse_multiplicative()?;
            expr = ModelExpr::Binary {
                op,
                left: Box::new(expr),
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_multiplicative(&mut self) -> Result<ModelExpr, ()> {
        let mut expr = self.parse_unary()?;
        loop {
            let op = if self.consume_if(ExprToken::Star) {
                Some(ModelBinaryOp::Mul)
            } else if self.consume_if(ExprToken::Slash) {
                Some(ModelBinaryOp::Div)
            } else if self.consume_if(ExprToken::Percent) {
                Some(ModelBinaryOp::Mod)
            } else {
                None
            };
            let Some(op) = op else {
                break;
            };
            let right = self.parse_unary()?;
            expr = ModelExpr::Binary {
                op,
                left: Box::new(expr),
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_unary(&mut self) -> Result<ModelExpr, ()> {
        if self.consume_if(ExprToken::Minus) {
            return Ok(ModelExpr::UnaryNeg(Box::new(self.parse_unary()?)));
        }
        if self.consume_if(ExprToken::Not) {
            return Ok(ModelExpr::UnaryNot(Box::new(self.parse_unary()?)));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<ModelExpr, ()> {
        match self.peek().copied() {
            Some(ExprToken::Number(raw)) => {
                self.pos += 1;
                raw.parse::<f32>()
                    .ok()
                    .map(SafetyValue::number)
                    .map(ModelExpr::Literal)
                    .ok_or(())
            }
            Some(ExprToken::Bool(value)) => {
                self.pos += 1;
                Ok(ModelExpr::Literal(SafetyValue::bool(value)))
            }
            Some(ExprToken::Ident(name)) => {
                self.pos += 1;
                let variable_id = self.variable_index.get(name).copied().ok_or(())?;
                Ok(ModelExpr::Variable(variable_id))
            }
            Some(ExprToken::LParen) => {
                self.pos += 1;
                let expr = self.parse_expression()?;
                if !self.consume_if(ExprToken::RParen) {
                    return Err(());
                }
                Ok(expr)
            }
            _ => Err(()),
        }
    }

    fn peek(&self) -> Option<&ExprToken<'a>> {
        self.tokens.get(self.pos)
    }

    fn consume_if(&mut self, token: ExprToken<'a>) -> bool {
        if self.peek().is_some_and(|candidate| *candidate == token) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
}

fn render_semantic_resource_path(
    model: &SafetyModel,
    nodes: &[SearchNode],
    terminal_node: usize,
    resource_name: &str,
    holders: &[SemanticResourceHolder],
) -> Vec<String> {
    let mut order = Vec::new();
    let mut cursor = Some(terminal_node);
    while let Some(node_id) = cursor {
        order.push(node_id);
        cursor = nodes[node_id].parent;
    }
    order.reverse();

    let initial = &nodes[order[0]].state;
    let mut lines = vec![format!(
        "初始状态 {}",
        render_global_state_name(model, initial)
    )];

    for window in order.windows(2) {
        let from = &nodes[window[0]].state;
        let to_node = &nodes[window[1]];

        let step = to_node.via_edge.unwrap_or_else(|| {
            let fallback_task_slot = 0usize;
            let fallback_control_state = from.task_states.first().copied().unwrap_or(0);
            let fallback_edge = model
                .outgoing
                .get(fallback_control_state)
                .and_then(|edges| edges.first())
                .copied()
                .unwrap_or(0);
            TransitionStep {
                task_slot: fallback_task_slot,
                edge_id: fallback_edge,
            }
        });
        let edge = &model.edges[step.edge_id];
        let from_state_id = from
            .task_states
            .get(step.task_slot)
            .copied()
            .unwrap_or(edge.from);
        let to_state_id = to_node
            .state
            .task_states
            .get(step.task_slot)
            .copied()
            .unwrap_or(edge.to);
        let from_name = state_name(&model.states[from_state_id]);
        let to_name = state_name(&model.states[to_state_id]);
        let task_name = model
            .active_task_names
            .get(step.task_slot)
            .cloned()
            .unwrap_or_else(|| model.states[to_state_id].task_name.clone());
        lines.push(format!(
            "{from_name} --[{}]--> {to_name} (task={task_name})",
            edge.label
        ));
    }

    lines.push(format!(
        "在 {} 检测到资源 `{}` 冲突：{}",
        render_global_state_name(model, &nodes[terminal_node].state),
        resource_name,
        holders
            .iter()
            .map(|holder| holder.description.clone())
            .collect::<Vec<_>>()
            .join(", ")
    ));

    lines
}

fn initial_concrete_state(model: &SafetyModel) -> ConcreteState {
    let device_states = model
        .devices
        .iter()
        .map(|device| device.default_state)
        .collect::<Vec<_>>();
    let variable_values = model
        .variables
        .iter()
        .map(|variable| variable.initial_value)
        .collect::<Vec<_>>();
    let mut task_states = model.active_task_entries.clone();
    if task_states.is_empty() {
        task_states.push(model.initial_state);
    }
    let task_pending = task_states
        .iter()
        .map(|state_id| model.pending_source_states.contains(state_id))
        .collect::<Vec<_>>();

    ConcreteState {
        task_states,
        task_pending,
        device_states,
        variable_values,
    }
}

fn apply_edge(
    model: &SafetyModel,
    edge: &ModelEdge,
    current: &ConcreteState,
    task_slot: usize,
) -> ConcreteState {
    let mut device_states = current.device_states.clone();
    for (&device_id, &state_id) in &edge.effects {
        if device_id < device_states.len() {
            device_states[device_id] = state_id;
        }
    }
    for effect in &edge.analog_expr_effects {
        let Some(value) = eval_model_expr(model, current, &effect.expr) else {
            continue;
        };
        let analog_value = value.as_f32().to_string();
        let Some(state_id) =
            analog_state_for_value(&model.devices, effect.device_id, &analog_value)
        else {
            continue;
        };
        if effect.device_id < device_states.len() {
            device_states[effect.device_id] = state_id;
        }
    }

    let mut variable_values = current.variable_values.clone();
    for effect in &edge.variable_effects {
        let Some(value) = eval_model_expr(model, current, &effect.expr) else {
            continue;
        };
        if effect.variable_id < variable_values.len() {
            variable_values[effect.variable_id] = model
                .variables
                .get(effect.variable_id)
                .map(|variable| coerce_safety_value_for_type(value, &variable.var_type))
                .unwrap_or(value);
        }
    }

    let mut task_states = current.task_states.clone();
    if task_slot < task_states.len() {
        task_states[task_slot] = edge.to;
    }
    let mut task_pending = current.task_pending.clone();
    if task_pending.len() != task_states.len() {
        task_pending.resize(task_states.len(), false);
    }
    if task_slot < task_pending.len() {
        task_pending[task_slot] = model.pending_source_states.contains(&edge.to);
    }

    ConcreteState {
        task_states,
        task_pending,
        device_states,
        variable_values,
    }
}

fn violates_rule(state: &ConcreteState, rule: &RuleBinding) -> bool {
    let left_state = state.device_states[rule.left_device];
    let right_state = state.device_states[rule.right_device];
    let left_matches = rule.left_states.contains(&left_state);
    let right_matches = rule.right_states.contains(&right_state);

    match rule.relation {
        SafetyRelation::ConflictsWith => left_matches && right_matches,
        SafetyRelation::Requires => left_matches && !right_matches,
    }
}

fn render_path(
    model: &SafetyModel,
    nodes: &[SearchNode],
    terminal_node: usize,
    rule: &RuleBinding,
) -> Vec<String> {
    let mut order = Vec::new();
    let mut cursor = Some(terminal_node);
    while let Some(node_id) = cursor {
        order.push(node_id);
        cursor = nodes[node_id].parent;
    }
    order.reverse();

    let initial = &nodes[order[0]].state;
    let mut lines = vec![format!(
        "初始状态 {}",
        render_global_state_name(model, initial)
    )];

    for window in order.windows(2) {
        let from = &nodes[window[0]].state;
        let to_node = &nodes[window[1]];
        let to = &to_node.state;

        let step = to_node.via_edge.unwrap_or_else(|| {
            let fallback_task_slot = 0usize;
            let fallback_control_state = from.task_states.first().copied().unwrap_or(0);
            let fallback_edge = model
                .outgoing
                .get(fallback_control_state)
                .and_then(|edges| edges.first())
                .copied()
                .unwrap_or(0);
            TransitionStep {
                task_slot: fallback_task_slot,
                edge_id: fallback_edge,
            }
        });
        let edge = &model.edges[step.edge_id];
        let from_state_id = from
            .task_states
            .get(step.task_slot)
            .copied()
            .unwrap_or(edge.from);
        let to_state_id = to
            .task_states
            .get(step.task_slot)
            .copied()
            .unwrap_or(edge.to);
        let from_name = state_name(&model.states[from_state_id]);
        let to_name = state_name(&model.states[to_state_id]);
        let task_name = model
            .active_task_names
            .get(step.task_slot)
            .cloned()
            .unwrap_or_else(|| model.states[to_state_id].task_name.clone());
        lines.push(format!(
            "{from_name} --[{}]--> {to_name} (task={task_name})",
            edge.label
        ));
    }

    let conflict_state = &nodes[terminal_node].state;
    let conflict_state_name = render_global_state_name(model, conflict_state);
    let left_state_id = conflict_state.device_states[rule.left_device];
    let right_state_id = conflict_state.device_states[rule.right_device];
    let left_text = format!(
        "{}.{}",
        model.devices[rule.left_device].name, model.devices[rule.left_device].states[left_state_id]
    );
    let right_text = format!(
        "{}.{}",
        model.devices[rule.right_device].name,
        model.devices[rule.right_device].states[right_state_id]
    );

    match rule.relation {
        SafetyRelation::ConflictsWith => {
            lines.push(format!(
                "在 {conflict_state_name} 检测到冲突：{left_text} 与 {right_text} 同时为真"
            ));
        }
        SafetyRelation::Requires => {
            lines.push(format!(
                "在 {conflict_state_name} 检测到依赖违反：{left_text} 为真但 {right_text} 不为真"
            ));
        }
    }

    lines
}

fn render_global_state_name(model: &SafetyModel, state: &ConcreteState) -> String {
    let mut parts = Vec::new();
    for (slot, state_id) in state.task_states.iter().enumerate() {
        let state_name_text = model
            .states
            .get(*state_id)
            .map(state_name)
            .unwrap_or_else(|| format!("unknown_state_{state_id}"));
        let task_name = model
            .active_task_names
            .get(slot)
            .cloned()
            .unwrap_or_else(|| format!("task_{slot}"));
        let pending = state.task_pending.get(slot).copied().unwrap_or(false);
        parts.push(format!(
            "{task_name}:{state_name_text}{}",
            if pending { "[pending]" } else { "" }
        ));
    }
    if !model.variables.is_empty() {
        let rendered_variables = model
            .variables
            .iter()
            .enumerate()
            .filter_map(|(index, variable)| {
                state
                    .variable_values
                    .get(index)
                    .copied()
                    .map(|value| format!("{}={}", variable.name, render_safety_value(value)))
            })
            .collect::<Vec<_>>();
        if !rendered_variables.is_empty() {
            parts.push(format!("vars[{}]", rendered_variables.join(", ")));
        }
    }
    parts.join(" | ")
}

fn state_name(state: &State) -> String {
    format!("{}.{}", state.task_name, state.step_name)
}

fn relation_text(relation: &SafetyRelation) -> &'static str {
    match relation {
        SafetyRelation::ConflictsWith => "conflicts_with",
        SafetyRelation::Requires => "requires",
    }
}

#[cfg(feature = "z3-solver")]
fn z3_sanity_probe() {
    // Keep a minimal Z3 interaction enabled behind feature-gating so this module
    // can run in toolchains without system cmake/libz3 while still supporting Z3 runs.
    let mut cfg = Config::new();
    cfg.set_model_generation(false);
    let ctx = Context::new(&cfg);
    let solver = Solver::new(&ctx);
    solver.assert(&Bool::from_bool(&ctx, true));
    let _ = solver.check() == SatResult::Sat;
}
