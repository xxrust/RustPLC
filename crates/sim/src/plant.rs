#![forbid(unsafe_code)]

use io_traits::{DigitalInputId, Tick};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitKind {
    Retracted,
    Extended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValveCommand {
    Off,
    Extend,
    Retract,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValveState {
    Off,
    Extend,
    Retract,
}

impl ValveCommand {
    fn to_state(self) -> ValveState {
        match self {
            ValveCommand::Off => ValveState::Off,
            ValveCommand::Extend => ValveState::Extend,
            ValveCommand::Retract => ValveState::Retract,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingValve {
    effective_tick: u64,
    state: ValveState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SolenoidValveConfig {
    /// Digital output index for the extend coil.
    pub extend_coil: usize,
    /// Optional digital output index for the retract coil.
    pub retract_coil: Option<usize>,
    /// Response time before the valve state changes (in ticks).
    pub response_ticks: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SolenoidValve {
    cfg: SolenoidValveConfig,
    last_cmd: ValveCommand,
    state: ValveState,
    pending: Option<PendingValve>,
}

impl SolenoidValve {
    fn new(cfg: SolenoidValveConfig) -> Self {
        Self {
            cfg,
            last_cmd: ValveCommand::Off,
            // Default to retract so a fresh plant is in a known "safe" state.
            state: ValveState::Retract,
            pending: None,
        }
    }

    fn observe_outputs(&mut self, prev_tick: Tick, digital_outputs: &[bool]) {
        let ext = digital_outputs
            .get(self.cfg.extend_coil)
            .copied()
            .unwrap_or(false);
        let ret = self
            .cfg
            .retract_coil
            .and_then(|idx| digital_outputs.get(idx).copied())
            .unwrap_or(false);

        // If both coils are on, prefer extend.
        let cmd = if ext {
            ValveCommand::Extend
        } else if ret {
            ValveCommand::Retract
        } else {
            ValveCommand::Off
        };

        if cmd != self.last_cmd {
            self.last_cmd = cmd;
            self.pending = Some(PendingValve {
                // We only observe the final output values at the end of `prev_tick`,
                // so model the command change as happening at `prev_tick`.
                effective_tick: prev_tick.0.saturating_add(self.cfg.response_ticks),
                state: cmd.to_state(),
            });
        }
    }

    fn apply_pending_up_to(&mut self, tick: Tick) {
        if let Some(p) = self.pending {
            if p.effective_tick <= tick.0 {
                self.state = p.state;
                self.pending = None;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CylinderConfig {
    pub valve: usize,
    pub stroke_ticks: u64,
    pub retract_ticks: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MotionDir {
    Extend,
    Retract,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CylinderState {
    Retracted,
    Extended,
    Moving { dir: MotionDir, remaining: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Cylinder {
    cfg: CylinderConfig,
    state: CylinderState,
}

impl Cylinder {
    fn new(cfg: CylinderConfig) -> Self {
        Self {
            cfg,
            state: CylinderState::Retracted,
        }
    }

    fn desired_from_valve_state(v: ValveState) -> Option<MotionDir> {
        match v {
            ValveState::Extend => Some(MotionDir::Extend),
            ValveState::Retract => Some(MotionDir::Retract),
            ValveState::Off => None,
        }
    }

    fn start_motion(&mut self, dir: MotionDir) {
        let ticks = match dir {
            MotionDir::Extend => self.cfg.stroke_ticks,
            MotionDir::Retract => self.cfg.retract_ticks,
        };

        if ticks == 0 {
            self.state = match dir {
                MotionDir::Extend => CylinderState::Extended,
                MotionDir::Retract => CylinderState::Retracted,
            };
            return;
        }

        self.state = CylinderState::Moving {
            dir,
            remaining: ticks,
        };
    }

    fn tick_one(&mut self, valve_state: ValveState) {
        let desired = Self::desired_from_valve_state(valve_state);

        match (self.state, desired) {
            (CylinderState::Retracted, Some(MotionDir::Extend)) => {
                self.start_motion(MotionDir::Extend)
            }
            (CylinderState::Extended, Some(MotionDir::Retract)) => {
                self.start_motion(MotionDir::Retract)
            }
            (CylinderState::Moving { dir, remaining }, Some(want)) => {
                if dir != want {
                    self.start_motion(want);
                } else {
                    self.state = CylinderState::Moving { dir, remaining };
                }
            }
            // If no desired motion, keep current state (including mid-motion).
            _ => {}
        }

        // Advance any ongoing motion by one tick.
        if let CylinderState::Moving { dir, remaining } = self.state {
            if remaining <= 1 {
                self.state = match dir {
                    MotionDir::Extend => CylinderState::Extended,
                    MotionDir::Retract => CylinderState::Retracted,
                };
            } else {
                self.state = CylinderState::Moving {
                    dir,
                    remaining: remaining - 1,
                };
            }
        }
    }

    fn is_extended(&self) -> bool {
        self.state == CylinderState::Extended
    }

    fn is_retracted(&self) -> bool {
        self.state == CylinderState::Retracted
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimitSensorConfig {
    pub cylinder: usize,
    pub kind: LimitKind,
    pub digital_input: DigitalInputId,
    pub debounce_ticks: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LimitSensor {
    cfg: LimitSensorConfig,
    debounced: bool,
    last_raw: bool,
    stable_ticks: u64,
}

impl LimitSensor {
    fn new(cfg: LimitSensorConfig) -> Self {
        Self {
            cfg,
            debounced: false,
            last_raw: false,
            stable_ticks: 0,
        }
    }

    fn raw(&self, cyl: &Cylinder) -> bool {
        match self.cfg.kind {
            LimitKind::Retracted => cyl.is_retracted(),
            LimitKind::Extended => cyl.is_extended(),
        }
    }

    fn tick_one(&mut self, cyl: &Cylinder) -> bool {
        let raw = self.raw(cyl);

        if self.cfg.debounce_ticks == 0 {
            self.debounced = raw;
            self.last_raw = raw;
            self.stable_ticks = 0;
            return self.debounced;
        }

        if raw == self.last_raw {
            self.stable_ticks = self.stable_ticks.saturating_add(1);
        } else {
            self.last_raw = raw;
            self.stable_ticks = 1;
        }

        if raw != self.debounced && self.stable_ticks >= self.cfg.debounce_ticks {
            self.debounced = raw;
        }

        self.debounced
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DigitalInputUpdate {
    pub id: DigitalInputId,
    pub value: bool,
}

/// Minimal plant model: solenoid valve -> cylinder -> limit sensor.
///
/// This is intentionally tick-based and deterministic.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Plant {
    valves: Vec<SolenoidValve>,
    cylinders: Vec<Cylinder>,
    sensors: Vec<LimitSensor>,
}

impl Plant {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_solenoid_valve(&mut self, cfg: SolenoidValveConfig) -> usize {
        let idx = self.valves.len();
        self.valves.push(SolenoidValve::new(cfg));
        idx
    }

    pub fn add_cylinder(&mut self, cfg: CylinderConfig) -> usize {
        let idx = self.cylinders.len();
        self.cylinders.push(Cylinder::new(cfg));
        idx
    }

    pub fn add_limit_sensor(&mut self, cfg: LimitSensorConfig) -> usize {
        let idx = self.sensors.len();
        self.sensors.push(LimitSensor::new(cfg));
        idx
    }

    /// Advance the plant from `prev_tick` to `now_tick` (usually `prev_tick + 1`).
    ///
    /// The returned updates should be applied to digital inputs for `now_tick`.
    pub fn advance_tick(
        &mut self,
        prev_tick: Tick,
        now_tick: Tick,
        digital_outputs: &[bool],
    ) -> Vec<DigitalInputUpdate> {
        // Observe valve commands at the end of the previous tick.
        for v in &mut self.valves {
            v.observe_outputs(prev_tick, digital_outputs);
        }

        // If `now_tick == prev_tick` (e.g., when attaching the plant at tick 0),
        // skip motion but still refresh sensors deterministically.
        if now_tick.0 > prev_tick.0 {
            // Apply any valve pending state changes that are already effective by `prev_tick`,
            // so motion during (prev_tick, now_tick] uses the correct state.
            for v in &mut self.valves {
                v.apply_pending_up_to(prev_tick);
            }

            // Advance cylinder motion by one tick.
            for c in &mut self.cylinders {
                let v_state = self
                    .valves
                    .get(c.cfg.valve)
                    .map(|v| v.state)
                    .unwrap_or(ValveState::Off);
                c.tick_one(v_state);
            }

            // Apply any valve state changes that become effective at the new tick boundary.
            for v in &mut self.valves {
                v.apply_pending_up_to(now_tick);
            }
        }

        // Update sensors (with debounce).
        let mut updates = Vec::with_capacity(self.sensors.len());
        for s in &mut self.sensors {
            let cyl = self
                .cylinders
                .get(s.cfg.cylinder)
                .expect("sensor cylinder exists");
            let v = s.tick_one(cyl);
            updates.push(DigitalInputUpdate {
                id: s.cfg.digital_input,
                value: v,
            });
        }

        updates
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SimIo;
    use io_traits::DigitalOutputId;
    use runtime_core::{
        Action, Instr, Program, Runtime, Step, StepId, Task, Timeout, TransitionReason,
    };

    #[test]
    fn plant_valve_cylinder_sensor_chain_satisfies_wait_in_expected_window() {
        // Topology:
        // - DO0 energizes the valve's extend coil.
        // - Valve drives a cylinder.
        // - DI0 is the cylinder's extended limit sensor.

        let mut plant = Plant::new();
        let valve = plant.add_solenoid_valve(SolenoidValveConfig {
            extend_coil: 0,
            retract_coil: None,
            response_ticks: 1,
        });
        let cyl = plant.add_cylinder(CylinderConfig {
            valve,
            stroke_ticks: 3,
            retract_ticks: 3,
        });
        plant.add_limit_sensor(LimitSensorConfig {
            cylinder: cyl,
            kind: LimitKind::Extended,
            digital_input: DigitalInputId(0),
            debounce_ticks: 1,
        });

        static STEP0_ACTIONS: [Action; 1] = [Action::Extend {
            output: DigitalOutputId(0),
        }];
        static STEPS: [Step<'static>; 3] = [
            Step {
                name: "extend",
                instr: Instr::Action {
                    actions: &STEP0_ACTIONS,
                    next: StepId(1),
                },
            },
            Step {
                name: "wait_extended",
                instr: Instr::WaitDigital {
                    id: DigitalInputId(0),
                    equals: true,
                    next: StepId(2),
                    timeout: Some(Timeout {
                        after_ticks: 99,
                        target: StepId(2),
                    }),
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

        let mut io = SimIo::new(1, 1, 0, 0).with_plant(plant);
        let mut rt = Runtime::new(&PROGRAM).unwrap();

        let mut wait_satisfied_at: Option<u64> = None;
        for _ in 0..10 {
            rt.tick_with_trace(&mut io, |e| {
                if e.reason == TransitionReason::WaitSatisfied {
                    wait_satisfied_at = Some(e.tick.0);
                }
            })
            .unwrap();

            if rt.location().step == StepId(2) {
                break;
            }
        }

        // response_ticks=1, stroke_ticks=3 => wait satisfies at tick 4.
        assert_eq!(wait_satisfied_at, Some(4));
        assert_eq!(rt.location().step, StepId(2));
    }
}
