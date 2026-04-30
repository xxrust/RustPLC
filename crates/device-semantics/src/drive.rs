#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveCapabilityKind {
    DiscreteRun,
    VariableSpeed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveResult {
    Done,
    Timeout,
    Reject,
    DriveFault,
    SafetyFault,
}

pub const FAMILY: &str = "drive";
pub const RUN_ACTION: &str = "run";
pub const STOP_ACTION: &str = "stop";
pub const RESET_FAULT_ACTION: &str = "reset_fault";
