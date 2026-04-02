use crate::ir::{
    AxisAutoResetPolicy as IrAxisAutoResetPolicy,
    AxisFaultPropagationScope as IrAxisFaultPropagationScope,
    AxisFaultRouteKind as IrAxisFaultRouteKind, AxisFaultSeverity as IrAxisFaultSeverity,
    AxisStopMode as IrAxisStopMode, BinaryValue as IrBinaryValue,
    CamInterpolation as IrCamInterpolation, ConstraintSet, DeviceKind, State, StateMachine,
    TopologyGraph, Transition, TransitionAction, TransitionGuard,
};
use crate::device_semantics::axis::move_transition_view as axis_move_transition_view;
use crate::device_semantics::cylinder::{
    complementary_end_state_port as cylinder_complementary_state_port,
    is_end_state_port as is_cylinder_end_state_port,
    state_port_key,
    CylinderStrokeVerb,
};
use crate::plc_port::{PlcPortKind, parse_physical_plc_port_ref};
use io_traits::{AnalogInputId, AnalogOutputId, DigitalInputId, DigitalOutputId};
use petgraph::Direction;
use petgraph::graph::NodeIndex;
use runtime_core::{
    Action, AnalogRange, AntiWindup, AxisAutoResetPolicy as RtAxisAutoResetPolicy, AxisFaultPolicy,
    AxisFaultPropagationScope as RtAxisFaultPropagationScope,
    AxisFaultRouteKind as RtAxisFaultRouteKind, AxisFaultRouteRule, AxisFaultRouting,
    AxisFaultSeverity as RtAxisFaultSeverity, AxisMotionCommand, AxisMoveKind,
    AxisStopMode as RtAxisStopMode, CamAnalogField, CamCouplingConfig, CamDigitalField,
    CamInterpolation as RtCamInterpolation, CamTableData, CompareOp, CylinderFaultRouting,
    DigitalCondition, ExprOp, ExprProgram, Instr, MAX_CAM_POINTS, MAX_TRACKED_DIGITAL_OUTPUTS,
    MAX_TRANSITIONS_PER_TASK_PER_TICK, PidConfig, Program,
    ResourceClaimRule as RtResourceClaimRule, ResourceClaimSource as RtResourceClaimSource,
    SemanticResource as RtSemanticResource, SemanticResourceMode as RtSemanticResourceMode,
    SplineCoeff as RtSplineCoeff, Step, StepId, Task, Timeout,
    WorkpieceHolderDef as RtWorkpieceHolderDef, WorkpieceSiteDef as RtWorkpieceSiteDef,
    WorkpieceSiteKind as RtWorkpieceSiteKind, WorkpieceTypeDef as RtWorkpieceTypeDef,
};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("tick_ms must be > 0")]
    InvalidTickMs,

    #[error("duration {duration_ms}ms is not aligned to tick_ms={tick_ms} (state {state})")]
    DurationNotAligned {
        state: String,
        duration_ms: u64,
        tick_ms: u64,
    },

    #[error("state machine initial state {state} is not present in states list")]
    MissingInitialState { state: String },

    #[error("transition from {state} points to unknown target state {target}")]
    UnknownTransitionTarget { state: String, target: String },

    #[error("unsupported transition shape for {state}: {details}")]
    UnsupportedTransitionShape { state: String, details: String },

    #[error("unsupported guard expression in {state}: {expression}")]
    UnsupportedGuardExpression { state: String, expression: String },

    #[error(
        "closed-loop cylinder {device} in {state} is missing required complementary feedback for action state {requested_state}"
    )]
    IncompleteClosedLoopCylinderMotion {
        state: String,
        device: String,
        requested_state: String,
    },

    #[error(
        "closed-loop cylinder {device} in {state} must declare both on_motion_fault and on_safety_fault when cylinder fault routing is used"
    )]
    IncompleteClosedLoopCylinderRouting { state: String, device: String },

    #[error("unsupported action in {state}: {action}")]
    UnsupportedAction { state: String, action: String },

    #[error("device {device} referenced in {state} is not defined in topology")]
    UnknownDevice { state: String, device: String },

    #[error(
        "unable to resolve a unique physical digital input for device {device} (state {state})"
    )]
    UnresolvableDigitalInput { state: String, device: String },

    #[error(
        "unable to resolve a unique physical digital output for device {device} (state {state})"
    )]
    UnresolvableDigitalOutput { state: String, device: String },

    #[error("unable to resolve a unique physical analog input for device {device} (state {state})")]
    UnresolvableAnalogInput { state: String, device: String },

    #[error(
        "unable to resolve a unique physical analog output for device {device} (state {state})"
    )]
    UnresolvableAnalogOutput { state: String, device: String },

    #[error("invalid analog literal in {state}: set_analog {target} {value_raw}")]
    InvalidAnalogLiteral {
        state: String,
        target: String,
        value_raw: String,
    },

    #[error("invalid axis literal in {state}: {field} of {target} = {value_raw}")]
    InvalidAxisLiteral {
        state: String,
        target: String,
        field: String,
        value_raw: String,
    },

    #[error("axis profile for {target} is missing in topology (state {state})")]
    MissingAxisProfile { state: String, target: String },

    #[error(
        "axis speed {speed} exceeds configured max_speed={max_speed} for {target} (state {state})"
    )]
    AxisSpeedOutOfRange {
        state: String,
        target: String,
        speed: f32,
        max_speed: f32,
    },

    #[error("unsupported analog wait guard in {state}: {expression}")]
    UnsupportedAnalogWait { state: String, expression: String },

    #[error("analog input {device} has no region table in state machine (state {state})")]
    MissingAnalogRegions { state: String, device: String },

    #[error("pid loop {pid} period_ms={period_ms} is not aligned to tick_ms={tick_ms}")]
    PidPeriodNotAligned {
        pid: String,
        period_ms: u64,
        tick_ms: u64,
    },

    #[error("pid loop {pid} has invalid literal for {field}: {value}")]
    InvalidPidLiteral {
        pid: String,
        field: String,
        value: String,
    },

    #[error(
        "Phase 1 workpiece lowering requires exactly one declared workpiece type, found {count}"
    )]
    Phase1WorkpieceTypeArity { count: usize },

    #[error("workpiece carrier {carrier} is not declared in runtime bridge metadata")]
    UnknownWorkpieceCarrier { carrier: String },

    #[error("invalid workpiece slot reference {slot}: {details}")]
    InvalidWorkpieceSlotReference { slot: String, details: String },

    #[error("unsupported workpiece effect in {state}: {effect}")]
    UnsupportedWorkpieceEffect { state: String, effect: String },

    #[error("cam_coupling {cam} references unknown cam table {table}")]
    UnknownCamTableReference { cam: String, table: String },

    #[error(
        "extern worst-case execution budget exceeded: {worst_case_us}us > tick budget {tick_budget_us}us (tick_ms={tick_ms})"
    )]
    ExternTickBudgetExceeded {
        tick_ms: u64,
        tick_budget_us: u64,
        worst_case_us: u64,
    },

    #[error("unsupported semantic resource claim `{claim}`: {detail}")]
    UnsupportedSemanticResourceClaim { claim: String, detail: String },
}

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
/// - Runtime tasks are generated from root task contexts (tasks without cross-task incoming edges).
/// - Per-task step graphs stay local (`StepId` is scoped per runtime task).
/// - Generated program uses leaked allocations to produce a `'static` `Program`.
pub fn state_machine_to_runtime_program(
    topology: &TopologyGraph,
    constraints: &ConstraintSet,
    sm: &StateMachine,
    tick_ms: u64,
) -> Result<Program<'static>, BridgeError> {
    if tick_ms == 0 {
        return Err(BridgeError::InvalidTickMs);
    }
    validate_extern_tick_budget(topology, sm, tick_ms)?;
    let workpiece_ctx = WorkpieceBridgeContext::new(constraints, sm)?;

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

    let mut runtime_tasks: Vec<Task<'static>> = Vec::new();
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
        let mut step_names: Vec<&'static str> = Vec::with_capacity(local_states.len());
        for (idx, state) in local_states.iter().enumerate() {
            let name = format!("{}.{}", state.task_name, state.step_name);
            let leaked_name: &'static str = Box::leak(name.into_boxed_str());
            step_names.push(leaked_name);
            local_state_to_step.insert(
                (state.task_name.clone(), state.step_name.clone()),
                StepId(idx as u16),
            );
        }

        let mut steps: Vec<Step<'static>> = step_names
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

        let leaked_steps: &'static [Step<'static>] = Box::leak(steps.into_boxed_slice());
        let leaked_task_name: &'static str = Box::leak(root_task.into_boxed_str());
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
    let leaked_tasks: &'static [Task<'static>] = Box::leak(runtime_tasks.into_boxed_slice());

    let leaked_pid_loops: &'static [PidConfig] =
        Box::leak(build_pid_configs(&resolver, topology, tick_ms)?.into_boxed_slice());
    let leaked_cam_tables: &'static [CamTableData] =
        Box::leak(build_cam_tables(topology).into_boxed_slice());
    let leaked_cam_configs: &'static [CamCouplingConfig] = Box::leak(
        build_cam_configs(&resolver, topology, &cam_table_index_by_name)?.into_boxed_slice(),
    );
    let leaked_var_init: &'static [f32] = Box::leak(
        topology
            .variables
            .iter()
            .map(|var| var.initial_value)
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    let leaked_axis_fault_policies: &'static [AxisFaultPolicy<'static>] =
        Box::leak(build_axis_fault_policies(topology).into_boxed_slice());
    let leaked_semantic_resources: &'static [RtSemanticResource<'static>] =
        Box::leak(build_semantic_resources(constraints).into_boxed_slice());
    let leaked_resource_claims: &'static [RtResourceClaimRule<'static>] =
        Box::leak(build_resource_claims(&resolver, constraints)?.into_boxed_slice());
    Ok(Program {
        tasks: leaked_tasks,
        pid_loops: leaked_pid_loops,
        var_init: leaked_var_init,
        cam_configs: leaked_cam_configs,
        cam_tables: leaked_cam_tables,
        axis_fault_policies: leaked_axis_fault_policies,
        semantic_resources: leaked_semantic_resources,
        resource_claims: leaked_resource_claims,
        workpiece_types: workpiece_ctx.runtime_types,
        workpiece_sites: workpiece_ctx.runtime_sites,
        workpiece_holders: workpiece_ctx.runtime_holders,
    })
}

fn select_runtime_root_tasks(
    sm: &StateMachine,
    task_entry_states: &HashMap<String, State>,
) -> Vec<String> {
    let mut cross_task_incoming = HashSet::<String>::new();
    for transition in &sm.transitions {
        if transition.from.task_name != transition.to.task_name {
            cross_task_incoming.insert(transition.to.task_name.clone());
        }
        for target_task in motion_branch_target_task_names(&transition.actions) {
            if transition.from.task_name != target_task {
                cross_task_incoming.insert(target_task);
            }
        }
    }

    let mut roots = Vec::new();
    for ctx in &sm.task_contexts {
        if task_entry_states.contains_key(&ctx.task_name)
            && !cross_task_incoming.contains(&ctx.task_name)
        {
            roots.push(ctx.task_name.clone());
        }
    }

    if roots.is_empty() {
        if task_entry_states.contains_key(&sm.initial.task_name) {
            roots.push(sm.initial.task_name.clone());
        } else if let Some(first) = sm.task_contexts.first() {
            roots.push(first.task_name.clone());
        }
    }

    roots
}

fn motion_branch_target_task_names(actions: &[TransitionAction]) -> Vec<String> {
    let mut targets = Vec::new();
    for action in actions {
        if let Some(view) = axis_move_transition_view(action) {
            view.for_each_target(|task: &str, _: Option<&str>| targets.push(task.to_string()));
            continue;
        }
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
        if let Some(view) = axis_move_transition_view(action) {
            view.for_each_target(|task: &str, step: Option<&str>| {
                let target_step = step.map(str::to_string);
                push_axis_branch_target_state_key(
                    &mut targets,
                    task,
                    &target_step,
                    task_entry_states,
                    known_state_keys,
                );
            });
            continue;
        }
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

fn build_axis_fault_policies(topology: &TopologyGraph) -> Vec<AxisFaultPolicy<'static>> {
    topology
        .axis_fault_contracts
        .iter()
        .map(|contract| AxisFaultPolicy {
            axis: Box::leak(contract.axis.clone().into_boxed_str()),
            severity: lower_axis_fault_severity(contract.severity.clone()),
            stop_mode: lower_axis_stop_mode(contract.stop_mode.clone()),
            auto_reset_policy: lower_axis_auto_reset_policy(contract.auto_reset_policy.clone()),
            manual_ack_required: contract.manual_ack_required,
            propagation_scope: lower_axis_fault_propagation_scope(
                contract.propagation_scope.clone(),
            ),
            propagation_targets: leak_str_slice(&contract.propagation_targets),
        })
        .collect()
}

fn build_semantic_resources(constraints: &ConstraintSet) -> Vec<RtSemanticResource<'static>> {
    constraints
        .semantic_resources
        .iter()
        .map(|resource| RtSemanticResource {
            name: Box::leak(resource.name.clone().into_boxed_str()),
            mode: lower_semantic_resource_mode(resource.mode.clone()),
        })
        .collect()
}

fn build_resource_claims(
    resolver: &TopologyResolver,
    constraints: &ConstraintSet,
) -> Result<Vec<RtResourceClaimRule<'static>>, BridgeError> {
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
        let source = lower_runtime_claim_source(resolver, &claim_text, &claim.source)?;
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

