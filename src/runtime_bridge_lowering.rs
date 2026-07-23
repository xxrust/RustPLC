/// Convert a compiler/semantic `StateMachine` IR into a minimal `runtime-core` `Program`.
///
/// Supported subset:
/// - `action`: set (digital), extend, retract
/// - `action`: log
/// - `action`: set_analog
/// - `wait`: boolean equality/inequality and arithmetic expression comparisons (no AND/OR/NOT)
/// - `delay`
/// - `timeout -> goto`
/// - `goto`
///
/// Notes:
/// - Runtime tasks are generated from condensed root task contexts (task-SCC roots without
///   external cross-task incoming edges).
/// - Per-task step graphs stay local (`StepId` is scoped per runtime task).
/// - Generated program owns one arena that releases all dynamic allocations on drop.
pub fn state_machine_to_runtime_program(
    topology: &TopologyGraph,
    constraints: &ConstraintSet,
    sm: &StateMachine,
    tick_ms: u64,
) -> Result<CompiledRuntimeProgram, BridgeError> {
    CompiledRuntimeProgram::try_new(Bump::new(), |arena| {
        lower_runtime_program(arena, topology, constraints, sm, tick_ms)
    })
}

fn lower_runtime_program<'a>(
    arena: &'a Bump,
    topology: &TopologyGraph,
    constraints: &ConstraintSet,
    sm: &StateMachine,
    tick_ms: u64,
) -> Result<Program<'a>, BridgeError> {
    if tick_ms == 0 {
        return Err(BridgeError::InvalidTickMs);
    }
    validate_extern_tick_budget(topology, sm, tick_ms)?;
    let workpiece_ctx = WorkpieceBridgeContext::new(arena, constraints, sm)?;

    let resolver = TopologyResolver::new(topology);
    let variable_indices = topology
        .variables
        .iter()
        .map(|v| (v.name.clone(), v.index))
        .collect::<HashMap<_, _>>();
    let cam_index_by_name = topology
        .cam_couplings
        .iter()
        .enumerate()
        .map(|(idx, cam)| (cam.name.clone(), idx as u16))
        .collect::<HashMap<_, _>>();
    let cam_table_index_by_name = topology
        .cam_tables
        .iter()
        .enumerate()
        .map(|(idx, table)| (table.name.clone(), idx as u16))
        .collect::<HashMap<_, _>>();
    let extern_signature_by_name = topology
        .extern_functions
        .iter()
        .map(|function| {
            (
                function.name.clone(),
                (function.params.len(), function.return_types.len()),
            )
        })
        .collect::<HashMap<_, _>>();

    let task_entry_states = sm
        .task_contexts
        .iter()
        .map(|ctx| (ctx.task_name.clone(), ctx.entry_state.clone()))
        .collect::<HashMap<_, _>>();
    if task_entry_states.is_empty() {
        return Err(BridgeError::MissingInitialState {
            state: format!("{}.{}", sm.initial.task_name, sm.initial.step_name),
        });
    }
    let known_state_keys = sm
        .states
        .iter()
        .map(|state| (state.task_name.clone(), state.step_name.clone()))
        .collect::<HashSet<_>>();
    let runtime_root_tasks = select_runtime_root_tasks(sm, &task_entry_states);
    if runtime_root_tasks.is_empty() {
        return Err(BridgeError::MissingInitialState {
            state: format!("{}.{}", sm.initial.task_name, sm.initial.step_name),
        });
    }

    // Index transitions by from-state.
    let mut outgoing_indices: HashMap<(String, String), Vec<usize>> = HashMap::new();
    for (idx, transition) in sm.transitions.iter().enumerate() {
        outgoing_indices
            .entry((
                transition.from.task_name.clone(),
                transition.from.step_name.clone(),
            ))
            .or_default()
            .push(idx);
    }

    let mut runtime_tasks: Vec<Task<'a>> = Vec::new();
    for root_task in runtime_root_tasks {
        let reachable_state_keys = collect_runtime_task_state_keys(
            &root_task,
            &task_entry_states,
            &known_state_keys,
            &outgoing_indices,
            sm,
        );
        if reachable_state_keys.is_empty() {
            continue;
        }

        let local_states = sm
            .states
            .iter()
            .filter(|state| {
                reachable_state_keys.contains(&(state.task_name.clone(), state.step_name.clone()))
            })
            .collect::<Vec<_>>();
        if local_states.is_empty() {
            continue;
        }

        let mut local_state_to_step = HashMap::<(String, String), StepId>::new();
        let mut step_names: Vec<&'a str> = Vec::with_capacity(local_states.len());
        for (idx, state) in local_states.iter().enumerate() {
            let name = format!("{}.{}", state.task_name, state.step_name);
            step_names.push(arena.alloc_str(&name));
            local_state_to_step.insert(
                (state.task_name.clone(), state.step_name.clone()),
                checked_step_id(&root_task, idx)?,
            );
        }

        let mut steps: Vec<Step<'a>> = step_names
            .iter()
            .map(|&name| Step {
                name,
                instr: Instr::Halt,
            })
            .collect();
        let local_task_entry_steps = task_entry_states
            .iter()
            .filter_map(|(task_name, entry_state)| {
                local_state_to_step
                    .get(&(entry_state.task_name.clone(), entry_state.step_name.clone()))
                    .copied()
                    .map(|step_id| (task_name.clone(), step_id))
            })
            .collect::<HashMap<_, _>>();

        for (idx, state) in local_states.iter().enumerate() {
            let state_name = format!("{}.{}", state.task_name, state.step_name);
            let state_key = (state.task_name.clone(), state.step_name.clone());
            let outs = outgoing_indices
                .get(&state_key)
                .map(|indices| {
                    indices
                        .iter()
                        .map(|transition_idx| &sm.transitions[*transition_idx])
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            let instr = convert_state_outgoing(
                arena,
                &resolver,
                &state_name,
                &outs,
                &workpiece_ctx,
                &local_state_to_step,
                &local_task_entry_steps,
                &mut steps,
                sm,
                tick_ms,
                &variable_indices,
                &cam_index_by_name,
                &cam_table_index_by_name,
                &extern_signature_by_name,
            )?;
            steps[idx].instr = instr;
        }

        let entry_state =
            task_entry_states
                .get(&root_task)
                .ok_or_else(|| BridgeError::MissingInitialState {
                    state: root_task.clone(),
                })?;
        let entry = local_state_to_step
            .get(&(entry_state.task_name.clone(), entry_state.step_name.clone()))
            .copied()
            .ok_or_else(|| BridgeError::MissingInitialState {
                state: format!("{}.{}", entry_state.task_name, entry_state.step_name),
            })?;

        let leaked_steps: &'a [Step<'a>] = arena.alloc_slice_copy(&steps);
        let leaked_task_name: &'a str = arena.alloc_str(&root_task);
        runtime_tasks.push(Task {
            name: leaked_task_name,
            steps: leaked_steps,
            entry,
        });
    }

    if runtime_tasks.is_empty() {
        return Err(BridgeError::MissingInitialState {
            state: format!("{}.{}", sm.initial.task_name, sm.initial.step_name),
        });
    }
    let leaked_tasks: &'a [Task<'a>] = arena.alloc_slice_copy(&runtime_tasks);

    let pid_loops = build_pid_configs(&resolver, topology, tick_ms)?;
    let cam_tables = build_cam_tables(topology);
    let cam_configs = build_cam_configs(&resolver, topology, &cam_table_index_by_name)?;
    let var_init = topology
        .variables
        .iter()
        .map(|var| var.initial_value)
        .collect::<Vec<_>>();
    let axis_fault_policies = build_axis_fault_policies(arena, topology);
    let semantic_resources = build_semantic_resources(arena, constraints);
    let resource_claims = build_resource_claims(arena, &resolver, constraints)?;
    Ok(Program {
        tasks: leaked_tasks,
        pid_loops: arena.alloc_slice_copy(&pid_loops),
        var_init: arena.alloc_slice_copy(&var_init),
        cam_configs: arena.alloc_slice_copy(&cam_configs),
        cam_tables: arena.alloc_slice_copy(&cam_tables),
        axis_fault_policies: arena.alloc_slice_copy(&axis_fault_policies),
        semantic_resources: arena.alloc_slice_copy(&semantic_resources),
        resource_claims: arena.alloc_slice_copy(&resource_claims),
        workpiece_types: workpiece_ctx.runtime_types,
        workpiece_sites: workpiece_ctx.runtime_sites,
        workpiece_holders: workpiece_ctx.runtime_holders,
    })
}

