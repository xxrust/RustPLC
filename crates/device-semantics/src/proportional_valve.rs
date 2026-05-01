use crate::{ActionContract, ActionResultBucket, DefaultFeedbackPolicy, SourceDeviceContract};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProportionalValveAction {
    SetOpening,
    OpenTo,
    Close,
    Hold,
    ResetFault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProportionalValveResult {
    Done,
    Timeout,
    Reject,
    MotionFault,
    SafetyFault,
}

pub const FAMILY: &str = "proportional_valve";
pub const CMD_PORT: &str = "cmd";
pub const FEEDBACK_PORT: &str = "feedback";
pub const FAULT_PORT: &str = "fault";

pub const ACTION_CONTRACTS: &[ActionContract] = &[
    ActionContract {
        name: "set_opening",
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
    command_ports: &[CMD_PORT],
    required_feedback_ports: &[FEEDBACK_PORT],
    fault_ports: &[FAULT_PORT],
    actions: ACTION_CONTRACTS,
    default_feedback_policy: DefaultFeedbackPolicy::FeedbackRequiredUnlessExplicitOpenLoop,
};
