#![no_std]
#![forbid(unsafe_code)]

pub mod axis;
pub mod cam;
pub mod conveyor;
pub mod cylinder;
pub mod drive;
pub mod gripper;
pub mod heater;
pub mod proportional_valve;
pub mod pump;
pub mod valve;
pub mod vision;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionResultBucket {
    Complete,
    Timeout,
    Reject,
    MotionFault,
    SafetyFault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultFeedbackPolicy {
    FeedbackRequiredUnlessExplicitOpenLoop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionContract {
    pub name: &'static str,
    pub result_buckets: &'static [ActionResultBucket],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceDeviceContract {
    pub family: &'static str,
    pub command_ports: &'static [&'static str],
    pub required_feedback_ports: &'static [&'static str],
    pub fault_ports: &'static [&'static str],
    pub actions: &'static [ActionContract],
    pub default_feedback_policy: DefaultFeedbackPolicy,
}
