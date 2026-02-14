#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use io_traits::{
    AnalogInputId, AnalogOutputId, DigitalInputId, DigitalOutputId, Io, Tick,
};
use runtime_core::{TraceEvent, TransitionReason};
use serde::Serialize;

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
pub enum InputChange {
    Digital { id: DigitalInputId, value: bool },
    Analog { id: AnalogInputId, value: f32 },
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

    scheduled: BTreeMap<u64, Vec<InputChange>>,

    digital_edges: Vec<DigitalEdge>,
    analog_edges: Vec<AnalogEdge>,
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
            scheduled: BTreeMap::new(),
            digital_edges: Vec::new(),
            analog_edges: Vec::new(),
        };
        s.apply_scheduled_for_current_tick();
        s
    }

    pub fn with_scheduled_changes(mut self, tick: Tick, changes: impl IntoIterator<Item = InputChange>) -> Self {
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

    pub fn analog_output_edges(&self) -> &[AnalogEdge] {
        &self.analog_edges
    }

    fn apply_scheduled_for_current_tick(&mut self) {
        let Some(changes) = self.scheduled.get(&self.tick.0) else {
            return;
        };
        for c in changes {
            match *c {
                InputChange::Digital { id, value } => {
                    if let Some(slot) = self.di.get_mut(id.0 as usize) {
                        *slot = value;
                    }
                }
                InputChange::Analog { id, value } => {
                    if let Some(slot) = self.ai.get_mut(id.0 as usize) {
                        *slot = value;
                    }
                }
            }
        }
    }
}

impl Io for SimIo {
    fn tick(&self) -> Tick {
        self.tick
    }

    fn advance_tick(&mut self) {
        self.tick.0 += 1;
        self.apply_scheduled_for_current_tick();
    }

    fn read_digital_input(&self, id: DigitalInputId) -> bool {
        self.di.get(id.0 as usize).copied().unwrap_or(false)
    }

    fn read_analog_input(&self, id: AnalogInputId) -> f32 {
        self.ai.get(id.0 as usize).copied().unwrap_or(0.0)
    }

    fn write_digital_output(&mut self, id: DigitalOutputId, value: bool) {
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

    fn write_analog_output(&mut self, id: AnalogOutputId, value: f32) {
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
    use runtime_core::{Action, Instr, Program, Runtime, Step, StepId, Task, Timeout, TraceEvent, TransitionReason};

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
        static PROGRAM: Program<'static> = Program { tasks: &TASKS };

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
}

