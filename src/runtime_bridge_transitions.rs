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
