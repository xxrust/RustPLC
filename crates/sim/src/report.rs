use serde::Serialize;

use crate::Scenario;
use crate::scenario::FaultEvent;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SimFailure {
    pub kind: String,
    pub message: String,
    pub at_ms: u64,
    pub task: usize,
    pub step: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SimReport {
    pub seed: Option<u64>,
    pub scenario: ScenarioSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<SimFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScenarioSummary {
    pub tick_ms: u64,
    pub duration_ms: u64,
    pub inputs_count: usize,
    pub faults: Vec<FaultSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultSummary {
    SensorStuck {
        at_ms: u64,
        target: u16,
        value: bool,
    },
}

impl ScenarioSummary {
    pub fn from_scenario(s: &Scenario) -> Self {
        Self {
            tick_ms: s.tick_ms,
            duration_ms: s.duration_ms,
            inputs_count: s.inputs.len(),
            faults: s
                .faults
                .iter()
                .cloned()
                .map(FaultSummary::from_fault)
                .collect(),
        }
    }
}

impl FaultSummary {
    fn from_fault(f: FaultEvent) -> Self {
        let s = f.sensor_stuck;
        FaultSummary::SensorStuck {
            at_ms: s.at_ms,
            target: s.target,
            value: s.value,
        }
    }
}
