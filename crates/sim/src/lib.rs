#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use io_traits::{AnalogInputId, AnalogOutputId, DigitalInputId, DigitalOutputId, Io, Tick};
use runtime_core::{TraceEvent, TransitionReason};
use serde::Serialize;

mod plant;
mod report;
mod runner;
mod scenario;
mod waveform;
mod control_kpi;
pub use plant::{CylinderConfig, LimitKind, LimitSensorConfig, Plant, SolenoidValveConfig};
pub use report::{ScenarioSummary, SimFailure, SimReport};
pub use runner::{SimRunError, SimRunOutput, run_program_for_scenario};
pub use scenario::{
    DigitalBurstEvent, FaultEvent, InputEvent, InputSet, Scenario, ScenarioError, SensorStuckFault,
};
pub use waveform::{export_analog_outputs_csv, export_analog_outputs_jsonl, export_vcd_digital};
pub use control_kpi::{ControlKpiError, PidControlScenario, PidKpiReport, ProcessModelConfig, run_pid_kpi};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DigitalEdge {
    pub tick: Tick,
    pub id: DigitalOutputId,
    pub value: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnalogEdge {
    pub tick: Tick,
    pub id: AnalogOutputId,
    pub value: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DigitalInputEdge {
    pub tick: Tick,
    pub id: DigitalInputId,
    pub value: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputChange {
    Digital { id: DigitalInputId, value: bool },
    Analog { id: AnalogInputId, value: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FaultChange {
    SensorStuck { id: DigitalInputId, value: bool },
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ForceChange {
    DigitalInput {
        id: DigitalInputId,
        value: Option<bool>,
    },
    AnalogInput {
        id: AnalogInputId,
        value: Option<f32>,
    },
    DigitalOutput {
        id: DigitalOutputId,
        value: Option<bool>,
    },
    AnalogOutput {
        id: AnalogOutputId,
        value: Option<f32>,
    },
}

/// A simple deterministic in-memory `Io` implementation for SIL.
///
/// - Inputs can be scheduled to change at specific ticks.
/// - Output writes are recorded as edges (value changes only).
///
/// Scheduling semantics:
/// - Changes scheduled for tick `0` are applied immediately in `new()`.
/// - Changes scheduled for tick `N>0` are applied when `advance_tick()` moves to `N`,
///   so they are visible during the runtime's tick `N` evaluation.
#[derive(Debug, Clone)]
pub struct SimIo {
    tick: Tick,

    di: Vec<bool>,
    do_: Vec<bool>,
    ai: Vec<f32>,
    ao: Vec<f32>,

    // Persistent per-channel force overrides.
    // Inputs: affect read_* results (and have higher priority than plant/scheduled updates).
    forced_di: BTreeMap<u16, bool>,
    forced_ai: BTreeMap<u16, f32>,
    // Outputs: override program writes (and are reflected in recorded edges).
    forced_do: BTreeMap<u16, bool>,
    forced_ao: BTreeMap<u16, f32>,

    scheduled: BTreeMap<u64, Vec<InputChange>>,
    scheduled_faults: BTreeMap<u64, Vec<FaultChange>>,
    scheduled_forces: BTreeMap<u64, Vec<ForceChange>>,
    stuck_di: BTreeMap<u16, bool>,

    plant: Option<Plant>,

    digital_edges: Vec<DigitalEdge>,
    analog_edges: Vec<AnalogEdge>,
    digital_input_edges: Vec<DigitalInputEdge>,
}

impl SimIo {
    pub fn new(
        num_digital_inputs: usize,
        num_digital_outputs: usize,
        num_analog_inputs: usize,
        num_analog_outputs: usize,
    ) -> Self {
        let mut s = Self {
            tick: Tick(0),
            di: vec![false; num_digital_inputs],
            do_: vec![false; num_digital_outputs],
            ai: vec![0.0; num_analog_inputs],
            ao: vec![0.0; num_analog_outputs],
            forced_di: BTreeMap::new(),
            forced_ai: BTreeMap::new(),
            forced_do: BTreeMap::new(),
            forced_ao: BTreeMap::new(),
            scheduled: BTreeMap::new(),
            scheduled_faults: BTreeMap::new(),
            scheduled_forces: BTreeMap::new(),
            stuck_di: BTreeMap::new(),
            plant: None,
            digital_edges: Vec::new(),
            analog_edges: Vec::new(),
            digital_input_edges: Vec::new(),
        };
        s.apply_scheduled_for_current_tick();
        s.apply_forces_for_current_tick();
        s
    }

    pub fn with_plant(mut self, plant: Plant) -> Self {
        self.plant = Some(plant);
        // Ensure tick-0 plant state is applied to inputs before the first runtime tick.
        self.apply_plant_for_current_tick(Tick(0));
        self
    }

    pub fn with_scheduled_changes(
        mut self,
        tick: Tick,
        changes: impl IntoIterator<Item = InputChange>,
    ) -> Self {
        for c in changes {
            self.scheduled.entry(tick.0).or_default().push(c);
        }
        if tick.0 == self.tick.0 {
            self.apply_scheduled_for_current_tick();
        }
        self
    }

    pub fn schedule_digital_input(&mut self, tick: Tick, id: DigitalInputId, value: bool) {
        self.scheduled
            .entry(tick.0)
            .or_default()
            .push(InputChange::Digital { id, value });
        if tick.0 == self.tick.0 {
            self.apply_scheduled_for_current_tick();
        }
    }

    pub fn schedule_sensor_stuck(&mut self, tick: Tick, id: DigitalInputId, value: bool) {
        self.scheduled_faults
            .entry(tick.0)
            .or_default()
            .push(FaultChange::SensorStuck { id, value });
        if tick.0 == self.tick.0 {
            self.apply_faults_for_current_tick();
        }
    }

    pub fn schedule_analog_input(&mut self, tick: Tick, id: AnalogInputId, value: f32) {
        self.scheduled
            .entry(tick.0)
            .or_default()
            .push(InputChange::Analog { id, value });
        if tick.0 == self.tick.0 {
            self.apply_scheduled_for_current_tick();
        }
    }

    pub fn digital_output_edges(&self) -> &[DigitalEdge] {
        &self.digital_edges
    }

    pub fn digital_input_edges(&self) -> &[DigitalInputEdge] {
        &self.digital_input_edges
    }

    pub fn analog_output_edges(&self) -> &[AnalogEdge] {
        &self.analog_edges
    }

    pub fn force_digital_input(&mut self, id: DigitalInputId, value: Option<bool>) {
        match value {
            Some(v) => {
                self.forced_di.insert(id.0, v);
            }
            None => {
                self.forced_di.remove(&id.0);
            }
        }
    }

    pub fn schedule_force_digital_input(&mut self, tick: Tick, id: DigitalInputId, value: Option<bool>) {
        self.scheduled_forces
            .entry(tick.0)
            .or_default()
            .push(ForceChange::DigitalInput { id, value });
        if tick.0 == self.tick.0 {
            self.apply_forces_for_current_tick();
        }
    }

    pub fn schedule_force_analog_input(&mut self, tick: Tick, id: AnalogInputId, value: Option<f32>) {
        self.scheduled_forces
            .entry(tick.0)
            .or_default()
            .push(ForceChange::AnalogInput { id, value });
        if tick.0 == self.tick.0 {
            self.apply_forces_for_current_tick();
        }
    }

    pub fn schedule_force_digital_output(
        &mut self,
        tick: Tick,
        id: DigitalOutputId,
        value: Option<bool>,
    ) {
        self.scheduled_forces
            .entry(tick.0)
            .or_default()
            .push(ForceChange::DigitalOutput { id, value });
        if tick.0 == self.tick.0 {
            self.apply_forces_for_current_tick();
        }
    }

    pub fn schedule_force_analog_output(
        &mut self,
        tick: Tick,
        id: AnalogOutputId,
        value: Option<f32>,
    ) {
        self.scheduled_forces
            .entry(tick.0)
            .or_default()
            .push(ForceChange::AnalogOutput { id, value });
        if tick.0 == self.tick.0 {
            self.apply_forces_for_current_tick();
        }
    }

    pub fn force_analog_input(&mut self, id: AnalogInputId, value: Option<f32>) {
        match value {
            Some(v) => {
                self.forced_ai.insert(id.0, v);
            }
            None => {
                self.forced_ai.remove(&id.0);
            }
        }
    }

    pub fn force_digital_output(&mut self, id: DigitalOutputId, value: Option<bool>) {
        match value {
            Some(v) => {
                self.forced_do.insert(id.0, v);
                self.set_digital_output_final(id, v);
            }
            None => {
                self.forced_do.remove(&id.0);
            }
        }
    }

    pub fn force_analog_output(&mut self, id: AnalogOutputId, value: Option<f32>) {
        match value {
            Some(v) => {
                self.forced_ao.insert(id.0, v);
                self.set_analog_output_final(id, v);
            }
            None => {
                self.forced_ao.remove(&id.0);
            }
        }
    }

    pub fn num_digital_inputs(&self) -> usize {
        self.di.len()
    }

    pub fn num_digital_outputs(&self) -> usize {
        self.do_.len()
    }

    pub fn num_analog_outputs(&self) -> usize {
        self.ao.len()
    }

    fn apply_scheduled_for_current_tick(&mut self) {
        let Some(changes) = self.scheduled.remove(&self.tick.0) else {
            return;
        };
        for c in changes {
            match c {
                InputChange::Digital { id, value } => {
                    self.set_digital_input(id, value);
                }
                InputChange::Analog { id, value } => {
                    if let Some(slot) = self.ai.get_mut(id.0 as usize) {
                        *slot = value;
                    }
                }
            }
        }
    }

    fn apply_faults_for_current_tick(&mut self) {
        let Some(changes) = self.scheduled_faults.remove(&self.tick.0) else {
            return;
        };
        for c in changes {
            match c {
                FaultChange::SensorStuck { id, value } => {
                    self.stuck_di.insert(id.0, value);
                    // Enforce the stuck value immediately (so it also affects tick-0).
                    self.set_digital_input(id, value);
                }
            }
        }
    }

    fn apply_forces_for_current_tick(&mut self) {
        let Some(changes) = self.scheduled_forces.remove(&self.tick.0) else {
            return;
        };
        for c in changes {
            match c {
                ForceChange::DigitalInput { id, value } => self.force_digital_input(id, value),
                ForceChange::AnalogInput { id, value } => self.force_analog_input(id, value),
                ForceChange::DigitalOutput { id, value } => self.force_digital_output(id, value),
                ForceChange::AnalogOutput { id, value } => self.force_analog_output(id, value),
            }
        }
    }

    fn set_digital_input(&mut self, id: DigitalInputId, value: bool) {
        // Faults/stuck override any normal input updates for the tick.
        // (Force is handled at read-time so plant/scheduled updates keep progressing underneath.)
        let value = self.stuck_di.get(&id.0).copied().unwrap_or(value);
        let idx = id.0 as usize;
        if let Some(slot) = self.di.get_mut(idx) {
            let prev = *slot;
            *slot = value;
            if prev != value {
                self.digital_input_edges.push(DigitalInputEdge {
                    tick: self.tick,
                    id,
                    value,
                });
            }
        }
    }

    fn set_digital_output_final(&mut self, id: DigitalOutputId, value: bool) {
        let idx = id.0 as usize;
        let prev = self.do_.get(idx).copied().unwrap_or(false);
        if let Some(slot) = self.do_.get_mut(idx) {
            *slot = value;
        }
        if prev != value {
            self.digital_edges.push(DigitalEdge {
                tick: self.tick,
                id,
                value,
            });
        }
    }

    fn set_analog_output_final(&mut self, id: AnalogOutputId, value: f32) {
        let idx = id.0 as usize;
        let prev = self.ao.get(idx).copied().unwrap_or(0.0);
        if let Some(slot) = self.ao.get_mut(idx) {
            *slot = value;
        }
        if prev != value {
            self.analog_edges.push(AnalogEdge {
                tick: self.tick,
                id,
                value,
            });
        }
    }

    fn apply_plant_for_current_tick(&mut self, prev_tick: Tick) {
        let now = self.tick;
        let Some(plant) = self.plant.as_mut() else {
            return;
        };
        let updates = plant.advance_tick(prev_tick, now, &self.do_);
        for u in updates {
            self.set_digital_input(u.id, u.value);
        }
    }
}

impl Io for SimIo {
    fn tick(&self) -> Tick {
        self.tick
    }

    fn advance_tick(&mut self) {
        let prev = self.tick;
        self.tick.0 += 1;
        self.apply_scheduled_for_current_tick();
        // Let the plant update sensor inputs for the new tick.
        self.apply_plant_for_current_tick(prev);
        // Faults override any normal input updates for the tick.
        self.apply_faults_for_current_tick();
        // Force events are applied at tick boundaries and persist until explicitly cleared.
        self.apply_forces_for_current_tick();
    }

    fn read_digital_input(&self, id: DigitalInputId) -> bool {
        // Priority: force input > plant update > scheduled inputs.
        if let Some(v) = self.forced_di.get(&id.0) {
            return *v;
        }
        if let Some(v) = self.stuck_di.get(&id.0) {
            return *v;
        }
        self.di.get(id.0 as usize).copied().unwrap_or(false)
    }

    fn read_analog_input(&self, id: AnalogInputId) -> f32 {
        // Priority: force input > plant update > scheduled inputs.
        if let Some(v) = self.forced_ai.get(&id.0) {
            return *v;
        }
        self.ai.get(id.0 as usize).copied().unwrap_or(0.0)
    }

    fn write_digital_output(&mut self, id: DigitalOutputId, value: bool) {
        // Force outputs have priority over program writes.
        if let Some(forced) = self.forced_do.get(&id.0).copied() {
            self.set_digital_output_final(id, forced);
            return;
        }
        self.set_digital_output_final(id, value);
    }

    fn write_analog_output(&mut self, id: AnalogOutputId, value: f32) {
        // Force outputs have priority over program writes.
        if let Some(forced) = self.forced_ao.get(&id.0).copied() {
            self.set_analog_output_final(id, forced);
            return;
        }
        self.set_analog_output_final(id, value);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TraceJsonlRecord {
    pub tick: u64,
    pub task: usize,
    pub from_step: u16,
    pub to_step: u16,
    pub reason: &'static str,
}

/// Minimal JSONL trace recorder for `runtime-core` transitions.
#[derive(Debug, Default)]
pub struct JsonlTraceRecorder {
    lines: Vec<String>,
}

impl JsonlTraceRecorder {
    pub fn new() -> Self {
        Self { lines: Vec::new() }
    }

    pub fn record(&mut self, e: TraceEvent) {
        let rec = TraceJsonlRecord {
            tick: e.tick.0,
            task: e.task,
            from_step: e.from.0,
            to_step: e.to.0,
            reason: reason_str(e.reason),
        };
        // A single record per line.
        let line = serde_json::to_string(&rec).expect("trace record serializes");
        self.lines.push(line);
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn into_string(self) -> String {
        if self.lines.is_empty() {
            return String::new();
        }
        let mut out = self.lines.join("\n");
        out.push('\n');
        out
    }
}

fn reason_str(r: TransitionReason) -> &'static str {
    match r {
        TransitionReason::Action => "action",
        TransitionReason::DelayElapsed => "delay_elapsed",
        TransitionReason::WaitSatisfied => "wait_satisfied",
        TransitionReason::Timeout => "timeout",
        TransitionReason::Goto => "goto",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime_core::{
        Action, Instr, Program, Runtime, Step, StepId, Task, Timeout, TraceEvent, TransitionReason,
    };

    #[test]
    fn simio_records_output_edges_and_jsonl_trace() {
        static STEP0_ACTIONS: [Action; 1] = [Action::SetDigital {
            id: DigitalOutputId(0),
            value: true,
        }];
        static STEP2_ACTIONS: [Action; 1] = [Action::SetDigital {
            id: DigitalOutputId(0),
            value: false,
        }];

        static STEPS: [Step<'static>; 5] = [
            Step {
                name: "set_do0_true",
                instr: Instr::Action {
                    actions: &STEP0_ACTIONS,
                    next: StepId(1),
                },
            },
            Step {
                name: "delay1",
                instr: Instr::Delay {
                    ticks: 1,
                    next: StepId(2),
                },
            },
            Step {
                name: "set_do0_false",
                instr: Instr::Action {
                    actions: &STEP2_ACTIONS,
                    next: StepId(3),
                },
            },
            Step {
                name: "wait_di0_true",
                instr: Instr::WaitDigital {
                    id: DigitalInputId(0),
                    equals: true,
                    next: StepId(4),
                    timeout: Some(Timeout {
                        after_ticks: 99,
                        target: StepId(4),
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
        static PROGRAM: Program<'static> = Program { tasks: &TASKS, pid_loops: &[] };

        let mut io = SimIo::new(1, 1, 0, 0);
        io.schedule_digital_input(Tick(2), DigitalInputId(0), true);

        let mut rt = Runtime::new(&PROGRAM).unwrap();
        let mut trace = JsonlTraceRecorder::new();
        let mut raw_events: Vec<TraceEvent> = Vec::new();

        // Tick 0: action -> delay (not done yet)
        rt.tick_with_trace(&mut io, |e| {
            trace.record(e);
            raw_events.push(e);
        })
        .unwrap();
        // Tick 1: delay completes, then action, then wait blocks
        rt.tick_with_trace(&mut io, |e| {
            trace.record(e);
            raw_events.push(e);
        })
        .unwrap();
        // Tick 2: scheduled input becomes true (via `advance_tick` at end of tick 1), wait completes.
        rt.tick_with_trace(&mut io, |e| {
            trace.record(e);
            raw_events.push(e);
        })
        .unwrap();

        assert_eq!(
            io.digital_output_edges(),
            &[
                DigitalEdge {
                    tick: Tick(0),
                    id: DigitalOutputId(0),
                    value: true,
                },
                DigitalEdge {
                    tick: Tick(1),
                    id: DigitalOutputId(0),
                    value: false,
                }
            ]
        );

        assert_eq!(
            raw_events,
            vec![
                TraceEvent {
                    tick: Tick(0),
                    task: 0,
                    from: StepId(0),
                    to: StepId(1),
                    reason: TransitionReason::Action,
                },
                TraceEvent {
                    tick: Tick(1),
                    task: 0,
                    from: StepId(1),
                    to: StepId(2),
                    reason: TransitionReason::DelayElapsed,
                },
                TraceEvent {
                    tick: Tick(1),
                    task: 0,
                    from: StepId(2),
                    to: StepId(3),
                    reason: TransitionReason::Action,
                },
                TraceEvent {
                    tick: Tick(2),
                    task: 0,
                    from: StepId(3),
                    to: StepId(4),
                    reason: TransitionReason::WaitSatisfied,
                },
            ]
        );

        // Sanity: should be valid JSONL with same number of lines as events.
        assert_eq!(trace.lines().len(), raw_events.len());
        for (line, e) in trace.lines().iter().zip(raw_events.iter()) {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(v["tick"], e.tick.0);
            assert_eq!(v["task"], e.task);
        }
    }

    #[test]
    fn force_di_overrides_plant_updates_until_cleared() {
        // A tiny plant: DO0 energizes a valve -> cylinder -> DI0 extended limit sensor.
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

        let mut io = SimIo::new(1, 1, 0, 0).with_plant(plant);
        io.force_digital_input(DigitalInputId(0), Some(false));
        io.write_digital_output(DigitalOutputId(0), true);

        for _ in 0..10 {
            assert_eq!(io.read_digital_input(DigitalInputId(0)), false);
            io.advance_tick();
        }

        // Once cleared, reads should reflect the latest plant-updated value.
        io.force_digital_input(DigitalInputId(0), None);
        assert_eq!(io.read_digital_input(DigitalInputId(0)), true);
    }

    #[test]
    fn force_ai_overrides_scheduled_inputs_until_cleared() {
        let mut io = SimIo::new(0, 0, 1, 0);
        io.schedule_analog_input(Tick(0), AnalogInputId(0), 1.25);
        io.force_analog_input(AnalogInputId(0), Some(9.0));

        assert_eq!(io.read_analog_input(AnalogInputId(0)), 9.0);

        io.force_analog_input(AnalogInputId(0), None);
        assert_eq!(io.read_analog_input(AnalogInputId(0)), 1.25);
    }

    #[test]
    fn force_outputs_override_program_writes_and_edges_reflect_final_values() {
        let mut io = SimIo::new(0, 1, 0, 1);

        // Program writes take effect initially.
        io.write_digital_output(DigitalOutputId(0), true);
        io.write_analog_output(AnalogOutputId(0), 1.0);
        assert_eq!(io.digital_output_edges().len(), 1);
        assert_eq!(io.analog_output_edges().len(), 1);
        assert_eq!(io.digital_output_edges()[0].value, true);
        assert_eq!(io.analog_output_edges()[0].value, 1.0);

        // Force overrides and is recorded as an edge (final values).
        io.force_digital_output(DigitalOutputId(0), Some(false));
        io.force_analog_output(AnalogOutputId(0), Some(0.5));
        assert_eq!(io.digital_output_edges().len(), 2);
        assert_eq!(io.analog_output_edges().len(), 2);
        assert_eq!(io.digital_output_edges()[1].value, false);
        assert_eq!(io.analog_output_edges()[1].value, 0.5);

        // Further program writes are ignored while forced; no extra edges.
        io.write_digital_output(DigitalOutputId(0), true);
        io.write_analog_output(AnalogOutputId(0), 1.0);
        assert_eq!(io.digital_output_edges().len(), 2);
        assert_eq!(io.analog_output_edges().len(), 2);
    }
}
