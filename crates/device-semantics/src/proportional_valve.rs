#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProportionalValveAction {
    SetOpening,
    OpenTo,
    Close,
    Hold,
    ResetFault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProportionalValveResult {
    Done,
    Timeout,
    Reject,
    MotionFault,
    SafetyFault,
}

pub const FAMILY: &str = "proportional_valve";
pub const CMD_PORT: &str = "cmd";
pub const FEEDBACK_PORT: &str = "feedback";
pub const FAULT_PORT: &str = "fault";