struct WorkpieceBridgeContext {
    carrier_layouts: HashMap<String, BridgeCarrierLayout>,
    merge_input_types: HashMap<(String, usize), &'static [&'static str]>,
    phase1_workpiece_type: Option<&'static str>,
    runtime_types: &'static [RtWorkpieceTypeDef<'static>],
    runtime_sites: &'static [RtWorkpieceSiteDef<'static>],
    runtime_holders: &'static [RtWorkpieceHolderDef<'static>],
}

impl WorkpieceBridgeContext {
    fn new(constraints: &ConstraintSet, sm: &StateMachine) -> Result<Self, BridgeError> {
        let carrier_layouts = collect_workpiece_carrier_layouts(&constraints.workpiece_carriers);
        let has_phase1_effects = sm
            .transitions
            .iter()
            .flat_map(|transition| transition.effects.iter())
            .any(workpiece_effect_requires_phase1_type);
        let phase1_workpiece_type = if has_phase1_effects {
            match constraints.workpiece_types.as_slice() {
                [workpiece] => {
                    Some(Box::leak(workpiece.name.clone().into_boxed_str()) as &'static str)
                }
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
            merge_input_types: collect_workpiece_merge_input_types(&constraints.workpiece_types),
            phase1_workpiece_type,
            runtime_types: leak_workpiece_types(&constraints.workpiece_types, &carrier_layouts)?,
            runtime_sites: leak_workpiece_sites(
                &constraints.workpiece_sites,
                &constraints.workpiece_carriers,
            ),
            runtime_holders: leak_workpiece_holders(&constraints.workpiece_holders),
        })
    }
}

fn collect_workpiece_merge_input_types(
    workpiece_types: &[crate::ir::WorkpieceTypeDef],
) -> HashMap<(String, usize), &'static [&'static str]> {
    let mut merge_input_types = HashMap::new();
    for workpiece in workpiece_types {
        for rule in &workpiece.derived_from {
            let crate::ir::WorkpieceDerivationDef::Merge { inputs } = rule else {
                continue;
            };
            merge_input_types.insert(
                (workpiece.name.clone(), inputs.len()),
                leak_str_slice(inputs),
            );
        }
    }
    merge_input_types
}

fn leak_workpiece_types(
    workpiece_types: &[crate::ir::WorkpieceTypeDef],
    carrier_layouts: &HashMap<String, BridgeCarrierLayout>,
) -> Result<&'static [RtWorkpieceTypeDef<'static>], BridgeError> {
    let leaked_types = workpiece_types
        .iter()
        .map(|workpiece| {
            Ok(RtWorkpieceTypeDef {
                name: Box::leak(workpiece.name.clone().into_boxed_str()),
                normal_terminal_states: leak_str_slice(&workpiece.normal_terminal_states),
                abnormal_terminal_states: leak_str_slice(&workpiece.abnormal_terminal_states),
                ingress_sites: leak_expanded_workpiece_endpoint_slice(
                    &workpiece.ingress_sites,
                    carrier_layouts,
                )?,
                normal_egress_sites: leak_expanded_workpiece_endpoint_slice(
                    &workpiece.normal_egress_sites,
                    carrier_layouts,
                )?,
                abnormal_egress_sites: leak_expanded_workpiece_endpoint_slice(
                    &workpiece.abnormal_egress_sites,
                    carrier_layouts,
                )?,
            })
        })
        .collect::<Result<Vec<_>, BridgeError>>()?;
    Ok(Box::leak(leaked_types.into_boxed_slice()))
}

fn leak_workpiece_sites(
    workpiece_sites: &[crate::ir::WorkpieceSiteDef],
    workpiece_carriers: &[crate::ir::WorkpieceCarrierDef],
) -> &'static [RtWorkpieceSiteDef<'static>] {
    let mut leaked_sites = workpiece_sites
        .iter()
        .map(|site| RtWorkpieceSiteDef {
            name: Box::leak(site.name.clone().into_boxed_str()),
            kind: lower_workpiece_site_kind(site.kind.clone()),
            capacity: site.capacity,
        })
        .collect::<Vec<_>>();
    for carrier in workpiece_carriers {
        for endpoint in expand_all_carrier_slot_endpoints(&carrier.name, &carrier.layout) {
            leaked_sites.push(RtWorkpieceSiteDef {
                name: Box::leak(endpoint.into_boxed_str()),
                kind: RtWorkpieceSiteKind::CarrierLocation,
                capacity: 1,
            });
        }
    }
    Box::leak(leaked_sites.into_boxed_slice())
}

fn leak_expanded_workpiece_endpoint_slice(
    endpoints: &[String],
    carrier_layouts: &HashMap<String, BridgeCarrierLayout>,
) -> Result<&'static [&'static str], BridgeError> {
    let expanded = endpoints
        .iter()
        .map(|endpoint| expand_runtime_endpoint_pattern(endpoint, carrier_layouts))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .map(|endpoint| Box::leak(endpoint.into_boxed_str()) as &'static str)
        .collect::<Vec<_>>();
    Ok(Box::leak(expanded.into_boxed_slice()))
}

fn collect_workpiece_carrier_layouts(
    workpiece_carriers: &[crate::ir::WorkpieceCarrierDef],
) -> HashMap<String, BridgeCarrierLayout> {
    workpiece_carriers
        .iter()
        .map(|carrier| {
            (
                carrier.name.clone(),
                match &carrier.layout {
                    crate::ir::WorkpieceCarrierLayoutDef::Slots { count } => {
                        BridgeCarrierLayout::Slots { count: *count }
                    }
                    crate::ir::WorkpieceCarrierLayoutDef::Grid { rows, cols } => {
                        BridgeCarrierLayout::Grid {
                            rows: *rows,
                            cols: *cols,
                        }
                    }
                },
            )
        })
        .collect()
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

fn validate_runtime_effect_endpoint(
    endpoint: &str,
    carrier_layouts: &HashMap<String, BridgeCarrierLayout>,
) -> Result<&'static str, BridgeError> {
    let expanded = expand_runtime_endpoint_pattern(endpoint, carrier_layouts)?;
    match expanded.as_slice() {
        [single] => Ok(Box::leak(single.clone().into_boxed_str())),
        _ => Err(BridgeError::InvalidWorkpieceSlotReference {
            slot: endpoint.to_string(),
            details: "runtime effects must use a concrete slot index, not a wildcard".to_string(),
        }),
    }
}

fn validate_runtime_carrier_name(
    carrier: &str,
    carrier_layouts: &HashMap<String, BridgeCarrierLayout>,
) -> Result<&'static str, BridgeError> {
    if carrier_layouts.contains_key(carrier) {
        Ok(Box::leak(carrier.to_string().into_boxed_str()))
    } else {
        Err(BridgeError::UnknownWorkpieceCarrier {
            carrier: carrier.to_string(),
        })
    }
}

fn expand_all_carrier_slot_endpoints(
    carrier: &str,
    layout: &crate::ir::WorkpieceCarrierLayoutDef,
) -> Vec<String> {
    match layout {
        crate::ir::WorkpieceCarrierLayoutDef::Slots { count } => (0..*count)
            .map(|slot| format!("{carrier}.slot[{slot}]"))
            .collect(),
        crate::ir::WorkpieceCarrierLayoutDef::Grid { rows, cols } => {
            let mut out = Vec::with_capacity((*rows as usize).saturating_mul(*cols as usize));
            for row in 0..*rows {
                for col in 0..*cols {
                    out.push(format!("{carrier}.slot[{row},{col}]"));
                }
            }
            out
        }
    }
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

fn leak_workpiece_holders(
    workpiece_holders: &[crate::ir::WorkpieceHolderDef],
) -> &'static [RtWorkpieceHolderDef<'static>] {
    let leaked_holders = workpiece_holders
        .iter()
        .map(|holder| RtWorkpieceHolderDef {
            name: Box::leak(holder.name.clone().into_boxed_str()),
            capacity: holder.capacity,
        })
        .collect::<Vec<_>>();
    Box::leak(leaked_holders.into_boxed_slice())
}

fn lower_workpiece_site_kind(kind: crate::ir::WorkpieceSiteKind) -> RtWorkpieceSiteKind {
    match kind {
        crate::ir::WorkpieceSiteKind::WorkpieceLocation => RtWorkpieceSiteKind::WorkpieceLocation,
        crate::ir::WorkpieceSiteKind::CarrierLocation => RtWorkpieceSiteKind::CarrierLocation,
    }
}

fn lower_runtime_claim_source(
    resolver: &TopologyResolver,
    claim_text: &str,
    source: &crate::ir::ResourceClaimSource,
) -> Result<RtResourceClaimSource<'static>, BridgeError> {
    match source {
        crate::ir::ResourceClaimSource::ActionTag { tag } => Ok(RtResourceClaimSource::ActionTag {
            tag: Box::leak(tag.clone().into_boxed_str()),
        }),
        crate::ir::ResourceClaimSource::State(state_expr) => {
            lower_runtime_state_claim_source(resolver, claim_text, state_expr)
        }
    }
}

fn lower_runtime_state_claim_source(
    resolver: &TopologyResolver,
    claim_text: &str,
    state_expr: &crate::ir::StateExpr,
) -> Result<RtResourceClaimSource<'static>, BridgeError> {
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

