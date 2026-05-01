use crate::{ActionContract, ActionResultBucket, DefaultFeedbackPolicy, SourceDeviceContract};

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
        name: "stop",
        result_buckets: &[ActionResultBucket::Complete, ActionResultBucket::Timeout],
    },
];

pub const SOURCE_CONTRACT: SourceDeviceContract = SourceDeviceContract {
    family: FAMILY,
    command_ports: &[DRIVE_PORT],
    required_feedback_ports: &[RUNNING_PORT],
    fault_ports: &[JAM_PORT, FAULT_PORT],
    actions: ACTION_CONTRACTS,
    default_feedback_policy: DefaultFeedbackPolicy::FeedbackRequiredUnlessExplicitOpenLoop,
};
