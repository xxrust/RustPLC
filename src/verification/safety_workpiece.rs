fn verify_workpiece_flow(
    program: &PlcProgram,
    constraints: &ConstraintSet,
    state_machine: &StateMachine,
) -> Vec<SafetyDiagnostic> {
    if constraints.workpiece_types.is_empty()
        && constraints.workpiece_sites.is_empty()
        && constraints.workpiece_holders.is_empty()
        && constraints.workpiece_carriers.is_empty()
        && state_machine
            .transitions
            .iter()
            .all(|transition| transition.effects.is_empty())
    {
        return Vec::new();
    }

    let registry = WorkpieceFlowRegistry::from_constraints(constraints);

    let Some((state_index, outgoing, initial_state_idx)) = workpiece_state_graph(state_machine)
    else {
        return Vec::new();
    };

    let reachable_transitions = collect_reachable_workpiece_transition_indices(
        state_machine,
        &state_index,
        &outgoing,
        initial_state_idx,
    );
    let initial_flow =
        initial_workpiece_flow_state(state_machine, &registry, &reachable_transitions);
    let mut queue: VecDeque<(usize, WorkpieceFlowState, Vec<String>)> = VecDeque::from([(
        initial_state_idx,
        initial_flow.clone(),
        vec![state_name(&state_machine.initial)],
    )]);
    let mut visited = HashSet::from([(initial_state_idx, initial_flow)]);

    while let Some((state_idx, flow_state, path)) = queue.pop_front() {
        if outgoing[state_idx].is_empty() {
            let occupied = flow_state.occupied_endpoints(&registry);
            if !occupied.is_empty() {
                return vec![SafetyDiagnostic {
                    line: find_state_line(program, &state_machine.states[state_idx]),
                    constraint: "workpiece_flow".to_string(),
                    reason: format!(
                        "reachable terminal state still holds workpieces at {}",
                        occupied.join(", ")
                    ),
                    violation_path: path,
                    suggestion:
                        "finish, unmount, or transfer every workpiece before the flow terminates"
                            .to_string(),
                }];
            }
            continue;
        }

        for transition_idx in &outgoing[state_idx] {
            let transition = &state_machine.transitions[*transition_idx];
            let mut next_flow = flow_state.clone();
            if let Some(diagnostic) =
                apply_workpiece_effects(program, transition, &registry, &mut next_flow, &path)
            {
                return vec![diagnostic];
            }

            let Some(next_state_idx) = state_index
                .get(&workpiece_state_key(&transition.to))
                .copied()
            else {
                continue;
            };
            if visited.insert((next_state_idx, next_flow.clone())) {
                let mut next_path = path.clone();
                next_path.push(format_transition_label(transition));
                queue.push_back((next_state_idx, next_flow, next_path));
            }
        }
    }

    Vec::new()
}

fn collect_workpiece_contract_warnings(
    constraints: &ConstraintSet,
    state_machine: &StateMachine,
) -> Vec<String> {
    if constraints.workpiece_types.is_empty() {
        return Vec::new();
    }
    let Some((state_index, outgoing, initial_state_idx)) = workpiece_state_graph(state_machine)
    else {
        return Vec::new();
    };
    let reachable_transition_indices = collect_reachable_workpiece_transition_indices(
        state_machine,
        &state_index,
        &outgoing,
        initial_state_idx,
    );
    let reachable_transitions = reachable_transition_indices
        .iter()
        .filter_map(|idx| state_machine.transitions.get(*idx))
        .collect::<Vec<_>>();
    let mut warnings = Vec::new();
    let single_type = (constraints.workpiece_types.len() == 1)
        .then_some(constraints.workpiece_types[0].name.as_str());

    for workpiece in &constraints.workpiece_types {
        for ingress in &workpiece.ingress_sites {
            let ingress_reachable = reachable_transitions.iter().any(|transition| {
                transition.effects.iter().any(|effect| {
                    workpiece_effect_source(effect).is_some_and(|endpoint| {
                        single_type == Some(workpiece.name.as_str())
                            && workpiece_endpoint_matches_pattern(&endpoint, ingress)
                    }) || matches!(
                        effect,
                        WorkpieceEffect::Mount { workpiece_type, slot }
                            if workpiece_type == &workpiece.name
                                && workpiece_endpoint_matches_pattern(slot, ingress)
                    )
                })
            });
            if !ingress_reachable {
                warnings.push(format!(
                    "WARNING: workpiece type '{}' declares ingress site '{}', but no reachable effect uses that ingress endpoint",
                    workpiece.name, ingress
                ));
            }
        }

        for terminal_state in &workpiece.normal_terminal_states {
            let terminal_reachable = reachable_transitions.iter().any(|transition| {
                transition.effects.iter().any(|effect| {
                    matches!(effect, WorkpieceEffect::Finish { terminal_state: actual, .. }
                        if single_type == Some(workpiece.name.as_str()) && actual == terminal_state)
                })
            });
            if !terminal_reachable {
                warnings.push(format!(
                    "WARNING: workpiece type '{}' declares normal terminal state '{}', but no reachable finish lands on it",
                    workpiece.name, terminal_state
                ));
            }
        }

        for site in &workpiece.normal_egress_sites {
            let egress_reachable = reachable_transitions.iter().any(|transition| {
                transition.effects.iter().any(|effect| {
                    matches!(effect, WorkpieceEffect::Finish { at, terminal_state }
                        if single_type == Some(workpiece.name.as_str())
                            && workpiece.normal_terminal_states.iter().any(|state| state == terminal_state)
                            && workpiece_endpoint_matches_pattern(at, site))
                })
            });
            if !egress_reachable {
                warnings.push(format!(
                    "WARNING: workpiece type '{}' declares normal egress site '{}', but no reachable finish satisfies that egress contract",
                    workpiece.name, site
                ));
            }
        }

        for terminal_state in &workpiece.abnormal_terminal_states {
            let terminal_reachable = reachable_transitions.iter().any(|transition| {
                transition.effects.iter().any(|effect| {
                    matches!(effect, WorkpieceEffect::Finish { terminal_state: actual, .. }
                        if single_type == Some(workpiece.name.as_str()) && actual == terminal_state)
                })
            });
            if !terminal_reachable {
                warnings.push(format!(
                    "WARNING: workpiece type '{}' declares abnormal terminal state '{}', but no reachable finish lands on it",
                    workpiece.name, terminal_state
                ));
            }
        }

        for site in &workpiece.abnormal_egress_sites {
            let egress_reachable = reachable_transitions.iter().any(|transition| {
                transition.effects.iter().any(|effect| {
                    matches!(effect, WorkpieceEffect::Finish { at, terminal_state }
                        if single_type == Some(workpiece.name.as_str())
                            && workpiece.abnormal_terminal_states.iter().any(|state| state == terminal_state)
                            && workpiece_endpoint_matches_pattern(at, site))
                })
            });
            if !egress_reachable {
                warnings.push(format!(
                    "WARNING: workpiece type '{}' declares abnormal egress site '{}', but no reachable finish satisfies that egress contract",
                    workpiece.name, site
                ));
            }
        }

        for allow in &workpiece.allows {
            match allow {
                crate::ir::WorkpieceAllowDef::SplitInto { target } => {
                    let split_reachable = reachable_transitions.iter().any(|transition| {
                        transition.effects.iter().any(|effect| {
                            matches!(effect, WorkpieceEffect::Split { source_type, target_type, .. }
                                if source_type == &workpiece.name && target_type == target)
                        })
                    });
                    if !split_reachable {
                        warnings.push(format!(
                            "WARNING: workpiece type '{}' declares split_into({}), but no reachable split effect uses that contract",
                            workpiece.name, target
                        ));
                    }
                }
            }
        }

        for derivation in &workpiece.derived_from {
            match derivation {
                crate::ir::WorkpieceDerivationDef::WorkpieceType { workpiece_type } => {
                    let split_reachable = reachable_transitions.iter().any(|transition| {
                        transition.effects.iter().any(|effect| {
                            matches!(effect, WorkpieceEffect::Split { source_type, target_type, .. }
                                if source_type == workpiece_type && target_type == &workpiece.name)
                        })
                    });
                    if !split_reachable {
                        warnings.push(format!(
                            "WARNING: workpiece type '{}' is derived_from '{}', but no reachable split effect produces it",
                            workpiece.name, workpiece_type
                        ));
                    }
                }
                crate::ir::WorkpieceDerivationDef::Merge { inputs } => {
                    let merge_reachable = reachable_transitions.iter().any(|transition| {
                        transition.effects.iter().any(|effect| {
                            matches!(effect, WorkpieceEffect::Merge { target_type, inputs: actual_inputs, .. }
                                if target_type == &workpiece.name && actual_inputs.len() == inputs.len())
                        })
                    });
                    if !merge_reachable {
                        warnings.push(format!(
                            "WARNING: workpiece type '{}' declares merge({}) derivation, but no reachable merge effect produces it",
                            workpiece.name,
                            inputs.join(", ")
                        ));
                    }
                }
            }
        }
    }

    warnings
}

