use crate::ast::{ActionStatement, StepStatement};
use crate::error::PlcError;
use crate::ir::DeviceKind;
use std::collections::HashMap;

pub fn validate_legacy_set_actions(
    statements: &[StepStatement],
    line: usize,
    device_kinds: &HashMap<String, DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::Set { target, value })
                if target.port == "self"
                    && matches!(device_kinds.get(&target.device), Some(DeviceKind::Motor)) =>
            {
                errors.push(PlcError::semantic_with_reason(
                    line,
                    format!("set {} {value} 旧写法已废弃", target.device),
                    format!(
                        "请改用显式端口写法：set {}.run on/off 或 set {}.direction forward/reverse",
                        target.device, target.device
                    ),
                ));
            }
            StepStatement::Repeat { body, .. } => {
                validate_legacy_set_actions(body, line, device_kinds, errors)
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_legacy_set_actions(&branch.statements, line, device_kinds, errors);
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_legacy_set_actions(&branch.statements, line, device_kinds, errors);
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

pub fn validate_legacy_wait_operand(
    operand: &str,
    line: usize,
    device_kinds: &HashMap<String, DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    let mut parts = operand.split('.');
    let Some(device) = parts.next() else {
        return;
    };
    let Some(state) = parts.next() else {
        return;
    };
    if parts.next().is_some() {
        return;
    }

    if matches!(device_kinds.get(device), Some(DeviceKind::Motor)) {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!("wait 条件使用了已废弃的电机状态写法 {device}.{state}"),
            format!(
                "请改用显式端口状态，例如 {device}.run.on/off 或 {device}.direction.forward/reverse"
            ),
        ));
    }
}
