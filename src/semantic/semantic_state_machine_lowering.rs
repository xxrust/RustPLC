fn build_parallel_block(
    builder: &mut StateMachineBuilder,
    task: &TaskDeclaration,
    step_name: &str,
    source_state: &State,
    block_index: usize,
    block: &ParallelBlock,
    completion_target: Option<State>,
    task_initial_states: &HashMap<String, State>,
    task_defined_steps: &HashMap<String, HashSet<String>>,
    errors: &mut Vec<PlcError>,
    parent_actions: Vec<TransitionAction>,
    wait_ctx: &WaitExpressionContext,
) {
    let fork_state_name = format!("{step_name}__parallel_{}_fork", block_index + 1);
    let join_state_name = format!("{step_name}__parallel_{}_join", block_index + 1);

    let fork_state = builder.add_state(&task.name, &fork_state_name);
    let join_state = builder.add_state(&task.name, &join_state_name);

    builder.add_transition(
        source_state.clone(),
        fork_state.clone(),
        TransitionGuard::Always,
        parent_actions,
        Vec::new(),
        Vec::new(),
    );

    let mut previous_state = fork_state.clone();

    for (branch_index, branch) in block.branches.iter().enumerate() {
        let branch_state_name = format!(
            "{step_name}__parallel_{}_branch_{}_active",
            block_index + 1,
            branch_index + 1
        );
        let branch_state = builder.add_state(&task.name, &branch_state_name);
        let branch_done_state_name = format!(
            "{step_name}__parallel_{}_branch_{}_done",
            block_index + 1,
            branch_index + 1
        );
        let branch_done_state = builder.add_state(&task.name, &branch_done_state_name);

        builder.add_transition(
            previous_state.clone(),
            branch_state.clone(),
            TransitionGuard::Always,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );

        let analyzed = analyze_statements(&branch.statements, wait_ctx);

        for goto in &analyzed.gotos {
            if let Some(target) = resolve_task_target(
                goto,
                task_initial_states,
                task_defined_steps,
                errors,
                "goto",
            ) {
                builder.add_transition(
                    branch_state.clone(),
                    target,
                    TransitionGuard::Always,
                    analyzed.actions.clone(),
                    analyzed.effects.clone(),
                    Vec::new(),
                );
            }
        }

        for if_else in &analyzed.if_elses {
            let expr = condition_to_expression(&if_else.condition);

            if let Some(then_target) = resolve_task_target(
                &if_else.then_goto,
                task_initial_states,
                task_defined_steps,
                errors,
                "if/else then goto",
            ) {
                builder.add_transition(
                    branch_state.clone(),
                    then_target,
                    TransitionGuard::Condition {
                        expression: expr.clone(),
                    },
                    analyzed.actions.clone(),
                    analyzed.effects.clone(),
                    Vec::new(),
                );
            }

            if let Some(else_target) = resolve_task_target(
                &if_else.else_goto,
                task_initial_states,
                task_defined_steps,
                errors,
                "if/else else goto",
            ) {
                builder.add_transition(
                    branch_state.clone(),
                    else_target,
                    TransitionGuard::Condition {
                        expression: format!("NOT({expr})"),
                    },
                    analyzed.actions.clone(),
                    analyzed.effects.clone(),
                    Vec::new(),
                );
            }
        }

        for (delay_index, duration_ms) in analyzed.delays_ms.iter().enumerate() {
            builder.add_transition(
                branch_state.clone(),
                branch_done_state.clone(),
                TransitionGuard::Delay {
                    duration_ms: *duration_ms,
                },
                Vec::new(),
                Vec::new(),
                vec![TimerOperation {
                    timer_name: format!(
                        "{}.{}.parallel_{}_branch_{}.delay_{}",
                        task.name,
                        step_name,
                        block_index + 1,
                        branch_index + 1,
                        delay_index + 1
                    ),
                    operation: TimerOperationKind::Start,
                    duration_ms: Some(*duration_ms),
                }],
            );
        }

        for (timeout_index, timeout) in analyzed.timeouts.iter().enumerate() {
            if let Some(target) = resolve_task_target(
                &timeout.target,
                task_initial_states,
                task_defined_steps,
                errors,
                "timeout -> goto",
            ) {
                let duration_ms = duration_to_ms(timeout);
                builder.add_transition(
                    branch_state.clone(),
                    target,
                    TransitionGuard::Timeout { duration_ms },
                    Vec::new(),
                    Vec::new(),
                    vec![TimerOperation {
                        timer_name: format!(
                            "{}.{}.parallel_{}_branch_{}.timeout_{}",
                            task.name,
                            step_name,
                            block_index + 1,
                            branch_index + 1,
                            timeout_index + 1
                        ),
                        operation: TimerOperationKind::Start,
                        duration_ms: Some(duration_ms),
                    }],
                );
            }
        }

        for wait_expression in &analyzed.waits {
            builder.add_transition(
                branch_state.clone(),
                branch_done_state.clone(),
                TransitionGuard::Condition {
                    expression: wait_expression.clone(),
                },
                analyzed.actions.clone(),
                analyzed.effects.clone(),
                Vec::new(),
            );
        }

        for (nested_parallel_index, nested_parallel) in analyzed.parallel_blocks.iter().enumerate()
        {
            build_parallel_block(
                builder,
                task,
                &format!(
                    "{step_name}__parallel_{}_branch_{}_active",
                    block_index + 1,
                    branch_index + 1
                ),
                &branch_state,
                nested_parallel_index,
                nested_parallel,
                Some(branch_done_state.clone()),
                task_initial_states,
                task_defined_steps,
                errors,
                analyzed.actions.clone(),
                wait_ctx,
            );
        }

        for (nested_race_index, nested_race) in analyzed.race_blocks.iter().enumerate() {
            build_race_block(
                builder,
                task,
                &format!(
                    "{step_name}__parallel_{}_branch_{}_active",
                    block_index + 1,
                    branch_index + 1
                ),
                &branch_state,
                nested_race_index,
                nested_race,
                Some(branch_done_state.clone()),
                task_initial_states,
                task_defined_steps,
                errors,
                analyzed.actions.clone(),
                wait_ctx,
            );
        }

        let has_control_flow = !analyzed.waits.is_empty()
            || !analyzed.delays_ms.is_empty()
            || !analyzed.gotos.is_empty()
            || !analyzed.if_elses.is_empty()
            || !analyzed.parallel_blocks.is_empty()
            || !analyzed.race_blocks.is_empty();
        if !has_control_flow {
            builder.add_transition(
                branch_state,
                branch_done_state.clone(),
                TransitionGuard::Always,
                analyzed.actions,
                analyzed.effects,
                Vec::new(),
            );
        }

        previous_state = branch_done_state;
    }

    builder.add_transition(
        previous_state,
        join_state.clone(),
        TransitionGuard::Always,
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );

    if let Some(target) = completion_target {
        builder.add_transition(
            join_state,
            target,
            TransitionGuard::Always,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    }
}

fn build_race_block(
    builder: &mut StateMachineBuilder,
    task: &TaskDeclaration,
    step_name: &str,
    source_state: &State,
    block_index: usize,
    block: &RaceBlock,
    completion_target: Option<State>,
    task_initial_states: &HashMap<String, State>,
    task_defined_steps: &HashMap<String, HashSet<String>>,
    errors: &mut Vec<PlcError>,
    parent_actions: Vec<TransitionAction>,
    wait_ctx: &WaitExpressionContext,
) {
    let decision_state_name = format!("{step_name}__race_{}_decision", block_index + 1);
    let decision_state = builder.add_state(&task.name, &decision_state_name);

    builder.add_transition(
        source_state.clone(),
        decision_state.clone(),
        TransitionGuard::Always,
        parent_actions,
        Vec::new(),
        Vec::new(),
    );

    if block.branches.is_empty() {
        if let Some(target) = completion_target {
            builder.add_transition(
                decision_state,
                target,
                TransitionGuard::Always,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            );
        }
        return;
    }

    let first_branch_state_name = format!("{step_name}__race_{}_branch_1", block_index + 1);
    let first_branch_state = builder.add_state(&task.name, &first_branch_state_name);
    builder.add_transition(
        decision_state,
        first_branch_state,
        TransitionGuard::Always,
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );

    for (branch_index, branch) in block.branches.iter().enumerate() {
        let branch_state_name = format!(
            "{step_name}__race_{}_branch_{}",
            block_index + 1,
            branch_index + 1
        );
        let branch_state = builder.add_state(&task.name, &branch_state_name);

        let analyzed = analyze_statements(&branch.statements, wait_ctx);
        let branch_completion_target = branch
            .then_goto
            .as_ref()
            .and_then(|goto| {
                resolve_task_target(
                    goto,
                    task_initial_states,
                    task_defined_steps,
                    errors,
                    "race then goto",
                )
            })
            .or_else(|| completion_target.clone());

        for goto in &analyzed.gotos {
            if let Some(target) = resolve_task_target(
                goto,
                task_initial_states,
                task_defined_steps,
                errors,
                "goto",
            ) {
                builder.add_transition(
                    branch_state.clone(),
                    target,
                    TransitionGuard::Always,
                    analyzed.actions.clone(),
                    analyzed.effects.clone(),
                    Vec::new(),
                );
            }
        }

        for if_else in &analyzed.if_elses {
            let expr = condition_to_expression(&if_else.condition);

            if let Some(then_target) = resolve_task_target(
                &if_else.then_goto,
                task_initial_states,
                task_defined_steps,
                errors,
                "if/else then goto",
            ) {
                builder.add_transition(
                    branch_state.clone(),
                    then_target,
                    TransitionGuard::Condition {
                        expression: expr.clone(),
                    },
                    analyzed.actions.clone(),
                    analyzed.effects.clone(),
                    Vec::new(),
                );
            }

            if let Some(else_target) = resolve_task_target(
                &if_else.else_goto,
                task_initial_states,
                task_defined_steps,
                errors,
                "if/else else goto",
            ) {
                builder.add_transition(
                    branch_state.clone(),
                    else_target,
                    TransitionGuard::Condition {
                        expression: format!("NOT({expr})"),
                    },
                    analyzed.actions.clone(),
                    analyzed.effects.clone(),
                    Vec::new(),
                );
            }
        }

        for (delay_index, duration_ms) in analyzed.delays_ms.iter().enumerate() {
            if let Some(target) = branch_completion_target.clone() {
                builder.add_transition(
                    branch_state.clone(),
                    target,
                    TransitionGuard::Delay {
                        duration_ms: *duration_ms,
                    },
                    Vec::new(),
                    Vec::new(),
                    vec![TimerOperation {
                        timer_name: format!(
                            "{}.{}.race_{}_branch_{}.delay_{}",
                            task.name,
                            step_name,
                            block_index + 1,
                            branch_index + 1,
                            delay_index + 1
                        ),
                        operation: TimerOperationKind::Start,
                        duration_ms: Some(*duration_ms),
                    }],
                );
            }
        }

        for (timeout_index, timeout) in analyzed.timeouts.iter().enumerate() {
            if let Some(target) = resolve_task_target(
                &timeout.target,
                task_initial_states,
                task_defined_steps,
                errors,
                "timeout -> goto",
            ) {
                let duration_ms = duration_to_ms(timeout);
                builder.add_transition(
                    branch_state.clone(),
                    target,
                    TransitionGuard::Timeout { duration_ms },
                    Vec::new(),
                    Vec::new(),
                    vec![TimerOperation {
                        timer_name: format!(
                            "{}.{}.race_{}_branch_{}.timeout_{}",
                            task.name,
                            step_name,
                            block_index + 1,
                            branch_index + 1,
                            timeout_index + 1
                        ),
                        operation: TimerOperationKind::Start,
                        duration_ms: Some(duration_ms),
                    }],
                );
            }
        }

        for wait_expression in &analyzed.waits {
            if let Some(target) = branch_completion_target.clone() {
                builder.add_transition(
                    branch_state.clone(),
                    target,
                    TransitionGuard::Condition {
                        expression: wait_expression.clone(),
                    },
                    analyzed.actions.clone(),
                    analyzed.effects.clone(),
                    Vec::new(),
                );
            }
        }

        for (nested_parallel_index, nested_parallel) in analyzed.parallel_blocks.iter().enumerate()
        {
            build_parallel_block(
                builder,
                task,
                &format!(
                    "{step_name}__race_{}_branch_{}",
                    block_index + 1,
                    branch_index + 1
                ),
                &branch_state,
                nested_parallel_index,
                nested_parallel,
                branch_completion_target.clone(),
                task_initial_states,
                task_defined_steps,
                errors,
                analyzed.actions.clone(),
                wait_ctx,
            );
        }

        for (nested_race_index, nested_race) in analyzed.race_blocks.iter().enumerate() {
            build_race_block(
                builder,
                task,
                &format!(
                    "{step_name}__race_{}_branch_{}",
                    block_index + 1,
                    branch_index + 1
                ),
                &branch_state,
                nested_race_index,
                nested_race,
                branch_completion_target.clone(),
                task_initial_states,
                task_defined_steps,
                errors,
                analyzed.actions.clone(),
                wait_ctx,
            );
        }

        let has_control_flow = !analyzed.waits.is_empty()
            || !analyzed.delays_ms.is_empty()
            || !analyzed.gotos.is_empty()
            || !analyzed.if_elses.is_empty()
            || !analyzed.parallel_blocks.is_empty()
            || !analyzed.race_blocks.is_empty();
        if !has_control_flow {
            if let Some(target) = branch_completion_target {
                builder.add_transition(
                    branch_state.clone(),
                    target,
                    TransitionGuard::Always,
                    analyzed.actions,
                    analyzed.effects,
                    Vec::new(),
                );
            }
        }

        if branch_index + 1 < block.branches.len() {
            let next_branch_state_name = format!(
                "{step_name}__race_{}_branch_{}",
                block_index + 1,
                branch_index + 2
            );
            let next_branch_state = builder.add_state(&task.name, &next_branch_state_name);
            builder.add_transition(
                branch_state.clone(),
                next_branch_state,
                TransitionGuard::Always,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            );
        }
    }
}

fn resolve_task_target(
    target: &GotoDirective,
    task_initial_states: &HashMap<String, State>,
    task_defined_steps: &HashMap<String, HashSet<String>>,
    errors: &mut Vec<PlcError>,
    source: &str,
) -> Option<State> {
    let line = target.line.max(1);
    let Some(initial_state) = task_initial_states.get(&target.task) else {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            " task",
            &target.task,
            format!("{source} 目标必须是已定义 task 名称"),
        ));
        return None;
    };

    let Some(step) = &target.step else {
        return Some(initial_state.clone());
    };

    let Some(steps) = task_defined_steps.get(&target.task) else {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            " task",
            &target.task,
            format!("{source} 目标必须是已定义 task 名称"),
        ));
        return None;
    };

    if !steps.contains(step) {
        let synthetic_hint = step.contains("__parallel_") || step.contains("__race_");
        if synthetic_hint {
            errors.push(PlcError::semantic(
                line,
                format!(
                    "{source} 不允许跳转到 parallel/race 内部合成 step {}.{step}",
                    target.task
                ),
            ));
        } else {
            errors.push(PlcError::semantic(
                line,
                format!("{source} 引用了未定义 step {}.{step}", target.task),
            ));
        }
        return None;
    }

    Some(State {
        task_name: target.task.clone(),
        step_name: step.clone(),
    })
}

