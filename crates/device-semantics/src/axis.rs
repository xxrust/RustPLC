pub const FAMILY: &str = "axis";
pub const DEFAULT_PORT: &str = "self";
pub const MOVE_RELATIVE_ACTION: &str = "move_relative";
pub const MOVE_ABSOLUTE_ACTION: &str = "move_absolute";
pub const DEFAULT_REQUIRE_HOMED: bool = true;

pub const AXIS_FAULT_POLICY_LOG_MESSAGE: &str = "axis_fault_policy_applied";
const AXIS_FAULT_POLICY_LOG_BASE_ID: u16 = 50_000;
pub const AXIS_STOP_TRANSITION_ENTER_LOG_MESSAGE: &str = "axis_stop_transition_enter";
pub const AXIS_STOP_TRANSITION_COMPLETED_LOG_MESSAGE: &str = "axis_stop_transition_completed";
const AXIS_STOP_TRANSITION_LOG_BASE_ID: u16 = 51_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisMoveKind {
    Relative,
    Absolute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisFaultCategory {
    Recoverable,
    NonRecoverable,
    Safety,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisFaultKind {
    Reject,
    Motion,
    Safety,
    Vendor {
        category: AxisFaultCategory,
        vendor_code: i32,
    },
}

impl AxisFaultKind {
    pub const fn category(self) -> AxisFaultCategory {
        match self {
            AxisFaultKind::Reject => AxisFaultCategory::Recoverable,
            AxisFaultKind::Motion => AxisFaultCategory::NonRecoverable,
            AxisFaultKind::Safety => AxisFaultCategory::Safety,
            AxisFaultKind::Vendor { category, .. } => category,
        }
    }

    pub const fn vendor_code(self) -> Option<i32> {
        match self {
            AxisFaultKind::Vendor { vendor_code, .. } => Some(vendor_code),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxisFault {
    pub kind: AxisFaultKind,
    pub category: AxisFaultCategory,
    pub error_code: i32,
    pub vendor_code: Option<i32>,
}

impl AxisFault {
    pub const fn new(kind: AxisFaultKind, error_code: i32) -> Self {
        Self {
            category: kind.category(),
            vendor_code: kind.vendor_code(),
            kind,
            error_code,
        }
    }

    pub const fn reject(error_code: i32) -> Self {
        Self::new(AxisFaultKind::Reject, error_code)
    }

    pub const fn motion(error_code: i32) -> Self {
        Self::new(AxisFaultKind::Motion, error_code)
    }

    pub const fn safety(error_code: i32) -> Self {
        Self::new(AxisFaultKind::Safety, error_code)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisMotionResult {
    Pending,
    Done,
    Fault(AxisFault),
}

impl AxisMotionResult {
    pub const fn reject(error_code: i32) -> Self {
        Self::Fault(AxisFault::reject(error_code))
    }

    pub const fn motion_fault(error_code: i32) -> Self {
        Self::Fault(AxisFault::motion(error_code))
    }

    pub const fn safety_fault(error_code: i32) -> Self {
        Self::Fault(AxisFault::safety(error_code))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisFaultSeverity {
    Recoverable,
    NonRecoverable,
    Safety,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisStopMode {
    Controlled,
    Quick,
    Immediate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisStopState {
    Running,
    ControlledStopping,
    QuickStopping,
    ImmediateStopping,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisAutoResetPolicy {
    Never,
    OnClear,
    Immediate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisFaultPropagationScope {
    SelfOnly,
    Group,
    All,
    Followers,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxisFaultPolicy<'a> {
    pub axis: &'a str,
    pub severity: AxisFaultSeverity,
    pub stop_mode: AxisStopMode,
    pub auto_reset_policy: AxisAutoResetPolicy,
    pub manual_ack_required: bool,
    pub propagation_scope: AxisFaultPropagationScope,
    pub propagation_targets: &'a [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisFaultRouteKind {
    Reject,
    Motion,
    Safety,
    Vendor,
}

impl AxisFaultRouteKind {
    pub const fn from_fault_kind(kind: AxisFaultKind) -> Self {
        match kind {
            AxisFaultKind::Reject => Self::Reject,
            AxisFaultKind::Motion => Self::Motion,
            AxisFaultKind::Safety => Self::Safety,
            AxisFaultKind::Vendor { .. } => Self::Vendor,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisStopTransitionPhase {
    Enter,
    Completed,
}

pub const fn axis_fault_policy_log_message_id(
    severity: AxisFaultSeverity,
    stop_mode: AxisStopMode,
    auto_reset_policy: AxisAutoResetPolicy,
    manual_ack_required: bool,
    fault_kind: AxisFaultKind,
) -> u16 {
    let severity_bits = match severity {
        AxisFaultSeverity::Recoverable => 0,
        AxisFaultSeverity::NonRecoverable => 1,
        AxisFaultSeverity::Safety => 2,
    };
    let stop_mode_bits = match stop_mode {
        AxisStopMode::Controlled => 0,
        AxisStopMode::Quick => 1,
        AxisStopMode::Immediate => 2,
    };
    let auto_reset_bits = match auto_reset_policy {
        AxisAutoResetPolicy::Never => 0,
        AxisAutoResetPolicy::OnClear => 1,
        AxisAutoResetPolicy::Immediate => 2,
    };
    let ack_bits = if manual_ack_required { 1 } else { 0 };
    let fault_kind_bits = match fault_kind {
        AxisFaultKind::Reject => 0,
        AxisFaultKind::Motion => 1,
        AxisFaultKind::Safety => 2,
        AxisFaultKind::Vendor { .. } => 3,
    };

    AXIS_FAULT_POLICY_LOG_BASE_ID
        + severity_bits
        + (stop_mode_bits << 2)
        + (auto_reset_bits << 4)
        + (ack_bits << 6)
        + (fault_kind_bits << 7)
}

pub const fn axis_stop_transition_log_message_id(
    stop_mode: AxisStopMode,
    phase: AxisStopTransitionPhase,
) -> u16 {
    let stop_mode_bits = match stop_mode {
        AxisStopMode::Controlled => 0,
        AxisStopMode::Quick => 1,
        AxisStopMode::Immediate => 2,
    };
    let phase_bits = match phase {
        AxisStopTransitionPhase::Enter => 0,
        AxisStopTransitionPhase::Completed => 1,
    };

    AXIS_STOP_TRANSITION_LOG_BASE_ID + stop_mode_bits + (phase_bits << 2)
}

#[cfg(test)]
mod tests {
    use super::{
        AxisAutoResetPolicy, AxisFaultCategory, AxisFaultKind, AxisFaultRouteKind,
        AxisFaultSeverity, AxisMotionResult, AxisStopMode, AxisStopTransitionPhase, DEFAULT_PORT,
        DEFAULT_REQUIRE_HOMED, FAMILY, MOVE_ABSOLUTE_ACTION, MOVE_RELATIVE_ACTION,
        axis_fault_policy_log_message_id, axis_stop_transition_log_message_id,
    };

    #[test]
    fn exposes_axis_motion_defaults_and_action_names() {
        assert_eq!(FAMILY, "axis");
        assert_eq!(DEFAULT_PORT, "self");
        assert_eq!(MOVE_RELATIVE_ACTION, "move_relative");
        assert_eq!(MOVE_ABSOLUTE_ACTION, "move_absolute");
        assert!(DEFAULT_REQUIRE_HOMED);
    }

    #[test]
    fn maps_fault_kind_to_category_and_route_bucket() {
        assert_eq!(
            AxisFaultKind::Reject.category(),
            AxisFaultCategory::Recoverable
        );
        assert_eq!(
            AxisFaultKind::Motion.category(),
            AxisFaultCategory::NonRecoverable
        );
        assert_eq!(AxisFaultKind::Safety.category(), AxisFaultCategory::Safety);
        assert_eq!(
            AxisFaultRouteKind::from_fault_kind(AxisFaultKind::Vendor {
                category: AxisFaultCategory::Safety,
                vendor_code: 3303,
            }),
            AxisFaultRouteKind::Vendor
        );
    }

    #[test]
    fn constructs_standard_axis_motion_fault_results() {
        assert!(matches!(
            AxisMotionResult::reject(41),
            AxisMotionResult::Fault(fault)
                if fault.kind == AxisFaultKind::Reject
                    && fault.category == AxisFaultCategory::Recoverable
                    && fault.error_code == 41
        ));
    }

    #[test]
    fn keeps_axis_log_message_ids_stable() {
        assert_eq!(
            axis_fault_policy_log_message_id(
                AxisFaultSeverity::Safety,
                AxisStopMode::Immediate,
                AxisAutoResetPolicy::Never,
                true,
                AxisFaultKind::Safety,
            ),
            50_330
        );
        assert_eq!(
            axis_stop_transition_log_message_id(
                AxisStopMode::Immediate,
                AxisStopTransitionPhase::Completed,
            ),
            51_006
        );
    }
}
