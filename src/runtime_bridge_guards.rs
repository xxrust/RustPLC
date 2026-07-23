fn condition_to_wait_instr<'a>(
    arena: &'a Bump,
    resolver: &TopologyResolver,
    state_name: &str,
    expression: &str,
    sm: &StateMachine,
    variable_indices: &HashMap<String, u16>,
    cam_indices: &HashMap<String, u16>,
    cond_next: StepId,
    timeout: Option<Timeout>,
) -> Result<Instr<'a>, BridgeError> {
    let expr = expression.trim();
    let analog_wait = parse_analog_region_guard(expr);

    if let Some((device, ranges)) = analog_wait {
        let id = resolver.resolve_analog_input_id(state_name, &device)?;
        let analog_ranges = ranges_to_analog_ranges(arena, sm, state_name, &device, &ranges)?;
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
            arena,
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

fn edge_guard_to_instr<'a>(
    resolver: &TopologyResolver,
    state_name: &str,
    edge: IrEdgeKind,
    operand: &str,
    variable_indices: &HashMap<String, u16>,
    next: StepId,
    timeout: Option<Timeout>,
) -> Result<Instr<'a>, BridgeError> {
    let rt_edge = match edge {
        IrEdgeKind::Rising => RtEdgeKind::Rising,
        IrEdgeKind::Falling => RtEdgeKind::Falling,
    };
    match parse_bool_guard_operand(operand).ok_or_else(|| BridgeError::UnsupportedGuardExpression {
        state: state_name.to_string(),
        expression: format!("edge({operand})"),
    })? {
        BoolGuardOperand::Identifier(name) => {
            if let Some(index) = variable_indices.get(&name).copied() {
                Ok(Instr::WaitVariableEdge {
                    index,
                    edge: rt_edge,
                    next,
                    timeout,
                })
            } else {
                if resolver.sensor_is_cylinder_end_feedback(&name) {
                    return Err(BridgeError::UnsupportedGuardExpression {
                        state: state_name.to_string(),
                        expression: format!("edge({name})"),
                    });
                }
                let id = resolver.resolve_digital_input_id(state_name, &name)?;
                if resolver.digital_input_is_cylinder_end_feedback(id) {
                    return Err(BridgeError::UnsupportedGuardExpression {
                        state: state_name.to_string(),
                        expression: format!("edge({name})"),
                    });
                }
                Ok(Instr::WaitDigitalEdge {
                    id,
                    edge: rt_edge,
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
            let id = DigitalInputId(port.id);
            if resolver.digital_input_is_cylinder_end_feedback(id) {
                return Err(BridgeError::UnsupportedGuardExpression {
                    state: state_name.to_string(),
                    expression: raw,
                });
            }
            Ok(Instr::WaitDigitalEdge {
                id,
                edge: rt_edge,
                next,
                timeout,
            })
        }
        BoolGuardOperand::StateRef(state_ref) => Err(BridgeError::UnsupportedGuardExpression {
            state: state_name.to_string(),
            expression: format!(
                "edge({}.{}.{})",
                state_ref.device, state_ref.port, state_ref.state
            ),
        }),
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
    fn into_instr<'a>(self, next: StepId, timeout: Option<Timeout>) -> Instr<'a> {
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

fn bool_guard_to_instr<'a>(
    arena: &'a Bump,
    resolver: &TopologyResolver,
    state_name: &str,
    lhs: BoolGuardOperand,
    equals: bool,
    variable_indices: &HashMap<String, u16>,
    next: StepId,
    timeout: Option<Timeout>,
) -> Result<Instr<'a>, BridgeError> {
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
            resolver.resolve_state_guard_instr(arena, state_name, &state_ref, next, timeout)
        }
    }
}

fn ranges_to_analog_ranges<'a>(
    arena: &'a Bump,
    sm: &StateMachine,
    state_name: &str,
    device: &str,
    region_indices: &[usize],
) -> Result<&'a [AnalogRange], BridgeError> {
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

    Ok(arena.alloc_slice_copy(&out))
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

fn leak_axis_fault_route_rules<'a>(
    arena: &'a Bump,
    state_name: &str,
    branch_label: &str,
    routes: &[crate::ir::AxisFaultRouteBranch],
    state_to_step: &HashMap<(String, String), StepId>,
    task_entry_steps: &HashMap<String, StepId>,
) -> Result<&'a [AxisFaultRouteRule], BridgeError> {
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
    Ok(arena.alloc_slice_copy(&out))
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