fn select_runtime_root_tasks(
    sm: &StateMachine,
    task_entry_states: &HashMap<String, State>,
) -> Vec<String> {
    crate::task_root_selection::select_root_task_contexts(sm, motion_branch_target_task_names)
        .into_iter()
        .filter(|task_name| task_entry_states.contains_key(task_name))
        .collect()
}

fn motion_branch_target_task_names(actions: &[TransitionAction]) -> Vec<String> {
    let mut targets = Vec::new();
    for action in actions {
        match action {
            TransitionAction::Extend {
                timeout,
                on_motion_fault,
                on_safety_fault,
                ..
            }
            | TransitionAction::Retract {
                timeout,
                on_motion_fault,
                on_safety_fault,
                ..
            } => {
                if let Some(timeout) = timeout {
                    targets.push(timeout.target_task.clone());
                }
                if let Some(on_motion_fault) = on_motion_fault {
                    targets.push(on_motion_fault.target_task.clone());
                }
                if let Some(on_safety_fault) = on_safety_fault {
                    targets.push(on_safety_fault.target_task.clone());
                }
            }
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

fn collect_runtime_task_state_keys(
    root_task: &str,
    task_entry_states: &HashMap<String, State>,
    known_state_keys: &HashSet<(String, String)>,
    outgoing_indices: &HashMap<(String, String), Vec<usize>>,
    sm: &StateMachine,
) -> HashSet<(String, String)> {
    let Some(entry_state) = task_entry_states.get(root_task) else {
        return HashSet::new();
    };

    let mut reachable = HashSet::<(String, String)>::new();
    let mut queue = VecDeque::new();
    queue.push_back((entry_state.task_name.clone(), entry_state.step_name.clone()));

    while let Some(state_key) = queue.pop_front() {
        if !reachable.insert(state_key.clone()) {
            continue;
        }

        let Some(transition_indices) = outgoing_indices.get(&state_key) else {
            continue;
        };

        for transition_idx in transition_indices {
            let transition = &sm.transitions[*transition_idx];
            queue.push_back((
                transition.to.task_name.clone(),
                transition.to.step_name.clone(),
            ));
            for branch_target in motion_branch_target_state_keys(
                &transition.actions,
                task_entry_states,
                known_state_keys,
            ) {
                queue.push_back(branch_target);
            }
        }
    }

    reachable
}

fn motion_branch_target_state_keys(
    actions: &[TransitionAction],
    task_entry_states: &HashMap<String, State>,
    known_state_keys: &HashSet<(String, String)>,
) -> Vec<(String, String)> {
    let mut targets = Vec::new();
    for action in actions {
        match action {
            TransitionAction::Extend {
                timeout,
                on_motion_fault,
                on_safety_fault,
                ..
            }
            | TransitionAction::Retract {
                timeout,
                on_motion_fault,
                on_safety_fault,
                ..
            } => {
                if let Some(timeout) = timeout {
                    push_axis_branch_target_state_key(
                        &mut targets,
                        &timeout.target_task,
                        &timeout.target_step,
                        task_entry_states,
                        known_state_keys,
                    );
                }
                if let Some(on_motion_fault) = on_motion_fault {
                    push_axis_branch_target_state_key(
                        &mut targets,
                        &on_motion_fault.target_task,
                        &on_motion_fault.target_step,
                        task_entry_states,
                        known_state_keys,
                    );
                }
                if let Some(on_safety_fault) = on_safety_fault {
                    push_axis_branch_target_state_key(
                        &mut targets,
                        &on_safety_fault.target_task,
                        &on_safety_fault.target_step,
                        task_entry_states,
                        known_state_keys,
                    );
                }
            }
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
                push_axis_branch_target_state_key(
                    &mut targets,
                    &timeout.target_task,
                    &timeout.target_step,
                    task_entry_states,
                    known_state_keys,
                );
                push_axis_branch_target_state_key(
                    &mut targets,
                    &on_reject.target_task,
                    &on_reject.target_step,
                    task_entry_states,
                    known_state_keys,
                );
                push_axis_branch_target_state_key(
                    &mut targets,
                    &on_motion_fault.target_task,
                    &on_motion_fault.target_step,
                    task_entry_states,
                    known_state_keys,
                );
                push_axis_branch_target_state_key(
                    &mut targets,
                    &on_safety_fault.target_task,
                    &on_safety_fault.target_step,
                    task_entry_states,
                    known_state_keys,
                );
                for route in on_reject_routes {
                    push_axis_branch_target_state_key(
                        &mut targets,
                        &route.target_task,
                        &route.target_step,
                        task_entry_states,
                        known_state_keys,
                    );
                }
                for route in on_motion_fault_routes {
                    push_axis_branch_target_state_key(
                        &mut targets,
                        &route.target_task,
                        &route.target_step,
                        task_entry_states,
                        known_state_keys,
                    );
                }
                for route in on_safety_fault_routes {
                    push_axis_branch_target_state_key(
                        &mut targets,
                        &route.target_task,
                        &route.target_step,
                        task_entry_states,
                        known_state_keys,
                    );
                }
            }
            _ => {}
        }
    }
    targets
}

fn push_axis_branch_target_state_key(
    out: &mut Vec<(String, String)>,
    target_task: &str,
    target_step: &Option<String>,
    task_entry_states: &HashMap<String, State>,
    known_state_keys: &HashSet<(String, String)>,
) {
    if let Some(step) = target_step {
        let key = (target_task.to_string(), step.clone());
        if known_state_keys.contains(&key) {
            out.push(key);
        }
        return;
    }

    if let Some(entry_state) = task_entry_states.get(target_task) {
        out.push((entry_state.task_name.clone(), entry_state.step_name.clone()));
    }
}

fn build_axis_fault_policies<'a>(
    arena: &'a Bump,
    topology: &TopologyGraph,
) -> Vec<AxisFaultPolicy<'a>> {
    topology
        .axis_fault_contracts
        .iter()
        .map(|contract| AxisFaultPolicy {
            axis: arena.alloc_str(&contract.axis),
            severity: lower_axis_fault_severity(contract.severity.clone()),
            stop_mode: lower_axis_stop_mode(contract.stop_mode.clone()),
            auto_reset_policy: lower_axis_auto_reset_policy(contract.auto_reset_policy.clone()),
            manual_ack_required: contract.manual_ack_required,
            propagation_scope: lower_axis_fault_propagation_scope(
                contract.propagation_scope.clone(),
            ),
            propagation_targets: leak_str_slice(arena, &contract.propagation_targets),
        })
        .collect()
}

