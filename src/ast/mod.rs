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
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeviceAttributes {
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
    pub rated_speed: Option<MeasuredValue>,
    pub ramp_time: Option<DurationValue>,
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
    pub left: StateReference,
    pub relation: SafetyRelation,
    pub right: StateReference,
    pub reason: Option<String>,
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
    Delay { duration_ms: u64 },
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
    pub condition: ConditionExpression,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum LiteralValue {
    Boolean(bool),
    Number(f64),
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
    pub step: String,
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
    Goto { step: String },
    Unreachable,
}