fn leak_str_slice(values: &[String]) -> &'static [&'static str] {
    let leaked_values = values
        .iter()
        .map(|value| Box::leak(value.clone().into_boxed_str()) as &'static str)
        .collect::<Vec<_>>();
    Box::leak(leaked_values.into_boxed_slice())
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

    let mut memo = HashMap::<((String, String), usize), u64>::new();
    let mut worst_case_us = 0u64;
    for state in &sm.states {
        let state_key = (state.task_name.clone(), state.step_name.clone());
        let state_cost = worst_case_extern_cost_from_state(
            &state_key,
            MAX_TRANSITIONS_PER_TASK_PER_TICK,
            &outgoing,
            &extern_bound_us,
            &mut memo,
        );
        worst_case_us = worst_case_us.max(state_cost);
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

fn build_cam_configs(
    resolver: &TopologyResolver,
    topology: &TopologyGraph,
    table_indices: &HashMap<String, u16>,
) -> Result<Vec<CamCouplingConfig>, BridgeError> {
    let mut out = Vec::with_capacity(topology.cam_couplings.len());

    for cam in &topology.cam_couplings {
        let ctx = format!("cam:{}", cam.name);
        let Some(table_index) = table_indices.get(&cam.table).copied() else {
            return Err(BridgeError::UnknownCamTableReference {
                cam: cam.name.clone(),
                table: cam.table.clone(),
            });
        };
        let interpolation = match cam.interpolation {
            IrCamInterpolation::Linear => RtCamInterpolation::Linear,
            IrCamInterpolation::CubicSpline => RtCamInterpolation::CubicSpline,
        };

        out.push(CamCouplingConfig {
            master_input: resolver.resolve_analog_input_id(&ctx, &cam.master)?,
            slave_output: resolver.resolve_analog_output_id(&ctx, &cam.slave, "self")?,
            table_index,
            interpolation,
            gear_ratio: cam.gear_ratio,
            initial_phase_offset: cam.phase_offset,
            following_error_limit: cam.following_error_limit,
            slave_feedback: resolver.resolve_analog_input_id(&ctx, &cam.slave_feedback)?,
        });
    }

    Ok(out)
}

fn convert_state_outgoing(
    resolver: &TopologyResolver,
    state_name: &str,
    outs: &[&Transition],
    workpiece_ctx: &WorkpieceBridgeContext,
    state_to_step: &HashMap<(String, String), StepId>,
    task_entry_steps: &HashMap<String, StepId>,
    steps: &mut Vec<Step<'static>>,
    sm: &StateMachine,
    tick_ms: u64,
    variable_indices: &HashMap<String, u16>,
    cam_indices: &HashMap<String, u16>,
    cam_table_indices: &HashMap<String, u16>,
    extern_signatures: &HashMap<String, (usize, usize)>,
) -> Result<Instr<'static>, BridgeError> {
    match outs.len() {
        0 => Ok(Instr::Halt),
        1 => convert_single_transition(
            resolver,
            state_name,
            outs[0],
            workpiece_ctx,
            state_to_step,
            task_entry_steps,
            steps,
            sm,
            tick_ms,
            variable_indices,
            cam_indices,
            cam_table_indices,
            extern_signatures,
        ),
        2 => convert_two_transitions(
            resolver,
            state_name,
            outs,
            workpiece_ctx,
            state_to_step,
            task_entry_steps,
            steps,
            sm,
            tick_ms,
            variable_indices,
            cam_indices,
            cam_table_indices,
            extern_signatures,
        ),
        n => Err(BridgeError::UnsupportedTransitionShape {
            state: state_name.to_string(),
            details: format!("expected 0..=2 outgoing transitions, got {n}"),
        }),
    }
}

fn convert_single_transition(
    resolver: &TopologyResolver,
    state_name: &str,
    t: &Transition,
    workpiece_ctx: &WorkpieceBridgeContext,
    state_to_step: &HashMap<(String, String), StepId>,
    task_entry_steps: &HashMap<String, StepId>,
    steps: &mut Vec<Step<'static>>,
    sm: &StateMachine,
    tick_ms: u64,
    variable_indices: &HashMap<String, u16>,
    cam_indices: &HashMap<String, u16>,
    cam_table_indices: &HashMap<String, u16>,
    extern_signatures: &HashMap<String, (usize, usize)>,
) -> Result<Instr<'static>, BridgeError> {
    match &t.guard {
        TransitionGuard::Always => {
            let target = lookup_target_step(state_name, &t.to, state_to_step)?;
            if t.actions.is_empty() && t.effects.is_empty() {
                Ok(Instr::Goto { target })
            } else {
                let actions = leak_actions(
                    resolver,
                    state_name,
                    &t.actions,
                    &t.effects,
                    workpiece_ctx,
                    state_to_step,
                    task_entry_steps,
                    tick_ms,
                    variable_indices,
                    cam_indices,
                    cam_table_indices,
                    extern_signatures,
                    None,
                )?;
                Ok(Instr::Action {
                    actions,
                    next: target,
                })
            }
        }
        TransitionGuard::Delay { duration_ms } => {
            let ticks = ms_to_ticks(state_name, *duration_ms, tick_ms)?;
            let target = lookup_target_step(state_name, &t.to, state_to_step)?;

            if t.actions.is_empty() && t.effects.is_empty() {
                Ok(Instr::Delay {
                    ticks,
                    next: target,
                })
            } else {
                let action_step = push_action_step(
                    steps,
                    &format!("{state_name}__delay_actions"),
                    resolver,
                    state_name,
                    &t.actions,
                    &t.effects,
                    workpiece_ctx,
                    target,
                    state_to_step,
                    task_entry_steps,
                    tick_ms,
                    variable_indices,
                    cam_indices,
                    cam_table_indices,
                    extern_signatures,
                    None,
                )?;
                Ok(Instr::Delay {
                    ticks,
                    next: action_step,
                })
            }
        }
        TransitionGuard::Condition { expression } => {
            let expr = expression.trim();
            let target = lookup_target_step(state_name, &t.to, state_to_step)?;
            let next = if t.actions.is_empty() && t.effects.is_empty() {
                target
            } else {
                push_action_step(
                    steps,
                    &format!("{state_name}__cond_actions"),
                    resolver,
                    state_name,
                    &t.actions,
                    &t.effects,
                    workpiece_ctx,
                    target,
                    state_to_step,
                    task_entry_steps,
                    tick_ms,
                    variable_indices,
                    cam_indices,
                    cam_table_indices,
                    extern_signatures,
                    None,
                )?
            };
            if let Some((device, ranges)) = parse_analog_region_guard(expr) {
                let id = resolver.resolve_analog_input_id(state_name, &device)?;
                let analog_ranges = ranges_to_analog_ranges(sm, state_name, &device, &ranges)?;

                Ok(Instr::WaitAnalog {
                    id,
                    ranges: analog_ranges,
                    next,
                    timeout: None,
                })
            } else if let Some(cam_guard) = parse_cam_wait_guard(expr, cam_indices) {
                Ok(cam_guard.into_instr(next, None))
            } else if let Ok((lhs, equals)) = parse_single_bool_guard(state_name, expr) {
                bool_guard_to_instr(
                    resolver,
                    state_name,
                    lhs,
                    equals,
                    variable_indices,
                    next,
                    None,
                )
            } else if let Some((left_raw, op, right_raw)) = parse_compare_guard(expr) {
                let left = compile_guard_expr_program(state_name, &left_raw, variable_indices)?;
                let right = compile_guard_expr_program(state_name, &right_raw, variable_indices)?;
                Ok(Instr::WaitExpr {
                    left,
                    op,
                    right,
                    next,
                    timeout: None,
                })
            } else {
                Err(BridgeError::UnsupportedGuardExpression {
                    state: state_name.to_string(),
                    expression: expr.to_string(),
                })
            }
        }
        TransitionGuard::Timeout { .. } => Err(BridgeError::UnsupportedTransitionShape {
            state: state_name.to_string(),
            details: "timeout-only transition is not supported (expected wait+timeout)".to_string(),
        }),
    }
}

fn convert_two_transitions(
    resolver: &TopologyResolver,
    state_name: &str,
    outs: &[&Transition],
    workpiece_ctx: &WorkpieceBridgeContext,
    state_to_step: &HashMap<(String, String), StepId>,
    task_entry_steps: &HashMap<String, StepId>,
    steps: &mut Vec<Step<'static>>,
    sm: &StateMachine,
    tick_ms: u64,
    variable_indices: &HashMap<String, u16>,
    cam_indices: &HashMap<String, u16>,
    cam_table_indices: &HashMap<String, u16>,
    extern_signatures: &HashMap<String, (usize, usize)>,
) -> Result<Instr<'static>, BridgeError> {
    let pair = (outs[0], outs[1]);

    if let Some((always, timeout)) = match pair {
        (a, b)
            if matches!(a.guard, TransitionGuard::Always)
                && matches!(b.guard, TransitionGuard::Timeout { .. }) =>
        {
            Some((a, b))
        }
        (a, b)
            if matches!(b.guard, TransitionGuard::Always)
                && matches!(a.guard, TransitionGuard::Timeout { .. }) =>
        {
            Some((b, a))
        }
        _ => None,
    } {
        let TransitionGuard::Timeout { duration_ms } = &timeout.guard else {
            unreachable!();
        };
        let next = lookup_target_step(state_name, &always.to, state_to_step)?;
        let timeout_target = lookup_target_step(state_name, &timeout.to, state_to_step)?;
        let actions = leak_actions(
            resolver,
            state_name,
            &always.actions,
            &always.effects,
            workpiece_ctx,
            state_to_step,
            task_entry_steps,
            tick_ms,
            variable_indices,
            cam_indices,
            cam_table_indices,
            extern_signatures,
            Some(Timeout {
                after_ticks: ms_to_ticks(state_name, *duration_ms, tick_ms)?,
                target: timeout_target,
            }),
        )?;
        return Ok(Instr::Action { actions, next });
    }

    let (cond, fallback_transition, after_ticks) = if let Some((cond, timeout)) = match pair {
        (a, b)
            if matches!(a.guard, TransitionGuard::Condition { .. })
                && matches!(b.guard, TransitionGuard::Timeout { .. }) =>
        {
            Some((a, b))
        }
        (a, b)
            if matches!(b.guard, TransitionGuard::Condition { .. })
                && matches!(a.guard, TransitionGuard::Timeout { .. }) =>
        {
            Some((b, a))
        }
        _ => None,
    } {
        let TransitionGuard::Timeout { duration_ms } = &timeout.guard else {
            unreachable!();
        };
        (
            cond,
            timeout,
            ms_to_ticks(state_name, *duration_ms, tick_ms)?,
        )
    } else if let Some((cond, fallback)) = match pair {
        (a, b)
            if matches!(a.guard, TransitionGuard::Condition { .. })
                && matches!(b.guard, TransitionGuard::Always) =>
        {
            Some((a, b))
        }
        (a, b)
            if matches!(b.guard, TransitionGuard::Condition { .. })
                && matches!(a.guard, TransitionGuard::Always) =>
        {
            Some((b, a))
        }
        _ => None,
    } {
        (cond, fallback, 0)
    } else {
        return Err(BridgeError::UnsupportedTransitionShape {
            state: state_name.to_string(),
            details: "expected condition+timeout, condition+always, or always+timeout".to_string(),
        });
    };

    let TransitionGuard::Condition { expression } = &cond.guard else {
        unreachable!();
    };

    let cond_target = lookup_target_step(state_name, &cond.to, state_to_step)?;
    let cond_next = if cond.actions.is_empty() && cond.effects.is_empty() {
        cond_target
    } else {
        push_action_step(
            steps,
            &format!("{state_name}__cond_actions"),
            resolver,
            state_name,
            &cond.actions,
            &cond.effects,
            workpiece_ctx,
            cond_target,
            state_to_step,
            task_entry_steps,
            tick_ms,
            variable_indices,
            cam_indices,
            cam_table_indices,
            extern_signatures,
            None,
        )?
    };

    let fallback_target = lookup_target_step(state_name, &fallback_transition.to, state_to_step)?;
    let fallback_next =
        if fallback_transition.actions.is_empty() && fallback_transition.effects.is_empty() {
            fallback_target
        } else {
            push_action_step(
                steps,
                &format!("{state_name}__fallback_actions"),
                resolver,
                state_name,
                &fallback_transition.actions,
                &fallback_transition.effects,
                workpiece_ctx,
                fallback_target,
                state_to_step,
                task_entry_steps,
                tick_ms,
                variable_indices,
                cam_indices,
                cam_table_indices,
                extern_signatures,
                None,
            )?
        };

    condition_to_wait_instr(
        resolver,
        state_name,
        &expression,
        sm,
        variable_indices,
        cam_indices,
        cond_next,
        Some(Timeout {
            after_ticks,
            target: fallback_next,
        }),
    )
}

fn condition_to_wait_instr(
    resolver: &TopologyResolver,
    state_name: &str,
    expression: &str,
    sm: &StateMachine,
    variable_indices: &HashMap<String, u16>,
    cam_indices: &HashMap<String, u16>,
    cond_next: StepId,
    timeout: Option<Timeout>,
) -> Result<Instr<'static>, BridgeError> {
    let expr = expression.trim();
    let analog_wait = parse_analog_region_guard(expr);

    if let Some((device, ranges)) = analog_wait {
        let id = resolver.resolve_analog_input_id(state_name, &device)?;
        let analog_ranges = ranges_to_analog_ranges(sm, state_name, &device, &ranges)?;
        Ok(Instr::WaitAnalog {
            id,
            ranges: analog_ranges,
            next: cond_next,
            timeout,
        })
    } else if let Some(cam_guard) = parse_cam_wait_guard(expr, cam_indices) {
        Ok(cam_guard.into_instr(cond_next, timeout))
    } else if let Ok((lhs, equals)) = parse_single_bool_guard(state_name, expr) {
        bool_guard_to_instr(
            resolver,
            state_name,
            lhs,
            equals,
            variable_indices,
            cond_next,
            timeout,
        )
    } else if let Some((left_raw, op, right_raw)) = parse_compare_guard(expr) {
        let left = compile_guard_expr_program(state_name, &left_raw, variable_indices)?;
        let right = compile_guard_expr_program(state_name, &right_raw, variable_indices)?;
        Ok(Instr::WaitExpr {
            left,
            op,
            right,
            next: cond_next,
            timeout,
        })
    } else {
        Err(BridgeError::UnsupportedGuardExpression {
            state: state_name.to_string(),
            expression: expr.to_string(),
        })
    }
}

fn parse_analog_region_guard(expr: &str) -> Option<(String, Vec<usize>)> {
    // Expected: "<device> in {region_1, region_2}"
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
    for tok in rhs.split(',') {
        let t = tok.trim();
        let idx_str = t.strip_prefix("region_")?;
        let idx: usize = idx_str.parse().ok()?;
        out.push(idx);
    }
    if out.is_empty() {
        None
    } else {
        Some((device.to_string(), out))
    }
}

fn parse_compare_guard(expr: &str) -> Option<(String, CompareOp, String)> {
    if expr.contains(" AND ") || expr.contains(" OR ") || expr.contains("NOT(") {
        return None;
    }
    if is_single_bool_guard_shape(expr) {
        return None;
    }

    for (raw_op, op) in [
        ("==", CompareOp::Eq),
        ("!=", CompareOp::Ne),
        (">=", CompareOp::Ge),
        ("<=", CompareOp::Le),
        (">", CompareOp::Gt),
        ("<", CompareOp::Lt),
    ] {
        if let Some((left, right)) = expr.split_once(raw_op) {
            let left = left.trim();
            let right = right.trim();
            if left.is_empty() || right.is_empty() {
                return None;
            }
            return Some((left.to_string(), op, right.to_string()));
        }
    }

    None
}

enum CamWaitGuard {
    Digital {
        cam_index: u16,
        field: CamDigitalField,
        equals: bool,
    },
    Analog {
        cam_index: u16,
        field: CamAnalogField,
        op: CompareOp,
        value: f32,
    },
}

impl CamWaitGuard {
    fn into_instr(self, next: StepId, timeout: Option<Timeout>) -> Instr<'static> {
        match self {
            CamWaitGuard::Digital {
                cam_index,
                field,
                equals,
            } => Instr::WaitCamDigital {
                cam_index,
                field,
                equals,
                next,
                timeout,
            },
            CamWaitGuard::Analog {
                cam_index,
                field,
                op,
                value,
            } => Instr::WaitCamAnalog {
                cam_index,
                field,
                op,
                value,
                next,
                timeout,
            },
        }
    }
}

enum BoolGuardOperand {
    Identifier(String),
    PlcPort(String),
    StateRef(StateGuardRef),
}

struct StateGuardRef {
    device: String,
    port: String,
    state: String,
}

