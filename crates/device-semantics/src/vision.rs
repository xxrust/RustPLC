use crate::{ActionContract, ActionResultBucket, DefaultFeedbackPolicy, SourceDeviceContract};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisionAction {
    Trigger,
    Acquire,
    Inspect,
    ResetFault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisionResult {
    Pass,
    Fail,
    Timeout,
    Reject,
    CommunicationFault,
    SafetyFault,
}

pub const FAMILY: &str = "vision_sensor";
pub const TRIGGER_PORT: &str = "trigger";
pub const READY_PORT: &str = "ready";
pub const BUSY_PORT: &str = "busy";
pub const PASS_PORT: &str = "pass";
pub const FAIL_PORT: &str = "fail";
pub const FAULT_PORT: &str = "fault";

pub const ACTION_CONTRACTS: &[ActionContract] = &[
    ActionContract {
        name: "inspect",
        result_buckets: &[
            ActionResultBucket::Complete,
            ActionResultBucket::Timeout,
            ActionResultBucket::Reject,
            ActionResultBucket::MotionFault,
            ActionResultBucket::SafetyFault,
        ],
    },
    ActionContract {
        name: "reset_fault",
        result_buckets: &[
            ActionResultBucket::Complete,
            ActionResultBucket::Reject,
            ActionResultBucket::SafetyFault,
        ],
    },
];

pub const SOURCE_CONTRACT: SourceDeviceContract = SourceDeviceContract {
    family: FAMILY,
    command_ports: &[TRIGGER_PORT],
    required_feedback_ports: &[READY_PORT, PASS_PORT, FAIL_PORT],
    fault_ports: &[FAULT_PORT],
    actions: ACTION_CONTRACTS,
    default_feedback_policy: DefaultFeedbackPolicy::FeedbackRequiredUnlessExplicitOpenLoop,
};
