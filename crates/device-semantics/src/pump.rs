#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PumpAction {
    Start,
    Stop,
    Prime,
    HoldPressure,
    ResetFault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PumpResult {
    Done,
    Timeout,
    DryRun,
    Overpressure,
    NoFlow,
    LowLevel,
    SafetyFault,
}

pub const FAMILY: &str = "pump";
pub const DRIVE_PORT: &str = "drive";
pub const PRESSURE_PORT: &str = "pressure";
pub const FLOW_PORT: &str = "flow";
pub const FAULT_PORT: &str = "fault";
