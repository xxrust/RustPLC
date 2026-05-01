use crate::device_semantics::cylinder::{
    CylinderContractError, CylinderStrokeContract, CylinderStrokeVerb,
    is_end_state_port as is_cylinder_end_state_port, state_port_key,
};
use crate::ir::{
    AxisAutoResetPolicy as IrAxisAutoResetPolicy,
    AxisFaultPropagationScope as IrAxisFaultPropagationScope,
    AxisFaultRouteKind as IrAxisFaultRouteKind, AxisFaultSeverity as IrAxisFaultSeverity,
    AxisStopMode as IrAxisStopMode, BinaryValue as IrBinaryValue,
    CamInterpolation as IrCamInterpolation, ConstraintSet, DeviceKind, State, StateMachine,
    TopologyGraph, Transition, TransitionAction, TransitionGuard,
};
use crate::plc_port::{PlcPortKind, parse_physical_plc_port_ref};
use io_traits::{AnalogInputId, AnalogOutputId, DigitalInputId, DigitalOutputId};
use petgraph::Direction;
use petgraph::graph::NodeIndex;
use runtime_core::{
    Action, AnalogRange, AntiWindup, AxisAutoResetPolicy as RtAxisAutoResetPolicy, AxisFaultPolicy,
    AxisFaultPropagationScope as RtAxisFaultPropagationScope,
    AxisFaultRouteKind as RtAxisFaultRouteKind, AxisFaultRouteRule, AxisFaultRouting,
    AxisFaultSeverity as RtAxisFaultSeverity, AxisMotionCommand, AxisMoveKind,
    AxisStopMode as RtAxisStopMode, CamAnalogField, CamCouplingConfig, CamDigitalField,
    CamInterpolation as RtCamInterpolation, CamTableData, CompareOp, CylinderFaultRouting,
    DigitalCondition, ExprOp, ExprProgram, Instr, MAX_CAM_POINTS, MAX_TRACKED_DIGITAL_OUTPUTS,
    MAX_TRANSITIONS_PER_TASK_PER_TICK, PidConfig, Program,
    ResourceClaimRule as RtResourceClaimRule, ResourceClaimSource as RtResourceClaimSource,
    SemanticResource as RtSemanticResource, SemanticResourceMode as RtSemanticResourceMode,
    SplineCoeff as RtSplineCoeff, Step, StepId, Task, Timeout,
    WorkpieceHolderDef as RtWorkpieceHolderDef, WorkpieceSiteDef as RtWorkpieceSiteDef,
    WorkpieceSiteKind as RtWorkpieceSiteKind, WorkpieceTypeDef as RtWorkpieceTypeDef,
};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("tick_ms must be > 0")]
    InvalidTickMs,

    #[error("duration {duration_ms}ms is not aligned to tick_ms={tick_ms} (state {state})")]
    DurationNotAligned {
        state: String,
        duration_ms: u64,
        tick_ms: u64,
    },

    #[error("state machine initial state {state} is not present in states list")]
    MissingInitialState { state: String },

    #[error("transition from {state} points to unknown target state {target}")]
    UnknownTransitionTarget { state: String, target: String },

    #[error("unsupported transition shape for {state}: {details}")]
    UnsupportedTransitionShape { state: String, details: String },

    #[error("unsupported guard expression in {state}: {expression}")]
    UnsupportedGuardExpression { state: String, expression: String },

    #[error(
        "closed-loop cylinder {device} in {state} is missing required complementary feedback for action state {requested_state}"
    )]
    IncompleteClosedLoopCylinderMotion {
        state: String,
        device: String,
        requested_state: String,
    },

    #[error(
        "closed-loop cylinder {device} in {state} must declare both on_motion_fault and on_safety_fault when cylinder fault routing is used"
    )]
    IncompleteClosedLoopCylinderRouting { state: String, device: String },

    #[error("unsupported action in {state}: {action}")]
    UnsupportedAction { state: String, action: String },

    #[error("device {device} referenced in {state} is not defined in topology")]
    UnknownDevice { state: String, device: String },

    #[error(
        "unable to resolve a unique physical digital input for device {device} (state {state})"
    )]
    UnresolvableDigitalInput { state: String, device: String },

    #[error(
        "unable to resolve a unique physical digital output for device {device} (state {state})"
    )]
    UnresolvableDigitalOutput { state: String, device: String },

    #[error("unable to resolve a unique physical analog input for device {device} (state {state})")]
    UnresolvableAnalogInput { state: String, device: String },

    #[error(
        "unable to resolve a unique physical analog output for device {device} (state {state})"
    )]
    UnresolvableAnalogOutput { state: String, device: String },

    #[error("invalid analog literal in {state}: set_analog {target} {value_raw}")]
    InvalidAnalogLiteral {
        state: String,
        target: String,
        value_raw: String,
    },

    #[error("invalid axis literal in {state}: {field} of {target} = {value_raw}")]
    InvalidAxisLiteral {
        state: String,
        target: String,
        field: String,
        value_raw: String,
    },

    #[error("axis profile for {target} is missing in topology (state {state})")]
    MissingAxisProfile { state: String, target: String },

    #[error(
        "axis speed {speed} exceeds configured max_speed={max_speed} for {target} (state {state})"
    )]
    AxisSpeedOutOfRange {
        state: String,
        target: String,
        speed: f32,
        max_speed: f32,
    },

    #[error(
        "axis {field} {value} exceeds configured max_acceleration={max_acceleration} for {target} (state {state})"
    )]
    AxisAccelerationOutOfRange {
        state: String,
        target: String,
        field: String,
        value: f32,
        max_acceleration: f32,
    },

    #[error("unsupported analog wait guard in {state}: {expression}")]
    UnsupportedAnalogWait { state: String, expression: String },

    #[error("analog input {device} has no region table in state machine (state {state})")]
    MissingAnalogRegions { state: String, device: String },

    #[error("pid loop {pid} period_ms={period_ms} is not aligned to tick_ms={tick_ms}")]
    PidPeriodNotAligned {
        pid: String,
        period_ms: u64,
        tick_ms: u64,
    },

    #[error("pid loop {pid} has invalid literal for {field}: {value}")]
    InvalidPidLiteral {
        pid: String,
        field: String,
        value: String,
    },

    #[error(
        "Phase 1 workpiece lowering requires exactly one declared workpiece type, found {count}"
    )]
    Phase1WorkpieceTypeArity { count: usize },

    #[error("workpiece carrier {carrier} is not declared in runtime bridge metadata")]
    UnknownWorkpieceCarrier { carrier: String },

    #[error("invalid workpiece slot reference {slot}: {details}")]
    InvalidWorkpieceSlotReference { slot: String, details: String },

    #[error("unsupported workpiece effect in {state}: {effect}")]
    UnsupportedWorkpieceEffect { state: String, effect: String },

    #[error("cam_coupling {cam} references unknown cam table {table}")]
    UnknownCamTableReference { cam: String, table: String },

    #[error(
        "extern worst-case execution budget exceeded: {worst_case_us}us > tick budget {tick_budget_us}us (tick_ms={tick_ms})"
    )]
    ExternTickBudgetExceeded {
        tick_ms: u64,
        tick_budget_us: u64,
        worst_case_us: u64,
    },

    #[error("unsupported semantic resource claim `{claim}`: {detail}")]
    UnsupportedSemanticResourceClaim { claim: String, detail: String },
}

include!("runtime_bridge_lowering.rs");
include!("runtime_bridge_support.rs");
include!("runtime_bridge_tests.rs");
