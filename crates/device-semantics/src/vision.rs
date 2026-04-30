#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisionAction {
    Trigger,
    Acquire,
    Inspect,
    ResetFault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisionResult {
    Pass,
    Fail,
    Timeout,
    Reject,
    CommunicationFault,
    SafetyFault,
}

pub const FAMILY: &str = "vision_sensor";
pub const TRIGGER_PORT: &str = "trigger";
pub const READY_PORT: &str = "ready";
pub const BUSY_PORT: &str = "busy";
pub const PASS_PORT: &str = "pass";
pub const FAIL_PORT: &str = "fail";
pub const FAULT_PORT: &str = "fault";