fn workpiece_state_graph(
    state_machine: &StateMachine,
) -> Option<(HashMap<(String, String), usize>, Vec<Vec<usize>>, usize)> {
    let state_index = state_machine
        .states
        .iter()
        .enumerate()
        .map(|(idx, state)| (workpiece_state_key(state), idx))
        .collect::<HashMap<_, _>>();
    let initial_state_idx = state_index
        .get(&workpiece_state_key(&state_machine.initial))
        .copied()?;

    let mut outgoing: Vec<Vec<usize>> = vec![Vec::new(); state_machine.states.len()];
    for (transition_idx, transition) in state_machine.transitions.iter().enumerate() {
        if let Some(from_idx) = state_index
            .get(&workpiece_state_key(&transition.from))
            .copied()
        {
            outgoing[from_idx].push(transition_idx);
        }
    }

    Some((state_index, outgoing, initial_state_idx))
}

fn collect_reachable_workpiece_transition_indices(
    state_machine: &StateMachine,
    state_index: &HashMap<(String, String), usize>,
    outgoing: &[Vec<usize>],
    initial_state_idx: usize,
) -> HashSet<usize> {
    let mut reachable_transitions = HashSet::new();
    let mut visited_states = HashSet::from([initial_state_idx]);
    let mut queue = VecDeque::from([initial_state_idx]);

    while let Some(state_idx) = queue.pop_front() {
        for transition_idx in outgoing.get(state_idx).into_iter().flatten() {
            reachable_transitions.insert(*transition_idx);
            if let Some(next_state_idx) = state_machine
                .transitions
                .get(*transition_idx)
                .and_then(|transition| state_index.get(&workpiece_state_key(&transition.to)))
                .copied()
            {
                if visited_states.insert(next_state_idx) {
                    queue.push_back(next_state_idx);
                }
            }
        }
    }

    reachable_transitions
}

#[derive(Debug, Clone)]
struct WorkpieceEndpointRegistry {
    names: Vec<String>,
    capacities: Vec<u16>,
    index: HashMap<String, usize>,
}

impl WorkpieceEndpointRegistry {
    fn from_constraints(constraints: &ConstraintSet) -> Self {
        let mut names = Vec::new();
        let mut capacities = Vec::new();
        let mut index = HashMap::new();

        let mut push_endpoint = |name: String, capacity: u16| {
            if index.contains_key(&name) {
                return;
            }
            index.insert(name.clone(), names.len());
            names.push(name);
            capacities.push(capacity.max(1));
        };

        for site in &constraints.workpiece_sites {
            if site.kind == WorkpieceSiteKind::WorkpieceLocation {
                push_endpoint(site.name.clone(), site.capacity as u16);
            }
        }
        for holder in &constraints.workpiece_holders {
            push_endpoint(holder.name.clone(), holder.capacity as u16);
        }
        for carrier in &constraints.workpiece_carriers {
            match &carrier.layout {
                WorkpieceCarrierLayoutDef::Slots { count } => {
                    for idx in 0..*count {
                        push_endpoint(format!("{}.slot[{idx}]", carrier.name), 1);
                    }
                }
                WorkpieceCarrierLayoutDef::Grid { rows, cols } => {
                    for row in 0..*rows {
                        for col in 0..*cols {
                            push_endpoint(format!("{}.slot[{row},{col}]", carrier.name), 1);
                        }
                    }
                }
            }
        }

        Self {
            names,
            capacities,
            index,
        }
    }

