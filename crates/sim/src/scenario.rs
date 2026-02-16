use core::fmt;
use std::collections::BTreeMap;

use io_traits::{AnalogInputId, DigitalInputId, Tick};
use serde::{Deserialize, Serialize};

use crate::SimIo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScenarioError {
    Parse { path: String, message: String },
    Validation { path: String, message: String },
}

impl fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScenarioError::Parse { path, message } => {
                write!(f, "scenario yaml parse error at {path}: {message}")
            }
            ScenarioError::Validation { path, message } => {
                write!(f, "scenario validation error at {path}: {message}")
            }
        }
    }
}

impl std::error::Error for ScenarioError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scenario {
    /// Used for reproducibility when we later add randomized disturbances.
    #[serde(default)]
    pub seed: Option<u64>,

    /// Duration of one tick in ms.
    pub tick_ms: u64,

    /// Total simulated duration in ms.
    pub duration_ms: u64,

    /// Scripted input changes over time.
    #[serde(default)]
    pub inputs: Vec<InputEvent>,

    /// Scripted fault injections over time.
    #[serde(default)]
    pub faults: Vec<FaultEvent>,
}

impl Scenario {
    pub fn from_yaml_str(yaml: &str) -> Result<Self, ScenarioError> {
        let mut docs = serde_yaml::Deserializer::from_str(yaml);
        let Some(doc) = docs.next() else {
            return Err(ScenarioError::Parse {
                path: "<document>".to_string(),
                message: "empty yaml document".to_string(),
            });
        };

        let s: Scenario =
            serde_path_to_error::deserialize(doc).map_err(|e| ScenarioError::Parse {
                path: e.path().to_string(),
                message: e.into_inner().to_string(),
            })?;
        s.validate()?;
        Ok(s)
    }

    pub fn duration_ticks(&self) -> u64 {
        if self.tick_ms == 0 {
            0
        } else {
            (self.duration_ms + self.tick_ms - 1) / self.tick_ms
        }
    }