fn parse_cam_wait_guard(expr: &str, cam_indices: &HashMap<String, u16>) -> Option<CamWaitGuard> {
    let (left_raw, op, right_raw) = parse_compare_guard(expr)?;
    let mut parts = left_raw.split('.');
    let cam_name = parts.next()?.trim();
    let field_name = parts.next()?.trim();
    if parts.next().is_some() {
        return None;
    }
    let cam_index = cam_indices.get(cam_name).copied()?;

    let parse_bool = |raw: &str| match raw.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    };

    match field_name {
        "engage" => {
            let rhs = parse_bool(&right_raw)?;
            let equals = match op {
                CompareOp::Eq => rhs,
                CompareOp::Ne => !rhs,
                _ => return None,
            };
            Some(CamWaitGuard::Digital {
                cam_index,
                field: CamDigitalField::Engage,
                equals,
            })
        }
        "in_sync" => {
            let rhs = parse_bool(&right_raw)?;
            let equals = match op {
                CompareOp::Eq => rhs,
                CompareOp::Ne => !rhs,
                _ => return None,
            };
            Some(CamWaitGuard::Digital {
                cam_index,
                field: CamDigitalField::InSync,
                equals,
            })
        }
        "fault" => {
            let rhs = parse_bool(&right_raw)?;
            let equals = match op {
                CompareOp::Eq => rhs,
                CompareOp::Ne => !rhs,
                _ => return None,
            };
            Some(CamWaitGuard::Digital {
                cam_index,
                field: CamDigitalField::Fault,
                equals,
            })
        }
        "following_error" => right_raw
            .parse::<f32>()
            .ok()
            .map(|value| CamWaitGuard::Analog {
                cam_index,
                field: CamAnalogField::FollowingError,
                op,
                value,
            }),
        "master_pos" => right_raw
            .parse::<f32>()
            .ok()
            .map(|value| CamWaitGuard::Analog {
                cam_index,
                field: CamAnalogField::MasterPos,
                op,
                value,
            }),
        "slave_cmd" => right_raw
            .parse::<f32>()
            .ok()
            .map(|value| CamWaitGuard::Analog {
                cam_index,
                field: CamAnalogField::SlaveCmd,
                op,
                value,
            }),
        _ => None,
    }
}

fn is_single_bool_guard_shape(expr: &str) -> bool {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 3 {
        return false;
    }
    let lhs = parts[0].trim();
    let op = parts[1].trim();
    let rhs = parts[2].trim();
    parse_bool_guard_operand(lhs).is_some()
        && (op == "==" || op == "!=")
        && (rhs == "true" || rhs == "false")
}

fn parse_bool_guard_operand(raw: &str) -> Option<BoolGuardOperand> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if !raw.contains('.') {
        return Some(BoolGuardOperand::Identifier(raw.to_string()));
    }

    let parts = raw.split('.').map(str::trim).collect::<Vec<_>>();
    if let [_, port] = parts.as_slice()
        && parse_physical_plc_port_ref(port).is_some()
    {
        return Some(BoolGuardOperand::PlcPort(raw.to_string()));
    }
    match parts.as_slice() {
        [device, state] if !device.is_empty() && !state.is_empty() => {
            Some(BoolGuardOperand::StateRef(StateGuardRef {
                device: (*device).to_string(),
                port: "self".to_string(),
                state: (*state).to_string(),
            }))
        }
        [device, port, state] if !device.is_empty() && !port.is_empty() && !state.is_empty() => {
            Some(BoolGuardOperand::StateRef(StateGuardRef {
                device: (*device).to_string(),
                port: (*port).to_string(),
                state: (*state).to_string(),
            }))
        }
        _ => None,
    }
}

fn bool_guard_to_instr(
    resolver: &TopologyResolver,
    state_name: &str,
    lhs: BoolGuardOperand,
    equals: bool,
    variable_indices: &HashMap<String, u16>,
    next: StepId,
    timeout: Option<Timeout>,
) -> Result<Instr<'static>, BridgeError> {
    match lhs {
        BoolGuardOperand::Identifier(name) => {
            if variable_indices.contains_key(&name) {
                let left = compile_guard_expr_program(state_name, &name, variable_indices)?;
                let right = compile_guard_expr_program(
                    state_name,
                    if equals { "1.0" } else { "0.0" },
                    variable_indices,
                )?;
                Ok(Instr::WaitExpr {
                    left,
                    op: CompareOp::Eq,
                    right,
                    next,
                    timeout,
                })
            } else {
                if resolver.sensor_is_cylinder_end_feedback(&name) {
                    return Err(BridgeError::UnsupportedGuardExpression {
                        state: state_name.to_string(),
                        expression: format!("{name} == {equals}"),
                    });
                }
                let id = resolver.resolve_digital_input_id(state_name, &name)?;
                if resolver.digital_input_is_cylinder_end_feedback(id) {
                    return Err(BridgeError::UnsupportedGuardExpression {
                        state: state_name.to_string(),
                        expression: format!("{name} == {equals}"),
                    });
                }
                Ok(Instr::WaitDigital {
                    id,
                    equals,
                    next,
                    timeout,
                })
            }
        }
        BoolGuardOperand::PlcPort(raw) => {
            let port = raw
                .split('.')
                .next_back()
                .and_then(parse_physical_plc_port_ref)
                .ok_or_else(|| BridgeError::UnsupportedGuardExpression {
                    state: state_name.to_string(),
                    expression: raw.clone(),
                })?;
            if !matches!(port.kind, PlcPortKind::DigitalInput) {
                return Err(BridgeError::UnsupportedGuardExpression {
                    state: state_name.to_string(),
                    expression: raw,
                });
            }
            if resolver.digital_input_is_cylinder_end_feedback(DigitalInputId(port.id)) {
                return Err(BridgeError::UnsupportedGuardExpression {
                    state: state_name.to_string(),
                    expression: raw,
                });
            }
            Ok(Instr::WaitDigital {
                id: DigitalInputId(port.id),
                equals,
                next,
                timeout,
            })
        }
        BoolGuardOperand::StateRef(state_ref) => {
            if !equals {
                return Err(BridgeError::UnsupportedGuardExpression {
                    state: state_name.to_string(),
                    expression: format!("{}.{} == false", state_ref.device, state_ref.state),
                });
            }
            resolver.resolve_state_guard_instr(state_name, &state_ref, next, timeout)
        }
    }
}

fn ranges_to_analog_ranges(
    sm: &StateMachine,
    state_name: &str,
    device: &str,
    region_indices: &[usize],
) -> Result<&'static [AnalogRange], BridgeError> {
    let regions =
        sm.analog_regions
            .get(device)
            .ok_or_else(|| BridgeError::MissingAnalogRegions {
                state: state_name.to_string(),
                device: device.to_string(),
            })?;

    let mut out = Vec::new();
    for &idx in region_indices {
        let (min_s, max_s) =
            regions
                .get(idx)
                .ok_or_else(|| BridgeError::UnsupportedAnalogWait {
                    state: state_name.to_string(),
                    expression: format!("{device} region_{idx}"),
                })?;
        let min = min_s
            .parse::<f32>()
            .map_err(|_| BridgeError::UnsupportedAnalogWait {
                state: state_name.to_string(),
                expression: format!("{device} region_{idx}"),
            })?;
        let max = max_s
            .parse::<f32>()
            .map_err(|_| BridgeError::UnsupportedAnalogWait {
                state: state_name.to_string(),
                expression: format!("{device} region_{idx}"),
            })?;
        out.push(AnalogRange { min, max });
    }

    Ok(Box::leak(out.into_boxed_slice()))
}

fn lookup_target_step(
    state_name: &str,
    target: &State,
    state_to_step: &HashMap<(String, String), StepId>,
) -> Result<StepId, BridgeError> {
    state_to_step
        .get(&(target.task_name.clone(), target.step_name.clone()))
        .copied()
        .ok_or_else(|| BridgeError::UnknownTransitionTarget {
            state: state_name.to_string(),
            target: format!("{}.{}", target.task_name, target.step_name),
        })
}

fn ms_to_ticks(state_name: &str, duration_ms: u64, tick_ms: u64) -> Result<u64, BridgeError> {
    if duration_ms % tick_ms != 0 {
        return Err(BridgeError::DurationNotAligned {
            state: state_name.to_string(),
            duration_ms,
            tick_ms,
        });
    }
    Ok(duration_ms / tick_ms)
}

fn lookup_branch_target(
    state_name: &str,
    target_task: &str,
    target_step: &Option<String>,
    state_to_step: &HashMap<(String, String), StepId>,
    task_entry_steps: &HashMap<String, StepId>,
    branch_label: &str,
) -> Result<StepId, BridgeError> {
    if let Some(step) = target_step {
        return state_to_step
            .get(&(target_task.to_string(), step.clone()))
            .copied()
            .ok_or_else(|| BridgeError::UnknownTransitionTarget {
                state: state_name.to_string(),
                target: format!("{branch_label}: {target_task}.{step}"),
            });
    }

    task_entry_steps
        .get(target_task)
        .copied()
        .ok_or_else(|| BridgeError::UnknownTransitionTarget {
            state: state_name.to_string(),
            target: format!("{branch_label}: {target_task}"),
        })
}

fn lower_axis_fault_route_kind(kind: IrAxisFaultRouteKind) -> RtAxisFaultRouteKind {
    match kind {
        IrAxisFaultRouteKind::Reject => RtAxisFaultRouteKind::Reject,
        IrAxisFaultRouteKind::Motion => RtAxisFaultRouteKind::Motion,
        IrAxisFaultRouteKind::Safety => RtAxisFaultRouteKind::Safety,
        IrAxisFaultRouteKind::Vendor => RtAxisFaultRouteKind::Vendor,
    }
}

fn leak_axis_fault_route_rules(
    state_name: &str,
    branch_label: &str,
    routes: &[crate::ir::AxisFaultRouteBranch],
    state_to_step: &HashMap<(String, String), StepId>,
    task_entry_steps: &HashMap<String, StepId>,
) -> Result<&'static [AxisFaultRouteRule], BridgeError> {
    let mut out = Vec::with_capacity(routes.len());
    for route in routes {
        out.push(AxisFaultRouteRule {
            kind: route.kind.map(lower_axis_fault_route_kind),
            code: route.code,
            target: lookup_branch_target(
                state_name,
                &route.target_task,
                &route.target_step,
                state_to_step,
                task_entry_steps,
                branch_label,
            )?,
        });
    }
    Ok(Box::leak(out.into_boxed_slice()))
}

fn parse_single_bool_guard(
    state_name: &str,
    expression: &str,
) -> Result<(BoolGuardOperand, bool), BridgeError> {
    let expr = expression.trim();

    if expr.contains(" AND ") || expr.contains(" OR ") || expr.contains("NOT(") {
        return Err(BridgeError::UnsupportedGuardExpression {
            state: state_name.to_string(),
            expression: expr.to_string(),
        });
    }

    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 3 {
        return Err(BridgeError::UnsupportedGuardExpression {
            state: state_name.to_string(),
            expression: expr.to_string(),
        });
    }

    let lhs = parts[0].trim();
    let op = parts[1].trim();
    let rhs = parts[2].trim();

    let Some(lhs) = parse_bool_guard_operand(lhs) else {
        return Err(BridgeError::UnsupportedGuardExpression {
            state: state_name.to_string(),
            expression: expr.to_string(),
        });
    };

    let rhs_bool = match rhs {
        "true" => true,
        "false" => false,
        _ => {
            return Err(BridgeError::UnsupportedGuardExpression {
                state: state_name.to_string(),
                expression: expr.to_string(),
            });
        }
    };

    let equals = match op {
        "==" => rhs_bool,
        "!=" => !rhs_bool,
        _ => {
            return Err(BridgeError::UnsupportedGuardExpression {
                state: state_name.to_string(),
                expression: expr.to_string(),
            });
        }
    };

    Ok((lhs, equals))
}

fn compile_guard_expr_program(
    state_name: &str,
    raw: &str,
    variable_indices: &HashMap<String, u16>,
) -> Result<ExprProgram, BridgeError> {
    compile_expr_program(state_name, raw, variable_indices).map_err(|_| {
        BridgeError::UnsupportedGuardExpression {
            state: state_name.to_string(),
            expression: raw.to_string(),
        }
    })
}

fn push_action_step(
    steps: &mut Vec<Step<'static>>,
    name: &str,
    resolver: &TopologyResolver,
    state_name: &str,
    actions: &[TransitionAction],
    effects: &[crate::ir::WorkpieceEffect],
    workpiece_ctx: &WorkpieceBridgeContext,
    next: StepId,
    state_to_step: &HashMap<(String, String), StepId>,
    task_entry_steps: &HashMap<String, StepId>,
    tick_ms: u64,
    variable_indices: &HashMap<String, u16>,
    cam_indices: &HashMap<String, u16>,
    cam_table_indices: &HashMap<String, u16>,
    extern_signatures: &HashMap<String, (usize, usize)>,
    action_timeout: Option<Timeout>,
) -> Result<StepId, BridgeError> {
    let leaked_name: &'static str = Box::leak(name.to_string().into_boxed_str());
    let leaked_actions = leak_actions(
        resolver,
        state_name,
        actions,
        effects,
        workpiece_ctx,
        state_to_step,
        task_entry_steps,
        tick_ms,
        variable_indices,
        cam_indices,
        cam_table_indices,
        extern_signatures,
        action_timeout,
    )?;
    let id = StepId(steps.len() as u16);
    steps.push(Step {
        name: leaked_name,
        instr: Instr::Action {
            actions: leaked_actions,
            next,
        },
    });
    Ok(id)
}

fn leak_actions(
    resolver: &TopologyResolver,
    state_name: &str,
    actions: &[TransitionAction],
    effects: &[crate::ir::WorkpieceEffect],
    workpiece_ctx: &WorkpieceBridgeContext,
    state_to_step: &HashMap<(String, String), StepId>,
    task_entry_steps: &HashMap<String, StepId>,
    tick_ms: u64,
    variable_indices: &HashMap<String, u16>,
    cam_indices: &HashMap<String, u16>,
    cam_table_indices: &HashMap<String, u16>,
    extern_signatures: &HashMap<String, (usize, usize)>,
    action_timeout: Option<Timeout>,
) -> Result<&'static [Action], BridgeError> {
    let mut out: Vec<Action> = Vec::with_capacity(actions.len() + effects.len());
    for a in actions {
        out.push(convert_action(
            resolver,
            state_name,
            workpiece_ctx,
            RuntimeActionRef::Transition(a),
            state_to_step,
            task_entry_steps,
            tick_ms,
            variable_indices,
            cam_indices,
            cam_table_indices,
            extern_signatures,
            action_timeout,
        )?);
    }
    for effect in effects {
        out.push(convert_action(
            resolver,
            state_name,
            workpiece_ctx,
            RuntimeActionRef::Workpiece(effect),
            state_to_step,
            task_entry_steps,
            tick_ms,
            variable_indices,
            cam_indices,
            cam_table_indices,
            extern_signatures,
            action_timeout,
        )?);
    }
    Ok(Box::leak(out.into_boxed_slice()))
}