fn build_semantic_resources<'a>(
    arena: &'a Bump,
    constraints: &ConstraintSet,
) -> Vec<RtSemanticResource<'a>> {
    constraints
        .semantic_resources
        .iter()
        .map(|resource| RtSemanticResource {
            name: arena.alloc_str(&resource.name),
            mode: lower_semantic_resource_mode(resource.mode.clone()),
        })
        .collect()
}

fn build_resource_claims<'a>(
    arena: &'a Bump,
    resolver: &TopologyResolver,
    constraints: &ConstraintSet,
) -> Result<Vec<RtResourceClaimRule<'a>>, BridgeError> {
    let resource_index_by_name = constraints
        .semantic_resources
        .iter()
        .enumerate()
        .map(|(idx, resource)| (resource.name.as_str(), idx as u16))
        .collect::<HashMap<_, _>>();

    let mut out = Vec::with_capacity(constraints.resource_claims.len());
    for claim in &constraints.resource_claims {
        let claim_text = render_resource_claim(claim);
        let Some(resource_index) = resource_index_by_name.get(claim.resource.as_str()).copied()
        else {
            return Err(BridgeError::UnsupportedSemanticResourceClaim {
                claim: claim_text,
                detail: format!("semantic resource `{}` is not declared", claim.resource),
            });
        };
        let source = lower_runtime_claim_source(arena, resolver, &claim_text, &claim.source)?;
        out.push(RtResourceClaimRule {
            source,
            resource_index,
        });
    }
    Ok(out)
}

