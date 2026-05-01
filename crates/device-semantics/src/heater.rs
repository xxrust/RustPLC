use crate::{ActionContract, ActionResultBucket, DefaultFeedbackPolicy, SourceDeviceContract};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaterAction {
    HeatTo,
    HoldTemperature,
    StopHeat,
    ResetFault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaterResult {
    Done,
    Timeout,
    Overtemperature,
    SensorFault,
    ThermalRunaway,
    SafetyFault,
}

pub const FAMILY: &str = "heater";
pub const POWER_PORT: &str = "power";
pub const TEMPERATURE_PORT: &str = "temperature";
pub const FAULT_PORT: &str = "fault";

pub const ACTION_CONTRACTS: &[ActionContract] = &[
    ActionContract {
        name: "heat_to",
        result_buckets: &[
            ActionResultBucket::Complete,
            ActionResultBucket::Timeout,
            ActionResultBucket::Reject,
            ActionResultBucket::MotionFault,
            ActionResultBucket::SafetyFault,
        ],
    },
    ActionContract {
        name: "stop_heat",
        result_buckets: &[ActionResultBucket::Complete, ActionResultBucket::Timeout],
    },
];

pub const SOURCE_CONTRACT: SourceDeviceContract = SourceDeviceContract {
    family: FAMILY,
    command_ports: &[POWER_PORT],
    required_feedback_ports: &[TEMPERATURE_PORT],
    fault_ports: &[FAULT_PORT],
    actions: ACTION_CONTRACTS,
    default_feedback_policy: DefaultFeedbackPolicy::FeedbackRequiredUnlessExplicitOpenLoop,
};