    fn occupied_endpoints(&self, counts: &[u16]) -> Vec<String> {
        counts
            .iter()
            .enumerate()
            .filter(|(_, count)| **count > 0)
            .map(|(idx, count)| format!("{}({})", self.names[idx], count))
            .collect()
    }
}

#[derive(Debug, Clone)]
struct WorkpieceFlowRegistry {
    endpoints: WorkpieceEndpointRegistry,
    workpiece_types: Vec<crate::ir::WorkpieceTypeDef>,
    workpiece_index: HashMap<String, usize>,
}

impl WorkpieceFlowRegistry {
    fn from_constraints(constraints: &ConstraintSet) -> Self {
        let mut workpiece_index = HashMap::new();
        for (idx, workpiece) in constraints.workpiece_types.iter().enumerate() {
            workpiece_index.insert(workpiece.name.clone(), idx);
        }

        Self {
            endpoints: WorkpieceEndpointRegistry::from_constraints(constraints),
            workpiece_types: constraints.workpiece_types.clone(),
            workpiece_index,
        }
    }

    fn endpoint_idx(&self, endpoint: &str) -> Option<usize> {
        self.endpoints.index.get(endpoint).copied()
    }

    fn endpoint_matches_any_ingress(&self, endpoint: &str) -> bool {
        self.workpiece_types.iter().any(|workpiece| {
            workpiece
                .ingress_sites
                .iter()
                .any(|pattern| workpiece_endpoint_matches_pattern(endpoint, pattern))
        })
    }

    fn endpoint_matches_ingress_for_type(&self, workpiece_type_idx: usize, endpoint: &str) -> bool {
        self.workpiece_types
            .get(workpiece_type_idx)
            .is_some_and(|workpiece| {
                workpiece
                    .ingress_sites
                    .iter()
                    .any(|pattern| workpiece_endpoint_matches_pattern(endpoint, pattern))
            })
    }

