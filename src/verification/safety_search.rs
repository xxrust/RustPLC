fn scc_minimum_depth(state_count: usize, edges: &[ModelEdge]) -> usize {
    if state_count == 0 {
        return 1;
    }

    let mut graph = DiGraph::<usize, ()>::new();
    let mut nodes = Vec::with_capacity(state_count);
    for index in 0..state_count {
        nodes.push(graph.add_node(index));
    }

    for edge in edges {
        if edge.from >= state_count || edge.to >= state_count {
            continue;
        }
        graph.add_edge(nodes[edge.from], nodes[edge.to], ());
    }

    let mut depth_requirement = 0usize;
    for component in kosaraju_scc(&graph) {
        if component.is_empty() {
            continue;
        }

        let has_cycle = component.len() > 1
            || graph
                .edges(component[0])
                .any(|edge| edge.target() == component[0]);

        if !has_cycle {
            continue;
        }

        depth_requirement = depth_requirement.max(component.len() + 1);
    }

    depth_requirement
}

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

fn analyze_rule(model: &SafetyModel, rule: RuleBinding, max_depth: usize) -> SearchOutcome {
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
        let node = nodes[node_id].clone();

        if violates_rule(&node.state, &rule) {
            let path = render_path(model, &nodes, node_id, &rule);
            return SearchOutcome {
                counterexample: Some(Counterexample { path }),
                fully_explored,
            };
        }

        for (task_slot, &control_state) in node.state.task_states.iter().enumerate() {
            let outgoing = model
                .outgoing
                .get(control_state)
                .cloned()
                .unwrap_or_default();
            if node.depth == max_depth {
                for edge_id in outgoing {
                    let edge = &model.edges[edge_id];
                    let candidate = apply_edge(model, edge, &node.state, task_slot);
                    if !shortest_depth.contains_key(&candidate) {
                        fully_explored = false;
                    }
                }
                continue;
            }

            for edge_id in outgoing {
                let edge = &model.edges[edge_id];
                let next_state = apply_edge(model, edge, &node.state, task_slot);
                let next_depth = node.depth + 1;

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
                    via_edge: Some(TransitionStep { task_slot, edge_id }),
                });
                queue.push_back(next_id);
            }
        }
    }

    SearchOutcome {
        counterexample: None,
        fully_explored,
    }
}

fn check_semantic_resource_interlocks(
    program: &PlcProgram,
    constraints: &ConstraintSet,
    model: &SafetyModel,
    max_depth: usize,
) -> Vec<SafetyDiagnostic> {
    if constraints.semantic_resources.is_empty() || constraints.resource_claims.is_empty() {
        return Vec::new();
    }

    let Some(counterexample) = find_semantic_resource_counterexample(model, constraints, max_depth)
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
    max_depth: usize,
) -> Option<SemanticResourceCounterexample> {
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

    while let Some(node_id) = queue.pop_front() {
        let node = nodes[node_id].clone();

        if let Some((resource_name, holders)) =
            semantic_resource_conflict_in_state(model, constraints, &node.state)
        {
            let path =
                render_semantic_resource_path(model, &nodes, node_id, &resource_name, &holders);
            return Some(SemanticResourceCounterexample {
                resource_name,
                holders,
                path,
            });
        }

        for (task_slot, &control_state) in node.state.task_states.iter().enumerate() {
            let outgoing = model
                .outgoing
                .get(control_state)
                .cloned()
                .unwrap_or_default();
            if node.depth == max_depth {
                continue;
            }

            for edge_id in outgoing {
                let edge = &model.edges[edge_id];
                let next_state = apply_edge(model, edge, &node.state, task_slot);
                let next_depth = node.depth + 1;

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
                    via_edge: Some(TransitionStep { task_slot, edge_id }),
                });
                queue.push_back(next_id);
            }
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
