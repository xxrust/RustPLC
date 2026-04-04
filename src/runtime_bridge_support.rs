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
                    if motion_timeout.is_some()
                        || on_motion_fault.is_some()
                        || on_safety_fault.is_some()
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
                    if motion_timeout.is_some()
                        || on_motion_fault.is_some()
                        || on_safety_fault.is_some()
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
