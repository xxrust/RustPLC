fn collect_device_timing_profiles(
    topology: &TopologySection,
) -> HashMap<String, DeviceTimingProfile> {
    topology
        .devices
        .iter()
        .map(|device| {
            (
                device.name.clone(),
                DeviceTimingProfile {
                    response_ms: device
                        .attributes
                        .response_time
                        .as_ref()
                        .map(duration_value_to_ms),
                    stroke_ms: device
                        .attributes
                        .stroke_time
                        .as_ref()
                        .map(duration_value_to_ms),
                    retract_ms: device
                        .attributes
                        .retract_time
                        .as_ref()
                        .map(duration_value_to_ms),
                    ramp_ms: device
                        .attributes
                        .ramp_time
                        .as_ref()
                        .map(duration_value_to_ms),
                },
            )
        })
        .collect()
}

fn collect_actions(statements: &[StepStatement], actions: &mut Vec<ActionStatement>) {
    for statement in statements {
        match statement {
            StepStatement::Action(action) => actions.push(action.clone()),
            StepStatement::IfElse { .. } => {}
            StepStatement::Repeat { body, .. } => collect_actions(body, actions),
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    collect_actions(&branch.statements, actions);
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    collect_actions(&branch.statements, actions);
                }
            }
            StepStatement::Wait(_)
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_)
            | StepStatement::Effect(_) => {}
        }
    }
}

fn action_to_timing(
    task_name: &str,
    step_name: &str,
    line: usize,
    action: &ActionStatement,
    profiles: &HashMap<String, DeviceTimingProfile>,
    errors: &mut Vec<PlcError>,
) -> Option<ActionTiming> {
    let (action_kind, target) = match action {
        ActionStatement::Extend { target, .. } => {
            (ActionKind::Extend, Some(target.device.as_str()))
        }
        ActionStatement::Retract { target, .. } => {
            (ActionKind::Retract, Some(target.device.as_str()))
        }
        ActionStatement::Set { target, .. } => (ActionKind::Set, Some(target.device.as_str())),
        ActionStatement::SetAnalog { target, .. } => {
            (ActionKind::SetAnalog, Some(target.device.as_str()))
        }
        ActionStatement::SetAnalogExpr { target, .. } => {
            (ActionKind::SetAnalogExpr, Some(target.device.as_str()))
        }
        ActionStatement::AxisMoveRelative { target, .. } => {
            (ActionKind::AxisMoveRelative, Some(target.device.as_str()))
        }
        ActionStatement::AxisMoveAbsolute { target, .. } => {
            (ActionKind::AxisMoveAbsolute, Some(target.device.as_str()))
        }
        ActionStatement::Compute { .. } => (ActionKind::Compute, None),
        ActionStatement::Call { .. } => return None,
        ActionStatement::CamEngage { target } => (ActionKind::CamEngage, Some(target.as_str())),
        ActionStatement::CamDisengage { target } => {
            (ActionKind::CamDisengage, Some(target.as_str()))
        }
        ActionStatement::CamSwitch { target, .. } => (ActionKind::CamSwitch, Some(target.as_str())),
        ActionStatement::CamPhase { target, .. } => (ActionKind::CamPhase, Some(target.as_str())),
        ActionStatement::Log { .. } => (ActionKind::Log, None),
    };

    let Some(target) = target else {
        return None;
    };

    let Some(profile) = profiles.get(target) else {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            "设备",
            target,
            "请先在 [topology] 段定义该设备并补充物理参数".to_string(),
        ));
        return None;
    };

    let duration_ms = match action_kind {
        ActionKind::Extend => profile
            .stroke_ms
            .or(profile.response_ms)
            .or(profile.ramp_ms),
        ActionKind::Retract => profile
            .retract_ms
            .or(profile.response_ms)
            .or(profile.ramp_ms),
        ActionKind::Set | ActionKind::SetAnalog | ActionKind::SetAnalogExpr => {
            profile.ramp_ms.or(profile.response_ms)
        }
        ActionKind::CamEngage
        | ActionKind::CamDisengage
        | ActionKind::CamSwitch
        | ActionKind::CamPhase
        | ActionKind::AxisMoveRelative
        | ActionKind::AxisMoveAbsolute
        | ActionKind::CallExtern
        | ActionKind::Compute
        | ActionKind::Log => None,
    }?;

    Some(ActionTiming {
        action: ActionRef {
            task_name: task_name.to_string(),
            step_name: step_name.to_string(),
            action_kind,
            target: Some(target.to_string()),
        },
        interval: TimeInterval {
            min_ms: duration_ms,
            max_ms: duration_ms,
        },
    })
}

