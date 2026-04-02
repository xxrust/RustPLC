pub mod cylinder;

use crate::ast::TasksSection;
use crate::error::PlcError;
use crate::ir::DeviceKind;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceActionResultBucket {
    Complete,
    Timeout,
    MotionFault,
    SafetyFault,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceActionContract<'a> {
    pub family: &'a str,
    pub action: &'a str,
    pub result_buckets: &'a [DeviceActionResultBucket],
}

pub fn validate_task_action_semantics(
    tasks: &TasksSection,
    device_kinds: &HashMap<String, DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    cylinder::validate_cylinder_actions_in_tasks(tasks, device_kinds, errors);
}
