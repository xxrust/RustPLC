impl SafetyModel {
    fn from_inputs(
        program: &PlcProgram,
        constraints: &ConstraintSet,
        state_machine: &StateMachine,
    ) -> Self {
        let mut states = state_machine.states.clone();
        if states.is_empty() {
            states.push(state_machine.initial.clone());
        }

        let mut state_index = HashMap::<(String, String), usize>::new();
        for (index, state) in states.iter().enumerate() {
            state_index.insert((state.task_name.clone(), state.step_name.clone()), index);
        }

        let initial_state = state_index
            .get(&(
                state_machine.initial.task_name.clone(),
                state_machine.initial.step_name.clone(),
            ))
            .copied()
            .unwrap_or(0);

        let (devices, device_index, device_state_index) =
            collect_device_domains(program, constraints, state_machine);
        let (variables, variable_index) = collect_variable_domains(program, state_machine);

        let mut edges = Vec::new();
        let mut outgoing = vec![Vec::new(); states.len()];

        let analog_inputs = collect_analog_input_states(program, &device_index, &devices);

        for transition in &state_machine.transitions {
            let Some(from) = state_index
                .get(&(
                    transition.from.task_name.clone(),
                    transition.from.step_name.clone(),
                ))
                .copied()
            else {
                continue;
            };
            let Some(to) = state_index
                .get(&(
                    transition.to.task_name.clone(),
                    transition.to.step_name.clone(),
                ))
                .copied()
            else {
                continue;
            };

            let guard = compile_model_guard(
                &transition.guard,
                &device_index,
                &device_state_index,
                &devices,
                &variable_index,
            );
            let effects = transition_effects(
                transition,
                &device_index,
                &device_state_index,
                &devices,
                &variable_index,
            );
            let expanded_effects = expand_analog_input_effects(effects, &analog_inputs);
            let label = transition_label(transition);

            for effects in expanded_effects {
                let edge_index = edges.len();
                edges.push(ModelEdge {
                    from,
                    to,
                    guard: guard.clone(),
                    ordered_effects: effects.ordered_effects,
                    effects: effects.device_effects,
                    variable_effects: effects.variable_effects,
                    analog_expr_effects: effects.analog_expr_effects,
                    label: label.clone(),
                });
                outgoing[from].push(edge_index);
            }
        }

        for state_id in 0..states.len() {
            if !outgoing[state_id].is_empty() {
                continue;
            }

            let edge_index = edges.len();
            edges.push(ModelEdge {
                from: state_id,
                to: state_id,
                guard: ModelGuard::Always,
                ordered_effects: Vec::new(),
                effects: HashMap::new(),
                variable_effects: Vec::new(),
                analog_expr_effects: Vec::new(),
                label: "无出边，保持当前状态".to_string(),
            });
            outgoing[state_id].push(edge_index);
        }

        merge_parallel_join_effects(&states, &mut edges);

        let task_entry_states = collect_task_entry_state_indices(state_machine, &state_index);
        let runtime_root_tasks = select_safety_root_tasks(state_machine, &task_entry_states);
        let pending_source_states = collect_pending_source_states(state_machine, &state_index);
        let pending_action_tags = collect_pending_action_tags(state_machine, &state_index);
        let relevant_device_ids = collect_relevant_device_ids(constraints, &device_index);
        let relevant_action_tags = collect_relevant_action_tags(constraints);
        let should_slice_active_tasks =
            !relevant_device_ids.is_empty() || !relevant_action_tags.is_empty();
        let mut active_task_names = Vec::new();
        let mut active_task_entries = Vec::new();
        let mut seen_task = HashSet::<String>::new();
        for task_name in runtime_root_tasks {
            if !seen_task.insert(task_name.clone()) {
                continue;
            }
            if let Some(entry_state) = task_entry_states.get(&task_name).copied() {
                if should_slice_active_tasks
                    && !task_entry_reaches_relevant_state(
                        entry_state,
                        &outgoing,
                        &edges,
                        &pending_action_tags,
                        &relevant_device_ids,
                        &relevant_action_tags,
                    )
                {
                    continue;
                }
                active_task_names.push(task_name);
                active_task_entries.push(entry_state);
            }
        }
        if active_task_entries.is_empty() {
            active_task_names.push(state_machine.initial.task_name.clone());
            active_task_entries.push(initial_state);
        }

        let reachable_state_ids =
            collect_reachable_state_ids(&active_task_entries, initial_state, &outgoing, &edges);
        let max_scc_depth = scc_minimum_depth_for_subset(&reachable_state_ids, &edges);
        let suggested_depth = reachable_state_ids.len().max(max_scc_depth).max(1);

        Self {
            states,
            initial_state,
            edges,
            outgoing,
            active_task_names,
            active_task_entries,
            pending_source_states,
            pending_action_tags,
            devices,
            device_index,
            device_state_index,
            variables,
            suggested_depth,
            max_scc_depth,
        }
    }
}

#[derive(Clone)]
struct TransitionEffects {
    ordered_effects: Vec<ModelEffect>,
    device_effects: HashMap<usize, usize>,
    variable_effects: Vec<VariableAssignment>,
    analog_expr_effects: Vec<AnalogExprEffect>,
}

fn collect_variable_domains(
    program: &PlcProgram,
    state_machine: &StateMachine,
) -> (Vec<VariableDomain>, HashMap<String, usize>) {
    let relevant = collect_relevant_variable_names(program, state_machine);
    let mut variables = Vec::new();
    let mut variable_index = HashMap::<String, usize>::new();

    for variable in &program.topology.variables {
        if !relevant.contains(&variable.name) {
            continue;
        }
        let Some(initial_value) = parse_initial_variable_value(variable) else {
            continue;
        };
        let slot = variables.len();
        variable_index.insert(variable.name.clone(), slot);
        variables.push(VariableDomain {
            name: variable.name.clone(),
            var_type: variable.var_type.clone(),
            initial_value,
        });
    }

    (variables, variable_index)
}