fn insert_action_timing(intervals: &mut BTreeMap<String, ActionTiming>, timing: ActionTiming) {
    let action_name = action_kind_name(&timing.action.action_kind);
    let target = timing.action.target.as_deref().unwrap_or("_");
    let base_key = format!(
        "{}.{}.{}.{}",
        timing.action.task_name, timing.action.step_name, action_name, target
    );

    if !intervals.contains_key(&base_key) {
        intervals.insert(base_key, timing);
        return;
    }

    let mut duplicate_index = 2usize;
    loop {
        let key = format!("{base_key}.{duplicate_index}");
        if !intervals.contains_key(&key) {
            intervals.insert(key, timing);
            return;
        }
        duplicate_index += 1;
    }
}

fn action_kind_name(action_kind: &ActionKind) -> &'static str {
    match action_kind {
        ActionKind::Extend => "extend",
        ActionKind::Retract => "retract",
        ActionKind::Set => "set",
        ActionKind::SetAnalog => "set_analog",
        ActionKind::SetAnalogExpr => "set_analog_expr",
        ActionKind::Compute => "compute",
        ActionKind::CallExtern => "call_extern",
        ActionKind::CamEngage => "cam_engage",
        ActionKind::CamDisengage => "cam_disengage",
        ActionKind::CamSwitch => "cam_switch",
        ActionKind::CamPhase => "cam_phase",
        ActionKind::AxisMoveRelative => "axis_move_relative",
        ActionKind::AxisMoveAbsolute => "axis_move_absolute",
        ActionKind::Log => "log",
    }
}

fn default_states_for_kind(kind: &DeviceKind) -> &'static [&'static str] {
    match kind {
        DeviceKind::Cylinder => &["extended", "retracted"],
        DeviceKind::DigitalOutput
        | DeviceKind::DigitalInput
        | DeviceKind::SolenoidValve
        | DeviceKind::Sensor
        | DeviceKind::Motor
        | DeviceKind::StepperMotor
        | DeviceKind::Vfd
        | DeviceKind::ServoDrive
        | DeviceKind::CamCoupling => &["on", "off", "forward", "reverse", "active", "idle"],
        DeviceKind::AnalogInput | DeviceKind::AnalogOutput | DeviceKind::Pid | DeviceKind::Plc => {
            &[]
        }
    }
}

fn completion_target_for_step(
    task: &TaskDeclaration,
    step_index: usize,
    task_on_complete_targets: &HashMap<String, Option<State>>,
) -> Option<State> {
    if step_index + 1 < task.steps.len() {
        return Some(State {
            task_name: task.name.clone(),
            step_name: task.steps[step_index + 1].name.clone(),
        });
    }

    task_on_complete_targets
        .get(&task.name)
        .cloned()
        .unwrap_or(None)
}

fn analyze_statements(
    statements: &[StepStatement],
    wait_ctx: &WaitExpressionContext,
) -> AnalyzedStatements {
    let mut analyzed = AnalyzedStatements::default();

    for statement in statements {
        match statement {
            StepStatement::Action(action) => {
                if let Some(mapped) = action_to_transition_action(action) {
                    analyzed.actions.push(mapped);
                }
            }
            StepStatement::Effect(effect) => {
                analyzed.effects.push(effect_to_transition_effect(effect));
            }
            StepStatement::Wait(wait) => {
                analyzed
                    .waits
                    .push(wait_to_guard_expression(wait, wait_ctx));
            }
            StepStatement::IfElse {
                condition,
                then_goto,
                else_goto,
            } => analyzed.if_elses.push(IfElseSpec {
                condition: condition.clone(),
                then_goto: then_goto.clone(),
                else_goto: else_goto.clone(),
            }),
            StepStatement::Delay { duration_ms } => analyzed.delays_ms.push(*duration_ms),
            StepStatement::Repeat { .. } => {}
            StepStatement::Timeout(timeout) => analyzed.timeouts.push(timeout.clone()),
            StepStatement::Goto(goto) => analyzed.gotos.push(goto.clone()),
            StepStatement::Parallel(block) => analyzed.parallel_blocks.push(block.clone()),
            StepStatement::Race(block) => analyzed.race_blocks.push(block.clone()),
            StepStatement::AllowIndefiniteWait(_) => {}
        }
    }

    analyzed
}

