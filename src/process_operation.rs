use crate::ir::{
    ConstraintSet, EdgeKind, ResourceClaimSource, State, StateMachine, Transition,
    TransitionAction, TransitionGuard, WorkpieceEffect,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessOperationModel {
    pub schema_version: u32,
    pub policy: SchedulingPolicy,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operation_classes: Vec<ProcessOperationClass>,
    pub operations: Vec<ProcessOperation>,
    pub diagnostics: Vec<ProcessOperationDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SchedulingPolicy {
    OpportunisticAdmission,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessOperation {
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub contract_key: String,
    pub operation_class: String,
    pub task_name: String,
    pub step_name: String,
    pub from_state: State,
    pub to_state: State,
    pub guard: OperationGuard,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub admissions: Vec<AdmissionRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<OperationEffect>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub action_targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub program_predecessors: Vec<OperationPredecessor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessOperationClass {
    pub key: String,
    pub operation_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_patterns: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub destination_patterns: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effect_kinds: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OperationGuard {
    Always,
    Condition { expression: String },
    Edge { edge: EdgeKind, operand: String },
    Timeout { duration_ms: u64 },
    Delay { duration_ms: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AdmissionRule {
    SourceAvailable { endpoint: String },
    DestinationHasCapacity { endpoint: String },
    ResourceAvailable { resource: String },
    ProgramGuard { expression: String },
    OperatorEdge { edge: EdgeKind, operand: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OperationEffect {
    WorkpieceMove {
        effect: WorkpieceMoveEffect,
        from: String,
        to: String,
    },
    WorkpieceFinish {
        at: String,
        terminal_state: String,
    },
    WorkpieceMount {
        workpiece_type: String,
        slot: String,
    },
    WorkpieceSplit {
        source_type: String,
        target_type: String,
        count: u32,
        consumed: bool,
    },
    WorkpieceMerge {
        inputs: Vec<String>,
        target_type: String,
        consumed_inputs: bool,
    },
    WorkpieceTransformCarrier {
        carrier: String,
        frame: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkpieceMoveEffect {
    Acquire,
    Transfer,
    Unmount,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessOperationDiagnostic {
    pub code: String,
    pub operation_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperationPredecessor {
    pub operation_id: String,
    pub reason: PredecessorReason,
    pub justified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PredecessorReason {
    SharedEndpoint { endpoint: String },
    SharedResource { resource: String },
    ProgramGuard { expression: String },
    OperatorEdge { edge: EdgeKind, operand: String },
    SameTaskOrder,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessOperationRefinementReport {
    pub schema_version: u32,
    pub status: RefinementStatus,
    pub expected_operation_count: usize,
    pub actual_operation_count: usize,
    pub issues: Vec<ProcessOperationRefinementIssue>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RefinementStatus {
    Pass,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessOperationRefinementIssue {
    pub code: String,
    pub operation_id: String,
    pub message: String,
}

pub fn build_process_operation_model(
    state_machine: &StateMachine,
    constraints: &ConstraintSet,
) -> ProcessOperationModel {
    let resources_by_tag = resources_by_action_tag(constraints);
    let mut operations = Vec::new();

    for (idx, transition) in state_machine.transitions.iter().enumerate() {
        if !is_process_operation_transition(transition) {
            continue;
        }
        let id = format!(
            "{}.{}.op{}",
            transition.from.task_name,
            transition.from.step_name,
            idx + 1
        );
        let resources = resources_for_transition(transition, &resources_by_tag);
        let mut admissions = admission_rules_for_transition(transition, &resources);
        let effects = transition
            .effects
            .iter()
            .map(operation_effect_from_workpiece_effect)
            .collect::<Vec<_>>();
        admissions.extend(admission_rules_from_effects(&effects));
        dedup_admissions(&mut admissions);

        let operation_class = operation_class_key(&effects);
        let contract_key = operation_contract_key(&effects, &admissions, &resources);

        operations.push(ProcessOperation {
            id,
            contract_key,
            operation_class,
            task_name: transition.from.task_name.clone(),
            step_name: transition.from.step_name.clone(),
            from_state: transition.from.clone(),
            to_state: transition.to.clone(),
            guard: operation_guard_from_transition_guard(&transition.guard),
            admissions,
            effects,
            resources,
            action_targets: action_targets_for_transition(transition),
            program_predecessors: Vec::new(),
        });
    }

    attach_program_predecessors(&mut operations);
    let operation_classes = build_operation_classes(&operations);
    let diagnostics = diagnose_process_operations(&operations);
    ProcessOperationModel {
        schema_version: 1,
        policy: SchedulingPolicy::OpportunisticAdmission,
        operation_classes,
        operations,
        diagnostics,
    }
}

pub fn read_process_operation_model(path: &Path) -> Result<ProcessOperationModel, String> {
    let body = fs::read_to_string(path).map_err(|err| {
        format!(
            "Failed to read process operation model {}: {err}",
            path.display()
        )
    })?;
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
    {
        serde_json::from_str(&body).map_err(|err| {
            format!(
                "Failed to parse process operation model JSON {}: {err}",
                path.display()
            )
        })
    } else {
        toml::from_str(&body).map_err(|err| {
            format!(
                "Failed to parse process operation model TOML {}: {err}",
                path.display()
            )
        })
    }
}

pub fn verify_process_operation_refinement(
    expected: &ProcessOperationModel,
    actual: &ProcessOperationModel,
) -> ProcessOperationRefinementReport {
    let mut issues = Vec::new();

    if expected.schema_version != actual.schema_version {
        issues.push(ProcessOperationRefinementIssue {
            code: "OPREF-001".to_string(),
            operation_id: "*".to_string(),
            message: format!(
                "schema_version mismatch: expected {}, actual {}",
                expected.schema_version, actual.schema_version
            ),
        });
    }
    if expected.policy != actual.policy {
        issues.push(ProcessOperationRefinementIssue {
            code: "OPREF-002".to_string(),
            operation_id: "*".to_string(),
            message: format!(
                "scheduling policy mismatch: expected {:?}, actual {:?}",
                expected.policy, actual.policy
            ),
        });
    }

    let mut actual_by_key = operations_by_contract_key(&actual.operations);

    for operation in &expected.operations {
        let key = operation_semantic_key(operation);
        let Some(actual_operations) = actual_by_key.get_mut(&key) else {
            issues.push(ProcessOperationRefinementIssue {
                code: "OPREF-010".to_string(),
                operation_id: operation.id.clone(),
                message: format!(
                    "operation declared in process_model is missing from task/step program flow: {key}"
                ),
            });
            continue;
        };
        actual_operations.pop();
        if actual_operations.is_empty() {
            actual_by_key.remove(&key);
        }
    }

    for (key, operations) in actual_by_key {
        for operation in operations {
            issues.push(ProcessOperationRefinementIssue {
                code: "OPREF-011".to_string(),
                operation_id: operation.id.clone(),
                message: format!(
                    "task/step program flow introduces a process operation not declared in process_model: {key}"
                ),
            });
        }
    }

    for diagnostic in &actual.diagnostics {
        issues.push(ProcessOperationRefinementIssue {
            code: diagnostic.code.clone(),
            operation_id: diagnostic.operation_id.clone(),
            message: diagnostic.message.clone(),
        });
    }

    ProcessOperationRefinementReport {
        schema_version: 1,
        status: if issues.is_empty() {
            RefinementStatus::Pass
        } else {
            RefinementStatus::Fail
        },
        expected_operation_count: expected.operations.len(),
        actual_operation_count: actual.operations.len(),
        issues,
    }
}

fn is_process_operation_transition(transition: &Transition) -> bool {
    !transition.effects.is_empty()
}

fn operation_guard_from_transition_guard(guard: &TransitionGuard) -> OperationGuard {
    match guard {
        TransitionGuard::Always => OperationGuard::Always,
        TransitionGuard::Condition { expression } => OperationGuard::Condition {
            expression: expression.clone(),
        },
        TransitionGuard::Edge { edge, operand } => OperationGuard::Edge {
            edge: *edge,
            operand: operand.clone(),
        },
        TransitionGuard::Timeout { duration_ms } => OperationGuard::Timeout {
            duration_ms: *duration_ms,
        },
        TransitionGuard::Delay { duration_ms } => OperationGuard::Delay {
            duration_ms: *duration_ms,
        },
    }
}

fn resources_by_action_tag(constraints: &ConstraintSet) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::<String, BTreeSet<String>>::new();
    for claim in &constraints.resource_claims {
        if let ResourceClaimSource::ActionTag { tag } = &claim.source {
            out.entry(tag.clone())
                .or_default()
                .insert(claim.resource.clone());
        }
    }
    out.into_iter()
        .map(|(tag, resources)| (tag, resources.into_iter().collect()))
        .collect()
}

fn resources_for_transition(
    transition: &Transition,
    resources_by_tag: &BTreeMap<String, Vec<String>>,
) -> Vec<String> {
    let mut resources = BTreeSet::new();
    for action in &transition.actions {
        let Some(tag) = action_semantic_tag(action) else {
            continue;
        };
        if let Some(tag_resources) = resources_by_tag.get(tag) {
            resources.extend(tag_resources.iter().cloned());
        }
    }
    resources.into_iter().collect()
}

fn admission_rules_for_transition(
    transition: &Transition,
    resources: &[String],
) -> Vec<AdmissionRule> {
    let mut admissions = Vec::new();
    match &transition.guard {
        TransitionGuard::Condition { expression } => admissions.push(AdmissionRule::ProgramGuard {
            expression: expression.clone(),
        }),
        TransitionGuard::Edge { edge, operand } => admissions.push(AdmissionRule::OperatorEdge {
            edge: *edge,
            operand: operand.clone(),
        }),
        TransitionGuard::Always
        | TransitionGuard::Timeout { .. }
        | TransitionGuard::Delay { .. } => {}
    }
    admissions.extend(
        resources
            .iter()
            .cloned()
            .map(|resource| AdmissionRule::ResourceAvailable { resource }),
    );
    admissions
}

fn admission_rules_from_effects(effects: &[OperationEffect]) -> Vec<AdmissionRule> {
    let mut admissions = Vec::new();
    for effect in effects {
        match effect {
            OperationEffect::WorkpieceMove { from, to, .. } => {
                admissions.push(AdmissionRule::SourceAvailable {
                    endpoint: from.clone(),
                });
                admissions.push(AdmissionRule::DestinationHasCapacity {
                    endpoint: to.clone(),
                });
            }
            OperationEffect::WorkpieceFinish { at, .. } => {
                admissions.push(AdmissionRule::SourceAvailable {
                    endpoint: at.clone(),
                });
            }
            OperationEffect::WorkpieceMount { slot, .. } => {
                admissions.push(AdmissionRule::DestinationHasCapacity {
                    endpoint: slot.clone(),
                });
            }
            OperationEffect::WorkpieceSplit { .. }
            | OperationEffect::WorkpieceMerge { .. }
            | OperationEffect::WorkpieceTransformCarrier { .. } => {}
        }
    }
    admissions
}

fn dedup_admissions(admissions: &mut Vec<AdmissionRule>) {
    let mut seen = BTreeSet::new();
    admissions.retain(|admission| seen.insert(admission_key(admission)));
}

fn admission_key(admission: &AdmissionRule) -> String {
    match admission {
        AdmissionRule::SourceAvailable { endpoint } => format!("source:{endpoint}"),
        AdmissionRule::DestinationHasCapacity { endpoint } => format!("dest:{endpoint}"),
        AdmissionRule::ResourceAvailable { resource } => format!("resource:{resource}"),
        AdmissionRule::ProgramGuard { expression } => format!("guard:{expression}"),
        AdmissionRule::OperatorEdge { edge, operand } => format!("edge:{edge:?}:{operand}"),
    }
}

fn operation_effect_from_workpiece_effect(effect: &WorkpieceEffect) -> OperationEffect {
    match effect {
        WorkpieceEffect::Acquire { holder, from } => OperationEffect::WorkpieceMove {
            effect: WorkpieceMoveEffect::Acquire,
            from: from.clone(),
            to: holder.clone(),
        },
        WorkpieceEffect::Transfer { from, to } => OperationEffect::WorkpieceMove {
            effect: WorkpieceMoveEffect::Transfer,
            from: from.clone(),
            to: to.clone(),
        },
        WorkpieceEffect::Finish { at, terminal_state } => OperationEffect::WorkpieceFinish {
            at: at.clone(),
            terminal_state: terminal_state.clone(),
        },
        WorkpieceEffect::Mount {
            workpiece_type,
            slot,
        } => OperationEffect::WorkpieceMount {
            workpiece_type: workpiece_type.clone(),
            slot: slot.clone(),
        },
        WorkpieceEffect::Unmount {
            workpiece_type: _,
            slot,
            to,
        } => OperationEffect::WorkpieceMove {
            effect: WorkpieceMoveEffect::Unmount,
            from: slot.clone(),
            to: to.clone(),
        },
        WorkpieceEffect::Split {
            source_type,
            target_type,
            count,
            consumed,
        } => OperationEffect::WorkpieceSplit {
            source_type: source_type.clone(),
            target_type: target_type.clone(),
            count: *count,
            consumed: *consumed,
        },
        WorkpieceEffect::Merge {
            inputs,
            target_type,
            consumed_inputs,
        } => OperationEffect::WorkpieceMerge {
            inputs: inputs.clone(),
            target_type: target_type.clone(),
            consumed_inputs: *consumed_inputs,
        },
        WorkpieceEffect::TransformCarrier { carrier, frame } => {
            OperationEffect::WorkpieceTransformCarrier {
                carrier: carrier.clone(),
                frame: frame.clone(),
            }
        }
    }
}

fn operation_class_key(effects: &[OperationEffect]) -> String {
    effects
        .iter()
        .map(operation_effect_class_key)
        .collect::<Vec<_>>()
        .join("+")
}

fn operation_effect_class_key(effect: &OperationEffect) -> String {
    match effect {
        OperationEffect::WorkpieceMove { effect, from, to } => format!(
            "move:{effect:?}:{}->{}",
            normalize_endpoint_pattern(from),
            normalize_endpoint_pattern(to)
        ),
        OperationEffect::WorkpieceFinish { at, terminal_state } => {
            format!("finish:{}:{terminal_state}", normalize_endpoint_pattern(at))
        }
        OperationEffect::WorkpieceMount {
            workpiece_type,
            slot,
        } => format!(
            "mount:{workpiece_type}:{}",
            normalize_endpoint_pattern(slot)
        ),
        OperationEffect::WorkpieceSplit {
            source_type,
            target_type,
            count,
            consumed,
        } => format!("split:{source_type}->{target_type}:{count}:{consumed}"),
        OperationEffect::WorkpieceMerge {
            inputs,
            target_type,
            consumed_inputs,
        } => format!(
            "merge:{}->{target_type}:{consumed_inputs}",
            inputs.join(",")
        ),
        OperationEffect::WorkpieceTransformCarrier { carrier, frame } => {
            format!("transform_carrier:{carrier}:{frame}")
        }
    }
}

fn build_operation_classes(operations: &[ProcessOperation]) -> Vec<ProcessOperationClass> {
    let mut grouped = BTreeMap::<String, ProcessOperationClassBuilder>::new();
    for operation in operations {
        let builder = grouped
            .entry(operation.operation_class.clone())
            .or_insert_with(|| ProcessOperationClassBuilder::new(&operation.operation_class));
        builder.operation_ids.push(operation.id.clone());
        for effect in &operation.effects {
            builder.record_effect(effect);
        }
    }

    grouped
        .into_values()
        .map(ProcessOperationClassBuilder::finish)
        .collect()
}

struct ProcessOperationClassBuilder {
    key: String,
    operation_ids: Vec<String>,
    source_patterns: BTreeSet<String>,
    destination_patterns: BTreeSet<String>,
    effect_kinds: BTreeSet<String>,
}

impl ProcessOperationClassBuilder {
    fn new(key: &str) -> Self {
        Self {
            key: key.to_string(),
            operation_ids: Vec::new(),
            source_patterns: BTreeSet::new(),
            destination_patterns: BTreeSet::new(),
            effect_kinds: BTreeSet::new(),
        }
    }

    fn record_effect(&mut self, effect: &OperationEffect) {
        match effect {
            OperationEffect::WorkpieceMove { effect, from, to } => {
                self.effect_kinds.insert(format!("{effect:?}"));
                self.source_patterns
                    .insert(normalize_endpoint_pattern(from));
                self.destination_patterns
                    .insert(normalize_endpoint_pattern(to));
            }
            OperationEffect::WorkpieceFinish { at, .. } => {
                self.effect_kinds.insert("Finish".to_string());
                self.source_patterns.insert(normalize_endpoint_pattern(at));
            }
            OperationEffect::WorkpieceMount { slot, .. } => {
                self.effect_kinds.insert("Mount".to_string());
                self.destination_patterns
                    .insert(normalize_endpoint_pattern(slot));
            }
            OperationEffect::WorkpieceSplit { .. } => {
                self.effect_kinds.insert("Split".to_string());
            }
            OperationEffect::WorkpieceMerge { .. } => {
                self.effect_kinds.insert("Merge".to_string());
            }
            OperationEffect::WorkpieceTransformCarrier { .. } => {
                self.effect_kinds.insert("TransformCarrier".to_string());
            }
        }
    }

    fn finish(self) -> ProcessOperationClass {
        ProcessOperationClass {
            key: self.key,
            operation_ids: self.operation_ids,
            source_patterns: self.source_patterns.into_iter().collect(),
            destination_patterns: self.destination_patterns.into_iter().collect(),
            effect_kinds: self.effect_kinds.into_iter().collect(),
        }
    }
}

fn normalize_endpoint_pattern(endpoint: &str) -> String {
    let mut out = String::with_capacity(endpoint.len());
    let mut chars = endpoint.chars().peekable();
    while let Some(ch) = chars.next() {
        out.push(ch);
        if ch == '[' {
            let mut content = String::new();
            while let Some(&next) = chars.peek() {
                chars.next();
                if next == ']' {
                    break;
                }
                content.push(next);
            }
            if content.chars().all(|c| c.is_ascii_digit()) {
                out.push('*');
            } else {
                out.push_str(&content);
            }
            out.push(']');
        }
    }
    out
}

fn action_semantic_tag(action: &TransitionAction) -> Option<&str> {
    match action {
        TransitionAction::AxisMoveRelative { semantic_tag, .. }
        | TransitionAction::AxisMoveAbsolute { semantic_tag, .. } => semantic_tag.as_deref(),
        _ => None,
    }
}

fn action_targets_for_transition(transition: &Transition) -> Vec<String> {
    let mut targets = BTreeSet::new();
    for action in &transition.actions {
        if let Some(target) = action_target(action) {
            targets.insert(target.to_string());
        }
    }
    targets.into_iter().collect()
}

fn action_target(action: &TransitionAction) -> Option<&str> {
    match action {
        TransitionAction::Extend { target, .. }
        | TransitionAction::Retract { target, .. }
        | TransitionAction::Set { target, .. }
        | TransitionAction::SetAnalog { target, .. }
        | TransitionAction::SetAnalogExpr { target, .. }
        | TransitionAction::CamEngage { target }
        | TransitionAction::CamDisengage { target }
        | TransitionAction::CamSwitch { target, .. }
        | TransitionAction::CamPhase { target, .. }
        | TransitionAction::DeviceAction { target, .. }
        | TransitionAction::AxisMoveRelative { target, .. }
        | TransitionAction::AxisMoveAbsolute { target, .. } => Some(target.as_str()),
        TransitionAction::Compute { .. }
        | TransitionAction::CallExtern { .. }
        | TransitionAction::Log { .. } => None,
    }
}

fn attach_program_predecessors(operations: &mut [ProcessOperation]) {
    let mut by_to_state = BTreeMap::<(String, String), Vec<usize>>::new();
    for (idx, operation) in operations.iter().enumerate() {
        by_to_state
            .entry((
                operation.to_state.task_name.clone(),
                operation.to_state.step_name.clone(),
            ))
            .or_default()
            .push(idx);
    }

    for idx in 0..operations.len() {
        let from_key = (
            operations[idx].from_state.task_name.clone(),
            operations[idx].from_state.step_name.clone(),
        );
        let predecessor_indices = by_to_state.get(&from_key).cloned().unwrap_or_default();
        let mut predecessors = Vec::new();
        for prev_idx in predecessor_indices {
            if prev_idx == idx {
                continue;
            }
            predecessors.extend(predecessor_reasons(&operations[prev_idx], &operations[idx]));
        }
        operations[idx].program_predecessors = predecessors;
    }
}

fn predecessor_reasons(
    previous: &ProcessOperation,
    current: &ProcessOperation,
) -> Vec<OperationPredecessor> {
    let mut reasons = Vec::new();
    let previous_endpoints = operation_endpoints(previous);
    let current_endpoints = operation_endpoints(current);
    for endpoint in previous_endpoints.intersection(&current_endpoints) {
        reasons.push(OperationPredecessor {
            operation_id: previous.id.clone(),
            reason: PredecessorReason::SharedEndpoint {
                endpoint: endpoint.clone(),
            },
            justified: true,
        });
    }
    let previous_resources = previous.resources.iter().cloned().collect::<BTreeSet<_>>();
    let current_resources = current.resources.iter().cloned().collect::<BTreeSet<_>>();
    for resource in previous_resources.intersection(&current_resources) {
        reasons.push(OperationPredecessor {
            operation_id: previous.id.clone(),
            reason: PredecessorReason::SharedResource {
                resource: resource.clone(),
            },
            justified: true,
        });
    }
    if reasons.is_empty() {
        reasons.push(OperationPredecessor {
            operation_id: previous.id.clone(),
            reason: PredecessorReason::SameTaskOrder,
            justified: false,
        });
    }
    reasons
}

fn operation_endpoints(operation: &ProcessOperation) -> BTreeSet<String> {
    let mut endpoints = BTreeSet::new();
    for effect in &operation.effects {
        match effect {
            OperationEffect::WorkpieceMove { from, to, .. } => {
                endpoints.insert(from.clone());
                endpoints.insert(to.clone());
            }
            OperationEffect::WorkpieceFinish { at, .. } => {
                endpoints.insert(at.clone());
            }
            OperationEffect::WorkpieceMount { slot, .. } => {
                endpoints.insert(slot.clone());
            }
            OperationEffect::WorkpieceSplit { .. }
            | OperationEffect::WorkpieceMerge { .. }
            | OperationEffect::WorkpieceTransformCarrier { .. } => {}
        }
    }
    endpoints
}

fn diagnose_process_operations(operations: &[ProcessOperation]) -> Vec<ProcessOperationDiagnostic> {
    let mut diagnostics = Vec::new();
    for operation in operations {
        let has_workpiece_effect = operation.effects.iter().any(|effect| {
            matches!(
                effect,
                OperationEffect::WorkpieceMove { .. }
                    | OperationEffect::WorkpieceFinish { .. }
                    | OperationEffect::WorkpieceMount { .. }
            )
        });
        if has_workpiece_effect
            && !operation.admissions.iter().any(|admission| {
                matches!(
                    admission,
                    AdmissionRule::SourceAvailable { .. }
                        | AdmissionRule::DestinationHasCapacity { .. }
                )
            })
        {
            diagnostics.push(ProcessOperationDiagnostic {
                code: "OP-001".to_string(),
                operation_id: operation.id.clone(),
                message: "workpiece operation has no explicit admission facts".to_string(),
            });
        }
        for predecessor in operation
            .program_predecessors
            .iter()
            .filter(|predecessor| !predecessor.justified)
        {
            diagnostics.push(ProcessOperationDiagnostic {
                code: "OP-002".to_string(),
                operation_id: operation.id.clone(),
                message: format!(
                    "task/step program flow serializes `{}` after `{}` without shared endpoint or shared resource",
                    operation.id, predecessor.operation_id
                ),
            });
        }
        for effect in &operation.effects {
            if matches!(
                effect,
                OperationEffect::WorkpieceSplit { .. }
                    | OperationEffect::WorkpieceMerge { .. }
                    | OperationEffect::WorkpieceTransformCarrier { .. }
            ) {
                diagnostics.push(ProcessOperationDiagnostic {
                    code: "OP-003".to_string(),
                    operation_id: operation.id.clone(),
                    message: "split/merge/transform-carrier operation is present in process_model but admission/refinement semantics are not fully modeled yet".to_string(),
                });
            }
        }
    }
    diagnostics
}

fn operations_by_contract_key(
    operations: &[ProcessOperation],
) -> BTreeMap<String, Vec<&ProcessOperation>> {
    let mut out = BTreeMap::<String, Vec<&ProcessOperation>>::new();
    for operation in operations {
        out.entry(operation_semantic_key(operation))
            .or_default()
            .push(operation);
    }
    out
}

fn operation_semantic_key(operation: &ProcessOperation) -> String {
    if !operation.contract_key.is_empty() {
        return operation.contract_key.clone();
    }
    operation_contract_key(
        &operation.effects,
        &operation.admissions,
        &operation.resources,
    )
}

fn operation_contract_key(
    effects: &[OperationEffect],
    admissions: &[AdmissionRule],
    resources: &[String],
) -> String {
    format!(
        "effects=[{}];admissions=[{}];resources=[{}]",
        canonical_effects(effects).join("|"),
        canonical_admissions(admissions).join("|"),
        canonical_strings(resources).join("|")
    )
}

fn canonical_effects(effects: &[OperationEffect]) -> Vec<String> {
    let mut out = effects
        .iter()
        .map(|effect| format!("{effect:?}"))
        .collect::<Vec<_>>();
    out.sort();
    out
}

fn canonical_admissions(admissions: &[AdmissionRule]) -> Vec<String> {
    let mut out = admissions.iter().map(admission_key).collect::<Vec<_>>();
    out.sort();
    out
}

fn canonical_strings(values: &[String]) -> Vec<String> {
    let mut out = values.to_vec();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_plc;
    use crate::semantic::{build_constraint_set, build_state_machine};

    #[test]
    fn process_operation_model_derives_admission_from_workpiece_effects() {
        let source = r#"
[topology]

workpiece part: workpiece_type {
    normal_terminal_states: [done]
    ingress_sites: [infeed]
    normal_egress_sites: [outfeed]
}

location infeed: workpiece_location { capacity: 1 }
location outfeed: workpiece_location { capacity: 1 }
holder arm: workpiece_holder { capacity: 1 }

[constraints]

[tasks]

task main:
    step pick:
        effect: acquire holder arm from infeed
    step place:
        effect: transfer from arm to outfeed
    step finish:
        effect: finish workpiece at outfeed as done
"#;
        let program = parse_plc(source).expect("parse");
        let constraints = build_constraint_set(&program).expect("constraints");
        let state_machine = build_state_machine(&program).expect("state machine");
        let model = build_process_operation_model(&state_machine, &constraints);

        assert_eq!(model.schema_version, 1);
        assert_eq!(model.policy, SchedulingPolicy::OpportunisticAdmission);
        assert_eq!(model.operations.len(), 2);
        assert_eq!(model.operation_classes.len(), 2);
        let pick = &model.operations[0];
        assert_eq!(pick.task_name, "main");
        assert_eq!(pick.step_name, "pick");
        assert_eq!(pick.operation_class, "move:Acquire:infeed->arm");
        assert!(pick.admissions.iter().any(|rule| matches!(
            rule,
            AdmissionRule::SourceAvailable { endpoint } if endpoint == "infeed"
        )));
        assert!(pick.admissions.iter().any(|rule| matches!(
            rule,
            AdmissionRule::DestinationHasCapacity { endpoint } if endpoint == "arm"
        )));
        assert!(model.diagnostics.is_empty());
    }

    #[test]
    fn process_operation_class_normalizes_carrier_slots() {
        let effects = vec![OperationEffect::WorkpieceMove {
            effect: WorkpieceMoveEffect::Transfer,
            from: "storage_box.slot[7]".to_string(),
            to: "pickup_position".to_string(),
        }];

        assert_eq!(
            operation_class_key(&effects),
            "move:Transfer:storage_box.slot[*]->pickup_position"
        );
    }
}