fn collect_relevant_variable_names(
    program: &PlcProgram,
    state_machine: &StateMachine,
) -> HashSet<String> {
    let declared = program
        .topology
        .variables
        .iter()
        .map(|variable| variable.name.clone())
        .collect::<HashSet<_>>();
    let mut relevant = HashSet::<String>::new();

    for transition in &state_machine.transitions {
        if let TransitionGuard::Condition { expression } = &transition.guard {
            collect_raw_expression_variables(expression, &declared, &mut relevant);
        }

        for action in &transition.actions {
            match action {
                TransitionAction::Compute { target, expr_raw } => {
                    if declared.contains(target) {
                        relevant.insert(target.clone());
                    }
                    collect_raw_expression_variables(expr_raw, &declared, &mut relevant);
                }
                TransitionAction::SetAnalogExpr { expr_raw, .. } => {
                    collect_raw_expression_variables(expr_raw, &declared, &mut relevant);
                }
                TransitionAction::CallExtern { args_raw, .. } => {
                    for arg in args_raw {
                        collect_raw_expression_variables(arg, &declared, &mut relevant);
                    }
                }
                TransitionAction::Extend { .. }
                | TransitionAction::Retract { .. }
                | TransitionAction::Set { .. }
                | TransitionAction::SetAnalog { .. }
                | TransitionAction::CamEngage { .. }
                | TransitionAction::CamDisengage { .. }
                | TransitionAction::CamSwitch { .. }
                | TransitionAction::CamPhase { .. }
                | TransitionAction::AxisMoveRelative { .. }
                | TransitionAction::AxisMoveAbsolute { .. }
                | TransitionAction::Log { .. } => {}
            }
        }
    }

    relevant
}

fn collect_raw_expression_variables(
    raw: &str,
    declared: &HashSet<String>,
    relevant: &mut HashSet<String>,
) {
    let mut current = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch);
            continue;
        }
        maybe_record_variable_ident(&current, declared, relevant);
        current.clear();
    }
    maybe_record_variable_ident(&current, declared, relevant);
}

fn maybe_record_variable_ident(
    ident: &str,
    declared: &HashSet<String>,
    relevant: &mut HashSet<String>,
) {
    if ident.is_empty() {
        return;
    }
    let lowered = ident.to_ascii_lowercase();
    if matches!(lowered.as_str(), "and" | "or" | "not" | "true" | "false") {
        return;
    }
    if declared.contains(ident) {
        relevant.insert(ident.to_string());
    }
}

fn parse_initial_variable_value(variable: &crate::ast::VariableDeclaration) -> Option<SafetyValue> {
    match variable.var_type {
        AstVariableType::Bool => match variable.initial_value.trim() {
            "true" => Some(SafetyValue::bool(true)),
            "false" => Some(SafetyValue::bool(false)),
            _ => None,
        },
        AstVariableType::Float => variable
            .initial_value
            .trim()
            .parse::<f32>()
            .ok()
            .map(SafetyValue::number),
        AstVariableType::Int => variable
            .initial_value
            .trim()
            .parse::<i32>()
            .ok()
            .map(|value| SafetyValue::number(value as f32)),
    }
}

fn compile_model_guard(
    guard: &TransitionGuard,
    device_index: &HashMap<(String, String), usize>,
    device_state_index: &[HashMap<String, usize>],
    device_domains: &[DeviceDomain],
    variable_index: &HashMap<String, usize>,
) -> ModelGuard {
    match guard {
        TransitionGuard::Always => ModelGuard::Always,
        TransitionGuard::Timeout { .. } => ModelGuard::Timeout,
        TransitionGuard::Delay { .. } => ModelGuard::Delay,
        TransitionGuard::Condition { expression } => compile_condition_guard(
            expression,
            device_index,
            device_state_index,
            device_domains,
            variable_index,
        ),
    }
}

fn compile_condition_guard(
    expression: &str,
    device_index: &HashMap<(String, String), usize>,
    device_state_index: &[HashMap<String, usize>],
    device_domains: &[DeviceDomain],
    variable_index: &HashMap<String, usize>,
) -> ModelGuard {
    if let Some((device, regions)) = parse_model_analog_region_guard(expression) {
        if let Some(device_id) = lookup_device_domain_id(device_index, &device, "self", false) {
            return ModelGuard::AnalogRegions {
                device_id,
                allowed_states: regions,
            };
        }
    }

    if let Some((lhs, equals)) = parse_model_bool_guard(expression) {
        if let Some(variable_id) = variable_index.get(&lhs).copied() {
            return ModelGuard::VariableBool {
                variable_id,
                equals,
            };
        }
        if let Some((device_id, expected_state)) = resolve_guard_state_operand(
            &lhs,
            equals,
            device_index,
            device_state_index,
            device_domains,
        ) {
            return ModelGuard::DeviceState {
                device_id,
                expected_state,
                equals,
            };
        }
    }

    compile_model_expr(expression, variable_index)
        .map(ModelGuard::Expr)
        .unwrap_or(ModelGuard::Unsupported)
}

fn resolve_guard_state_operand(
    raw: &str,
    equals: bool,
    device_index: &HashMap<(String, String), usize>,
    device_state_index: &[HashMap<String, usize>],
    device_domains: &[DeviceDomain],
) -> Option<(usize, usize)> {
    let parts = raw.split('.').map(str::trim).collect::<Vec<_>>();
    match parts.as_slice() {
        [device] => {
            let device_id = lookup_device_domain_id(device_index, device, "self", true)?;
            let value = if equals {
                crate::ir::BinaryValue::On
            } else {
                crate::ir::BinaryValue::Off
            };
            let state_id = binary_state_for_domain(
                &device_domains[device_id],
                &device_state_index[device_id],
                &value,
            )?;
            Some((device_id, state_id))
        }
        [device, state] => {
            let device_id = lookup_device_domain_id(device_index, device, "self", false)?;
            let state_id = device_state_index[device_id].get(*state).copied()?;
            Some((device_id, state_id))
        }
        [device, port, state] => {
            let device_id = lookup_device_domain_id(device_index, device, port, false)?;
            let state_id = device_state_index[device_id].get(*state).copied()?;
            Some((device_id, state_id))
        }
        _ => None,
    }
}

