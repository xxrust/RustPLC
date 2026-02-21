use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlcProgram {
    pub topology: TopologySection,
    pub constraints: ConstraintsSection,
    pub tasks: TasksSection,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TopologySection {
    pub devices: Vec<DeviceDeclaration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceDeclaration {
    #[serde(default)]
    pub line: usize,
    pub name: String,
    pub device_type: DeviceType,
    pub attributes: DeviceAttributes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceType {
    DigitalOutput,
    DigitalInput,
    SolenoidValve,
    Cylinder,
    Sensor,
    Motor,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeviceAttributes {
    pub driven_by: Option<String>,
    pub reports_to: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<DevicePort>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connected_to: Option<String>,
    pub response_time: Option<DurationValue>,
    pub stroke_time: Option<DurationValue>,
    pub retract_time: Option<DurationValue>,
    pub stroke: Option<MeasuredValue>,
    #[serde(rename = "type")]
    pub r#type: Option<String>,
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
    pub state: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ActionStatement {
    Extend { target: String },
    Retract { target: String },
    Set { target: String, value: BinaryValue },
    SetAnalog { target: String, value: f64 },
    Log { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryValue {
    On,
    Off,
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