#[derive(Clone, Debug)]
enum BridgeCarrierLayout {
    Slots { count: u32 },
    Grid { rows: u32, cols: u32 },
}

struct WorkpieceBridgeContext<'a> {
    carrier_layouts: HashMap<String, BridgeCarrierLayout>,
    merge_input_types: HashMap<(String, usize), &'a [&'a str]>,
    phase1_workpiece_type: Option<&'a str>,
    runtime_types: &'a [RtWorkpieceTypeDef<'a>],
    runtime_sites: &'a [RtWorkpieceSiteDef<'a>],
    runtime_holders: &'a [RtWorkpieceHolderDef<'a>],
}

impl<'a> WorkpieceBridgeContext<'a> {
    fn new(
        arena: &'a Bump,
        constraints: &ConstraintSet,
        sm: &StateMachine,
    ) -> Result<Self, BridgeError> {
        let carrier_layouts = collect_workpiece_carrier_layouts(&constraints.workpiece_carriers)?;
        let has_phase1_effects = sm
            .transitions
            .iter()
            .flat_map(|transition| transition.effects.iter())
            .any(workpiece_effect_requires_phase1_type);
        let phase1_workpiece_type = if has_phase1_effects {
            match constraints.workpiece_types.as_slice() {
                [workpiece] => Some(arena.alloc_str(&workpiece.name) as &'a str),
                _ => {
                    return Err(BridgeError::Phase1WorkpieceTypeArity {
                        count: constraints.workpiece_types.len(),
                    });
                }
            }
        } else {
            None
        };

        Ok(Self {
            carrier_layouts: carrier_layouts.clone(),
            merge_input_types: collect_workpiece_merge_input_types(
                arena,
                &constraints.workpiece_types,
            ),
            phase1_workpiece_type,
            runtime_types: leak_workpiece_types(
                arena,
                &constraints.workpiece_types,
                &carrier_layouts,
            )?,
            runtime_sites: leak_workpiece_sites(
                arena,
                &constraints.workpiece_sites,
                &constraints.workpiece_carriers,
            )?,
            runtime_holders: leak_workpiece_holders(arena, &constraints.workpiece_holders),
        })
    }
}

fn collect_workpiece_merge_input_types<'a>(
    arena: &'a Bump,
    workpiece_types: &[crate::ir::WorkpieceTypeDef],
) -> HashMap<(String, usize), &'a [&'a str]> {
    let mut merge_input_types = HashMap::new();
    for workpiece in workpiece_types {
        for rule in &workpiece.derived_from {
            let crate::ir::WorkpieceDerivationDef::Merge { inputs } = rule else {
                continue;
            };
            merge_input_types.insert(
                (workpiece.name.clone(), inputs.len()),
                leak_str_slice(arena, inputs),
            );
        }
    }
    merge_input_types
}

fn leak_workpiece_types<'a>(
    arena: &'a Bump,
    workpiece_types: &[crate::ir::WorkpieceTypeDef],
    carrier_layouts: &HashMap<String, BridgeCarrierLayout>,
) -> Result<&'a [RtWorkpieceTypeDef<'a>], BridgeError> {
    let leaked_types = workpiece_types
        .iter()
        .map(|workpiece| {
            Ok(RtWorkpieceTypeDef {
                name: arena.alloc_str(&workpiece.name),
                normal_terminal_states: leak_str_slice(arena, &workpiece.normal_terminal_states),
                abnormal_terminal_states: leak_str_slice(
                    arena,
                    &workpiece.abnormal_terminal_states,
                ),
                ingress_sites: leak_expanded_workpiece_endpoint_slice(
                    arena,
                    &workpiece.ingress_sites,
                    carrier_layouts,
                )?,
                normal_egress_sites: leak_expanded_workpiece_endpoint_slice(
                    arena,
                    &workpiece.normal_egress_sites,
                    carrier_layouts,
                )?,
                abnormal_egress_sites: leak_expanded_workpiece_endpoint_slice(
                    arena,
                    &workpiece.abnormal_egress_sites,
                    carrier_layouts,
                )?,
            })
        })
        .collect::<Result<Vec<_>, BridgeError>>()?;
    Ok(arena.alloc_slice_copy(&leaked_types))
}

fn leak_workpiece_sites<'a>(
    arena: &'a Bump,
    workpiece_sites: &[crate::ir::WorkpieceSiteDef],
    workpiece_carriers: &[crate::ir::WorkpieceCarrierDef],
) -> Result<&'a [RtWorkpieceSiteDef<'a>], BridgeError> {
    let mut leaked_sites = workpiece_sites
        .iter()
        .map(|site| RtWorkpieceSiteDef {
            name: arena.alloc_str(&site.name),
            kind: lower_workpiece_site_kind(site.kind.clone()),
            capacity: site.capacity,
        })
        .collect::<Vec<_>>();
    for carrier in workpiece_carriers {
        for endpoint in expand_all_carrier_slot_endpoints(&carrier.name, &carrier.layout)? {
            leaked_sites.push(RtWorkpieceSiteDef {
                name: arena.alloc_str(&endpoint),
                kind: RtWorkpieceSiteKind::CarrierLocation,
                capacity: 1,
            });
        }
    }
    Ok(arena.alloc_slice_copy(&leaked_sites))
}

