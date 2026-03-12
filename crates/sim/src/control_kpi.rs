use core::fmt;
use std::collections::VecDeque;

use io_traits::{AnalogInputId, AnalogOutputId, DigitalInputId, DigitalOutputId, Io, Tick};
use runtime_core::{Program, Runtime, RuntimeError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq)]
pub enum ControlKpiError {
    Parse { path: String, message: String },
    Validation { path: String, message: String },
    MissingPidLoop { requested: usize, available: usize },
    Runtime(RuntimeError),
}

impl fmt::Display for ControlKpiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ControlKpiError::Parse { path, message } => {
                write!(f, "pid-kpi scenario parse error at {path}: {message}")
            }
            ControlKpiError::Validation { path, message } => {
                write!(f, "pid-kpi scenario validation error at {path}: {message}")
            }
            ControlKpiError::MissingPidLoop {
                requested,
                available,
            } => write!(
                f,
                "pid-kpi requested loop_index={requested}, but program only has {available} pid loop(s)"
            ),
            ControlKpiError::Runtime(err) => write!(f, "pid-kpi runtime error: {err:?}"),
        }
    }
}

impl std::error::Error for ControlKpiError {}

impl From<RuntimeError> for ControlKpiError {
    fn from(value: RuntimeError) -> Self {
        ControlKpiError::Runtime(value)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PidControlScenario {
    pub tick_ms: u64,
    pub duration_ms: u64,
    #[serde(default)]
    pub loop_index: usize,
    #[serde(default)]
    pub initial_pv: f32,
    pub model: ProcessModelConfig,
}

impl PidControlScenario {
    pub fn from_yaml_str(yaml: &str) -> Result<Self, ControlKpiError> {
        let mut docs = serde_yaml::Deserializer::from_str(yaml);
        let Some(doc) = docs.next() else {
            return Err(ControlKpiError::Parse {
                path: "<document>".to_string(),
                message: "empty yaml document".to_string(),
            });
        };

        let scenario: PidControlScenario =
            serde_path_to_error::deserialize(doc).map_err(|e| ControlKpiError::Parse {
                path: e.path().to_string(),
                message: e.into_inner().to_string(),
            })?;
        scenario.validate()?;
        Ok(scenario)
    }

    pub fn duration_ticks(&self) -> u64 {
        if self.tick_ms == 0 {
            0
        } else {
            (self.duration_ms + self.tick_ms - 1) / self.tick_ms
        }
    }

    fn validate(&self) -> Result<(), ControlKpiError> {
        if self.tick_ms == 0 {
            return Err(ControlKpiError::Validation {
                path: "tick_ms".to_string(),
                message: "must be > 0".to_string(),
            });
        }
        if self.duration_ms == 0 {
            return Err(ControlKpiError::Validation {
                path: "duration_ms".to_string(),
                message: "must be > 0".to_string(),
            });
        }
        self.model.validate()?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProcessModelConfig {
    FirstOrder {
        gain: f32,
        tau_ms: u64,
    },
    DeadTimeFirstOrder {
        gain: f32,
        tau_ms: u64,
        dead_time_ms: u64,
    },
}

impl ProcessModelConfig {
    fn validate(&self) -> Result<(), ControlKpiError> {
        match self {
            ProcessModelConfig::FirstOrder { tau_ms, .. } => {
                if *tau_ms == 0 {
                    return Err(ControlKpiError::Validation {
                        path: "model.tau_ms".to_string(),
                        message: "must be > 0".to_string(),
                    });
                }
            }
            ProcessModelConfig::DeadTimeFirstOrder { tau_ms, .. } => {
                if *tau_ms == 0 {
                    return Err(ControlKpiError::Validation {
                        path: "model.tau_ms".to_string(),
                        message: "must be > 0".to_string(),
                    });
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PidKpi {
    pub overshoot_percent: f32,
    pub settling_time_ms: Option<u64>,
    pub steady_state_error: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PidKpiReport {
    pub schema_version: u32,
    pub tick_ms: u64,
    pub duration_ms: u64,
    pub loop_index: usize,
    pub model: ProcessModelConfig,
    pub setpoint: f32,
    pub samples: usize,
    pub kpi: PidKpi,
}

pub fn run_pid_kpi(
    program: &Program<'_>,
    scenario: &PidControlScenario,
) -> Result<PidKpiReport, ControlKpiError> {
    let Some(pid) = program.pid_loops.get(scenario.loop_index).copied() else {
        return Err(ControlKpiError::MissingPidLoop {
            requested: scenario.loop_index,
            available: program.pid_loops.len(),
        });
    };

    let mut process = model_from_config(&scenario.model, scenario.tick_ms, scenario.initial_pv);
    let max_ai = (pid.pv.0 as usize + 1).max(1);
    let max_ao = (pid.out.0 as usize + 1).max(1);
    let mut io = ControlIo::new(max_ai, max_ao);
    io.set_analog_input(pid.pv, scenario.initial_pv);

    let mut rt = Runtime::new(program)?;
    let mut pv_samples = Vec::with_capacity(scenario.duration_ticks() as usize);

    for _ in 0..scenario.duration_ticks() {
        rt.tick(&mut io)?;
        let u = io.read_analog_output(pid.out);
        let y = process.step(u);
        io.set_analog_input(pid.pv, y);
        pv_samples.push(y);
    }

    let kpi = compute_pid_kpi(pid.sp, scenario.tick_ms, &pv_samples);
    Ok(PidKpiReport {
        schema_version: 1,
        tick_ms: scenario.tick_ms,
        duration_ms: scenario.duration_ms,
        loop_index: scenario.loop_index,
        model: scenario.model.clone(),
        setpoint: pid.sp,
        samples: pv_samples.len(),
        kpi,
    })
}

pub fn compute_pid_kpi(setpoint: f32, tick_ms: u64, pv_samples: &[f32]) -> PidKpi {
    if pv_samples.is_empty() {
        return PidKpi {
            overshoot_percent: 0.0,
            settling_time_ms: None,
            steady_state_error: setpoint.abs(),
        };
    }

    let steady_state_error = (setpoint - pv_samples[pv_samples.len() - 1]).abs();

    let overshoot_percent = if setpoint.abs() < 1e-6 {
        0.0
    } else if setpoint >= 0.0 {
        let peak = pv_samples.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        ((peak - setpoint).max(0.0) / setpoint.abs()) * 100.0
    } else {
        let trough = pv_samples.iter().copied().fold(f32::INFINITY, f32::min);
        ((setpoint - trough).max(0.0) / setpoint.abs()) * 100.0
    };

    let settle_band = (setpoint.abs() * 0.02).max(0.01);
    let mut settling_idx: Option<usize> = None;
    for start in 0..pv_samples.len() {
        if pv_samples[start..]
            .iter()
            .all(|v| (*v - setpoint).abs() <= settle_band)
        {
            settling_idx = Some(start);
            break;
        }
    }
    let settling_time_ms = settling_idx.map(|idx| (idx as u64 + 1) * tick_ms);

    PidKpi {
        overshoot_percent,
        settling_time_ms,
        steady_state_error,
    }
}

struct ControlIo {
    tick: Tick,
    ai: Vec<f32>,
    ao: Vec<f32>,
}

impl ControlIo {
    fn new(num_ai: usize, num_ao: usize) -> Self {
        Self {
            tick: Tick(0),
            ai: vec![0.0; num_ai],
            ao: vec![0.0; num_ao],
        }
    }

    fn set_analog_input(&mut self, id: AnalogInputId, value: f32) {
        if let Some(slot) = self.ai.get_mut(id.0 as usize) {
            *slot = value;
        }
    }

    fn read_analog_output(&self, id: AnalogOutputId) -> f32 {
        self.ao.get(id.0 as usize).copied().unwrap_or(0.0)
    }
}

impl Io for ControlIo {
    fn tick(&self) -> Tick {
        self.tick
    }

    fn advance_tick(&mut self) {
        self.tick.0 += 1;
    }

    fn read_digital_input(&self, _id: DigitalInputId) -> bool {
        false
    }

    fn read_analog_input(&self, id: AnalogInputId) -> f32 {
        self.ai.get(id.0 as usize).copied().unwrap_or(0.0)
    }

    fn write_digital_output(&mut self, _id: DigitalOutputId, _value: bool) {}

    fn write_analog_output(&mut self, id: AnalogOutputId, value: f32) {
        if let Some(slot) = self.ao.get_mut(id.0 as usize) {
            *slot = value;
        }
    }
}

trait ProcessModel {
    fn step(&mut self, u: f32) -> f32;
}

fn model_from_config(
    config: &ProcessModelConfig,
    tick_ms: u64,
    initial: f32,
) -> Box<dyn ProcessModel> {
    match config {
        ProcessModelConfig::FirstOrder { gain, tau_ms } => Box::new(FirstOrderModel {
            gain: *gain,
            tau_s: (*tau_ms as f32) / 1000.0,
            dt_s: (tick_ms as f32) / 1000.0,
            y: initial,
        }),
        ProcessModelConfig::DeadTimeFirstOrder {
            gain,
            tau_ms,
            dead_time_ms,
        } => {
            let delay_ticks = (*dead_time_ms + tick_ms - 1) / tick_ms;
            Box::new(DeadTimeFirstOrderModel {
                gain: *gain,
                tau_s: (*tau_ms as f32) / 1000.0,
                dt_s: (tick_ms as f32) / 1000.0,
                delay_ticks,
                delay: VecDeque::from(vec![0.0; delay_ticks as usize]),
                y: initial,
            })
        }
    }
}

struct FirstOrderModel {
    gain: f32,
    tau_s: f32,
    dt_s: f32,
    y: f32,
}

impl ProcessModel for FirstOrderModel {
    fn step(&mut self, u: f32) -> f32 {
        // dy/dt = (-y + K*u)/tau
        let tau = if self.tau_s > 1e-6 { self.tau_s } else { 1e-6 };
        self.y += (self.dt_s / tau) * (-self.y + self.gain * u);
        self.y
    }
}

struct DeadTimeFirstOrderModel {
    gain: f32,
    tau_s: f32,
    dt_s: f32,
    delay_ticks: u64,
    delay: VecDeque<f32>,
    y: f32,
}

impl ProcessModel for DeadTimeFirstOrderModel {
    fn step(&mut self, u: f32) -> f32 {
        let delayed_u = if self.delay_ticks == 0 {
            u
        } else {
            self.delay.push_back(u);
            self.delay.pop_front().unwrap_or(0.0)
        };

        let tau = if self.tau_s > 1e-6 { self.tau_s } else { 1e-6 };
        self.y += (self.dt_s / tau) * (-self.y + self.gain * delayed_u);
        self.y
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime_core::{AntiWindup, Instr, PidConfig, Program, Step, StepId, Task};

    fn program_with_single_pid() -> Program<'static> {
        static STEPS: [Step<'static>; 1] = [Step {
            name: "halt",
            instr: Instr::Halt,
        }];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static PID: [PidConfig; 1] = [PidConfig {
            pv: AnalogInputId(0),
            out: AnalogOutputId(0),
            sp: 0.8,
            kp: 2.0,
            ki: 0.8,
            kd: 0.0,
            dt_s: 0.1,
            period_ticks: 1,
            limit_min: 0.0,
            limit_max: 1.0,
            anti_windup: AntiWindup::ConditionalIntegration,
        }];
        Program {
            tasks: &TASKS,
            pid_loops: &PID,
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &[],
        }
    }

    #[test]
    fn dead_time_model_starts_responding_after_configured_delay() {
        let mut model = model_from_config(
            &ProcessModelConfig::DeadTimeFirstOrder {
                gain: 1.0,
                tau_ms: 1000,
                dead_time_ms: 300,
            },
            100,
            0.0,
        );

        let y0 = model.step(1.0);
        let y1 = model.step(1.0);
        let y2 = model.step(1.0);
        let y3 = model.step(1.0);

        assert_eq!(y0, 0.0);
        assert_eq!(y1, 0.0);
        assert_eq!(y2, 0.0);
        assert!(y3 > 0.0, "response should start after dead-time delay");
    }

    #[test]
    fn pid_kpi_is_deterministic_and_within_reasonable_thresholds() {
        let program = program_with_single_pid();
        let scenario = PidControlScenario {
            tick_ms: 100,
            duration_ms: 15_000,
            loop_index: 0,
            initial_pv: 0.0,
            model: ProcessModelConfig::FirstOrder {
                gain: 1.0,
                tau_ms: 1_200,
            },
        };

        let r1 = run_pid_kpi(&program, &scenario).expect("first run");
        let r2 = run_pid_kpi(&program, &scenario).expect("second run");

        assert_eq!(r1, r2, "same scenario should produce deterministic KPI");
        assert!(r1.kpi.overshoot_percent <= 20.0);
        assert!(r1.kpi.steady_state_error <= 0.2);
        assert!(
            r1.kpi
                .settling_time_ms
                .map(|ms| ms <= scenario.duration_ms)
                .unwrap_or(false),
            "response should settle within scenario horizon"
        );
    }
}