fn collect_task_entry_state_indices(
    state_machine: &StateMachine,
    state_index: &HashMap<(String, String), usize>,
) -> HashMap<String, usize> {
    let mut entry_states = HashMap::<String, usize>::new();
    for ctx in &state_machine.task_contexts {
        let key = (
            ctx.entry_state.task_name.clone(),
            ctx.entry_state.step_name.clone(),
        );
        if let Some(entry) = state_index.get(&key).copied() {
            entry_states.insert(ctx.task_name.clone(), entry);
        }
    }
    entry_states
}

fn collect_pending_source_states(
    state_machine: &StateMachine,
    state_index: &HashMap<(String, String), usize>,
) -> HashSet<usize> {
    let mut pending = HashSet::<usize>::new();
    for ctx in &state_machine.task_contexts {
        for action in &ctx.pending_actions {
            let key = (
                action.source_state.task_name.clone(),
                action.source_state.step_name.clone(),
            );
            if let Some(state_id) = state_index.get(&key).copied() {
                pending.insert(state_id);
            }
        }
    }
    pending
}

fn collect_pending_action_tags(
    state_machine: &StateMachine,
    state_index: &HashMap<(String, String), usize>,
) -> HashMap<usize, Vec<String>> {
    let mut pending_tags = HashMap::<usize, Vec<String>>::new();
    for ctx in &state_machine.task_contexts {
        for action in &ctx.pending_actions {
            let Some(tag) = action.semantic_tag.as_ref() else {
                continue;
            };
            let key = (
                action.source_state.task_name.clone(),
                action.source_state.step_name.clone(),
            );
            let Some(state_id) = state_index.get(&key).copied() else {
                continue;
            };
            let tags = pending_tags.entry(state_id).or_default();
            if !tags.iter().any(|existing| existing == tag) {
                tags.push(tag.clone());
            }
        }
    }
    pending_tags
}

fn collect_relevant_device_ids(
    constraints: &ConstraintSet,
    device_index: &HashMap<(String, String), usize>,
) -> HashSet<usize> {
    let mut relevant = HashSet::new();

    for rule in &constraints.safety {
        collect_relevant_device_ids_from_expr(&rule.left, device_index, &mut relevant);
        collect_relevant_device_ids_from_expr(&rule.right, device_index, &mut relevant);
    }

    for claim in &constraints.resource_claims {
        if let crate::ir::ResourceClaimSource::State(state_expr) = &claim.source {
            if let Some(device_id) =
                lookup_device_domain_id(device_index, &state_expr.device, &state_expr.port, false)
            {
                relevant.insert(device_id);
            }
        }
    }

    relevant
}

fn collect_relevant_device_ids_from_expr(
    expr: &SafetyExpr,
    device_index: &HashMap<(String, String), usize>,
    relevant: &mut HashSet<usize>,
) {
    match expr {
        SafetyExpr::State(state_expr) => {
            if let Some(device_id) =
                lookup_device_domain_id(device_index, &state_expr.device, &state_expr.port, false)
            {
                relevant.insert(device_id);
            }
        }
        SafetyExpr::Threshold { device, .. } => {
            let (device_name, port_name) = split_threshold_target(device);
            if let Some(device_id) =
                lookup_device_domain_id(device_index, device_name, port_name, false)
            {
                relevant.insert(device_id);
            }
        }
    }
}

fn collect_relevant_action_tags(constraints: &ConstraintSet) -> HashSet<String> {
    let mut tags = HashSet::new();
    for claim in &constraints.resource_claims {
        if let crate::ir::ResourceClaimSource::ActionTag { tag } = &claim.source {
            tags.insert(tag.clone());
        }
    }
    tags
}

fn task_entry_reaches_relevant_state(
    entry_state: usize,
    outgoing: &[Vec<usize>],
    edges: &[ModelEdge],
    pending_action_tags: &HashMap<usize, Vec<String>>,
    relevant_device_ids: &HashSet<usize>,
    relevant_action_tags: &HashSet<String>,
) -> bool {
    let mut queue = VecDeque::from([entry_state]);
    let mut visited = HashSet::from([entry_state]);

    while let Some(state_id) = queue.pop_front() {
        if state_is_relevant(
            state_id,
            outgoing,
            edges,
            pending_action_tags,
            relevant_device_ids,
            relevant_action_tags,
        ) {
            return true;
        }

        let Some(edge_ids) = outgoing.get(state_id) else {
            continue;
        };
        for &edge_id in edge_ids {
            let Some(edge) = edges.get(edge_id) else {
                continue;
            };
            if visited.insert(edge.to) {
                queue.push_back(edge.to);
            }
        }
    }

    false
}

fn state_is_relevant(
    state_id: usize,
    outgoing: &[Vec<usize>],
    edges: &[ModelEdge],
    pending_action_tags: &HashMap<usize, Vec<String>>,
    relevant_device_ids: &HashSet<usize>,
    relevant_action_tags: &HashSet<String>,
) -> bool {
    if let Some(tags) = pending_action_tags.get(&state_id) {
        if tags.iter().any(|tag| relevant_action_tags.contains(tag)) {
            return true;
        }
    }

    outgoing
        .get(state_id)
        .into_iter()
        .flatten()
        .filter_map(|edge_id| edges.get(*edge_id))
        .any(|edge| {
            edge.effects
                .keys()
                .chain(
                    edge.analog_expr_effects
                        .iter()
                        .map(|effect| &effect.device_id),
                )
                .any(|device_id| relevant_device_ids.contains(device_id))
        })
}

fn collect_reachable_state_ids(
    active_task_entries: &[usize],
    initial_state: usize,
    outgoing: &[Vec<usize>],
    edges: &[ModelEdge],
) -> HashSet<usize> {
    let mut roots = active_task_entries.to_vec();
    if roots.is_empty() {
        roots.push(initial_state);
    }

    let mut queue = VecDeque::from(roots.clone());
    let mut visited = roots.into_iter().collect::<HashSet<_>>();

    while let Some(state_id) = queue.pop_front() {
        let Some(edge_ids) = outgoing.get(state_id) else {
            continue;
        };
        for &edge_id in edge_ids {
            let Some(edge) = edges.get(edge_id) else {
                continue;
            };
            if visited.insert(edge.to) {
                queue.push_back(edge.to);
            }
        }
    }

    visited
}

