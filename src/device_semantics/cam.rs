use super::device_kind_name;
use crate::ast::{ActionStatement, StepStatement, TasksSection};
use crate::error::PlcError;
use crate::ir::DeviceKind;
use std::collections::{HashMap, HashSet};

pub fn validate_cam_actions_in_tasks(
    tasks: &TasksSection,
    device_kinds: &HashMap<String, DeviceKind>,
    cam_table_names: &HashSet<String>,
    errors: &mut Vec<PlcError>,
) {
    for task in &tasks.tasks {
        for step in &task.steps {
            validate_cam_actions_in_statements(
                &step.statements,
                step.line.max(1),
                device_kinds,
                cam_table_names,
                errors,
            );
        }
    }
}

fn validate_cam_actions_in_statements(
    statements: &[StepStatement],
    line: usize,
    device_kinds: &HashMap<String, DeviceKind>,
    cam_table_names: &HashSet<String>,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::CamEngage { target })
            | StepStatement::Action(ActionStatement::CamDisengage { target })
            | StepStatement::Action(ActionStatement::CamPhase { target, .. }) => {
                match device_kinds.get(target) {
                    Some(DeviceKind::CamCoupling) => {}
                    Some(kind) => errors.push(PlcError::type_mismatch_with_reason(
                        line,
                        "cam_coupling",
                        device_kind_name(kind),
                        format!("cam action {target}"),
                        "cam 动作仅支持作用于 cam_coupling 设备",
                    )),
                    None => errors.push(PlcError::undefined_reference_with_reason(
                        line,
                        "设备",
                        target,
                        "cam 动作引用前需要在 [topology] 中定义 cam_coupling 设备".to_string(),
                    )),
                }
            }
            StepStatement::Action(ActionStatement::CamSwitch { target, new_table }) => {
                match device_kinds.get(target) {
                    Some(DeviceKind::CamCoupling) => {}
                    Some(kind) => errors.push(PlcError::type_mismatch_with_reason(
                        line,
                        "cam_coupling",
                        device_kind_name(kind),
                        format!("cam_switch {target}"),
                        "cam_switch 仅支持作用于 cam_coupling 设备",
                    )),
                    None => errors.push(PlcError::undefined_reference_with_reason(
                        line,
                        "设备",
                        target,
                        "cam_switch 引用前需要定义 cam_coupling 设备".to_string(),
                    )),
                }
                if !cam_table_names.contains(new_table) {
                    errors.push(PlcError::undefined_reference_with_reason(
                        line,
                        "cam_table",
                        new_table,
                        "cam_switch 的目标表需要先在 [topology] 中声明".to_string(),
                    ));
                }
            }
            StepStatement::Repeat { body, .. } => validate_cam_actions_in_statements(
                body,
                line,
                device_kinds,
                cam_table_names,
                errors,
            ),
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_cam_actions_in_statements(
                        &branch.statements,
                        line,
                        device_kinds,
                        cam_table_names,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_cam_actions_in_statements(
                        &branch.statements,
                        line,
                        device_kinds,
                        cam_table_names,
                        errors,
                    );
                }
            }
            StepStatement::Action(_)
            | StepStatement::Wait(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_)
            | StepStatement::Effect(_) => {}
        }
    }
}