fn effect_to_transition_effect(effect: &AstEffectStatement) -> IrWorkpieceEffect {
    match &effect.kind {
        AstEffectKind::Acquire { holder, from } => IrWorkpieceEffect::Acquire {
            holder: holder.clone(),
            from: from.clone(),
        },
        AstEffectKind::Transfer { from, to } => IrWorkpieceEffect::Transfer {
            from: from.clone(),
            to: to.clone(),
        },
        AstEffectKind::Finish { at, terminal_state } => IrWorkpieceEffect::Finish {
            at: at.clone(),
            terminal_state: terminal_state.clone(),
        },
        AstEffectKind::Mount {
            workpiece_type,
            slot,
        } => IrWorkpieceEffect::Mount {
            workpiece_type: workpiece_type.clone(),
            slot: slot.clone(),
        },
        AstEffectKind::Unmount {
            workpiece_type,
            slot,
            to,
        } => IrWorkpieceEffect::Unmount {
            workpiece_type: workpiece_type.clone(),
            slot: slot.clone(),
            to: to.clone(),
        },
        AstEffectKind::Split {
            source_type,
            target_type,
            count,
            consumed,
        } => IrWorkpieceEffect::Split {
            source_type: source_type.clone(),
            target_type: target_type.clone(),
            count: *count,
            consumed: *consumed,
        },
        AstEffectKind::Merge {
            inputs,
            target_type,
            consumed_inputs,
        } => IrWorkpieceEffect::Merge {
            inputs: inputs.clone(),
            target_type: target_type.clone(),
            consumed_inputs: *consumed_inputs,
        },
        AstEffectKind::TransformCarrier { carrier, frame } => IrWorkpieceEffect::TransformCarrier {
            carrier: carrier.clone(),
            frame: frame.clone(),
        },
    }
}

fn annotate_axis_absolute_homing_guards(state_machine: &mut StateMachine) {
    let reachable = reachable_states(state_machine);
    let axis_targets = collect_axis_targets(state_machine);
    if axis_targets.is_empty() {
        return;
    }

    let mut homed_facts = HashMap::<String, HashMap<(String, String), bool>>::new();
    for axis in &axis_targets {
        homed_facts.insert(
            axis.clone(),
            infer_definitely_homed_states(state_machine, &reachable, axis),
        );
    }

    for transition in &mut state_machine.transitions {
        let mut local_homed = HashMap::<String, bool>::new();
        for axis in &axis_targets {
            let homed = homed_facts
                .get(axis)
                .and_then(|facts| facts.get(&state_key(&transition.from)))
                .copied()
                .unwrap_or(false);
            local_homed.insert(axis.clone(), homed);
        }

        for action in &mut transition.actions {
            match action {
                TransitionAction::AxisMoveRelative { target, .. } => {
                    local_homed.insert(target.clone(), true);
                }
                TransitionAction::AxisMoveAbsolute {
                    target,
                    require_homed,
                    ..
                } => {
                    let proven = local_homed.get(target).copied().unwrap_or(false);
                    *require_homed = !proven;
                }
                _ => {}
            }
        }
    }
}

fn state_key(state: &State) -> (String, String) {
    (state.task_name.clone(), state.step_name.clone())
}

fn collect_axis_targets(state_machine: &StateMachine) -> BTreeSet<String> {
    let mut axis_targets = BTreeSet::new();
    for transition in &state_machine.transitions {
        for action in &transition.actions {
            match action {
                TransitionAction::AxisMoveRelative { target, .. }
                | TransitionAction::AxisMoveAbsolute { target, .. } => {
                    axis_targets.insert(target.clone());
                }
                _ => {}
            }
        }
    }
    axis_targets
}

fn reachable_states(state_machine: &StateMachine) -> HashSet<(String, String)> {
    let mut reachable = HashSet::new();
    let mut stack = vec![state_machine.initial.clone()];

    while let Some(state) = stack.pop() {
        let key = state_key(&state);
        if !reachable.insert(key) {
            continue;
        }

        for transition in state_machine.transitions.iter().filter(|t| t.from == state) {
            stack.push(transition.to.clone());
        }
    }

    reachable
}

fn infer_definitely_homed_states(
    state_machine: &StateMachine,
    reachable: &HashSet<(String, String)>,
    axis: &str,
) -> HashMap<(String, String), bool> {
    let mut facts = HashMap::<(String, String), bool>::new();
    for key in reachable {
        facts.insert(key.clone(), true);
    }
    let initial_key = state_key(&state_machine.initial);
    facts.insert(initial_key.clone(), false);

    let mut changed = true;
    while changed {
        changed = false;
        for state in reachable {
            if *state == initial_key {
                continue;
            }

            let predecessors = state_machine
                .transitions
                .iter()
                .filter(|transition| {
                    state_key(&transition.to) == *state
                        && reachable.contains(&state_key(&transition.from))
                })
                .collect::<Vec<_>>();

            if predecessors.is_empty() {
                continue;
            }

            let next = predecessors.iter().all(|transition| {
                let in_homed = facts
                    .get(&state_key(&transition.from))
                    .copied()
                    .unwrap_or(false);
                apply_homing_transition_effect(in_homed, &transition.actions, axis)
            });

            if let Some(entry) = facts.get_mut(state) {
                if *entry != next {
                    *entry = next;
                    changed = true;
                }
            }
        }
    }

    facts
}

fn apply_homing_transition_effect(
    in_homed: bool,
    actions: &[TransitionAction],
    axis: &str,
) -> bool {
    let mut homed = in_homed;
    for action in actions {
        if let TransitionAction::AxisMoveRelative { target, .. } = action {
            if target == axis {
                homed = true;
            }
        }
    }
    homed
}