    fn finish_bucket_error(
        &self,
        workpiece_type_idx: usize,
        endpoint: &str,
        terminal_state: &str,
    ) -> Option<String> {
        let workpiece = self.workpiece_types.get(workpiece_type_idx)?;
        if workpiece
            .normal_terminal_states
            .iter()
            .any(|state| state == terminal_state)
        {
            if workpiece
                .normal_egress_sites
                .iter()
                .any(|pattern| workpiece_endpoint_matches_pattern(endpoint, pattern))
            {
                return None;
            }
            return Some(format!(
                "finish exits endpoint '{}' with normal terminal state '{}', but workpiece type '{}' only allows that bucket through normal egress sites [{}]",
                endpoint,
                terminal_state,
                workpiece.name,
                workpiece.normal_egress_sites.join(", ")
            ));
        }

        if workpiece
            .abnormal_terminal_states
            .iter()
            .any(|state| state == terminal_state)
        {
            if workpiece
                .abnormal_egress_sites
                .iter()
                .any(|pattern| workpiece_endpoint_matches_pattern(endpoint, pattern))
            {
                return None;
            }
            return Some(format!(
                "finish exits endpoint '{}' with abnormal terminal state '{}', but workpiece type '{}' only allows that bucket through abnormal egress sites [{}]",
                endpoint,
                terminal_state,
                workpiece.name,
                workpiece.abnormal_egress_sites.join(", ")
            ));
        }

        Some(format!(
            "finish uses undeclared terminal state '{}' for workpiece type '{}'",
            terminal_state, workpiece.name
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct WorkpieceFlowToken {
    workpiece_type_idx: usize,
    endpoint_idx: usize,
    mounted_endpoint_idx: Option<usize>,
    provenance: WorkpieceFlowTokenProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum WorkpieceFlowTokenProvenance {
    Ingress,
    MountIngress,
    Split { source_type_idx: usize },
    Merge { input_type_indices: Vec<usize> },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
struct WorkpieceFlowState {
    tokens: Vec<WorkpieceFlowToken>,
}

impl WorkpieceFlowState {
    fn canonicalize(&mut self) {
        self.tokens.sort_unstable();
    }

    fn occupancy(&self, endpoint_idx: usize) -> usize {
        self.tokens
            .iter()
            .filter(|token| token.endpoint_idx == endpoint_idx)
            .count()
    }

    fn active_token_indices_of_type(&self, workpiece_type_idx: usize) -> Vec<usize> {
        self.tokens
            .iter()
            .enumerate()
            .filter_map(|(idx, token)| {
                (token.workpiece_type_idx == workpiece_type_idx).then_some(idx)
            })
            .collect()
    }

    fn unique_token_index_at(
        &self,
        endpoint_idx: usize,
        mounted: Option<bool>,
    ) -> Result<usize, usize> {
        let matches = self
            .tokens
            .iter()
            .enumerate()
            .filter_map(|(idx, token)| {
                let mount_matches = match mounted {
                    Some(true) => token.mounted_endpoint_idx == Some(endpoint_idx),
                    Some(false) => token.mounted_endpoint_idx.is_none(),
                    None => true,
                };
                (token.endpoint_idx == endpoint_idx && mount_matches).then_some(idx)
            })
            .collect::<Vec<_>>();
        match matches.len() {
            0 => Err(0),
            1 => Ok(matches[0]),
            count => Err(count),
        }
    }

    fn occupied_endpoints(&self, registry: &WorkpieceFlowRegistry) -> Vec<String> {
        let mut counts = vec![0u16; registry.endpoints.names.len()];
        for token in &self.tokens {
            if let Some(count) = counts.get_mut(token.endpoint_idx) {
                *count = count.saturating_add(1);
            }
        }
        registry.endpoints.occupied_endpoints(&counts)
    }

    fn inconsistent_mount_state(
        &self,
        registry: &WorkpieceFlowRegistry,
    ) -> Option<(String, String, String)> {
        self.tokens.iter().find_map(|token| {
            let mounted_endpoint_idx = token.mounted_endpoint_idx?;
            if mounted_endpoint_idx == token.endpoint_idx {
                return None;
            }
            Some((
                registry.workpiece_types[token.workpiece_type_idx]
                    .name
                    .clone(),
                registry.endpoints.names[mounted_endpoint_idx].clone(),
                registry.endpoints.names[token.endpoint_idx].clone(),
            ))
        })
    }
}

fn initial_workpiece_flow_state(
    state_machine: &StateMachine,
    registry: &WorkpieceFlowRegistry,
    reachable_transition_indices: &HashSet<usize>,
) -> WorkpieceFlowState {
    let mut flow_state = WorkpieceFlowState::default();
    let mut seeded = HashSet::new();

    for (transition_idx, transition) in state_machine.transitions.iter().enumerate() {
        if !reachable_transition_indices.contains(&transition_idx) {
            continue;
        }
        for effect in &transition.effects {
            let Some(source) = workpiece_ingress_source(effect) else {
                continue;
            };
            let Some(endpoint_idx) = registry.endpoint_idx(&source) else {
                continue;
            };
            for (workpiece_type_idx, workpiece) in registry.workpiece_types.iter().enumerate() {
                if workpiece
                    .ingress_sites
                    .iter()
                    .any(|pattern| workpiece_endpoint_matches_pattern(&source, pattern))
                {
                    seeded.insert(WorkpieceFlowToken {
                        workpiece_type_idx,
                        endpoint_idx,
                        mounted_endpoint_idx: None,
                        provenance: WorkpieceFlowTokenProvenance::Ingress,
                    });
                }
            }
        }
    }

    flow_state.tokens.extend(seeded);
    flow_state.canonicalize();
    flow_state
}

fn workpiece_effect_source(effect: &WorkpieceEffect) -> Option<String> {
    match effect {
        WorkpieceEffect::Acquire { from, .. } => Some(from.clone()),
        WorkpieceEffect::Transfer { from, .. } => Some(from.clone()),
        WorkpieceEffect::Unmount { slot, .. } => Some(slot.clone()),
        WorkpieceEffect::Finish { at, .. } => Some(at.clone()),
        WorkpieceEffect::Mount { .. }
        | WorkpieceEffect::Split { .. }
        | WorkpieceEffect::Merge { .. }
        | WorkpieceEffect::TransformCarrier { .. } => None,
    }
}

fn workpiece_ingress_source(effect: &WorkpieceEffect) -> Option<String> {
    match effect {
        WorkpieceEffect::Acquire { from, .. } | WorkpieceEffect::Transfer { from, .. } => {
            Some(from.clone())
        }
        WorkpieceEffect::Finish { .. }
        | WorkpieceEffect::Mount { .. }
        | WorkpieceEffect::Unmount { .. }
        | WorkpieceEffect::Split { .. }
        | WorkpieceEffect::Merge { .. }
        | WorkpieceEffect::TransformCarrier { .. } => None,
    }
}

fn apply_workpiece_effects(
    program: &PlcProgram,
    transition: &Transition,
    registry: &WorkpieceFlowRegistry,
    flow_state: &mut WorkpieceFlowState,
    path: &[String],
) -> Option<SafetyDiagnostic> {
    for effect in &transition.effects {
        match effect {
            WorkpieceEffect::Acquire { holder, from } => {
                if let Some(diag) = move_workpiece(
                    program,
                    transition,
                    registry,
                    flow_state,
                    from,
                    holder,
                    path,
                    "acquire",
                    Some(false),
                    None,
                ) {
                    return Some(diag);
                }
            }
            WorkpieceEffect::Transfer { from, to } => {
                if let Some(diag) = move_workpiece(
                    program,
                    transition,
                    registry,
                    flow_state,
                    from,
                    to,
                    path,
                    "transfer",
                    Some(false),
                    None,
                ) {
                    return Some(diag);
                }
            }
            WorkpieceEffect::Unmount { slot, to, .. } => {
                if let Some(diag) = move_workpiece(
                    program,
                    transition,
                    registry,
                    flow_state,
                    slot,
                    to,
                    path,
                    "unmount",
                    Some(true),
                    None,
                ) {
                    return Some(diag);
                }
            }
            WorkpieceEffect::Finish { at, terminal_state } => {
                if let Some(diag) = finish_workpiece(
                    program,
                    transition,
                    registry,
                    flow_state,
                    at,
                    terminal_state,
                    path,
                    Some(false),
                ) {
                    return Some(diag);
                }
            }
            WorkpieceEffect::Mount {
                workpiece_type,
                slot,
            } => {
                if let Some(diag) = mount_workpiece(
                    program,
                    transition,
                    registry,
                    flow_state,
                    workpiece_type,
                    slot,
                    path,
                ) {
                    return Some(diag);
                }
            }
            WorkpieceEffect::Split {
                source_type,
                target_type,
                count,
                consumed,
            } => {
                if let Some(diag) = split_workpiece(
                    program,
                    transition,
                    registry,
                    flow_state,
                    source_type,
                    target_type,
                    *count,
                    *consumed,
                    path,
                ) {
                    return Some(diag);
                }
            }
            WorkpieceEffect::Merge {
                inputs,
                target_type,
                consumed_inputs,
            } => {
                if let Some(diag) = merge_workpiece(
                    program,
                    transition,
                    registry,
                    flow_state,
                    inputs,
                    target_type,
                    *consumed_inputs,
                    path,
                ) {
                    return Some(diag);
                }
            }
            WorkpieceEffect::TransformCarrier { .. } => {}
        }
    }

    if let Some(diag) =
        validate_workpiece_flow_invariants(program, transition, registry, flow_state, path)
    {
        return Some(diag);
    }

    flow_state.canonicalize();
    None
}

fn move_workpiece(
    program: &PlcProgram,
    transition: &Transition,
    registry: &WorkpieceFlowRegistry,
    flow_state: &mut WorkpieceFlowState,
    from: &str,
    to: &str,
    path: &[String],
    effect_name: &str,
    source_mounted: Option<bool>,
    destination_mounted: Option<&str>,
) -> Option<SafetyDiagnostic> {
    let token_idx = match unique_active_workpiece(
        program,
        transition,
        registry,
        flow_state,
        from,
        source_mounted,
        path,
        effect_name,
    ) {
        Ok(token_idx) => token_idx,
        Err(diag) => return Some(diag),
    };
    let Some(endpoint_idx) = registry.endpoint_idx(to) else {
        return Some(SafetyDiagnostic {
            line: find_state_line(program, &transition.from),
            constraint: "workpiece_flow".to_string(),
            reason: format!("{effect_name} references undefined endpoint '{}'", to),
            violation_path: extend_path(path, transition),
            suggestion: "declare the endpoint in topology before using it in workpiece effects"
                .to_string(),
        });
    };
    let mounted_endpoint_idx = match destination_mounted {
        Some(slot) => {
            let Some(slot_idx) = registry.endpoint_idx(slot) else {
                return Some(SafetyDiagnostic {
                    line: find_state_line(program, &transition.from),
                    constraint: "workpiece_flow".to_string(),
                    reason: format!("{effect_name} references undefined endpoint '{}'", slot),
                    violation_path: extend_path(path, transition),
                    suggestion:
                        "declare the endpoint in topology before using it in workpiece effects"
                            .to_string(),
                });
            };
            Some(slot_idx)
        }
        None => None,
    };
    if from != to {
        if let Some(diag) = ensure_workpiece_destination(
            program,
            transition,
            registry,
            flow_state,
            to,
            path,
            effect_name,
        ) {
            return Some(diag);
        }
    }
    flow_state.tokens[token_idx].endpoint_idx = endpoint_idx;
    flow_state.tokens[token_idx].mounted_endpoint_idx = mounted_endpoint_idx;
    None
}

fn finish_workpiece(
    program: &PlcProgram,
    transition: &Transition,
    registry: &WorkpieceFlowRegistry,
    flow_state: &mut WorkpieceFlowState,
    endpoint: &str,
    terminal_state: &str,
    path: &[String],
    source_mounted: Option<bool>,
) -> Option<SafetyDiagnostic> {
    let token_idx = match unique_active_workpiece(
        program,
        transition,
        registry,
        flow_state,
        endpoint,
        source_mounted,
        path,
        "finish",
    ) {
        Ok(token_idx) => token_idx,
        Err(diag) => return Some(diag),
    };
    if let Some(reason) = registry.finish_bucket_error(
        flow_state.tokens[token_idx].workpiece_type_idx,
        endpoint,
        terminal_state,
    ) {
        return Some(SafetyDiagnostic {
            line: find_state_line(program, &transition.from),
            constraint: "workpiece_flow".to_string(),
            reason,
            violation_path: extend_path(path, transition),
            suggestion:
                "align the finish terminal_state and endpoint with the declared normal/abnormal egress bucket"
                    .to_string(),
        });
    }
    flow_state.tokens.remove(token_idx);
    None
}

fn mount_workpiece(
    program: &PlcProgram,
    transition: &Transition,
    registry: &WorkpieceFlowRegistry,
    flow_state: &mut WorkpieceFlowState,
    workpiece_type: &str,
    slot: &str,
    path: &[String],
) -> Option<SafetyDiagnostic> {
    let Some(workpiece_type_idx) = registry.workpiece_index.get(workpiece_type).copied() else {
        return Some(SafetyDiagnostic {
            line: find_state_line(program, &transition.from),
            constraint: "workpiece_flow".to_string(),
            reason: format!(
                "mount introduces undeclared workpiece type '{}'",
                workpiece_type
            ),
            violation_path: extend_path(path, transition),
            suggestion: "declare the workpiece type before using it in runtime effects".to_string(),
        });
    };
    if !registry.endpoint_matches_ingress_for_type(workpiece_type_idx, slot) {
        return Some(SafetyDiagnostic {
            line: find_state_line(program, &transition.from),
            constraint: "workpiece_flow".to_string(),
            reason: format!(
                "mount introduces workpiece type '{}' at endpoint '{}', but that endpoint is not a declared ingress site",
                workpiece_type, slot
            ),
            violation_path: extend_path(path, transition),
            suggestion:
                "introduce new workpieces only through ingress_sites declared on the matching workpiece type"
                    .to_string(),
        });
    }
    let Some(slot_idx) = registry.endpoint_idx(slot) else {
        return Some(SafetyDiagnostic {
            line: find_state_line(program, &transition.from),
            constraint: "workpiece_flow".to_string(),
            reason: format!("mount references undefined endpoint '{}'", slot),
            violation_path: extend_path(path, transition),
            suggestion: "declare the endpoint in topology before using it in workpiece effects"
                .to_string(),
        });
    };

    match flow_state.unique_token_index_at(slot_idx, Some(true)) {
        Ok(_) => {
            return Some(SafetyDiagnostic {
                line: find_state_line(program, &transition.from),
                constraint: "workpiece_flow".to_string(),
                reason: format!(
                    "mount would place a second mounted workpiece at slot '{}'",
                    slot
                ),
                violation_path: extend_path(path, transition),
                suggestion:
                    "unmount or finish the mounted workpiece before mounting another token on the same slot"
                        .to_string(),
            });
        }
        Err(count) if count > 1 => {
            return Some(SafetyDiagnostic {
                line: find_state_line(program, &transition.from),
                constraint: "workpiece_flow".to_string(),
                reason: format!(
                    "reachable state already has duplicate mounted occupancy ({count} tokens) at slot '{}'",
                    slot
                ),
                violation_path: extend_path(path, transition),
                suggestion:
                    "preserve at most one mounted workpiece per carrier slot in every reachable state"
                        .to_string(),
            });
        }
        Err(_) => {}
    }

    let free_candidates = flow_state
        .tokens
        .iter()
        .enumerate()
        .filter_map(|(idx, token)| {
            (token.endpoint_idx == slot_idx
                && token.mounted_endpoint_idx.is_none()
                && token.workpiece_type_idx == workpiece_type_idx)
                .then_some(idx)
        })
        .collect::<Vec<_>>();

    match free_candidates.as_slice() {
        [token_idx] => {
            flow_state.tokens[*token_idx].mounted_endpoint_idx = Some(slot_idx);
            return None;
        }
        [] => {}
        _ => {
            return Some(SafetyDiagnostic {
                line: find_state_line(program, &transition.from),
                constraint: "workpiece_flow".to_string(),
                reason: format!(
                    "mount requires a unique free-standing workpiece type '{}' at slot '{}', but reachable state has duplicate candidates",
                    workpiece_type, slot
                ),
                violation_path: extend_path(path, transition),
                suggestion:
                    "ensure each slot resolves to at most one free-standing token before mounting it"
                        .to_string(),
            });
        }
    }

    if let Some(diag) = ensure_workpiece_destination(
        program, transition, registry, flow_state, slot, path, "mount",
    ) {
        return Some(diag);
    }

    flow_state.tokens.push(WorkpieceFlowToken {
        workpiece_type_idx,
        endpoint_idx: slot_idx,
        mounted_endpoint_idx: Some(slot_idx),
        provenance: WorkpieceFlowTokenProvenance::MountIngress,
    });
    None
}

fn split_workpiece(
    program: &PlcProgram,
    transition: &Transition,
    registry: &WorkpieceFlowRegistry,
    flow_state: &mut WorkpieceFlowState,
    source_type: &str,
    target_type: &str,
    count: u32,
    consumed: bool,
    path: &[String],
) -> Option<SafetyDiagnostic> {
    let Some(source_type_idx) = registry.workpiece_index.get(source_type).copied() else {
        return Some(SafetyDiagnostic {
            line: find_state_line(program, &transition.from),
            constraint: "workpiece_flow".to_string(),
            reason: format!(
                "split references undeclared source workpiece type '{}'",
                source_type
            ),
            violation_path: extend_path(path, transition),
            suggestion: "declare the split source workpiece type before using it in effects"
                .to_string(),
        });
    };
    let Some(target_type_idx) = registry.workpiece_index.get(target_type).copied() else {
        return Some(SafetyDiagnostic {
            line: find_state_line(program, &transition.from),
            constraint: "workpiece_flow".to_string(),
            reason: format!(
                "split references undeclared target workpiece type '{}'",
                target_type
            ),
            violation_path: extend_path(path, transition),
            suggestion: "declare the split target workpiece type before using it in effects"
                .to_string(),
        });
    };

    let source_candidates = flow_state.active_token_indices_of_type(source_type_idx);
    let source_idx = match source_candidates.as_slice() {
        [idx] => *idx,
        [] => {
            return Some(SafetyDiagnostic {
                line: find_state_line(program, &transition.from),
                constraint: "workpiece_flow".to_string(),
                reason: format!(
                    "split into '{}' requires a valid active source token of type '{}', but no reachable token instance is available",
                    target_type, source_type
                ),
                violation_path: extend_path(path, transition),
                suggestion:
                    "introduce exactly one reachable source token instance before splitting it"
                        .to_string(),
            });
        }
        matches => {
            return Some(SafetyDiagnostic {
                line: find_state_line(program, &transition.from),
                constraint: "workpiece_flow".to_string(),
                reason: format!(
                    "split into '{}' requires a unique active source token of type '{}', but reachable state has {} instances",
                    target_type,
                    source_type,
                    matches.len()
                ),
                violation_path: extend_path(path, transition),
                suggestion:
                    "disambiguate the split source so exactly one active token instance matches the source type"
                        .to_string(),
            });
        }
    };

    let source = flow_state.tokens[source_idx].clone();
    let capacity = registry.endpoints.capacities[source.endpoint_idx] as usize;
    let occupancy = flow_state.occupancy(source.endpoint_idx);
    let final_occupancy = occupancy
        .saturating_sub(usize::from(consumed))
        .saturating_add(count as usize);
    if final_occupancy > capacity {
        return Some(SafetyDiagnostic {
            line: find_state_line(program, &transition.from),
            constraint: "workpiece_flow".to_string(),
            reason: format!(
                "split into '{}' would exceed capacity at endpoint '{}' (capacity={})",
                target_type, registry.endpoints.names[source.endpoint_idx], capacity
            ),
            violation_path: extend_path(path, transition),
            suggestion:
                "move or finish workpieces before splitting so the destination endpoint has enough capacity"
                    .to_string(),
        });
    }

    if consumed {
        flow_state.tokens.remove(source_idx);
    }
    for _ in 0..count {
        flow_state.tokens.push(WorkpieceFlowToken {
            workpiece_type_idx: target_type_idx,
            endpoint_idx: source.endpoint_idx,
            mounted_endpoint_idx: source.mounted_endpoint_idx,
            provenance: WorkpieceFlowTokenProvenance::Split { source_type_idx },
        });
    }

    None
}

fn merge_workpiece(
    program: &PlcProgram,
    transition: &Transition,
    registry: &WorkpieceFlowRegistry,
    flow_state: &mut WorkpieceFlowState,
    inputs: &[String],
    target_type: &str,
    consumed_inputs: bool,
    path: &[String],
) -> Option<SafetyDiagnostic> {
    let Some(target_type_idx) = registry.workpiece_index.get(target_type).copied() else {
        return Some(SafetyDiagnostic {
            line: find_state_line(program, &transition.from),
            constraint: "workpiece_flow".to_string(),
            reason: format!(
                "merge references undeclared target workpiece type '{}'",
                target_type
            ),
            violation_path: extend_path(path, transition),
            suggestion: "declare the merge target workpiece type before using it in effects"
                .to_string(),
        });
    };

    let Some(required_input_names) = resolve_merge_input_types_from_registry(
        &registry.workpiece_types,
        target_type,
        inputs.len(),
    ) else {
        return Some(SafetyDiagnostic {
            line: find_state_line(program, &transition.from),
            constraint: "workpiece_flow".to_string(),
            reason: format!(
                "merge into '{}' has no unique declared input derivation matching {} inputs",
                target_type,
                inputs.len()
            ),
            violation_path: extend_path(path, transition),
            suggestion:
                "keep exactly one merge(...) derivation for the target type and match the effect arity to it"
                    .to_string(),
        });
    };

    let mut required_input_indices = Vec::with_capacity(required_input_names.len());
    for required_name in &required_input_names {
        let Some(required_type_idx) = registry.workpiece_index.get(required_name).copied() else {
            return Some(SafetyDiagnostic {
                line: find_state_line(program, &transition.from),
                constraint: "workpiece_flow".to_string(),
                reason: format!(
                    "merge into '{}' declares undeclared input workpiece type '{}'",
                    target_type, required_name
                ),
                violation_path: extend_path(path, transition),
                suggestion:
                    "declare every merge input workpiece type before using the derivation in verification"
                        .to_string(),
            });
        };
        required_input_indices.push(required_type_idx);
    }

    let mut selected_indices = Vec::with_capacity(required_input_indices.len());
    for required_type_idx in &required_input_indices {
        let selected = flow_state
            .tokens
            .iter()
            .enumerate()
            .find_map(|(idx, token)| {
                (token.workpiece_type_idx == *required_type_idx && !selected_indices.contains(&idx))
                    .then_some(idx)
            });
        let Some(selected) = selected else {
            let missing = missing_merge_inputs(flow_state, registry, &required_input_indices);
            return Some(SafetyDiagnostic {
                line: find_state_line(program, &transition.from),
                constraint: "workpiece_flow".to_string(),
                reason: format!(
                    "merge into '{}' requires the declared legal input set [{}], but reachable state is missing {}",
                    target_type,
                    required_input_names.join(", "),
                    missing.join(", ")
                ),
                violation_path: extend_path(path, transition),
                suggestion:
                    "produce the declared merge inputs as distinct active token instances before consuming them"
                        .to_string(),
            });
        };
        selected_indices.push(selected);
    }

    let output_location = flow_state.tokens[selected_indices[0]].endpoint_idx;
    let output_slot = flow_state.tokens[selected_indices[0]].mounted_endpoint_idx;
    let capacity = registry.endpoints.capacities[output_location] as usize;
    let removed_here = if consumed_inputs {
        selected_indices
            .iter()
            .filter(|idx| flow_state.tokens[**idx].endpoint_idx == output_location)
            .count()
    } else {
        0
    };
    let final_occupancy = flow_state
        .occupancy(output_location)
        .saturating_sub(removed_here)
        .saturating_add(1);
    if final_occupancy > capacity {
        return Some(SafetyDiagnostic {
            line: find_state_line(program, &transition.from),
            constraint: "workpiece_flow".to_string(),
            reason: format!(
                "merge into '{}' would exceed capacity at endpoint '{}' (capacity={})",
                target_type, registry.endpoints.names[output_location], capacity
            ),
            violation_path: extend_path(path, transition),
            suggestion:
                "drain the destination endpoint before materializing the merge output token"
                    .to_string(),
        });
    }

    if consumed_inputs {
        selected_indices.sort_unstable();
        for idx in selected_indices.into_iter().rev() {
            flow_state.tokens.remove(idx);
        }
    }

    required_input_indices.sort_unstable();
    flow_state.tokens.push(WorkpieceFlowToken {
        workpiece_type_idx: target_type_idx,
        endpoint_idx: output_location,
        mounted_endpoint_idx: output_slot,
        provenance: WorkpieceFlowTokenProvenance::Merge {
            input_type_indices: required_input_indices,
        },
    });

    None
}

fn missing_merge_inputs(
    flow_state: &WorkpieceFlowState,
    registry: &WorkpieceFlowRegistry,
    required_input_indices: &[usize],
) -> Vec<String> {
    let mut requirements = HashMap::<usize, usize>::new();
    for input_idx in required_input_indices {
        *requirements.entry(*input_idx).or_default() += 1;
    }

    let mut available = HashMap::<usize, usize>::new();
    for token in &flow_state.tokens {
        *available.entry(token.workpiece_type_idx).or_default() += 1;
    }

    let mut missing = requirements
        .into_iter()
        .filter_map(|(workpiece_type_idx, required)| {
            let actual = available.get(&workpiece_type_idx).copied().unwrap_or(0);
            (actual < required).then_some(format!(
                "{}x {}",
                required - actual,
                registry.workpiece_types[workpiece_type_idx].name
            ))
        })
        .collect::<Vec<_>>();
    missing.sort();
    missing
}

fn resolve_merge_input_types_from_registry(
    workpiece_types: &[crate::ir::WorkpieceTypeDef],
    target_type: &str,
    input_count: usize,
) -> Option<Vec<String>> {
    let workpiece = workpiece_types
        .iter()
        .find(|candidate| candidate.name == target_type)?;
    let matches = workpiece
        .derived_from
        .iter()
        .filter_map(|rule| match rule {
            crate::ir::WorkpieceDerivationDef::Merge { inputs } if inputs.len() == input_count => {
                Some(inputs.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        matches.into_iter().next()
    } else {
        None
    }
}

fn unique_active_workpiece(
    program: &PlcProgram,
    transition: &Transition,
    registry: &WorkpieceFlowRegistry,
    flow_state: &WorkpieceFlowState,
    endpoint: &str,
    mounted: Option<bool>,
    path: &[String],
    effect_name: &str,
) -> Result<usize, SafetyDiagnostic> {
    let Some(endpoint_idx) = registry.endpoint_idx(endpoint) else {
        return Err(SafetyDiagnostic {
            line: find_state_line(program, &transition.from),
            constraint: "workpiece_flow".to_string(),
            reason: format!("{effect_name} references undefined endpoint '{}'", endpoint),
            violation_path: extend_path(path, transition),
            suggestion: "declare the endpoint in topology before using it in workpiece effects"
                .to_string(),
        });
    };

    let expectation = match mounted {
        Some(true) => "mounted",
        Some(false) => "free-standing",
        None => "active",
    };

    match flow_state.unique_token_index_at(endpoint_idx, mounted) {
        Ok(token_idx) => Ok(token_idx),
        Err(0) => {
            let mut reason = format!(
                "{effect_name} reads endpoint '{}' before any {expectation} workpiece is available",
                endpoint,
            );
            if path.len() == 1
                && mounted != Some(true)
                && !registry.endpoint_matches_any_ingress(endpoint)
            {
                reason.push_str("; the endpoint is not a declared ingress site");
            }
            Err(SafetyDiagnostic {
                line: find_state_line(program, &transition.from),
                constraint: "workpiece_flow".to_string(),
                reason,
                violation_path: extend_path(path, transition),
                suggestion: match mounted {
                    Some(true) => {
                        "mount the workpiece on the slot before consuming it through unmount"
                            .to_string()
                    }
                    _ => "introduce the workpiece through a declared ingress or move it into the source endpoint first".to_string(),
                },
            })
        }
        Err(count) => Err(SafetyDiagnostic {
            line: find_state_line(program, &transition.from),
            constraint: "workpiece_flow".to_string(),
            reason: format!(
                "{effect_name} requires a unique {expectation} workpiece at endpoint '{}', but reachable state has duplicate occupancy ({count} tokens)",
                endpoint,
            ),
            violation_path: extend_path(path, transition),
            suggestion:
                "drain or disambiguate the endpoint so each acquire/transfer/finish source resolves to exactly one active workpiece"
                    .to_string(),
        }),
    }
}

fn validate_workpiece_flow_invariants(
    program: &PlcProgram,
    transition: &Transition,
    registry: &WorkpieceFlowRegistry,
    flow_state: &WorkpieceFlowState,
    path: &[String],
) -> Option<SafetyDiagnostic> {
    let (workpiece_type, mounted_slot, current_endpoint) =
        flow_state.inconsistent_mount_state(registry)?;
    Some(SafetyDiagnostic {
        line: find_state_line(program, &transition.from),
        constraint: "workpiece_flow".to_string(),
        reason: format!(
            "workpiece type '{}' is still mounted on slot '{}' while also being reachable at '{}'",
            workpiece_type, mounted_slot, current_endpoint
        ),
        violation_path: extend_path(path, transition),
        suggestion:
            "mounted workpieces must remain bound to their slot until an explicit unmount clears the mounted association"
                .to_string(),
    })
}

fn ensure_workpiece_destination(
    program: &PlcProgram,
    transition: &Transition,
    registry: &WorkpieceFlowRegistry,
    flow_state: &WorkpieceFlowState,
    endpoint: &str,
    path: &[String],
    effect_name: &str,
) -> Option<SafetyDiagnostic> {
    let Some(endpoint_idx) = registry.endpoint_idx(endpoint) else {
        return Some(SafetyDiagnostic {
            line: find_state_line(program, &transition.from),
            constraint: "workpiece_flow".to_string(),
            reason: format!("{effect_name} references undefined endpoint '{}'", endpoint),
            violation_path: extend_path(path, transition),
            suggestion: "declare the endpoint in topology before using it in workpiece effects"
                .to_string(),
        });
    };
    let occupancy = flow_state.occupancy(endpoint_idx);
    if occupancy.saturating_add(1) > registry.endpoints.capacities[endpoint_idx] as usize {
        return Some(SafetyDiagnostic {
            line: find_state_line(program, &transition.from),
            constraint: "workpiece_flow".to_string(),
            reason: format!(
                "{effect_name} would exceed capacity at endpoint '{}' (capacity={})",
                endpoint, registry.endpoints.capacities[endpoint_idx]
            ),
            violation_path: extend_path(path, transition),
            suggestion:
                "move or finish the existing workpiece before placing another one into the endpoint"
                    .to_string(),
        });
    }
    None
}

fn extend_path(path: &[String], transition: &Transition) -> Vec<String> {
    let mut out = path.to_vec();
    out.push(format_transition_label(transition));
    out
}

fn format_transition_label(transition: &Transition) -> String {
    format!(
        "{}.{} -> {}.{}",
        transition.from.task_name,
        transition.from.step_name,
        transition.to.task_name,
        transition.to.step_name
    )
}

fn workpiece_state_key(state: &State) -> (String, String) {
    (state.task_name.clone(), state.step_name.clone())
}

fn find_state_line(program: &PlcProgram, state: &State) -> usize {
    program
        .tasks
        .tasks
        .iter()
        .find(|task| task.name == state.task_name)
        .and_then(|task| task.steps.iter().find(|step| step.name == state.step_name))
        .map(|step| step.line.max(1))
        .unwrap_or(1)
}

fn workpiece_endpoint_matches_pattern(endpoint: &str, pattern: &str) -> bool {
    if endpoint == pattern {
        return true;
    }
    let Some((endpoint_carrier, endpoint_selectors)) = parse_slot_reference(endpoint) else {
        return false;
    };
    let Some((pattern_carrier, pattern_selectors)) = parse_slot_reference(pattern) else {
        return false;
    };
    if endpoint_carrier != pattern_carrier || endpoint_selectors.len() != pattern_selectors.len() {
        return false;
    }
    endpoint_selectors
        .iter()
        .zip(pattern_selectors.iter())
        .all(|(value, expected)| expected == "*" || value == expected)
}

fn parse_slot_reference(raw: &str) -> Option<(String, Vec<String>)> {
    let (carrier, rest) = raw.split_once(".slot[")?;
    let selectors = rest.strip_suffix(']')?;
    let values = selectors
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if carrier.is_empty() || values.is_empty() {
        return None;
    }
    Some((carrier.to_string(), values))
}

