pub fn build_state_machine_from_ast(tasks: &TasksSection) -> Result<StateMachine, Vec<PlcError>> {
    build_state_machine_from_ast_with_context(tasks, &WaitExpressionContext::default(), None)
}

fn build_state_machine_from_ast_with_context(
    tasks: &TasksSection,
    wait_ctx: &WaitExpressionContext,
    device_kinds: Option<&HashMap<String, DeviceKind>>,
) -> Result<StateMachine, Vec<PlcError>> {
    let mut builder = StateMachineBuilder::default();
    let mut errors = Vec::new();

    if tasks.tasks.is_empty() {
        errors.push(PlcError::semantic(1, "[tasks] 段至少需要一个 task"));
        return Err(errors);
    }

    let mut task_initial_states = HashMap::<String, State>::new();

    for task in &tasks.tasks {
        if task.steps.is_empty() {
            errors.push(PlcError::semantic(
                task.line,
                format!("task {} 至少需要一个 step", task.name),
            ));
            continue;
        }

        let initial_state = State {
            task_name: task.name.clone(),
            step_name: task.steps[0].name.clone(),
        };

        if task_initial_states
            .insert(task.name.clone(), initial_state)
            .is_some()
        {
            errors.push(PlcError::duplicate_definition_with_reason(
                task.line,
                "task",
                &task.name,
                "请确保每个 task 名称唯一",
            ));
        }

        for step in &task.steps {
            builder.add_state(&task.name, &step.name);
        }
    }

    let Some(initial) = tasks.tasks.iter().find_map(|task| {
        task.steps.first().map(|step| State {
            task_name: task.name.clone(),
            step_name: step.name.clone(),
        })
    }) else {
        errors.push(PlcError::semantic(1, "未找到可执行的 task/step 初始状态"));
        return Err(errors);
    };

    let task_defined_steps = collect_task_steps(tasks);

    let mut task_on_complete_targets = HashMap::<String, Option<State>>::new();
    for task in &tasks.tasks {
        let on_complete_target = match &task.on_complete {
            Some(OnCompleteDirective::Goto { target }) => resolve_task_target(
                target,
                &task_initial_states,
                &task_defined_steps,
                &mut errors,
                "on_complete",
            ),
            _ => None,
        };
        task_on_complete_targets.insert(task.name.clone(), on_complete_target);
    }

    for task in &tasks.tasks {
        for (step_index, step) in task.steps.iter().enumerate() {
            validate_set_enum_values(&step.statements, step.line.max(1), &mut errors);
            if let Some(device_kinds) = device_kinds {
                device_semantics::motor::validate_legacy_set_actions(
                    &step.statements,
                    step.line.max(1),
                    device_kinds,
                    &mut errors,
                );
            }
            let from_state = State {
                task_name: task.name.clone(),
                step_name: step.name.clone(),
            };
            let completion_target =
                completion_target_for_step(task, step_index, &task_on_complete_targets);

            let analyzed = analyze_statements(&step.statements, wait_ctx);

            for (block_index, block) in analyzed.parallel_blocks.iter().enumerate() {
                build_parallel_block(
                    &mut builder,
                    task,
                    &step.name,
                    &from_state,
                    block_index,
                    block,
                    completion_target.clone(),
                    &task_initial_states,
                    &task_defined_steps,
                    &mut errors,
                    analyzed.actions.clone(),
                    wait_ctx,
                );
            }

            for (block_index, block) in analyzed.race_blocks.iter().enumerate() {
                build_race_block(
                    &mut builder,
                    task,
                    &step.name,
                    &from_state,
                    block_index,
                    block,
                    completion_target.clone(),
                    &task_initial_states,
                    &task_defined_steps,
                    &mut errors,
                    analyzed.actions.clone(),
                    wait_ctx,
                );
            }

            for goto in &analyzed.gotos {
                if let Some(target) = resolve_task_target(
                    goto,
                    &task_initial_states,
                    &task_defined_steps,
                    &mut errors,
                    "goto",
                ) {
                    builder.add_transition(
                        from_state.clone(),
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
                    &task_initial_states,
                    &task_defined_steps,
                    &mut errors,
                    "if/else then goto",
                ) {
                    builder.add_transition(
                        from_state.clone(),
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
                    &task_initial_states,
                    &task_defined_steps,
                    &mut errors,
                    "if/else else goto",
                ) {
                    builder.add_transition(
                        from_state.clone(),
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
                if let Some(target) = completion_target.clone() {
                    builder.add_transition(
                        from_state.clone(),
                        target,
                        TransitionGuard::Delay {
                            duration_ms: *duration_ms,
                        },
                        Vec::new(),
                        Vec::new(),
                        vec![TimerOperation {
                            timer_name: format!(
                                "{}.{}.delay_{}",
                                task.name,
                                step.name,
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
                    &task_initial_states,
                    &task_defined_steps,
                    &mut errors,
                    "timeout -> goto",
                ) {
                    let duration_ms = duration_to_ms(timeout);
                    builder.add_transition(
                        from_state.clone(),
                        target,
                        TransitionGuard::Timeout { duration_ms },
                        Vec::new(),
                        Vec::new(),
                        vec![TimerOperation {
                            timer_name: format!(
                                "{}.{}.timeout_{}",
                                task.name,
                                step.name,
                                timeout_index + 1
                            ),
                            operation: TimerOperationKind::Start,
                            duration_ms: Some(duration_ms),
                        }],
                    );
                }
            }

            for wait_expression in &analyzed.waits {
                if let Some(target) = completion_target.clone() {
                    builder.add_transition(
                        from_state.clone(),
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

            let has_control_flow = !analyzed.waits.is_empty()
                || !analyzed.delays_ms.is_empty()
                || !analyzed.gotos.is_empty()
                || !analyzed.if_elses.is_empty()
                || !analyzed.parallel_blocks.is_empty()
                || !analyzed.race_blocks.is_empty();
            if !has_control_flow {
                if let Some(target) = completion_target {
                    builder.add_transition(
                        from_state,
                        target,
                        TransitionGuard::Always,
                        analyzed.actions,
                        analyzed.effects,
                        Vec::new(),
                    );
                }
            }
        }
    }

    if errors.is_empty() {
        let analog_regions = wait_ctx
            .analog_input_regions
            .iter()
            .map(|(device, regions)| {
                (
                    device.clone(),
                    regions
                        .iter()
                        .map(|(min, max)| {
                            (format_numeric_literal(*min), format_numeric_literal(*max))
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let task_contexts = build_task_execution_contexts(tasks, &builder.transitions);
        let mut state_machine = StateMachine {
            states: builder.states,
            transitions: builder.transitions,
            initial,
            analog_regions,
            task_contexts,
        };
        annotate_axis_absolute_homing_guards(&mut state_machine);
        Ok(state_machine)
    } else {
        Err(errors)
    }
}

fn format_numeric_literal(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.1}")
    } else {
        v.to_string()
    }
}

fn format_numeric_literal_from_literal(literal: &LiteralValue) -> Option<String> {
    match literal {
        LiteralValue::Number(v) => Some(format_numeric_literal(*v)),
        LiteralValue::Measured(measured) => Some(format_numeric_literal(measured.value)),
        LiteralValue::Boolean(_) | LiteralValue::String(_) | LiteralValue::State(_) => None,
    }
}

#[derive(Debug, Clone, Default)]
struct StateMachineBuilder {
    states: Vec<State>,
    transitions: Vec<Transition>,
    seen_states: HashSet<(String, String)>,
}

impl StateMachineBuilder {
    fn add_state(&mut self, task_name: &str, step_name: &str) -> State {
        let key = (task_name.to_string(), step_name.to_string());
        if self.seen_states.insert(key.clone()) {
            self.states.push(State {
                task_name: key.0.clone(),
                step_name: key.1.clone(),
            });
        }

        State {
            task_name: key.0,
            step_name: key.1,
        }
    }

    fn add_transition(
        &mut self,
        from: State,
        to: State,
        guard: TransitionGuard,
        actions: Vec<TransitionAction>,
        effects: Vec<crate::ir::WorkpieceEffect>,
        timers: Vec<TimerOperation>,
    ) {
        self.transitions.push(Transition {
            from,
            to,
            guard,
            actions,
            effects,
            timers,
        });
    }
}

fn build_task_execution_contexts(
    tasks: &TasksSection,
    transitions: &[Transition],
) -> Vec<TaskExecutionContext> {
    let mut timers_by_task = HashMap::<String, Vec<TaskTimerContext>>::new();
    let mut pending_actions_by_task = HashMap::<String, Vec<IrPendingActionContext>>::new();
    let mut seen_timers = HashSet::<(String, String, String, Option<u64>)>::new();
    let mut seen_pending =
        HashSet::<(String, String, String, Option<String>, Option<String>)>::new();

    for transition in transitions {
        let task_name = transition.from.task_name.clone();
        let source_state = transition.from.clone();

        for timer in &transition.timers {
            let key = (
                task_name.clone(),
                source_state.step_name.clone(),
                timer.timer_name.clone(),
                timer.duration_ms,
            );
            if seen_timers.insert(key) {
                timers_by_task
                    .entry(task_name.clone())
                    .or_default()
                    .push(TaskTimerContext {
                        timer_name: timer.timer_name.clone(),
                        source_state: source_state.clone(),
                        duration_ms: timer.duration_ms,
                        active: false,
                    });
            }
        }
    }

    for task in &tasks.tasks {
        for step in &task.steps {
            let source_state = State {
                task_name: task.name.clone(),
                step_name: step.name.clone(),
            };
            let mut actions = Vec::new();
            collect_actions(&step.statements, &mut actions);

            for action in actions {
                let Some((action_kind, target, semantic_tag)) =
                    pending_action_descriptor_from_statement(&action)
                else {
                    continue;
                };
                let pending_key = (
                    task.name.clone(),
                    step.name.clone(),
                    action_kind_name(&action_kind).to_string(),
                    target.clone(),
                    semantic_tag.clone(),
                );
                if seen_pending.insert(pending_key) {
                    pending_actions_by_task
                        .entry(task.name.clone())
                        .or_default()
                        .push(IrPendingActionContext {
                            source_state: source_state.clone(),
                            action_kind,
                            target,
                            semantic_tag,
                            active: false,
                        });
                }
            }
        }
    }

    tasks
        .tasks
        .iter()
        .filter_map(|task| {
            let entry_step = task.steps.first()?;
            let entry_state = State {
                task_name: task.name.clone(),
                step_name: entry_step.name.clone(),
            };
            Some(TaskExecutionContext {
                task_name: task.name.clone(),
                entry_state: entry_state.clone(),
                current_state: entry_state,
                blocking_state: TaskBlockingState::Ready,
                timers: timers_by_task.remove(&task.name).unwrap_or_default(),
                pending_actions: pending_actions_by_task
                    .remove(&task.name)
                    .unwrap_or_default(),
            })
        })
        .collect()
}

fn pending_action_descriptor_from_statement(
    action: &ActionStatement,
) -> Option<(ActionKind, Option<String>, Option<String>)> {
    match action {
        ActionStatement::AxisMoveRelative {
            target,
            semantic_tag,
            ..
        } => Some((
            ActionKind::AxisMoveRelative,
            Some(target.device.clone()),
            semantic_tag.clone(),
        )),
        ActionStatement::AxisMoveAbsolute {
            target,
            semantic_tag,
            ..
        } => Some((
            ActionKind::AxisMoveAbsolute,
            Some(target.device.clone()),
            semantic_tag.clone(),
        )),
        ActionStatement::Call { function, .. } => {
            Some((ActionKind::CallExtern, Some(function.clone()), None))
        }
        ActionStatement::Extend { .. }
        | ActionStatement::Retract { .. }
        | ActionStatement::Set { .. }
        | ActionStatement::SetAnalog { .. }
        | ActionStatement::SetAnalogExpr { .. }
        | ActionStatement::Compute { .. }
        | ActionStatement::CamEngage { .. }
        | ActionStatement::CamDisengage { .. }
        | ActionStatement::CamSwitch { .. }
        | ActionStatement::CamPhase { .. }
        | ActionStatement::Log { .. } => None,
    }
}