fn action_to_transition_action(action: &ActionStatement) -> Option<TransitionAction> {
    match action {
        ActionStatement::Extend {
            target,
            timeout,
            on_motion_fault,
            on_safety_fault,
        } => Some(TransitionAction::Extend {
            target: target.device.clone(),
            port: target.port.clone(),
            timeout: timeout.as_ref().map(lower_motion_timeout_branch),
            on_motion_fault: on_motion_fault.as_ref().map(lower_motion_fault_branch),
            on_safety_fault: on_safety_fault.as_ref().map(lower_motion_fault_branch),
        }),
        ActionStatement::Retract {
            target,
            timeout,
            on_motion_fault,
            on_safety_fault,
        } => Some(TransitionAction::Retract {
            target: target.device.clone(),
            port: target.port.clone(),
            timeout: timeout.as_ref().map(lower_motion_timeout_branch),
            on_motion_fault: on_motion_fault.as_ref().map(lower_motion_fault_branch),
            on_safety_fault: on_safety_fault.as_ref().map(lower_motion_fault_branch),
        }),
        ActionStatement::Set { target, value } => Some(TransitionAction::Set {
            target: target.device.clone(),
            port: target.port.clone(),
            value: set_enum_to_binary(value)?,
        }),
        ActionStatement::SetAnalog { target, value } => Some(TransitionAction::SetAnalog {
            target: target.device.clone(),
            port: target.port.clone(),
            value_raw: value.to_string(),
        }),
        ActionStatement::SetAnalogExpr { target, expr } => Some(TransitionAction::SetAnalogExpr {
            target: target.device.clone(),
            port: target.port.clone(),
            expr_raw: expression_to_raw(expr),
        }),
        ActionStatement::Compute { target, expr } => Some(TransitionAction::Compute {
            target: target.clone(),
            expr_raw: expression_to_raw(expr),
        }),
        ActionStatement::Call {
            function,
            args,
            binding,
        } => Some(TransitionAction::CallExtern {
            function: function.clone(),
            args_raw: args.iter().map(expression_to_raw).collect(),
            binding: lower_extern_call_binding(binding),
        }),
        ActionStatement::CamEngage { target } => Some(TransitionAction::CamEngage {
            target: target.clone(),
        }),
        ActionStatement::CamDisengage { target } => Some(TransitionAction::CamDisengage {
            target: target.clone(),
        }),
        ActionStatement::CamSwitch { target, new_table } => Some(TransitionAction::CamSwitch {
            target: target.clone(),
            new_table: new_table.clone(),
        }),
        ActionStatement::CamPhase { target, offset } => Some(TransitionAction::CamPhase {
            target: target.clone(),
            offset_expr_raw: expression_to_raw(offset),
        }),
        ActionStatement::AxisMoveRelative {
            target,
            params: _,
            distance,
            speed,
            acceleration,
            deceleration,
            timeout,
            on_reject,
            on_motion_fault,
            on_safety_fault,
            on_reject_routes,
            on_motion_fault_routes,
            on_safety_fault_routes,
            semantic_tag,
        } => Some(TransitionAction::AxisMoveRelative {
            target: target.device.clone(),
            port: target.port.clone(),
            distance_raw: distance.to_string(),
            speed_raw: speed
                .expect("axis.move_relative speed must be resolved in semantic pass")
                .to_string(),
            acceleration_raw: acceleration.map(|value| value.to_string()),
            deceleration_raw: deceleration.map(|value| value.to_string()),
            timeout: lower_axis_timeout_branch(timeout.as_ref()?),
            on_reject: lower_axis_fault_branch(on_reject.as_ref()?, AxisFaultKind::Reject, None),
            on_motion_fault: lower_axis_fault_branch(
                on_motion_fault.as_ref()?,
                AxisFaultKind::Motion,
                None,
            ),
            on_safety_fault: lower_axis_fault_branch(
                on_safety_fault.as_ref()?,
                AxisFaultKind::Safety,
                None,
            ),
            on_reject_routes: lower_axis_fault_routes(on_reject_routes),
            on_motion_fault_routes: lower_axis_fault_routes(on_motion_fault_routes),
            on_safety_fault_routes: lower_axis_fault_routes(on_safety_fault_routes),
            semantic_tag: semantic_tag.clone(),
        }),
        ActionStatement::AxisMoveAbsolute {
            target,
            params: _,
            position,
            speed,
            acceleration,
            deceleration,
            timeout,
            on_reject,
            on_motion_fault,
            on_safety_fault,
            on_reject_routes,
            on_motion_fault_routes,
            on_safety_fault_routes,
            semantic_tag,
        } => Some(TransitionAction::AxisMoveAbsolute {
            target: target.device.clone(),
            port: target.port.clone(),
            position_raw: position.to_string(),
            speed_raw: speed
                .expect("axis.move_absolute speed must be resolved in semantic pass")
                .to_string(),
            acceleration_raw: acceleration.map(|value| value.to_string()),
            deceleration_raw: deceleration.map(|value| value.to_string()),
            require_homed: true,
            timeout: lower_axis_timeout_branch(timeout.as_ref()?),
            on_reject: lower_axis_fault_branch(on_reject.as_ref()?, AxisFaultKind::Reject, None),
            on_motion_fault: lower_axis_fault_branch(
                on_motion_fault.as_ref()?,
                AxisFaultKind::Motion,
                None,
            ),
            on_safety_fault: lower_axis_fault_branch(
                on_safety_fault.as_ref()?,
                AxisFaultKind::Safety,
                None,
            ),
            on_reject_routes: lower_axis_fault_routes(on_reject_routes),
            on_motion_fault_routes: lower_axis_fault_routes(on_motion_fault_routes),
            on_safety_fault_routes: lower_axis_fault_routes(on_safety_fault_routes),
            semantic_tag: semantic_tag.clone(),
        }),
        ActionStatement::Log { message } => Some(TransitionAction::Log {
            message: message.clone(),
        }),
    }
}

