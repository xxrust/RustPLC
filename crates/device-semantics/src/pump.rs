use crate::{ActionContract, ActionResultBucket, DefaultFeedbackPolicy, SourceDeviceContract};

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
pub const RUNNING_PORT: &str = "running";
pub const PRESSURE_PORT: &str = "pressure";
pub const FLOW_PORT: &str = "flow";
pub const FAULT_PORT: &str = "fault";

pub const ACTION_CONTRACTS: &[ActionContract] = &[
    ActionContract {
        name: "start",
        result_buckets: &[
            ActionResultBucket::Complete,
            ActionResultBucket::Timeout,
            ActionResultBucket::Reject,
            ActionResultBucket::MotionFault,
            ActionResultBucket::SafetyFault,
        ],
    },
    ActionContract {
        name: "hold_pressure",
        result_buckets: &[
            ActionResultBucket::Complete,
            ActionResultBucket::Timeout,
            ActionResultBucket::MotionFault,
            ActionResultBucket::SafetyFault,
        ],
    },
];

pub const SOURCE_CONTRACT: SourceDeviceContract = SourceDeviceContract {
    family: FAMILY,
    command_ports: &[DRIVE_PORT],
    required_feedback_ports: &[PRESSURE_PORT, FLOW_PORT],
    fault_ports: &[FAULT_PORT],
    actions: ACTION_CONTRACTS,
    default_feedback_policy: DefaultFeedbackPolicy::FeedbackRequiredUnlessExplicitOpenLoop,
};