fn leak_expanded_workpiece_endpoint_slice<'a>(
    arena: &'a Bump,
    endpoints: &[String],
    carrier_layouts: &HashMap<String, BridgeCarrierLayout>,
) -> Result<&'a [&'a str], BridgeError> {
    let expanded = endpoints
        .iter()
        .map(|endpoint| expand_runtime_endpoint_pattern(endpoint, carrier_layouts))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .map(|endpoint| arena.alloc_str(&endpoint) as &'a str)
        .collect::<Vec<_>>();
    Ok(arena.alloc_slice_copy(&expanded))
}

fn collect_workpiece_carrier_layouts(
    workpiece_carriers: &[crate::ir::WorkpieceCarrierDef],
) -> Result<HashMap<String, BridgeCarrierLayout>, BridgeError> {
    let mut layouts = HashMap::new();
    let mut total_slots = 0u64;
    for carrier in workpiece_carriers {
        let (layout, slot_count) = match &carrier.layout {
            crate::ir::WorkpieceCarrierLayoutDef::Slots { count } => (
                BridgeCarrierLayout::Slots { count: *count },
                u64::from(*count),
            ),
            crate::ir::WorkpieceCarrierLayoutDef::Grid { rows, cols } => {
                let slot_count = u64::from(*rows).checked_mul(u64::from(*cols)).ok_or(
                    BridgeError::WorkpieceCarrierSlotLimitExceeded {
                        total_slots: u64::MAX,
                        max_slots: MAX_RUNTIME_CARRIER_SLOTS,
                    },
                )?;
                (
                    BridgeCarrierLayout::Grid {
                        rows: *rows,
                        cols: *cols,
                    },
                    slot_count,
                )
            }
        };
        total_slots = total_slots.checked_add(slot_count).ok_or(
            BridgeError::WorkpieceCarrierSlotLimitExceeded {
                total_slots: u64::MAX,
                max_slots: MAX_RUNTIME_CARRIER_SLOTS,
            },
        )?;
        if total_slots > MAX_RUNTIME_CARRIER_SLOTS as u64 {
            return Err(BridgeError::WorkpieceCarrierSlotLimitExceeded {
                total_slots,
                max_slots: MAX_RUNTIME_CARRIER_SLOTS,
            });
        }
        layouts.insert(carrier.name.clone(), layout);
    }
    Ok(layouts)
}

fn workpiece_effect_requires_phase1_type(effect: &crate::ir::WorkpieceEffect) -> bool {
    matches!(
        effect,
        crate::ir::WorkpieceEffect::Acquire { .. }
            | crate::ir::WorkpieceEffect::Transfer { .. }
            | crate::ir::WorkpieceEffect::Finish { .. }
    )
}

fn expand_runtime_endpoint_pattern(
    endpoint: &str,
    carrier_layouts: &HashMap<String, BridgeCarrierLayout>,
) -> Result<Vec<String>, BridgeError> {
    let Some((carrier, selectors)) = parse_workpiece_slot_reference(endpoint) else {
        return Ok(vec![endpoint.to_string()]);
    };
    let Some(layout) = carrier_layouts.get(&carrier) else {
        return Err(BridgeError::UnknownWorkpieceCarrier { carrier });
    };
    expand_slot_reference(&carrier, &selectors, layout)
}

fn validate_runtime_effect_endpoint<'a>(
    arena: &'a Bump,
    endpoint: &str,
    carrier_layouts: &HashMap<String, BridgeCarrierLayout>,
) -> Result<&'a str, BridgeError> {
    let expanded = expand_runtime_endpoint_pattern(endpoint, carrier_layouts)?;
    match expanded.as_slice() {
        [single] => Ok(arena.alloc_str(single)),
        _ => Err(BridgeError::InvalidWorkpieceSlotReference {
            slot: endpoint.to_string(),
            details: "runtime effects must use a concrete slot index, not a wildcard".to_string(),
        }),
    }
}

fn validate_runtime_carrier_name<'a>(
    arena: &'a Bump,
    carrier: &str,
    carrier_layouts: &HashMap<String, BridgeCarrierLayout>,
) -> Result<&'a str, BridgeError> {
    if carrier_layouts.contains_key(carrier) {
        Ok(arena.alloc_str(carrier))
    } else {
        Err(BridgeError::UnknownWorkpieceCarrier {
            carrier: carrier.to_string(),
        })
    }
}

fn expand_all_carrier_slot_endpoints(
    carrier: &str,
    layout: &crate::ir::WorkpieceCarrierLayoutDef,
) -> Result<Vec<String>, BridgeError> {
    match layout {
        crate::ir::WorkpieceCarrierLayoutDef::Slots { count } => Ok((0..*count)
            .map(|slot| format!("{carrier}.slot[{slot}]"))
            .collect()),
        crate::ir::WorkpieceCarrierLayoutDef::Grid { rows, cols } => {
            let slot_count =
                usize::try_from(u64::from(*rows) * u64::from(*cols)).map_err(|_| {
                    BridgeError::WorkpieceCarrierSlotLimitExceeded {
                        total_slots: u64::MAX,
                        max_slots: MAX_RUNTIME_CARRIER_SLOTS,
                    }
                })?;
            let mut out = Vec::with_capacity(slot_count);
            for row in 0..*rows {
                for col in 0..*cols {
                    out.push(format!("{carrier}.slot[{row},{col}]"));
                }
            }
            Ok(out)
        }
    }
}

fn checked_step_id(task: &str, index: usize) -> Result<StepId, BridgeError> {
    let raw = u16::try_from(index).map_err(|_| BridgeError::TooManyRuntimeSteps {
        task: task.to_string(),
        step_count: index.saturating_add(1),
        max_steps: usize::from(u16::MAX).saturating_add(1),
    })?;
    Ok(StepId(raw))
}

