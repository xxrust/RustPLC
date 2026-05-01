use crate::ast::{ActionStatement, StepStatement, TasksSection};
use crate::error::PlcError;
use crate::ir::DeviceKind;
use std::collections::{HashMap, HashSet};

pub fn validate_solenoid_valve_actions_in_tasks(
    tasks: &TasksSection,
    device_kinds: &HashMap<String, DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    for task in &tasks.tasks {
        for step in &task.steps {
            validate_solenoid_valve_actions_in_statements(
                &step.statements,
                step.line.max(task.line).max(1),
                device_kinds,
                errors,
            );
        }
    }
}

fn validate_solenoid_valve_actions_in_statements(
    statements: &[StepStatement],
    line: usize,
    device_kinds: &HashMap<String, DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    let mut energized_coils_by_valve = HashMap::<String, HashSet<String>>::new();

    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::Set { target, value })
                if matches!(
                    device_kinds.get(&target.device),
                    Some(DeviceKind::SolenoidValve)
                ) && target.port.contains("coil")
                    && value == "on" =>
            {
                energized_coils_by_valve
                    .entry(target.device.clone())
                    .or_default()
                    .insert(target.port.clone());
            }
            StepStatement::Repeat { body, .. } => {
                validate_solenoid_valve_actions_in_statements(body, line, device_kinds, errors);
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_solenoid_valve_actions_in_statements(
                        &branch.statements,
                        line,
                        device_kinds,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_solenoid_valve_actions_in_statements(
                        &branch.statements,
                        line,
                        device_kinds,
                        errors,
                    );
                }
            }
            _ => {}
        }
    }

    for (valve, coils) in energized_coils_by_valve {
        if coils.len() < 2 {
            continue;
        }
        let mut coil_list = coils.into_iter().collect::<Vec<_>>();
        coil_list.sort();
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "solenoid_valve `{valve}` energizes mutually exclusive coils in one step: {}",
                coil_list.join(", ")
            ),
            "drive only one coil per double-solenoid valve state, or express the higher-level cylinder/valve action instead of raw coil choreography",
        ));
    }
}
