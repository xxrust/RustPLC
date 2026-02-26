#![no_std]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate std;

use io_traits::{AnalogInputId, AnalogOutputId, DigitalInputId, DigitalOutputId, Io, Tick};
use libm::{cosf, floorf, fmodf, powf, sinf, sqrtf};

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
    TooManyVariables { configured: usize, max: usize },
    TooManyCamCouplings { configured: usize, max: usize },
    InvalidCamTableIndex { cam_index: usize, table_index: u16 },
    InvalidCamIndex { cam_index: u16 },
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
    SetAnalogExpr {
        id: AnalogOutputId,
        expr: ExprProgram,
    },
    Compute {
        target_var: u16,
        expr: ExprProgram,
    },
    CamEngage {
        cam_index: u16,
    },
    CamDisengage {
        cam_index: u16,
    },
    CamSwitch {
        cam_index: u16,
        table_index: u16,
    },
    CamPhase {
        cam_index: u16,
        offset_expr: ExprProgram,
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExprOp {
    PushLiteral(f32),
    PushVariable(u16),
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Neg,
    CallAbs,
    CallMin,
    CallMax,
    CallSin,
    CallCos,
    CallSqrt,
    CallPow,
    CallFmod,
    CallClamp,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExprProgram {
    pub ops: [ExprOp; MAX_EXPR_OPS],
    pub len: u8,
}

impl ExprProgram {
    pub const fn empty() -> Self {
        Self {
            ops: [ExprOp::PushLiteral(0.0); MAX_EXPR_OPS],
            len: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeout {
    pub after_ticks: u64,
    pub target: StepId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Eq,
    Ne,
    Gt,
    Lt,
    Ge,
    Le,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CamDigitalField {
    Engage,
    InSync,
    Fault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CamAnalogField {
    FollowingError,
    MasterPos,
    SlaveCmd,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnalogRange {
    pub min: f32,
    pub max: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplineCoeff {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CamTableData {
    pub periodic: bool,
    pub num_points: u16,
    pub master: [f32; MAX_CAM_POINTS],
    pub slave: [f32; MAX_CAM_POINTS],
    pub coeffs: [SplineCoeff; MAX_CAM_POINTS],
    pub last_index: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CamInterpolation {
    Linear,
    CubicSpline,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CamCouplingConfig {
    pub master_input: AnalogInputId,
    pub slave_output: AnalogOutputId,
    pub table_index: u16,
    pub interpolation: CamInterpolation,
    pub gear_ratio: f32,
    pub initial_phase_offset: f32,
    pub following_error_limit: f32,
    pub slave_feedback: AnalogInputId,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CamState {
    pub engaged: bool,
    pub master_pos: f32,
    pub slave_cmd: f32,
    pub slave_actual: f32,
    pub following_error: f32,
    pub in_sync: bool,
    pub fault: bool,
    pub active_table: u16,
    pub phase_offset: f32,
    pub switch_offset: f32,
    pub switch_decay_ticks: u16,
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
    WaitExpr {
        left: ExprProgram,
        op: CompareOp,
        right: ExprProgram,
        next: StepId,
        timeout: Option<Timeout>,
    },
    WaitCamDigital {
        cam_index: u16,
        field: CamDigitalField,
        equals: bool,
        next: StepId,
        timeout: Option<Timeout>,
    },
    WaitCamAnalog {
        cam_index: u16,
        field: CamAnalogField,
        op: CompareOp,
        value: f32,
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
    pub var_init: &'a [f32],
    pub cam_configs: &'a [CamCouplingConfig],
    pub cam_tables: &'a [CamTableData],
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
    variables: [f32; MAX_VARIABLES],
    cam_states: [CamState; MAX_CAM_COUPLINGS],
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
        if program.var_init.len() > MAX_VARIABLES {
            return Err(RuntimeError::TooManyVariables {
                configured: program.var_init.len(),
                max: MAX_VARIABLES,
            });
        }
        if program.cam_configs.len() > MAX_CAM_COUPLINGS {
            return Err(RuntimeError::TooManyCamCouplings {
                configured: program.cam_configs.len(),
                max: MAX_CAM_COUPLINGS,
            });
        }
        for (cam_index, cfg) in program.cam_configs.iter().enumerate() {
            if cfg.table_index as usize >= program.cam_tables.len() {
                return Err(RuntimeError::InvalidCamTableIndex {
                    cam_index,
                    table_index: cfg.table_index,
                });
            }
        }

        let entry = program.task(0)?.entry;
        let mut variables = [0.0f32; MAX_VARIABLES];
        for (idx, value) in program.var_init.iter().enumerate() {
            variables[idx] = *value;
        }
        let mut cam_states = [CamState::default(); MAX_CAM_COUPLINGS];
        for (idx, cfg) in program.cam_configs.iter().enumerate() {
            cam_states[idx].active_table = cfg.table_index;
            cam_states[idx].phase_offset = cfg.initial_phase_offset;
        }
        Ok(Self {
            program,
            loc: Location {
                task: 0,
                step: entry,
            },
            step_entered_at: None,
            pid_states: [PidState::default(); MAX_PID_LOOPS],
            variables,
            cam_states,
        })
    }

    pub fn location(&self) -> Location {
        self.loc
    }

    pub fn variables(&self) -> &[f32; MAX_VARIABLES] {
        &self.variables
    }

    pub fn cam_states(&self) -> &[CamState; MAX_CAM_COUPLINGS] {
        &self.cam_states
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
        self.update_cam_couplings(now, io);

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
                            Action::SetDigital { id, value } => io.write_digital_output(id, value),
                            Action::SetAnalog { id, value } => io.write_analog_output(id, value),
                            Action::SetAnalogExpr { id, expr } => {
                                let value = eval_expr(&expr, &self.variables);
                                io.write_analog_output(id, value);
                            }
                            Action::Compute { target_var, expr } => {
                                let idx = target_var as usize;
                                if idx < MAX_VARIABLES {
                                    self.variables[idx] = eval_expr(&expr, &self.variables);
                                }
                            }
                            Action::CamEngage { cam_index } => {
                                self.cam_engage(cam_index)?;
                            }
                            Action::CamDisengage { cam_index } => {
                                self.cam_disengage(cam_index)?;
                            }
                            Action::CamSwitch {
                                cam_index,
                                table_index,
                            } => {
                                self.cam_switch(cam_index, table_index)?;
                            }
                            Action::CamPhase {
                                cam_index,
                                offset_expr,
                            } => {
                                let offset = eval_expr(&offset_expr, &self.variables);
                                self.cam_phase(cam_index, offset)?;
                            }
                            Action::Extend { output } => io.write_digital_output(output, true),
                            Action::Retract { output } => io.write_digital_output(output, false),
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
                Instr::WaitExpr {
                    left,
                    op,
                    right,
                    next,
                    timeout,
                } => {
                    let lhs = eval_expr(&left, &self.variables);
                    let rhs = eval_expr(&right, &self.variables);
                    if compare_f32(lhs, op, rhs) {
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
                Instr::WaitCamDigital {
                    cam_index,
                    field,
                    equals,
                    next,
                    timeout,
                } => {
                    let actual = self.cam_digital_field(cam_index, field)?;
                    if actual == equals {
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
                Instr::WaitCamAnalog {
                    cam_index,
                    field,
                    op,
                    value,
                    next,
                    timeout,
                } => {
                    let actual = self.cam_analog_field(cam_index, field)?;
                    if compare_f32(actual, op, value) {
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

    fn cam_engage(&mut self, cam_index: u16) -> Result<(), RuntimeError> {
        let cam_idx = cam_index as usize;
        let Some(cfg) = self.program.cam_configs.get(cam_idx).copied() else {
            return Err(RuntimeError::InvalidCamIndex { cam_index });
        };
        let state = &mut self.cam_states[cam_idx];
        state.engaged = true;
        state.fault = false;
        state.in_sync = false;
        state.active_table = cfg.table_index;
        state.phase_offset = cfg.initial_phase_offset;
        state.switch_offset = 0.0;
        state.switch_decay_ticks = 0;
        Ok(())
    }

    fn cam_disengage(&mut self, cam_index: u16) -> Result<(), RuntimeError> {
        let cam_idx = cam_index as usize;
        if cam_idx >= self.program.cam_configs.len() {
            return Err(RuntimeError::InvalidCamIndex { cam_index });
        }
        let state = &mut self.cam_states[cam_idx];
        state.engaged = false;
        state.in_sync = false;
        Ok(())
    }

    fn cam_switch(&mut self, cam_index: u16, table_index: u16) -> Result<(), RuntimeError> {
        let cam_idx = cam_index as usize;
        let Some(cfg) = self.program.cam_configs.get(cam_idx) else {
            return Err(RuntimeError::InvalidCamIndex { cam_index });
        };
        if table_index as usize >= self.program.cam_tables.len() {
            return Err(RuntimeError::InvalidCamTableIndex {
                cam_index: cam_idx,
                table_index,
            });
        }

        let old_cmd = self.cam_states[cam_idx].slave_cmd;
        self.cam_states[cam_idx].active_table = table_index;
        let adjusted =
            self.cam_states[cam_idx].master_pos * cfg.gear_ratio + self.cam_states[cam_idx].phase_offset;
        let new_table = &self.program.cam_tables[table_index as usize];
        let new_cmd = interpolate_cam(cfg.interpolation, new_table, adjusted);
        let state = &mut self.cam_states[cam_idx];
        state.switch_offset = old_cmd - new_cmd;
        state.switch_decay_ticks = 100;
        Ok(())
    }

    fn cam_phase(&mut self, cam_index: u16, offset: f32) -> Result<(), RuntimeError> {
        let cam_idx = cam_index as usize;
        if cam_idx >= self.program.cam_configs.len() {
            return Err(RuntimeError::InvalidCamIndex { cam_index });
        }
        let state = &mut self.cam_states[cam_idx];
        state.phase_offset = offset;
        Ok(())
    }

    fn cam_digital_field(
        &self,
        cam_index: u16,
        field: CamDigitalField,
    ) -> Result<bool, RuntimeError> {
        let cam_idx = cam_index as usize;
        if cam_idx >= self.program.cam_configs.len() {
            return Err(RuntimeError::InvalidCamIndex { cam_index });
        }
        let state = &self.cam_states[cam_idx];
        Ok(match field {
            CamDigitalField::Engage => state.engaged,
            CamDigitalField::InSync => state.in_sync,
            CamDigitalField::Fault => state.fault,
        })
    }

    fn cam_analog_field(
        &self,
        cam_index: u16,
        field: CamAnalogField,
    ) -> Result<f32, RuntimeError> {
        let cam_idx = cam_index as usize;
        if cam_idx >= self.program.cam_configs.len() {
            return Err(RuntimeError::InvalidCamIndex { cam_index });
        }
        let state = &self.cam_states[cam_idx];
        Ok(match field {
            CamAnalogField::FollowingError => state.following_error,
            CamAnalogField::MasterPos => state.master_pos,
            CamAnalogField::SlaveCmd => state.slave_cmd,
        })
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

fn eval_expr(program: &ExprProgram, vars: &[f32; MAX_VARIABLES]) -> f32 {
    if program.len == 0 {
        return 0.0;
    }

    let mut stack = [0.0f32; MAX_EXPR_STACK];
    let mut sp = 0usize;
    for op in program.ops.iter().take(program.len as usize) {
        match *op {
            ExprOp::PushLiteral(v) => {
                if sp >= MAX_EXPR_STACK {
                    return 0.0;
                }
                stack[sp] = v;
                sp += 1;
            }
            ExprOp::PushVariable(idx) => {
                let idx = idx as usize;
                if idx >= MAX_VARIABLES || sp >= MAX_EXPR_STACK {
                    return 0.0;
                }
                stack[sp] = vars[idx];
                sp += 1;
            }
            ExprOp::Add => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                stack[sp - 1] += stack[sp];
            }
            ExprOp::Sub => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                stack[sp - 1] -= stack[sp];
            }
            ExprOp::Mul => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                stack[sp - 1] *= stack[sp];
            }
            ExprOp::Div => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                let rhs = stack[sp];
                if rhs == 0.0 {
                    return 0.0;
                }
                stack[sp - 1] /= rhs;
            }
            ExprOp::Mod => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                let rhs = stack[sp];
                if rhs == 0.0 {
                    return 0.0;
                }
                stack[sp - 1] = fmodf(stack[sp - 1], rhs);
            }
            ExprOp::Neg => {
                if sp < 1 {
                    return 0.0;
                }
                stack[sp - 1] = -stack[sp - 1];
            }
            ExprOp::CallAbs => {
                if sp < 1 {
                    return 0.0;
                }
                stack[sp - 1] = stack[sp - 1].abs();
            }
            ExprOp::CallMin => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                stack[sp - 1] = stack[sp - 1].min(stack[sp]);
            }
            ExprOp::CallMax => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                stack[sp - 1] = stack[sp - 1].max(stack[sp]);
            }
            ExprOp::CallSin => {
                if sp < 1 {
                    return 0.0;
                }
                stack[sp - 1] = sinf(stack[sp - 1]);
            }
            ExprOp::CallCos => {
                if sp < 1 {
                    return 0.0;
                }
                stack[sp - 1] = cosf(stack[sp - 1]);
            }
            ExprOp::CallSqrt => {
                if sp < 1 {
                    return 0.0;
                }
                stack[sp - 1] = sqrtf(stack[sp - 1]);
            }
            ExprOp::CallPow => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                stack[sp - 1] = powf(stack[sp - 1], stack[sp]);
            }
            ExprOp::CallFmod => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                let rhs = stack[sp];
                if rhs == 0.0 {
                    return 0.0;
                }
                stack[sp - 1] = fmodf(stack[sp - 1], rhs);
            }
            ExprOp::CallClamp => {
                if sp < 3 {
                    return 0.0;
                }
                let hi = stack[sp - 1];
                let lo = stack[sp - 2];
                let value = stack[sp - 3];
                sp -= 2;
                stack[sp - 1] = clamp_f32(value, lo, hi);
            }
        }
    }

    if sp == 0 {
        0.0
    } else {
        stack[0]
    }
}

const MAX_PID_LOOPS: usize = 8;
pub const MAX_VARIABLES: usize = 64;
pub const MAX_EXPR_OPS: usize = 32;
pub const MAX_EXPR_STACK: usize = 16;
pub const MAX_CAM_POINTS: usize = 256;
pub const MAX_CAM_COUPLINGS: usize = 8;

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

    fn update_cam_couplings<IO: Io>(&mut self, _now: Tick, io: &mut IO) {
        if self.program.cam_configs.is_empty() {
            return;
        }

        for (idx, cfg) in self.program.cam_configs.iter().enumerate() {
            if idx >= MAX_CAM_COUPLINGS {
                break;
            }

            let state = &mut self.cam_states[idx];
            if !state.engaged {
                continue;
            }

            let Some(table) = self.program.cam_tables.get(state.active_table as usize) else {
                state.fault = true;
                state.engaged = false;
                state.in_sync = false;
                continue;
            };

            state.master_pos = io.read_analog_input(cfg.master_input);
            let adjusted_master = state.master_pos * cfg.gear_ratio + state.phase_offset;
            state.slave_cmd = interpolate_cam(cfg.interpolation, table, adjusted_master);

            if state.switch_decay_ticks > 0 {
                state.slave_cmd += state.switch_offset;
                state.switch_offset *= 0.95;
                state.switch_decay_ticks -= 1;
            }

            io.write_analog_output(cfg.slave_output, state.slave_cmd);

            state.slave_actual = io.read_analog_input(cfg.slave_feedback);
            state.following_error = (state.slave_cmd - state.slave_actual).abs();

            let limit = cfg.following_error_limit;
            state.in_sync = limit > 0.0 && state.following_error < limit;
            if limit > 0.0 && state.following_error > limit * 3.0 {
                state.fault = true;
                state.engaged = false;
                state.in_sync = false;
            }
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

pub fn binary_search_interval(table: &CamTableData, x: f32) -> u16 {
    let n = table.num_points as usize;
    if n < 2 {
        return 0;
    }
    let mut lo = 0usize;
    let mut hi = n - 1;
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if table.master[mid] <= x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo as u16
}

pub fn normalize_master(table: &CamTableData, master_pos: f32) -> f32 {
    let n = table.num_points as usize;
    if n == 0 {
        return 0.0;
    }
    let x0 = table.master[0];
    if n == 1 {
        return x0;
    }
    let xn = table.master[n - 1];

    if table.periodic {
        let period = xn - x0;
        if period <= 0.0 {
            return x0;
        }
        let offset = master_pos - x0;
        x0 + offset - floorf(offset / period) * period
    } else if master_pos < x0 {
        x0
    } else if master_pos > xn {
        xn
    } else {
        master_pos
    }
}

pub fn linear_interpolate(table: &CamTableData, master_pos: f32) -> f32 {
    let n = table.num_points as usize;
    if n == 0 {
        return 0.0;
    }
    if n == 1 {
        return table.slave[0];
    }

    let x = normalize_master(table, master_pos);
    let i = binary_search_interval(table, x) as usize;
    let x0 = table.master[i];
    let x1 = table.master[i + 1];
    let y0 = table.slave[i];
    let y1 = table.slave[i + 1];
    let dx = x1 - x0;
    if dx == 0.0 {
        return y0;
    }
    let t = (x - x0) / dx;
    y0 + t * (y1 - y0)
}

pub fn cubic_interpolate(table: &CamTableData, master_pos: f32) -> f32 {
    let n = table.num_points as usize;
    if n == 0 {
        return 0.0;
    }
    if n == 1 {
        return table.slave[0];
    }

    let x = normalize_master(table, master_pos);
    let i = binary_search_interval(table, x) as usize;
    let dx = x - table.master[i];
    let c = table.coeffs[i];
    c.a + dx * (c.b + dx * (c.c + dx * c.d))
}

pub fn cubic_derivative(table: &CamTableData, master_pos: f32) -> f32 {
    let n = table.num_points as usize;
    if n < 2 {
        return 0.0;
    }
    let x = normalize_master(table, master_pos);
    let i = binary_search_interval(table, x) as usize;
    let dx = x - table.master[i];
    let c = table.coeffs[i];
    c.b + dx * (2.0 * c.c + 3.0 * c.d * dx)
}

fn interpolate_cam(interpolation: CamInterpolation, table: &CamTableData, master_pos: f32) -> f32 {
    match interpolation {
        CamInterpolation::Linear => linear_interpolate(table, master_pos),
        CamInterpolation::CubicSpline => cubic_interpolate(table, master_pos),
    }
}

fn compare_f32(left: f32, op: CompareOp, right: f32) -> bool {
    match op {
        CompareOp::Eq => left == right,
        CompareOp::Ne => left != right,
        CompareOp::Gt => left > right,
        CompareOp::Lt => left < right,
        CompareOp::Ge => left >= right,
        CompareOp::Le => left <= right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use io_traits::{AnalogInputId, Tick};
    use std::{boxed::Box, vec};

    struct MemIo {
        t: Tick,
        di: [bool; 4],
        do_: [bool; 4],
        ai: [f32; 4],
        ao: [f32; 4],
    }

    impl MemIo {
        fn new() -> Self {
            Self {
                t: Tick(0),
                di: [false; 4],
                do_: [false; 4],
                ai: [0.0; 4],
                ao: [0.0; 4],
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

    fn build_cam_table(periodic: bool, points: &[(f32, f32)]) -> CamTableData {
        let mut master = [0.0f32; MAX_CAM_POINTS];
        let mut slave = [0.0f32; MAX_CAM_POINTS];
        let mut coeffs = [SplineCoeff {
            a: 0.0,
            b: 0.0,
            c: 0.0,
            d: 0.0,
        }; MAX_CAM_POINTS];

        for (idx, (x, y)) in points.iter().copied().enumerate() {
            master[idx] = x;
            slave[idx] = y;
        }
        for i in 0..points.len().saturating_sub(1) {
            let dx = master[i + 1] - master[i];
            let slope = if dx == 0.0 {
                0.0
            } else {
                (slave[i + 1] - slave[i]) / dx
            };
            coeffs[i] = SplineCoeff {
                a: slave[i],
                b: slope,
                c: 0.0,
                d: 0.0,
            };
        }

        CamTableData {
            periodic,
            num_points: points.len() as u16,
            master,
            slave,
            coeffs,
            last_index: 0,
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
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
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
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
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
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
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
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
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
    fn linear_interpolate_handles_periodic_wrap_and_oneshot_clamp() {
        let periodic = build_cam_table(true, &[(0.0, 0.0), (100.0, 100.0), (200.0, 0.0)]);
        let wrapped_neg = linear_interpolate(&periodic, -50.0);
        let wrapped_over = linear_interpolate(&periodic, 250.0);
        assert!(
            (wrapped_neg - 50.0).abs() < 1e-5,
            "periodic wrap(-50) 期望约 50，实际 {wrapped_neg}"
        );
        assert!(
            (wrapped_over - 50.0).abs() < 1e-5,
            "periodic wrap(250) 期望约 50，实际 {wrapped_over}"
        );

        let oneshot = build_cam_table(false, &[(0.0, 0.0), (100.0, 100.0)]);
        assert_eq!(
            linear_interpolate(&oneshot, -10.0),
            0.0,
            "oneshot 应在左侧钳制"
        );
        assert_eq!(
            linear_interpolate(&oneshot, 150.0),
            100.0,
            "oneshot 应在右侧钳制"
        );
    }

    #[test]
    fn cubic_interpolate_evaluates_horner_polynomial() {
        let mut table = build_cam_table(false, &[(0.0, 0.0), (10.0, 10.0)]);
        table.coeffs[0] = SplineCoeff {
            a: 1.0,
            b: 2.0,
            c: 3.0,
            d: 4.0,
        };
        let out = cubic_interpolate(&table, 2.0);
        assert!(
            (out - 49.0).abs() < 1e-6,
            "Horner 多项式应为 49，实际 {out}"
        );
    }

    #[test]
    fn wait_expr_satisfies_and_supports_timeout() {
        const fn lit_expr(value: f32) -> ExprProgram {
            let mut ops = [ExprOp::PushLiteral(0.0); MAX_EXPR_OPS];
            ops[0] = ExprOp::PushLiteral(value);
            ExprProgram { ops, len: 1 }
        }
        const fn add_var_and_lit(var_idx: u16, value: f32) -> ExprProgram {
            let mut ops = [ExprOp::PushLiteral(0.0); MAX_EXPR_OPS];
            ops[0] = ExprOp::PushVariable(var_idx);
            ops[1] = ExprOp::PushLiteral(value);
            ops[2] = ExprOp::Add;
            ExprProgram { ops, len: 3 }
        }

        static STEPS: [Step<'static>; 4] = [
            Step {
                name: "wait_expr_ok",
                instr: Instr::WaitExpr {
                    left: add_var_and_lit(0, 1.0),
                    op: CompareOp::Gt,
                    right: lit_expr(1.5),
                    next: StepId(1),
                    timeout: None,
                },
            },
            Step {
                name: "wait_expr_timeout",
                instr: Instr::WaitExpr {
                    left: lit_expr(0.0),
                    op: CompareOp::Eq,
                    right: lit_expr(1.0),
                    next: StepId(3),
                    timeout: Some(Timeout {
                        after_ticks: 1,
                        target: StepId(2),
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
        static VARS: [f32; 1] = [1.0];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &VARS,
            cam_configs: &[],
            cam_tables: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).unwrap();
        let mut events: std::vec::Vec<TraceEvent> = std::vec::Vec::new();

        rt.tick_with_trace(&mut io, |e| events.push(e)).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].to, StepId(1));
        assert_eq!(events[0].reason, TransitionReason::WaitSatisfied);

        rt.tick_with_trace(&mut io, |e| events.push(e)).unwrap();
        rt.tick_with_trace(&mut io, |e| events.push(e)).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].to, StepId(2));
        assert_eq!(events[1].reason, TransitionReason::Timeout);
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
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
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
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
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
    fn eval_expr_supports_builtin_math_functions() {
        let mut vars = [0.0f32; MAX_VARIABLES];
        vars[0] = -4.0;

        let mut ops = [ExprOp::PushLiteral(0.0); MAX_EXPR_OPS];
        ops[0] = ExprOp::PushVariable(0);
        ops[1] = ExprOp::CallAbs;
        ops[2] = ExprOp::PushLiteral(2.0);
        ops[3] = ExprOp::CallPow;
        ops[4] = ExprOp::PushLiteral(0.0);
        ops[5] = ExprOp::PushLiteral(9.0);
        ops[6] = ExprOp::CallClamp;
        let expr = ExprProgram { ops, len: 7 };
        let out = eval_expr(&expr, &vars);
        assert!((out - 9.0).abs() < 1e-6, "clamp(pow(abs(x),2),0,9) 应为 9");

        let mut ops2 = [ExprOp::PushLiteral(0.0); MAX_EXPR_OPS];
        ops2[0] = ExprOp::PushLiteral(3.0);
        ops2[1] = ExprOp::PushLiteral(2.0);
        ops2[2] = ExprOp::CallFmod;
        ops2[3] = ExprOp::PushLiteral(0.0);
        ops2[4] = ExprOp::CallSin;
        ops2[5] = ExprOp::CallCos;
        ops2[6] = ExprOp::CallMax;
        let expr2 = ExprProgram { ops: ops2, len: 7 };
        let out2 = eval_expr(&expr2, &vars);
        assert!(
            (out2 - 1.0).abs() < 1e-6,
            "max(fmod(3,2), cos(sin(0))) 应为 1"
        );
    }

    #[test]
    fn runtime_loads_variable_initial_values() {
        static STEPS: [Step<'static>; 1] = [Step {
            name: "halt",
            instr: Instr::Halt,
        }];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static VARS: [f32; 3] = [1.5, 2.0, 0.0];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &VARS,
            cam_configs: &[],
            cam_tables: &[],
        };

        let rt = Runtime::new(&PROGRAM).expect("runtime 创建应成功");
        assert_eq!(rt.variables()[0], 1.5);
        assert_eq!(rt.variables()[1], 2.0);
        assert_eq!(rt.variables()[2], 0.0);
        assert_eq!(rt.variables()[3], 0.0, "未初始化槽位应保持 0");
    }

    #[test]
    fn runtime_rejects_too_many_variables() {
        static STEPS: [Step<'static>; 1] = [Step {
            name: "halt",
            instr: Instr::Halt,
        }];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static VARS: [f32; MAX_VARIABLES + 1] = [0.0; MAX_VARIABLES + 1];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &VARS,
            cam_configs: &[],
            cam_tables: &[],
        };

        let err = match Runtime::new(&PROGRAM) {
            Ok(_) => panic!("超过变量上限应报错"),
            Err(err) => err,
        };
        assert_eq!(
            err,
            RuntimeError::TooManyVariables {
                configured: MAX_VARIABLES + 1,
                max: MAX_VARIABLES,
            }
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

    #[test]
    fn cam_action_rejects_invalid_index() {
        static ACTIONS: [Action; 1] = [Action::CamDisengage { cam_index: 1 }];
        static STEPS: [Step<'static>; 1] = [Step {
            name: "bad_cam",
            instr: Instr::Action {
                actions: &ACTIONS,
                next: StepId(0),
            },
        }];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];

        let cam_tables: &'static [CamTableData] =
            Box::leak(vec![build_cam_table(false, &[(0.0, 0.0), (360.0, 360.0)])].into_boxed_slice());
        let cam_configs: &'static [CamCouplingConfig] = Box::leak(
            vec![CamCouplingConfig {
                master_input: AnalogInputId(0),
                slave_output: AnalogOutputId(0),
                table_index: 0,
                interpolation: CamInterpolation::Linear,
                gear_ratio: 1.0,
                initial_phase_offset: 0.0,
                following_error_limit: 1.0,
                slave_feedback: AnalogInputId(1),
            }]
            .into_boxed_slice(),
        );
        let program = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs,
            cam_tables,
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&program).expect("runtime init");
        let err = rt.tick(&mut io).expect_err("非法 cam_index 应报错");
        assert_eq!(err, RuntimeError::InvalidCamIndex { cam_index: 1 });
    }

    #[test]
    fn cam_switch_rejects_invalid_table_index() {
        static ACTIONS: [Action; 1] = [Action::CamSwitch {
            cam_index: 0,
            table_index: 9,
        }];
        static STEPS: [Step<'static>; 1] = [Step {
            name: "bad_table",
            instr: Instr::Action {
                actions: &ACTIONS,
                next: StepId(0),
            },
        }];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];

        let cam_tables: &'static [CamTableData] =
            Box::leak(vec![build_cam_table(false, &[(0.0, 0.0), (360.0, 360.0)])].into_boxed_slice());
        let cam_configs: &'static [CamCouplingConfig] = Box::leak(
            vec![CamCouplingConfig {
                master_input: AnalogInputId(0),
                slave_output: AnalogOutputId(0),
                table_index: 0,
                interpolation: CamInterpolation::Linear,
                gear_ratio: 1.0,
                initial_phase_offset: 0.0,
                following_error_limit: 1.0,
                slave_feedback: AnalogInputId(1),
            }]
            .into_boxed_slice(),
        );
        let program = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs,
            cam_tables,
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&program).expect("runtime init");
        let err = rt.tick(&mut io).expect_err("非法 table_index 应报错");
        assert_eq!(
            err,
            RuntimeError::InvalidCamTableIndex {
                cam_index: 0,
                table_index: 9,
            }
        );
    }

    #[test]
    fn cam_wait_and_phase_actions_work_with_runtime_state() {
        static ENGAGE: [Action; 1] = [Action::CamEngage { cam_index: 0 }];
        static PHASE_EXPR: ExprProgram = ExprProgram {
            ops: [ExprOp::PushLiteral(10.0); MAX_EXPR_OPS],
            len: 1,
        };
        static PHASE: [Action; 1] = [Action::CamPhase {
            cam_index: 0,
            offset_expr: PHASE_EXPR,
        }];
        static STEPS: [Step<'static>; 5] = [
            Step {
                name: "engage",
                instr: Instr::Action {
                    actions: &ENGAGE,
                    next: StepId(1),
                },
            },
            Step {
                name: "wait_engaged",
                instr: Instr::WaitCamDigital {
                    cam_index: 0,
                    field: CamDigitalField::Engage,
                    equals: true,
                    next: StepId(2),
                    timeout: None,
                },
            },
            Step {
                name: "phase",
                instr: Instr::Action {
                    actions: &PHASE,
                    next: StepId(3),
                },
            },
            Step {
                name: "wait_master",
                instr: Instr::WaitCamAnalog {
                    cam_index: 0,
                    field: CamAnalogField::MasterPos,
                    op: CompareOp::Gt,
                    value: 5.0,
                    next: StepId(4),
                    timeout: None,
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

        let cam_tables: &'static [CamTableData] =
            Box::leak(vec![build_cam_table(false, &[(0.0, 0.0), (360.0, 360.0)])].into_boxed_slice());
        let cam_configs: &'static [CamCouplingConfig] = Box::leak(
            vec![CamCouplingConfig {
                master_input: AnalogInputId(0),
                slave_output: AnalogOutputId(0),
                table_index: 0,
                interpolation: CamInterpolation::Linear,
                gear_ratio: 1.0,
                initial_phase_offset: 0.0,
                following_error_limit: 1000.0,
                slave_feedback: AnalogInputId(1),
            }]
            .into_boxed_slice(),
        );
        let program = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs,
            cam_tables,
        };

        let mut io = MemIo::new();
        io.ai[0] = 20.0;
        io.ai[1] = 0.0;
        let mut rt = Runtime::new(&program).expect("runtime init");

        rt.tick(&mut io).expect("tick0 should progress to wait_master");
        assert_eq!(rt.location().step, StepId(3));

        rt.tick(&mut io).expect("tick1 should satisfy wait_master");
        assert_eq!(rt.location().step, StepId(4));
        assert!((io.ao[0] - 30.0).abs() < 1e-5, "phase offset should shift cam output");
    }

    #[test]
    fn cam_fault_disengages_when_following_error_exceeds_limit() {
        static ENGAGE: [Action; 1] = [Action::CamEngage { cam_index: 0 }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "engage",
                instr: Instr::Action {
                    actions: &ENGAGE,
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

        let cam_tables: &'static [CamTableData] =
            Box::leak(vec![build_cam_table(false, &[(0.0, 0.0), (360.0, 360.0)])].into_boxed_slice());
        let cam_configs: &'static [CamCouplingConfig] = Box::leak(
            vec![CamCouplingConfig {
                master_input: AnalogInputId(0),
                slave_output: AnalogOutputId(0),
                table_index: 0,
                interpolation: CamInterpolation::Linear,
                gear_ratio: 1.0,
                initial_phase_offset: 0.0,
                following_error_limit: 1.0,
                slave_feedback: AnalogInputId(1),
            }]
            .into_boxed_slice(),
        );
        let program = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs,
            cam_tables,
        };

        let mut io = MemIo::new();
        io.ai[0] = 180.0;
        io.ai[1] = 0.0;
        let mut rt = Runtime::new(&program).expect("runtime init");

        rt.tick(&mut io).expect("tick0 engage");
        rt.tick(&mut io).expect("tick1 update cam and detect fault");

        let cam = rt.cam_states()[0];
        assert!(cam.fault, "following error should raise fault");
        assert!(!cam.engaged, "fault should disengage cam");
    }
}