fn scc_minimum_depth_for_subset(state_ids: &HashSet<usize>, edges: &[ModelEdge]) -> usize {
    if state_ids.is_empty() {
        return 1;
    }

    let mut graph = DiGraph::<usize, ()>::new();
    let mut nodes = HashMap::<usize, _>::new();
    for &state_id in state_ids {
        nodes.insert(state_id, graph.add_node(state_id));
    }

    for edge in edges {
        let Some(&from) = nodes.get(&edge.from) else {
            continue;
        };
        let Some(&to) = nodes.get(&edge.to) else {
            continue;
        };
        graph.add_edge(from, to, ());
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

fn select_safety_root_tasks(
    state_machine: &StateMachine,
    task_entry_states: &HashMap<String, usize>,
) -> Vec<String> {
    let mut cross_task_incoming = HashSet::<String>::new();
    for transition in &state_machine.transitions {
        if transition.from.task_name != transition.to.task_name {
            cross_task_incoming.insert(transition.to.task_name.clone());
        }
        for target_task in axis_branch_target_task_names(&transition.actions) {
            if transition.from.task_name != target_task {
                cross_task_incoming.insert(target_task);
            }
        }
    }

    let mut roots = Vec::new();
    for ctx in &state_machine.task_contexts {
        if task_entry_states.contains_key(&ctx.task_name)
            && !cross_task_incoming.contains(&ctx.task_name)
        {
            roots.push(ctx.task_name.clone());
        }
    }

    if roots.is_empty() {
        if task_entry_states.contains_key(&state_machine.initial.task_name) {
            roots.push(state_machine.initial.task_name.clone());
        } else if let Some(first) = state_machine.task_contexts.first() {
            roots.push(first.task_name.clone());
        }
    }

    roots
}

fn axis_branch_target_task_names(actions: &[TransitionAction]) -> Vec<String> {
    let mut targets = Vec::new();
    for action in actions {
        match action {
            TransitionAction::AxisMoveRelative {
                timeout,
                on_reject,
                on_motion_fault,
                on_safety_fault,
                on_reject_routes,
                on_motion_fault_routes,
                on_safety_fault_routes,
                ..
            }
            | TransitionAction::AxisMoveAbsolute {
                timeout,
                on_reject,
                on_motion_fault,
                on_safety_fault,
                on_reject_routes,
                on_motion_fault_routes,
                on_safety_fault_routes,
                ..
            } => {
                targets.push(timeout.target_task.clone());
                targets.push(on_reject.target_task.clone());
                targets.push(on_motion_fault.target_task.clone());
                targets.push(on_safety_fault.target_task.clone());
                targets.extend(
                    on_reject_routes
                        .iter()
                        .map(|route| route.target_task.clone()),
                );
                targets.extend(
                    on_motion_fault_routes
                        .iter()
                        .map(|route| route.target_task.clone()),
                );
                targets.extend(
                    on_safety_fault_routes
                        .iter()
                        .map(|route| route.target_task.clone()),
                );
            }
            _ => {}
        }
    }
    targets
}

fn merge_parallel_join_effects(states: &[State], edges: &mut [ModelEdge]) {
    let mut join_effects = HashMap::<usize, HashMap<usize, usize>>::new();
    let mut join_variable_effects = HashMap::<usize, HashMap<usize, ModelExpr>>::new();
    let mut join_analog_expr_effects = HashMap::<usize, HashMap<usize, ModelExpr>>::new();

    for edge in edges.iter() {
        if !is_parallel_branch_state(states.get(edge.from))
            || !is_parallel_join_state(states.get(edge.to))
        {
            continue;
        }

        let merged = join_effects.entry(edge.to).or_default();
        for (&device_id, &state_id) in &edge.effects {
            merged.insert(device_id, state_id);
        }
        let merged_variables = join_variable_effects.entry(edge.to).or_default();
        for effect in &edge.variable_effects {
            merged_variables.insert(effect.variable_id, effect.expr.clone());
        }
        let merged_analog = join_analog_expr_effects.entry(edge.to).or_default();
        for effect in &edge.analog_expr_effects {
            merged_analog.insert(effect.device_id, effect.expr.clone());
        }
    }

    for edge in edges.iter_mut() {
        if !is_parallel_branch_state(states.get(edge.from))
            || !is_parallel_join_state(states.get(edge.to))
        {
            continue;
        }

        if let Some(merged) = join_effects.get(&edge.to) {
            edge.effects = merged.clone();
        }
        if let Some(merged) = join_variable_effects.get(&edge.to) {
            edge.variable_effects = merged
                .iter()
                .map(|(variable_id, expr)| VariableAssignment {
                    variable_id: *variable_id,
                    expr: expr.clone(),
                })
                .collect();
        }
        if let Some(merged) = join_analog_expr_effects.get(&edge.to) {
            edge.analog_expr_effects = merged
                .iter()
                .map(|(device_id, expr)| AnalogExprEffect {
                    device_id: *device_id,
                    expr: expr.clone(),
                })
                .collect();
        }
        edge.ordered_effects = edge
            .effects
            .iter()
            .map(|(device_id, state_id)| ModelEffect::DeviceState {
                device_id: *device_id,
                state_id: *state_id,
            })
            .chain(
                edge.variable_effects
                    .iter()
                    .cloned()
                    .map(ModelEffect::VariableAssignment),
            )
            .chain(
                edge.analog_expr_effects
                    .iter()
                    .cloned()
                    .map(ModelEffect::AnalogExpr),
            )
            .collect();
    }
}

fn is_parallel_branch_state(state: Option<&State>) -> bool {
    state.is_some_and(|state| {
        state.step_name.contains("__parallel_") && state.step_name.contains("_branch_")
    })
}

fn is_parallel_join_state(state: Option<&State>) -> bool {
    state.is_some_and(|state| {
        state.step_name.contains("__parallel_") && state.step_name.ends_with("_join")
    })
}

fn analog_region_state_name(index: usize) -> String {
    format!("region_{index}")
}

fn compute_analog_regions(
    program: &PlcProgram,
    constraints: &ConstraintSet,
) -> HashMap<String, Vec<(f64, f64)>> {
    let mut values_by_device: HashMap<String, Vec<f64>> = HashMap::new();

    for rule in &constraints.safety {
        for expr in [&rule.left, &rule.right] {
            if let SafetyExpr::Threshold { device, value, .. } = expr {
                if let Ok(parsed) = value.parse::<f64>() {
                    add_threshold_value(&mut values_by_device, device, parsed);
                }
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
        if !matches!(
            device.device_type,
            DeviceType::AnalogInput | DeviceType::AnalogOutput
        ) {
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

        if bounds.len() < 2 {
            bounds.push(max);
        }

        let mut regions = Vec::new();
        for window in bounds.windows(2) {
            regions.push((window[0], window[1]));
        }

        if regions.is_empty() {
            regions.push((min, max));
        }

        regions_by_device.insert(device.name.clone(), regions);
    }

    for (target, values) in values_by_device {
        if regions_by_device.contains_key(&target) {
            continue;
        }
        let Some((device, port)) = split_device_port_ref(&target) else {
            continue;
        };
        if !is_analog_port_target(program, device, port) {
            continue;
        }
        regions_by_device.insert(target, synthetic_regions_from_threshold_values(&values));
    }

    regions_by_device
}

fn split_device_port_ref(target: &str) -> Option<(&str, &str)> {
    let mut parts = target.split('.');
    let device = parts.next()?.trim();
    let port = parts.next()?.trim();
    if device.is_empty() || port.is_empty() || parts.next().is_some() {
        return None;
    }
    Some((device, port))
}

fn is_analog_port_target(program: &PlcProgram, device: &str, port: &str) -> bool {
    let Some(decl) = program
        .topology
        .devices
        .iter()
        .find(|entry| entry.name == device)
    else {
        return false;
    };

    if let Some(explicit_port) = decl.attributes.ports.iter().find(|entry| entry.id == port) {
        return matches!(explicit_port.port_type, PortType::Analog);
    }

    default_analog_port_for_device_type(&decl.device_type, port)
}

fn default_analog_port_for_device_type(device_type: &DeviceType, port: &str) -> bool {
    match device_type {
        DeviceType::CamCoupling => matches!(port, "following_error" | "master_pos" | "slave_cmd"),
        DeviceType::AnalogInput => port == "in",
        DeviceType::AnalogOutput => port == "out",
        DeviceType::Pid => matches!(port, "in" | "out"),
        _ => false,
    }
}

fn synthetic_regions_from_threshold_values(values: &[f64]) -> Vec<(f64, f64)> {
    if values.is_empty() {
        return vec![(0.0, 1.0)];
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    sorted.dedup_by(|a, b| (*a - *b).abs() <= f64::EPSILON);

    let min = sorted[0];
    let max = *sorted.last().unwrap_or(&min);
    let span = (max - min).abs();
    let pad = if span > f64::EPSILON {
        span
    } else {
        max.abs().max(1.0)
    };

    let mut bounds = vec![min - pad, max + pad];
    bounds.extend(sorted);
    bounds.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    bounds.dedup_by(|a, b| (*a - *b).abs() <= f64::EPSILON);

    let mut regions = Vec::new();
    for window in bounds.windows(2) {
        regions.push((window[0], window[1]));
    }
    if regions.is_empty() {
        regions.push((min - pad, max + pad));
    }
    regions
}

fn add_threshold_value(values_by_device: &mut HashMap<String, Vec<f64>>, device: &str, value: f64) {
    values_by_device
        .entry(device.to_string())
        .or_default()
        .push(value);
}

fn collect_threshold_values_from_statements(
    statements: &[StepStatement],
    values_by_device: &mut HashMap<String, Vec<f64>>,
) {
    for statement in statements {
        match statement {
            StepStatement::Wait(wait) => {
                collect_threshold_values_from_wait(wait, values_by_device);
            }
            StepStatement::Action(ActionStatement::SetAnalog { target, value }) => {
                add_threshold_value(values_by_device, &target.device, *value);
            }
            StepStatement::Action(ActionStatement::SetAnalogExpr { .. })
            | StepStatement::Action(ActionStatement::Compute { .. })
            | StepStatement::Action(ActionStatement::Call { .. }) => {}
            StepStatement::Repeat { body, .. } => {
                collect_threshold_values_from_statements(body, values_by_device);
            }
            StepStatement::Parallel(parallel) => {
                for branch in &parallel.branches {
                    collect_threshold_values_from_statements(&branch.statements, values_by_device);
                }
            }
            StepStatement::Race(race) => {
                for branch in &race.branches {
                    collect_threshold_values_from_statements(&branch.statements, values_by_device);
                }
            }
            _ => {}
        }
    }
}

fn collect_threshold_values_from_wait(
    wait: &WaitStatement,
    values_by_device: &mut HashMap<String, Vec<f64>>,
) {
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
            add_threshold_value(values_by_device, &condition.left, *value);
        }
        if let LiteralValue::Measured(measured) = &condition.right {
            add_threshold_value(values_by_device, &condition.left, measured.value);
        }
    }
}

fn collect_device_domains(
    program: &PlcProgram,
    constraints: &ConstraintSet,
    _state_machine: &StateMachine,
) -> (
    Vec<DeviceDomain>,
    HashMap<(String, String), usize>,
    Vec<HashMap<String, usize>>,
) {
    let analog_regions = compute_analog_regions(program, constraints);
    let tracked_refs = collect_tracked_device_refs(constraints);
    let mut devices = Vec::<DeviceDomain>::new();
    let mut device_index = HashMap::<(String, String), usize>::new();

    for device in &program.topology.devices {
        let Some(tracked_ports) = tracked_refs.get(&device.name) else {
            continue;
        };
        let (states, default_state, is_analog, region_bounds) = match device.device_type {
            DeviceType::Cylinder => {
                let states = vec!["extended".to_string(), "retracted".to_string()];
                let default_state = states
                    .iter()
                    .position(|state| state == "retracted")
                    .unwrap_or(0);
                (states, default_state, false, None)
            }
            DeviceType::DigitalOutput
            | DeviceType::DigitalInput
            | DeviceType::Plc
            | DeviceType::SolenoidValve
            | DeviceType::Sensor
            | DeviceType::Motor
            | DeviceType::StepperMotor
            | DeviceType::Vfd
            | DeviceType::ServoDrive
            | DeviceType::CamCoupling
            | DeviceType::Pid => {
                let states = vec!["on".to_string(), "off".to_string()];
                let default_state = states.iter().position(|state| state == "off").unwrap_or(0);
                (states, default_state, false, None)
            }
            DeviceType::AnalogInput | DeviceType::AnalogOutput => {
                let regions = analog_regions.get(&device.name);
                if let Some(regions) = regions {
                    let states = regions
                        .iter()
                        .enumerate()
                        .map(|(index, _)| analog_region_state_name(index))
                        .collect::<Vec<_>>();
                    (states, 0, true, Some(regions.clone()))
                } else {
                    let states = vec!["analog_active".to_string()];
                    (states, 0, true, None)
                }
            }
        };

        if tracked_ports.contains("self") {
            let index = devices.len();
            devices.push(DeviceDomain {
                name: device.name.clone(),
                states,
                default_state,
                is_analog,
                region_bounds,
            });
            device_index.insert((device.name.clone(), "self".to_string()), index);
        }
        for port in tracked_ports.iter().filter(|port| port.as_str() != "self") {
            if device_index.contains_key(&(device.name.clone(), port.clone())) {
                continue;
            }

            let declared_port = device
                .attributes
                .ports
                .iter()
                .find(|candidate| candidate.id == *port);

            let is_analog = declared_port
                .map(|candidate| matches!(candidate.port_type, PortType::Analog))
                .unwrap_or_else(|| default_analog_port_for_device_type(&device.device_type, &port));
            let display_name = format!("{}.{}", device.name, port);
            let region_bounds = if is_analog {
                analog_regions.get(&display_name).cloned()
            } else {
                None
            };
            let states = if let Some(bounds) = &region_bounds {
                bounds
                    .iter()
                    .enumerate()
                    .map(|(index, _)| analog_region_state_name(index))
                    .collect::<Vec<_>>()
            } else {
                let mut out = declared_port
                    .map(|candidate| candidate.states.clone())
                    .unwrap_or_default();
                if out.is_empty() {
                    out = if is_analog {
                        vec!["analog_active".to_string()]
                    } else {
                        inferred_states_for_port(&port)
                    };
                }
                out
            };

            let mut default_state = 0usize;
            if region_bounds.is_none() {
                let default_state_name = declared_port
                    .and_then(|candidate| {
                        if candidate.default_state.is_empty() {
                            None
                        } else {
                            Some(candidate.default_state.clone())
                        }
                    })
                    .or_else(|| inferred_default_state_for_port(&states));
                if let Some(name) = default_state_name.as_deref() {
                    if let Some(idx) = states.iter().position(|state| state == name) {
                        default_state = idx;
                    }
                } else if let Some(idx) = states.iter().position(|state| state == "off") {
                    default_state = idx;
                }
            }

            let index = devices.len();
            devices.push(DeviceDomain {
                name: display_name,
                states,
                default_state,
                is_analog,
                region_bounds,
            });
            device_index.insert((device.name.clone(), port.clone()), index);
        }
    }

    for rule in &constraints.safety {
        if let SafetyExpr::State(ref expr) = rule.left
            && let Some(left_device) =
                lookup_device_domain_id(&device_index, &expr.device, &expr.port, false)
        {
            ensure_device_state(&mut devices[left_device], &expr.state);
        }

        if let SafetyExpr::State(ref expr) = rule.right
            && let Some(right_device) =
                lookup_device_domain_id(&device_index, &expr.device, &expr.port, false)
        {
            ensure_device_state(&mut devices[right_device], &expr.state);
        }
    }

    let mut state_index = Vec::with_capacity(devices.len());
    for domain in &devices {
        let mut map = HashMap::new();
        for (idx, state) in domain.states.iter().enumerate() {
            map.insert(state.clone(), idx);
        }
        state_index.push(map);
    }

    (devices, device_index, state_index)
}

fn collect_tracked_device_refs(constraints: &ConstraintSet) -> HashMap<String, HashSet<String>> {
    let mut refs = HashMap::<String, HashSet<String>>::new();

    for rule in &constraints.safety {
        collect_tracked_refs_from_safety_expr(&rule.left, &mut refs);
        collect_tracked_refs_from_safety_expr(&rule.right, &mut refs);
    }

    for claim in &constraints.resource_claims {
        if let crate::ir::ResourceClaimSource::State(state_expr) = &claim.source {
            refs.entry(state_expr.device.clone())
                .or_default()
                .insert(state_expr.port.clone());
        }
    }

    refs
}

fn collect_tracked_refs_from_safety_expr(
    expr: &SafetyExpr,
    refs: &mut HashMap<String, HashSet<String>>,
) {
    match expr {
        SafetyExpr::State(state_expr) => {
            refs.entry(state_expr.device.clone())
                .or_default()
                .insert(state_expr.port.clone());
        }
        SafetyExpr::Threshold { device, .. } => {
            let (device_name, port_name) = split_threshold_target(device);
            refs.entry(device_name.to_string())
                .or_default()
                .insert(port_name.to_string());
        }
    }
}

fn collect_analog_input_states(
    program: &PlcProgram,
    device_index: &HashMap<(String, String), usize>,
    devices: &[DeviceDomain],
) -> Vec<(usize, Vec<usize>)> {
    let mut inputs = Vec::new();

    for device in &program.topology.devices {
        if !matches!(device.device_type, DeviceType::AnalogInput) {
            continue;
        }

        let Some(device_id) = lookup_device_domain_id(device_index, &device.name, "self", false)
        else {
            continue;
        };

        let state_count = devices
            .get(device_id)
            .map(|domain| domain.states.len())
            .unwrap_or(0);

        if state_count == 0 {
            continue;
        }

        let states = (0..state_count).collect::<Vec<_>>();
        inputs.push((device_id, states));
    }

    inputs
}

fn expand_analog_input_effects(
    base_effects: TransitionEffects,
    analog_inputs: &[(usize, Vec<usize>)],
) -> Vec<TransitionEffects> {
    let mut expanded = vec![base_effects];

    for (device_id, states) in analog_inputs {
        if states.is_empty() {
            continue;
        }

        let mut next = Vec::new();
        for effects in expanded {
            if effects.device_effects.contains_key(device_id) {
                next.push(effects);
                continue;
            }

            for state_id in states {
                let mut cloned = effects.clone();
                cloned.device_effects.insert(*device_id, *state_id);
                cloned.ordered_effects.insert(
                    0,
                    ModelEffect::DeviceState {
                        device_id: *device_id,
                        state_id: *state_id,
                    },
                );
                next.push(cloned);
            }
        }
        expanded = next;
    }

    expanded
}

fn ensure_device_state(domain: &mut DeviceDomain, state_name: &str) {
    if domain.states.iter().any(|state| state == state_name) {
        return;
    }

    domain.states.push(state_name.to_string());
}

fn transition_effects(
    transition: &Transition,
    device_index: &HashMap<(String, String), usize>,
    device_state_index: &[HashMap<String, usize>],
    device_domains: &[DeviceDomain],
    variable_index: &HashMap<String, usize>,
) -> TransitionEffects {
    let mut effects = TransitionEffects {
        ordered_effects: Vec::new(),
        device_effects: HashMap::new(),
        variable_effects: Vec::new(),
        analog_expr_effects: Vec::new(),
    };

    for action in &transition.actions {
        match action {
            TransitionAction::SetAnalog {
                target,
                port,
                value_raw,
            } => {
                let Some(device_id) = lookup_device_domain_id(device_index, target, port, true)
                else {
                    continue;
                };
                let Some(state_id) = analog_state_for_value(device_domains, device_id, value_raw)
                else {
                    continue;
                };
                effects
                    .ordered_effects
                    .push(ModelEffect::DeviceState { device_id, state_id });
                effects.device_effects.insert(device_id, state_id);
            }
            TransitionAction::SetAnalogExpr {
                target,
                port,
                expr_raw,
            } => {
                let Some(device_id) = lookup_device_domain_id(device_index, target, port, true)
                else {
                    continue;
                };
                let Some(expr) = compile_model_expr(expr_raw, variable_index) else {
                    continue;
                };
                let effect = AnalogExprEffect { device_id, expr };
                effects
                    .ordered_effects
                    .push(ModelEffect::AnalogExpr(effect.clone()));
                effects.analog_expr_effects.push(effect);
            }
            TransitionAction::Set {
                target,
                port,
                value,
            } => {
                let Some(device_id) = lookup_device_domain_id(device_index, target, port, true)
                else {
                    continue;
                };
                let Some(state_id) = binary_state_for_domain(
                    &device_domains[device_id],
                    &device_state_index[device_id],
                    value,
                ) else {
                    continue;
                };
                effects
                    .ordered_effects
                    .push(ModelEffect::DeviceState { device_id, state_id });
                effects.device_effects.insert(device_id, state_id);
            }
            TransitionAction::Compute { target, expr_raw } => {
                let Some(variable_id) = variable_index.get(target).copied() else {
                    continue;
                };
                let Some(expr) = compile_model_expr(expr_raw, variable_index) else {
                    continue;
                };
                let effect = VariableAssignment { variable_id, expr };
                effects
                    .ordered_effects
                    .push(ModelEffect::VariableAssignment(effect.clone()));
                effects.variable_effects.push(effect);
            }
            TransitionAction::CallExtern { .. } => {}
            TransitionAction::AxisMoveRelative { target, .. }
            | TransitionAction::AxisMoveAbsolute { target, .. } => {
                let Some(device_id) = lookup_device_domain_id(device_index, target, "pulse", false)
                else {
                    continue;
                };
                let Some(state_id) = device_state_index[device_id]
                    .get("active")
                    .or_else(|| device_state_index[device_id].get("on"))
                    .copied()
                else {
                    continue;
                };
                effects
                    .ordered_effects
                    .push(ModelEffect::DeviceState { device_id, state_id });
                effects.device_effects.insert(device_id, state_id);
            }
            TransitionAction::CamEngage { .. }
            | TransitionAction::CamDisengage { .. }
            | TransitionAction::CamSwitch { .. }
            | TransitionAction::CamPhase { .. } => {}
            TransitionAction::Extend { target, port, .. } => {
                let Some(device_id) = lookup_device_domain_id(device_index, target, port, true)
                else {
                    continue;
                };
                let Some(state_id) = device_state_index[device_id].get("extended").copied() else {
                    continue;
                };
                effects
                    .ordered_effects
                    .push(ModelEffect::DeviceState { device_id, state_id });
                effects.device_effects.insert(device_id, state_id);
            }
            TransitionAction::Retract { target, port, .. } => {
                let Some(device_id) = lookup_device_domain_id(device_index, target, port, true)
                else {
                    continue;
                };
                let Some(state_id) = device_state_index[device_id].get("retracted").copied() else {
                    continue;
                };
                effects
                    .ordered_effects
                    .push(ModelEffect::DeviceState { device_id, state_id });
                effects.device_effects.insert(device_id, state_id);
            }
            TransitionAction::Log { .. } => {}
        }
    }

    effects
}

fn lookup_device_domain_id(
    device_index: &HashMap<(String, String), usize>,
    device: &str,
    port: &str,
    allow_self_fallback: bool,
) -> Option<usize> {
    if let Some(id) = device_index
        .get(&(device.to_string(), port.to_string()))
        .copied()
    {
        return Some(id);
    }
    if allow_self_fallback && port != "self" {
        return device_index
            .get(&(device.to_string(), "self".to_string()))
            .copied();
    }
    None
}

fn inferred_states_for_port(port: &str) -> Vec<String> {
    let lowered = port.to_ascii_lowercase();
    if lowered.contains("direction") || lowered.ends_with("_dir") || lowered == "dir" {
        return vec!["forward".to_string(), "reverse".to_string()];
    }
    if lowered.contains("pulse") {
        return vec!["active".to_string(), "idle".to_string()];
    }
    vec!["on".to_string(), "off".to_string()]
}

fn inferred_default_state_for_port(states: &[String]) -> Option<String> {
    for candidate in ["off", "idle", "retracted", "reverse"] {
        if states.iter().any(|state| state == candidate) {
            return Some(candidate.to_string());
        }
    }
    states.first().cloned()
}

fn binary_state_for_domain(
    domain: &DeviceDomain,
    state_index: &HashMap<String, usize>,
    value: &crate::ir::BinaryValue,
) -> Option<usize> {
    let candidates = match value {
        crate::ir::BinaryValue::On => ["on", "forward", "active", "extended"],
        crate::ir::BinaryValue::Off => ["off", "reverse", "idle", "retracted"],
    };

    for candidate in candidates {
        if let Some(state_id) = state_index.get(candidate).copied() {
            return Some(state_id);
        }
    }

    if domain.states.len() == 2 {
        return Some(match value {
            crate::ir::BinaryValue::On => {
                if domain.default_state == 0 {
                    1
                } else {
                    0
                }
            }
            crate::ir::BinaryValue::Off => domain.default_state.min(1),
        });
    }

    if domain.states.len() == 1 {
        return Some(0);
    }

    None
}

fn analog_state_for_value(
    device_domains: &[DeviceDomain],
    device_id: usize,
    value_raw: &str,
) -> Option<usize> {
    let domain = device_domains.get(device_id)?;
    if !domain.is_analog {
        return None;
    }
    let bounds = domain.region_bounds.as_ref()?;
    let value = value_raw.parse::<f64>().ok()?;

    bounds.iter().enumerate().find_map(|(index, (min, max))| {
        if value >= *min && value <= *max {
            Some(index)
        } else {
            None
        }
    })
}

fn transition_label(transition: &Transition) -> String {
    let guard = guard_name(&transition.guard);
    let action_text = transition
        .actions
        .iter()
        .filter_map(action_name)
        .collect::<Vec<_>>();

    if action_text.is_empty() {
        guard.to_string()
    } else {
        format!("{}；动作: {}", guard, action_text.join(", "))
    }
}

fn guard_name(guard: &TransitionGuard) -> &'static str {
    match guard {
        TransitionGuard::Always => "always",
        TransitionGuard::Condition { .. } => "condition",
        TransitionGuard::Timeout { .. } => "timeout",
        TransitionGuard::Delay { .. } => "delay",
    }
}

fn action_name(action: &TransitionAction) -> Option<String> {
    match action {
        TransitionAction::Extend { target, .. } => Some(format!("extend {target}")),
        TransitionAction::Retract { target, .. } => Some(format!("retract {target}")),
        TransitionAction::Set { target, value, .. } => Some(format!(
            "set {} {}",
            target,
            match value {
                crate::ir::BinaryValue::On => "on",
                crate::ir::BinaryValue::Off => "off",
            }
        )),
        TransitionAction::SetAnalog {
            target, value_raw, ..
        } => Some(format!("set_analog {target} {value_raw}")),
        TransitionAction::SetAnalogExpr {
            target, expr_raw, ..
        } => Some(format!("set_analog {target} {expr_raw}")),
        TransitionAction::Compute { target, expr_raw } => {
            Some(format!("compute {target}={expr_raw}"))
        }
        TransitionAction::CallExtern {
            function,
            args_raw,
            binding,
        } => Some(format!(
            "call {}({}) -> {}",
            function,
            args_raw.join(", "),
            match binding {
                crate::ir::ExternCallBinding::Single(name) => name.clone(),
                crate::ir::ExternCallBinding::Tuple(names) => format!("({})", names.join(", ")),
            }
        )),
        TransitionAction::CamEngage { target } => Some(format!("cam_engage {target}")),
        TransitionAction::CamDisengage { target } => Some(format!("cam_disengage {target}")),
        TransitionAction::CamSwitch { target, new_table } => {
            Some(format!("cam_switch {target} {new_table}"))
        }
        TransitionAction::CamPhase {
            target,
            offset_expr_raw,
        } => Some(format!("cam_phase {target} {offset_expr_raw}")),
        TransitionAction::AxisMoveRelative {
            target,
            distance_raw,
            speed_raw,
            timeout,
            on_reject,
            on_motion_fault,
            on_safety_fault,
            ..
        } => Some(format!(
            "axis_move_relative {target} distance={distance_raw} speed={speed_raw} {} {} {} {}",
            render_axis_timeout_branch(timeout),
            render_axis_fault_branch("on_reject", on_reject),
            render_axis_fault_branch("on_motion_fault", on_motion_fault),
            render_axis_fault_branch("on_safety_fault", on_safety_fault),
        )),
        TransitionAction::AxisMoveAbsolute {
            target,
            position_raw,
            speed_raw,
            timeout,
            on_reject,
            on_motion_fault,
            on_safety_fault,
            ..
        } => Some(format!(
            "axis_move_absolute {target} position={position_raw} speed={speed_raw} {} {} {} {}",
            render_axis_timeout_branch(timeout),
            render_axis_fault_branch("on_reject", on_reject),
            render_axis_fault_branch("on_motion_fault", on_motion_fault),
            render_axis_fault_branch("on_safety_fault", on_safety_fault),
        )),
        TransitionAction::Log { message } => Some(format!("log \"{message}\"")),
    }
}

fn render_axis_timeout_branch(branch: &AxisTimeoutBranch) -> String {
    format!(
        "timeout={}ms->{}",
        branch.duration_ms,
        render_axis_target(branch.target_task.as_str(), branch.target_step.as_deref())
    )
}

fn render_axis_fault_branch(label: &str, branch: &AxisFaultBranch) -> String {
    let mut rendered = format!(
        "{label}->{}",
        render_axis_target(branch.target_task.as_str(), branch.target_step.as_deref())
    );
    if let Some(error_code) = branch.error_code.as_deref() {
        rendered.push('[');
        rendered.push_str(error_code);
        rendered.push(']');
    }
    rendered
}

fn render_axis_target(task: &str, step: Option<&str>) -> String {
    match step {
        Some(step_name) => format!("{task}.{step_name}"),
        None => task.to_string(),
    }
}