fn expand_slot_reference(
    carrier: &str,
    selectors: &[String],
    layout: &BridgeCarrierLayout,
) -> Result<Vec<String>, BridgeError> {
    match layout {
        BridgeCarrierLayout::Slots { count } => {
            if selectors.len() != 1 {
                return Err(BridgeError::InvalidWorkpieceSlotReference {
                    slot: render_slot_reference(carrier, selectors),
                    details: format!("carrier '{carrier}' expects 1 slot dimension"),
                });
            }
            let slots = expand_slot_selector(carrier, selectors, 0, *count)?;
            Ok(slots
                .into_iter()
                .map(|slot| format!("{carrier}.slot[{slot}]"))
                .collect())
        }
        BridgeCarrierLayout::Grid { rows, cols } => {
            if selectors.len() != 2 {
                return Err(BridgeError::InvalidWorkpieceSlotReference {
                    slot: render_slot_reference(carrier, selectors),
                    details: format!("carrier '{carrier}' expects 2 slot dimensions"),
                });
            }
            let row_values = expand_slot_selector(carrier, selectors, 0, *rows)?;
            let col_values = expand_slot_selector(carrier, selectors, 1, *cols)?;
            let mut out = Vec::with_capacity(row_values.len().saturating_mul(col_values.len()));
            for row in &row_values {
                for col in &col_values {
                    out.push(format!("{carrier}.slot[{row},{col}]"));
                }
            }
            Ok(out)
        }
    }
}

fn expand_slot_selector(
    carrier: &str,
    selectors: &[String],
    dim_idx: usize,
    bound: u32,
) -> Result<Vec<u32>, BridgeError> {
    let selector =
        selectors
            .get(dim_idx)
            .ok_or_else(|| BridgeError::InvalidWorkpieceSlotReference {
                slot: render_slot_reference(carrier, selectors),
                details: format!("missing slot selector at dimension {}", dim_idx + 1),
            })?;
    if selector == "*" {
        return Ok((0..bound).collect());
    }
    let parsed =
        selector
            .parse::<u32>()
            .map_err(|_| BridgeError::InvalidWorkpieceSlotReference {
                slot: render_slot_reference(carrier, selectors),
                details: format!("slot selector '{selector}' must be '*' or an integer"),
            })?;
    if parsed >= bound {
        return Err(BridgeError::InvalidWorkpieceSlotReference {
            slot: render_slot_reference(carrier, selectors),
            details: format!(
                "slot index {parsed} is out of range for carrier '{carrier}' dimension {}",
                dim_idx + 1
            ),
        });
    }
    Ok(vec![parsed])
}

fn parse_workpiece_slot_reference(raw: &str) -> Option<(String, Vec<String>)> {
    let (carrier, rest) = raw.split_once(".slot[")?;
    let selectors = rest.strip_suffix(']')?;
    Some((
        carrier.to_string(),
        selectors
            .split(',')
            .map(|selector| selector.trim().to_string())
            .collect(),
    ))
}

fn render_slot_reference(carrier: &str, selectors: &[String]) -> String {
    format!("{carrier}.slot[{}]", selectors.join(","))
}

fn leak_workpiece_holders<'a>(
    arena: &'a Bump,
    workpiece_holders: &[crate::ir::WorkpieceHolderDef],
) -> &'a [RtWorkpieceHolderDef<'a>] {
    let leaked_holders = workpiece_holders
        .iter()
        .map(|holder| RtWorkpieceHolderDef {
            name: arena.alloc_str(&holder.name),
            capacity: holder.capacity,
        })
        .collect::<Vec<_>>();
    arena.alloc_slice_copy(&leaked_holders)
}

fn lower_workpiece_site_kind(kind: crate::ir::WorkpieceSiteKind) -> RtWorkpieceSiteKind {
    match kind {
        crate::ir::WorkpieceSiteKind::WorkpieceLocation => RtWorkpieceSiteKind::WorkpieceLocation,
        crate::ir::WorkpieceSiteKind::CarrierLocation => RtWorkpieceSiteKind::CarrierLocation,
    }
}

fn lower_runtime_claim_source<'a>(
    arena: &'a Bump,
    resolver: &TopologyResolver,
    claim_text: &str,
    source: &crate::ir::ResourceClaimSource,
) -> Result<RtResourceClaimSource<'a>, BridgeError> {
    match source {
        crate::ir::ResourceClaimSource::ActionTag { tag } => Ok(RtResourceClaimSource::ActionTag {
            tag: arena.alloc_str(tag),
        }),
        crate::ir::ResourceClaimSource::State(state_expr) => {
            lower_runtime_state_claim_source(resolver, claim_text, state_expr)
        }
    }
}

fn lower_runtime_state_claim_source<'a>(
    resolver: &TopologyResolver,
    claim_text: &str,
    state_expr: &crate::ir::StateExpr,
) -> Result<RtResourceClaimSource<'a>, BridgeError> {
    let Some(value) = binary_state_value(&state_expr.state) else {
        return Err(BridgeError::UnsupportedSemanticResourceClaim {
            claim: claim_text.to_string(),
            detail: format!(
                "state `{}` is not runtime-lowerable; only binary output-backed states are supported",
                render_state_expr(state_expr)
            ),
        });
    };
    let id = resolver
        .resolve_digital_output_id(
            "semantic resource claim",
            &state_expr.device,
            &state_expr.port,
        )
        .map_err(|err| BridgeError::UnsupportedSemanticResourceClaim {
            claim: claim_text.to_string(),
            detail: err.to_string(),
        })?;
    if id.0 as usize >= MAX_TRACKED_DIGITAL_OUTPUTS {
        return Err(BridgeError::UnsupportedSemanticResourceClaim {
            claim: claim_text.to_string(),
            detail: format!(
                "digital output {} exceeds runtime shadow limit {}",
                id.0, MAX_TRACKED_DIGITAL_OUTPUTS
            ),
        });
    }
    Ok(RtResourceClaimSource::DigitalOutputState { id, value })
}

