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
    TooManyTransitionsInOneTick {
        task: usize,
        attempted: usize,
        per_task_cap: usize,
        active_tasks: usize,
    },
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
    InvalidSemanticResourceIndex {
        claim_index: usize,
        resource_index: u16,
        resource_count: usize,
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
    CylinderFeedbackFault {
        target: &'static str,
        fault: CylinderFeedbackFault,
    },
    WorkpieceSourceUnderflow {
        endpoint: &'static str,
    },
    WorkpieceDuplicateOccupancy {
        endpoint: &'static str,
        count: usize,
    },
    WorkpieceOverflow {
        endpoint: &'static str,
        capacity: u32,
        occupancy: usize,
    },
    WorkpieceDuplicateMount {
        slot: &'static str,
        token_id: WorkpieceTokenId,
    },
    WorkpieceTypeSourceUnderflow {
        workpiece_type: &'static str,
    },
    WorkpieceTypeSourceAmbiguity {
        workpiece_type: &'static str,
        count: usize,
    },
    WorkpieceSplitOverflow {
        workpiece_type: &'static str,
        capacity: u32,
        occupancy: usize,
    },
    WorkpieceMergeInputUnderflow {
        target_type: &'static str,
        input_ref: &'static str,
        required_type: &'static str,
    },
    WorkpieceDuplicateConsumedMergeInput {
        input_ref: &'static str,
    },
    WorkpieceMergeArityMismatch {
        target_type: &'static str,
        input_refs: usize,
        input_types: usize,
    },
    WorkpieceMergeOverflow {
        target_type: &'static str,
        capacity: u32,
        occupancy: usize,
    },
    WorkpieceEndpointUndefined {
        endpoint: &'static str,
    },
    WorkpieceTokenCapacityExceeded {
        required: usize,
        max: usize,
    },
    WorkpieceLineageCapacityExceeded {
        required: usize,
        max: usize,
    },
    WorkpieceStoreInvariantViolation {
        token_id: WorkpieceTokenId,
    },
    UnsupportedWorkpieceEffect {
        effect: &'static str,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CylinderFeedbackFault {
    OppositeFeedback,
    ContradictoryFeedback,
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
    WorkpieceAcquire {
        workpiece_type: &'static str,
        holder: &'static str,
        from: &'static str,
    },
    WorkpieceTransfer {
        from: &'static str,
        to: &'static str,
    },
    WorkpieceFinish {
        at: &'static str,
        terminal_state: &'static str,
    },
    WorkpieceMount {
        workpiece_type: &'static str,
        slot: &'static str,
    },
    WorkpieceUnmount {
        workpiece_type: &'static str,
        slot: &'static str,
        to: &'static str,
    },
    WorkpieceTransformCarrier {
        carrier: &'static str,
        frame: &'static str,
    },
    WorkpieceSplit {
        source_type: &'static str,
        target_type: &'static str,
        count: u32,
        consumed: bool,
    },
    WorkpieceMerge {
        input_refs: &'static [&'static str],
        input_types: &'static [&'static str],
        target_type: &'static str,
        consumed_inputs: bool,
    },
    Extend {
        output: DigitalOutputId,
    },
    Retract {
        output: DigitalOutputId,
    },
    CylinderMotion {
        target: &'static str,
        output: DigitalOutputId,
        expect_extended: bool,
        confirm_inputs: &'static [DigitalInputId],
        opposing_inputs: &'static [DigitalInputId],
        timeout: Option<Timeout>,
        fault_routing: Option<CylinderFaultRouting>,
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
    pub semantic_tag: Option<&'static str>,
    pub require_homed: bool,
    pub timeout: Option<Timeout>,
    pub fault_routing: Option<AxisFaultRouting>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticResourceMode {
    Exclusive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticResource<'a> {
    pub name: &'a str,
    pub mode: SemanticResourceMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceClaimSource<'a> {
    DigitalOutputState { id: DigitalOutputId, value: bool },
    ActionTag { tag: &'a str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceClaimRule<'a> {
    pub source: ResourceClaimSource<'a>,
    pub resource_index: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisFaultRouteKind {
    Reject,
    Motion,
    Safety,
    Vendor,
}

impl AxisFaultRouteKind {
    pub const fn from_fault_kind(kind: AxisFaultKind) -> Self {
        match kind {
            AxisFaultKind::Reject => Self::Reject,
            AxisFaultKind::Motion => Self::Motion,
            AxisFaultKind::Safety => Self::Safety,
            AxisFaultKind::Vendor { .. } => Self::Vendor,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxisFaultRouteRule {
    pub kind: Option<AxisFaultRouteKind>,
    pub code: Option<i32>,
    pub target: StepId,
}

impl AxisFaultRouteRule {
    pub fn matches(&self, kind: AxisFaultRouteKind, code: i32) -> bool {
        let kind_match = match self.kind {
            Some(expected) => expected == kind,
            None => true,
        };
        let code_match = match self.code {
            Some(expected) => expected == code,
            None => true,
        };
        kind_match && code_match
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxisFaultRouting {
    pub on_reject: StepId,
    pub on_motion_fault: StepId,
    pub on_safety_fault: StepId,
    pub on_reject_routes: &'static [AxisFaultRouteRule],
    pub on_motion_fault_routes: &'static [AxisFaultRouteRule],
    pub on_safety_fault_routes: &'static [AxisFaultRouteRule],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CylinderFaultRouting {
    pub on_motion_fault: StepId,
    pub on_safety_fault: StepId,
}

impl AxisFaultRouting {
    pub fn resolve_target(&self, fault: AxisFault) -> StepId {
        let route_kind = AxisFaultRouteKind::from_fault_kind(fault.kind);
        let (primary, routes) = match fault.kind {
            AxisFaultKind::Reject => (self.on_reject, self.on_reject_routes),
            AxisFaultKind::Motion => (self.on_motion_fault, self.on_motion_fault_routes),
            AxisFaultKind::Safety => (self.on_safety_fault, self.on_safety_fault_routes),
            AxisFaultKind::Vendor { category, .. } => match category {
                AxisFaultCategory::Recoverable => (self.on_reject, self.on_reject_routes),
                AxisFaultCategory::NonRecoverable => {
                    (self.on_motion_fault, self.on_motion_fault_routes)
                }
                AxisFaultCategory::Safety => (self.on_safety_fault, self.on_safety_fault_routes),
            },
        };

        routes
            .iter()
            .find(|route| route.matches(route_kind, fault.error_code))
            .map_or(primary, |route| route.target)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisMotionResult {
    Pending,
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
    WaitAllDigital {
        conditions: &'a [DigitalCondition],
        next: StepId,
        timeout: Option<Timeout>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DigitalCondition {
    pub id: DigitalInputId,
    pub equals: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Task<'a> {
    pub name: &'a str,
    pub steps: &'a [Step<'a>],
    pub entry: StepId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkpieceSiteKind {
    WorkpieceLocation,
    CarrierLocation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkpieceTypeDef<'a> {
    pub name: &'a str,
    pub normal_terminal_states: &'a [&'a str],
    pub abnormal_terminal_states: &'a [&'a str],
    pub ingress_sites: &'a [&'a str],
    pub normal_egress_sites: &'a [&'a str],
    pub abnormal_egress_sites: &'a [&'a str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkpieceSiteDef<'a> {
    pub name: &'a str,
    pub kind: WorkpieceSiteKind,
    pub capacity: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkpieceHolderDef<'a> {
    pub name: &'a str,
    pub capacity: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Program<'a> {
    pub tasks: &'a [Task<'a>],
    pub pid_loops: &'a [PidConfig],
    pub var_init: &'a [f32],
    pub cam_configs: &'a [CamCouplingConfig],
    pub cam_tables: &'a [CamTableData],
    pub axis_fault_policies: &'a [AxisFaultPolicy<'a>],
    pub semantic_resources: &'a [SemanticResource<'a>],
    pub resource_claims: &'a [ResourceClaimRule<'a>],
    pub workpiece_types: &'a [WorkpieceTypeDef<'a>],
    pub workpiece_sites: &'a [WorkpieceSiteDef<'a>],
    pub workpiece_holders: &'a [WorkpieceHolderDef<'a>],
}

pub type WorkpieceTokenId = u32;
pub type WorkpieceLineageRecordId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkpieceTerminalStatus<'a> {
    TerminalState { state: &'a str },
    Consumed,
}

include!("runtime_workpiece_store.rs");
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
    Armed { after_ticks: u64, target: StepId },
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
        action_index: usize,
        semantic_tag: Option<&'static str>,
    },
    CylinderMotion {
        target: &'static str,
        action_index: usize,
        opposing_cleared_once: bool,
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
    digital_output_shadow: [bool; MAX_TRACKED_DIGITAL_OUTPUTS],
    workpiece_tokens: WorkpieceTokenStore<'a>,
    workpiece_lineage: WorkpieceLineageStore,
    next_workpiece_token_id: WorkpieceTokenId,
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
        for (claim_index, claim) in program.resource_claims.iter().enumerate() {
            if claim.resource_index as usize >= program.semantic_resources.len() {
                return Err(RuntimeError::InvalidSemanticResourceIndex {
                    claim_index,
                    resource_index: claim.resource_index,
                    resource_count: program.semantic_resources.len(),
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
        let (workpiece_tokens, next_workpiece_token_id) = Self::seed_workpiece_tokens(program)?;

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
            digital_output_shadow: [false; MAX_TRACKED_DIGITAL_OUTPUTS],
            workpiece_tokens,
            workpiece_lineage: WorkpieceLineageStore::new(),
            next_workpiece_token_id,
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

    pub fn workpiece_tokens(&self) -> &WorkpieceTokenStore<'a> {
        &self.workpiece_tokens
    }

    pub fn workpiece_lineage(&self) -> &WorkpieceLineageStore {
        &self.workpiece_lineage
    }

    fn seed_workpiece_tokens(
        program: &'a Program<'a>,
    ) -> Result<(WorkpieceTokenStore<'a>, WorkpieceTokenId), RuntimeError> {
        let mut store = WorkpieceTokenStore::new();
        let mut next_token_id = 0u32;

        for workpiece_type in program.workpiece_types {
            for &ingress in workpiece_type.ingress_sites {
                if !Self::program_consumes_ingress_source(program, ingress) {
                    continue;
                }
                store
                    .create_token(next_token_id, workpiece_type.name, ingress)
                    .map_err(|error| match error {
                        WorkpieceTokenStoreError::CapacityExceeded { max } => {
                            RuntimeError::WorkpieceTokenCapacityExceeded {
                                required: store.slots_used().saturating_add(1),
                                max,
                            }
                        }
                        _ => RuntimeError::WorkpieceStoreInvariantViolation {
                            token_id: next_token_id,
                        },
                    })?;
                next_token_id = next_token_id.saturating_add(1);
            }
        }

        Ok((store, next_token_id))
    }

    fn program_consumes_ingress_source(program: &Program<'a>, ingress: &str) -> bool {
        program.tasks.iter().any(|task| {
            task.steps.iter().any(|step| {
                let Instr::Action { actions, .. } = step.instr else {
                    return false;
                };
                actions.iter().any(|action| match action {
                    Action::WorkpieceAcquire { from, .. }
                    | Action::WorkpieceTransfer { from, .. } => *from == ingress,
                    _ => false,
                })
            })
        })
    }

    fn workpiece_endpoint_capacity(&self, endpoint: &str) -> Option<u32> {
        self.program
            .workpiece_sites
            .iter()
            .find(|site| site.name == endpoint)
            .map(|site| site.capacity)
            .or_else(|| {
                self.program
                    .workpiece_holders
                    .iter()
                    .find(|holder| holder.name == endpoint)
                    .map(|holder| holder.capacity)
            })
    }

    fn unique_active_token_id_at(
        &self,
        endpoint: &'static str,
        mounted: Option<bool>,
    ) -> Result<WorkpieceTokenId, RuntimeError> {
        if self.workpiece_endpoint_capacity(endpoint).is_none() {
            return Err(RuntimeError::WorkpieceEndpointUndefined { endpoint });
        }

        let mut token_id = None;
        let mut count = 0usize;
        for token in self.workpiece_tokens.tokens.iter().flatten() {
            let mount_matches = match mounted {
                Some(true) => token.mounted_slot == Some(endpoint),
                Some(false) => token.mounted_slot.is_none(),
                None => true,
            };
            if token.active && token.current_location == endpoint && mount_matches {
                count = count.saturating_add(1);
                token_id.get_or_insert(token.token_id);
                if count > 1 {
                    return Err(RuntimeError::WorkpieceDuplicateOccupancy { endpoint, count });
                }
            }
        }

        token_id.ok_or(RuntimeError::WorkpieceSourceUnderflow { endpoint })
    }

    fn unique_active_token_of_type(
        &self,
        workpiece_type: &'static str,
    ) -> Result<WorkpieceToken<'a>, RuntimeError> {
        let mut token = None;
        let mut count = 0usize;
        for candidate in self.workpiece_tokens.tokens.iter().flatten() {
            if candidate.active && candidate.workpiece_type == workpiece_type {
                count = count.saturating_add(1);
                token.get_or_insert(*candidate);
                if count > 1 {
                    return Err(RuntimeError::WorkpieceTypeSourceAmbiguity {
                        workpiece_type,
                        count,
                    });
                }
            }
        }

        token.ok_or(RuntimeError::WorkpieceTypeSourceUnderflow { workpiece_type })
    }

    fn ensure_workpiece_destination_capacity(
        &self,
        endpoint: &'static str,
    ) -> Result<(), RuntimeError> {
        let Some(capacity) = self.workpiece_endpoint_capacity(endpoint) else {
            return Err(RuntimeError::WorkpieceEndpointUndefined { endpoint });
        };

        let occupancy = self.workpiece_tokens.active_tokens_at(endpoint);
        if occupancy >= capacity as usize {
            return Err(RuntimeError::WorkpieceOverflow {
                endpoint,
                capacity,
                occupancy,
            });
        }

        Ok(())
    }

    fn relocate_workpiece_token(
        &mut self,
        token_id: WorkpieceTokenId,
        to: &'a str,
        mounted_slot: Option<&'a str>,
    ) -> Result<(), RuntimeError> {
        self.workpiece_tokens
            .move_token(token_id, to)
            .and_then(|_| {
                self.workpiece_tokens
                    .set_mounted_slot(token_id, mounted_slot)
            })
            .map(|_| ())
            .map_err(|_| RuntimeError::WorkpieceStoreInvariantViolation { token_id })
    }

    fn finish_workpiece_token(
        &mut self,
        token_id: WorkpieceTokenId,
        terminal_status: WorkpieceTerminalStatus<'a>,
    ) -> Result<(), RuntimeError> {
        self.workpiece_tokens
            .finish_token(token_id, terminal_status)
            .map(|_| ())
            .map_err(|_| RuntimeError::WorkpieceStoreInvariantViolation { token_id })
    }

    fn execute_workpiece_acquire(
        &mut self,
        _workpiece_type: &'static str,
        holder: &'static str,
        from: &'static str,
    ) -> Result<(), RuntimeError> {
        let token_id = self.unique_active_token_id_at(from, Some(false))?;
        self.ensure_workpiece_destination_capacity(holder)?;
        self.relocate_workpiece_token(token_id, holder, None)
    }

    fn execute_workpiece_transfer(
        &mut self,
        from: &'static str,
        to: &'static str,
    ) -> Result<(), RuntimeError> {
        if from == to {
            return Ok(());
        }

        let token_id = self.unique_active_token_id_at(from, Some(false))?;
        self.ensure_workpiece_destination_capacity(to)?;
        self.relocate_workpiece_token(token_id, to, None)
    }

    fn execute_workpiece_finish(
        &mut self,
        at: &'static str,
        terminal_state: &'static str,
    ) -> Result<(), RuntimeError> {
        let token_id = self.unique_active_token_id_at(at, Some(false))?;
        self.finish_workpiece_token(
            token_id,
            WorkpieceTerminalStatus::TerminalState {
                state: terminal_state,
            },
        )
    }

    fn create_workpiece_token(
        &mut self,
        workpiece_type: &'a str,
        location: &'a str,
        mounted_slot: Option<&'a str>,
    ) -> Result<WorkpieceTokenId, RuntimeError> {
        let token_id = self.next_workpiece_token_id;
        self.workpiece_tokens
            .create_token(token_id, workpiece_type, location)
            .map_err(|error| match error {
                WorkpieceTokenStoreError::CapacityExceeded { max } => {
                    RuntimeError::WorkpieceTokenCapacityExceeded {
                        required: self.workpiece_tokens.slots_used().saturating_add(1),
                        max,
                    }
                }
                _ => RuntimeError::WorkpieceStoreInvariantViolation { token_id },
            })?;
        self.workpiece_tokens
            .set_mounted_slot(token_id, mounted_slot)
            .map_err(|_| RuntimeError::WorkpieceStoreInvariantViolation { token_id })?;
        self.next_workpiece_token_id = self.next_workpiece_token_id.saturating_add(1);
        Ok(token_id)
    }

    fn ensure_workpiece_token_capacity_for_new_tokens(
        &self,
        additional_tokens: usize,
    ) -> Result<(), RuntimeError> {
        let required = self
            .workpiece_tokens
            .slots_used()
            .saturating_add(additional_tokens);
        if required > MAX_WORKPIECE_TOKENS {
            return Err(RuntimeError::WorkpieceTokenCapacityExceeded {
                required,
                max: MAX_WORKPIECE_TOKENS,
            });
        }
        Ok(())
    }

    fn ensure_workpiece_lineage_capacity_for_new_records(
        &self,
        additional_records: usize,
    ) -> Result<(), RuntimeError> {
        let required = self
            .workpiece_lineage
            .len()
            .saturating_add(additional_records);
        if required > MAX_WORKPIECE_LINEAGE_RECORDS {
            return Err(RuntimeError::WorkpieceLineageCapacityExceeded {
                required,
                max: MAX_WORKPIECE_LINEAGE_RECORDS,
            });
        }
        Ok(())
    }

    fn execute_workpiece_mount(
        &mut self,
        workpiece_type: &'static str,
        slot: &'static str,
    ) -> Result<(), RuntimeError> {
        if self.workpiece_endpoint_capacity(slot).is_none() {
            return Err(RuntimeError::WorkpieceEndpointUndefined { endpoint: slot });
        }

        let mut free_token_id = None;
        for token in self.workpiece_tokens.tokens.iter().flatten() {
            if !token.active || token.current_location != slot {
                continue;
            }
            if token.mounted_slot == Some(slot) {
                return Err(RuntimeError::WorkpieceDuplicateMount {
                    slot,
                    token_id: token.token_id,
                });
            }
            if token.mounted_slot.is_none() && token.workpiece_type == workpiece_type {
                free_token_id = Some(token.token_id);
            }
        }

        if let Some(token_id) = free_token_id {
            self.workpiece_tokens
                .set_mounted_slot(token_id, Some(slot))
                .map(|_| ())
                .map_err(|_| RuntimeError::WorkpieceStoreInvariantViolation { token_id })
        } else {
            self.ensure_workpiece_destination_capacity(slot)?;
            self.create_workpiece_token(workpiece_type, slot, Some(slot))
                .map(|_| ())
        }
    }

    fn execute_workpiece_unmount(
        &mut self,
        _workpiece_type: &'static str,
        slot: &'static str,
        to: &'static str,
    ) -> Result<(), RuntimeError> {
        let token_id = self.unique_active_token_id_at(slot, Some(true))?;
        if slot == to {
            return self
                .workpiece_tokens
                .set_mounted_slot(token_id, None)
                .map(|_| ())
                .map_err(|_| RuntimeError::WorkpieceStoreInvariantViolation { token_id });
        }

        self.ensure_workpiece_destination_capacity(to)?;
        self.relocate_workpiece_token(token_id, to, None)
    }

    fn execute_workpiece_transform_carrier(
        &mut self,
        _carrier: &'static str,
        _frame: &'static str,
    ) {
        // Carrier transforms only change the carrier reference frame in this phase.
        // Mounted token association is preserved by keeping the slot binding intact.
    }

    fn execute_workpiece_split(
        &mut self,
        source_type: &'static str,
        target_type: &'static str,
        count: u32,
        consumed: bool,
    ) -> Result<(), RuntimeError> {
        let source = self.unique_active_token_of_type(source_type)?;
        let output_count = count as usize;
        self.ensure_workpiece_token_capacity_for_new_tokens(output_count)?;
        self.ensure_workpiece_lineage_capacity_for_new_records(output_count)?;
        let capacity = self
            .workpiece_endpoint_capacity(source.current_location)
            .ok_or(RuntimeError::WorkpieceStoreInvariantViolation {
                token_id: source.token_id,
            })?;
        let occupancy = self
            .workpiece_tokens
            .active_tokens_at(source.current_location);
        let final_occupancy = occupancy
            .saturating_sub(if consumed { 1 } else { 0 })
            .saturating_add(output_count);
        if final_occupancy > capacity as usize {
            return Err(RuntimeError::WorkpieceSplitOverflow {
                workpiece_type: source_type,
                capacity,
                occupancy,
            });
        }

        if consumed {
            self.finish_workpiece_token(source.token_id, WorkpieceTerminalStatus::Consumed)?;
        }

        for _ in 0..count {
            let child_id = self.create_workpiece_token(
                target_type,
                source.current_location,
                source.mounted_slot,
            )?;
            self.workpiece_lineage
                .record_split_child(source.token_id, child_id)
                .map_err(|error| match error {
                    WorkpieceLineageStoreError::CapacityExceeded { max } => {
                        RuntimeError::WorkpieceLineageCapacityExceeded {
                            required: self.workpiece_lineage.len().saturating_add(1),
                            max,
                        }
                    }
                    WorkpieceLineageStoreError::DuplicateRelation { .. } => {
                        RuntimeError::WorkpieceStoreInvariantViolation {
                            token_id: source.token_id,
                        }
                    }
                })?;
        }

        Ok(())
    }

    fn execute_workpiece_merge(
        &mut self,
        input_refs: &'static [&'static str],
        input_types: &'static [&'static str],
        target_type: &'static str,
        consumed_inputs: bool,
    ) -> Result<(), RuntimeError> {
        if input_refs.len() != input_types.len() {
            return Err(RuntimeError::WorkpieceMergeArityMismatch {
                target_type,
                input_refs: input_refs.len(),
                input_types: input_types.len(),
            });
        }

        let mut selected_tokens = [None; MAX_WORKPIECE_TOKENS];
        let mut selected_count = 0usize;
        let mut output_location = None;
        let mut output_slot = None;

        for (idx, (&input_ref, &required_type)) in
            input_refs.iter().zip(input_types.iter()).enumerate()
        {
            if consumed_inputs && input_refs[..idx].contains(&input_ref) {
                return Err(RuntimeError::WorkpieceDuplicateConsumedMergeInput { input_ref });
            }

            let mut selected = None;
            for candidate in self.workpiece_tokens.tokens.iter().flatten() {
                if !candidate.active || candidate.workpiece_type != required_type {
                    continue;
                }
                if selected_tokens[..selected_count]
                    .iter()
                    .flatten()
                    .any(|token_id| *token_id == candidate.token_id)
                {
                    continue;
                }
                selected = Some(*candidate);
                break;
            }

            let Some(token) = selected else {
                return Err(RuntimeError::WorkpieceMergeInputUnderflow {
                    target_type,
                    input_ref,
                    required_type,
                });
            };

            if output_location.is_none() {
                output_location = Some(token.current_location);
                output_slot = token.mounted_slot;
            }
            selected_tokens[selected_count] = Some(token.token_id);
            selected_count = selected_count.saturating_add(1);
        }

        let Some(output_location) = output_location else {
            return Err(RuntimeError::WorkpieceMergeArityMismatch {
                target_type,
                input_refs: input_refs.len(),
                input_types: input_types.len(),
            });
        };

        self.ensure_workpiece_token_capacity_for_new_tokens(1)?;
        self.ensure_workpiece_lineage_capacity_for_new_records(selected_count)?;
        let capacity = self.workpiece_endpoint_capacity(output_location).ok_or(
            RuntimeError::WorkpieceStoreInvariantViolation {
                token_id: selected_tokens[0].unwrap_or_default(),
            },
        )?;
        let occupancy = self.workpiece_tokens.active_tokens_at(output_location);
        let final_occupancy = occupancy
            .saturating_sub(if consumed_inputs { selected_count } else { 0 })
            .saturating_add(1);
        if final_occupancy > capacity as usize {
            return Err(RuntimeError::WorkpieceMergeOverflow {
                target_type,
                capacity,
                occupancy,
            });
        }

        if consumed_inputs {
            for token_id in selected_tokens[..selected_count].iter().flatten() {
                self.finish_workpiece_token(*token_id, WorkpieceTerminalStatus::Consumed)?;
            }
        }

        let output_token_id =
            self.create_workpiece_token(target_type, output_location, output_slot)?;
        for token_id in selected_tokens[..selected_count].iter().flatten() {
            self.workpiece_lineage
                .record_merge_input(*token_id, output_token_id)
                .map_err(|error| match error {
                    WorkpieceLineageStoreError::CapacityExceeded { max } => {
                        RuntimeError::WorkpieceLineageCapacityExceeded {
                            required: self.workpiece_lineage.len().saturating_add(1),
                            max,
                        }
                    }
                    WorkpieceLineageStoreError::DuplicateRelation { .. } => {
                        RuntimeError::WorkpieceStoreInvariantViolation {
                            token_id: *token_id,
                        }
                    }
                })?;
        }

        Ok(())
    }

    fn write_digital_output<IO: Io>(&mut self, io: &mut IO, id: DigitalOutputId, value: bool) {
        io.write_digital_output(id, value);
        if let Some(slot) = self.digital_output_shadow.get_mut(id.0 as usize) {
            *slot = value;
        }
    }

    fn active_action_tag_holders(
        &self,
        tag: &str,
        ignore_axis_motion: Option<(usize, usize)>,
    ) -> usize {
        let mut holders = 0usize;
        for task_idx in 0..self.active_task_count {
            let TaskPendingActionState::AxisMotion {
                action_index,
                semantic_tag: Some(active_tag),
                ..
            } = self.task_contexts[task_idx].pending_action_state
            else {
                continue;
            };
            if active_tag != tag {
                continue;
            }
            if ignore_axis_motion.is_some_and(|(ignore_task, ignore_action_index)| {
                ignore_task == task_idx && ignore_action_index == action_index
            }) {
                continue;
            }
            holders = holders.saturating_add(1);
        }
        holders
    }

    fn claim_holders(
        &self,
        resource_index: usize,
        ignore_axis_motion: Option<(usize, usize)>,
    ) -> usize {
        let mut holders = 0usize;
        for claim in self.program.resource_claims {
            if claim.resource_index as usize != resource_index {
                continue;
            }
            match claim.source {
                ResourceClaimSource::DigitalOutputState { id, value } => {
                    if self
                        .digital_output_shadow
                        .get(id.0 as usize)
                        .copied()
                        .unwrap_or(false)
                        == value
                    {
                        holders = holders.saturating_add(1);
                    }
                }
                ResourceClaimSource::ActionTag { tag } => {
                    holders = holders
                        .saturating_add(self.active_action_tag_holders(tag, ignore_axis_motion));
                }
            }
            if holders > 1 {
                break;
            }
        }
        holders
    }

    fn semantic_resource_conflict_for_axis_motion(
        &self,
        task_idx: usize,
        action_index: usize,
        command: AxisMotionCommand,
    ) -> Option<AxisFault> {
        let tag = command.semantic_tag?;
        for claim in self.program.resource_claims {
            let ResourceClaimSource::ActionTag { tag: claim_tag } = claim.source else {
                continue;
            };
            if claim_tag != tag {
                continue;
            }
            let resource_index = claim.resource_index as usize;
            let resource = self.program.semantic_resources.get(resource_index)?;
            if matches!(resource.mode, SemanticResourceMode::Exclusive)
                && self.claim_holders(resource_index, Some((task_idx, action_index))) > 0
            {
                return Some(AxisFault::safety(SEMANTIC_RESOURCE_CONFLICT_ERROR_CODE));
            }
        }
        None
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

        for task_idx in 0..self.active_task_count {
            self.active_task = task_idx;
            if self.task_contexts[task_idx].step_entered_at.is_none() {
                self.task_contexts[task_idx].step_entered_at = Some(now);
            }
            let mut task_transitions = 0usize;
            loop {
                task_transitions = task_transitions.saturating_add(1);
                if task_transitions > MAX_TRANSITIONS_PER_TASK_PER_TICK {
                    return Err(RuntimeTickError::Core(
                        RuntimeError::TooManyTransitionsInOneTick {
                            task: task_idx,
                            attempted: task_transitions,
                            per_task_cap: MAX_TRANSITIONS_PER_TASK_PER_TICK,
                            active_tasks: self.active_task_count,
                        },
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
                        let mut action_completion = ActionCompletionState::Completed;
                        let mut action_transition_override: Option<(StepId, TransitionReason)> =
                            None;
                        let mut action_start_index = 0usize;
                        match self.task_contexts[task_idx].pending_action_state {
                            TaskPendingActionState::AxisMotion {
                                target,
                                action_index,
                                semantic_tag: _,
                            } => {
                                if let Some(Action::AxisMove { command }) =
                                    actions.get(action_index)
                                {
                                    if command.target == target {
                                        action_start_index = action_index;
                                    } else {
                                        self.task_contexts[task_idx].pending_action_state =
                                            TaskPendingActionState::Idle;
                                    }
                                } else {
                                    self.task_contexts[task_idx].pending_action_state =
                                        TaskPendingActionState::Idle;
                                }
                            }
                            TaskPendingActionState::CylinderMotion {
                                target,
                                action_index,
                                opposing_cleared_once: _,
                            } => {
                                if let Some(Action::CylinderMotion {
                                    target: action_target,
                                    ..
                                }) = actions.get(action_index)
                                {
                                    if *action_target == target {
                                        action_start_index = action_index;
                                    } else {
                                        self.task_contexts[task_idx].pending_action_state =
                                            TaskPendingActionState::Idle;
                                    }
                                } else {
                                    self.task_contexts[task_idx].pending_action_state =
                                        TaskPendingActionState::Idle;
                                }
                            }
                            TaskPendingActionState::Idle
                            | TaskPendingActionState::ExternCall { .. } => {}
                        }
                        for (action_index, a) in actions.iter().enumerate().skip(action_start_index)
                        {
                            match *a {
                                Action::SetDigital { id, value } => {
                                    self.write_digital_output(io, id, value)
                                }
                                Action::SetAnalog { id, value } => {
                                    io.write_analog_output(id, value)
                                }
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
                                    let polling_this_action = matches!(
                                        self.task_contexts[task_idx].pending_action_state,
                                        TaskPendingActionState::AxisMotion {
                                            action_index: pending_index,
                                            ..
                                        } if pending_index == action_index
                                    );
                                    if !polling_this_action
                                        && command.kind == AxisMoveKind::Absolute
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

                                    if let Some(fault) = self
                                        .semantic_resource_conflict_for_axis_motion(
                                            task_idx,
                                            action_index,
                                            command,
                                        )
                                    {
                                        if let Some(routing) = command.fault_routing {
                                            action_transition_override = Some((
                                                routing.resolve_target(fault),
                                                TransitionReason::Action,
                                            ));
                                            break;
                                        }
                                        return Err(RuntimeTickError::Core(
                                            RuntimeError::AxisFault {
                                                target: command.target,
                                                fault,
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
                                        AxisMotionResult::Pending => {
                                            self.task_contexts[task_idx].pending_action_state =
                                                TaskPendingActionState::AxisMotion {
                                                    target: command.target,
                                                    action_index,
                                                    semantic_tag: command.semantic_tag,
                                                };
                                            if let Some(timeout) = command.timeout
                                                && elapsed >= timeout.after_ticks
                                            {
                                                self.task_contexts[task_idx].pending_action_state =
                                                    TaskPendingActionState::Idle;
                                                self.set_axis_homed(command.target, false)
                                                    .map_err(RuntimeTickError::Core)?;
                                                action_transition_override = Some((
                                                    timeout.target,
                                                    TransitionReason::Timeout,
                                                ));
                                                break;
                                            }
                                            action_completion = ActionCompletionState::Pending;
                                            break;
                                        }
                                        AxisMotionResult::Done => {
                                            self.task_contexts[task_idx].pending_action_state =
                                                TaskPendingActionState::Idle;
                                            if command.kind == AxisMoveKind::Relative {
                                                self.set_axis_homed(command.target, true)
                                                    .map_err(RuntimeTickError::Core)?;
                                            }
                                        }
                                        AxisMotionResult::Fault(fault) => {
                                            self.task_contexts[task_idx].pending_action_state =
                                                TaskPendingActionState::Idle;
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
                                                        on_axis_fault_policy(
                                                            propagated_command,
                                                            fault,
                                                        );
                                                    }
                                                }
                                            }
                                            if polling_this_action
                                                && let Some(routing) = command.fault_routing
                                            {
                                                action_transition_override = Some((
                                                    routing.resolve_target(fault),
                                                    TransitionReason::Action,
                                                ));
                                                break;
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
                                Action::Extend { output } => {
                                    self.write_digital_output(io, output, true)
                                }
                                Action::Retract { output } => {
                                    self.write_digital_output(io, output, false)
                                }
                                Action::CylinderMotion {
                                    target,
                                    output,
                                    expect_extended,
                                    confirm_inputs,
                                    opposing_inputs,
                                    timeout,
                                    fault_routing,
                                } => {
                                    self.write_digital_output(io, output, expect_extended);
                                    let opposing_cleared_once =
                                        match self.task_contexts[task_idx].pending_action_state {
                                            TaskPendingActionState::CylinderMotion {
                                                opposing_cleared_once,
                                                ..
                                            } => opposing_cleared_once,
                                            _ => false,
                                        };
                                    let confirm_active =
                                        confirm_inputs.iter().all(|id| io.read_digital_input(*id));
                                    let opposing_active =
                                        opposing_inputs.iter().any(|id| io.read_digital_input(*id));

                                    if confirm_active && opposing_active {
                                        self.task_contexts[task_idx].pending_action_state =
                                            TaskPendingActionState::Idle;
                                        if let Some(routing) = fault_routing {
                                            action_transition_override = Some((
                                                routing.on_safety_fault,
                                                TransitionReason::Action,
                                            ));
                                            break;
                                        }
                                        return Err(RuntimeTickError::Core(
                                            RuntimeError::CylinderFeedbackFault {
                                                target,
                                                fault: CylinderFeedbackFault::ContradictoryFeedback,
                                            },
                                        ));
                                    }
                                    if confirm_active {
                                        self.task_contexts[task_idx].pending_action_state =
                                            TaskPendingActionState::Idle;
                                    } else if opposing_active && opposing_cleared_once {
                                        self.task_contexts[task_idx].pending_action_state =
                                            TaskPendingActionState::Idle;
                                        if let Some(routing) = fault_routing {
                                            action_transition_override = Some((
                                                routing.on_motion_fault,
                                                TransitionReason::Action,
                                            ));
                                            break;
                                        }
                                        return Err(RuntimeTickError::Core(
                                            RuntimeError::CylinderFeedbackFault {
                                                target,
                                                fault: CylinderFeedbackFault::OppositeFeedback,
                                            },
                                        ));
                                    } else {
                                        self.task_contexts[task_idx].pending_action_state =
                                            TaskPendingActionState::CylinderMotion {
                                                target,
                                                action_index,
                                                opposing_cleared_once: opposing_cleared_once
                                                    || !opposing_active,
                                            };
                                        if let Some(timeout) = timeout
                                            && elapsed >= timeout.after_ticks
                                        {
                                            self.task_contexts[task_idx].pending_action_state =
                                                TaskPendingActionState::Idle;
                                            action_transition_override =
                                                Some((timeout.target, TransitionReason::Timeout));
                                            break;
                                        }
                                        action_completion = ActionCompletionState::Pending;
                                        break;
                                    }
                                }
                                Action::WorkpieceAcquire {
                                    workpiece_type,
                                    holder,
                                    from,
                                } => self
                                    .execute_workpiece_acquire(workpiece_type, holder, from)
                                    .map_err(RuntimeTickError::Core)?,
                                Action::WorkpieceTransfer { from, to } => self
                                    .execute_workpiece_transfer(from, to)
                                    .map_err(RuntimeTickError::Core)?,
                                Action::WorkpieceFinish { at, terminal_state } => self
                                    .execute_workpiece_finish(at, terminal_state)
                                    .map_err(RuntimeTickError::Core)?,
                                Action::WorkpieceMount {
                                    workpiece_type,
                                    slot,
                                } => self
                                    .execute_workpiece_mount(workpiece_type, slot)
                                    .map_err(RuntimeTickError::Core)?,
                                Action::WorkpieceUnmount {
                                    workpiece_type,
                                    slot,
                                    to,
                                } => self
                                    .execute_workpiece_unmount(workpiece_type, slot, to)
                                    .map_err(RuntimeTickError::Core)?,
                                Action::WorkpieceTransformCarrier { carrier, frame } => {
                                    self.execute_workpiece_transform_carrier(carrier, frame)
                                }
                                Action::WorkpieceSplit {
                                    source_type,
                                    target_type,
                                    count,
                                    consumed,
                                } => self
                                    .execute_workpiece_split(
                                        source_type,
                                        target_type,
                                        count,
                                        consumed,
                                    )
                                    .map_err(RuntimeTickError::Core)?,
                                Action::WorkpieceMerge {
                                    input_refs,
                                    input_types,
                                    target_type,
                                    consumed_inputs,
                                } => self
                                    .execute_workpiece_merge(
                                        input_refs,
                                        input_types,
                                        target_type,
                                        consumed_inputs,
                                    )
                                    .map_err(RuntimeTickError::Core)?,
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
                            if action_completion == ActionCompletionState::Pending {
                                break;
                            }
                        }
                        if let Some((target, reason)) = action_transition_override {
                            self.transition(task_idx, now, target, reason, on_event)
                                .map_err(RuntimeTickError::Core)?;
                            continue;
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
                    Instr::WaitAllDigital {
                        conditions,
                        next,
                        timeout,
                    } => {
                        let satisfied = conditions.iter().all(|condition| {
                            io.read_digital_input(condition.id) == condition.equals
                        });
                        match Self::wait_completion_decision(satisfied, elapsed, timeout, next) {
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
                        match Self::wait_completion_decision(
                            actual == equals,
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
            Instr::WaitAllDigital { .. }
            | Instr::WaitDigital { .. }
            | Instr::WaitAnalog { .. }
            | Instr::WaitExpr { .. }
            | Instr::WaitCamDigital { .. }
            | Instr::WaitCamAnalog { .. } => TaskWaitState::WaitCondition,
            _ => TaskWaitState::Ready,
        };

        ctx.timeout_state = match instr {
            Instr::WaitAllDigital {
                timeout: Some(timeout),
                ..
            }
            | Instr::WaitDigital {
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

        if !matches!(instr, Instr::Action { .. }) {
            ctx.pending_action_state = TaskPendingActionState::Idle;
        }
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

include!("runtime_helpers.rs");
#[cfg(test)]
include!("tests.rs");
