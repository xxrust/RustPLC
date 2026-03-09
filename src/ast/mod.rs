use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlcProgram {
    pub topology: TopologySection,
    pub constraints: ConstraintsSection,
    pub tasks: TasksSection,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TopologySection {
    pub devices: Vec<DeviceDeclaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connections: Vec<TopologyConnection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<VariableDeclaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cam_tables: Vec<CamTableDeclaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extern_functions: Vec<ExternFunctionDeclaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub axis_fault_contracts: Vec<AxisFaultContractDeclaration>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VariableType {
    Float,
    Int,
    Bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableDeclaration {
    #[serde(default)]
    pub line: usize,
    pub name: String,
    pub var_type: VariableType,
    pub initial_value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExternFunctionDeclaration {
    #[serde(default)]
    pub line: usize,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<ExternFunctionParameter>,
    pub return_types: Vec<VariableType>,
    pub contract: ExternFunctionContract,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExternFunctionParameter {
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
pub struct AxisFaultContractDeclaration {
    #[serde(default)]
    pub line: usize,
    pub name: String,
    pub axis: String,
    pub severity: AxisFaultSeverity,
    pub stop_mode: AxisStopMode,
    pub auto_reset_policy: AxisAutoResetPolicy,
    pub manual_ack_required: bool,
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
#[serde(rename_all = "snake_case")]
pub enum CamTableMode {
    Periodic,
    Oneshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CamPoint {
    pub master: f64,
    pub slave: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CamTableDeclaration {
    #[serde(default)]
    pub line: usize,
    pub name: String,
    pub mode: CamTableMode,
    pub points: Vec<CamPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceDeclaration {
    #[serde(default)]
    pub line: usize,
    pub name: String,
    pub device_type: DeviceType,
    pub attributes: DeviceAttributes,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceType {
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PortType {
    Digital,
    Analog,
    Pneumatic,
    Logical,
    Generic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PortRole {
    Producer,
    Consumer,
    Bidirectional,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevicePort {
    pub id: String,
    #[serde(rename = "type")]
    pub port_type: PortType,
    pub role: PortRole,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub states: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub default_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TopologyRelation {
    DrivenBy,
    ReportsTo,
    Detects,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TopologyConnection {
    pub from: String,
    pub to: String,
    pub relation: TopologyRelation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_port: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_port: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeviceAttributes {
    pub driven_by: Option<String>,
    pub reports_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<DevicePort>,
    #[serde(default, skip_serializing_if = "DeviceTags::is_empty")]
    pub tags: DeviceTags,
    pub response_time: Option<DurationValue>,
    pub stroke_time: Option<DurationValue>,
    pub retract_time: Option<DurationValue>,
    pub stroke: Option<MeasuredValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtype: Option<String>,
    pub detects: Option<StateReference>,
    pub debounce: Option<DurationValue>,
    pub inverted: Option<bool>,
    pub external: Option<bool>,
    pub rated_speed: Option<MeasuredValue>,
    pub ramp_time: Option<DurationValue>,
    pub custom_states: Option<Vec<String>>,
    pub range: Option<AnalogRange>,
    pub unit: Option<String>,

    // ===== PID minimal loop (DeviceType::Pid) =====
    // These are intentionally stored on `DeviceAttributes` so PID loops can be declared as devices
    // inside `[topology]` while keeping the DSL surface small.
    pub pv: Option<String>,
    pub sp: Option<LiteralValue>,
    pub kp: Option<f64>,
    pub ki: Option<f64>,
    pub kd: Option<f64>,
    pub out: Option<String>,
    pub period_ms: Option<u64>,
    pub limit: Option<AnalogRange>,
    pub master: Option<String>,
    pub slave: Option<String>,
    pub table: Option<String>,
    pub interpolation: Option<String>,
    pub gear_ratio: Option<f64>,
    pub phase_offset: Option<f64>,
    pub following_error_limit: Option<f64>,
    pub slave_feedback: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion_param_set: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub extra_params: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct DeviceTags {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub functional_group: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub danger_level: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub location_group: Vec<String>,
}

impl DeviceTags {
    pub fn is_empty(&self) -> bool {
        self.functional_group.is_empty()
            && self.danger_level.is_empty()
            && self.location_group.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DurationValue {
    pub value: u64,
    pub unit: TimeUnit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeUnit {
    Ms,
    S,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeasuredValue {
    pub value: f64,
    pub unit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalogRange {
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateReference {
    pub device: String,
    #[serde(default = "default_port")]
    pub port: String,
    pub state: String,
}

fn default_port() -> String {
    "self".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConstraintsSection {
    pub safety: Vec<SafetyConstraint>,
    pub timing: Vec<TimingConstraint>,
    pub causality: Vec<CausalityConstraint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyConstraint {
    #[serde(default)]
    pub line: usize,
    pub left: SafetyOperand,
    pub relation: SafetyRelation,
    pub right: SafetyOperand,
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SafetyOperand {
    State(StateReference),
    Threshold {
        device: String,
        operator: ComparisonOperator,
        value: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyRelation {
    ConflictsWith,
    Requires,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingConstraint {
    #[serde(default)]
    pub line: usize,
    pub target: TimingTarget,
    pub relation: TimingRelation,
    pub duration: DurationValue,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimingRelation {
    MustCompleteWithin,
    MustCompleteWithinWorstCase,
    MustStartAfter,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimingTarget {
    Task { task: String },
    Step { task: String, step: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalityConstraint {
    #[serde(default)]
    pub line: usize,
    pub chain: Vec<StateReference>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TasksSection {
    pub tasks: Vec<TaskDeclaration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDeclaration {
    #[serde(default)]
    pub line: usize,
    pub name: String,
    pub steps: Vec<StepDeclaration>,
    #[serde(default)]
    pub on_complete_line: Option<usize>,
    pub on_complete: Option<OnCompleteDirective>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepDeclaration {
    #[serde(default)]
    pub line: usize,
    pub name: String,
    pub statements: Vec<StepStatement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "statement", rename_all = "snake_case")]
pub enum StepStatement {
    Action(ActionStatement),
    Wait(WaitStatement),
    IfElse {
        condition: ConditionExpression,
        then_goto: GotoDirective,
        else_goto: GotoDirective,
    },
    Delay {
        duration_ms: u64,
    },
    Repeat {
        count: u64,
        body: Vec<StepStatement>,
    },
    Timeout(TimeoutDirective),
    Goto(GotoDirective),
    Parallel(ParallelBlock),
    Race(RaceBlock),
    AllowIndefiniteWait(bool),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ActionTarget {
    pub device: String,
    #[serde(default = "default_port")]
    pub port: String,
}

impl std::fmt::Display for ActionTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.port == "self" {
            write!(f, "{}", self.device)
        } else {
            write!(f, "{}.{}", self.device, self.port)
        }
    }
}

impl ActionTarget {
    pub fn simple(device: impl Into<String>) -> Self {
        Self {
            device: device.into(),
            port: "self".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ActionStatement {
    Extend {
        target: ActionTarget,
    },
    Retract {
        target: ActionTarget,
    },
    Set {
        target: ActionTarget,
        value: String,
    },
    SetAnalog {
        target: ActionTarget,
        value: f64,
    },
    SetAnalogExpr {
        target: ActionTarget,
        expr: Expression,
    },
    Compute {
        target: String,
        expr: Expression,
    },
    Call {
        function: String,
        args: Vec<Expression>,
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
        offset: Expression,
    },
    AxisMoveRelative {
        target: ActionTarget,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        params: Option<String>,
        distance: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        speed: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        acceleration: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        deceleration: Option<f64>,
        timeout: Option<TimeoutDirective>,
        on_reject: Option<GotoDirective>,
        on_motion_fault: Option<GotoDirective>,
        on_safety_fault: Option<GotoDirective>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        on_reject_routes: Vec<AxisFaultRouteDirective>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        on_motion_fault_routes: Vec<AxisFaultRouteDirective>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        on_safety_fault_routes: Vec<AxisFaultRouteDirective>,
    },
    AxisMoveAbsolute {
        target: ActionTarget,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        params: Option<String>,
        position: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        speed: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        acceleration: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        deceleration: Option<f64>,
        timeout: Option<TimeoutDirective>,
        on_reject: Option<GotoDirective>,
        on_motion_fault: Option<GotoDirective>,
        on_safety_fault: Option<GotoDirective>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        on_reject_routes: Vec<AxisFaultRouteDirective>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        on_motion_fault_routes: Vec<AxisFaultRouteDirective>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        on_safety_fault_routes: Vec<AxisFaultRouteDirective>,
    },
    Log {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "binding", content = "targets", rename_all = "snake_case")]
pub enum ExternCallBinding {
    Single(String),
    Tuple(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Expression {
    Literal(f64),
    Boolean(bool),
    Variable(String),
    UnaryNeg(Box<Expression>),
    UnaryNot(Box<Expression>),
    BinaryOp {
        op: BinaryOperator,
        left: Box<Expression>,
        right: Box<Expression>,
    },
    FunctionCall {
        name: String,
        args: Vec<Expression>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BinaryOperator {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Neq,
    Gt,
    Lt,
    Gte,
    Lte,
    And,
    Or,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitStatement {
    pub condition: WaitCondition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum WaitCondition {
    Single(ConditionExpression),
    And(Vec<ConditionExpression>),
    Or(Vec<ConditionExpression>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionExpression {
    pub left: String,
    pub operator: ComparisonOperator,
    pub right: LiteralValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_expr: Option<Expression>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_expr: Option<Expression>,
}

impl ConditionExpression {
    pub fn legacy(left: String, operator: ComparisonOperator, right: LiteralValue) -> Self {
        Self {
            left,
            operator,
            right,
            left_expr: None,
            right_expr: None,
        }
    }

    pub fn expression(
        left_expr: Expression,
        operator: ComparisonOperator,
        right_expr: Expression,
    ) -> Self {
        Self {
            left: String::new(),
            operator,
            right: LiteralValue::Number(0.0),
            left_expr: Some(left_expr),
            right_expr: Some(right_expr),
        }
    }

    pub fn expression_pair(&self) -> Option<(&Expression, &Expression)> {
        self.left_expr.as_ref().zip(self.right_expr.as_ref())
    }

    pub fn is_expression_compare(&self) -> bool {
        self.expression_pair().is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonOperator {
    Eq,
    Neq,
    Gt,
    Lt,
    Gte,
    Lte,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum LiteralValue {
    Boolean(bool),
    Number(f64),
    Measured(MeasuredValue),
    String(String),
    State(StateReference),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutDirective {
    pub duration: DurationValue,
    pub target: GotoDirective,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GotoDirective {
    #[serde(default)]
    pub line: usize,
    pub task: String,
    pub step: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AxisFaultRouteKind {
    Reject,
    Motion,
    Safety,
    Vendor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AxisFaultRouteDirective {
    #[serde(default)]
    pub line: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<AxisFaultRouteKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<i32>,
    pub target: GotoDirective,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParallelBlock {
    pub branches: Vec<Branch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaceBlock {
    pub branches: Vec<RaceBranch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Branch {
    pub statements: Vec<StepStatement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaceBranch {
    pub statements: Vec<StepStatement>,
    pub then_goto: Option<GotoDirective>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "on_complete", rename_all = "snake_case")]
pub enum OnCompleteDirective {
    Goto { target: GotoDirective },
    Unreachable,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extern_nodes_roundtrip_through_serde_json() {
        let program = PlcProgram {
            topology: TopologySection {
                devices: vec![DeviceDeclaration {
                    line: 1,
                    name: "Y0".to_string(),
                    device_type: DeviceType::DigitalOutput,
                    attributes: DeviceAttributes::default(),
                }],
                connections: Vec::new(),
                variables: Vec::new(),
                cam_tables: Vec::new(),
                extern_functions: vec![ExternFunctionDeclaration {
                    line: 2,
                    name: "split".to_string(),
                    params: vec![ExternFunctionParameter {
                        name: "v".to_string(),
                        var_type: VariableType::Float,
                    }],
                    return_types: vec![VariableType::Float, VariableType::Float],
                    contract: ExternFunctionContract {
                        rust_module: "math::split".to_string(),
                        pure: true,
                        time_bound_us: 200,
                    },
                }],
                axis_fault_contracts: vec![AxisFaultContractDeclaration {
                    line: 3,
                    name: "axis_x_fault".to_string(),
                    axis: "axis_x".to_string(),
                    severity: AxisFaultSeverity::Safety,
                    stop_mode: AxisStopMode::Immediate,
                    auto_reset_policy: AxisAutoResetPolicy::Never,
                    manual_ack_required: true,
                }],
            },
            constraints: ConstraintsSection::default(),
            tasks: TasksSection {
                tasks: vec![TaskDeclaration {
                    line: 6,
                    name: "main".to_string(),
                    steps: vec![StepDeclaration {
                        line: 7,
                        name: "run".to_string(),
                        statements: vec![StepStatement::Action(ActionStatement::Call {
                            function: "split".to_string(),
                            args: vec![Expression::Variable("input".to_string())],
                            binding: ExternCallBinding::Tuple(vec![
                                "lo".to_string(),
                                "hi".to_string(),
                            ]),
                        })],
                    }],
                    on_complete_line: None,
                    on_complete: None,
                }],
            },
        };

        let json = serde_json::to_string(&program).expect("AST should serialize");
        let decoded: PlcProgram = serde_json::from_str(&json).expect("AST should deserialize");

        assert_eq!(decoded.topology.extern_functions.len(), 1);
        let function = &decoded.topology.extern_functions[0];
        assert_eq!(function.name, "split");
        assert_eq!(
            function.return_types,
            vec![VariableType::Float, VariableType::Float]
        );
        assert_eq!(function.contract.rust_module, "math::split");
        assert_eq!(function.contract.time_bound_us, 200);
        assert_eq!(decoded.topology.axis_fault_contracts.len(), 1);
        let contract = &decoded.topology.axis_fault_contracts[0];
        assert_eq!(contract.name, "axis_x_fault");
        assert_eq!(contract.axis, "axis_x");
        assert_eq!(contract.severity, AxisFaultSeverity::Safety);
        assert_eq!(contract.stop_mode, AxisStopMode::Immediate);
        assert_eq!(contract.auto_reset_policy, AxisAutoResetPolicy::Never);
        assert!(contract.manual_ack_required);

        let step = &decoded.tasks.tasks[0].steps[0];
        match &step.statements[0] {
            StepStatement::Action(ActionStatement::Call {
                function,
                args,
                binding,
            }) => {
                assert_eq!(function, "split");
                assert_eq!(args.len(), 1);
                assert!(matches!(
                    binding,
                    ExternCallBinding::Tuple(names) if names == &vec!["lo".to_string(), "hi".to_string()]
                ));
            }
            other => panic!("expected action call after roundtrip, got {other:?}"),
        }
    }
}
