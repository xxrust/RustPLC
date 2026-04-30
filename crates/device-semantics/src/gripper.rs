#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GripperAction {
    Grip,
    Release,
    Hold,
    ResetFault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GripperResult {
    Done,
    Timeout,
    NoPart,
    LostPart,
    MotionFault,
    SafetyFault,
}

pub const FAMILY: &str = "gripper";
pub const GRIPPED_PORT: &str = "gripped";
pub const RELEASED_PORT: &str = "released";
pub const PART_PRESENT_PORT: &str = "part_present";
pub const FAULT_PORT: &str = "fault";
