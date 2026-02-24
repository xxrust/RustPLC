use crate::ir::{
    BinaryValue as IrBinaryValue, DeviceKind, State, StateMachine, TopologyGraph, Transition,
    TransitionAction, TransitionGuard,
};
use crate::plc_port::{PlcPortKind, parse_physical_plc_port_ref};
use io_traits::{AnalogInputId, AnalogOutputId, DigitalInputId, DigitalOutputId};
use petgraph::Direction;
use petgraph::graph::NodeIndex;
use runtime_core::{
    Action, AnalogRange, AntiWindup, Instr, PidConfig, Program, Step, StepId, Task, Timeout,
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
}

/// Convert a compiler/semantic `StateMachine` IR into a minimal `runtime-core` `Program`.
///
/// Supported subset:
/// - `action`: set (digital), extend, retract
/// - `action`: log
/// - `action`: set_analog
/// - `wait`: single boolean equality/inequality (no AND/OR/NOT)
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

    let resolver = TopologyResolver::new(topology);

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
    Ok(Program {
        tasks: leaked_tasks,
        pid_loops: leaked_pid_loops,
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
        let out_id = resolver.resolve_analog_output_id(&ctx, &loop_spec.out)?;

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

fn convert_state_outgoing(
    resolver: &TopologyResolver,
    state_name: &str,
    outs: &[&Transition],
    state_to_step: &HashMap<(String, String), StepId>,
    steps: &mut Vec<Step<'static>>,
    sm: &StateMachine,
    tick_ms: u64,
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
        ),
        2 => convert_wait_with_timeout(
            resolver,
            state_name,
            outs,
            state_to_step,
            steps,
            sm,
            tick_ms,
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
) -> Result<Instr<'static>, BridgeError> {
    match &t.guard {
        TransitionGuard::Always => {
            let target = lookup_target_step(state_name, &t.to, state_to_step)?;
            if t.actions.is_empty() {
                Ok(Instr::Goto { target })
            } else {
                let actions = leak_actions(resolver, state_name, &t.actions)?;
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
                )?;
                Ok(Instr::Delay {
                    ticks,
                    next: action_step,
                })
            }
        }
        TransitionGuard::Condition { expression } => {
            let expr = expression.trim();
            if let Some((device, ranges)) = parse_analog_region_guard(expr) {
                let id = resolver.resolve_analog_input_id(state_name, &device)?;
                let analog_ranges = ranges_to_analog_ranges(sm, state_name, &device, &ranges)?;

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
                    )?
                };

                Ok(Instr::WaitAnalog {
                    id,
                    ranges: analog_ranges,
                    next,
                    timeout: None,
                })
            } else {
                let (device, equals) = parse_single_bool_guard(state_name, expr)?;
                let id = resolver.resolve_digital_input_id(state_name, &device)?;

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
                    )?
                };

                Ok(Instr::WaitDigital {
                    id,
                    equals,
                    next,
                    timeout: None,
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
    } else {
        let (device, equals) = parse_single_bool_guard(state_name, expr)?;
        let id = resolver.resolve_digital_input_id(state_name, &device)?;
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

fn push_action_step(
    steps: &mut Vec<Step<'static>>,
    name: &str,
    resolver: &TopologyResolver,
    state_name: &str,
    actions: &[TransitionAction],
    next: StepId,
) -> Result<StepId, BridgeError> {
    let leaked_name: &'static str = Box::leak(name.to_string().into_boxed_str());
    let leaked_actions = leak_actions(resolver, state_name, actions)?;
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
) -> Result<&'static [Action], BridgeError> {
    let mut out: Vec<Action> = Vec::with_capacity(actions.len());
    for a in actions {
        out.push(convert_action(resolver, state_name, a)?);
    }
    Ok(Box::leak(out.into_boxed_slice()))
}

fn convert_action(
    resolver: &TopologyResolver,
    state_name: &str,
    a: &TransitionAction,
) -> Result<Action, BridgeError> {
    match a {
        TransitionAction::Extend { target, .. } => {
            let output = resolver.resolve_digital_output_id(state_name, target)?;
            Ok(Action::Extend { output })
        }
        TransitionAction::Retract { target, .. } => {
            let output = resolver.resolve_digital_output_id(state_name, target)?;
            Ok(Action::Retract { output })
        }
        TransitionAction::Set { target, value, .. } => {
            let id = resolver.resolve_digital_output_id(state_name, target)?;
            let value = match value {
                IrBinaryValue::On => true,
                IrBinaryValue::Off => false,
            };
            Ok(Action::SetDigital { id, value })
        }
        TransitionAction::SetAnalog { target, value_raw, .. } => {
            let id = resolver.resolve_analog_output_id(state_name, target)?;
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
        TransitionAction::Log { message } => {
            let leaked_message: &'static str = Box::leak(message.clone().into_boxed_str());
            Ok(Action::Log {
                message_id: stable_log_message_id(message),
                message: leaked_message,
            })
        }
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
    ) -> Result<DigitalOutputId, BridgeError> {
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
    ) -> Result<AnalogOutputId, BridgeError> {
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
