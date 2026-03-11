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
pub enum AxisFaultCategory {
    Recoverable,
    NonRecoverable,
    Safety,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisFaultKind {
    Reject,
    Motion,
    Safety,
    Vendor {
        category: AxisFaultCategory,
        vendor_code: i32,
    },
}

impl AxisFaultKind {
    pub const fn category(self) -> AxisFaultCategory {
        match self {
            AxisFaultKind::Reject => AxisFaultCategory::Recoverable,
            AxisFaultKind::Motion => AxisFaultCategory::NonRecoverable,
            AxisFaultKind::Safety => AxisFaultCategory::Safety,
            AxisFaultKind::Vendor { category, .. } => category,
        }
    }

    pub const fn vendor_code(self) -> Option<i32> {
        match self {
            AxisFaultKind::Vendor { vendor_code, .. } => Some(vendor_code),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxisFault {
    pub kind: AxisFaultKind,
    pub category: AxisFaultCategory,
    pub error_code: i32,
    pub vendor_code: Option<i32>,
}

impl AxisFault {
    pub const fn new(kind: AxisFaultKind, error_code: i32) -> Self {
        Self {
            category: kind.category(),
            vendor_code: kind.vendor_code(),
            kind,
            error_code,
        }
    }

    pub const fn reject(error_code: i32) -> Self {
        Self::new(AxisFaultKind::Reject, error_code)
    }

    pub const fn motion(error_code: i32) -> Self {
        Self::new(AxisFaultKind::Motion, error_code)
    }

    pub const fn safety(error_code: i32) -> Self {
        Self::new(AxisFaultKind::Safety, error_code)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisFaultSeverity {
    Recoverable,
    NonRecoverable,
    Safety,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisStopMode {
    Controlled,
    Quick,
    Immediate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisStopState {
    Running,
    ControlledStopping,
    QuickStopping,
    ImmediateStopping,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisAutoResetPolicy {
    Never,
    OnClear,
    Immediate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisFaultPropagationScope {
    SelfOnly,
    Group,
    All,
    Followers,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxisFaultPolicy<'a> {
    pub axis: &'a str,
    pub severity: AxisFaultSeverity,
    pub stop_mode: AxisStopMode,
    pub auto_reset_policy: AxisAutoResetPolicy,
    pub manual_ack_required: bool,
    pub propagation_scope: AxisFaultPropagationScope,
    pub propagation_targets: &'a [&'static str],
}

pub const AXIS_FAULT_POLICY_LOG_MESSAGE: &str = "axis_fault_policy_applied";
const AXIS_FAULT_POLICY_LOG_BASE_ID: u16 = 50_000;
pub const AXIS_STOP_TRANSITION_ENTER_LOG_MESSAGE: &str = "axis_stop_transition_enter";
pub const AXIS_STOP_TRANSITION_COMPLETED_LOG_MESSAGE: &str = "axis_stop_transition_completed";
const AXIS_STOP_TRANSITION_LOG_BASE_ID: u16 = 51_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisStopTransitionPhase {
    Enter,
    Completed,
}

pub const fn axis_fault_policy_log_message_id(
    severity: AxisFaultSeverity,
    stop_mode: AxisStopMode,
    auto_reset_policy: AxisAutoResetPolicy,
    manual_ack_required: bool,
    fault_kind: AxisFaultKind,
) -> u16 {
    let severity_bits = match severity {
        AxisFaultSeverity::Recoverable => 0,
        AxisFaultSeverity::NonRecoverable => 1,
        AxisFaultSeverity::Safety => 2,
    };
    let stop_mode_bits = match stop_mode {
        AxisStopMode::Controlled => 0,
        AxisStopMode::Quick => 1,
        AxisStopMode::Immediate => 2,
    };
    let auto_reset_bits = match auto_reset_policy {
        AxisAutoResetPolicy::Never => 0,
        AxisAutoResetPolicy::OnClear => 1,
        AxisAutoResetPolicy::Immediate => 2,
    };
    let ack_bits = if manual_ack_required { 1 } else { 0 };
    let fault_kind_bits = match fault_kind {
        AxisFaultKind::Reject => 0,
        AxisFaultKind::Motion => 1,
        AxisFaultKind::Safety => 2,
        AxisFaultKind::Vendor { .. } => 3,
    };

    AXIS_FAULT_POLICY_LOG_BASE_ID
        + severity_bits
        + (stop_mode_bits << 2)
        + (auto_reset_bits << 4)
        + (ack_bits << 6)
        + (fault_kind_bits << 7)
}

pub const fn axis_stop_transition_log_message_id(
    stop_mode: AxisStopMode,
    phase: AxisStopTransitionPhase,
) -> u16 {
    let stop_mode_bits = match stop_mode {
        AxisStopMode::Controlled => 0,
        AxisStopMode::Quick => 1,
        AxisStopMode::Immediate => 2,
    };
    let phase_bits = match phase {
        AxisStopTransitionPhase::Enter => 0,
        AxisStopTransitionPhase::Completed => 1,
    };

    AXIS_STOP_TRANSITION_LOG_BASE_ID + stop_mode_bits + (phase_bits << 2)
}

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
    TooManyTasks {
        configured: usize,
        max: usize,
    },
    InvalidTaskIndex {
        task: usize,
    },
    InvalidStepId {
        task: usize,
        step: StepId,
    },
    TooManyTransitionsInOneTick,
    TooManyPidLoops {
        configured: usize,
        max: usize,
    },
    TooManyVariables {
        configured: usize,
        max: usize,
    },
    TooManyCamCouplings {
        configured: usize,
        max: usize,
    },
    InvalidCamTableIndex {
        cam_index: usize,
        table_index: u16,
    },
    InvalidCamIndex {
        cam_index: u16,
    },
    ExternCallRequiresHandler {
        function: &'static str,
    },
    AxisMotionRequiresHandler {
        target: &'static str,
    },
    AxisNotHomed {
        target: &'static str,
    },
    TooManyAxisHomingTargets {
        max: usize,
    },
    ExternCallFailed {
        function: &'static str,
    },
    ExternReturnArityMismatch {
        function: &'static str,
        expected: usize,
        got: usize,
    },
    ExternArgumentLimitExceeded {
        function: &'static str,
        configured: usize,
        max: usize,
    },
    ExternReturnLimitExceeded {
        function: &'static str,
        configured: usize,
        max: usize,
    },
    ExternBindingVariableOutOfRange {
        function: &'static str,
        variable: u16,
    },
    ExternErrorCodeVariableOutOfRange {
        function: &'static str,
        variable: u16,
    },
    AxisFault {
        target: &'static str,
        fault: AxisFault,
    },
}

#[derive(Debug, PartialEq)]
pub enum RuntimeTickError<E> {
    Core(RuntimeError),
    ExternCallFailed {
        function: &'static str,
        error: E,
    },
    ExternReturnArityMismatch {
        function: &'static str,
        expected: usize,
        got: usize,
    },
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
    CallExtern {
        function: &'static str,
        arg_exprs: &'static [ExprProgram],
        binding_vars: &'static [u16],
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
    AxisMove {
        command: AxisMotionCommand,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisMoveKind {
    Relative,
    Absolute,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisMotionCommand {
    pub target: &'static str,
    pub port: &'static str,
    pub kind: AxisMoveKind,
    pub value: f32,
    pub speed: f32,
    pub require_homed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisMotionResult {
    Done,
    Fault(AxisFault),
}

impl AxisMotionResult {
    pub const fn reject(error_code: i32) -> Self {
        Self::Fault(AxisFault::reject(error_code))
    }

    pub const fn motion_fault(error_code: i32) -> Self {
        Self::Fault(AxisFault::motion(error_code))
    }

    pub const fn safety_fault(error_code: i32) -> Self {
        Self::Fault(AxisFault::safety(error_code))
    }
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
    CmpEq,
    CmpNe,
    CmpGt,
    CmpLt,
    CmpGe,
    CmpLe,
    BoolAnd,
    BoolOr,
    BoolNot,
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
    pub axis_fault_policies: &'a [AxisFaultPolicy<'a>],
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskWaitState {
    #[default]
    Ready,
    Delay,
    WaitCondition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskTimeoutState {
    Inactive,
    Armed {
        after_ticks: u64,
        target: StepId,
    },
}

impl Default for TaskTimeoutState {
    fn default() -> Self {
        Self::Inactive
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskPendingActionState {
    #[default]
    Idle,
    AxisMotion {
        target: &'static str,
    },
    ExternCall {
        function: &'static str,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskRuntimeContext {
    pub current_step: StepId,
    pub step_entered_at: Option<Tick>,
    pub wait_state: TaskWaitState,
    pub timeout_state: TaskTimeoutState,
    pub pending_action_state: TaskPendingActionState,
}

impl Default for TaskRuntimeContext {
    fn default() -> Self {
        Self {
            current_step: StepId(0),
            step_entered_at: None,
            wait_state: TaskWaitState::Ready,
            timeout_state: TaskTimeoutState::Inactive,
            pending_action_state: TaskPendingActionState::Idle,
        }
    }
}

/// A minimal deterministic tick executor.
///
/// - One call to `tick()` consumes exactly one `Io` tick (it calls `Io::advance_tick()`).
/// - Active tasks are evaluated in declaration/index order (`0..active_task_count`) every tick.
/// - Within a tick, non-blocking steps (`Action`, `Goto`, and completed `Delay`/`Wait`) may chain.
pub struct Runtime<'a> {
    program: &'a Program<'a>,
    active_task: usize,
    active_task_count: usize,
    task_contexts: [TaskRuntimeContext; MAX_ACTIVE_TASKS],
    axis_stop_state: AxisStopState,
    axis_homing_targets: [Option<&'static str>; MAX_AXIS_HOMING_TARGETS],
    axis_homing_flags: [bool; MAX_AXIS_HOMING_TARGETS],
    pid_states: [PidState; MAX_PID_LOOPS],
    variables: [f32; MAX_VARIABLES],
    cam_states: [CamState; MAX_CAM_COUPLINGS],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExternActionResult {
    Completed,
    HandledFailure,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActionCompletionState {
    Completed,
    Pending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StepCompletionDecision {
    ContinueWith {
        target: StepId,
        reason: TransitionReason,
    },
    StayOnStep,
}

impl<'a> Runtime<'a> {
    pub fn new(program: &'a Program<'a>) -> Result<Self, RuntimeError> {
        if program.tasks.is_empty() {
            return Err(RuntimeError::ProgramHasNoTasks);
        }
        if program.tasks.len() > MAX_ACTIVE_TASKS {
            return Err(RuntimeError::TooManyTasks {
                configured: program.tasks.len(),
                max: MAX_ACTIVE_TASKS,
            });
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

        let mut task_contexts = [TaskRuntimeContext::default(); MAX_ACTIVE_TASKS];
        for (task_idx, task) in program.tasks.iter().enumerate() {
            task_contexts[task_idx].current_step = task.entry;
        }

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
            active_task: 0,
            active_task_count: program.tasks.len(),
            task_contexts,
            axis_stop_state: AxisStopState::Running,
            axis_homing_targets: [None; MAX_AXIS_HOMING_TARGETS],
            axis_homing_flags: [false; MAX_AXIS_HOMING_TARGETS],
            pid_states: [PidState::default(); MAX_PID_LOOPS],
            variables,
            cam_states,
        })
    }

    pub fn location(&self) -> Location {
        let ctx = self.task_contexts[self.active_task];
        Location {
            task: self.active_task,
            step: ctx.current_step,
        }
    }

    pub fn active_task_count(&self) -> usize {
        self.active_task_count
    }

    pub fn task_context(&self, task: usize) -> Result<TaskRuntimeContext, RuntimeError> {
        if task >= self.active_task_count {
            return Err(RuntimeError::InvalidTaskIndex { task });
        }
        Ok(self.task_contexts[task])
    }

    pub fn axis_stop_state(&self) -> AxisStopState {
        self.axis_stop_state
    }

    fn axis_homing_slot(&mut self, target: &'static str) -> Result<usize, RuntimeError> {
        let mut free_slot = None;
        for idx in 0..MAX_AXIS_HOMING_TARGETS {
            match self.axis_homing_targets[idx] {
                Some(existing) if existing == target => return Ok(idx),
                Some(_) => {}
                None => {
                    if free_slot.is_none() {
                        free_slot = Some(idx);
                    }
                }
            }
        }

        if let Some(idx) = free_slot {
            self.axis_homing_targets[idx] = Some(target);
            self.axis_homing_flags[idx] = false;
            return Ok(idx);
        }

        Err(RuntimeError::TooManyAxisHomingTargets {
            max: MAX_AXIS_HOMING_TARGETS,
        })
    }

    fn axis_is_homed(&mut self, target: &'static str) -> Result<bool, RuntimeError> {
        let idx = self.axis_homing_slot(target)?;
        Ok(self.axis_homing_flags[idx])
    }

    fn set_axis_homed(&mut self, target: &'static str, homed: bool) -> Result<(), RuntimeError> {
        let idx = self.axis_homing_slot(target)?;
        self.axis_homing_flags[idx] = homed;
        Ok(())
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
        #[derive(Debug)]
        struct MissingExternHandler;
        let mut missing_extern =
            |_function: &'static str,
             _args: &[f32],
             _results: &mut [f32]|
             -> Result<usize, MissingExternHandler> { Err(MissingExternHandler) };
        let mut ignore_error_code = |_function: &'static str, _error: &MissingExternHandler| 0.0;
        let mut missing_axis = |command: AxisMotionCommand| {
            Err(RuntimeError::AxisMotionRequiresHandler {
                target: command.target,
            })
        };
        self.tick_with_trace_and_logs_impl(
            io,
            &mut on_event,
            &mut on_log,
            &mut missing_extern,
            None,
            &mut ignore_error_code,
            &mut missing_axis,
            &mut |_: AxisMotionCommand, _: AxisFault| {},
        )
        .map_err(|err| match err {
            RuntimeTickError::Core(err) => err,
            RuntimeTickError::ExternCallFailed { function, .. } => {
                RuntimeError::ExternCallRequiresHandler { function }
            }
            RuntimeTickError::ExternReturnArityMismatch {
                function,
                expected,
                got,
            } => RuntimeError::ExternReturnArityMismatch {
                function,
                expected,
                got,
            },
        })
    }

    pub fn tick_with_axis<IO: Io>(
        &mut self,
        io: &mut IO,
        mut on_axis_motion: impl FnMut(AxisMotionCommand) -> AxisMotionResult,
    ) -> Result<(), RuntimeError> {
        #[derive(Debug)]
        struct MissingExternHandler;

        let mut on_event = |_| {};
        let mut on_log = |_| {};
        let mut missing_extern =
            |_function: &'static str,
             _args: &[f32],
             _results: &mut [f32]|
             -> Result<usize, MissingExternHandler> { Err(MissingExternHandler) };
        let mut ignore_error_code = |_function: &'static str, _error: &MissingExternHandler| 0.0;
        let mut axis_adapter = |command: AxisMotionCommand| Ok(on_axis_motion(command));

        self.tick_with_trace_and_logs_impl(
            io,
            &mut on_event,
            &mut on_log,
            &mut missing_extern,
            None,
            &mut ignore_error_code,
            &mut axis_adapter,
            &mut |_: AxisMotionCommand, _: AxisFault| {},
        )
        .map_err(|err| match err {
            RuntimeTickError::Core(err) => err,
            RuntimeTickError::ExternCallFailed { function, .. } => {
                RuntimeError::ExternCallRequiresHandler { function }
            }
            RuntimeTickError::ExternReturnArityMismatch {
                function,
                expected,
                got,
            } => RuntimeError::ExternReturnArityMismatch {
                function,
                expected,
                got,
            },
        })
    }

    pub fn tick_with_axis_and_logs<IO: Io>(
        &mut self,
        io: &mut IO,
        mut on_log: impl FnMut(LogEvent),
        mut on_axis_motion: impl FnMut(AxisMotionCommand) -> AxisMotionResult,
    ) -> Result<(), RuntimeError> {
        #[derive(Debug)]
        struct MissingExternHandler;

        let mut on_event = |_| {};
        let mut missing_extern =
            |_function: &'static str,
             _args: &[f32],
             _results: &mut [f32]|
             -> Result<usize, MissingExternHandler> { Err(MissingExternHandler) };
        let mut ignore_error_code = |_function: &'static str, _error: &MissingExternHandler| 0.0;
        let mut axis_adapter = |command: AxisMotionCommand| Ok(on_axis_motion(command));

        self.tick_with_trace_and_logs_impl(
            io,
            &mut on_event,
            &mut on_log,
            &mut missing_extern,
            None,
            &mut ignore_error_code,
            &mut axis_adapter,
            &mut |_: AxisMotionCommand, _: AxisFault| {},
        )
        .map_err(|err| match err {
            RuntimeTickError::Core(err) => err,
            RuntimeTickError::ExternCallFailed { function, .. } => {
                RuntimeError::ExternCallRequiresHandler { function }
            }
            RuntimeTickError::ExternReturnArityMismatch {
                function,
                expected,
                got,
            } => RuntimeError::ExternReturnArityMismatch {
                function,
                expected,
                got,
            },
        })
    }

    pub fn tick_with_extern<IO: Io, E>(
        &mut self,
        io: &mut IO,
        mut on_extern_call: impl FnMut(&'static str, &[f32], &mut [f32]) -> Result<usize, E>,
    ) -> Result<(), RuntimeTickError<E>> {
        let mut on_event = |_| {};
        let mut on_log = |_| {};
        let mut ignore_error_code = |_function: &'static str, _error: &E| 0.0;
        let mut missing_axis = |command: AxisMotionCommand| {
            Err(RuntimeError::AxisMotionRequiresHandler {
                target: command.target,
            })
        };
        self.tick_with_trace_and_logs_impl(
            io,
            &mut on_event,
            &mut on_log,
            &mut on_extern_call,
            None,
            &mut ignore_error_code,
            &mut missing_axis,
            &mut |_: AxisMotionCommand, _: AxisFault| {},
        )
    }

    pub fn tick_with_extern_error_code<IO: Io, E>(
        &mut self,
        io: &mut IO,
        error_code_var: u16,
        mut on_extern_call: impl FnMut(&'static str, &[f32], &mut [f32]) -> Result<usize, E>,
        mut map_error_code: impl FnMut(&'static str, &E) -> f32,
    ) -> Result<(), RuntimeTickError<E>> {
        let mut on_event = |_| {};
        let mut on_log = |_| {};
        let mut missing_axis = |command: AxisMotionCommand| {
            Err(RuntimeError::AxisMotionRequiresHandler {
                target: command.target,
            })
        };
        self.tick_with_trace_and_logs_impl(
            io,
            &mut on_event,
            &mut on_log,
            &mut on_extern_call,
            Some(error_code_var),
            &mut map_error_code,
            &mut missing_axis,
            &mut |_: AxisMotionCommand, _: AxisFault| {},
        )
    }

    pub fn tick_with_trace_and_extern<IO: Io, E>(
        &mut self,
        io: &mut IO,
        mut on_event: impl FnMut(TraceEvent),
        mut on_extern_call: impl FnMut(&'static str, &[f32], &mut [f32]) -> Result<usize, E>,
    ) -> Result<(), RuntimeTickError<E>> {
        let mut on_log = |_| {};
        let mut ignore_error_code = |_function: &'static str, _error: &E| 0.0;
        let mut missing_axis = |command: AxisMotionCommand| {
            Err(RuntimeError::AxisMotionRequiresHandler {
                target: command.target,
            })
        };
        self.tick_with_trace_and_logs_impl(
            io,
            &mut on_event,
            &mut on_log,
            &mut on_extern_call,
            None,
            &mut ignore_error_code,
            &mut missing_axis,
            &mut |_: AxisMotionCommand, _: AxisFault| {},
        )
    }

    pub fn tick_with_trace_and_logs_and_extern<IO: Io, E>(
        &mut self,
        io: &mut IO,
        mut on_event: impl FnMut(TraceEvent),
        mut on_log: impl FnMut(LogEvent),
        mut on_extern_call: impl FnMut(&'static str, &[f32], &mut [f32]) -> Result<usize, E>,
    ) -> Result<(), RuntimeTickError<E>> {
        let mut ignore_error_code = |_function: &'static str, _error: &E| 0.0;
        let mut missing_axis = |command: AxisMotionCommand| {
            Err(RuntimeError::AxisMotionRequiresHandler {
                target: command.target,
            })
        };
        self.tick_with_trace_and_logs_impl(
            io,
            &mut on_event,
            &mut on_log,
            &mut on_extern_call,
            None,
            &mut ignore_error_code,
            &mut missing_axis,
            &mut |_: AxisMotionCommand, _: AxisFault| {},
        )
    }

    fn tick_with_trace_and_logs_impl<IO: Io, E>(
        &mut self,
        io: &mut IO,
        on_event: &mut impl FnMut(TraceEvent),
        on_log: &mut impl FnMut(LogEvent),
        on_extern_call: &mut impl FnMut(&'static str, &[f32], &mut [f32]) -> Result<usize, E>,
        extern_error_code_var: Option<u16>,
        map_extern_error_code: &mut impl FnMut(&'static str, &E) -> f32,
        on_axis_motion: &mut impl FnMut(AxisMotionCommand) -> Result<AxisMotionResult, RuntimeError>,
        on_axis_fault_policy: &mut impl FnMut(AxisMotionCommand, AxisFault),
    ) -> Result<(), RuntimeTickError<E>> {
        let now = io.tick();

        // PID loops are executed once per tick before state-machine evaluation. This keeps the
        // execution deterministic, and allows task actions to override the output when needed.
        self.update_pid_loops(now, io);
        self.update_cam_couplings(now, io);

        let mut transitions = 0usize;
        for task_idx in 0..self.active_task_count {
            self.active_task = task_idx;
            if self.task_contexts[task_idx].step_entered_at.is_none() {
                self.task_contexts[task_idx].step_entered_at = Some(now);
            }
            loop {
                transitions += 1;
                if transitions > 64 {
                    return Err(RuntimeTickError::Core(
                        RuntimeError::TooManyTransitionsInOneTick,
                    ));
                }

                let task = self
                    .program
                    .task(task_idx)
                    .map_err(RuntimeTickError::Core)?;
            let step_id = self.task_contexts[task_idx].current_step;
            let Some(step) = task.step(step_id) else {
                return Err(RuntimeTickError::Core(RuntimeError::InvalidStepId {
                    task: task_idx,
                    step: step_id,
                }));
            };
            let instr = step.instr;
            self.sync_task_context_for_instr(task_idx, instr);

            let entered_at = self.task_contexts[task_idx].step_entered_at.unwrap_or(now);
            let elapsed = now.0.saturating_sub(entered_at.0);

                match instr {
                    Instr::Action { actions, next } => {
                        let action_completion = ActionCompletionState::Completed;
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
                            Action::CallExtern {
                                function,
                                arg_exprs,
                                binding_vars,
                            } => {
                                let result = self.execute_extern_action(
                                    function,
                                    arg_exprs,
                                    binding_vars,
                                    on_extern_call,
                                    extern_error_code_var,
                                    map_extern_error_code,
                                )?;
                                if result == ExternActionResult::HandledFailure {
                                    break;
                                }
                            }
                            Action::CamEngage { cam_index } => {
                                self.cam_engage(cam_index).map_err(RuntimeTickError::Core)?;
                            }
                            Action::CamDisengage { cam_index } => {
                                self.cam_disengage(cam_index)
                                    .map_err(RuntimeTickError::Core)?;
                            }
                            Action::CamSwitch {
                                cam_index,
                                table_index,
                            } => {
                                self.cam_switch(cam_index, table_index)
                                    .map_err(RuntimeTickError::Core)?;
                            }
                            Action::CamPhase {
                                cam_index,
                                offset_expr,
                            } => {
                                let offset = eval_expr(&offset_expr, &self.variables);
                                self.cam_phase(cam_index, offset)
                                    .map_err(RuntimeTickError::Core)?;
                            }
                            Action::AxisMove { command } => {
                                if command.kind == AxisMoveKind::Absolute
                                    && command.require_homed
                                    && !self
                                        .axis_is_homed(command.target)
                                        .map_err(RuntimeTickError::Core)?
                                {
                                    return Err(RuntimeTickError::Core(
                                        RuntimeError::AxisNotHomed {
                                            target: command.target,
                                        },
                                    ));
                                }

                                let result = match on_axis_motion(command) {
                                    Ok(result) => result,
                                    Err(err) => {
                                        self.set_axis_homed(command.target, false)
                                            .map_err(RuntimeTickError::Core)?;
                                        return Err(RuntimeTickError::Core(err));
                                    }
                                };
                                match result {
                                    AxisMotionResult::Done => {
                                        if command.kind == AxisMoveKind::Relative {
                                            self.set_axis_homed(command.target, true)
                                                .map_err(RuntimeTickError::Core)?;
                                        }
                                    }
                                    AxisMotionResult::Fault(fault) => {
                                        self.set_axis_homed(command.target, false)
                                            .map_err(RuntimeTickError::Core)?;
                                        if let Some(policy) =
                                            self.axis_fault_policy_for(command.target).copied()
                                        {
                                            on_log(LogEvent {
                                                tick: now,
                                                task: task_idx,
                                                step: step_id,
                                                message_id: axis_fault_policy_log_message_id(
                                                    policy.severity,
                                                    policy.stop_mode,
                                                    policy.auto_reset_policy,
                                                    policy.manual_ack_required,
                                                    fault.kind,
                                                ),
                                                message: AXIS_FAULT_POLICY_LOG_MESSAGE,
                                            });
                                            self.apply_axis_stop_transition(
                                                policy.stop_mode,
                                                now,
                                                task_idx,
                                                step_id,
                                                on_log,
                                            );
                                            if policy.propagation_targets.is_empty() {
                                                on_axis_fault_policy(command, fault);
                                            } else {
                                                for target in policy.propagation_targets {
                                                    let propagated_command =
                                                        AxisMotionCommand { target, ..command };
                                                    on_axis_fault_policy(propagated_command, fault);
                                                }
                                            }
                                        }
                                        return Err(RuntimeTickError::Core(
                                            RuntimeError::AxisFault {
                                                target: command.target,
                                                fault,
                                            },
                                        ));
                                    }
                                }
                            }
                            Action::Extend { output } => io.write_digital_output(output, true),
                            Action::Retract { output } => io.write_digital_output(output, false),
                            Action::Log {
                                message_id,
                                message,
                            } => on_log(LogEvent {
                                tick: now,
                                task: task_idx,
                                step: step_id,
                                message_id,
                                message,
                            }),
                        }
                    }
                    match Self::action_completion_decision(next, action_completion) {
                        StepCompletionDecision::ContinueWith { target, reason } => {
                            self.transition(task_idx, now, target, reason, on_event)
                                .map_err(RuntimeTickError::Core)?;
                            continue;
                        }
                        StepCompletionDecision::StayOnStep => break,
                    }
                }
                Instr::Goto { target } => {
                    self.transition(task_idx, now, target, TransitionReason::Goto, on_event)
                        .map_err(RuntimeTickError::Core)?;
                    continue;
                }
                Instr::Delay { ticks, next } => {
                    match Self::delay_completion_decision(elapsed, ticks, next) {
                        StepCompletionDecision::ContinueWith { target, reason } => {
                            self.transition(task_idx, now, target, reason, on_event)
                                .map_err(RuntimeTickError::Core)?;
                            continue;
                        }
                        StepCompletionDecision::StayOnStep => break,
                    }
                }
                Instr::WaitDigital {
                    id,
                    equals,
                    next,
                    timeout,
                } => {
                    let v = io.read_digital_input(id);
                    match Self::wait_completion_decision(v == equals, elapsed, timeout, next) {
                        StepCompletionDecision::ContinueWith { target, reason } => {
                            self.transition(task_idx, now, target, reason, on_event)
                                .map_err(RuntimeTickError::Core)?;
                            continue;
                        }
                        StepCompletionDecision::StayOnStep => break,
                    }
                }
                Instr::WaitAnalog {
                    id,
                    ranges,
                    next,
                    timeout,
                } => {
                    let v = io.read_analog_input(id);
                    match Self::wait_completion_decision(
                        analog_in_selected_ranges(v, ranges),
                        elapsed,
                        timeout,
                        next,
                    ) {
                        StepCompletionDecision::ContinueWith { target, reason } => {
                            self.transition(task_idx, now, target, reason, on_event)
                                .map_err(RuntimeTickError::Core)?;
                            continue;
                        }
                        StepCompletionDecision::StayOnStep => break,
                    }
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
                    match Self::wait_completion_decision(
                        compare_f32(lhs, op, rhs),
                        elapsed,
                        timeout,
                        next,
                    ) {
                        StepCompletionDecision::ContinueWith { target, reason } => {
                            self.transition(task_idx, now, target, reason, on_event)
                                .map_err(RuntimeTickError::Core)?;
                            continue;
                        }
                        StepCompletionDecision::StayOnStep => break,
                    }
                }
                Instr::WaitCamDigital {
                    cam_index,
                    field,
                    equals,
                    next,
                    timeout,
                } => {
                    let actual = self
                        .cam_digital_field(cam_index, field)
                        .map_err(RuntimeTickError::Core)?;
                    match Self::wait_completion_decision(actual == equals, elapsed, timeout, next) {
                        StepCompletionDecision::ContinueWith { target, reason } => {
                            self.transition(task_idx, now, target, reason, on_event)
                                .map_err(RuntimeTickError::Core)?;
                            continue;
                        }
                        StepCompletionDecision::StayOnStep => break,
                    }
                }
                Instr::WaitCamAnalog {
                    cam_index,
                    field,
                    op,
                    value,
                    next,
                    timeout,
                } => {
                    let actual = self
                        .cam_analog_field(cam_index, field)
                        .map_err(RuntimeTickError::Core)?;
                    match Self::wait_completion_decision(
                        compare_f32(actual, op, value),
                        elapsed,
                        timeout,
                        next,
                    ) {
                        StepCompletionDecision::ContinueWith { target, reason } => {
                            self.transition(task_idx, now, target, reason, on_event)
                                .map_err(RuntimeTickError::Core)?;
                            continue;
                        }
                        StepCompletionDecision::StayOnStep => break,
                    }
                }
                Instr::Halt => break,
            }
        }
        }
        self.active_task = 0;

        io.advance_tick();
        Ok(())
    }

    fn action_completion_decision(
        next: StepId,
        action_completion: ActionCompletionState,
    ) -> StepCompletionDecision {
        match action_completion {
            ActionCompletionState::Completed => StepCompletionDecision::ContinueWith {
                target: next,
                reason: TransitionReason::Action,
            },
            ActionCompletionState::Pending => StepCompletionDecision::StayOnStep,
        }
    }

    fn delay_completion_decision(elapsed: u64, ticks: u64, next: StepId) -> StepCompletionDecision {
        if elapsed >= ticks {
            return StepCompletionDecision::ContinueWith {
                target: next,
                reason: TransitionReason::DelayElapsed,
            };
        }
        StepCompletionDecision::StayOnStep
    }

    fn wait_completion_decision(
        condition_satisfied: bool,
        elapsed: u64,
        timeout: Option<Timeout>,
        next: StepId,
    ) -> StepCompletionDecision {
        if condition_satisfied {
            return StepCompletionDecision::ContinueWith {
                target: next,
                reason: TransitionReason::WaitSatisfied,
            };
        }

        if let Some(tmo) = timeout {
            if elapsed >= tmo.after_ticks {
                return StepCompletionDecision::ContinueWith {
                    target: tmo.target,
                    reason: TransitionReason::Timeout,
                };
            }
        }

        StepCompletionDecision::StayOnStep
    }

    fn sync_task_context_for_instr(&mut self, task: usize, instr: Instr<'a>) {
        let ctx = &mut self.task_contexts[task];
        ctx.wait_state = match instr {
            Instr::Delay { .. } => TaskWaitState::Delay,
            Instr::WaitDigital { .. }
            | Instr::WaitAnalog { .. }
            | Instr::WaitExpr { .. }
            | Instr::WaitCamDigital { .. }
            | Instr::WaitCamAnalog { .. } => TaskWaitState::WaitCondition,
            _ => TaskWaitState::Ready,
        };

        ctx.timeout_state = match instr {
            Instr::WaitDigital {
                timeout: Some(timeout),
                ..
            }
            | Instr::WaitAnalog {
                timeout: Some(timeout),
                ..
            }
            | Instr::WaitExpr {
                timeout: Some(timeout),
                ..
            }
            | Instr::WaitCamDigital {
                timeout: Some(timeout),
                ..
            }
            | Instr::WaitCamAnalog {
                timeout: Some(timeout),
                ..
            } => TaskTimeoutState::Armed {
                after_ticks: timeout.after_ticks,
                target: timeout.target,
            },
            _ => TaskTimeoutState::Inactive,
        };

        ctx.pending_action_state = match instr {
            Instr::Action { actions, .. } => Self::pending_action_state_for_actions(actions),
            _ => TaskPendingActionState::Idle,
        };
    }

    fn pending_action_state_for_actions(actions: &[Action]) -> TaskPendingActionState {
        for action in actions {
            match action {
                Action::AxisMove { command } => {
                    return TaskPendingActionState::AxisMotion {
                        target: command.target,
                    };
                }
                Action::CallExtern { function, .. } => {
                    return TaskPendingActionState::ExternCall { function };
                }
                _ => {}
            }
        }
        TaskPendingActionState::Idle
    }

    fn execute_extern_action<E>(
        &mut self,
        function: &'static str,
        arg_exprs: &'static [ExprProgram],
        binding_vars: &'static [u16],
        on_extern_call: &mut impl FnMut(&'static str, &[f32], &mut [f32]) -> Result<usize, E>,
        extern_error_code_var: Option<u16>,
        map_extern_error_code: &mut impl FnMut(&'static str, &E) -> f32,
    ) -> Result<ExternActionResult, RuntimeTickError<E>> {
        if arg_exprs.len() > MAX_EXTERN_ARGS {
            return Err(RuntimeTickError::Core(
                RuntimeError::ExternArgumentLimitExceeded {
                    function,
                    configured: arg_exprs.len(),
                    max: MAX_EXTERN_ARGS,
                },
            ));
        }
        if binding_vars.len() > MAX_EXTERN_RETURNS {
            return Err(RuntimeTickError::Core(
                RuntimeError::ExternReturnLimitExceeded {
                    function,
                    configured: binding_vars.len(),
                    max: MAX_EXTERN_RETURNS,
                },
            ));
        }

        let mut args = [0.0_f32; MAX_EXTERN_ARGS];
        for (idx, expr) in arg_exprs.iter().enumerate() {
            args[idx] = eval_expr(expr, &self.variables);
        }

        if let Some(error_var) = extern_error_code_var {
            let error_idx = error_var as usize;
            if error_idx >= MAX_VARIABLES {
                return Err(RuntimeTickError::Core(
                    RuntimeError::ExternErrorCodeVariableOutOfRange {
                        function,
                        variable: error_var,
                    },
                ));
            }
            self.variables[error_idx] = 0.0;
        }

        let mut results = [0.0_f32; MAX_EXTERN_RETURNS];
        let produced = match on_extern_call(
            function,
            &args[..arg_exprs.len()],
            &mut results[..binding_vars.len()],
        ) {
            Ok(produced) => produced,
            Err(error) => {
                if let Some(error_var) = extern_error_code_var {
                    let code = map_extern_error_code(function, &error);
                    self.variables[error_var as usize] = code;
                    return Ok(ExternActionResult::HandledFailure);
                }
                return Err(RuntimeTickError::ExternCallFailed { function, error });
            }
        };

        if produced != binding_vars.len() {
            return Err(RuntimeTickError::ExternReturnArityMismatch {
                function,
                expected: binding_vars.len(),
                got: produced,
            });
        }

        for (result_idx, var_index) in binding_vars.iter().enumerate() {
            let idx = *var_index as usize;
            if idx >= MAX_VARIABLES {
                return Err(RuntimeTickError::Core(
                    RuntimeError::ExternBindingVariableOutOfRange {
                        function,
                        variable: *var_index,
                    },
                ));
            }
            self.variables[idx] = results[result_idx];
        }
        Ok(ExternActionResult::Completed)
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
        let adjusted = self.cam_states[cam_idx].master_pos * cfg.gear_ratio
            + self.cam_states[cam_idx].phase_offset;
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

    fn cam_analog_field(&self, cam_index: u16, field: CamAnalogField) -> Result<f32, RuntimeError> {
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

    fn axis_fault_policy_for(&self, target: &str) -> Option<&AxisFaultPolicy<'a>> {
        self.program
            .axis_fault_policies
            .iter()
            .find(|policy| policy.axis == target)
    }

    fn apply_axis_stop_transition(
        &mut self,
        stop_mode: AxisStopMode,
        tick: Tick,
        task: usize,
        step: StepId,
        on_log: &mut impl FnMut(LogEvent),
    ) {
        let transition_state = match stop_mode {
            AxisStopMode::Controlled => AxisStopState::ControlledStopping,
            AxisStopMode::Quick => AxisStopState::QuickStopping,
            AxisStopMode::Immediate => AxisStopState::ImmediateStopping,
        };

        self.axis_stop_state = transition_state;
        on_log(LogEvent {
            tick,
            task,
            step,
            message_id: axis_stop_transition_log_message_id(
                stop_mode,
                AxisStopTransitionPhase::Enter,
            ),
            message: AXIS_STOP_TRANSITION_ENTER_LOG_MESSAGE,
        });

        self.axis_stop_state = AxisStopState::Stopped;
        on_log(LogEvent {
            tick,
            task,
            step,
            message_id: axis_stop_transition_log_message_id(
                stop_mode,
                AxisStopTransitionPhase::Completed,
            ),
            message: AXIS_STOP_TRANSITION_COMPLETED_LOG_MESSAGE,
        });
    }

    fn transition(
        &mut self,
        task: usize,
        tick: Tick,
        to: StepId,
        reason: TransitionReason,
        on_event: &mut impl FnMut(TraceEvent),
    ) -> Result<(), RuntimeError> {
        if task >= self.active_task_count {
            return Err(RuntimeError::InvalidTaskIndex { task });
        }
        let ctx = &mut self.task_contexts[task];
        let from = ctx.current_step;
        ctx.current_step = to;
        ctx.step_entered_at = Some(tick);
        ctx.wait_state = TaskWaitState::Ready;
        ctx.timeout_state = TaskTimeoutState::Inactive;
        ctx.pending_action_state = TaskPendingActionState::Idle;
        on_event(TraceEvent {
            tick,
            task,
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
            ExprOp::CmpEq => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                stack[sp - 1] = if compare_f32(stack[sp - 1], CompareOp::Eq, stack[sp]) {
                    1.0
                } else {
                    0.0
                };
            }
            ExprOp::CmpNe => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                stack[sp - 1] = if compare_f32(stack[sp - 1], CompareOp::Ne, stack[sp]) {
                    1.0
                } else {
                    0.0
                };
            }
            ExprOp::CmpGt => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                stack[sp - 1] = if compare_f32(stack[sp - 1], CompareOp::Gt, stack[sp]) {
                    1.0
                } else {
                    0.0
                };
            }
            ExprOp::CmpLt => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                stack[sp - 1] = if compare_f32(stack[sp - 1], CompareOp::Lt, stack[sp]) {
                    1.0
                } else {
                    0.0
                };
            }
            ExprOp::CmpGe => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                stack[sp - 1] = if compare_f32(stack[sp - 1], CompareOp::Ge, stack[sp]) {
                    1.0
                } else {
                    0.0
                };
            }
            ExprOp::CmpLe => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                stack[sp - 1] = if compare_f32(stack[sp - 1], CompareOp::Le, stack[sp]) {
                    1.0
                } else {
                    0.0
                };
            }
            ExprOp::BoolAnd => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                let lhs = stack[sp - 1] != 0.0;
                let rhs = stack[sp] != 0.0;
                stack[sp - 1] = if lhs && rhs { 1.0 } else { 0.0 };
            }
            ExprOp::BoolOr => {
                if sp < 2 {
                    return 0.0;
                }
                sp -= 1;
                let lhs = stack[sp - 1] != 0.0;
                let rhs = stack[sp] != 0.0;
                stack[sp - 1] = if lhs || rhs { 1.0 } else { 0.0 };
            }
            ExprOp::BoolNot => {
                if sp < 1 {
                    return 0.0;
                }
                let value = stack[sp - 1] != 0.0;
                stack[sp - 1] = if value { 0.0 } else { 1.0 };
            }
        }
    }

    if sp == 0 { 0.0 } else { stack[0] }
}

const MAX_PID_LOOPS: usize = 8;
pub const MAX_ACTIVE_TASKS: usize = 64;
pub const MAX_VARIABLES: usize = 64;
pub const MAX_EXPR_OPS: usize = 32;
pub const MAX_EXPR_STACK: usize = 16;
pub const MAX_CAM_POINTS: usize = 256;
pub const MAX_CAM_COUPLINGS: usize = 8;
pub const MAX_AXIS_HOMING_TARGETS: usize = 32;
pub const MAX_EXTERN_ARGS: usize = 16;
pub const MAX_EXTERN_RETURNS: usize = 8;

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
    fn runtime_initializes_independent_task_contexts() {
        static TASK0_STEPS: [Step<'static>; 2] = [
            Step {
                name: "idle",
                instr: Instr::Halt,
            },
            Step {
                name: "entry",
                instr: Instr::Halt,
            },
        ];
        static TASK1_STEPS: [Step<'static>; 1] = [Step {
            name: "wait",
            instr: Instr::WaitDigital {
                id: DigitalInputId(0),
                equals: true,
                next: StepId(0),
                timeout: None,
            },
        }];
        static TASKS: [Task<'static>; 2] = [
            Task {
                name: "loader",
                steps: &TASK0_STEPS,
                entry: StepId(1),
            },
            Task {
                name: "unloader",
                steps: &TASK1_STEPS,
                entry: StepId(0),
            },
        ];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &[],
        };

        let rt = Runtime::new(&PROGRAM).expect("runtime init should succeed");
        assert_eq!(rt.active_task_count(), 2);
        assert_eq!(
            rt.location(),
            Location {
                task: 0,
                step: StepId(1),
            }
        );

        let task0 = rt.task_context(0).expect("task0 context");
        assert_eq!(task0.current_step, StepId(1));
        assert_eq!(task0.step_entered_at, None);
        assert_eq!(task0.wait_state, TaskWaitState::Ready);
        assert_eq!(task0.timeout_state, TaskTimeoutState::Inactive);
        assert_eq!(task0.pending_action_state, TaskPendingActionState::Idle);

        let task1 = rt.task_context(1).expect("task1 context");
        assert_eq!(task1.current_step, StepId(0));
        assert_eq!(task1.step_entered_at, None);
        assert_eq!(task1.wait_state, TaskWaitState::Ready);
        assert_eq!(task1.timeout_state, TaskTimeoutState::Inactive);
        assert_eq!(task1.pending_action_state, TaskPendingActionState::Idle);
    }

    #[test]
    fn runtime_tick_keeps_blocked_task_isolated_while_advancing_other_tasks() {
        static TASK0_STEPS: [Step<'static>; 2] = [
            Step {
                name: "wait_part",
                instr: Instr::WaitDigital {
                    id: DigitalInputId(0),
                    equals: true,
                    next: StepId(1),
                    timeout: Some(Timeout {
                        after_ticks: 3,
                        target: StepId(1),
                    }),
                },
            },
            Step {
                name: "halt",
                instr: Instr::Halt,
            },
        ];
        static TASK1_STEPS: [Step<'static>; 3] = [
            Step {
                name: "prepare_output",
                instr: Instr::Action {
                    actions: &[Action::SetDigital {
                        id: DigitalOutputId(0),
                        value: true,
                    }],
                    next: StepId(1),
                },
            },
            Step {
                name: "to_halt",
                instr: Instr::Goto { target: StepId(2) },
            },
            Step {
                name: "halt",
                instr: Instr::Halt,
            },
        ];
        static TASKS: [Task<'static>; 2] = [
            Task {
                name: "loader",
                steps: &TASK0_STEPS,
                entry: StepId(0),
            },
            Task {
                name: "background",
                steps: &TASK1_STEPS,
                entry: StepId(0),
            },
        ];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init should succeed");
        let mut events: std::vec::Vec<TraceEvent> = std::vec::Vec::new();

        rt.tick_with_trace(&mut io, |e| events.push(e))
            .expect("tick should evaluate all tasks");

        let loader = rt.task_context(0).expect("loader context");
        assert_eq!(loader.current_step, StepId(0));
        assert_eq!(loader.step_entered_at, Some(Tick(0)));
        assert_eq!(loader.wait_state, TaskWaitState::WaitCondition);
        assert_eq!(
            loader.timeout_state,
            TaskTimeoutState::Armed {
                after_ticks: 3,
                target: StepId(1),
            }
        );
        assert_eq!(loader.pending_action_state, TaskPendingActionState::Idle);

        let background = rt.task_context(1).expect("background context");
        assert_eq!(background.current_step, StepId(2));
        assert_eq!(background.step_entered_at, Some(Tick(0)));
        assert_eq!(background.wait_state, TaskWaitState::Ready);
        assert_eq!(background.timeout_state, TaskTimeoutState::Inactive);
        assert_eq!(background.pending_action_state, TaskPendingActionState::Idle);
        assert!(io.do_[0]);
        assert_eq!(
            events,
            std::vec![
                TraceEvent {
                    tick: Tick(0),
                    task: 1,
                    from: StepId(0),
                    to: StepId(1),
                    reason: TransitionReason::Action,
                },
                TraceEvent {
                    tick: Tick(0),
                    task: 1,
                    from: StepId(1),
                    to: StepId(2),
                    reason: TransitionReason::Goto,
                },
            ]
        );
        assert_eq!(rt.location().task, 0);

        io.di[0] = true;
        rt.tick(&mut io).expect("tick should satisfy wait and transition");

        let loader = rt.task_context(0).expect("loader context");
        assert_eq!(loader.current_step, StepId(1));
        assert_eq!(loader.step_entered_at, Some(Tick(1)));
        assert_eq!(loader.wait_state, TaskWaitState::Ready);
        assert_eq!(loader.timeout_state, TaskTimeoutState::Inactive);
    }

    #[test]
    fn runtime_tick_schedules_tasks_in_fixed_index_order() {
        static TASK0_ACTIONS: [Action; 1] = [Action::Log {
            message_id: 10,
            message: "task0",
        }];
        static TASK1_ACTIONS: [Action; 1] = [Action::Log {
            message_id: 20,
            message: "task1",
        }];
        static TASK0_STEPS: [Step<'static>; 2] = [
            Step {
                name: "emit_log",
                instr: Instr::Action {
                    actions: &TASK0_ACTIONS,
                    next: StepId(1),
                },
            },
            Step {
                name: "halt",
                instr: Instr::Halt,
            },
        ];
        static TASK1_STEPS: [Step<'static>; 2] = [
            Step {
                name: "emit_log",
                instr: Instr::Action {
                    actions: &TASK1_ACTIONS,
                    next: StepId(1),
                },
            },
            Step {
                name: "halt",
                instr: Instr::Halt,
            },
        ];
        static TASKS: [Task<'static>; 2] = [
            Task {
                name: "first",
                steps: &TASK0_STEPS,
                entry: StepId(0),
            },
            Task {
                name: "second",
                steps: &TASK1_STEPS,
                entry: StepId(0),
            },
        ];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init should succeed");
        let mut events: std::vec::Vec<TraceEvent> = std::vec::Vec::new();
        let mut logs: std::vec::Vec<LogEvent> = std::vec::Vec::new();

        rt.tick_with_trace_and_logs(&mut io, |e| events.push(e), |l| logs.push(l))
            .expect("tick should process both tasks");

        assert_eq!(
            events,
            std::vec![
                TraceEvent {
                    tick: Tick(0),
                    task: 0,
                    from: StepId(0),
                    to: StepId(1),
                    reason: TransitionReason::Action,
                },
                TraceEvent {
                    tick: Tick(0),
                    task: 1,
                    from: StepId(0),
                    to: StepId(1),
                    reason: TransitionReason::Action,
                },
            ]
        );
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].task, 0);
        assert_eq!(logs[0].message_id, 10);
        assert_eq!(logs[1].task, 1);
        assert_eq!(logs[1].message_id, 20);
    }

    #[test]
    fn step_completion_rules_cover_immediate_delay_wait_and_pending_paths() {
        static TASK0_STEPS: [Step<'static>; 2] = [
            Step {
                name: "immediate_set",
                instr: Instr::Action {
                    actions: &[Action::SetDigital {
                        id: DigitalOutputId(0),
                        value: true,
                    }],
                    next: StepId(1),
                },
            },
            Step {
                name: "halt",
                instr: Instr::Halt,
            },
        ];
        static TASK1_STEPS: [Step<'static>; 2] = [
            Step {
                name: "delay_two_ticks",
                instr: Instr::Delay {
                    ticks: 2,
                    next: StepId(1),
                },
            },
            Step {
                name: "halt",
                instr: Instr::Halt,
            },
        ];
        static TASK2_STEPS: [Step<'static>; 2] = [
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
                name: "halt",
                instr: Instr::Halt,
            },
        ];
        static TASKS: [Task<'static>; 3] = [
            Task {
                name: "immediate_task",
                steps: &TASK0_STEPS,
                entry: StepId(0),
            },
            Task {
                name: "delay_task",
                steps: &TASK1_STEPS,
                entry: StepId(0),
            },
            Task {
                name: "wait_task",
                steps: &TASK2_STEPS,
                entry: StepId(0),
            },
        ];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init should succeed");
        rt.tick(&mut io)
            .expect("tick should evaluate immediate/delay/wait paths");

        let immediate = rt.task_context(0).expect("immediate task");
        assert_eq!(immediate.current_step, StepId(1));
        assert_eq!(immediate.wait_state, TaskWaitState::Ready);
        assert!(io.do_[0], "immediate action should commit output before completion");

        let delay = rt.task_context(1).expect("delay task");
        assert_eq!(delay.current_step, StepId(0));
        assert_eq!(delay.wait_state, TaskWaitState::Delay);

        let wait = rt.task_context(2).expect("wait task");
        assert_eq!(wait.current_step, StepId(0));
        assert_eq!(wait.wait_state, TaskWaitState::WaitCondition);

        assert_eq!(
            Runtime::action_completion_decision(StepId(9), ActionCompletionState::Pending),
            StepCompletionDecision::StayOnStep
        );
        assert_eq!(
            Runtime::action_completion_decision(StepId(9), ActionCompletionState::Completed),
            StepCompletionDecision::ContinueWith {
                target: StepId(9),
                reason: TransitionReason::Action,
            }
        );
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
            axis_fault_policies: &[],
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
            axis_fault_policies: &[],
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
            axis_fault_policies: &[],
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
            axis_fault_policies: &[],
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
    fn binary_search_interval_covers_boundaries_exact_hits_and_inner_points() {
        let table = build_cam_table(false, &[(0.0, 0.0), (100.0, 40.0), (200.0, 100.0)]);
        assert_eq!(
            binary_search_interval(&table, 0.0),
            0,
            "表首边界应定位到首段"
        );
        assert_eq!(
            binary_search_interval(&table, 40.0),
            0,
            "区间内点应定位到对应段"
        );
        assert_eq!(
            binary_search_interval(&table, 100.0),
            1,
            "精确命中中间节点时应定位到右侧段"
        );
        assert_eq!(
            binary_search_interval(&table, 200.0),
            1,
            "表尾边界应钉在最后一段"
        );
    }

    #[test]
    fn linear_interpolate_matches_known_midpoint_precision() {
        let table = build_cam_table(false, &[(0.0, 0.0), (10.0, 20.0)]);
        let y = linear_interpolate(&table, 5.0);
        assert!(
            (y - 10.0).abs() < 1e-6,
            "线性插值中值误差应小于 1e-6，实际 {y}"
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
    fn cubic_derivative_matches_central_difference() {
        let mut table = build_cam_table(false, &[(0.0, 0.0), (10.0, 0.0)]);
        table.coeffs[0] = SplineCoeff {
            a: 0.5,
            b: 1.2,
            c: -0.3,
            d: 0.08,
        };

        let x = 3.0f32;
        let h = 1e-3f32;
        let analytical = cubic_derivative(&table, x);
        let finite_diff =
            (cubic_interpolate(&table, x + h) - cubic_interpolate(&table, x - h)) / (2.0 * h);

        assert!(
            (analytical - finite_diff).abs() < 1e-3,
            "cubic_derivative 应与有限差分近似一致，解析={analytical}, 差分={finite_diff}"
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
            axis_fault_policies: &[],
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
            axis_fault_policies: &[],
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
    fn axis_move_requires_handler_when_using_plain_tick() {
        static ACTIONS: [Action; 1] = [Action::AxisMove {
            command: AxisMotionCommand {
                target: "axis_x",
                port: "self",
                kind: AxisMoveKind::Relative,
                value: 10.0,
                speed: 2.0,
                require_homed: false,
            },
        }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "axis_run",
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
            axis_fault_policies: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).unwrap();
        let err = rt.tick(&mut io).expect_err("未提供 axis handler 应报错");
        assert_eq!(
            err,
            RuntimeError::AxisMotionRequiresHandler { target: "axis_x" }
        );
    }

    #[test]
    fn axis_move_handler_done_transitions_successfully() {
        static ACTIONS: [Action; 1] = [Action::AxisMove {
            command: AxisMotionCommand {
                target: "axis_x",
                port: "self",
                kind: AxisMoveKind::Absolute,
                value: 120.0,
                speed: 5.0,
                require_homed: false,
            },
        }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "axis_run",
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
            axis_fault_policies: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).unwrap();
        rt.tick_with_axis(&mut io, |command| {
            assert_eq!(command.target, "axis_x");
            assert_eq!(command.kind, AxisMoveKind::Absolute);
            AxisMotionResult::Done
        })
        .expect("axis handler 返回 done 应继续执行");
        assert_eq!(rt.location().step, StepId(1));
        assert_eq!(io.tick(), Tick(1));
    }

    #[test]
    fn axis_move_absolute_requires_homing_predicate() {
        static ACTIONS: [Action; 1] = [Action::AxisMove {
            command: AxisMotionCommand {
                target: "axis_x",
                port: "self",
                kind: AxisMoveKind::Absolute,
                value: 120.0,
                speed: 5.0,
                require_homed: true,
            },
        }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "axis_run",
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
            axis_fault_policies: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).unwrap();
        let mut invoked = false;
        let err = rt
            .tick_with_axis(&mut io, |_| {
                invoked = true;
                AxisMotionResult::Done
            })
            .expect_err("未回零时 absolute 应被 runtime 拦截");
        assert_eq!(err, RuntimeError::AxisNotHomed { target: "axis_x" });
        assert!(
            !invoked,
            "runtime homing guard should short-circuit handler"
        );
    }

    #[test]
    fn axis_move_relative_sets_homing_predicate_for_absolute() {
        static ACTIONS_REL: [Action; 1] = [Action::AxisMove {
            command: AxisMotionCommand {
                target: "axis_x",
                port: "self",
                kind: AxisMoveKind::Relative,
                value: 5.0,
                speed: 1.0,
                require_homed: false,
            },
        }];
        static ACTIONS_ABS: [Action; 1] = [Action::AxisMove {
            command: AxisMotionCommand {
                target: "axis_x",
                port: "self",
                kind: AxisMoveKind::Absolute,
                value: 120.0,
                speed: 5.0,
                require_homed: true,
            },
        }];
        static STEPS: [Step<'static>; 3] = [
            Step {
                name: "home",
                instr: Instr::Action {
                    actions: &ACTIONS_REL,
                    next: StepId(1),
                },
            },
            Step {
                name: "move_abs",
                instr: Instr::Action {
                    actions: &ACTIONS_ABS,
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
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).unwrap();
        rt.tick_with_axis(&mut io, |_| AxisMotionResult::Done)
            .expect("relative 应设置 homing 谓词");
        rt.tick_with_axis(&mut io, |_| AxisMotionResult::Done)
            .expect("已回零后 absolute 应允许执行");
        assert_eq!(rt.location().step, StepId(2));
    }

    #[test]
    fn axis_move_fault_invalidates_homing_predicate() {
        static ACTIONS_REL: [Action; 1] = [Action::AxisMove {
            command: AxisMotionCommand {
                target: "axis_x",
                port: "self",
                kind: AxisMoveKind::Relative,
                value: 5.0,
                speed: 1.0,
                require_homed: false,
            },
        }];
        static ACTIONS_ABS: [Action; 1] = [Action::AxisMove {
            command: AxisMotionCommand {
                target: "axis_x",
                port: "self",
                kind: AxisMoveKind::Absolute,
                value: 120.0,
                speed: 5.0,
                require_homed: true,
            },
        }];
        static STEPS: [Step<'static>; 3] = [
            Step {
                name: "home",
                instr: Instr::Action {
                    actions: &ACTIONS_REL,
                    next: StepId(1),
                },
            },
            Step {
                name: "move_abs",
                instr: Instr::Action {
                    actions: &ACTIONS_ABS,
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
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).unwrap();
        let mut call_count = 0;
        let err = rt
            .tick_with_axis(&mut io, |command| {
                call_count += 1;
                if command.kind == AxisMoveKind::Relative {
                    AxisMotionResult::Done
                } else {
                    AxisMotionResult::motion_fault(77)
                }
            })
            .expect_err("fault 应中断 absolute 并清空 homing 谓词");
        assert_eq!(
            call_count, 2,
            "single tick should execute relative then absolute"
        );
        assert_eq!(
            err,
            RuntimeError::AxisFault {
                target: "axis_x",
                fault: AxisFault::motion(77),
            }
        );

        let mut invoked = false;
        let err = rt
            .tick_with_axis(&mut io, |_| {
                invoked = true;
                AxisMotionResult::Done
            })
            .expect_err("fault 后重试 absolute 应被未回零拦截");
        assert_eq!(err, RuntimeError::AxisNotHomed { target: "axis_x" });
        assert!(!invoked, "homing guard 应在 handler 前触发");
    }

    #[test]
    fn axis_move_handler_reject_returns_classified_error() {
        static ACTIONS: [Action; 1] = [Action::AxisMove {
            command: AxisMotionCommand {
                target: "axis_x",
                port: "self",
                kind: AxisMoveKind::Relative,
                value: 5.0,
                speed: 1.0,
                require_homed: false,
            },
        }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "axis_run",
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
            axis_fault_policies: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).unwrap();
        let err = rt
            .tick_with_axis(&mut io, |_| AxisMotionResult::reject(11))
            .expect_err("reject 应返回分类错误");
        assert_eq!(
            err,
            RuntimeError::AxisFault {
                target: "axis_x",
                fault: AxisFault::reject(11),
            }
        );
    }

    #[test]
    fn axis_move_handler_motion_fault_returns_classified_error() {
        static ACTIONS: [Action; 1] = [Action::AxisMove {
            command: AxisMotionCommand {
                target: "axis_x",
                port: "self",
                kind: AxisMoveKind::Relative,
                value: 5.0,
                speed: 1.0,
                require_homed: false,
            },
        }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "axis_run",
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
            axis_fault_policies: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).unwrap();
        let err = rt
            .tick_with_axis(&mut io, |_| AxisMotionResult::motion_fault(21))
            .expect_err("motion_fault 应返回分类错误");
        assert_eq!(
            err,
            RuntimeError::AxisFault {
                target: "axis_x",
                fault: AxisFault::motion(21),
            }
        );
    }

    #[test]
    fn axis_move_handler_safety_fault_returns_classified_error() {
        static ACTIONS: [Action; 1] = [Action::AxisMove {
            command: AxisMotionCommand {
                target: "axis_x",
                port: "self",
                kind: AxisMoveKind::Relative,
                value: 5.0,
                speed: 1.0,
                require_homed: false,
            },
        }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "axis_run",
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
            axis_fault_policies: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).unwrap();
        let err = rt
            .tick_with_axis(&mut io, |_| AxisMotionResult::safety_fault(31))
            .expect_err("safety_fault 应返回分类错误");
        assert_eq!(
            err,
            RuntimeError::AxisFault {
                target: "axis_x",
                fault: AxisFault::safety(31),
            }
        );
    }

    #[test]
    fn axis_fault_policy_applies_mode_specific_stop_transitions() {
        static ACTIONS: [Action; 1] = [Action::AxisMove {
            command: AxisMotionCommand {
                target: "axis_x",
                port: "self",
                kind: AxisMoveKind::Relative,
                value: 5.0,
                speed: 1.0,
                require_homed: false,
            },
        }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "axis_run",
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

        let cases = [
            (
                AxisFaultSeverity::Recoverable,
                AxisStopMode::Controlled,
                AxisMotionResult::reject(101),
            ),
            (
                AxisFaultSeverity::NonRecoverable,
                AxisStopMode::Quick,
                AxisMotionResult::motion_fault(102),
            ),
            (
                AxisFaultSeverity::Safety,
                AxisStopMode::Immediate,
                AxisMotionResult::safety_fault(103),
            ),
        ];

        for (severity, stop_mode, axis_result) in cases {
            let policies = [AxisFaultPolicy {
                axis: "axis_x",
                severity,
                stop_mode,
                auto_reset_policy: AxisAutoResetPolicy::Never,
                manual_ack_required: true,
                propagation_scope: AxisFaultPropagationScope::SelfOnly,
                propagation_targets: &["axis_x"],
            }];
            let program = Program {
                tasks: &TASKS,
                pid_loops: &[],
                var_init: &[],
                cam_configs: &[],
                cam_tables: &[],
                axis_fault_policies: &policies,
            };

            let expected_fault = match axis_result {
                AxisMotionResult::Fault(fault) => fault,
                AxisMotionResult::Done => panic!("test case must carry fault result"),
            };

            let mut io = MemIo::new();
            let mut rt = Runtime::new(&program).expect("runtime init");
            assert_eq!(rt.axis_stop_state(), AxisStopState::Running);

            let mut logs = std::vec::Vec::new();
            let err = rt
                .tick_with_axis_and_logs(&mut io, |event| logs.push(event), |_| axis_result)
                .expect_err("fault result should be surfaced");

            assert_eq!(
                err,
                RuntimeError::AxisFault {
                    target: "axis_x",
                    fault: expected_fault,
                }
            );
            assert_eq!(rt.axis_stop_state(), AxisStopState::Stopped);
            assert_eq!(logs.len(), 3);
            assert_eq!(logs[0].message, AXIS_FAULT_POLICY_LOG_MESSAGE);
            assert_eq!(
                logs[0].message_id,
                axis_fault_policy_log_message_id(
                    severity,
                    stop_mode,
                    AxisAutoResetPolicy::Never,
                    true,
                    expected_fault.kind,
                )
            );
            assert_eq!(logs[1].message, AXIS_STOP_TRANSITION_ENTER_LOG_MESSAGE);
            assert_eq!(
                logs[1].message_id,
                axis_stop_transition_log_message_id(stop_mode, AxisStopTransitionPhase::Enter)
            );
            assert_eq!(logs[2].message, AXIS_STOP_TRANSITION_COMPLETED_LOG_MESSAGE);
            assert_eq!(
                logs[2].message_id,
                axis_stop_transition_log_message_id(stop_mode, AxisStopTransitionPhase::Completed)
            );
        }
    }

    #[test]
    fn axis_fault_policy_propagates_targets_within_same_tick() {
        static ACTIONS: [Action; 1] = [Action::AxisMove {
            command: AxisMotionCommand {
                target: "axis_x",
                port: "self",
                kind: AxisMoveKind::Relative,
                value: 5.0,
                speed: 1.0,
                require_homed: false,
            },
        }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "axis_run",
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

        let policies = [AxisFaultPolicy {
            axis: "axis_x",
            severity: AxisFaultSeverity::Safety,
            stop_mode: AxisStopMode::Immediate,
            auto_reset_policy: AxisAutoResetPolicy::Never,
            manual_ack_required: true,
            propagation_scope: AxisFaultPropagationScope::Followers,
            propagation_targets: &["axis_x", "axis_y"],
        }];
        let program = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &policies,
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&program).expect("runtime init");
        let mut on_event = |_| {};
        let mut on_log = |_| {};
        let mut on_extern_call = |_function: &'static str,
                                  _args: &[f32],
                                  _results: &mut [f32]|
         -> Result<usize, ()> { Err(()) };
        let mut map_extern_error_code = |_function: &'static str, _error: &()| 0.0;
        let mut on_axis_motion =
            |_command: AxisMotionCommand| Ok(AxisMotionResult::safety_fault(55));
        let mut applied_targets = std::vec::Vec::new();

        let err = rt.tick_with_trace_and_logs_impl(
            &mut io,
            &mut on_event,
            &mut on_log,
            &mut on_extern_call,
            None,
            &mut map_extern_error_code,
            &mut on_axis_motion,
            &mut |command: AxisMotionCommand, _fault: AxisFault| {
                applied_targets.push(command.target)
            },
        );

        assert!(matches!(
            err,
            Err(RuntimeTickError::Core(RuntimeError::AxisFault {
                target: "axis_x",
                fault,
            })) if fault == AxisFault::safety(55)
        ));
        assert_eq!(applied_targets, vec!["axis_x", "axis_y"]);
    }

    #[test]
    fn axis_fault_vendor_slot_preserves_category_and_vendor_code() {
        let fault = AxisFault::new(
            AxisFaultKind::Vendor {
                category: AxisFaultCategory::NonRecoverable,
                vendor_code: 9001,
            },
            77,
        );

        assert_eq!(fault.category, AxisFaultCategory::NonRecoverable);
        assert_eq!(fault.vendor_code, Some(9001));
        assert_eq!(fault.error_code, 77);
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
            axis_fault_policies: &[],
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
    fn eval_expr_supports_boolean_and_comparison_operators() {
        // NOT(a) OR (b AND x > 0)
        let mut vars = [0.0f32; MAX_VARIABLES];
        vars[0] = 0.0; // a = false
        vars[1] = 1.0; // b = true
        vars[2] = 0.5; // x = 0.5

        let mut ops = [ExprOp::PushLiteral(0.0); MAX_EXPR_OPS];
        ops[0] = ExprOp::PushVariable(0);
        ops[1] = ExprOp::BoolNot;
        ops[2] = ExprOp::PushVariable(1);
        ops[3] = ExprOp::PushVariable(2);
        ops[4] = ExprOp::PushLiteral(0.0);
        ops[5] = ExprOp::CmpGt;
        ops[6] = ExprOp::BoolAnd;
        ops[7] = ExprOp::BoolOr;
        let expr = ExprProgram { ops, len: 8 };
        let out = eval_expr(&expr, &vars);
        assert!(
            (out - 1.0).abs() < 1e-6,
            "NOT(false) OR (true AND 0.5 > 0) 应为 true"
        );

        vars[0] = 1.0; // a = true
        vars[1] = 0.0; // b = false
        vars[2] = -0.5; // x = -0.5
        let out2 = eval_expr(&expr, &vars);
        assert!(
            (out2 - 0.0).abs() < 1e-6,
            "NOT(true) OR (false AND -0.5 > 0) 应为 false"
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
            axis_fault_policies: &[],
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
            axis_fault_policies: &[],
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
    fn runtime_rejects_too_many_cam_couplings_at_init() {
        static STEPS: [Step<'static>; 1] = [Step {
            name: "halt",
            instr: Instr::Halt,
        }];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];

        let cam_tables: &'static [CamTableData] = Box::leak(
            vec![build_cam_table(false, &[(0.0, 0.0), (360.0, 360.0)])].into_boxed_slice(),
        );
        let cam_configs: &'static [CamCouplingConfig] = Box::leak(
            vec![
                CamCouplingConfig {
                    master_input: AnalogInputId(0),
                    slave_output: AnalogOutputId(0),
                    table_index: 0,
                    interpolation: CamInterpolation::Linear,
                    gear_ratio: 1.0,
                    initial_phase_offset: 0.0,
                    following_error_limit: 1.0,
                    slave_feedback: AnalogInputId(1),
                };
                MAX_CAM_COUPLINGS + 1
            ]
            .into_boxed_slice(),
        );
        let program = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs,
            cam_tables,
            axis_fault_policies: &[],
        };

        let err = match Runtime::new(&program) {
            Ok(_) => panic!("超过 cam coupling 上限应报错"),
            Err(err) => err,
        };
        assert_eq!(
            err,
            RuntimeError::TooManyCamCouplings {
                configured: MAX_CAM_COUPLINGS + 1,
                max: MAX_CAM_COUPLINGS,
            }
        );
    }

    #[test]
    fn runtime_rejects_invalid_initial_cam_table_index() {
        static STEPS: [Step<'static>; 1] = [Step {
            name: "halt",
            instr: Instr::Halt,
        }];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];

        let cam_tables: &'static [CamTableData] = Box::leak(
            vec![build_cam_table(false, &[(0.0, 0.0), (360.0, 360.0)])].into_boxed_slice(),
        );
        let cam_configs: &'static [CamCouplingConfig] = Box::leak(
            vec![CamCouplingConfig {
                master_input: AnalogInputId(0),
                slave_output: AnalogOutputId(0),
                table_index: 1,
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
            axis_fault_policies: &[],
        };

        let err = match Runtime::new(&program) {
            Ok(_) => panic!("初始化 table_index 越界应报错"),
            Err(err) => err,
        };
        assert_eq!(
            err,
            RuntimeError::InvalidCamTableIndex {
                cam_index: 0,
                table_index: 1,
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

        let cam_tables: &'static [CamTableData] = Box::leak(
            vec![build_cam_table(false, &[(0.0, 0.0), (360.0, 360.0)])].into_boxed_slice(),
        );
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
            axis_fault_policies: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&program).expect("runtime init");
        let err = rt.tick(&mut io).expect_err("非法 cam_index 应报错");
        assert_eq!(err, RuntimeError::InvalidCamIndex { cam_index: 1 });
    }

    #[test]
    fn cam_phase_rejects_invalid_index() {
        static PHASE_EXPR: ExprProgram = ExprProgram {
            ops: [ExprOp::PushLiteral(5.0); MAX_EXPR_OPS],
            len: 1,
        };
        static ACTIONS: [Action; 1] = [Action::CamPhase {
            cam_index: 2,
            offset_expr: PHASE_EXPR,
        }];
        static STEPS: [Step<'static>; 1] = [Step {
            name: "bad_phase",
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

        let cam_tables: &'static [CamTableData] = Box::leak(
            vec![build_cam_table(false, &[(0.0, 0.0), (360.0, 360.0)])].into_boxed_slice(),
        );
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
            axis_fault_policies: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&program).expect("runtime init");
        let err = rt.tick(&mut io).expect_err("非法 cam_index 应报错");
        assert_eq!(err, RuntimeError::InvalidCamIndex { cam_index: 2 });
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

        let cam_tables: &'static [CamTableData] = Box::leak(
            vec![build_cam_table(false, &[(0.0, 0.0), (360.0, 360.0)])].into_boxed_slice(),
        );
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
            axis_fault_policies: &[],
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
    fn cam_switch_keeps_continuity_with_ratio_phase_and_decay() {
        static ENGAGE: [Action; 1] = [Action::CamEngage { cam_index: 0 }];
        static SWITCH: [Action; 1] = [Action::CamSwitch {
            cam_index: 0,
            table_index: 1,
        }];
        static STEPS: [Step<'static>; 4] = [
            Step {
                name: "engage",
                instr: Instr::Action {
                    actions: &ENGAGE,
                    next: StepId(1),
                },
            },
            Step {
                name: "settle_one_tick",
                instr: Instr::Delay {
                    ticks: 1,
                    next: StepId(2),
                },
            },
            Step {
                name: "switch",
                instr: Instr::Action {
                    actions: &SWITCH,
                    next: StepId(3),
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

        let cam_tables: &'static [CamTableData] = Box::leak(
            vec![
                build_cam_table(true, &[(0.0, 0.0), (180.0, 180.0), (360.0, 0.0)]),
                build_cam_table(true, &[(0.0, 50.0), (180.0, 100.0), (360.0, 50.0)]),
            ]
            .into_boxed_slice(),
        );
        let cam_configs: &'static [CamCouplingConfig] = Box::leak(
            vec![CamCouplingConfig {
                master_input: AnalogInputId(0),
                slave_output: AnalogOutputId(0),
                table_index: 0,
                interpolation: CamInterpolation::Linear,
                gear_ratio: 2.0,
                initial_phase_offset: 30.0,
                following_error_limit: 9999.0,
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
            axis_fault_policies: &[],
        };

        let mut io = MemIo::new();
        io.ai[0] = 45.0;
        io.ai[1] = 0.0;

        let mut rt = Runtime::new(&program).expect("runtime init");
        rt.tick(&mut io).expect("tick0 engage");
        rt.tick(&mut io).expect("tick1 switch");
        let before_switch = io.ao[0];

        rt.tick(&mut io).expect("tick2 apply switch offset");
        let after_switch = io.ao[0];
        assert!(
            (after_switch - before_switch).abs() < 1e-4,
            "切表瞬间输出应连续，before={before_switch}, after={after_switch}"
        );
        assert_eq!(
            rt.cam_states()[0].switch_decay_ticks,
            99,
            "切表后应进入衰减路径"
        );

        let adjusted_master = io.ai[0] * 2.0 + 30.0;
        let switched_base = linear_interpolate(&cam_tables[1], adjusted_master);
        assert!(
            (after_switch - switched_base).abs() > 1e-3,
            "刚切表时应仍含 switch_offset 补偿"
        );

        rt.tick(&mut io).expect("tick3 decay continues");
        assert_eq!(rt.cam_states()[0].switch_decay_ticks, 98);
        assert!(
            (io.ao[0] - after_switch).abs() > 1e-4,
            "衰减推进后输出应发生变化"
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

        let cam_tables: &'static [CamTableData] = Box::leak(
            vec![build_cam_table(false, &[(0.0, 0.0), (360.0, 360.0)])].into_boxed_slice(),
        );
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
            axis_fault_policies: &[],
        };

        let mut io = MemIo::new();
        io.ai[0] = 20.0;
        io.ai[1] = 0.0;
        let mut rt = Runtime::new(&program).expect("runtime init");

        rt.tick(&mut io)
            .expect("tick0 should progress to wait_master");
        assert_eq!(rt.location().step, StepId(3));

        rt.tick(&mut io).expect("tick1 should satisfy wait_master");
        assert_eq!(rt.location().step, StepId(4));
        assert!(
            (io.ao[0] - 30.0).abs() < 1e-5,
            "phase offset should shift cam output"
        );
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

        let cam_tables: &'static [CamTableData] = Box::leak(
            vec![build_cam_table(false, &[(0.0, 0.0), (360.0, 360.0)])].into_boxed_slice(),
        );
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
            axis_fault_policies: &[],
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
