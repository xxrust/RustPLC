#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConveyorAction {
    Start,
    Stop,
    MoveUntil,
    Index,
    Reverse,
    ClearJam,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConveyorResult {
    Done,
    Timeout,
    Reject,
    Jam,
    Overload,
    SafetyFault,
}

pub const FAMILY: &str = "conveyor";
pub const DRIVE_PORT: &str = "drive";
pub const RUNNING_PORT: &str = "running";
pub const JAM_PORT: &str = "jam";
pub const FAULT_PORT: &str = "fault";