fn lower_axis_timeout_branch(timeout: &TimeoutDirective) -> AxisTimeoutBranch {
    AxisTimeoutBranch {
        duration_ms: duration_to_ms(timeout),
        target_task: timeout.target.task.clone(),
        target_step: timeout.target.step.clone(),
    }
}

fn lower_motion_timeout_branch(timeout: &TimeoutDirective) -> IrMotionTimeoutBranch {
    IrMotionTimeoutBranch {
        duration_ms: duration_to_ms(timeout),
        target_task: timeout.target.task.clone(),
        target_step: timeout.target.step.clone(),
    }
}

fn lower_motion_fault_branch(target: &GotoDirective) -> IrMotionFaultBranch {
    IrMotionFaultBranch {
        target_task: target.task.clone(),
        target_step: target.step.clone(),
    }
}

fn lower_axis_fault_branch(
    goto: &GotoDirective,
    kind: AxisFaultKind,
    error_code: Option<&str>,
) -> AxisFaultBranch {
    AxisFaultBranch {
        target_task: goto.task.clone(),
        target_step: goto.step.clone(),
        category: kind.category(),
        vendor_code: kind.vendor_code(),
        kind,
        error_code: error_code.map(ToString::to_string),
    }
}

fn lower_axis_fault_routes(routes: &[AstAxisFaultRouteDirective]) -> Vec<IrAxisFaultRouteBranch> {
    routes
        .iter()
        .map(|route| IrAxisFaultRouteBranch {
            target_task: route.target.task.clone(),
            target_step: route.target.step.clone(),
            kind: route.kind.map(lower_axis_fault_route_kind),
            code: route.code,
        })
        .collect()
}

