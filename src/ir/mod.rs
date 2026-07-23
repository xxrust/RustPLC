use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Device {
    pub name: String,
    pub kind: DeviceKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    DigitalOutput,
    DigitalInput,
    Plc,
    SolenoidValve,
    Cylinder,
    Sensor,
    Motor,
    StepperMotor,
    Vfd,
    ServoDrive,
    CamCoupling,
    AnalogInput,
    AnalogOutput,
    Pid,
    ProportionalValve,
    Gripper,
    Conveyor,
    Pump,
    Heater,
    VisionSensor,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AxisDeviceType {
    StepperMotor,
    ServoDrive,
    Motor,
    Vfd,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AxisOrientation {
    Horizontal,
    Vertical,
}

fn default_axis_orientation() -> AxisOrientation {
    AxisOrientation::Horizontal
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AxisBrakeConfig {
    pub engage_port: String,
    pub engage_value: BinaryValue,
    pub engage_confirm_port: String,
    pub engage_confirm_value: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AxisProfile {
    pub device_type: AxisDeviceType,
    pub motor_class_id: String,
    pub family_id: String,
    #[serde(default = "default_axis_orientation")]
    pub orientation: AxisOrientation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brake: Option<AxisBrakeConfig>,
    pub position_unit: String,
    pub max_speed: f32,
    pub max_acceleration: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soft_limit_min: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soft_limit_max: Option<f32>,
    pub model_ref: String,
    pub config_ref: String,
    pub motion_param_set: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VariableType {
    Float,
    Int,
    Bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VariableDef {
    pub name: String,
    pub var_type: VariableType,
    pub initial_value: f32,
    pub index: u16,
}

pub const MAX_CAM_POINTS: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SplineCoeff {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CamTableIr {
    pub name: String,
    pub periodic: bool,
    pub num_points: usize,
    pub master_positions: Vec<f32>,
    pub slave_positions: Vec<f32>,
    pub spline_coeffs: Vec<SplineCoeff>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CamInterpolation {
    Linear,
    CubicSpline,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CamCouplingDef {
    pub name: String,
    pub master: String,
    pub slave: String,
    pub table: String,
    pub interpolation: CamInterpolation,
    pub gear_ratio: f32,
    pub phase_offset: f32,
    pub following_error_limit: f32,
    pub slave_feedback: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExternFunctionDef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<ExternFunctionParam>,
    pub return_types: Vec<VariableType>,
    pub contract: ExternFunctionContract,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExternFunctionParam {
    pub name: String,
    #[serde(rename = "type")]
    pub var_type: VariableType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExternFunctionContract {
    pub rust_module: String,
    pub pure: bool,
    pub time_bound_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AxisFaultContractDef {
    pub name: String,
    pub axis: String,
    pub severity: AxisFaultSeverity,
    pub stop_mode: AxisStopMode,
    pub auto_reset_policy: AxisAutoResetPolicy,
    pub manual_ack_required: bool,
    pub propagation_scope: AxisFaultPropagationScope,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub propagation_targets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AxisFaultSeverity {
    Recoverable,
    NonRecoverable,
    Safety,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AxisStopMode {
    Controlled,
    Quick,
    Immediate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AxisAutoResetPolicy {
    Never,
    OnClear,
    Immediate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AxisFaultPropagationScope {
    #[serde(rename = "self")]
    SelfOnly,
    #[serde(rename = "group")]
    Group,
    #[serde(rename = "all")]
    All,
    #[serde(rename = "followers")]
    Followers,
    #[serde(rename = "custom")]
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PidLoop {
    pub name: String,
    pub pv: String,
    pub sp: String,
    pub kp: String,
    pub ki: String,
    pub kd: String,
    pub out: String,
    pub period_ms: u64,
    pub limit_min: String,
    pub limit_max: String,
    pub anti_windup: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionType {
    Electrical,
    Pneumatic,
    Logical,
    Analog,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TopologyLink {
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_port: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_port: Option<String>,
    pub kind: ConnectionType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StationProtocol {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owns: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StationHandshake {
    pub name: String,
    pub from_station: String,
    pub to_station: String,
    pub request: String,
    pub allow: String,
    pub complete: String,
    pub timeout_ms: u64,
    pub timeout_target_task: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_target_step: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StationTransferPoint {
    pub name: String,
    pub from_station: String,
    pub to_station: String,
    pub site: String,
    pub handshake: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerSyncContract {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controllers: Vec<String>,
    pub max_skew_ms: u64,
    pub heartbeat_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct StationProtocolModel {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controllers: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stations: Vec<StationProtocol>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub handshakes: Vec<StationHandshake>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transfer_points: Vec<StationTransferPoint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controller_syncs: Vec<ControllerSyncContract>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TopologyGraph {
    pub graph: DiGraph<Device, ConnectionType>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pid_loops: Vec<PidLoop>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<TopologyLink>,
    #[serde(default, skip_serializing_if = "StationProtocolModel::is_empty")]
    pub station_protocol: StationProtocolModel,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<VariableDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cam_tables: Vec<CamTableIr>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cam_couplings: Vec<CamCouplingDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extern_functions: Vec<ExternFunctionDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub axis_fault_contracts: Vec<AxisFaultContractDef>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub axis_profiles: BTreeMap<String, AxisProfile>,
}

impl TopologyGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            pid_loops: Vec::new(),
            links: Vec::new(),
            station_protocol: StationProtocolModel::default(),
            variables: Vec::new(),
            cam_tables: Vec::new(),
            cam_couplings: Vec::new(),
            extern_functions: Vec::new(),
            axis_fault_contracts: Vec::new(),
            axis_profiles: BTreeMap::new(),
        }
    }

    pub fn add_device(&mut self, device: Device) -> NodeIndex {
        self.graph.add_node(device)
    }

    pub fn add_connection(&mut self, from: NodeIndex, to: NodeIndex, kind: ConnectionType) {
        self.graph.add_edge(from, to, kind);
    }
}

impl StationProtocolModel {
    pub fn is_empty(&self) -> bool {
        self.controllers.is_empty()
            && self.stations.is_empty()
            && self.handshakes.is_empty()
            && self.transfer_points.is_empty()
            && self.controller_syncs.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct State {
    pub task_name: String,
    pub step_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TransitionGuard {
    Always,
    Condition {
        expression: String,
    },
    Edge {
        edge: EdgeKind,
        operand: String,
    },
    Timeout {
        duration_ms: u64,
    },
    /// Internal bounded wait used by `delay: <duration>` DSL statements.
    Delay {
        duration_ms: u64,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Rising,
    Falling,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TransitionAction {
    Extend {
        target: String,
        port: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout: Option<MotionTimeoutBranch>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        on_motion_fault: Option<MotionFaultBranch>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        on_safety_fault: Option<MotionFaultBranch>,
    },
    Retract {
        target: String,
        port: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout: Option<MotionTimeoutBranch>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        on_motion_fault: Option<MotionFaultBranch>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        on_safety_fault: Option<MotionFaultBranch>,
    },
    Set {
        target: String,
        port: String,
        value: BinaryValue,
    },
    SetAnalog {
        target: String,
        port: String,
        value_raw: String,
    },
    SetAnalogExpr {
        target: String,
        port: String,
        expr_raw: String,
    },
    Compute {
        target: String,
        expr_raw: String,
    },
    CallExtern {
        function: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args_raw: Vec<String>,
        binding: ExternCallBinding,
    },
    CamEngage {
        target: String,
    },
    CamDisengage {
        target: String,
    },
    CamSwitch {
        target: String,
        new_table: String,
    },
    CamPhase {
        target: String,
        offset_expr_raw: String,
    },
    DeviceAction {
        family: String,
        action_name: String,
        target: String,
        port: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args_raw: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        result_buckets: Vec<String>,
    },
    AxisMoveRelative {
        target: String,
        port: String,
        distance_raw: String,
        speed_raw: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        acceleration_raw: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        deceleration_raw: Option<String>,
        timeout: AxisTimeoutBranch,
        on_reject: AxisFaultBranch,
        on_motion_fault: AxisFaultBranch,
        on_safety_fault: AxisFaultBranch,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        on_reject_routes: Vec<AxisFaultRouteBranch>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        on_motion_fault_routes: Vec<AxisFaultRouteBranch>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        on_safety_fault_routes: Vec<AxisFaultRouteBranch>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        semantic_tag: Option<String>,
    },
    AxisMoveAbsolute {
        target: String,
        port: String,
        position_raw: String,
        speed_raw: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        acceleration_raw: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        deceleration_raw: Option<String>,
        #[serde(default = "default_true", skip_serializing_if = "is_true")]
        require_homed: bool,
        timeout: AxisTimeoutBranch,
        on_reject: AxisFaultBranch,
        on_motion_fault: AxisFaultBranch,
        on_safety_fault: AxisFaultBranch,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        on_reject_routes: Vec<AxisFaultRouteBranch>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        on_motion_fault_routes: Vec<AxisFaultRouteBranch>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        on_safety_fault_routes: Vec<AxisFaultRouteBranch>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        semantic_tag: Option<String>,
    },
    Log {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AxisTimeoutBranch {
    pub duration_ms: u64,
    pub target_task: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_step: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MotionTimeoutBranch {
    pub duration_ms: u64,
    pub target_task: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_step: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MotionFaultBranch {
    pub target_task: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_step: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AxisFaultCategory {
    Recoverable,
    NonRecoverable,
    Safety,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
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
    pub const fn category(&self) -> AxisFaultCategory {
        match self {
            AxisFaultKind::Reject => AxisFaultCategory::Recoverable,
            AxisFaultKind::Motion => AxisFaultCategory::NonRecoverable,
            AxisFaultKind::Safety => AxisFaultCategory::Safety,
            AxisFaultKind::Vendor { category, .. } => *category,
        }
    }

    pub const fn vendor_code(&self) -> Option<i32> {
        match self {
            AxisFaultKind::Vendor { vendor_code, .. } => Some(*vendor_code),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AxisFaultBranch {
    pub target_task: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_step: Option<String>,
    pub kind: AxisFaultKind,
    pub category: AxisFaultCategory,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AxisFaultRouteKind {
    Reject,
    Motion,
    Safety,
    Vendor,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AxisFaultRouteBranch {
    pub target_task: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_step: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<AxisFaultRouteKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<i32>,
}

impl AxisFaultRouteKind {
    pub const fn from_fault_kind(kind: &AxisFaultKind) -> Self {
        match kind {
            AxisFaultKind::Reject => AxisFaultRouteKind::Reject,
            AxisFaultKind::Motion => AxisFaultRouteKind::Motion,
            AxisFaultKind::Safety => AxisFaultRouteKind::Safety,
            AxisFaultKind::Vendor { .. } => AxisFaultRouteKind::Vendor,
        }
    }
}

impl AxisFaultRouteBranch {
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

pub fn resolve_axis_fault_route_target<'a>(
    primary: &'a AxisFaultBranch,
    routes: &'a [AxisFaultRouteBranch],
    fault_kind: &AxisFaultKind,
    error_code: i32,
) -> (&'a str, Option<&'a str>) {
    let route_kind = AxisFaultRouteKind::from_fault_kind(fault_kind);
    if let Some(route) = routes
        .iter()
        .find(|route| route.matches(route_kind, error_code))
    {
        return (&route.target_task, route.target_step.as_deref());
    }

    (&primary.target_task, primary.target_step.as_deref())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "binding", content = "targets", rename_all = "snake_case")]
pub enum ExternCallBinding {
    Single(String),
    Tuple(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BinaryValue {
    On,
    Off,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimerOperationKind {
    Start,
    Cancel,
    Reset,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimerOperation {
    pub timer_name: String,
    pub operation: TimerOperationKind,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Transition {
    pub from: State,
    pub to: State,
    pub guard: TransitionGuard,
    pub actions: Vec<TransitionAction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<WorkpieceEffect>,
    pub timers: Vec<TimerOperation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskBlockingState {
    #[default]
    Ready,
    WaitingCondition,
    WaitingDelay,
    WaitingTimeout,
    WaitingPendingAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TaskTimerContext {
    pub timer_name: String,
    pub source_state: State,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingActionContext {
    pub source_state: State,
    pub action_kind: ActionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_tag: Option<String>,
    #[serde(default)]
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TaskExecutionContext {
    pub task_name: String,
    pub entry_state: State,
    pub current_state: State,
    #[serde(default)]
    pub blocking_state: TaskBlockingState,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timers: Vec<TaskTimerContext>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_actions: Vec<PendingActionContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct StateMachine {
    pub states: Vec<State>,
    pub transitions: Vec<Transition>,
    pub initial: State,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub analog_regions: BTreeMap<String, Vec<(String, String)>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub task_contexts: Vec<TaskExecutionContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateExpr {
    pub device: String,
    #[serde(default = "default_self_port")]
    pub port: String,
    pub state: String,
}

fn default_self_port() -> String {
    "self".to_string()
}

fn default_true() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SafetyRelation {
    ConflictsWith,
    Requires,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SafetyExpr {
    State(StateExpr),
    Threshold {
        device: String,
        operator: String,
        value: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SafetyRule {
    pub left: SafetyExpr,
    pub relation: SafetyRelation,
    pub right: SafetyExpr,
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SemanticResourceMode {
    Exclusive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticResource {
    pub name: String,
    pub mode: SemanticResourceMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResourceClaimSource {
    State(StateExpr),
    ActionTag { tag: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceClaimRule {
    pub source: ResourceClaimSource,
    pub resource: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimingScope {
    Task { task: String },
    Step { task: String, step: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimingRelation {
    MustCompleteWithin,
    MustCompleteWithinWorstCase,
    MustStartAfter,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimingRule {
    pub scope: TimingScope,
    pub relation: TimingRelation,
    pub duration_ms: u64,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CausalityChain {
    pub devices: Vec<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ConstraintSet {
    pub safety: Vec<SafetyRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workpiece_types: Vec<WorkpieceTypeDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workpiece_sites: Vec<WorkpieceSiteDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workpiece_holders: Vec<WorkpieceHolderDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workpiece_carriers: Vec<WorkpieceCarrierDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub semantic_resources: Vec<SemanticResource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_claims: Vec<ResourceClaimRule>,
    pub timing: Vec<TimingRule>,
    pub causality: Vec<CausalityChain>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Extend,
    Retract,
    Set,
    SetAnalog,
    SetAnalogExpr,
    Compute,
    CallExtern,
    CamEngage,
    CamDisengage,
    CamSwitch,
    CamPhase,
    DeviceAction,
    AxisMoveRelative,
    AxisMoveAbsolute,
    Log,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkpieceTypeDef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<WorkpiecePropertyDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub normal_terminal_states: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub abnormal_terminal_states: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ingress_sites: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub normal_egress_sites: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub abnormal_egress_sites: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allows: Vec<WorkpieceAllowDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub derived_from: Vec<WorkpieceDerivationDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkpiecePropertyDef {
    pub name: String,
    #[serde(rename = "type")]
    pub property_type: WorkpiecePropertyTypeDef,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkpiecePropertyTypeDef {
    Bool,
    Enum { values: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkpieceSiteDef {
    pub name: String,
    pub kind: WorkpieceSiteKind,
    pub capacity: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkpieceSiteKind {
    WorkpieceLocation,
    CarrierLocation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkpieceHolderDef {
    pub name: String,
    pub capacity: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkpieceCarrierDef {
    pub name: String,
    pub layout: WorkpieceCarrierLayoutDef,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkpieceCarrierLayoutDef {
    Slots { count: u32 },
    Grid { rows: u32, cols: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "allow", rename_all = "snake_case")]
pub enum WorkpieceAllowDef {
    SplitInto { target: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum WorkpieceDerivationDef {
    WorkpieceType { workpiece_type: String },
    Merge { inputs: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "effect", rename_all = "snake_case")]
pub enum WorkpieceEffect {
    Acquire {
        holder: String,
        from: String,
    },
    Transfer {
        from: String,
        to: String,
    },
    Finish {
        at: String,
        terminal_state: String,
    },
    Mount {
        workpiece_type: String,
        slot: String,
    },
    Unmount {
        workpiece_type: String,
        slot: String,
        to: String,
    },
    Split {
        source_type: String,
        target_type: String,
        count: u32,
        consumed: bool,
    },
    Merge {
        inputs: Vec<String>,
        target_type: String,
        consumed_inputs: bool,
    },
    TransformCarrier {
        carrier: String,
        frame: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionRef {
    pub task_name: String,
    pub step_name: String,
    pub action_kind: ActionKind,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimeInterval {
    pub min_ms: u64,
    pub max_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionTiming {
    pub action: ActionRef,
    pub interval: TimeInterval,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TimingModel {
    pub intervals: BTreeMap<String, ActionTiming>,
}

pub fn to_pretty_json<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use petgraph::visit::EdgeRef;

    #[test]
    fn topology_graph_supports_device_nodes_and_connection_edges() {
        let mut topology = TopologyGraph::new();

        let y0 = topology.add_device(Device {
            name: "Y0".to_string(),
            kind: DeviceKind::DigitalOutput,
        });
        let valve = topology.add_device(Device {
            name: "valve_A".to_string(),
            kind: DeviceKind::SolenoidValve,
        });

        topology.add_connection(y0, valve, ConnectionType::Electrical);

        assert_eq!(topology.graph.node_count(), 2);
        assert_eq!(topology.graph.edge_count(), 1);

        let edge = topology
            .graph
            .edge_references()
            .next()
            .expect("expected one edge");
        assert_eq!(edge.source(), y0);
        assert_eq!(edge.target(), valve);
        assert_eq!(edge.weight(), &ConnectionType::Electrical);
    }

    #[test]
    fn ir_structures_are_serializable_to_pretty_json() {
        let mut topology = TopologyGraph::new();
        let y0 = topology.add_device(Device {
            name: "Y0".to_string(),
            kind: DeviceKind::DigitalOutput,
        });
        let valve = topology.add_device(Device {
            name: "valve_A".to_string(),
            kind: DeviceKind::SolenoidValve,
        });
        topology.add_connection(y0, valve, ConnectionType::Electrical);
        topology.extern_functions.push(ExternFunctionDef {
            name: "add".to_string(),
            params: vec![
                ExternFunctionParam {
                    name: "a".to_string(),
                    var_type: VariableType::Float,
                },
                ExternFunctionParam {
                    name: "b".to_string(),
                    var_type: VariableType::Float,
                },
            ],
            return_types: vec![VariableType::Float],
            contract: ExternFunctionContract {
                rust_module: "math::add".to_string(),
                pure: true,
                time_bound_us: 50,
            },
        });
        topology.axis_fault_contracts.push(AxisFaultContractDef {
            name: "axis_x_fault".to_string(),
            axis: "axis_x".to_string(),
            severity: AxisFaultSeverity::Safety,
            stop_mode: AxisStopMode::Immediate,
            auto_reset_policy: AxisAutoResetPolicy::Never,
            manual_ack_required: true,
            propagation_scope: AxisFaultPropagationScope::SelfOnly,
            propagation_targets: vec!["axis_x".to_string()],
        });

        let state_machine = StateMachine {
            states: vec![
                State {
                    task_name: "init".to_string(),
                    step_name: "extend_A".to_string(),
                },
                State {
                    task_name: "ready".to_string(),
                    step_name: "idle".to_string(),
                },
            ],
            transitions: vec![Transition {
                from: State {
                    task_name: "init".to_string(),
                    step_name: "extend_A".to_string(),
                },
                to: State {
                    task_name: "ready".to_string(),
                    step_name: "idle".to_string(),
                },
                guard: TransitionGuard::Condition {
                    expression: "sensor_A_ext == true".to_string(),
                },
                actions: vec![
                    TransitionAction::Extend {
                        target: "cyl_A".to_string(),
                        port: "self".to_string(),
                        timeout: None,
                        on_motion_fault: None,
                        on_safety_fault: None,
                    },
                    TransitionAction::AxisMoveRelative {
                        target: "axis_x".to_string(),
                        port: "self".to_string(),
                        distance_raw: "10".to_string(),
                        speed_raw: "2".to_string(),
                        acceleration_raw: Some("2".to_string()),
                        deceleration_raw: Some("2".to_string()),
                        semantic_tag: None,
                        timeout: AxisTimeoutBranch {
                            duration_ms: 500,
                            target_task: "fault".to_string(),
                            target_step: Some("timeout".to_string()),
                        },
                        on_reject: AxisFaultBranch {
                            target_task: "fault".to_string(),
                            target_step: Some("reject".to_string()),
                            kind: AxisFaultKind::Reject,
                            category: AxisFaultCategory::Recoverable,
                            vendor_code: None,
                            error_code: Some("AXIS_REJECT".to_string()),
                        },
                        on_motion_fault: AxisFaultBranch {
                            target_task: "fault".to_string(),
                            target_step: Some("motion_fault".to_string()),
                            kind: AxisFaultKind::Motion,
                            category: AxisFaultCategory::NonRecoverable,
                            vendor_code: None,
                            error_code: Some("AXIS_MOTION_FAULT".to_string()),
                        },
                        on_safety_fault: AxisFaultBranch {
                            target_task: "fault".to_string(),
                            target_step: Some("safety_fault".to_string()),
                            kind: AxisFaultKind::Safety,
                            category: AxisFaultCategory::Safety,
                            vendor_code: None,
                            error_code: Some("AXIS_SAFETY_FAULT".to_string()),
                        },
                        on_reject_routes: vec![],
                        on_motion_fault_routes: vec![],
                        on_safety_fault_routes: vec![],
                    },
                    TransitionAction::CallExtern {
                        function: "add".to_string(),
                        args_raw: vec!["left".to_string(), "right".to_string()],
                        binding: ExternCallBinding::Single("sum".to_string()),
                    },
                ],
                effects: vec![],
                timers: vec![TimerOperation {
                    timer_name: "extend_A_timeout".to_string(),
                    operation: TimerOperationKind::Start,
                    duration_ms: Some(600),
                }],
            }],
            initial: State {
                task_name: "init".to_string(),
                step_name: "extend_A".to_string(),
            },
            analog_regions: BTreeMap::new(),
            task_contexts: vec![TaskExecutionContext {
                task_name: "init".to_string(),
                entry_state: State {
                    task_name: "init".to_string(),
                    step_name: "extend_A".to_string(),
                },
                current_state: State {
                    task_name: "init".to_string(),
                    step_name: "extend_A".to_string(),
                },
                blocking_state: TaskBlockingState::Ready,
                timers: vec![TaskTimerContext {
                    timer_name: "extend_A_timeout".to_string(),
                    source_state: State {
                        task_name: "init".to_string(),
                        step_name: "extend_A".to_string(),
                    },
                    duration_ms: Some(600),
                    active: false,
                }],
                pending_actions: vec![PendingActionContext {
                    source_state: State {
                        task_name: "init".to_string(),
                        step_name: "extend_A".to_string(),
                    },
                    action_kind: ActionKind::AxisMoveRelative,
                    target: Some("axis_x".to_string()),
                    semantic_tag: None,
                    active: false,
                }],
            }],
        };

        let constraints = ConstraintSet {
            safety: vec![SafetyRule {
                left: SafetyExpr::State(StateExpr {
                    device: "cyl_A".to_string(),
                    port: "self".to_string(),
                    state: "extended".to_string(),
                }),
                relation: SafetyRelation::ConflictsWith,
                right: SafetyExpr::State(StateExpr {
                    device: "cyl_B".to_string(),
                    port: "self".to_string(),
                    state: "extended".to_string(),
                }),
                reason: Some("避免机械冲突".to_string()),
                source: None,
            }],
            workpiece_types: vec![],
            workpiece_sites: vec![],
            workpiece_holders: vec![],
            workpiece_carriers: vec![],
            semantic_resources: vec![],
            resource_claims: vec![],
            timing: vec![TimingRule {
                scope: TimingScope::Task {
                    task: "extend_cycle".to_string(),
                },
                relation: TimingRelation::MustCompleteWithin,
                duration_ms: 500,
                reason: None,
            }],
            causality: vec![CausalityChain {
                devices: vec!["Y0".to_string(), "valve_A".to_string(), "cyl_A".to_string()],
                reason: None,
            }],
        };

        let mut timing_model = TimingModel::default();
        timing_model.intervals.insert(
            "init.extend_A.extend.cyl_A".to_string(),
            ActionTiming {
                action: ActionRef {
                    task_name: "init".to_string(),
                    step_name: "extend_A".to_string(),
                    action_kind: ActionKind::Extend,
                    target: Some("cyl_A".to_string()),
                },
                interval: TimeInterval {
                    min_ms: 180,
                    max_ms: 240,
                },
            },
        );

        let topology_json = to_pretty_json(&topology).expect("topology should serialize");
        let sm_json = to_pretty_json(&state_machine).expect("state machine should serialize");
        let constraints_json = to_pretty_json(&constraints).expect("constraints should serialize");
        let timing_json = to_pretty_json(&timing_model).expect("timing model should serialize");

        assert!(topology_json.contains("graph"));
        assert!(topology_json.contains("extern_functions"));
        assert!(topology_json.contains("axis_fault_contracts"));
        assert!(sm_json.contains("transitions"));
        assert!(sm_json.contains("call_extern"));
        assert!(sm_json.contains("axis_move_relative"));
        assert!(sm_json.contains("\"kind\": \"reject\""));
        assert!(sm_json.contains("\"category\": \"recoverable\""));
        assert!(sm_json.contains("error_code"));
        assert!(sm_json.contains("task_contexts"));
        assert!(constraints_json.contains("conflicts_with"));
        assert!(timing_json.contains("intervals"));

        let decoded_topology: TopologyGraph =
            serde_json::from_str(&topology_json).expect("topology should deserialize");
        assert_eq!(decoded_topology.graph.node_count(), 2);
        assert_eq!(decoded_topology.graph.edge_count(), 1);
        assert_eq!(decoded_topology.extern_functions.len(), 1);
        assert_eq!(decoded_topology.axis_fault_contracts.len(), 1);
    }

    #[test]
    fn state_machine_can_describe_task_execution_contexts_for_concurrency() {
        let loader_entry = State {
            task_name: "loader".to_string(),
            step_name: "pick".to_string(),
        };
        let unloader_entry = State {
            task_name: "unloader".to_string(),
            step_name: "wait_ready".to_string(),
        };
        let sm = StateMachine {
            states: vec![loader_entry.clone(), unloader_entry.clone()],
            transitions: vec![],
            initial: loader_entry.clone(),
            analog_regions: BTreeMap::new(),
            task_contexts: vec![
                TaskExecutionContext {
                    task_name: "loader".to_string(),
                    entry_state: loader_entry.clone(),
                    current_state: loader_entry.clone(),
                    blocking_state: TaskBlockingState::WaitingPendingAction,
                    timers: vec![TaskTimerContext {
                        timer_name: "loader.pick.timeout_1".to_string(),
                        source_state: loader_entry.clone(),
                        duration_ms: Some(500),
                        active: true,
                    }],
                    pending_actions: vec![PendingActionContext {
                        source_state: loader_entry,
                        action_kind: ActionKind::AxisMoveAbsolute,
                        target: Some("axis_x".to_string()),
                        semantic_tag: None,
                        active: true,
                    }],
                },
                TaskExecutionContext {
                    task_name: "unloader".to_string(),
                    entry_state: unloader_entry.clone(),
                    current_state: unloader_entry,
                    blocking_state: TaskBlockingState::WaitingCondition,
                    timers: vec![],
                    pending_actions: vec![],
                },
            ],
        };

        assert_eq!(sm.task_contexts.len(), 2);
        assert_eq!(sm.task_contexts[0].entry_state.task_name, "loader");
        assert_eq!(sm.task_contexts[0].entry_state.step_name, "pick");
        assert_eq!(
            sm.task_contexts[0].pending_actions[0].target.as_deref(),
            Some("axis_x")
        );
        assert!(matches!(
            sm.task_contexts[1].blocking_state,
            TaskBlockingState::WaitingCondition
        ));
    }

    #[test]
    fn axis_fault_kind_keeps_vendor_extension_slot() {
        let kind = AxisFaultKind::Vendor {
            category: AxisFaultCategory::NonRecoverable,
            vendor_code: 1201,
        };

        assert_eq!(kind.category(), AxisFaultCategory::NonRecoverable);
        assert_eq!(kind.vendor_code(), Some(1201));
    }

    #[test]
    fn resolve_axis_fault_route_target_prefers_first_matching_route_then_fallback() {
        let primary = AxisFaultBranch {
            target_task: "fault".to_string(),
            target_step: Some("motion_default".to_string()),
            kind: AxisFaultKind::Motion,
            category: AxisFaultCategory::NonRecoverable,
            vendor_code: None,
            error_code: None,
        };
        let routes = vec![
            AxisFaultRouteBranch {
                target_task: "fault".to_string(),
                target_step: Some("motion_vendor".to_string()),
                kind: Some(AxisFaultRouteKind::Vendor),
                code: None,
            },
            AxisFaultRouteBranch {
                target_task: "fault".to_string(),
                target_step: Some("motion_code_17".to_string()),
                kind: None,
                code: Some(17),
            },
        ];

        let (vendor_task, vendor_step) = resolve_axis_fault_route_target(
            &primary,
            &routes,
            &AxisFaultKind::Vendor {
                category: AxisFaultCategory::NonRecoverable,
                vendor_code: 9901,
            },
            9901,
        );
        assert_eq!(vendor_task, "fault");
        assert_eq!(vendor_step, Some("motion_vendor"));

        let (code_task, code_step) =
            resolve_axis_fault_route_target(&primary, &routes, &AxisFaultKind::Motion, 17);
        assert_eq!(code_task, "fault");
        assert_eq!(code_step, Some("motion_code_17"));

        let (fallback_task, fallback_step) =
            resolve_axis_fault_route_target(&primary, &routes, &AxisFaultKind::Motion, 99);
        assert_eq!(fallback_task, "fault");
        assert_eq!(fallback_step, Some("motion_default"));
    }
}