enum RuntimeActionRef<'a> {
    Transition(&'a TransitionAction),
    Workpiece(&'a crate::ir::WorkpieceEffect),
}

fn convert_action(
    resolver: &TopologyResolver,
    state_name: &str,
    workpiece_ctx: &WorkpieceBridgeContext,
    runtime_action: RuntimeActionRef<'_>,
    state_to_step: &HashMap<(String, String), StepId>,
    task_entry_steps: &HashMap<String, StepId>,
    tick_ms: u64,
    variable_indices: &HashMap<String, u16>,
    cam_indices: &HashMap<String, u16>,
    cam_table_indices: &HashMap<String, u16>,
    extern_signatures: &HashMap<String, (usize, usize)>,
    action_timeout: Option<Timeout>,
) -> Result<Action, BridgeError> {
    match runtime_action {
        RuntimeActionRef::Transition(a) => match a {
            TransitionAction::Extend {
                target,
                port,
                timeout: motion_timeout,
                on_motion_fault,
                on_safety_fault,
            } => {
                if let Some(motion) =
                    resolver.resolve_cylinder_motion(state_name, target, port, true)?
                {
                    let resolved_timeout = match (action_timeout, motion_timeout.as_ref()) {
                        (Some(_), Some(_)) => {
                            return Err(BridgeError::UnsupportedAction {
                                state: state_name.to_string(),
                                action: format!(
                                    "extend {target} cannot declare both step timeout and cylinder action timeout"
                                ),
                            });
                        }
                        (Some(timeout), None) => Some(timeout),
                        (None, Some(timeout_branch)) => Some(Timeout {
                            after_ticks: ms_to_ticks(
                                state_name,
                                timeout_branch.duration_ms,
                                tick_ms,
                            )?,
                            target: lookup_branch_target(
                                state_name,
                                &timeout_branch.target_task,
                                &timeout_branch.target_step,
                                state_to_step,
                                task_entry_steps,
                                "cylinder timeout",
                            )?,
                        }),
                        (None, None) => None,
                    };
                    let fault_routing = match (on_motion_fault, on_safety_fault) {
                        (None, None) => None,
                        (Some(motion_fault), Some(safety_fault)) => {
                            let on_motion_fault_target = lookup_branch_target(
                                state_name,
                                &motion_fault.target_task,
                                &motion_fault.target_step,
                                state_to_step,
                                task_entry_steps,
                                "cylinder on_motion_fault",
                            )?;
                            let on_safety_fault_target = lookup_branch_target(
                                state_name,
                                &safety_fault.target_task,
                                &safety_fault.target_step,
                                state_to_step,
                                task_entry_steps,
                                "cylinder on_safety_fault",
                            )?;
                            Some(CylinderFaultRouting {
                                on_motion_fault: on_motion_fault_target,
                                on_safety_fault: on_safety_fault_target,
                            })
                        }
                        _ => {
                            return Err(BridgeError::IncompleteClosedLoopCylinderRouting {
                                state: state_name.to_string(),
                                device: target.clone(),
                            });
                        }
                    };
                    Ok(Action::CylinderMotion {
                        target: motion.target,
                        output: motion.output,
                        expect_extended: true,
                        confirm_inputs: motion.confirm_inputs,
                        opposing_inputs: motion.opposing_inputs,
                        timeout: resolved_timeout,
                        fault_routing,
                    })
                } else {
                    if motion_timeout.is_some() || on_motion_fault.is_some() || on_safety_fault.is_some()
                    {
                        return Err(BridgeError::UnsupportedAction {
                            state: state_name.to_string(),
                            action: format!(
                                "extend {target} with fault routing requires topology-closed cylinder feedback"
                            ),
                        });
                    }
                    let output = resolver.resolve_digital_output_id(state_name, target, port)?;
                    Ok(Action::Extend { output })
                }
            }
            TransitionAction::Retract {
                target,
                port,
                timeout: motion_timeout,
                on_motion_fault,
                on_safety_fault,
            } => {
                if let Some(motion) =
                    resolver.resolve_cylinder_motion(state_name, target, port, false)?
                {
                    let resolved_timeout = match (action_timeout, motion_timeout.as_ref()) {
                        (Some(_), Some(_)) => {
                            return Err(BridgeError::UnsupportedAction {
                                state: state_name.to_string(),
                                action: format!(
                                    "retract {target} cannot declare both step timeout and cylinder action timeout"
                                ),
                            });
                        }
                        (Some(timeout), None) => Some(timeout),
                        (None, Some(timeout_branch)) => Some(Timeout {
                            after_ticks: ms_to_ticks(
                                state_name,
                                timeout_branch.duration_ms,
                                tick_ms,
                            )?,
                            target: lookup_branch_target(
                                state_name,
                                &timeout_branch.target_task,
                                &timeout_branch.target_step,
                                state_to_step,
                                task_entry_steps,
                                "cylinder timeout",
                            )?,
                        }),
                        (None, None) => None,
                    };
                    let fault_routing = match (on_motion_fault, on_safety_fault) {
                        (None, None) => None,
                        (Some(motion_fault), Some(safety_fault)) => {
                            let on_motion_fault_target = lookup_branch_target(
                                state_name,
                                &motion_fault.target_task,
                                &motion_fault.target_step,
                                state_to_step,
                                task_entry_steps,
                                "cylinder on_motion_fault",
                            )?;
                            let on_safety_fault_target = lookup_branch_target(
                                state_name,
                                &safety_fault.target_task,
                                &safety_fault.target_step,
                                state_to_step,
                                task_entry_steps,
                                "cylinder on_safety_fault",
                            )?;
                            Some(CylinderFaultRouting {
                                on_motion_fault: on_motion_fault_target,
                                on_safety_fault: on_safety_fault_target,
                            })
                        }
                        _ => {
                            return Err(BridgeError::IncompleteClosedLoopCylinderRouting {
                                state: state_name.to_string(),
                                device: target.clone(),
                            });
                        }
                    };
                    Ok(Action::CylinderMotion {
                        target: motion.target,
                        output: motion.output,
                        expect_extended: false,
                        confirm_inputs: motion.confirm_inputs,
                        opposing_inputs: motion.opposing_inputs,
                        timeout: resolved_timeout,
                        fault_routing,
                    })
                } else {
                    if motion_timeout.is_some() || on_motion_fault.is_some() || on_safety_fault.is_some()
                    {
                        return Err(BridgeError::UnsupportedAction {
                            state: state_name.to_string(),
                            action: format!(
                                "retract {target} with fault routing requires topology-closed cylinder feedback"
                            ),
                        });
                    }
                    let output = resolver.resolve_digital_output_id(state_name, target, port)?;
                    Ok(Action::Retract { output })
                }
            }
            TransitionAction::Set {
                target,
                port,
                value,
            } => {
                let id = resolver.resolve_digital_output_id(state_name, target, port)?;
                let value = match value {
                    IrBinaryValue::On => true,
                    IrBinaryValue::Off => false,
                };
                Ok(Action::SetDigital { id, value })
            }
            TransitionAction::SetAnalog {
                target,
                port,
                value_raw,
            } => {
                let id = resolver.resolve_analog_output_id(state_name, target, port)?;
                let value =
                    value_raw
                        .parse::<f32>()
                        .map_err(|_| BridgeError::InvalidAnalogLiteral {
                            state: state_name.to_string(),
                            target: target.clone(),
                            value_raw: value_raw.clone(),
                        })?;
                Ok(Action::SetAnalog { id, value })
            }
            TransitionAction::SetAnalogExpr {
                target,
                port,
                expr_raw,
            } => {
                let id = resolver.resolve_analog_output_id(state_name, target, port)?;
                let expr = compile_expr_program(state_name, expr_raw, variable_indices)?;
                Ok(Action::SetAnalogExpr { id, expr })
            }
            TransitionAction::Compute { target, expr_raw } => {
                let Some(target_var) = variable_indices.get(target).copied() else {
                    return Err(BridgeError::UnsupportedAction {
                        state: state_name.to_string(),
                        action: format!("compute {target}"),
                    });
                };
                let expr = compile_expr_program(state_name, expr_raw, variable_indices)?;
                Ok(Action::Compute { target_var, expr })
            }
            TransitionAction::CallExtern {
                function,
                args_raw,
                binding,
            } => {
                let rendered_binding = match binding {
                    crate::ir::ExternCallBinding::Single(name) => name.clone(),
                    crate::ir::ExternCallBinding::Tuple(names) => format!("({})", names.join(", ")),
                };
                let rendered_action = format!(
                    "call {}({}) -> {}",
                    function,
                    args_raw.join(", "),
                    rendered_binding
                );

                let Some((expected_args, expected_returns)) =
                    extern_signatures.get(function).copied()
                else {
                    return Err(BridgeError::UnsupportedAction {
                        state: state_name.to_string(),
                        action: format!("{rendered_action} (extern function not declared)"),
                    });
                };
                if args_raw.len() != expected_args {
                    return Err(BridgeError::UnsupportedAction {
                        state: state_name.to_string(),
                        action: format!(
                            "{rendered_action} (expected {expected_args} args, got {})",
                            args_raw.len()
                        ),
                    });
                }

                let arg_exprs = args_raw
                    .iter()
                    .map(|raw| compile_expr_program(state_name, raw, variable_indices))
                    .collect::<Result<Vec<_>, _>>()?;

                let binding_names = match binding {
                    crate::ir::ExternCallBinding::Single(name) => vec![name.clone()],
                    crate::ir::ExternCallBinding::Tuple(names) => names.clone(),
                };
                if binding_names.len() != expected_returns {
                    return Err(BridgeError::UnsupportedAction {
                        state: state_name.to_string(),
                        action: format!(
                            "{rendered_action} (expected {expected_returns} return bindings, got {})",
                            binding_names.len()
                        ),
                    });
                }

                let mut binding_vars = Vec::with_capacity(binding_names.len());
                for target in binding_names {
                    let Some(index) = variable_indices.get(&target).copied() else {
                        return Err(BridgeError::UnsupportedAction {
                            state: state_name.to_string(),
                            action: format!(
                                "{rendered_action} (unknown binding variable {target})"
                            ),
                        });
                    };
                    binding_vars.push(index);
                }

                let leaked_function: &'static str = Box::leak(function.clone().into_boxed_str());
                let leaked_arg_exprs: &'static [ExprProgram] =
                    Box::leak(arg_exprs.into_boxed_slice());
                let leaked_binding_vars: &'static [u16] =
                    Box::leak(binding_vars.into_boxed_slice());
                Ok(Action::CallExtern {
                    function: leaked_function,
                    arg_exprs: leaked_arg_exprs,
                    binding_vars: leaked_binding_vars,
                })
            }
            TransitionAction::CamEngage { target } => {
                let Some(cam_index) = cam_indices.get(target).copied() else {
                    return Err(BridgeError::UnsupportedAction {
                        state: state_name.to_string(),
                        action: format!("cam_engage {target}"),
                    });
                };
                Ok(Action::CamEngage { cam_index })
            }
            TransitionAction::CamDisengage { target } => {
                let Some(cam_index) = cam_indices.get(target).copied() else {
                    return Err(BridgeError::UnsupportedAction {
                        state: state_name.to_string(),
                        action: format!("cam_disengage {target}"),
                    });
                };
                Ok(Action::CamDisengage { cam_index })
            }
            TransitionAction::CamSwitch { target, new_table } => {
                let Some(cam_index) = cam_indices.get(target).copied() else {
                    return Err(BridgeError::UnsupportedAction {
                        state: state_name.to_string(),
                        action: format!("cam_switch {target} {new_table}"),
                    });
                };
                let Some(table_index) = cam_table_indices.get(new_table).copied() else {
                    return Err(BridgeError::UnsupportedAction {
                        state: state_name.to_string(),
                        action: format!("cam_switch {target} {new_table}"),
                    });
                };
                Ok(Action::CamSwitch {
                    cam_index,
                    table_index,
                })
            }
            TransitionAction::CamPhase {
                target,
                offset_expr_raw,
            } => {
                let Some(cam_index) = cam_indices.get(target).copied() else {
                    return Err(BridgeError::UnsupportedAction {
                        state: state_name.to_string(),
                        action: format!("cam_phase {target} {offset_expr_raw}"),
                    });
                };
                let offset_expr =
                    compile_expr_program(state_name, offset_expr_raw, variable_indices)?;
                Ok(Action::CamPhase {
                    cam_index,
                    offset_expr,
                })
            }
            TransitionAction::AxisMoveRelative {
                target,
                port,
                distance_raw,
                speed_raw,
                semantic_tag,
                timeout: timeout_branch,
                on_reject,
                on_motion_fault,
                on_safety_fault,
                on_reject_routes,
                on_motion_fault_routes,
                on_safety_fault_routes,
            } => {
                let profile = resolver.axis_profile(target).ok_or_else(|| {
                    BridgeError::MissingAxisProfile {
                        state: state_name.to_string(),
                        target: target.clone(),
                    }
                })?;
                let distance =
                    distance_raw
                        .parse::<f32>()
                        .map_err(|_| BridgeError::InvalidAxisLiteral {
                            state: state_name.to_string(),
                            target: target.clone(),
                            field: "distance".to_string(),
                            value_raw: distance_raw.clone(),
                        })?;
                let speed =
                    speed_raw
                        .parse::<f32>()
                        .map_err(|_| BridgeError::InvalidAxisLiteral {
                            state: state_name.to_string(),
                            target: target.clone(),
                            field: "speed".to_string(),
                            value_raw: speed_raw.clone(),
                        })?;
                if speed <= 0.0 || speed > profile.max_speed {
                    return Err(BridgeError::AxisSpeedOutOfRange {
                        state: state_name.to_string(),
                        target: target.clone(),
                        speed,
                        max_speed: profile.max_speed,
                    });
                }
                let leaked_target: &'static str = Box::leak(target.clone().into_boxed_str());
                let leaked_port: &'static str = Box::leak(port.clone().into_boxed_str());
                let timeout_target = lookup_branch_target(
                    state_name,
                    &timeout_branch.target_task,
                    &timeout_branch.target_step,
                    state_to_step,
                    task_entry_steps,
                    "axis timeout",
                )?;
                let timeout_ticks = ms_to_ticks(state_name, timeout_branch.duration_ms, tick_ms)?;
                let on_reject_target = lookup_branch_target(
                    state_name,
                    &on_reject.target_task,
                    &on_reject.target_step,
                    state_to_step,
                    task_entry_steps,
                    "axis on_reject",
                )?;
                let on_motion_fault_target = lookup_branch_target(
                    state_name,
                    &on_motion_fault.target_task,
                    &on_motion_fault.target_step,
                    state_to_step,
                    task_entry_steps,
                    "axis on_motion_fault",
                )?;
                let on_safety_fault_target = lookup_branch_target(
                    state_name,
                    &on_safety_fault.target_task,
                    &on_safety_fault.target_step,
                    state_to_step,
                    task_entry_steps,
                    "axis on_safety_fault",
                )?;
                let leaked_on_reject_routes = leak_axis_fault_route_rules(
                    state_name,
                    "axis on_reject route",
                    on_reject_routes,
                    state_to_step,
                    task_entry_steps,
                )?;
                let leaked_on_motion_fault_routes = leak_axis_fault_route_rules(
                    state_name,
                    "axis on_motion_fault route",
                    on_motion_fault_routes,
                    state_to_step,
                    task_entry_steps,
                )?;
                let leaked_on_safety_fault_routes = leak_axis_fault_route_rules(
                    state_name,
                    "axis on_safety_fault route",
                    on_safety_fault_routes,
                    state_to_step,
                    task_entry_steps,
                )?;
                Ok(Action::AxisMove {
                    command: AxisMotionCommand {
                        target: leaked_target,
                        port: leaked_port,
                        kind: AxisMoveKind::Relative,
                        value: distance,
                        speed,
                        semantic_tag: semantic_tag
                            .as_ref()
                            .map(|tag| Box::leak(tag.clone().into_boxed_str()) as &'static str),
                        require_homed: false,
                        timeout: Some(Timeout {
                            after_ticks: timeout_ticks,
                            target: timeout_target,
                        }),
                        fault_routing: Some(AxisFaultRouting {
                            on_reject: on_reject_target,
                            on_motion_fault: on_motion_fault_target,
                            on_safety_fault: on_safety_fault_target,
                            on_reject_routes: leaked_on_reject_routes,
                            on_motion_fault_routes: leaked_on_motion_fault_routes,
                            on_safety_fault_routes: leaked_on_safety_fault_routes,
                        }),
                    },
                })
            }
            TransitionAction::AxisMoveAbsolute {
                target,
                port,
                position_raw,
                speed_raw,
                require_homed,
                semantic_tag,
                timeout: timeout_branch,
                on_reject,
                on_motion_fault,
                on_safety_fault,
                on_reject_routes,
                on_motion_fault_routes,
                on_safety_fault_routes,
            } => {
                let profile = resolver.axis_profile(target).ok_or_else(|| {
                    BridgeError::MissingAxisProfile {
                        state: state_name.to_string(),
                        target: target.clone(),
                    }
                })?;
                let position =
                    position_raw
                        .parse::<f32>()
                        .map_err(|_| BridgeError::InvalidAxisLiteral {
                            state: state_name.to_string(),
                            target: target.clone(),
                            field: "position".to_string(),
                            value_raw: position_raw.clone(),
                        })?;
                let speed =
                    speed_raw
                        .parse::<f32>()
                        .map_err(|_| BridgeError::InvalidAxisLiteral {
                            state: state_name.to_string(),
                            target: target.clone(),
                            field: "speed".to_string(),
                            value_raw: speed_raw.clone(),
                        })?;
                if speed <= 0.0 || speed > profile.max_speed {
                    return Err(BridgeError::AxisSpeedOutOfRange {
                        state: state_name.to_string(),
                        target: target.clone(),
                        speed,
                        max_speed: profile.max_speed,
                    });
                }
                let leaked_target: &'static str = Box::leak(target.clone().into_boxed_str());
                let leaked_port: &'static str = Box::leak(port.clone().into_boxed_str());
                let timeout_target = lookup_branch_target(
                    state_name,
                    &timeout_branch.target_task,
                    &timeout_branch.target_step,
                    state_to_step,
                    task_entry_steps,
                    "axis timeout",
                )?;
                let timeout_ticks = ms_to_ticks(state_name, timeout_branch.duration_ms, tick_ms)?;
                let on_reject_target = lookup_branch_target(
                    state_name,
                    &on_reject.target_task,
                    &on_reject.target_step,
                    state_to_step,
                    task_entry_steps,
                    "axis on_reject",
                )?;
                let on_motion_fault_target = lookup_branch_target(
                    state_name,
                    &on_motion_fault.target_task,
                    &on_motion_fault.target_step,
                    state_to_step,
                    task_entry_steps,
                    "axis on_motion_fault",
                )?;
                let on_safety_fault_target = lookup_branch_target(
                    state_name,
                    &on_safety_fault.target_task,
                    &on_safety_fault.target_step,
                    state_to_step,
                    task_entry_steps,
                    "axis on_safety_fault",
                )?;
                let leaked_on_reject_routes = leak_axis_fault_route_rules(
                    state_name,
                    "axis on_reject route",
                    on_reject_routes,
                    state_to_step,
                    task_entry_steps,
                )?;
                let leaked_on_motion_fault_routes = leak_axis_fault_route_rules(
                    state_name,
                    "axis on_motion_fault route",
                    on_motion_fault_routes,
                    state_to_step,
                    task_entry_steps,
                )?;
                let leaked_on_safety_fault_routes = leak_axis_fault_route_rules(
                    state_name,
                    "axis on_safety_fault route",
                    on_safety_fault_routes,
                    state_to_step,
                    task_entry_steps,
                )?;
                Ok(Action::AxisMove {
                    command: AxisMotionCommand {
                        target: leaked_target,
                        port: leaked_port,
                        kind: AxisMoveKind::Absolute,
                        value: position,
                        speed,
                        semantic_tag: semantic_tag
                            .as_ref()
                            .map(|tag| Box::leak(tag.clone().into_boxed_str()) as &'static str),
                        require_homed: *require_homed,
                        timeout: Some(Timeout {
                            after_ticks: timeout_ticks,
                            target: timeout_target,
                        }),
                        fault_routing: Some(AxisFaultRouting {
                            on_reject: on_reject_target,
                            on_motion_fault: on_motion_fault_target,
                            on_safety_fault: on_safety_fault_target,
                            on_reject_routes: leaked_on_reject_routes,
                            on_motion_fault_routes: leaked_on_motion_fault_routes,
                            on_safety_fault_routes: leaked_on_safety_fault_routes,
                        }),
                    },
                })
            }
            TransitionAction::Log { message } => {
                let leaked_message: &'static str = Box::leak(message.clone().into_boxed_str());
                Ok(Action::Log {
                    message_id: stable_log_message_id(message),
                    message: leaked_message,
                })
            }
        },
        RuntimeActionRef::Workpiece(effect) => {
            convert_workpiece_effect(state_name, workpiece_ctx, effect)
        }
    }
}

fn convert_workpiece_effect(
    state_name: &str,
    workpiece_ctx: &WorkpieceBridgeContext,
    effect: &crate::ir::WorkpieceEffect,
) -> Result<Action, BridgeError> {
    match effect {
        crate::ir::WorkpieceEffect::Acquire { holder, from } => {
            let workpiece_type = workpiece_ctx
                .phase1_workpiece_type
                .ok_or(BridgeError::Phase1WorkpieceTypeArity { count: 0 })?;
            Ok(Action::WorkpieceAcquire {
                workpiece_type,
                holder: Box::leak(holder.clone().into_boxed_str()),
                from: validate_runtime_effect_endpoint(from, &workpiece_ctx.carrier_layouts)?,
            })
        }
        crate::ir::WorkpieceEffect::Transfer { from, to } => Ok(Action::WorkpieceTransfer {
            from: validate_runtime_effect_endpoint(from, &workpiece_ctx.carrier_layouts)?,
            to: validate_runtime_effect_endpoint(to, &workpiece_ctx.carrier_layouts)?,
        }),
        crate::ir::WorkpieceEffect::Finish { at, terminal_state } => Ok(Action::WorkpieceFinish {
            at: validate_runtime_effect_endpoint(at, &workpiece_ctx.carrier_layouts)?,
            terminal_state: Box::leak(terminal_state.clone().into_boxed_str()),
        }),
        crate::ir::WorkpieceEffect::Mount {
            workpiece_type,
            slot,
        } => Ok(Action::WorkpieceMount {
            workpiece_type: Box::leak(workpiece_type.clone().into_boxed_str()),
            slot: validate_runtime_effect_endpoint(slot, &workpiece_ctx.carrier_layouts)?,
        }),
        crate::ir::WorkpieceEffect::Unmount {
            workpiece_type,
            slot,
            to,
        } => Ok(Action::WorkpieceUnmount {
            workpiece_type: Box::leak(workpiece_type.clone().into_boxed_str()),
            slot: validate_runtime_effect_endpoint(slot, &workpiece_ctx.carrier_layouts)?,
            to: validate_runtime_effect_endpoint(to, &workpiece_ctx.carrier_layouts)?,
        }),
        crate::ir::WorkpieceEffect::TransformCarrier { carrier, frame } => {
            Ok(Action::WorkpieceTransformCarrier {
                carrier: validate_runtime_carrier_name(carrier, &workpiece_ctx.carrier_layouts)?,
                frame: Box::leak(frame.clone().into_boxed_str()),
            })
        }
        crate::ir::WorkpieceEffect::Split {
            source_type,
            target_type,
            count,
            consumed,
        } => Ok(Action::WorkpieceSplit {
            source_type: Box::leak(source_type.clone().into_boxed_str()),
            target_type: Box::leak(target_type.clone().into_boxed_str()),
            count: *count,
            consumed: *consumed,
        }),
        crate::ir::WorkpieceEffect::Merge {
            inputs,
            target_type,
            consumed_inputs,
        } => {
            let Some(&input_types) = workpiece_ctx
                .merge_input_types
                .get(&(target_type.clone(), inputs.len()))
            else {
                return Err(BridgeError::UnsupportedWorkpieceEffect {
                    state: state_name.to_string(),
                    effect: render_workpiece_effect(effect),
                });
            };
            Ok(Action::WorkpieceMerge {
                input_refs: leak_str_slice(inputs),
                input_types,
                target_type: Box::leak(target_type.clone().into_boxed_str()),
                consumed_inputs: *consumed_inputs,
            })
        }
    }
}

fn render_workpiece_effect(effect: &crate::ir::WorkpieceEffect) -> String {
    match effect {
        crate::ir::WorkpieceEffect::Acquire { holder, from } => {
            format!("acquire holder {holder} from {from}")
        }
        crate::ir::WorkpieceEffect::Transfer { from, to } => {
            format!("transfer from {from} to {to}")
        }
        crate::ir::WorkpieceEffect::Finish { at, terminal_state } => {
            format!("finish workpiece at {at} as {terminal_state}")
        }
        crate::ir::WorkpieceEffect::Mount {
            workpiece_type,
            slot,
        } => format!("mount {workpiece_type} on {slot}"),
        crate::ir::WorkpieceEffect::Unmount {
            workpiece_type,
            slot,
            to,
        } => format!("unmount {workpiece_type} from {slot} to {to}"),
        crate::ir::WorkpieceEffect::Split {
            source_type,
            target_type,
            count,
            consumed,
        } => {
            let suffix = if *consumed { " consumed" } else { "" };
            format!("split {source_type} into {target_type} count {count}{suffix}")
        }
        crate::ir::WorkpieceEffect::Merge {
            inputs,
            target_type,
            consumed_inputs,
        } => {
            let suffix = if *consumed_inputs {
                " consumed_inputs"
            } else {
                ""
            };
            format!("merge [{}] into {target_type}{suffix}", inputs.join(", "))
        }
        crate::ir::WorkpieceEffect::TransformCarrier { carrier, frame } => {
            format!("transform carrier {carrier} to frame {frame}")
        }
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
    Comma,
}

fn compile_expr_program(
    state_name: &str,
    raw: &str,
    variable_indices: &HashMap<String, u16>,
) -> Result<ExprProgram, BridgeError> {
    let tokens = tokenize_expr(raw).map_err(|_| BridgeError::UnsupportedAction {
        state: state_name.to_string(),
        action: format!("expr: {raw}"),
    })?;

    let mut compiler = ExprCompiler {
        state_name,
        raw,
        variable_indices,
        tokens,
        pos: 0,
        output: [ExprOp::PushLiteral(0.0); runtime_core::MAX_EXPR_OPS],
        out_len: 0,
    };
    compiler.parse_expression()?;
    if compiler.pos != compiler.tokens.len() {
        return Err(BridgeError::UnsupportedAction {
            state: state_name.to_string(),
            action: format!("unexpected trailing expression content: {raw}"),
        });
    }

    Ok(ExprProgram {
        ops: compiler.output,
        len: compiler.out_len as u8,
    })
}

fn tokenize_expr(raw: &str) -> Result<Vec<ExprToken<'_>>, ()> {
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
                let c = bytes[i] as char;
                if c.is_ascii_digit() || c == '.' {
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
                let c = bytes[i] as char;
                if c.is_ascii_alphanumeric() || c == '_' {
                    i += 1;
                } else {
                    break;
                }
            }
            let word = &raw[start..i];
            let lowered = word.to_ascii_lowercase();
            match lowered.as_str() {
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
            let two = &raw[i..i + 2];
            match two {
                "==" => {
                    out.push(ExprToken::EqEq);
                    i += 2;
                    continue;
                }
                "!=" => {
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
                "<>" => {
                    out.push(ExprToken::NotEq);
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }

        match ch {
            '(' => {
                out.push(ExprToken::LParen);
                i += 1;
            }
            ')' => {
                out.push(ExprToken::RParen);
                i += 1;
            }
            '+' => {
                out.push(ExprToken::Plus);
                i += 1;
            }
            '-' => {
                out.push(ExprToken::Minus);
                i += 1;
            }
            '*' => {
                out.push(ExprToken::Star);
                i += 1;
            }
            '/' => {
                out.push(ExprToken::Slash);
                i += 1;
            }
            '%' => {
                out.push(ExprToken::Percent);
                i += 1;
            }
            '>' => {
                out.push(ExprToken::Gt);
                i += 1;
            }
            '<' => {
                out.push(ExprToken::Lt);
                i += 1;
            }
            '=' => {
                out.push(ExprToken::EqEq);
                i += 1;
            }
            '!' => {
                out.push(ExprToken::Not);
                i += 1;
            }
            ',' => {
                out.push(ExprToken::Comma);
                i += 1;
            }
            _ => return Err(()),
        }
    }

    Ok(out)
}

struct ExprCompiler<'a> {
    state_name: &'a str,
    raw: &'a str,
    variable_indices: &'a HashMap<String, u16>,
    tokens: Vec<ExprToken<'a>>,
    pos: usize,
    output: [ExprOp; runtime_core::MAX_EXPR_OPS],
    out_len: usize,
}

impl<'a> ExprCompiler<'a> {
    fn parse_expression(&mut self) -> Result<(), BridgeError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<(), BridgeError> {
        self.parse_and()?;
        while self.consume_if(ExprToken::Or) {
            self.parse_and()?;
            self.push_op(ExprOp::BoolOr)?;
        }
        Ok(())
    }

    fn parse_and(&mut self) -> Result<(), BridgeError> {
        self.parse_comparison()?;
        while self.consume_if(ExprToken::And) {
            self.parse_comparison()?;
            self.push_op(ExprOp::BoolAnd)?;
        }
        Ok(())
    }

    fn parse_comparison(&mut self) -> Result<(), BridgeError> {
        self.parse_additive()?;
        let cmp_op = if self.consume_if(ExprToken::EqEq) {
            Some(ExprOp::CmpEq)
        } else if self.consume_if(ExprToken::NotEq) {
            Some(ExprOp::CmpNe)
        } else if self.consume_if(ExprToken::Ge) {
            Some(ExprOp::CmpGe)
        } else if self.consume_if(ExprToken::Le) {
            Some(ExprOp::CmpLe)
        } else if self.consume_if(ExprToken::Gt) {
            Some(ExprOp::CmpGt)
        } else if self.consume_if(ExprToken::Lt) {
            Some(ExprOp::CmpLt)
        } else {
            None
        };
        if let Some(op) = cmp_op {
            self.parse_additive()?;
            self.push_op(op)?;
        }
        Ok(())
    }

    fn parse_additive(&mut self) -> Result<(), BridgeError> {
        self.parse_multiplicative()?;
        loop {
            let op = if self.consume_if(ExprToken::Plus) {
                Some(ExprOp::Add)
            } else if self.consume_if(ExprToken::Minus) {
                Some(ExprOp::Sub)
            } else {
                None
            };
            let Some(op) = op else { break };
            self.parse_multiplicative()?;
            self.push_op(op)?;
        }
        Ok(())
    }

    fn parse_multiplicative(&mut self) -> Result<(), BridgeError> {
        self.parse_unary()?;
        loop {
            let op = if self.consume_if(ExprToken::Star) {
                Some(ExprOp::Mul)
            } else if self.consume_if(ExprToken::Slash) {
                Some(ExprOp::Div)
            } else if self.consume_if(ExprToken::Percent) {
                Some(ExprOp::Mod)
            } else {
                None
            };
            let Some(op) = op else { break };
            self.parse_unary()?;
            self.push_op(op)?;
        }
        Ok(())
    }

    fn parse_unary(&mut self) -> Result<(), BridgeError> {
        if self.consume_if(ExprToken::Minus) {
            self.parse_unary()?;
            self.push_op(ExprOp::Neg)?;
            return Ok(());
        }
        if self.consume_if(ExprToken::Not) {
            self.parse_unary()?;
            self.push_op(ExprOp::BoolNot)?;
            return Ok(());
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<(), BridgeError> {
        let Some(token) = self.peek().copied() else {
            return self.err("unexpected end of expression");
        };

        match token {
            ExprToken::Number(raw_number) => {
                self.pos += 1;
                let parsed =
                    raw_number
                        .parse::<f32>()
                        .map_err(|_| BridgeError::UnsupportedAction {
                            state: self.state_name.to_string(),
                            action: format!("invalid number in expr: {}", self.raw),
                        })?;
                self.push_op(ExprOp::PushLiteral(parsed))
            }
            ExprToken::Bool(value) => {
                self.pos += 1;
                self.push_op(ExprOp::PushLiteral(if value { 1.0 } else { 0.0 }))
            }
            ExprToken::Ident(name) => {
                self.pos += 1;
                if self.consume_if(ExprToken::LParen) {
                    self.parse_function_call(name)
                } else {
                    let Some(idx) = self.variable_indices.get(name).copied() else {
                        return self.err(format!("undefined variable in expr: {name}"));
                    };
                    self.push_op(ExprOp::PushVariable(idx))
                }
            }
            ExprToken::LParen => {
                self.pos += 1;
                self.parse_expression()?;
                if !self.consume_if(ExprToken::RParen) {
                    return self.err("missing ')' in expression");
                }
                Ok(())
            }
            _ => self.err("unexpected token in expression"),
        }
    }

    fn parse_function_call(&mut self, name: &str) -> Result<(), BridgeError> {
        let mut arg_count = 0usize;
        if !self.consume_if(ExprToken::RParen) {
            loop {
                self.parse_expression()?;
                arg_count += 1;
                if self.consume_if(ExprToken::Comma) {
                    continue;
                }
                if self.consume_if(ExprToken::RParen) {
                    break;
                }
                return self.err("function call missing ')'");
            }
        }

        let op = match (name, arg_count) {
            ("abs", 1) => ExprOp::CallAbs,
            ("min", 2) => ExprOp::CallMin,
            ("max", 2) => ExprOp::CallMax,
            ("sin", 1) => ExprOp::CallSin,
            ("cos", 1) => ExprOp::CallCos,
            ("sqrt", 1) => ExprOp::CallSqrt,
            ("pow", 2) => ExprOp::CallPow,
            ("fmod", 2) => ExprOp::CallFmod,
            ("clamp", 3) => ExprOp::CallClamp,
            _ => {
                return self.err(format!(
                    "unsupported function call in expr: {} with {} args",
                    name, arg_count
                ));
            }
        };
        self.push_op(op)
    }

    fn push_op(&mut self, op: ExprOp) -> Result<(), BridgeError> {
        if self.out_len >= runtime_core::MAX_EXPR_OPS {
            return self.err("expression too long");
        }
        self.output[self.out_len] = op;
        self.out_len += 1;
        Ok(())
    }

    fn consume_if(&mut self, token: ExprToken<'_>) -> bool {
        if self.peek().copied() == Some(token) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<&ExprToken<'a>> {
        self.tokens.get(self.pos)
    }

    fn err<T>(&self, detail: impl Into<String>) -> Result<T, BridgeError> {
        Err(BridgeError::UnsupportedAction {
            state: self.state_name.to_string(),
            action: format!("{}: {}", detail.into(), self.raw),
        })
    }
}

fn stable_log_message_id(message: &str) -> u16 {
    // Deterministic FNV-1a hash folded to 16-bit.
    let mut h: u32 = 0x811c9dc5;
    for b in message.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    (h ^ (h >> 16)) as u16
}

struct TopologyResolver<'a> {
    topology: &'a TopologyGraph,
    by_name: HashMap<&'a str, NodeIndex>,
}

struct CylinderMotionResolution {
    target: &'static str,
    output: DigitalOutputId,
    confirm_inputs: &'static [DigitalInputId],
    opposing_inputs: &'static [DigitalInputId],
}

impl<'a> TopologyResolver<'a> {
    fn new(topology: &'a TopologyGraph) -> Self {
        let mut by_name = HashMap::new();
        for idx in topology.graph.node_indices() {
            let device = &topology.graph[idx];
            by_name.insert(device.name.as_str(), idx);
        }
        Self { topology, by_name }
    }

    fn resolve_digital_input_id(
        &self,
        state_name: &str,
        device: &str,
    ) -> Result<DigitalInputId, BridgeError> {
        let start =
            self.by_name
                .get(device)
                .copied()
                .ok_or_else(|| BridgeError::UnknownDevice {
                    state: state_name.to_string(),
                    device: device.to_string(),
                })?;

        let ids = self.collect_input_physical_ids(start, DeviceKind::DigitalInput, parse_x_id);
        unique_physical_id(ids).map(DigitalInputId).map_err(|_| {
            BridgeError::UnresolvableDigitalInput {
                state: state_name.to_string(),
                device: device.to_string(),
            }
        })
    }

    fn resolve_digital_output_id(
        &self,
        state_name: &str,
        device: &str,
        port: &str,
    ) -> Result<DigitalOutputId, BridgeError> {
        if let Some(id) = self.resolve_digital_output_id_by_port(device, port) {
            return Ok(DigitalOutputId(id));
        }

        let start =
            self.by_name
                .get(device)
                .copied()
                .ok_or_else(|| BridgeError::UnknownDevice {
                    state: state_name.to_string(),
                    device: device.to_string(),
                })?;

        let ids = self.collect_physical_ids(start, DeviceKind::DigitalOutput, parse_y_id);
        unique_physical_id(ids).map(DigitalOutputId).map_err(|_| {
            BridgeError::UnresolvableDigitalOutput {
                state: state_name.to_string(),
                device: device.to_string(),
            }
        })
    }

    fn resolve_analog_output_id(
        &self,
        state_name: &str,
        device: &str,
        port: &str,
    ) -> Result<AnalogOutputId, BridgeError> {
        if let Some(id) = self.resolve_analog_output_id_by_port(device, port) {
            return Ok(AnalogOutputId(id));
        }

        let start =
            self.by_name
                .get(device)
                .copied()
                .ok_or_else(|| BridgeError::UnknownDevice {
                    state: state_name.to_string(),
                    device: device.to_string(),
                })?;

        let ids = self.collect_physical_ids(start, DeviceKind::AnalogOutput, parse_ao_id);
        unique_physical_id(ids).map(AnalogOutputId).map_err(|_| {
            BridgeError::UnresolvableAnalogOutput {
                state: state_name.to_string(),
                device: device.to_string(),
            }
        })
    }

    fn resolve_analog_input_id(
        &self,
        state_name: &str,
        device: &str,
    ) -> Result<AnalogInputId, BridgeError> {
        let start =
            self.by_name
                .get(device)
                .copied()
                .ok_or_else(|| BridgeError::UnknownDevice {
                    state: state_name.to_string(),
                    device: device.to_string(),
                })?;

        let ids = self.collect_input_physical_ids(start, DeviceKind::AnalogInput, parse_ai_id);
        unique_physical_id(ids).map(AnalogInputId).map_err(|_| {
            BridgeError::UnresolvableAnalogInput {
                state: state_name.to_string(),
                device: device.to_string(),
            }
        })
    }

    fn resolve_cylinder_motion(
        &self,
        state_name: &str,
        device: &str,
        port: &str,
        expect_extended: bool,
    ) -> Result<Option<CylinderMotionResolution>, BridgeError> {
        let start =
            self.by_name
                .get(device)
                .copied()
                .ok_or_else(|| BridgeError::UnknownDevice {
                    state: state_name.to_string(),
                    device: device.to_string(),
                })?;
        if self.topology.graph[start].kind != DeviceKind::Cylinder {
            return Ok(None);
        }

        let requested_port = state_port_key(
            port,
            if expect_extended {
                CylinderStrokeVerb::Extend.expected_state_port()
            } else {
                CylinderStrokeVerb::Retract.expected_state_port()
            },
        );
        let defined_state_ports = self.cylinder_detect_state_ports(device);
        if defined_state_ports.is_empty() {
            return Ok(None);
        }
        let confirm_ids = self.resolve_detect_state_input_ids(device, &requested_port);
        let opposing_port = cylinder_complementary_state_port(&requested_port).ok_or_else(|| {
            BridgeError::UnsupportedGuardExpression {
                state: state_name.to_string(),
                expression: format!(
                    "closed-loop cylinder action requires complementary end-state for {device}.{requested_port}"
                ),
            }
        })?;
        let opposing_ids = self.resolve_detect_state_input_ids(device, &opposing_port);
        if confirm_ids.is_empty() || opposing_ids.is_empty() {
            return Err(BridgeError::IncompleteClosedLoopCylinderMotion {
                state: state_name.to_string(),
                device: device.to_string(),
                requested_state: requested_port,
            });
        }

        Ok(Some(CylinderMotionResolution {
            target: Box::leak(device.to_string().into_boxed_str()),
            output: self.resolve_digital_output_id(state_name, device, port)?,
            confirm_inputs: leak_digital_input_ids(confirm_ids),
            opposing_inputs: leak_digital_input_ids(opposing_ids),
        }))
    }

    fn resolve_state_guard_instr(
        &self,
        state_name: &str,
        state_ref: &StateGuardRef,
        next: StepId,
        timeout: Option<Timeout>,
    ) -> Result<Instr<'static>, BridgeError> {
        let start = self
            .by_name
            .get(state_ref.device.as_str())
            .copied()
            .ok_or_else(|| BridgeError::UnknownDevice {
                state: state_name.to_string(),
                device: state_ref.device.clone(),
            })?;
        let device_kind = &self.topology.graph[start].kind;
        if *device_kind == DeviceKind::Cylinder {
            return Err(BridgeError::UnsupportedGuardExpression {
                state: state_name.to_string(),
                expression: format!("{}.{} == true", state_ref.device, state_ref.state),
            });
        }
        let requested_port = state_port_key(&state_ref.port, &state_ref.state);
        let target_ids = self.resolve_detect_state_input_ids(&state_ref.device, &requested_port);
        if target_ids.is_empty() {
            return Err(BridgeError::UnresolvableDigitalInput {
                state: state_name.to_string(),
                device: format!("{}.{}", state_ref.device, state_ref.state),
            });
        }

        let mut conditions = Vec::new();
        conditions.extend(target_ids.iter().copied().map(|id| DigitalCondition {
            id: DigitalInputId(id),
            equals: true,
        }));
        Ok(Instr::WaitAllDigital {
            conditions: leak_digital_conditions(conditions),
            next,
            timeout,
        })
    }

    fn axis_profile(&self, device: &str) -> Option<&crate::ir::AxisProfile> {
        self.topology.axis_profiles.get(device)
    }

    fn collect_physical_ids(
        &self,
        start: NodeIndex,
        kind: DeviceKind,
        parse: fn(&str) -> Option<u16>,
    ) -> Vec<u16> {
        let mut queue = VecDeque::new();
        let mut visited = HashSet::new();
        let mut out = Vec::new();

        queue.push_back(start);
        visited.insert(start);

        while let Some(n) = queue.pop_front() {
            let device = &self.topology.graph[n];
            if device.kind == kind {
                if let Some(id) = parse(&device.name) {
                    out.push(id);
                }
            }
            for link in self.topology.links.iter().filter(|link| {
                link.to == device.name && matches_physical_output_kind(&kind, &link.kind)
            }) {
                if let Some(id) = parse_link_source_physical_id(link, parse) {
                    out.push(id);
                }
            }

            for pred in self
                .topology
                .graph
                .neighbors_directed(n, Direction::Incoming)
            {
                if visited.insert(pred) {
                    queue.push_back(pred);
                }
            }
        }

        out
    }

    fn resolve_digital_output_id_by_port(&self, device: &str, port: &str) -> Option<u16> {
        let mut ids = self
            .topology
            .links
            .iter()
            .filter_map(|link| {
                if link.to != device || link.kind != crate::ir::ConnectionType::Electrical {
                    return None;
                }
                if port != "self" && link.to_port.as_deref() != Some(port) {
                    return None;
                }
                parse_link_source_physical_id(link, parse_y_id)
            })
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        match ids.as_slice() {
            [id] => Some(*id),
            _ => None,
        }
    }

    fn resolve_analog_output_id_by_port(&self, device: &str, port: &str) -> Option<u16> {
        let mut ids = self
            .topology
            .links
            .iter()
            .filter_map(|link| {
                if link.to != device || link.kind != crate::ir::ConnectionType::Analog {
                    return None;
                }
                if port != "self" && link.to_port.as_deref() != Some(port) {
                    return None;
                }
                parse_link_source_physical_id(link, parse_ao_id)
            })
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        match ids.as_slice() {
            [id] => Some(*id),
            _ => None,
        }
    }

    fn collect_input_physical_ids(
        &self,
        start: NodeIndex,
        kind: DeviceKind,
        parse: fn(&str) -> Option<u16>,
    ) -> Vec<u16> {
        let mut queue = VecDeque::new();
        let mut visited = HashSet::new();
        let mut out = Vec::new();

        queue.push_back(start);
        visited.insert(start);

        while let Some(n) = queue.pop_front() {
            let device = &self.topology.graph[n];
            if device.kind == kind {
                if let Some(id) = parse(&device.name) {
                    out.push(id);
                }
            }
            for link in self.topology.links.iter().filter(|link| {
                link.from == device.name && matches_physical_input_kind(&kind, &link.kind)
            }) {
                if let Some(id) = parse_link_target_physical_id(link, parse) {
                    out.push(id);
                }
            }

            for pred in self
                .topology
                .graph
                .neighbors_directed(n, Direction::Incoming)
            {
                let pred_kind = &self.topology.graph[pred].kind;
                if (*pred_kind == DeviceKind::Sensor || *pred_kind == kind) && visited.insert(pred)
                {
                    queue.push_back(pred);
                }
            }
            for succ in self
                .topology
                .graph
                .neighbors_directed(n, Direction::Outgoing)
            {
                let succ_kind = &self.topology.graph[succ].kind;
                if (*succ_kind == DeviceKind::Sensor || *succ_kind == kind) && visited.insert(succ)
                {
                    queue.push_back(succ);
                }
            }
        }

        out
    }

    fn resolve_detect_state_input_ids(&self, device: &str, state_port: &str) -> Vec<u16> {
        let mut ids = Vec::new();
        for sensor in self.detect_sensors_for_state_port(device, state_port) {
            ids.extend(self.sensor_reported_digital_input_ids(sensor));
        }
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    fn cylinder_detect_state_ports(&self, device: &str) -> Vec<String> {
        let mut state_ports = self
            .topology
            .links
            .iter()
            .filter_map(|link| {
                if link.from != device || link.kind != crate::ir::ConnectionType::Logical {
                    return None;
                }
                let port = link.from_port.as_deref()?;
                is_cylinder_end_state_port(port).then(|| port.to_string())
            })
            .collect::<Vec<_>>();
        state_ports.sort();
        state_ports.dedup();
        state_ports
    }

    fn detect_sensors_for_state_port(&self, device: &str, state_port: &str) -> Vec<&str> {
        let mut sensors = self
            .topology
            .links
            .iter()
            .filter_map(|link| {
                if link.from != device || link.kind != crate::ir::ConnectionType::Logical {
                    return None;
                }
                if !state_port_matches(link.from_port.as_deref(), state_port) {
                    return None;
                }
                Some(link.to.as_str())
            })
            .collect::<Vec<_>>();
        sensors.sort_unstable();
        sensors.dedup();
        sensors
    }

    fn sensor_reported_digital_input_ids(&self, sensor: &str) -> Vec<u16> {
        let mut ids = self
            .topology
            .links
            .iter()
            .filter_map(|link| {
                if link.from != sensor || link.kind != crate::ir::ConnectionType::Logical {
                    return None;
                }
                parse_link_target_physical_id(link, parse_x_id)
            })
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    fn sensor_is_cylinder_end_feedback(&self, sensor: &str) -> bool {
        self.topology.links.iter().any(|link| {
            link.to == sensor
                && link.kind == crate::ir::ConnectionType::Logical
                && link
                    .from_port
                    .as_deref()
                    .is_some_and(is_cylinder_end_state_port)
                && self
                    .by_name
                    .get(link.from.as_str())
                    .is_some_and(|idx| self.topology.graph[*idx].kind == DeviceKind::Cylinder)
        })
    }

    fn digital_input_is_cylinder_end_feedback(&self, id: DigitalInputId) -> bool {
        self.topology
            .graph
            .node_indices()
            .filter(|idx| self.topology.graph[*idx].kind == DeviceKind::Sensor)
            .map(|idx| self.topology.graph[idx].name.as_str())
            .filter(|sensor| self.sensor_is_cylinder_end_feedback(sensor))
            .any(|sensor| {
                self.sensor_reported_digital_input_ids(sensor)
                    .into_iter()
                    .any(|candidate| candidate == id.0)
            })
    }
}

fn leak_digital_input_ids(ids: Vec<u16>) -> &'static [DigitalInputId] {
    let leaked = ids
        .into_iter()
        .map(DigitalInputId)
        .collect::<Vec<DigitalInputId>>();
    Box::leak(leaked.into_boxed_slice())
}

fn leak_digital_conditions(conditions: Vec<DigitalCondition>) -> &'static [DigitalCondition] {
    Box::leak(conditions.into_boxed_slice())
}

fn state_port_matches(actual: Option<&str>, requested: &str) -> bool {
    matches!(actual, Some(port) if port == requested)
}

fn matches_physical_output_kind(kind: &DeviceKind, link_kind: &crate::ir::ConnectionType) -> bool {
    matches!(
        (kind, link_kind),
        (
            &DeviceKind::DigitalOutput,
            crate::ir::ConnectionType::Electrical
        ) | (&DeviceKind::AnalogOutput, crate::ir::ConnectionType::Analog)
    )
}

fn matches_physical_input_kind(kind: &DeviceKind, link_kind: &crate::ir::ConnectionType) -> bool {
    matches!(
        (kind, link_kind),
        (
            &DeviceKind::DigitalInput,
            crate::ir::ConnectionType::Logical
        ) | (&DeviceKind::AnalogInput, crate::ir::ConnectionType::Analog)
    )
}

fn parse_link_source_physical_id(
    link: &crate::ir::TopologyLink,
    parse: fn(&str) -> Option<u16>,
) -> Option<u16> {
    link.from_port
        .as_deref()
        .and_then(parse)
        .or_else(|| parse(&link.from))
}

fn parse_link_target_physical_id(
    link: &crate::ir::TopologyLink,
    parse: fn(&str) -> Option<u16>,
) -> Option<u16> {
    link.to_port
        .as_deref()
        .and_then(parse)
        .or_else(|| parse(&link.to))
}

fn unique_physical_id(mut ids: Vec<u16>) -> Result<u16, ()> {
    ids.sort_unstable();
    ids.dedup();
    match ids.len() {
        1 => Ok(ids[0]),
        _ => Err(()),
    }
}

fn parse_x_id(name: &str) -> Option<u16> {
    match parse_physical_plc_port_ref(name) {
        Some(port) if matches!(port.kind, PlcPortKind::DigitalInput) => Some(port.id),
        _ => None,
    }
}

fn parse_y_id(name: &str) -> Option<u16> {
    match parse_physical_plc_port_ref(name) {
        Some(port) if matches!(port.kind, PlcPortKind::DigitalOutput) => Some(port.id),
        _ => None,
    }
}

fn parse_ao_id(name: &str) -> Option<u16> {
    match parse_physical_plc_port_ref(name) {
        Some(port) if matches!(port.kind, PlcPortKind::AnalogOutput) => Some(port.id),
        _ => None,
    }
}

fn parse_ai_id(name: &str) -> Option<u16> {
    match parse_physical_plc_port_ref(name) {
        Some(port) if matches!(port.kind, PlcPortKind::AnalogInput) => Some(port.id),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::device_semantics::cylinder::{
        complementary_end_state_port as cylinder_complementary_state_port,
        is_end_state_port as is_cylinder_end_state_port,
    };

    #[test]
    fn cylinder_complementary_state_port_maps_default_end_states() {
        assert_eq!(
            cylinder_complementary_state_port("extended").as_deref(),
            Some("retracted")
        );
        assert_eq!(
            cylinder_complementary_state_port("retracted").as_deref(),
            Some("extended")
        );
    }

    #[test]
    fn cylinder_complementary_state_port_preserves_port_scope() {
        assert_eq!(
            cylinder_complementary_state_port("rod_a.extended").as_deref(),
            Some("rod_a.retracted")
        );
        assert_eq!(
            cylinder_complementary_state_port("rod_a.retracted").as_deref(),
            Some("rod_a.extended")
        );
        assert_eq!(cylinder_complementary_state_port("mid"), None);
    }

    #[test]
    fn cylinder_end_state_port_detection_matches_only_terminal_feedback() {
        assert!(is_cylinder_end_state_port("extended"));
        assert!(is_cylinder_end_state_port("retracted"));
        assert!(is_cylinder_end_state_port("rod_a.extended"));
        assert!(is_cylinder_end_state_port("rod_a.retracted"));
        assert!(!is_cylinder_end_state_port("sense"));
        assert!(!is_cylinder_end_state_port("mid"));
    }

    #[test]
    fn state_port_match_requires_exact_port_scope() {
        assert!(super::state_port_matches(Some("extended"), "extended"));
        assert!(super::state_port_matches(Some("rod_a.extended"), "rod_a.extended"));
        assert!(!super::state_port_matches(Some("extended"), "rod_a.extended"));
        assert!(!super::state_port_matches(Some("rod_a.extended"), "extended"));
    }
}
