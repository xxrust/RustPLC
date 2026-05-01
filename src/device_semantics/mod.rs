pub mod axis;
pub mod cam;
pub mod cylinder;
pub mod drive;
pub mod motor;
pub mod process;
pub mod valve;

use crate::ast::TasksSection;
use crate::error::PlcError;
use crate::ir::DeviceKind;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceActionResultBucket {
    Complete,
    Timeout,
    Reject,
    MotionFault,
    SafetyFault,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceActionContract<'a> {
    pub family: &'a str,
    pub action: &'a str,
    pub result_buckets: &'a [DeviceActionResultBucket],
}

pub(crate) const fn device_kind_name(kind: &DeviceKind) -> &'static str {
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

pub fn validate_task_action_semantics(
    tasks: &TasksSection,
    device_kinds: &HashMap<String, DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    cylinder::validate_cylinder_actions_in_tasks(tasks, device_kinds, errors);
    valve::validate_solenoid_valve_actions_in_tasks(tasks, device_kinds, errors);
}
