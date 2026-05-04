#[derive(Debug, Clone, Copy, Default)]
struct BrakeSequenceProgress {
    engage_seen: bool,
    confirm_seen: bool,
}

fn verify_vertical_axis_brake_sequence(program: &PlcProgram) -> Vec<SafetyDiagnostic> {
    let disable_targets = collect_axis_disable_targets_from_tasks(program);
    if disable_targets.is_empty() {
        return Vec::new();
    }

    let profile_devices = program
        .topology
        .devices
        .iter()
        .filter(|device| {
            disable_targets.contains(&device.name)
                && crate::device_semantics::axis::MotionAxisCapability::from_device(device)
                    .is_some()
                && device
                    .attributes
                    .model_ref
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty())
                && device
                    .attributes
                    .config_ref
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty())
        })
        .cloned()
        .collect::<Vec<_>>();

    if profile_devices.is_empty() {
        return Vec::new();
    }

    let axis_profiles = match resolve_axis_profiles(&profile_devices) {
        Ok(profiles) => profiles,
        Err(errors) => {
            return errors
                .into_iter()
                .map(|error| SafetyDiagnostic {
                    line: error.line().max(1),
                    constraint: "[AXIS-012] vertical axis brake sequencing".to_string(),
                    reason: error.to_string(),
                    violation_path: vec!["topology axis profile resolution".to_string()],
                    suggestion: "请先修复轴配置中的 orientation/brake 字段后再运行 safety 验证"
                        .to_string(),
                })
                .collect();
        }
    };

    let brake_requirements = axis_profiles
        .into_iter()
        .filter_map(|(axis, profile)| {
            if matches!(profile.orientation, AxisOrientation::Vertical) {
                profile.brake.map(|brake| (axis, brake))
            } else {
                None
            }
        })
        .collect::<HashMap<_, _>>();

    if brake_requirements.is_empty() {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    for task in &program.tasks.tasks {
        for step in &task.steps {
            let mut progress = brake_requirements
                .keys()
                .map(|axis| (axis.clone(), BrakeSequenceProgress::default()))
                .collect::<HashMap<_, _>>();
            verify_vertical_axis_brake_sequence_in_statements(
                &step.statements,
                &task.name,
                &step.name,
                step.line.max(1),
                &brake_requirements,
                &mut progress,
                &mut diagnostics,
            );
        }
    }

    diagnostics
}

fn collect_axis_disable_targets_from_tasks(program: &PlcProgram) -> HashSet<String> {
    let mut targets = HashSet::new();
    for task in &program.tasks.tasks {
        for step in &task.steps {
            collect_axis_disable_targets_from_statements(&step.statements, &mut targets);
        }
    }
    targets
}