fn lower_axis_fault_route_kind(kind: AstAxisFaultRouteKind) -> IrAxisFaultRouteKind {
    match kind {
        AstAxisFaultRouteKind::Reject => IrAxisFaultRouteKind::Reject,
        AstAxisFaultRouteKind::Motion => IrAxisFaultRouteKind::Motion,
        AstAxisFaultRouteKind::Safety => IrAxisFaultRouteKind::Safety,
        AstAxisFaultRouteKind::Vendor => IrAxisFaultRouteKind::Vendor,
    }
}

fn lower_extern_call_binding(binding: &AstExternCallBinding) -> IrExternCallBinding {
    match binding {
        AstExternCallBinding::Single(name) => IrExternCallBinding::Single(name.clone()),
        AstExternCallBinding::Tuple(names) => IrExternCallBinding::Tuple(names.clone()),
    }
}

fn expression_to_raw(expr: &AstExpression) -> String {
    match expr {
        AstExpression::Literal(v) => v.to_string(),
        AstExpression::Boolean(v) => v.to_string(),
        AstExpression::Variable(name) => name.clone(),
        AstExpression::UnaryNeg(inner) => format!("-({})", expression_to_raw(inner)),
        AstExpression::UnaryNot(inner) => format!("NOT({})", expression_to_raw(inner)),
        AstExpression::BinaryOp { op, left, right } => format!(
            "({} {} {})",
            expression_to_raw(left),
            binary_operator_to_raw(*op),
            expression_to_raw(right)
        ),
        AstExpression::FunctionCall { name, args } => format!(
            "{}({})",
            name,
            args.iter()
                .map(expression_to_raw)
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

fn binary_operator_to_raw(op: AstBinaryOperator) -> &'static str {
    match op {
        AstBinaryOperator::Add => "+",
        AstBinaryOperator::Sub => "-",
        AstBinaryOperator::Mul => "*",
        AstBinaryOperator::Div => "/",
        AstBinaryOperator::Mod => "%",
        AstBinaryOperator::Eq => "==",
        AstBinaryOperator::Neq => "!=",
        AstBinaryOperator::Gt => ">",
        AstBinaryOperator::Lt => "<",
        AstBinaryOperator::Gte => ">=",
        AstBinaryOperator::Lte => "<=",
        AstBinaryOperator::And => "AND",
        AstBinaryOperator::Or => "OR",
    }
}

fn set_enum_to_binary(value: &str) -> Option<IrBinaryValue> {
    match value {
        "on" | "forward" | "active" => Some(IrBinaryValue::On),
        "off" | "reverse" | "idle" => Some(IrBinaryValue::Off),
        _ => None,
    }
}

fn wait_to_guard_expression(wait: &WaitStatement, wait_ctx: &WaitExpressionContext) -> String {
    wait_condition_to_expression(&wait.condition, wait_ctx)
}

fn wait_condition_to_expression(
    condition: &WaitCondition,
    wait_ctx: &WaitExpressionContext,
) -> String {
    match condition {
        WaitCondition::Single(single) => wait_term_to_expression(single, wait_ctx),
        WaitCondition::And(conditions) => conditions
            .iter()
            .map(|condition| wait_term_to_expression(condition, wait_ctx))
            .collect::<Vec<_>>()
            .join(" AND "),
        WaitCondition::Or(conditions) => conditions
            .iter()
            .map(|condition| wait_term_to_expression(condition, wait_ctx))
            .collect::<Vec<_>>()
            .join(" OR "),
    }
}

fn analog_region_state_name(index: usize) -> String {
    format!("region_{index}")
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

fn comparison_op_from_ast(op: &ComparisonOperator) -> ComparisonOp {
    match op {
        ComparisonOperator::Eq => ComparisonOp::Eq,
        ComparisonOperator::Neq => ComparisonOp::Neq,
        ComparisonOperator::Gt => ComparisonOp::Gt,
        ComparisonOperator::Lt => ComparisonOp::Lt,
        ComparisonOperator::Gte => ComparisonOp::Gte,
        ComparisonOperator::Lte => ComparisonOp::Lte,
    }
}

fn region_intersects(op: ComparisonOp, value: f64, min: f64, max: f64) -> bool {
    match op {
        ComparisonOp::Eq => value >= min && value <= max,
        ComparisonOp::Neq => !(min == max && value == min),
        ComparisonOp::Gt => max > value,
        // For analog waits we need the selected region set to be a *sufficient* condition
        // (otherwise a wait may be satisfied even when the numeric predicate is false).
        //
        // Using intersection semantics for / becomes a tautology when regions overlap
        // at the split point (e.g. [0..T] and [T..MAX]), because both regions intersect.
        // So for non-strict comparisons we pick regions that are entirely within the predicate.
        ComparisonOp::Gte => min >= value,
        ComparisonOp::Lt => min < value,
        ComparisonOp::Lte => max <= value,
    }
}

fn wait_term_to_expression(
    condition: &ConditionExpression,
    wait_ctx: &WaitExpressionContext,
) -> String {
    if condition.is_expression_compare() {
        return condition_to_expression(condition);
    }

    if let Some((value, _unit)) = threshold_literal_value_and_unit(&condition.right) {
        if let Some(device_name) = wait_operand_device_name(&condition.left) {
            if let Some(regions) = wait_ctx.analog_input_regions.get(device_name) {
                let op = comparison_op_from_ast(&condition.operator);
                let mut matching = Vec::new();

                for (index, (min, max)) in regions.iter().enumerate() {
                    if region_intersects(op, value, *min, *max) {
                        matching.push(index);
                    }
                }

                if !matching.is_empty() {
                    let rendered = matching
                        .into_iter()
                        .map(analog_region_state_name)
                        .collect::<Vec<_>>()
                        .join(", ");
                    return format!("{device_name} in {{{rendered}}}");
                }
            }
        }
    }

    condition_to_expression(condition)
}

fn condition_to_expression(condition: &ConditionExpression) -> String {
    if let Some((left, right)) = condition.expression_pair() {
        return format!(
            "{} {} {}",
            expression_to_raw(left),
            match condition.operator {
                ComparisonOperator::Eq => "==",
                ComparisonOperator::Neq => "!=",
                ComparisonOperator::Gt => ">",
                ComparisonOperator::Lt => "<",
                ComparisonOperator::Gte => ">=",
                ComparisonOperator::Lte => "<=",
            },
            expression_to_raw(right)
        );
    }

    let operator = match condition.operator {
        ComparisonOperator::Eq => "==",
        ComparisonOperator::Neq => "!=",
        ComparisonOperator::Gt => ">",
        ComparisonOperator::Lt => "<",
        ComparisonOperator::Gte => ">=",
        ComparisonOperator::Lte => "<=",
    };

    format!(
        "{} {} {}",
        condition.left,
        operator,
        literal_to_expression(&condition.right)
    )
}

fn literal_to_expression(literal: &LiteralValue) -> String {
    match literal {
        LiteralValue::Boolean(value) => value.to_string(),
        LiteralValue::Number(value) => value.to_string(),
        LiteralValue::Measured(measured) => format!("{}{}", measured.value, measured.unit),
        LiteralValue::String(value) => format!("\"{}\"", value),
        LiteralValue::State(state) => format!("{}.{}", state.device, state.state),
    }
}

fn threshold_literal_value_and_unit(literal: &LiteralValue) -> Option<(f64, Option<&str>)> {
    match literal {
        LiteralValue::Number(value) => Some((*value, None)),
        LiteralValue::Measured(measured) => Some((measured.value, Some(measured.unit.as_str()))),
        LiteralValue::Boolean(_) | LiteralValue::String(_) | LiteralValue::State(_) => None,
    }
}

fn duration_to_ms(timeout: &TimeoutDirective) -> u64 {
    duration_value_to_ms(&timeout.duration)
}

fn duration_value_to_ms(duration: &DurationValue) -> u64 {
    match duration.unit {
        TimeUnit::Ms => duration.value,
        TimeUnit::S => duration.value.saturating_mul(1000),
    }
}

fn ast_type_to_ir_kind(device_type: &DeviceType) -> DeviceKind {
    match device_type {
        DeviceType::DigitalOutput => DeviceKind::DigitalOutput,
        DeviceType::DigitalInput => DeviceKind::DigitalInput,
        DeviceType::Plc => DeviceKind::Plc,
        DeviceType::SolenoidValve => DeviceKind::SolenoidValve,
        DeviceType::Cylinder => DeviceKind::Cylinder,
        DeviceType::Sensor => DeviceKind::Sensor,
        DeviceType::Motor => DeviceKind::Motor,
        DeviceType::StepperMotor => DeviceKind::StepperMotor,
        DeviceType::Vfd => DeviceKind::Vfd,
        DeviceType::ServoDrive => DeviceKind::ServoDrive,
        DeviceType::CamCoupling => DeviceKind::CamCoupling,
        DeviceType::AnalogInput => DeviceKind::AnalogInput,
        DeviceType::AnalogOutput => DeviceKind::AnalogOutput,
        DeviceType::Pid => DeviceKind::Pid,
        DeviceType::ProportionalValve => DeviceKind::ProportionalValve,
        DeviceType::Gripper => DeviceKind::Gripper,
        DeviceType::Conveyor => DeviceKind::Conveyor,
        DeviceType::Pump => DeviceKind::Pump,
        DeviceType::Heater => DeviceKind::Heater,
        DeviceType::VisionSensor => DeviceKind::VisionSensor,
    }
}

fn connection_type_for_relation(
    relation: &TopologyRelation,
    from: &DeviceKind,
    to: &DeviceKind,
) -> Option<ConnectionType> {
    match relation {
        TopologyRelation::DrivenBy => driven_by_connection_type_for(from, to),
        TopologyRelation::ReportsTo => reports_to_connection_type_for(from, to),
        TopologyRelation::Detects => detects_connection_type_for(from, to),
    }
}

fn driven_by_connection_type_for(from: &DeviceKind, to: &DeviceKind) -> Option<ConnectionType> {
    match (from, to) {
        (DeviceKind::DigitalOutput, DeviceKind::SolenoidValve)
        | (DeviceKind::DigitalOutput, DeviceKind::Motor)
        | (DeviceKind::DigitalOutput, DeviceKind::StepperMotor)
        | (DeviceKind::DigitalOutput, DeviceKind::Vfd)
        | (DeviceKind::DigitalOutput, DeviceKind::ServoDrive)
        | (DeviceKind::DigitalOutput, DeviceKind::CamCoupling) => Some(ConnectionType::Electrical),
        (DeviceKind::SolenoidValve, DeviceKind::Cylinder) => Some(ConnectionType::Pneumatic),
        (DeviceKind::AnalogOutput, DeviceKind::Motor)
        | (DeviceKind::AnalogOutput, DeviceKind::Vfd) => Some(ConnectionType::Analog),
        (DeviceKind::AnalogOutput, DeviceKind::CamCoupling) => Some(ConnectionType::Analog),
        _ => None,
    }
}

fn reports_to_connection_type_for(from: &DeviceKind, to: &DeviceKind) -> Option<ConnectionType> {
    match (from, to) {
        (DeviceKind::Sensor, DeviceKind::DigitalInput) => Some(ConnectionType::Logical),
        (DeviceKind::Sensor, DeviceKind::AnalogInput) => Some(ConnectionType::Analog),
        _ => None,
    }
}

fn detects_connection_type_for(from: &DeviceKind, to: &DeviceKind) -> Option<ConnectionType> {
    match (from, to) {
        (DeviceKind::Cylinder, DeviceKind::Sensor)
        | (DeviceKind::Motor, DeviceKind::Sensor)
        | (DeviceKind::StepperMotor, DeviceKind::Sensor)
        | (DeviceKind::Vfd, DeviceKind::Sensor)
        | (DeviceKind::ServoDrive, DeviceKind::Sensor)
        | (DeviceKind::CamCoupling, DeviceKind::Sensor)
        | (DeviceKind::SolenoidValve, DeviceKind::Sensor) => Some(ConnectionType::Logical),
        _ => None,
    }
}

fn device_kind_name(kind: &DeviceKind) -> &'static str {
    match kind {
        DeviceKind::DigitalOutput => "digital_output",
        DeviceKind::DigitalInput => "digital_input",
        DeviceKind::Plc => "plc",
        DeviceKind::SolenoidValve => "solenoid_valve",
        DeviceKind::Cylinder => "cylinder",
        DeviceKind::Sensor => "sensor",
        DeviceKind::Motor => "motor",
        DeviceKind::StepperMotor => "stepper_motor",
        DeviceKind::Vfd => "vfd",
        DeviceKind::ServoDrive => "servo_drive",
        DeviceKind::CamCoupling => "cam_coupling",
        DeviceKind::AnalogInput => "analog_input",
        DeviceKind::AnalogOutput => "analog_output",
        DeviceKind::Pid => "pid",
        DeviceKind::ProportionalValve => "proportional_valve",
        DeviceKind::Gripper => "gripper",
        DeviceKind::Conveyor => "conveyor",
        DeviceKind::Pump => "pump",
        DeviceKind::Heater => "heater",
        DeviceKind::VisionSensor => "vision_sensor",
    }
}