fn lower_semantic_resource_mode(mode: crate::ir::SemanticResourceMode) -> RtSemanticResourceMode {
    match mode {
        crate::ir::SemanticResourceMode::Exclusive => RtSemanticResourceMode::Exclusive,
    }
}

fn binary_state_value(state: &str) -> Option<bool> {
    match state {
        "on" | "forward" | "active" | "extended" => Some(true),
        "off" | "reverse" | "idle" | "retracted" => Some(false),
        _ => None,
    }
}

fn render_resource_claim(claim: &crate::ir::ResourceClaimRule) -> String {
    let source = match &claim.source {
        crate::ir::ResourceClaimSource::ActionTag { tag } => format!("action_tag {tag}"),
        crate::ir::ResourceClaimSource::State(state_expr) => render_state_expr(state_expr),
    };
    format!("claim: {source} occupies {}", claim.resource)
}

fn render_state_expr(state_expr: &crate::ir::StateExpr) -> String {
    if state_expr.port == "self" {
        format!("{}.{}", state_expr.device, state_expr.state)
    } else {
        format!(
            "{}.{}.{}",
            state_expr.device, state_expr.port, state_expr.state
        )
    }
}

fn leak_str_slice<'a>(arena: &'a Bump, values: &[String]) -> &'a [&'a str] {
    let leaked_values = values
        .iter()
        .map(|value| arena.alloc_str(value) as &'a str)
        .collect::<Vec<_>>();
    arena.alloc_slice_copy(&leaked_values)
}

fn lower_axis_fault_severity(severity: IrAxisFaultSeverity) -> RtAxisFaultSeverity {
    match severity {
        IrAxisFaultSeverity::Recoverable => RtAxisFaultSeverity::Recoverable,
        IrAxisFaultSeverity::NonRecoverable => RtAxisFaultSeverity::NonRecoverable,
        IrAxisFaultSeverity::Safety => RtAxisFaultSeverity::Safety,
    }
}

fn lower_axis_stop_mode(stop_mode: IrAxisStopMode) -> RtAxisStopMode {
    match stop_mode {
        IrAxisStopMode::Controlled => RtAxisStopMode::Controlled,
        IrAxisStopMode::Quick => RtAxisStopMode::Quick,
        IrAxisStopMode::Immediate => RtAxisStopMode::Immediate,
    }
}

fn lower_axis_auto_reset_policy(policy: IrAxisAutoResetPolicy) -> RtAxisAutoResetPolicy {
    match policy {
        IrAxisAutoResetPolicy::Never => RtAxisAutoResetPolicy::Never,
        IrAxisAutoResetPolicy::OnClear => RtAxisAutoResetPolicy::OnClear,
        IrAxisAutoResetPolicy::Immediate => RtAxisAutoResetPolicy::Immediate,
    }
}

fn lower_axis_fault_propagation_scope(
    scope: IrAxisFaultPropagationScope,
) -> RtAxisFaultPropagationScope {
    match scope {
        IrAxisFaultPropagationScope::SelfOnly => RtAxisFaultPropagationScope::SelfOnly,
        IrAxisFaultPropagationScope::Group => RtAxisFaultPropagationScope::Group,
        IrAxisFaultPropagationScope::All => RtAxisFaultPropagationScope::All,
        IrAxisFaultPropagationScope::Followers => RtAxisFaultPropagationScope::Followers,
        IrAxisFaultPropagationScope::Custom => RtAxisFaultPropagationScope::Custom,
    }
}

fn validate_extern_tick_budget(
    topology: &TopologyGraph,
    sm: &StateMachine,
    tick_ms: u64,
) -> Result<(), BridgeError> {
    let tick_budget_us = tick_ms.saturating_mul(1_000);
    if tick_budget_us == 0 {
        return Ok(());
    }

    let extern_bound_us = topology
        .extern_functions
        .iter()
        .map(|function| (function.name.as_str(), function.contract.time_bound_us))
        .collect::<HashMap<_, _>>();
    if extern_bound_us.is_empty() {
        return Ok(());
    }

    let mut outgoing: HashMap<(String, String), Vec<&Transition>> = HashMap::new();
    for transition in &sm.transitions {
        outgoing
            .entry((
                transition.from.task_name.clone(),
                transition.from.step_name.clone(),
            ))
            .or_default()
            .push(transition);
    }

    let task_entry_states = sm
        .task_contexts
        .iter()
        .map(|ctx| (ctx.task_name.clone(), ctx.entry_state.clone()))
        .collect::<HashMap<_, _>>();
    let roots = select_runtime_root_tasks(sm, &task_entry_states);
    let mut memo = HashMap::<((String, String), usize), u64>::new();
    let mut worst_case_us = 0u64;
    for root in roots {
        let mut reachable = HashSet::new();
        let mut queue = VecDeque::new();
        if let Some(entry) = task_entry_states.get(&root) {
            queue.push_back((entry.task_name.clone(), entry.step_name.clone()));
        }
        while let Some(state_key) = queue.pop_front() {
            if !reachable.insert(state_key.clone()) {
                continue;
            }
            if let Some(transitions) = outgoing.get(&state_key) {
                for transition in transitions {
                    queue.push_back((
                        transition.to.task_name.clone(),
                        transition.to.step_name.clone(),
                    ));
                }
            }
        }
        let root_cost = reachable.into_iter().fold(0u64, |best, state_key| {
            best.max(worst_case_extern_cost_from_state(
                &state_key,
                MAX_TRANSITIONS_PER_TASK_PER_TICK,
                &outgoing,
                &extern_bound_us,
                &mut memo,
            ))
        });
        worst_case_us = worst_case_us.saturating_add(root_cost);
    }

    if worst_case_us > tick_budget_us {
        return Err(BridgeError::ExternTickBudgetExceeded {
            tick_ms,
            tick_budget_us,
            worst_case_us,
        });
    }

    Ok(())
}

fn worst_case_extern_cost_from_state(
    state: &(String, String),
    remaining_transitions: usize,
    outgoing: &HashMap<(String, String), Vec<&Transition>>,
    extern_bound_us: &HashMap<&str, u64>,
    memo: &mut HashMap<((String, String), usize), u64>,
) -> u64 {
    if remaining_transitions == 0 {
        return 0;
    }
    let cache_key = (state.clone(), remaining_transitions);
    if let Some(cached) = memo.get(&cache_key).copied() {
        return cached;
    }

    let mut best = 0u64;
    if let Some(transitions) = outgoing.get(state) {
        for transition in transitions {
            let action_cost = extern_action_cost_us(&transition.actions, extern_bound_us);
            let next_state = (
                transition.to.task_name.clone(),
                transition.to.step_name.clone(),
            );
            let tail = worst_case_extern_cost_from_state(
                &next_state,
                remaining_transitions.saturating_sub(1),
                outgoing,
                extern_bound_us,
                memo,
            );
            best = best.max(action_cost.saturating_add(tail));
        }
    }

    memo.insert(cache_key, best);
    best
}

fn extern_action_cost_us(
    actions: &[TransitionAction],
    extern_bound_us: &HashMap<&str, u64>,
) -> u64 {
    actions.iter().fold(0u64, |total, action| {
        if let TransitionAction::CallExtern { function, .. } = action {
            let bound = extern_bound_us.get(function.as_str()).copied().unwrap_or(0);
            total.saturating_add(bound)
        } else {
            total
        }
    })
}

fn build_pid_configs(
    resolver: &TopologyResolver,
    topology: &TopologyGraph,
    tick_ms: u64,
) -> Result<Vec<PidConfig>, BridgeError> {
    let mut out = Vec::new();

    for loop_spec in &topology.pid_loops {
        let pid_name = loop_spec.name.clone();
        let ctx = format!("pid:{pid_name}");

        let period_ticks = if loop_spec.period_ms % tick_ms != 0 {
            return Err(BridgeError::PidPeriodNotAligned {
                pid: pid_name,
                period_ms: loop_spec.period_ms,
                tick_ms,
            });
        } else {
            (loop_spec.period_ms / tick_ms).max(1)
        };

        let parse = |field: &str, value: &str| -> Result<f32, BridgeError> {
            value
                .parse::<f32>()
                .map_err(|_| BridgeError::InvalidPidLiteral {
                    pid: pid_name.clone(),
                    field: field.to_string(),
                    value: value.to_string(),
                })
        };

        let pv = resolver.resolve_analog_input_id(&ctx, &loop_spec.pv)?;
        let out_id = resolver.resolve_analog_output_id(&ctx, &loop_spec.out, "self")?;

        let cfg = PidConfig {
            pv,
            out: out_id,
            sp: parse("sp", &loop_spec.sp)?,
            kp: parse("kp", &loop_spec.kp)?,
            ki: parse("ki", &loop_spec.ki)?,
            kd: parse("kd", &loop_spec.kd)?,
            dt_s: (loop_spec.period_ms as f32) / 1000.0,
            period_ticks,
            limit_min: parse("limit_min", &loop_spec.limit_min)?,
            limit_max: parse("limit_max", &loop_spec.limit_max)?,
            anti_windup: AntiWindup::ConditionalIntegration,
        };

        out.push(cfg);
    }

    Ok(out)
}

fn build_cam_tables(topology: &TopologyGraph) -> Vec<CamTableData> {
    let mut out = Vec::with_capacity(topology.cam_tables.len());

    for table in &topology.cam_tables {
        let mut master = [0.0f32; MAX_CAM_POINTS];
        let mut slave = [0.0f32; MAX_CAM_POINTS];
        let mut coeffs = [RtSplineCoeff {
            a: 0.0,
            b: 0.0,
            c: 0.0,
            d: 0.0,
        }; MAX_CAM_POINTS];

        for (idx, value) in table.master_positions.iter().copied().enumerate() {
            if idx >= MAX_CAM_POINTS {
                break;
            }
            master[idx] = value;
        }
        for (idx, value) in table.slave_positions.iter().copied().enumerate() {
            if idx >= MAX_CAM_POINTS {
                break;
            }
            slave[idx] = value;
        }
        for (idx, c) in table.spline_coeffs.iter().enumerate() {
            if idx >= MAX_CAM_POINTS {
                break;
            }
            coeffs[idx] = RtSplineCoeff {
                a: c.a,
                b: c.b,
                c: c.c,
                d: c.d,
            };
        }

        out.push(CamTableData {
            periodic: table.periodic,
            num_points: table.num_points.min(MAX_CAM_POINTS) as u16,
            master,
            slave,
            coeffs,
            last_index: 0,
        });
    }

    out
}
