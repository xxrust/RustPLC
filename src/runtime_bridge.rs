use crate::ir::{
    BinaryValue as IrBinaryValue, CamInterpolation as IrCamInterpolation, DeviceKind, State,
    StateMachine, TopologyGraph, Transition, TransitionAction, TransitionGuard,
};
use crate::plc_port::{PlcPortKind, parse_physical_plc_port_ref};
use io_traits::{AnalogInputId, AnalogOutputId, DigitalInputId, DigitalOutputId};
use petgraph::Direction;
use petgraph::graph::NodeIndex;
use runtime_core::{
    Action, AnalogRange, AntiWindup, CamAnalogField, CamCouplingConfig, CamDigitalField,
    CamInterpolation as RtCamInterpolation, CamTableData, CompareOp, ExprOp, ExprProgram, Instr,
    MAX_CAM_POINTS, PidConfig, Program, SplineCoeff as RtSplineCoeff, Step, StepId, Task, Timeout,
};
use std::collections::{HashMap, HashSet, VecDeque};

const MAX_TRANSITIONS_PER_TICK: usize = 64;

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
/// - This converter flattens all PLC tasks into a single `runtime-core` task.
/// - Generated program uses leaked allocations to produce a `'static` `Program`.
pub fn state_machine_to_runtime_program(
    topology: &TopologyGraph,
    sm: &StateMachine,
    tick_ms: u64,
) -> Result<Program<'static>, BridgeError> {
    if tick_ms == 0 {
        return Err(BridgeError::InvalidTickMs);
    }
    validate_extern_tick_budget(topology, sm, tick_ms)?;

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

    // Assign StepId to every IR state (flattened).
    let mut state_to_step = HashMap::<(String, String), StepId>::new();
    let mut step_names: Vec<&'static str> = Vec::with_capacity(sm.states.len());

    for (idx, s) in sm.states.iter().enumerate() {
        let name = format!("{}.{}", s.task_name, s.step_name);
        let leaked: &'static str = Box::leak(name.into_boxed_str());
        step_names.push(leaked);
        state_to_step.insert(
            (s.task_name.clone(), s.step_name.clone()),
            StepId(idx as u16),
        );
    }

    let initial_id = state_to_step
        .get(&(sm.initial.task_name.clone(), sm.initial.step_name.clone()))
        .copied()
        .ok_or_else(|| BridgeError::MissingInitialState {
            state: format!("{}.{}", sm.initial.task_name, sm.initial.step_name),
        })?;

    // Index transitions by from-state.
    let mut outgoing: HashMap<(String, String), Vec<&Transition>> = HashMap::new();
    for t in &sm.transitions {
        outgoing
            .entry((t.from.task_name.clone(), t.from.step_name.clone()))
            .or_default()
            .push(t);
    }

    // Placeholder steps for the base IR states.
    let mut steps: Vec<Step<'static>> = step_names
        .iter()
        .map(|&name| Step {
            name,
            instr: Instr::Halt,
        })
        .collect();

    for (idx, s) in sm.states.iter().enumerate() {
        let state_name = format!("{}.{}", s.task_name, s.step_name);
        let outs = outgoing
            .get(&(s.task_name.clone(), s.step_name.clone()))
            .cloned()
            .unwrap_or_default();

        let instr = convert_state_outgoing(
            &resolver,
            &state_name,
            &outs,
            &state_to_step,
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

    // Leak steps/tasks/program as 'static (sufficient for CLI + tests for now).
    let leaked_steps: &'static [Step<'static>] = Box::leak(steps.into_boxed_slice());
    let task = Task {
        name: "plc",
        steps: leaked_steps,
        entry: initial_id,
    };
    let leaked_tasks: &'static [Task<'static>] = Box::leak(vec![task].into_boxed_slice());

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
    Ok(Program {
        tasks: leaked_tasks,
        pid_loops: leaked_pid_loops,
        var_init: leaked_var_init,
        cam_configs: leaked_cam_configs,
        cam_tables: leaked_cam_tables,
    })
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
            MAX_TRANSITIONS_PER_TICK,
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
    state_to_step: &HashMap<(String, String), StepId>,
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
            state_to_step,
            steps,
            sm,
            tick_ms,
            variable_indices,
            cam_indices,
            cam_table_indices,
            extern_signatures,
        ),
        2 => convert_wait_with_timeout(
            resolver,
            state_name,
            outs,
            state_to_step,
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
    state_to_step: &HashMap<(String, String), StepId>,
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
            if t.actions.is_empty() {
                Ok(Instr::Goto { target })
            } else {
                let actions = leak_actions(
                    resolver,
                    state_name,
                    &t.actions,
                    variable_indices,
                    cam_indices,
                    cam_table_indices,
                    extern_signatures,
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

            if t.actions.is_empty() {
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
                    target,
                    variable_indices,
                    cam_indices,
                    cam_table_indices,
                    extern_signatures,
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
            let next = if t.actions.is_empty() {
                target
            } else {
                push_action_step(
                    steps,
                    &format!("{state_name}__cond_actions"),
                    resolver,
                    state_name,
                    &t.actions,
                    target,
                    variable_indices,
                    cam_indices,
                    cam_table_indices,
                    extern_signatures,
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
                if variable_indices.contains_key(&lhs) {
                    let left = compile_guard_expr_program(state_name, &lhs, variable_indices)?;
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
                        timeout: None,
                    })
                } else {
                    let id = resolver.resolve_digital_input_id(state_name, &lhs)?;
                    Ok(Instr::WaitDigital {
                        id,
                        equals,
                        next,
                        timeout: None,
                    })
                }
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

fn convert_wait_with_timeout(
    resolver: &TopologyResolver,
    state_name: &str,
    outs: &[&Transition],
    state_to_step: &HashMap<(String, String), StepId>,
    steps: &mut Vec<Step<'static>>,
    sm: &StateMachine,
    tick_ms: u64,
    variable_indices: &HashMap<String, u16>,
    cam_indices: &HashMap<String, u16>,
    cam_table_indices: &HashMap<String, u16>,
    extern_signatures: &HashMap<String, (usize, usize)>,
) -> Result<Instr<'static>, BridgeError> {
    let (cond, timeout) = match (outs[0], outs[1]) {
        (a, b)
            if matches!(a.guard, TransitionGuard::Condition { .. })
                && matches!(b.guard, TransitionGuard::Timeout { .. }) =>
        {
            (a, b)
        }
        (a, b)
            if matches!(b.guard, TransitionGuard::Condition { .. })
                && matches!(a.guard, TransitionGuard::Timeout { .. }) =>
        {
            (b, a)
        }
        _ => {
            return Err(BridgeError::UnsupportedTransitionShape {
                state: state_name.to_string(),
                details: "expected exactly one condition and one timeout transition".to_string(),
            });
        }
    };

    let TransitionGuard::Condition { expression } = &cond.guard else {
        unreachable!();
    };
    let TransitionGuard::Timeout { duration_ms } = &timeout.guard else {
        unreachable!();
    };

    let expr = expression.trim();
    let analog_wait = parse_analog_region_guard(expr);

    let cond_target = lookup_target_step(state_name, &cond.to, state_to_step)?;
    let cond_next = if cond.actions.is_empty() {
        cond_target
    } else {
        push_action_step(
            steps,
            &format!("{state_name}__cond_actions"),
            resolver,
            state_name,
            &cond.actions,
            cond_target,
            variable_indices,
            cam_indices,
            cam_table_indices,
            extern_signatures,
        )?
    };

    let timeout_target = lookup_target_step(state_name, &timeout.to, state_to_step)?;
    let timeout_next = if timeout.actions.is_empty() {
        timeout_target
    } else {
        push_action_step(
            steps,
            &format!("{state_name}__timeout_actions"),
            resolver,
            state_name,
            &timeout.actions,
            timeout_target,
            variable_indices,
            cam_indices,
            cam_table_indices,
            extern_signatures,
        )?
    };

    let after_ticks = ms_to_ticks(state_name, *duration_ms, tick_ms)?;
    if let Some((device, ranges)) = analog_wait {
        let id = resolver.resolve_analog_input_id(state_name, &device)?;
        let analog_ranges = ranges_to_analog_ranges(sm, state_name, &device, &ranges)?;
        Ok(Instr::WaitAnalog {
            id,
            ranges: analog_ranges,
            next: cond_next,
            timeout: Some(Timeout {
                after_ticks,
                target: timeout_next,
            }),
        })
    } else if let Some(cam_guard) = parse_cam_wait_guard(expr, cam_indices) {
        Ok(cam_guard.into_instr(
            cond_next,
            Some(Timeout {
                after_ticks,
                target: timeout_next,
            }),
        ))
    } else if let Ok((lhs, equals)) = parse_single_bool_guard(state_name, expr) {
        if variable_indices.contains_key(&lhs) {
            let left = compile_guard_expr_program(state_name, &lhs, variable_indices)?;
            let right = compile_guard_expr_program(
                state_name,
                if equals { "1.0" } else { "0.0" },
                variable_indices,
            )?;
            Ok(Instr::WaitExpr {
                left,
                op: CompareOp::Eq,
                right,
                next: cond_next,
                timeout: Some(Timeout {
                    after_ticks,
                    target: timeout_next,
                }),
            })
        } else {
            let id = resolver.resolve_digital_input_id(state_name, &lhs)?;
            Ok(Instr::WaitDigital {
                id,
                equals,
                next: cond_next,
                timeout: Some(Timeout {
                    after_ticks,
                    target: timeout_next,
                }),
            })
        }
    } else if let Some((left_raw, op, right_raw)) = parse_compare_guard(expr) {
        let left = compile_guard_expr_program(state_name, &left_raw, variable_indices)?;
        let right = compile_guard_expr_program(state_name, &right_raw, variable_indices)?;
        Ok(Instr::WaitExpr {
            left,
            op,
            right,
            next: cond_next,
            timeout: Some(Timeout {
                after_ticks,
                target: timeout_next,
            }),
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
    !lhs.is_empty()
        && !lhs.contains('.')
        && (op == "==" || op == "!=")
        && (rhs == "true" || rhs == "false")
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

fn parse_single_bool_guard(
    state_name: &str,
    expression: &str,
) -> Result<(String, bool), BridgeError> {
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

    if lhs.is_empty() || lhs.contains('.') {
        return Err(BridgeError::UnsupportedGuardExpression {
            state: state_name.to_string(),
            expression: expr.to_string(),
        });
    }

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

    Ok((lhs.to_string(), equals))
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
    next: StepId,
    variable_indices: &HashMap<String, u16>,
    cam_indices: &HashMap<String, u16>,
    cam_table_indices: &HashMap<String, u16>,
    extern_signatures: &HashMap<String, (usize, usize)>,
) -> Result<StepId, BridgeError> {
    let leaked_name: &'static str = Box::leak(name.to_string().into_boxed_str());
    let leaked_actions = leak_actions(
        resolver,
        state_name,
        actions,
        variable_indices,
        cam_indices,
        cam_table_indices,
        extern_signatures,
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
    variable_indices: &HashMap<String, u16>,
    cam_indices: &HashMap<String, u16>,
    cam_table_indices: &HashMap<String, u16>,
    extern_signatures: &HashMap<String, (usize, usize)>,
) -> Result<&'static [Action], BridgeError> {
    let mut out: Vec<Action> = Vec::with_capacity(actions.len());
    for a in actions {
        out.push(convert_action(
            resolver,
            state_name,
            a,
            variable_indices,
            cam_indices,
            cam_table_indices,
            extern_signatures,
        )?);
    }
    Ok(Box::leak(out.into_boxed_slice()))
}

fn convert_action(
    resolver: &TopologyResolver,
    state_name: &str,
    a: &TransitionAction,
    variable_indices: &HashMap<String, u16>,
    cam_indices: &HashMap<String, u16>,
    cam_table_indices: &HashMap<String, u16>,
    extern_signatures: &HashMap<String, (usize, usize)>,
) -> Result<Action, BridgeError> {
    match a {
        TransitionAction::Extend { target, port } => {
            let output = resolver.resolve_digital_output_id(state_name, target, port)?;
            Ok(Action::Extend { output })
        }
        TransitionAction::Retract { target, port } => {
            let output = resolver.resolve_digital_output_id(state_name, target, port)?;
            Ok(Action::Retract { output })
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

            let Some((expected_args, expected_returns)) = extern_signatures.get(function).copied()
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
                        action: format!("{rendered_action} (unknown binding variable {target})"),
                    });
                };
                binding_vars.push(index);
            }

            let leaked_function: &'static str = Box::leak(function.clone().into_boxed_str());
            let leaked_arg_exprs: &'static [ExprProgram] = Box::leak(arg_exprs.into_boxed_slice());
            let leaked_binding_vars: &'static [u16] = Box::leak(binding_vars.into_boxed_slice());
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
            let offset_expr = compile_expr_program(state_name, offset_expr_raw, variable_indices)?;
            Ok(Action::CamPhase {
                cam_index,
                offset_expr,
            })
        }
        TransitionAction::AxisMoveRelative {
            target,
            distance_raw,
            speed_raw,
            ..
        } => Err(BridgeError::UnsupportedAction {
            state: state_name.to_string(),
            action: format!("axis.move_relative {target} distance:{distance_raw} speed:{speed_raw}"),
        }),
        TransitionAction::AxisMoveAbsolute {
            target,
            position_raw,
            speed_raw,
            ..
        } => Err(BridgeError::UnsupportedAction {
            state: state_name.to_string(),
            action: format!("axis.move_absolute {target} position:{position_raw} speed:{speed_raw}"),
        }),
        TransitionAction::Log { message } => {
            let leaked_message: &'static str = Box::leak(message.clone().into_boxed_str());
            Ok(Action::Log {
                message_id: stable_log_message_id(message),
                message: leaked_message,
            })
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
                parse_y_id(&link.from)
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
                parse_ao_id(&link.from)
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
