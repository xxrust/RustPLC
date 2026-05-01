use crate::{ActionContract, ActionResultBucket, DefaultFeedbackPolicy, SourceDeviceContract};

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
pub const CMD_PORT: &str = "cmd";
pub const GRIPPED_PORT: &str = "gripped";
pub const RELEASED_PORT: &str = "released";
pub const PART_PRESENT_PORT: &str = "part_present";
pub const FAULT_PORT: &str = "fault";

pub const ACTION_CONTRACTS: &[ActionContract] = &[
    ActionContract {
        name: "grip",
        result_buckets: &[
            ActionResultBucket::Complete,
            ActionResultBucket::Timeout,
            ActionResultBucket::Reject,
            ActionResultBucket::MotionFault,
            ActionResultBucket::SafetyFault,
        ],
    },
    ActionContract {
        name: "release",
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
    command_ports: &[CMD_PORT],
    required_feedback_ports: &[GRIPPED_PORT, RELEASED_PORT],
    fault_ports: &[FAULT_PORT],
    actions: ACTION_CONTRACTS,
    default_feedback_policy: DefaultFeedbackPolicy::FeedbackRequiredUnlessExplicitOpenLoop,
};
