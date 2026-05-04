use crate::ast::{
    ActionStatement, ActionTarget, AxisAutoResetPolicy as AstAxisAutoResetPolicy,
    AxisFaultContractDeclaration as AstAxisFaultContractDeclaration,
    AxisFaultPropagationScope as AstAxisFaultPropagationScope,
    AxisFaultRouteDirective as AstAxisFaultRouteDirective,
    AxisFaultRouteKind as AstAxisFaultRouteKind, AxisFaultSeverity as AstAxisFaultSeverity,
    AxisStopMode as AstAxisStopMode, BinaryOperator as AstBinaryOperator, CamTableMode,
    ComparisonOperator, ConditionExpression, ConstraintsSection, ControllerIoDirection,
    DeviceAttributes, DeviceDeclaration, DeviceType, DurationValue, EffectKind as AstEffectKind,
    EffectStatement as AstEffectStatement, Expression as AstExpression,
    ExternCallBinding as AstExternCallBinding,
    ExternFunctionDeclaration as AstExternFunctionDeclaration, GotoDirective, LiteralValue,
    OnCompleteDirective, ParallelBlock, PlcProgram, PortRole, PortType, RaceBlock,
    ResourceClaimSource as AstResourceClaimSource, SafetyConstraint, SafetyOperand,
    SafetyRelation as AstSafetyRelation, SemanticResourceMode as AstSemanticResourceMode,
    StateReference, StepStatement, TaskDeclaration, TasksSection, TimeUnit, TimeoutDirective,
    TimingRelation as AstTimingRelation, TimingTarget, TopologyConnection, TopologyRelation,
    TopologySection, VariableDeclaration, VariableType as AstVariableType, WaitCondition,
    WaitStatement, WorkpieceAllowDeclaration as AstWorkpieceAllowDeclaration,
    WorkpieceCarrierLayout as AstWorkpieceCarrierLayout,
    WorkpieceDerivationDeclaration as AstWorkpieceDerivationDeclaration,
    WorkpiecePropertyType as AstWorkpiecePropertyType, WorkpieceSiteKind as AstWorkpieceSiteKind,
    WorkpieceTypeDeclaration as AstWorkpieceTypeDeclaration,
};
use crate::axis_profile::resolve_axis_profiles;
use crate::device_library::{DeviceDef, PortDef};
use crate::device_semantics;
use crate::error::PlcError;
use crate::ir::{
    ActionKind, ActionRef, ActionTiming, AxisAutoResetPolicy as IrAxisAutoResetPolicy,
    AxisFaultBranch, AxisFaultContractDef as IrAxisFaultContractDef, AxisFaultKind,
    AxisFaultPropagationScope as IrAxisFaultPropagationScope,
    AxisFaultRouteBranch as IrAxisFaultRouteBranch, AxisFaultRouteKind as IrAxisFaultRouteKind,
    AxisFaultSeverity as IrAxisFaultSeverity, AxisStopMode as IrAxisStopMode, AxisTimeoutBranch,
    BinaryValue as IrBinaryValue, CamCouplingDef, CamInterpolation, CamTableIr, CausalityChain,
    ConnectionType, ConstraintSet, Device, DeviceKind, EdgeKind as IrEdgeKind,
    ExternCallBinding as IrExternCallBinding, ExternFunctionContract as IrExternContract,
    ExternFunctionDef as IrExternFunctionDef, ExternFunctionParam as IrExternFunctionParam,
    MAX_CAM_POINTS, MotionFaultBranch as IrMotionFaultBranch,
    MotionTimeoutBranch as IrMotionTimeoutBranch, PendingActionContext as IrPendingActionContext,
    PidLoop as IrPidLoop, ResourceClaimRule as IrResourceClaimRule,
    ResourceClaimSource as IrResourceClaimSource, SafetyExpr, SafetyRelation as IrSafetyRelation,
    SafetyRule, SemanticResource as IrSemanticResource,
    SemanticResourceMode as IrSemanticResourceMode, SplineCoeff, State, StateExpr, StateMachine,
    TaskBlockingState, TaskExecutionContext, TaskTimerContext, TimeInterval, TimerOperation,
    TimerOperationKind, TimingModel, TimingRelation as IrTimingRelation, TimingRule, TimingScope,
    TopologyGraph, TopologyLink, Transition, TransitionAction, TransitionGuard, VariableDef,
    VariableType as IrVariableType, WorkpieceAllowDef as IrWorkpieceAllowDef,
    WorkpieceCarrierDef as IrWorkpieceCarrierDef,
    WorkpieceCarrierLayoutDef as IrWorkpieceCarrierLayoutDef,
    WorkpieceDerivationDef as IrWorkpieceDerivationDef, WorkpieceEffect as IrWorkpieceEffect,
    WorkpieceHolderDef as IrWorkpieceHolderDef, WorkpiecePropertyDef as IrWorkpiecePropertyDef,
    WorkpiecePropertyTypeDef as IrWorkpiecePropertyTypeDef, WorkpieceSiteDef as IrWorkpieceSiteDef,
    WorkpieceSiteKind as IrWorkpieceSiteKind, WorkpieceTypeDef as IrWorkpieceTypeDef,
};
use crate::plc_port::{
    PlcPortKind, canonical_physical_device_name, parse_physical_plc_port_ref, parse_plc_port_ref,
};
use crate::topology_semantic_gate::{
    TopologySemanticGateError, validate_removed_legacy_io_model, validate_topology_semantics,
};
use petgraph::graph::NodeIndex;
use runtime_core::MAX_VARIABLES as RUNTIME_MAX_VARIABLES;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;

include!("preprocess.rs");
include!("semantic_core.rs");
include!("state_machine.rs");
include!("tests.rs");
