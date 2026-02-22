#![no_std]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate std;

use io_traits::{AnalogInputId, AnalogOutputId, DigitalInputId, DigitalOutputId, Io, Tick};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StepId(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionReason {
    Action,
    DelayElapsed,
    WaitSatisfied,
    Timeout,
    Goto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceEvent {
    pub tick: Tick,
    pub task: usize,
    pub from: StepId,
    pub to: StepId,
    pub reason: TransitionReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogEvent {
    pub tick: Tick,
    pub task: usize,
    pub step: StepId,
    pub message_id: u16,
    pub message: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeError {
    ProgramHasNoTasks,
    InvalidTaskIndex { task: usize },
    InvalidStepId { task: usize, step: StepId },
    TooManyTransitionsInOneTick,
    TooManyPidLoops { configured: usize, max: usize },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    SetDigital {
        id: DigitalOutputId,
        value: bool,
    },
    SetAnalog {
        id: AnalogOutputId,
        value: f32,
    },
    Extend {
        output: DigitalOutputId,
    },
    Retract {
        output: DigitalOutputId,
    },
    Log {
        message_id: u16,
        message: &'static str,
    },
}

impl Action {
    pub fn apply<IO: Io>(&self, io: &mut IO) {
        match *self {
            Action::SetDigital { id, value } => io.write_digital_output(id, value),
            Action::SetAnalog { id, value } => io.write_analog_output(id, value),
            Action::Extend { output } => io.write_digital_output(output, true),
            Action::Retract { output } => io.write_digital_output(output, false),
            Action::Log { .. } => {}
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeout {
    pub after_ticks: u64,
    pub target: StepId,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnalogRange {
    pub min: f32,
    pub max: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AntiWindup {
    /// Conditional integration (a.k.a. "integrator clamping"):
    /// - If the controller output is saturated and the error would push it further into saturation,
    ///   the integrator is not updated for that cycle.
    ConditionalIntegration,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PidConfig {
    pub pv: AnalogInputId,
    pub out: AnalogOutputId,
    pub sp: f32,
    pub kp: f32,
    pub ki: f32,
    pub kd: f32,
    /// Discrete integration/derivative timestep in seconds.
    pub dt_s: f32,
    /// Execute controller when `now_tick - last_tick >= period_ticks`.
    pub period_ticks: u64,
    pub limit_min: f32,
    pub limit_max: f32,
    pub anti_windup: AntiWindup,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Instr<'a> {
    Action {
        actions: &'a [Action],
        next: StepId,
    },
    WaitDigital {
        id: DigitalInputId,
        equals: bool,
        next: StepId,
        timeout: Option<Timeout>,
    },
    WaitAnalog {
        id: AnalogInputId,
        ranges: &'a [AnalogRange],
        next: StepId,
        timeout: Option<Timeout>,
    },
    Delay {
        ticks: u64,
        next: StepId,
    },
    Goto {
        target: StepId,
    },
    Halt,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step<'a> {
    pub name: &'a str,
    pub instr: Instr<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Task<'a> {
    pub name: &'a str,
    pub steps: &'a [Step<'a>],
    pub entry: StepId,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Program<'a> {
    pub tasks: &'a [Task<'a>],
    pub pid_loops: &'a [PidConfig],
}

impl<'a> Program<'a> {
    pub fn task(&self, index: usize) -> Result<&Task<'a>, RuntimeError> {
        self.tasks
            .get(index)
            .ok_or(RuntimeError::InvalidTaskIndex { task: index })
    }
}

impl<'a> Task<'a> {
    pub fn step(&self, id: StepId) -> Option<&Step<'a>> {
        self.steps.get(id.0 as usize)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    pub task: usize,
    pub step: StepId,
}

/// A minimal deterministic tick executor.
///
/// - One call to `tick()` consumes exactly one `Io` tick (it calls `Io::advance_tick()`).
/// - Within a tick, non-blocking steps (`Action`, `Goto`, and completed `Delay`/`Wait`) may chain.
pub struct Runtime<'a> {
    program: &'a Program<'a>,
    loc: Location,
    step_entered_at: Option<Tick>,
    pid_states: [PidState; MAX_PID_LOOPS],
}

impl<'a> Runtime<'a> {
    pub fn new(program: &'a Program<'a>) -> Result<Self, RuntimeError> {
        if program.tasks.is_empty() {
            return Err(RuntimeError::ProgramHasNoTasks);
        }
        if program.pid_loops.len() > MAX_PID_LOOPS {
            return Err(RuntimeError::TooManyPidLoops {
                configured: program.pid_loops.len(),
                max: MAX_PID_LOOPS,
            });
        }

        let entry = program.task(0)?.entry;
        Ok(Self {
            program,
            loc: Location {
                task: 0,
                step: entry,
            },
            step_entered_at: None,
            pid_states: [PidState::default(); MAX_PID_LOOPS],
        })
    }

    pub fn location(&self) -> Location {
        self.loc
    }

    pub fn tick<IO: Io>(&mut self, io: &mut IO) -> Result<(), RuntimeError> {
        self.tick_with_trace_and_logs(io, |_| {}, |_| {})
    }

    pub fn tick_with_trace<IO: Io>(
        &mut self,
        io: &mut IO,
        mut on_event: impl FnMut(TraceEvent),
    ) -> Result<(), RuntimeError> {
        self.tick_with_trace_and_logs(io, |e| on_event(e), |_| {})
    }

    pub fn tick_with_trace_and_logs<IO: Io>(
        &mut self,
        io: &mut IO,
        mut on_event: impl FnMut(TraceEvent),
        mut on_log: impl FnMut(LogEvent),
    ) -> Result<(), RuntimeError> {
        let now = io.tick();
        if self.step_entered_at.is_none() {
            self.step_entered_at = Some(now);
        }

        // PID loops are executed once per tick before state-machine evaluation. This keeps the
        // execution deterministic, and allows task actions to override the output when needed.
        self.update_pid_loops(now, io);

        let mut transitions = 0usize;
        loop {
            transitions += 1;
            if transitions > 64 {
                return Err(RuntimeError::TooManyTransitionsInOneTick);
            }

            let task = self.program.task(self.loc.task)?;
            let Some(step) = task.step(self.loc.step) else {
                return Err(RuntimeError::InvalidStepId {
                    task: self.loc.task,
                    step: self.loc.step,
                });
            };

            let entered_at = self.step_entered_at.unwrap_or(now);
            let elapsed = now.0.saturating_sub(entered_at.0);

            match step.instr {
                Instr::Action { actions, next } => {
                    for a in actions {
                        match *a {
                            Action::Log {
                                message_id,
                                message,
                            } => on_log(LogEvent {
                                tick: now,
                                task: self.loc.task,
                                step: self.loc.step,
                                message_id,
                                message,
                            }),
                            _ => a.apply(io),
                        }
                    }
                    self.transition(now, next, TransitionReason::Action, &mut on_event)?;
                    continue;
                }
                Instr::Goto { target } => {
                    self.transition(now, target, TransitionReason::Goto, &mut on_event)?;
                    continue;
                }
                Instr::Delay { ticks, next } => {
                    if elapsed >= ticks {
                        self.transition(now, next, TransitionReason::DelayElapsed, &mut on_event)?;
                        continue;
                    }
                    break;
                }
                Instr::WaitDigital {
                    id,
                    equals,
                    next,
                    timeout,
                } => {
                    let v = io.read_digital_input(id);
                    if v == equals {
                        self.transition(now, next, TransitionReason::WaitSatisfied, &mut on_event)?;
                        continue;
                    }
                    if let Some(tmo) = timeout {
                        if elapsed >= tmo.after_ticks {
                            self.transition(
                                now,
                                tmo.target,
                                TransitionReason::Timeout,
                                &mut on_event,
                            )?;
                            continue;
                        }
                    }
                    break;
                }
                Instr::WaitAnalog {
                    id,
                    ranges,
                    next,
                    timeout,
                } => {
                    let v = io.read_analog_input(id);
                    if analog_in_selected_ranges(v, ranges) {
                        self.transition(now, next, TransitionReason::WaitSatisfied, &mut on_event)?;
                        continue;
                    }
                    if let Some(tmo) = timeout {
                        if elapsed >= tmo.after_ticks {
                            self.transition(
                                now,
                                tmo.target,
                                TransitionReason::Timeout,
                                &mut on_event,
                            )?;
                            continue;
                        }
                    }
                    break;
                }
                Instr::Halt => break,
            }
        }

        io.advance_tick();
        Ok(())
    }

    fn transition(
        &mut self,
        tick: Tick,
        to: StepId,
        reason: TransitionReason,
        on_event: &mut impl FnMut(TraceEvent),
    ) -> Result<(), RuntimeError> {
        let from = self.loc.step;
        self.loc.step = to;
        self.step_entered_at = Some(tick);
        on_event(TraceEvent {
            tick,
            task: self.loc.task,
            from,
            to,
            reason,
        });
        Ok(())
    }
}

const MAX_PID_LOOPS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq)]
struct PidState {
    integral: f32,
    prev_error: f32,
    last_updated: Option<Tick>,
}

impl Default for PidState {
    fn default() -> Self {
        Self {
            integral: 0.0,
            prev_error: 0.0,
            last_updated: None,
        }
    }
}

impl<'a> Runtime<'a> {
    fn update_pid_loops<IO: Io>(&mut self, now: Tick, io: &mut IO) {
        // Keep this branch-free for the common case: no PID loops.
        if self.program.pid_loops.is_empty() {
            return;
        }

        for (idx, cfg) in self.program.pid_loops.iter().enumerate() {
            if idx >= MAX_PID_LOOPS {
                break;
            }
            let state = &mut self.pid_states[idx];
            if !pid_should_run(now, state.last_updated, cfg.period_ticks) {
                continue;
            }
            let out = pid_step(cfg, state, io.read_analog_input(cfg.pv));
            io.write_analog_output(cfg.out, out);
            state.last_updated = Some(now);
        }
    }
}

fn pid_should_run(now: Tick, last: Option<Tick>, period_ticks: u64) -> bool {
    if period_ticks == 0 {
        return false;
    }
    match last {
        None => true,
        Some(t) => now.0.saturating_sub(t.0) >= period_ticks,
    }
}

fn pid_step(cfg: &PidConfig, state: &mut PidState, pv: f32) -> f32 {
    let sp = cfg.sp;
    let error = sp - pv;

    // Defensive: keep dt strictly positive to avoid NaN in derivative.
    let dt = if cfg.dt_s > 0.0 { cfg.dt_s } else { 1e-6 };

    let derivative = (error - state.prev_error) / dt;

    // Candidate integral update.
    let integral_candidate = state.integral + error * dt;
    let mut u_unsat = cfg.kp * error + cfg.ki * integral_candidate + cfg.kd * derivative;
    // Anti-windup: conditionally accept the integrator update.
    let integral = match cfg.anti_windup {
        AntiWindup::ConditionalIntegration => {
            if u_unsat > cfg.limit_max && error > 0.0 {
                state.integral
            } else if u_unsat < cfg.limit_min && error < 0.0 {
                state.integral
            } else {
                integral_candidate
            }
        }
    };

    u_unsat = cfg.kp * error + cfg.ki * integral + cfg.kd * derivative;
    let out = clamp_f32(u_unsat, cfg.limit_min, cfg.limit_max);

    state.integral = integral;
    state.prev_error = error;

    out
}

fn clamp_f32(v: f32, min: f32, max: f32) -> f32 {
    if v < min {
        min
    } else if v > max {
        max
    } else {
        v
    }
}

fn analog_in_selected_ranges(value: f32, ranges: &[AnalogRange]) -> bool {
    ranges.iter().any(|r| value >= r.min && value <= r.max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use io_traits::{AnalogInputId, Tick};

    struct MemIo {
        t: Tick,
        di: [bool; 1],
        do_: [bool; 1],
        ai: [f32; 1],
        ao: [f32; 1],
    }

    impl MemIo {
        fn new() -> Self {
            Self {
                t: Tick(0),
                di: [false],
                do_: [false],
                ai: [0.0],
                ao: [0.0],
            }
        }
    }

    impl Io for MemIo {
        fn tick(&self) -> Tick {
            self.t
        }

        fn advance_tick(&mut self) {
            self.t.0 += 1;
        }

        fn read_digital_input(&self, id: DigitalInputId) -> bool {
            self.di[id.0 as usize]
        }

        fn read_analog_input(&self, id: AnalogInputId) -> f32 {
            self.ai[id.0 as usize]
        }

        fn write_digital_output(&mut self, id: DigitalOutputId, value: bool) {
            self.do_[id.0 as usize] = value;
        }

        fn write_analog_output(&mut self, id: AnalogOutputId, value: f32) {
            self.ao[id.0 as usize] = value;
        }
    }

    #[test]
    fn delay_boundary_and_goto_chain_happen_on_expected_tick() {
        static STEPS: [Step<'static>; 3] = [
            Step {
                name: "delay2",
                instr: Instr::Delay {
                    ticks: 2,
                    next: StepId(1),
                },
            },
            Step {
                name: "goto2",
                instr: Instr::Goto { target: StepId(2) },
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
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).unwrap();

        let mut events: std::vec::Vec<TraceEvent> = std::vec::Vec::new();
        rt.tick_with_trace(&mut io, |e| events.push(e)).unwrap(); // tick 0
        rt.tick_with_trace(&mut io, |e| events.push(e)).unwrap(); // tick 1
        rt.tick_with_trace(&mut io, |e| events.push(e)).unwrap(); // tick 2 (delay completes + goto)

        assert_eq!(
            events,
            std::vec![
                TraceEvent {
                    tick: Tick(2),
                    task: 0,
                    from: StepId(0),
                    to: StepId(1),
                    reason: TransitionReason::DelayElapsed,
                },
                TraceEvent {
                    tick: Tick(2),
                    task: 0,
                    from: StepId(1),
                    to: StepId(2),
                    reason: TransitionReason::Goto,
                },
            ]
        );
    }

    #[test]
    fn wait_timeout_fires_when_elapsed_reaches_after_ticks() {
        static STEPS: [Step<'static>; 3] = [
            Step {
                name: "wait_di0_true_tmo2",
                instr: Instr::WaitDigital {
                    id: DigitalInputId(0),
                    equals: true,
                    next: StepId(2),
                    timeout: Some(Timeout {
                        after_ticks: 2,
                        target: StepId(1),
                    }),
                },
            },
            Step {
                name: "timed_out",
                instr: Instr::Halt,
            },
            Step {
                name: "ok",
                instr: Instr::Halt,
            },
        ];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).unwrap();

        let mut events: std::vec::Vec<TraceEvent> = std::vec::Vec::new();
        rt.tick_with_trace(&mut io, |e| events.push(e)).unwrap(); // tick 0
        rt.tick_with_trace(&mut io, |e| events.push(e)).unwrap(); // tick 1
        rt.tick_with_trace(&mut io, |e| events.push(e)).unwrap(); // tick 2 -> timeout

        assert_eq!(
            events,
            std::vec![TraceEvent {
                tick: Tick(2),
                task: 0,
                from: StepId(0),
                to: StepId(1),
                reason: TransitionReason::Timeout,
            }]
        );
    }

    #[test]
    fn timeout_zero_is_immediate_on_entry_tick() {
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "wait_di0_true_tmo0",
                instr: Instr::WaitDigital {
                    id: DigitalInputId(0),
                    equals: true,
                    next: StepId(1),
                    timeout: Some(Timeout {
                        after_ticks: 0,
                        target: StepId(1),
                    }),
                },
            },
            Step {
                name: "done",
                instr: Instr::Halt,
            },
        ];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).unwrap();

        let mut events: std::vec::Vec<TraceEvent> = std::vec::Vec::new();
        rt.tick_with_trace(&mut io, |e| events.push(e)).unwrap(); // tick 0 -> immediate timeout

        assert_eq!(
            events,
            std::vec![TraceEvent {
                tick: Tick(0),
                task: 0,
                from: StepId(0),
                to: StepId(1),
                reason: TransitionReason::Timeout,
            }]
        );
    }

    #[test]
    fn analog_wait_satisfies_when_value_enters_selected_region() {
        static RANGES: [AnalogRange; 1] = [AnalogRange {
            min: 80.0,
            max: 100.0,
        }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "wait_ai0_region",
                instr: Instr::WaitAnalog {
                    id: AnalogInputId(0),
                    ranges: &RANGES,
                    next: StepId(1),
                    timeout: None,
                },
            },
            Step {
                name: "done",
                instr: Instr::Halt,
            },
        ];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
        };

        let mut io = MemIo::new();
        io.ai[0] = 90.0;
        let mut rt = Runtime::new(&PROGRAM).unwrap();
        let mut events: std::vec::Vec<TraceEvent> = std::vec::Vec::new();

        rt.tick_with_trace(&mut io, |e| events.push(e)).unwrap();
        assert_eq!(
            events,
            std::vec![TraceEvent {
                tick: Tick(0),
                task: 0,
                from: StepId(0),
                to: StepId(1),
                reason: TransitionReason::WaitSatisfied,
            }]
        );
    }

    #[test]
    fn log_action_emits_log_event_without_touching_io() {
        static ACTIONS: [Action; 1] = [Action::Log {
            message_id: 7,
            message: "fault timeout",
        }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "log_once",
                instr: Instr::Action {
                    actions: &ACTIONS,
                    next: StepId(1),
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
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).unwrap();

        let mut logs = std::vec::Vec::new();
        let mut traces = std::vec::Vec::new();
        rt.tick_with_trace_and_logs(&mut io, |e| traces.push(e), |l| logs.push(l))
            .unwrap();

        assert_eq!(io.do_[0], false, "log action should not modify outputs");
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].tick, Tick(0));
        assert_eq!(logs[0].step, StepId(0));
        assert_eq!(logs[0].message_id, 7);
        assert_eq!(logs[0].message, "fault timeout");
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].reason, TransitionReason::Action);
    }

    #[test]
    fn pid_output_is_bounded_and_first_order_step_response_converges() {
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
            sp: 1.0,
            kp: 2.0,
            ki: 0.8,
            kd: 0.0,
            dt_s: 0.1,
            period_ticks: 1,
            limit_min: 0.0,
            limit_max: 1.0,
            anti_windup: AntiWindup::ConditionalIntegration,
        }];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &PID,
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).unwrap();

        // Simple first-order plant model: y[k+1] = y[k] + alpha*(u[k]-y[k]).
        let alpha = 0.2_f32;
        let mut pv_hist = std::vec::Vec::new();
        let mut u_hist = std::vec::Vec::new();

        for _ in 0..80 {
            rt.tick(&mut io).unwrap();
            let u = io.ao[0];
            io.ai[0] = io.ai[0] + alpha * (u - io.ai[0]);
            pv_hist.push(io.ai[0]);
            u_hist.push(u);
        }

        assert!(
            u_hist.iter().all(|u| *u >= 0.0 && *u <= 1.0),
            "PID output must stay in configured clamp range"
        );
        let initial_err = (1.0 - pv_hist[0]).abs();
        let final_err = (1.0 - pv_hist[pv_hist.len() - 1]).abs();
        assert!(
            final_err < initial_err,
            "step response should move toward setpoint (initial_err={initial_err}, final_err={final_err})"
        );
        assert!(
            pv_hist[pv_hist.len() - 1] > 0.8,
            "first-order response should converge near setpoint under this tuning"
        );
    }

    #[test]
    fn pid_conditional_integration_prevents_windup_after_saturation() {
        let cfg = PidConfig {
            pv: AnalogInputId(0),
            out: AnalogOutputId(0),
            sp: 10.0,
            kp: 0.0,
            ki: 1.0,
            kd: 0.0,
            dt_s: 0.1,
            period_ticks: 1,
            limit_min: 0.0,
            limit_max: 1.0,
            anti_windup: AntiWindup::ConditionalIntegration,
        };
        let mut state = PidState::default();

        // Large positive error; I-term-only controller hits clamp and should stop integrating.
        for _ in 0..20 {
            let _ = pid_step(&cfg, &mut state, 0.0);
        }

        // With conditional integration and ki=1.0, integrator should clamp near limit_max.
        assert!(
            (state.integral - 1.0).abs() < 1e-6,
            "integrator should clamp once output saturates (integral={})",
            state.integral
        );
    }
}
