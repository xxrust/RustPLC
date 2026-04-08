use crate::ast::{
    ActionStatement, ComparisonOperator, ConditionExpression, DeviceType, LiteralValue, PlcProgram,
    PortType, StepStatement, VariableType as AstVariableType, WaitCondition, WaitStatement,
};
use crate::axis_profile::resolve_axis_profiles;
use crate::ir::{
    AxisFaultBranch, AxisOrientation, AxisTimeoutBranch, ConstraintSet, SafetyExpr, SafetyRelation,
    State, StateExpr, StateMachine, Transition, TransitionAction, TransitionGuard,
    WorkpieceCarrierLayoutDef, WorkpieceEffect, WorkpieceSiteKind,
};
use petgraph::algo::kosaraju_scc;
use petgraph::graph::DiGraph;
use petgraph::visit::EdgeRef;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

#[cfg(feature = "z3-solver")]
use z3::ast::Bool;
#[cfg(feature = "z3-solver")]
use z3::{Config, Context, SatResult, Solver};

#[derive(Debug, Clone, Default)]
pub struct SafetyConfig {
    pub bmc_max_depth: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SafetyProofLevel {
    Complete,
    Bounded,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SafetyRuleStatusKind {
    Bound,
    Skipped,
    Degraded,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SafetyCoverage {
    pub bound_rules: usize,
    pub degraded_rules: usize,
    pub skipped_rules: usize,
    pub total_rules: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SafetyAnalogThresholdDetail {
    pub expr: String,
    pub device: String,
    pub operator: String,
    pub value: String,
    pub split_points: Vec<f64>,
    pub hit_intervals: usize,
    pub total_intervals: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SafetyRuleStatus {
    pub line: usize,
    pub rule: String,
    pub status: SafetyRuleStatusKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub analog_thresholds: Vec<SafetyAnalogThresholdDetail>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SafetyReport {
    pub level: SafetyProofLevel,
    pub explored_depth: usize,
    pub warnings: Vec<String>,
    pub checked_rules: usize,
    pub skipped_rules: usize,
    pub coverage: SafetyCoverage,
    pub rule_statuses: Vec<SafetyRuleStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafetyDiagnostic {
    pub line: usize,
    pub constraint: String,
    pub reason: String,
    pub violation_path: Vec<String>,
    pub suggestion: String,
}

impl fmt::Display for SafetyDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "ERROR [safety] 安全约束违反")?;
        writeln!(f, "  位置: <input>:{}:1", self.line)?;
        writeln!(f, "  约束: {}", self.constraint)?;
        writeln!(f, "  原因: {}", self.reason)?;
        writeln!(f, "  违反路径:")?;
        for (index, step) in self.violation_path.iter().enumerate() {
            writeln!(f, "    {}. {step}", index + 1)?;
        }
        write!(f, "  建议: {}", self.suggestion)
    }
}

#[derive(Debug, Clone)]
struct DeviceDomain {
    name: String,
    states: Vec<String>,
    default_state: usize,
    is_analog: bool,
    region_bounds: Option<Vec<(f64, f64)>>,
}

#[derive(Debug, Clone)]
struct VariableDomain {
    name: String,
    var_type: AstVariableType,
    initial_value: SafetyValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SafetyValue {
    Bool(bool),
    Number(u32),
}

impl SafetyValue {
    fn bool(value: bool) -> Self {
        Self::Bool(value)
    }

    fn number(value: f32) -> Self {
        Self::Number(value.to_bits())
    }

    fn as_bool(self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(value),
            Self::Number(bits) => {
                let value = f32::from_bits(bits);
                if (value - 0.0).abs() <= f32::EPSILON {
                    Some(false)
                } else if (value - 1.0).abs() <= f32::EPSILON {
                    Some(true)
                } else {
                    None
                }
            }
        }
    }

    fn as_f32(self) -> f32 {
        match self {
            Self::Bool(value) => {
                if value {
                    1.0
                } else {
                    0.0
                }
            }
            Self::Number(bits) => f32::from_bits(bits),
        }
    }
}

#[derive(Debug, Clone)]
enum ModelGuard {
    Always,
    AnalogRegions {
        device_id: usize,
        allowed_states: Vec<usize>,
    },
    DeviceState {
        device_id: usize,
        expected_state: usize,
        equals: bool,
    },
    VariableBool {
        variable_id: usize,
        equals: bool,
    },
    Expr(ModelExpr),
    Timeout,
    Delay,
    Unsupported,
}

#[derive(Debug, Clone)]
enum ModelExpr {
    Literal(SafetyValue),
    Variable(usize),
    Function {
        kind: ModelFunction,
        args: Vec<ModelExpr>,
    },
    UnaryNeg(Box<ModelExpr>),
    UnaryNot(Box<ModelExpr>),
    Binary {
        op: ModelBinaryOp,
        left: Box<ModelExpr>,
        right: Box<ModelExpr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelBinaryOp {
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

#[derive(Debug, Clone)]
enum ModelFunction {
    Abs,
    Min,
    Max,
    Sin,
    Cos,
    Sqrt,
    Pow,
    Fmod,
    Clamp,
}

#[derive(Debug, Clone)]
struct VariableAssignment {
    variable_id: usize,
    expr: ModelExpr,
}

#[derive(Debug, Clone)]
struct AnalogExprEffect {
    device_id: usize,
    expr: ModelExpr,
}

#[derive(Debug, Clone)]
enum ModelEffect {
    DeviceState { device_id: usize, state_id: usize },
    VariableAssignment(VariableAssignment),
    AnalogExpr(AnalogExprEffect),
}

#[derive(Debug, Clone)]
struct ModelEdge {
    from: usize,
    to: usize,
    guard: ModelGuard,
    ordered_effects: Vec<ModelEffect>,
    effects: HashMap<usize, usize>,
    variable_effects: Vec<VariableAssignment>,
    analog_expr_effects: Vec<AnalogExprEffect>,
    label: String,
}

#[derive(Debug, Clone)]
struct SafetyModel {
    states: Vec<State>,
    initial_state: usize,
    edges: Vec<ModelEdge>,
    outgoing: Vec<Vec<usize>>,
    active_task_names: Vec<String>,
    active_task_entries: Vec<usize>,
    pending_source_states: HashSet<usize>,
    pending_action_tags: HashMap<usize, Vec<String>>,
    devices: Vec<DeviceDomain>,
    device_index: HashMap<(String, String), usize>,
    device_state_index: Vec<HashMap<String, usize>>,
    variables: Vec<VariableDomain>,
    suggested_depth: usize,
    max_scc_depth: usize,
}

#[derive(Debug, Clone)]
struct RuleBinding {
    relation: SafetyRelation,
    left_device: usize,
    left_states: Vec<usize>,
    right_device: usize,
    right_states: Vec<usize>,
}

#[derive(Debug, Clone)]
struct DepthPlan {
    effective_depth: usize,
    warnings: Vec<String>,
    truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ConcreteState {
    task_states: Vec<usize>,
    task_pending: Vec<bool>,
    device_states: Vec<usize>,
    variable_values: Vec<SafetyValue>,
}

#[derive(Debug, Clone)]
struct SearchNode {
    state: ConcreteState,
    depth: usize,
    parent: Option<usize>,
    via_edge: Option<TransitionStep>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TransitionStep {
    task_slot: usize,
    edge_id: usize,
}

#[derive(Debug, Clone)]
struct SearchOutcome {
    counterexample: Option<Counterexample>,
    fully_explored: bool,
}

#[derive(Debug, Clone)]
struct SearchSpace {
    nodes: Vec<SearchNode>,
    fully_explored: bool,
}

#[derive(Debug, Clone)]
struct Counterexample {
    path: Vec<String>,
}

#[derive(Debug, Clone)]
struct SemanticResourceHolder {
    claim_index: usize,
    description: String,
}

#[derive(Debug, Clone)]
struct SemanticResourceCounterexample {
    resource_name: String,
    holders: Vec<SemanticResourceHolder>,
    path: Vec<String>,
}

pub fn verify_safety(
    program: &PlcProgram,
    constraints: &ConstraintSet,
    state_machine: &StateMachine,
) -> Result<SafetyReport, Vec<SafetyDiagnostic>> {
    verify_safety_with_config(program, constraints, state_machine, SafetyConfig::default())
}

pub fn verify_safety_with_config(
    program: &PlcProgram,
    constraints: &ConstraintSet,
    state_machine: &StateMachine,
    config: SafetyConfig,
) -> Result<SafetyReport, Vec<SafetyDiagnostic>> {
    let mut preflight_diagnostics = verify_vertical_axis_brake_sequence(program);
    preflight_diagnostics.extend(verify_workpiece_flow(program, constraints, state_machine));
    let preflight_warnings = collect_workpiece_contract_warnings(constraints, state_machine);
    if !preflight_diagnostics.is_empty() {
        return Err(preflight_diagnostics);
    }

    let model = SafetyModel::from_inputs(program, constraints, state_machine);
    let depth_plan = build_depth_plan(&model, &config);
    let search_space = explore_state_space(&model, depth_plan.effective_depth);

    #[cfg(feature = "z3-solver")]
    z3_sanity_probe();

    let mut diagnostics = Vec::new();
    let mut all_complete = true;
    let mut checked_rules = 0usize;
    let mut rule_statuses = Vec::with_capacity(constraints.safety.len());
    let mut bound_rules = 0usize;
    let mut degraded_rules = 0usize;
    let mut skipped_rules = 0usize;

    for (index, rule) in constraints.safety.iter().enumerate() {
        let relation = relation_text(&rule.relation);
        let left_text = safety_expr_text(&rule.left);
        let right_text = safety_expr_text(&rule.right);
        let rule_text = format!("{left_text} {relation} {right_text}");
        let line = program
            .constraints
            .safety
            .get(index)
            .map(|node| node.line.max(1))
            .unwrap_or(1);

        let analog_thresholds = collect_analog_threshold_details(&model, rule);

        let binding = match bind_safety_expr_rule_with_reason(&model, rule) {
            Ok(binding) => binding,
            Err(reason) => {
                skipped_rules += 1;
                rule_statuses.push(SafetyRuleStatus {
                    line,
                    rule: rule_text,
                    status: SafetyRuleStatusKind::Skipped,
                    reason: Some(reason),
                    analog_thresholds,
                });
                continue;
            }
        };

        checked_rules += 1;

        let outcome = analyze_rule(&model, &search_space, binding);
        if let Some(counterexample) = outcome.counterexample {
            let (reason, suggestion) = match rule.relation {
                SafetyRelation::ConflictsWith => (
                    format!("{} 与 {} 在可达状态同时成立", left_text, right_text),
                    format!(
                        "请在触发 {} 之前确保 {} 已复位，或调整并行/跳转逻辑避免两者同时成立",
                        right_text, left_text
                    ),
                ),
                SafetyRelation::Requires => (
                    format!("{} 成立时 {} 未满足", left_text, right_text),
                    format!(
                        "请在触发 {} 之前先确保 {} 成立，必要时添加 wait 或调整 step 顺序",
                        left_text, right_text
                    ),
                ),
            };

            diagnostics.push(SafetyDiagnostic {
                line,
                constraint: rule_text,
                reason,
                violation_path: counterexample.path,
                suggestion,
            });
            continue;
        }

        let has_threshold = safety_rule_has_threshold(rule);
        let mut status = SafetyRuleStatusKind::Bound;
        let mut reason: Option<String> = None;

        if has_threshold {
            status = SafetyRuleStatusKind::Degraded;
            reason = Some("模拟量阈值采用区间离散抽象（非连续完备模型）".to_string());
        }

        if depth_plan.truncated || !outcome.fully_explored {
            if matches!(status, SafetyRuleStatusKind::Bound) {
                status = SafetyRuleStatusKind::Degraded;
                reason = Some(format!(
                    "有界搜索深度上限导致覆盖不完备（max_depth={}）",
                    depth_plan.effective_depth
                ));
            } else if reason.is_some() {
                // Preserve stable message ordering for CI; append bounded-depth note.
                reason = Some(format!(
                    "{}；有界搜索深度上限（max_depth={}）",
                    reason.unwrap_or_default(),
                    depth_plan.effective_depth
                ));
            }
        }

        match status {
            SafetyRuleStatusKind::Bound => bound_rules += 1,
            SafetyRuleStatusKind::Degraded => degraded_rules += 1,
            SafetyRuleStatusKind::Skipped => skipped_rules += 1,
        }

        rule_statuses.push(SafetyRuleStatus {
            line,
            rule: rule_text,
            status,
            reason,
            analog_thresholds,
        });

        if depth_plan.truncated || !outcome.fully_explored || has_threshold {
            all_complete = false;
        }
    }

    diagnostics.extend(check_semantic_resource_interlocks(
        program,
        constraints,
        &model,
        &search_space,
    ));

    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let mut warnings = depth_plan.warnings;
    warnings.extend(preflight_warnings);
    let total_rules = constraints.safety.len();
    let skipped_rules_count = total_rules.saturating_sub(checked_rules).max(skipped_rules);

    for status in &rule_statuses {
        if matches!(status.status, SafetyRuleStatusKind::Skipped) {
            warnings.push(format!(
                "WARNING: Safety 规则 <input>:{} {} 已跳过：{}",
                status.line,
                status.rule,
                status.reason.as_deref().unwrap_or("无可用建模")
            ));
        }
    }

    let coverage = SafetyCoverage {
        bound_rules,
        degraded_rules,
        skipped_rules: skipped_rules_count,
        total_rules,
    };

    let level = if degraded_rules == 0
        && skipped_rules_count == 0
        && (checked_rules == 0 || all_complete)
    {
        SafetyProofLevel::Complete
    } else {
        if !all_complete {
            warnings.push(format!(
                "WARNING: Safety 在深度 {} 内未发现反例，但未获得完备证明。建议增大 bmc_max_depth 以提升有界覆盖，或调整模型以帮助 k-induction 收敛",
                depth_plan.effective_depth
            ));
        }
        SafetyProofLevel::Bounded
    };

    Ok(SafetyReport {
        level,
        explored_depth: depth_plan.effective_depth,
        warnings,
        checked_rules,
        skipped_rules: skipped_rules_count,
        coverage,
        rule_statuses,
    })
}

include!("safety_preflight.rs");
include!("safety_model_builder.rs");
include!("safety_workpiece.rs");
include!("safety_search.rs");
include!("safety_tests.rs");