fn collect_axis_disable_targets_from_statements(
    statements: &[StepStatement],
    targets: &mut HashSet<String>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::Set { target, value }) => {
                if target.port == "enable"
                    && set_value_matches_binary(value, &crate::ir::BinaryValue::Off)
                {
                    targets.insert(target.device.clone());
                }
            }
            StepStatement::Repeat { body, .. } => {
                collect_axis_disable_targets_from_statements(body, targets)
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    collect_axis_disable_targets_from_statements(&branch.statements, targets);
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    collect_axis_disable_targets_from_statements(&branch.statements, targets);
                }
            }
            StepStatement::Action(_)
            | StepStatement::Effect(_)
            | StepStatement::Wait(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn verify_vertical_axis_brake_sequence_in_statements(
    statements: &[StepStatement],
    task_name: &str,
    step_name: &str,
    line: usize,
    brake_requirements: &HashMap<String, crate::ir::AxisBrakeConfig>,
    progress: &mut HashMap<String, BrakeSequenceProgress>,
    diagnostics: &mut Vec<SafetyDiagnostic>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::Set { target, value }) => {
                let Some(brake) = brake_requirements.get(&target.device) else {
                    continue;
                };

                if target.port == brake.engage_port
                    && set_value_matches_binary(value, &brake.engage_value)
                {
                    if let Some(state) = progress.get_mut(&target.device) {
                        state.engage_seen = true;
                        state.confirm_seen = false;
                    }
                    continue;
                }

                if target.port == "enable"
                    && set_value_matches_binary(value, &crate::ir::BinaryValue::Off)
                {
                    let state = progress.get(&target.device).copied().unwrap_or_default();
                    if !(state.engage_seen && state.confirm_seen) {
                        diagnostics.push(SafetyDiagnostic {
                            line,
                            constraint: format!(
                                "[AXIS-012] {}.enable.off requires brake_engage_confirmed",
                                target.device
                            ),
                            reason: format!(
                                "垂直轴 {} 在未确认抱闸的情况下执行了 disable",
                                target.device
                            ),
                            violation_path: vec![format!("task.{task_name}.step.{step_name}")],
                            suggestion: format!(
                                "请先执行 `set {}.{} {}`，再 `wait: {}.{} == {}`，然后再 disable 轴使能",
                                target.device,
                                brake.engage_port,
                                binary_value_text(&brake.engage_value),
                                target.device,
                                brake.engage_confirm_port,
                                bool_text(brake.engage_confirm_value),
                            ),
                        });
                    }
                }
            }
            StepStatement::Wait(wait) => {
                for (axis, brake) in brake_requirements {
                    let Some(state) = progress.get(axis).copied() else {
                        continue;
                    };
                    if !state.engage_seen {
                        continue;
                    }
                    if wait_asserts_brake_confirmed(wait, axis, brake) {
                        if let Some(state_mut) = progress.get_mut(axis) {
                            state_mut.confirm_seen = true;
                        }
                    }
                }
            }
            StepStatement::Repeat { body, .. } => {
                verify_vertical_axis_brake_sequence_in_statements(
                    body,
                    task_name,
                    step_name,
                    line,
                    brake_requirements,
                    progress,
                    diagnostics,
                );
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    let mut branch_progress = progress.clone();
                    verify_vertical_axis_brake_sequence_in_statements(
                        &branch.statements,
                        task_name,
                        step_name,
                        line,
                        brake_requirements,
                        &mut branch_progress,
                        diagnostics,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    let mut branch_progress = progress.clone();
                    verify_vertical_axis_brake_sequence_in_statements(
                        &branch.statements,
                        task_name,
                        step_name,
                        line,
                        brake_requirements,
                        &mut branch_progress,
                        diagnostics,
                    );
                }
            }
            StepStatement::Action(_)
            | StepStatement::Effect(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
}

fn set_value_matches_binary(value: &str, expected: &crate::ir::BinaryValue) -> bool {
    let actual = match value {
        "on" | "forward" | "active" => Some(crate::ir::BinaryValue::On),
        "off" | "reverse" | "idle" => Some(crate::ir::BinaryValue::Off),
        _ => None,
    };
    actual.as_ref() == Some(expected)
}

fn wait_asserts_brake_confirmed(
    wait: &WaitStatement,
    axis: &str,
    brake: &crate::ir::AxisBrakeConfig,
) -> bool {
    let expected_left = format!("{axis}.{}", brake.engage_confirm_port);
    let expected_right = brake.engage_confirm_value;

    let terms = match &wait.condition {
        WaitCondition::Single(term) => vec![term],
        WaitCondition::And(terms) => terms.iter().collect(),
        WaitCondition::Or(_) => return false,
        WaitCondition::Edge(_) => return false,
    };

    terms.into_iter().any(|term| {
        !term.is_expression_compare()
            && matches!(term.operator, ComparisonOperator::Eq)
            && term.left == expected_left
            && literal_matches_bool(&term.right, expected_right)
    })
}

fn literal_matches_bool(literal: &LiteralValue, expected: bool) -> bool {
    match literal {
        LiteralValue::Boolean(value) => *value == expected,
        LiteralValue::String(value) => {
            let normalized = value.trim();
            (normalized == "true" && expected) || (normalized == "false" && !expected)
        }
        _ => false,
    }
}

fn binary_value_text(value: &crate::ir::BinaryValue) -> &'static str {
    match value {
        crate::ir::BinaryValue::On => "on",
        crate::ir::BinaryValue::Off => "off",
    }
}

fn bool_text(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