    pub fn apply_to_simio(&self, io: &mut SimIo) -> Result<(), ScenarioError> {
        // Assumes `validate()` was already called, but keep this deterministic and safe anyway.
        if self.tick_ms == 0 {
            return Err(ScenarioError::Validation {
                path: "tick_ms".to_string(),
                message: "must be > 0".to_string(),
            });
        }

        for (idx, ev) in self.inputs.iter().enumerate() {
            if ev.at_ms >= self.duration_ms && self.duration_ms != 0 {
                return Err(ScenarioError::Validation {
                    path: format!("inputs[{idx}].at_ms"),
                    message: format!("must be < duration_ms ({})", self.duration_ms),
                });
            }
            if ev.at_ms % self.tick_ms != 0 {
                return Err(ScenarioError::Validation {
                    path: format!("inputs[{idx}].at_ms"),
                    message: format!(
                        "must be aligned to tick_ms ({}); got {}",
                        self.tick_ms, ev.at_ms
                    ),
                });
            }
            let tick = ev.at_ms / self.tick_ms;
            ev.set.apply(io, Tick(tick));
        }

        for (idx, fault) in self.faults.iter().enumerate() {
            let (at_ms, path_prefix) = (fault.at_ms(), format!("faults[{idx}]"));
            if at_ms >= self.duration_ms && self.duration_ms != 0 {
                return Err(ScenarioError::Validation {
                    path: format!("{path_prefix}.at_ms"),
                    message: format!("must be < duration_ms ({})", self.duration_ms),
                });
            }
            if at_ms % self.tick_ms != 0 {
                return Err(ScenarioError::Validation {
                    path: format!("{path_prefix}.at_ms"),
                    message: format!(
                        "must be aligned to tick_ms ({}); got {}",
                        self.tick_ms, at_ms
                    ),
                });
            }
            let tick = at_ms / self.tick_ms;
            fault.apply(io, Tick(tick));
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), ScenarioError> {
        if self.tick_ms == 0 {
            return Err(ScenarioError::Validation {
                path: "tick_ms".to_string(),
                message: "must be > 0".to_string(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputEvent {
    pub at_ms: u64,
    pub set: InputSet,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct InputSet {
    /// Map from digital input id to value.
    #[serde(default)]
    pub digital_inputs: BTreeMap<u16, bool>,
    /// Map from analog input id to value.
    #[serde(default)]
    pub analog_inputs: BTreeMap<u16, f32>,
}

impl InputSet {
    fn apply(&self, io: &mut SimIo, tick: Tick) {
        for (id, value) in &self.digital_inputs {
            io.schedule_digital_input(tick, DigitalInputId(*id), *value);
        }
        for (id, value) in &self.analog_inputs {
            io.schedule_analog_input(tick, AnalogInputId(*id), *value);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaultEvent {
    pub sensor_stuck: SensorStuckFault,
}

impl FaultEvent {
    fn at_ms(&self) -> u64 {
        self.sensor_stuck.at_ms
    }

    fn apply(&self, io: &mut SimIo, tick: Tick) {
        io.schedule_sensor_stuck(
            tick,
            DigitalInputId(self.sensor_stuck.target),
            self.sensor_stuck.value,
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct SensorStuckFault {
    pub at_ms: u64,
    /// Digital input id (e.g. `0` for DI0).
    pub target: u16,
    pub value: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use io_traits::DigitalOutputId;
    use runtime_core::{Action, Instr, Program, Runtime, Step, StepId, Task};

    #[test]
    fn scenario_yaml_drives_inputs_and_is_reproducible() {
        // Tick 0 @ 0ms: DI0=false. Tick 2 @ 20ms: DI0=true.
        let yaml = r#"
seed: 123
tick_ms: 10
duration_ms: 30
inputs:
  - at_ms: 0
    set:
      digital_inputs:
        0: false
  - at_ms: 20
    set:
      digital_inputs:
        0: true
"#;

        let scenario = Scenario::from_yaml_str(yaml).unwrap();

        static STEP1_ACTIONS: [Action; 1] = [Action::SetDigital {
            id: DigitalOutputId(0),
            value: true,
        }];
        static STEPS: [Step<'static>; 3] = [
            Step {
                name: "wait_di0_true",
                instr: Instr::WaitDigital {
                    id: DigitalInputId(0),
                    equals: true,
                    next: StepId(1),
                    timeout: None,
                },
            },
            Step {
                name: "set_do0_true",
                instr: Instr::Action {
                    actions: &STEP1_ACTIONS,
                    next: StepId(2),
                },
            },
            Step {
                name: "halt",
                instr: Instr::Halt,
            },
        ];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static PROGRAM: Program<'static> = Program { tasks: &TASKS };

        fn run(scenario: &Scenario) -> (Vec<String>, Vec<crate::DigitalEdge>) {
            let mut io = SimIo::new(1, 1, 0, 0);
            scenario.apply_to_simio(&mut io).unwrap();

            let mut rt = Runtime::new(&PROGRAM).unwrap();
            let mut trace = crate::JsonlTraceRecorder::new();
            for _ in 0..scenario.duration_ticks() {
                rt.tick_with_trace(&mut io, |e| trace.record(e)).unwrap();
            }
            (trace.lines().to_vec(), io.digital_output_edges().to_vec())
        }

        let (t1, e1) = run(&scenario);
        let (t2, e2) = run(&scenario);

        assert_eq!(t1, t2);
        assert_eq!(e1, e2);

        // Should fire at tick 2 (20ms).
        assert_eq!(e1.len(), 1);
        assert_eq!(e1[0].tick, Tick(2));
        assert_eq!(e1[0].id, DigitalOutputId(0));
        assert_eq!(e1[0].value, true);
    }

    #[test]
    fn scenario_yaml_parses_sensor_stuck_faults() {
        let yaml = r#"
seed: 42
tick_ms: 10
duration_ms: 30
inputs:
  - at_ms: 10
    set:
      digital_inputs:
        0: true
faults:
  - sensor_stuck:
      at_ms: 0
      target: 0
      value: false
"#;

        let scenario = Scenario::from_yaml_str(yaml).unwrap();
        assert_eq!(scenario.seed, Some(42));
        assert_eq!(scenario.tick_ms, 10);
        assert_eq!(scenario.duration_ms, 30);
        assert_eq!(scenario.inputs.len(), 1);
        assert_eq!(scenario.faults.len(), 1);
    }
}
