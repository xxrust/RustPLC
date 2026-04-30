#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolenoidValveVariant {
    SingleSolenoid,
    DoubleSolenoid,
    ThreePosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValveResult {
    Done,
    Timeout,
    Reject,
    CoilConflict,
    SafetyFault,
}

pub const FAMILY: &str = "valve";
pub const ACTUATE_ACTION: &str = "actuate";
pub const RESET_FAULT_ACTION: &str = "reset_fault";
pub const SAFE_DEFAULT_OFF: &str = "off";
