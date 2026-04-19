use crate::device_semantics::cylinder::closed_loop_stroke_target;
use crate::ir::{
    BinaryValue, ConstraintSet, ExternCallBinding, State, StateMachine, TimerOperationKind,
    TopologyGraph, Transition, TransitionAction, TransitionGuard, VariableType,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

const STATE_ID_STRIDE: i32 = 10;
const RESERVED_INTERNAL_ERROR_STATE_ID: i32 = 9999;
const INTERNAL_STATE_VAR: &str = "_state";
const INTERNAL_STATE_TRACE_PREFIX: &str = "_state_trace_b";
const INTERNAL_STATE_TRACE_BITS: u8 = 14;
const INTERNAL_TIMER_PREFIX: &str = "_timer_";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StCodegenConfig {
    pub program_name: String,
    pub source_file: String,
    pub include_verification_summary: bool,
    pub task_interval_ms: u64,
}

impl Default for StCodegenConfig {
    fn default() -> Self {
        Self {
            program_name: "Main".to_string(),
            source_file: String::new(),
            include_verification_summary: true,
            task_interval_ms: 10,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StCodegenError {
    EmptyStateMachine,
    UnresolvedGoto {
        from: String,
        target: String,
    },
    VariableNameConflict {
        original: String,
        normalized: String,
    },
    TypeConflict {
        name: String,
    },
    ExpressionNotSupported {
        expr: String,
    },
    SemanticResourceInterlockUnsupported,
    ClosedLoopCylinderSemanticsUnsupported {
        target: String,
    },
}

impl fmt::Display for StCodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StCodegenError::EmptyStateMachine => write!(f, "state machine has no states"),
            StCodegenError::UnresolvedGoto { from, target } => {
                write!(f, "unresolved goto target from {from} -> {target}")
            }
            StCodegenError::VariableNameConflict {
                original,
                normalized,
            } => {
                write!(
                    f,
                    "variable name conflict after normalization: {original} -> {normalized}"
                )
            }
            StCodegenError::TypeConflict { name } => {
                write!(f, "type conflict for variable: {name}")
            }
            StCodegenError::ExpressionNotSupported { expr } => {
                write!(f, "expression is not supported in phase 1: {expr}")
            }
            StCodegenError::SemanticResourceInterlockUnsupported => write!(
                f,
                "semantic resource interlock is not supported by the ST backend"
            ),
            StCodegenError::ClosedLoopCylinderSemanticsUnsupported { target } => write!(
                f,
                "closed-loop cylinder semantics are not supported by the ST backend: {target}"
            ),
        }
    }
}

impl std::error::Error for StCodegenError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum StVarType {
    Bool,
    Int,
    Real,
}

#[derive(Debug, Clone)]
struct VariableCandidate {
    original: String,
    var_type: StVarType,
}

#[derive(Debug, Clone)]
struct ResolvedVariables {
    by_original: HashMap<String, String>,
    declarations: BTreeMap<String, StVarType>,
}

impl ResolvedVariables {
    fn resolve_identifier(&self, raw: &str) -> String {
        self.by_original
            .get(raw)
            .cloned()
            .unwrap_or_else(|| normalize_identifier_for_st(raw))
    }

    fn var_type_for(&self, raw: &str) -> Option<StVarType> {
        let normalized = self.resolve_identifier(raw);
        self.declarations.get(&normalized).copied()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedCondition {
    left: String,
    op: ConditionOp,
    right: ConditionValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConditionOp {
    Eq,
    Ne,
    Gt,
    Lt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConditionValue {
    Bool(bool),
    Number(String),
    Identifier(String),
}

pub fn generate_st(
    topology: &TopologyGraph,
    constraints: &ConstraintSet,
    state_machine: &StateMachine,
    config: &StCodegenConfig,
) -> Result<String, Vec<StCodegenError>> {
    let workpiece_semantics_erased = has_workpiece_semantics(constraints, state_machine);
    let erased_constraints = erase_workpiece_semantics_from_constraints(constraints);
    let erased_state_machine = erase_workpiece_semantics_from_state_machine(state_machine);

    let mut errors = Vec::new();
    if erased_state_machine.states.is_empty() {
        errors.push(StCodegenError::EmptyStateMachine);
    }
    if !erased_constraints.semantic_resources.is_empty()
        || !erased_constraints.resource_claims.is_empty()
        || erased_state_machine.transitions.iter().any(|transition| {
            transition.actions.iter().any(|action| {
                matches!(
                    action,
                    TransitionAction::AxisMoveRelative {
                        semantic_tag: Some(_),
                        ..
                    } | TransitionAction::AxisMoveAbsolute {
                        semantic_tag: Some(_),
                        ..
                    }
                )
            })
        })
    {
        errors.push(StCodegenError::SemanticResourceInterlockUnsupported);
    }

    for transition in &erased_state_machine.transitions {
        for action in &transition.actions {
            if let Some(target) = closed_loop_stroke_target(action) {
                errors.push(StCodegenError::ClosedLoopCylinderSemanticsUnsupported {
                    target: target.to_string(),
                });
            }
        }
    }

    let state_ids = assign_state_ids(&erased_state_machine);
    validate_state_references(&erased_state_machine, &state_ids, &mut errors);

    let mut variable_candidates = collect_variable_candidates_from_topology(topology);
    collect_variable_candidates_from_transitions(
        &erased_state_machine,
        &mut variable_candidates,
        &mut errors,
    );

    if !errors.is_empty() {
        return Err(errors);
    }

    let resolved_variables = match resolve_variables(variable_candidates) {
        Ok(vars) => vars,
        Err(mut name_errors) => {
            errors.append(&mut name_errors);
            return Err(errors);
        }
    };

    let initial_id = state_id_of(&erased_state_machine.initial, &state_ids)
        .expect("initial state must exist when all state references are valid");
    let transitions_by_state = collect_transitions_by_state(&erased_state_machine, &state_ids);
    let timers = collect_timers(&erased_state_machine, &state_ids);

    let mut out = String::new();
    emit_header(&mut out, config, workpiece_semantics_erased);
    out.push_str(&format!("PROGRAM {}\n", config.program_name));
    emit_var_block(
        &mut out,
        initial_id,
        &timers,
        &resolved_variables.declarations,
    );
    emit_trace_var_block(&mut out);
    emit_state_constants_comment(&mut out, &erased_state_machine, &state_ids);
    emit_timer_calls(&mut out, &timers);
    emit_trace_exports(&mut out);
    emit_case_block(
        &mut out,
        &erased_state_machine,
        &state_ids,
        &transitions_by_state,
        &resolved_variables,
    );
    out.push_str("END_PROGRAM\n\n");
    emit_openplc_configuration(&mut out, config);

    Ok(out)
}

fn has_workpiece_semantics(constraints: &ConstraintSet, state_machine: &StateMachine) -> bool {
    !constraints.workpiece_types.is_empty()
        || !constraints.workpiece_sites.is_empty()
        || !constraints.workpiece_holders.is_empty()
        || !constraints.workpiece_carriers.is_empty()
        || state_machine
            .transitions
            .iter()
            .any(|transition| !transition.effects.is_empty())
}

fn erase_workpiece_semantics_from_constraints(constraints: &ConstraintSet) -> ConstraintSet {
    let mut erased = constraints.clone();
    erased.workpiece_types.clear();
    erased.workpiece_sites.clear();
    erased.workpiece_holders.clear();
    erased.workpiece_carriers.clear();
    erased
}

fn erase_workpiece_semantics_from_state_machine(state_machine: &StateMachine) -> StateMachine {
    let mut erased = state_machine.clone();
    for transition in &mut erased.transitions {
        transition.effects.clear();
    }
    erased
}

fn assign_state_ids(state_machine: &StateMachine) -> HashMap<(String, String), i32> {
    let mut out = HashMap::new();
    for (index, state) in state_machine.states.iter().enumerate() {
        out.insert(state_key(state), (index as i32) * STATE_ID_STRIDE);
    }
    out
}

fn validate_state_references(
    state_machine: &StateMachine,
    state_ids: &HashMap<(String, String), i32>,
    errors: &mut Vec<StCodegenError>,
) {
    if state_id_of(&state_machine.initial, state_ids).is_none() {
        errors.push(StCodegenError::UnresolvedGoto {
            from: "<initial>".to_string(),
            target: format!(
                "{}.{}",
                state_machine.initial.task_name, state_machine.initial.step_name
            ),
        });
    }

    for transition in &state_machine.transitions {
        if state_id_of(&transition.from, state_ids).is_none() {
            errors.push(StCodegenError::UnresolvedGoto {
                from: "<transition.from>".to_string(),
                target: format!(
                    "{}.{}",
                    transition.from.task_name, transition.from.step_name
                ),
            });
        }
        if state_id_of(&transition.to, state_ids).is_none() {
            errors.push(StCodegenError::UnresolvedGoto {
                from: format!(
                    "{}.{}",
                    transition.from.task_name, transition.from.step_name
                ),
                target: format!("{}.{}", transition.to.task_name, transition.to.step_name),
            });
        }
    }
}

fn collect_variable_candidates_from_topology(topology: &TopologyGraph) -> Vec<VariableCandidate> {
    let mut out = Vec::new();
    for var in &topology.variables {
        let var_type = match var.var_type {
            VariableType::Bool => StVarType::Bool,
            VariableType::Int => StVarType::Int,
            VariableType::Float => StVarType::Real,
        };
        out.push(VariableCandidate {
            original: var.name.clone(),
            var_type,
        });
    }
    out
}

fn collect_variable_candidates_from_transitions(
    state_machine: &StateMachine,
    candidates: &mut Vec<VariableCandidate>,
    errors: &mut Vec<StCodegenError>,
) {
    for transition in &state_machine.transitions {
        for action in &transition.actions {
            match action {
                TransitionAction::Extend { target, .. }
                | TransitionAction::Retract { target, .. }
                | TransitionAction::Set { target, .. } => {
                    candidates.push(VariableCandidate {
                        original: target.clone(),
                        var_type: StVarType::Bool,
                    });
                }
                TransitionAction::SetAnalog { target, .. }
                | TransitionAction::SetAnalogExpr { target, .. } => {
                    candidates.push(VariableCandidate {
                        original: target.clone(),
                        var_type: StVarType::Real,
                    });
                }
                TransitionAction::Compute { target, .. } => {
                    let already_declared = candidates
                        .iter()
                        .any(|candidate| candidate.original == *target);
                    if !already_declared {
                        candidates.push(VariableCandidate {
                            original: target.clone(),
                            var_type: StVarType::Real,
                        });
                    }
                }
                TransitionAction::CallExtern { binding, .. } => match binding {
                    ExternCallBinding::Single(name) => {
                        candidates.push(VariableCandidate {
                            original: name.clone(),
                            var_type: StVarType::Real,
                        });
                    }
                    ExternCallBinding::Tuple(names) => {
                        for name in names {
                            candidates.push(VariableCandidate {
                                original: name.clone(),
                                var_type: StVarType::Real,
                            });
                        }
                    }
                },
                TransitionAction::CamEngage { .. }
                | TransitionAction::CamDisengage { .. }
                | TransitionAction::CamSwitch { .. }
                | TransitionAction::CamPhase { .. }
                | TransitionAction::AxisMoveRelative { .. }
                | TransitionAction::AxisMoveAbsolute { .. }
                | TransitionAction::Log { .. } => {}
            }
        }

        if let TransitionGuard::Condition { expression } = &transition.guard {
            match parse_condition_strict(expression) {
                Some(parsed) => {
                    candidates.push(VariableCandidate {
                        original: parsed.left.clone(),
                        var_type: infer_left_var_type(&parsed),
                    });

                    if let ConditionValue::Identifier(right_name) = &parsed.right {
                        candidates.push(VariableCandidate {
                            original: right_name.clone(),
                            var_type: infer_right_identifier_var_type(&parsed),
                        });
                    }
                }
                None => {
                    errors.push(StCodegenError::ExpressionNotSupported {
                        expr: expression.clone(),
                    });
                }
            }
        }
    }
}

fn infer_left_var_type(parsed: &ParsedCondition) -> StVarType {
    match parsed.op {
        ConditionOp::Gt | ConditionOp::Lt => StVarType::Real,
        ConditionOp::Eq | ConditionOp::Ne => match parsed.right {
            ConditionValue::Bool(_) => StVarType::Bool,
            ConditionValue::Number(_) => StVarType::Real,
            ConditionValue::Identifier(_) => StVarType::Bool,
        },
    }
}

fn infer_right_identifier_var_type(parsed: &ParsedCondition) -> StVarType {
    match parsed.op {
        ConditionOp::Gt | ConditionOp::Lt => StVarType::Real,
        ConditionOp::Eq | ConditionOp::Ne => StVarType::Bool,
    }
}

fn resolve_variables(
    candidates: Vec<VariableCandidate>,
) -> Result<ResolvedVariables, Vec<StCodegenError>> {
    let mut errors = Vec::new();
    let mut by_original: HashMap<String, String> = HashMap::new();
    let mut normalized_to_original: HashMap<String, String> = HashMap::new();
    let mut declarations: BTreeMap<String, StVarType> = BTreeMap::new();

    for candidate in candidates {
        let normalized = normalize_identifier_for_st(&candidate.original);

        if let Some(existing_original) = normalized_to_original.get(&normalized) {
            if existing_original != &candidate.original {
                errors.push(StCodegenError::VariableNameConflict {
                    original: candidate.original.clone(),
                    normalized: normalized.clone(),
                });
                continue;
            }
        }

        if let Some(existing_norm) = by_original.get(&candidate.original) {
            if existing_norm != &normalized {
                errors.push(StCodegenError::VariableNameConflict {
                    original: candidate.original.clone(),
                    normalized: normalized.clone(),
                });
                continue;
            }
        }

        if let Some(existing_ty) = declarations.get(&normalized).copied() {
            match merge_types(existing_ty, candidate.var_type, &normalized) {
                Ok(merged) => {
                    declarations.insert(normalized.clone(), merged);
                }
                Err(err) => errors.push(err),
            }
        } else {
            declarations.insert(normalized.clone(), candidate.var_type);
        }

        by_original.insert(candidate.original.clone(), normalized.clone());
        normalized_to_original.insert(normalized, candidate.original);
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(ResolvedVariables {
        by_original,
        declarations,
    })
}

fn merge_types(
    existing: StVarType,
    incoming: StVarType,
    normalized_name: &str,
) -> Result<StVarType, StCodegenError> {
    if existing == incoming {
        return Ok(existing);
    }

    match (existing, incoming) {
        (StVarType::Int, StVarType::Real) | (StVarType::Real, StVarType::Int) => {
            Ok(StVarType::Real)
        }
        _ => Err(StCodegenError::TypeConflict {
            name: normalized_name.to_string(),
        }),
    }
}

fn collect_transitions_by_state<'a>(
    state_machine: &'a StateMachine,
    state_ids: &HashMap<(String, String), i32>,
) -> BTreeMap<i32, Vec<&'a Transition>> {
    let mut out: BTreeMap<i32, Vec<&Transition>> = BTreeMap::new();
    for transition in &state_machine.transitions {
        if let Some(state_id) = state_id_of(&transition.from, state_ids) {
            out.entry(state_id).or_default().push(transition);
        }
    }
    out
}

fn collect_timers(
    state_machine: &StateMachine,
    state_ids: &HashMap<(String, String), i32>,
) -> BTreeMap<i32, u64> {
    let mut out: BTreeMap<i32, u64> = BTreeMap::new();

    for transition in &state_machine.transitions {
        let Some(state_id) = state_id_of(&transition.from, state_ids) else {
            continue;
        };

        match transition.guard {
            TransitionGuard::Timeout { duration_ms } | TransitionGuard::Delay { duration_ms } => {
                out.entry(state_id)
                    .and_modify(|existing| *existing = (*existing).max(duration_ms))
                    .or_insert(duration_ms);
            }
            TransitionGuard::Always | TransitionGuard::Condition { .. } => {}
        }

        for timer in &transition.timers {
            match timer.operation {
                TimerOperationKind::Cancel => {
                    // Phase-1 policy: cancel operations are intentionally ignored.
                }
                TimerOperationKind::Start | TimerOperationKind::Reset => {
                    if let Some(duration_ms) = timer.duration_ms {
                        out.entry(state_id)
                            .and_modify(|existing| *existing = (*existing).max(duration_ms))
                            .or_insert(duration_ms);
                    }
                }
            }
        }
    }

    out
}

fn emit_header(out: &mut String, config: &StCodegenConfig, workpiece_semantics_erased: bool) {
    out.push_str("(* Generated by RustPLC ST Codegen *)\n");
    if !config.source_file.is_empty() {
        out.push_str(&format!("(* Source: {} *)\n", config.source_file));
    }
    if workpiece_semantics_erased {
        out.push_str(
            "(* Workpiece verification semantics erased before ST codegen; executable control semantics only. *)\n",
        );
    }
    if config.include_verification_summary {
        out.push_str("(* Verification: produced by RustPLC semantic + verification pipeline *)\n");
    }
}

fn emit_var_block(
    out: &mut String,
    initial_id: i32,
    timers: &BTreeMap<i32, u64>,
    declarations: &BTreeMap<String, StVarType>,
) {
    out.push_str("VAR\n");
    out.push_str(&format!("    {INTERNAL_STATE_VAR}: INT := {initial_id};\n"));
    for timer_id in timers.keys() {
        out.push_str(&format!("    {INTERNAL_TIMER_PREFIX}{timer_id}: TON;\n"));
    }
    for (name, var_type) in declarations {
        let (st_type, init) = match var_type {
            StVarType::Bool => ("BOOL", "FALSE"),
            StVarType::Int => ("DINT", "0"),
            StVarType::Real => ("REAL", "0.0"),
        };
        out.push_str(&format!("    {name}: {st_type} := {init};\n"));
    }
    out.push_str("END_VAR\n\n");
}

fn emit_trace_var_block(out: &mut String) {
    out.push_str("VAR\n");
    for bit in 0..INTERNAL_STATE_TRACE_BITS {
        let byte = bit / 8;
        let offset = bit % 8;
        out.push_str(&format!(
            "    {INTERNAL_STATE_TRACE_PREFIX}{bit} AT %QX{byte}.{offset} : BOOL;\n"
        ));
    }
    out.push_str("END_VAR\n\n");
}

fn emit_state_constants_comment(
    out: &mut String,
    state_machine: &StateMachine,
    state_ids: &HashMap<(String, String), i32>,
) {
    out.push_str("(* State constants (order = StateMachine.states order):\n");
    for state in &state_machine.states {
        if let Some(state_id) = state_id_of(state, state_ids) {
            out.push_str(&format!(
                "   {}.{} = {}\n",
                state.task_name, state.step_name, state_id
            ));
        }
    }
    out.push_str(&format!(
        "   __internal_error__ = {RESERVED_INTERNAL_ERROR_STATE_ID} (reserved)\n"
    ));
    out.push_str("*)\n\n");
}

fn emit_timer_calls(out: &mut String, timers: &BTreeMap<i32, u64>) {
    for (state_id, duration_ms) in timers {
        out.push_str(&format!(
            "{INTERNAL_TIMER_PREFIX}{state_id}(IN := {INTERNAL_STATE_VAR} = {state_id}, PT := T#{duration_ms}ms);\n"
        ));
    }
    if !timers.is_empty() {
        out.push('\n');
    }
}

fn emit_trace_exports(out: &mut String) {
    for bit in 0..INTERNAL_STATE_TRACE_BITS {
        let mask = 1_i32 << bit;
        out.push_str(&format!(
            "{INTERNAL_STATE_TRACE_PREFIX}{bit} := ({INTERNAL_STATE_VAR} MOD {}) >= {mask};\n",
            mask * 2
        ));
    }
    out.push('\n');
}

fn emit_case_block(
    out: &mut String,
    state_machine: &StateMachine,
    state_ids: &HashMap<(String, String), i32>,
    transitions_by_state: &BTreeMap<i32, Vec<&Transition>>,
    resolved_variables: &ResolvedVariables,
) {
    out.push_str(&format!("CASE {INTERNAL_STATE_VAR} OF\n"));

    for state in &state_machine.states {
        let Some(state_id) = state_id_of(state, state_ids) else {
            continue;
        };

        out.push_str(&format!(
            "    {state_id}: (* {}.{} *)\n",
            state.task_name, state.step_name
        ));

        if let Some(transitions) = transitions_by_state.get(&state_id) {
            emit_actions(out, transitions, resolved_variables);
            emit_guard_branches(out, state_id, transitions, state_ids, resolved_variables);
        } else {
            // OpenPLC rejects empty CASE arms, so terminal states must still emit a valid statement.
            out.push_str(&format!(
                "        {INTERNAL_STATE_VAR} := {INTERNAL_STATE_VAR};\n"
            ));
        }

        out.push('\n');
    }

    out.push_str("END_CASE;\n\n");
}

fn emit_actions(
    out: &mut String,
    transitions: &[&Transition],
    resolved_variables: &ResolvedVariables,
) {
    let mut rendered = HashSet::new();

    for transition in transitions {
        for action in &transition.actions {
            let line = render_action(action, resolved_variables);
            if rendered.insert(line.clone()) {
                out.push_str("        ");
                out.push_str(&line);
                out.push('\n');
            }
        }
    }
}

fn emit_openplc_configuration(out: &mut String, config: &StCodegenConfig) {
    let task_interval_ms = config.task_interval_ms.max(1);
    out.push_str("CONFIGURATION Config0\n\n");
    out.push_str("  RESOURCE Res0 ON PLC\n");
    out.push_str(&format!(
        "    TASK MainTask(INTERVAL := T#{task_interval_ms}ms, PRIORITY := 0);\n"
    ));
    out.push_str(&format!(
        "    PROGRAM Inst0 WITH MainTask : {};\n",
        config.program_name
    ));
    out.push_str("  END_RESOURCE\n");
    out.push_str("END_CONFIGURATION\n");
}

fn render_action(action: &TransitionAction, resolved_variables: &ResolvedVariables) -> String {
    match action {
        TransitionAction::Extend { target, .. } => {
            format!("{} := TRUE;", resolved_variables.resolve_identifier(target))
        }
        TransitionAction::Retract { target, .. } => {
            format!(
                "{} := FALSE;",
                resolved_variables.resolve_identifier(target)
            )
        }
        TransitionAction::Set { target, value, .. } => {
            let rendered_value = match value {
                BinaryValue::On => "TRUE",
                BinaryValue::Off => "FALSE",
            };
            format!(
                "{} := {rendered_value};",
                resolved_variables.resolve_identifier(target)
            )
        }
        TransitionAction::SetAnalog {
            target, value_raw, ..
        } => {
            format!(
                "{} := {};",
                resolved_variables.resolve_identifier(target),
                value_raw.trim()
            )
        }
        TransitionAction::SetAnalogExpr {
            target, expr_raw, ..
        } => {
            format!(
                "{} := {};",
                resolved_variables.resolve_identifier(target),
                render_expression_for_st(expr_raw)
            )
        }
        TransitionAction::Compute {
            target, expr_raw, ..
        } => {
            let rendered_expr = if resolved_variables.var_type_for(target) == Some(StVarType::Bool)
            {
                render_bool_assignment_expr(expr_raw)
            } else {
                render_expression_for_st(expr_raw)
            };
            format!(
                "{} := {};",
                resolved_variables.resolve_identifier(target),
                rendered_expr
            )
        }
        TransitionAction::CallExtern {
            function,
            args_raw,
            binding,
        } => {
            let rendered_binding = match binding {
                ExternCallBinding::Single(name) => resolved_variables.resolve_identifier(name),
                ExternCallBinding::Tuple(names) => names
                    .iter()
                    .map(|name| resolved_variables.resolve_identifier(name))
                    .collect::<Vec<_>>()
                    .join(", "),
            };
            format!(
                "(* CALL_EXTERN {}({}) -> {} *)",
                normalize_identifier_for_st(function),
                args_raw.join(", "),
                rendered_binding
            )
        }
        TransitionAction::Log { message } => {
            format!("(* LOG: {} *)", message.replace("*)", "* /"))
        }
        TransitionAction::CamEngage { target } => {
            format!("(* CAM_ENGAGE {} *)", normalize_identifier_for_st(target))
        }
        TransitionAction::CamDisengage { target } => {
            format!(
                "(* CAM_DISENGAGE {} *)",
                normalize_identifier_for_st(target)
            )
        }
        TransitionAction::CamSwitch { target, new_table } => format!(
            "(* CAM_SWITCH {} -> {} *)",
            normalize_identifier_for_st(target),
            normalize_identifier_for_st(new_table)
        ),
        TransitionAction::CamPhase {
            target,
            offset_expr_raw,
        } => format!(
            "(* CAM_PHASE {} := {} *)",
            normalize_identifier_for_st(target),
            offset_expr_raw.trim()
        ),
        TransitionAction::AxisMoveRelative {
            target,
            distance_raw,
            speed_raw,
            port: _,
            semantic_tag: _,
            timeout,
            on_reject,
            on_motion_fault,
            on_safety_fault,
            on_reject_routes,
            on_motion_fault_routes,
            on_safety_fault_routes,
        } => format!(
            "(* AXIS_MOVE_RELATIVE {} distance={} speed={} timeout={}ms->{} on_reject->{} on_motion_fault->{} on_safety_fault->{} routes:{}|{}|{} *)",
            normalize_identifier_for_st(target),
            distance_raw.trim(),
            speed_raw.trim(),
            timeout.duration_ms,
            render_axis_target(&timeout.target_task, timeout.target_step.as_deref()),
            render_axis_fault_target(on_reject),
            render_axis_fault_target(on_motion_fault),
            render_axis_fault_target(on_safety_fault),
            render_axis_fault_routes(on_reject_routes),
            render_axis_fault_routes(on_motion_fault_routes),
            render_axis_fault_routes(on_safety_fault_routes),
        ),
        TransitionAction::AxisMoveAbsolute {
            target,
            position_raw,
            speed_raw,
            require_homed: _,
            port: _,
            semantic_tag: _,
            timeout,
            on_reject,
            on_motion_fault,
            on_safety_fault,
            on_reject_routes,
            on_motion_fault_routes,
            on_safety_fault_routes,
        } => format!(
            "(* AXIS_MOVE_ABSOLUTE {} position={} speed={} timeout={}ms->{} on_reject->{} on_motion_fault->{} on_safety_fault->{} routes:{}|{}|{} *)",
            normalize_identifier_for_st(target),
            position_raw.trim(),
            speed_raw.trim(),
            timeout.duration_ms,
            render_axis_target(&timeout.target_task, timeout.target_step.as_deref()),
            render_axis_fault_target(on_reject),
            render_axis_fault_target(on_motion_fault),
            render_axis_fault_target(on_safety_fault),
            render_axis_fault_routes(on_reject_routes),
            render_axis_fault_routes(on_motion_fault_routes),
            render_axis_fault_routes(on_safety_fault_routes),
        ),
    }
}

fn render_axis_target(task: &str, step: Option<&str>) -> String {
    match step {
        Some(step) => format!("{task}.{step}"),
        None => task.to_string(),
    }
}

fn render_axis_fault_target(branch: &crate::ir::AxisFaultBranch) -> String {
    let target = render_axis_target(&branch.target_task, branch.target_step.as_deref());
    match branch.error_code.as_deref() {
        Some(code) => format!("{target}[{code}]"),
        None => target,
    }
}

fn render_axis_fault_routes(routes: &[crate::ir::AxisFaultRouteBranch]) -> String {
    if routes.is_empty() {
        return "-".to_string();
    }
    routes
        .iter()
        .map(|route| {
            let target = render_axis_target(&route.target_task, route.target_step.as_deref());
            let kind = route
                .kind
                .map(|value| format!("{:?}", value).to_lowercase())
                .unwrap_or_else(|| "*".to_string());
            let code = route
                .code
                .map(|value| value.to_string())
                .unwrap_or_else(|| "*".to_string());
            format!("{kind}:{code}->{target}")
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn render_bool_assignment_expr(expr_raw: &str) -> String {
    let rendered = render_expression_for_st(expr_raw);
    match rendered.trim().to_ascii_uppercase().as_str() {
        "1" | "1.0" | "TRUE" => "TRUE".to_string(),
        "0" | "0.0" | "FALSE" => "FALSE".to_string(),
        _ => rendered,
    }
}

fn render_expression_for_st(expr_raw: &str) -> String {
    let mut out = String::with_capacity(expr_raw.len() + 8);
    let chars = expr_raw.chars().collect::<Vec<_>>();
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        if i + 1 < chars.len() {
            match (chars[i], chars[i + 1]) {
                ('=', '=') => {
                    out.push('=');
                    i += 2;
                    continue;
                }
                ('!', '=') => {
                    out.push_str("<>");
                    i += 2;
                    continue;
                }
                ('&', '&') => {
                    out.push_str("AND");
                    i += 2;
                    continue;
                }
                ('|', '|') => {
                    out.push_str("OR");
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word = chars[start..i].iter().collect::<String>();
            match word.to_ascii_lowercase().as_str() {
                "true" => out.push_str("TRUE"),
                "false" => out.push_str("FALSE"),
                "and" => out.push_str("AND"),
                "or" => out.push_str("OR"),
                "not" => out.push_str("NOT"),
                _ => out.push_str(&word),
            }
            continue;
        }
        out.push(ch);
        i += 1;
    }
    out
}

fn emit_guard_branches(
    out: &mut String,
    from_state_id: i32,
    transitions: &[&Transition],
    state_ids: &HashMap<(String, String), i32>,
    resolved_variables: &ResolvedVariables,
) {
    let mut conditional_branches: Vec<(String, i32)> = Vec::new();
    let mut always_target: Option<i32> = None;

    for transition in transitions {
        let Some(target_id) = state_id_of(&transition.to, state_ids) else {
            continue;
        };

        match &transition.guard {
            TransitionGuard::Always => {
                if always_target.is_none() {
                    always_target = Some(target_id);
                }
            }
            TransitionGuard::Condition { expression } => {
                let parsed = parse_condition_strict(expression)
                    .expect("condition was pre-validated before code generation");
                conditional_branches
                    .push((render_condition(&parsed, resolved_variables), target_id));
            }
            TransitionGuard::Timeout { .. } | TransitionGuard::Delay { .. } => {
                conditional_branches.push((
                    format!("{INTERNAL_TIMER_PREFIX}{from_state_id}.Q"),
                    target_id,
                ));
            }
        }
    }

    if let Some((first_condition, first_target)) = conditional_branches.first() {
        out.push_str(&format!("        IF {first_condition} THEN\n"));
        out.push_str(&format!(
            "            {INTERNAL_STATE_VAR} := {first_target};\n"
        ));

        for (condition, target_id) in conditional_branches.iter().skip(1) {
            out.push_str(&format!("        ELSIF {condition} THEN\n"));
            out.push_str(&format!(
                "            {INTERNAL_STATE_VAR} := {target_id};\n"
            ));
        }

        if let Some(always_target) = always_target {
            out.push_str("        ELSE\n");
            out.push_str(&format!(
                "            {INTERNAL_STATE_VAR} := {always_target};\n"
            ));
        }

        out.push_str("        END_IF;\n");
        return;
    }

    if let Some(always_target) = always_target {
        out.push_str(&format!(
            "        {INTERNAL_STATE_VAR} := {always_target};\n"
        ));
    }
}

fn render_condition(parsed: &ParsedCondition, resolved_variables: &ResolvedVariables) -> String {
    let left = resolved_variables.resolve_identifier(&parsed.left);

    match (&parsed.op, &parsed.right) {
        (ConditionOp::Eq, ConditionValue::Bool(true)) => left,
        (ConditionOp::Eq, ConditionValue::Bool(false)) => format!("NOT {left}"),
        (ConditionOp::Ne, ConditionValue::Bool(true)) => format!("NOT {left}"),
        (ConditionOp::Ne, ConditionValue::Bool(false)) => left,
        _ => {
            let op = match parsed.op {
                ConditionOp::Eq => "=",
                ConditionOp::Ne => "<>",
                ConditionOp::Gt => ">",
                ConditionOp::Lt => "<",
            };
            let right = match &parsed.right {
                ConditionValue::Bool(true) => "TRUE".to_string(),
                ConditionValue::Bool(false) => "FALSE".to_string(),
                ConditionValue::Number(raw) => raw.clone(),
                ConditionValue::Identifier(raw) => resolved_variables.resolve_identifier(raw),
            };
            format!("{left} {op} {right}")
        }
    }
}

fn parse_condition_strict(expression: &str) -> Option<ParsedCondition> {
    let expr = expression.trim();
    if expr.is_empty() {
        return None;
    }

    let (left, op, right) = split_condition(expr)?;
    if !is_identifier_token(left) {
        return None;
    }

    let right_value = if right.eq_ignore_ascii_case("true") {
        ConditionValue::Bool(true)
    } else if right.eq_ignore_ascii_case("false") {
        ConditionValue::Bool(false)
    } else if is_number_literal(right) {
        ConditionValue::Number(right.to_string())
    } else if is_identifier_token(right) {
        ConditionValue::Identifier(right.to_string())
    } else {
        return None;
    };

    if matches!(op, ConditionOp::Gt | ConditionOp::Lt)
        && matches!(right_value, ConditionValue::Bool(_))
    {
        return None;
    }

    Some(ParsedCondition {
        left: left.to_string(),
        op,
        right: right_value,
    })
}

fn split_condition(expr: &str) -> Option<(&str, ConditionOp, &str)> {
    for (needle, op) in [
        ("==", ConditionOp::Eq),
        ("!=", ConditionOp::Ne),
        (">", ConditionOp::Gt),
        ("<", ConditionOp::Lt),
    ] {
        let Some(index) = expr.find(needle) else {
            continue;
        };

        if needle == ">" && expr[index + needle.len()..].starts_with('=') {
            return None;
        }
        if needle == "<" && expr[index + needle.len()..].starts_with('=') {
            return None;
        }

        let left = expr[..index].trim();
        let right = expr[index + needle.len()..].trim();
        if left.is_empty() || right.is_empty() {
            return None;
        }

        if contains_any_operator(left) || contains_any_operator(right) {
            return None;
        }

        return Some((left, op, right));
    }

    None
}

fn contains_any_operator(input: &str) -> bool {
    ["==", "!=", ">=", "<=", ">", "<", "&&", "||"]
        .iter()
        .any(|op| input.contains(op))
}

fn is_identifier_token(raw: &str) -> bool {
    let mut chars = raw.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }

    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '.')
}

fn is_number_literal(raw: &str) -> bool {
    raw.parse::<f64>().is_ok()
}

fn normalize_identifier_for_st(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_was_underscore = false;

    for ch in raw.trim().chars() {
        let normalized = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '_'
        };

        if normalized == '_' {
            if last_was_underscore {
                continue;
            }
            last_was_underscore = true;
            out.push('_');
        } else {
            last_was_underscore = false;
            out.push(normalized);
        }
    }

    if out.is_empty() || out.chars().all(|ch| ch == '_') {
        out = "v".to_string();
    }

    if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        out.insert(0, '_');
    }

    if is_st_reserved_word(&out) {
        out.insert(0, '_');
    }

    if conflicts_internal_name(&out) {
        let suffix = out.trim_start_matches('_');
        out = format!("_u_{}", if suffix.is_empty() { "var" } else { suffix });
    }

    if is_st_reserved_word(&out) {
        out.insert(0, '_');
    }

    out
}

fn conflicts_internal_name(name: &str) -> bool {
    name == INTERNAL_STATE_VAR || name.starts_with(INTERNAL_TIMER_PREFIX)
}

fn is_st_reserved_word(name: &str) -> bool {
    matches!(
        name,
        "program"
            | "end_program"
            | "var"
            | "end_var"
            | "if"
            | "then"
            | "else"
            | "elsif"
            | "end_if"
            | "case"
            | "of"
            | "end_case"
            | "for"
            | "end_for"
            | "while"
            | "end_while"
            | "repeat"
            | "until"
            | "end_repeat"
            | "function"
            | "end_function"
            | "function_block"
            | "end_function_block"
            | "true"
            | "false"
            | "bool"
            | "int"
            | "dint"
            | "real"
            | "string"
            | "ton"
            | "to"
            | "do"
            | "and"
            | "or"
            | "not"
            | "xor"
    )
}

fn state_key(state: &State) -> (String, String) {
    (state.task_name.clone(), state.step_name.clone())
}

fn state_id_of(state: &State, state_ids: &HashMap<(String, String), i32>) -> Option<i32> {
    state_ids.get(&state_key(state)).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        AxisFaultBranch, AxisFaultCategory, AxisFaultKind, AxisTimeoutBranch, ConstraintSet,
        StateMachine, TimerOperation, Transition, TransitionAction, TransitionGuard,
        WorkpieceCarrierDef, WorkpieceCarrierLayoutDef, WorkpieceEffect, WorkpieceHolderDef,
        WorkpiecePropertyDef, WorkpiecePropertyTypeDef, WorkpieceSiteDef, WorkpieceSiteKind,
        WorkpieceTypeDef,
    };
    use std::collections::BTreeMap;

    fn state(task: &str, step: &str) -> State {
        State {
            task_name: task.to_string(),
            step_name: step.to_string(),
        }
    }

    fn empty_topology() -> TopologyGraph {
        TopologyGraph::default()
    }

    #[test]
    fn state_ids_follow_ir_order_and_never_use_reserved_error_state() {
        let s0 = state("main", "s0");
        let s1 = state("main", "s1");
        let s2 = state("main", "s2");
        let sm = StateMachine {
            states: vec![s0.clone(), s1.clone(), s2.clone()],
            transitions: vec![Transition {
                from: s0.clone(),
                to: s1.clone(),
                guard: TransitionGuard::Always,
                actions: vec![],
                effects: vec![],
                timers: vec![],
            }],
            initial: s1.clone(),
            analog_regions: BTreeMap::new(),
            task_contexts: vec![],
        };

        let ids = assign_state_ids(&sm);
        assert_eq!(state_id_of(&s0, &ids), Some(0));
        assert_eq!(state_id_of(&s1, &ids), Some(10));
        assert_eq!(state_id_of(&s2, &ids), Some(20));
        assert!(
            ids.values()
                .all(|id| *id != RESERVED_INTERNAL_ERROR_STATE_ID)
        );

        let st = generate_st(
            &empty_topology(),
            &ConstraintSet::default(),
            &sm,
            &StCodegenConfig::default(),
        )
        .expect("codegen should succeed");
        assert!(st.contains("_state: INT := 10;"));
        assert!(st.contains("main.s0 = 0"));
        assert!(st.contains("main.s1 = 10"));
        assert!(st.contains("main.s2 = 20"));
        assert!(st.contains("__internal_error__ = 9999 (reserved)"));
    }

    #[test]
    fn variable_naming_rules_cover_reserved_and_internal_conflicts() {
        let s0 = state("main", "idle");
        let sm = StateMachine {
            states: vec![s0.clone()],
            transitions: vec![Transition {
                from: s0.clone(),
                to: s0,
                guard: TransitionGuard::Condition {
                    expression: "IF == true".to_string(),
                },
                actions: vec![
                    TransitionAction::Set {
                        target: "_state".to_string(),
                        port: "self".to_string(),
                        value: BinaryValue::On,
                    },
                    TransitionAction::Set {
                        target: "Valve-A".to_string(),
                        port: "self".to_string(),
                        value: BinaryValue::Off,
                    },
                ],
                effects: vec![],
                timers: vec![],
            }],
            initial: state("main", "idle"),
            analog_regions: BTreeMap::new(),
            task_contexts: vec![],
        };

        let st = generate_st(
            &empty_topology(),
            &ConstraintSet::default(),
            &sm,
            &StCodegenConfig::default(),
        )
        .expect("codegen should succeed");
        assert!(st.contains("_if: BOOL := FALSE;"));
        assert!(st.contains("_u_state: BOOL := FALSE;"));
        assert!(st.contains("valve_a: BOOL := FALSE;"));
        assert!(st.contains("IF _if THEN"));
    }

    #[test]
    fn variable_name_conflict_after_normalization_is_rejected() {
        let s0 = state("main", "idle");
        let sm = StateMachine {
            states: vec![s0.clone()],
            transitions: vec![Transition {
                from: s0.clone(),
                to: s0,
                guard: TransitionGuard::Always,
                actions: vec![
                    TransitionAction::Set {
                        target: "Valve-A".to_string(),
                        port: "self".to_string(),
                        value: BinaryValue::On,
                    },
                    TransitionAction::Set {
                        target: "valve_a".to_string(),
                        port: "self".to_string(),
                        value: BinaryValue::Off,
                    },
                ],
                effects: vec![],
                timers: vec![],
            }],
            initial: state("main", "idle"),
            analog_regions: BTreeMap::new(),
            task_contexts: vec![],
        };

        let errors = generate_st(
            &empty_topology(),
            &ConstraintSet::default(),
            &sm,
            &StCodegenConfig::default(),
        )
        .expect_err("must fail");
        assert!(errors.iter().any(|e| {
            matches!(
                e,
                StCodegenError::VariableNameConflict {
                    normalized,
                    ..
                } if normalized == "valve_a"
            )
        }));
    }

    #[test]
    fn bool_real_type_conflict_is_rejected() {
        let s0 = state("main", "idle");
        let sm = StateMachine {
            states: vec![s0.clone()],
            transitions: vec![Transition {
                from: s0.clone(),
                to: s0,
                guard: TransitionGuard::Always,
                actions: vec![
                    TransitionAction::Set {
                        target: "mix".to_string(),
                        port: "self".to_string(),
                        value: BinaryValue::On,
                    },
                    TransitionAction::SetAnalog {
                        target: "mix".to_string(),
                        port: "self".to_string(),
                        value_raw: "1.0".to_string(),
                    },
                ],
                effects: vec![],
                timers: vec![],
            }],
            initial: state("main", "idle"),
            analog_regions: BTreeMap::new(),
            task_contexts: vec![],
        };

        let errors = generate_st(
            &empty_topology(),
            &ConstraintSet::default(),
            &sm,
            &StCodegenConfig::default(),
        )
        .expect_err("must fail");
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, StCodegenError::TypeConflict { name } if name == "mix"))
        );
    }

    #[test]
    fn compute_target_uses_declared_bool_type_from_topology() {
        let s0 = state("main", "idle");
        let sm = StateMachine {
            states: vec![s0.clone()],
            transitions: vec![Transition {
                from: s0.clone(),
                to: s0,
                guard: TransitionGuard::Always,
                actions: vec![TransitionAction::Compute {
                    target: "flag".to_string(),
                    expr_raw: "1".to_string(),
                }],
                effects: vec![],
                timers: vec![],
            }],
            initial: state("main", "idle"),
            analog_regions: BTreeMap::new(),
            task_contexts: vec![],
        };

        let mut topology = empty_topology();
        topology.variables.push(crate::ir::VariableDef {
            name: "flag".to_string(),
            var_type: VariableType::Bool,
            initial_value: 0.0,
            index: 0,
        });

        let st = generate_st(
            &topology,
            &ConstraintSet::default(),
            &sm,
            &StCodegenConfig::default(),
        )
        .expect("codegen should succeed");
        assert!(st.contains("flag: BOOL := FALSE;"));
        assert!(st.contains("flag := TRUE;"));
    }

    #[test]
    fn compute_bool_expression_is_rendered_with_st_boolean_operators() {
        let s0 = state("main", "idle");
        let sm = StateMachine {
            states: vec![s0.clone()],
            transitions: vec![Transition {
                from: s0.clone(),
                to: s0,
                guard: TransitionGuard::Always,
                actions: vec![TransitionAction::Compute {
                    target: "flag".to_string(),
                    expr_raw: "NOT(a) OR (b==1)".to_string(),
                }],
                effects: vec![],
                timers: vec![],
            }],
            initial: state("main", "idle"),
            analog_regions: BTreeMap::new(),
            task_contexts: vec![],
        };

        let mut topology = empty_topology();
        topology.variables.push(crate::ir::VariableDef {
            name: "flag".to_string(),
            var_type: VariableType::Bool,
            initial_value: 0.0,
            index: 0,
        });
        topology.variables.push(crate::ir::VariableDef {
            name: "a".to_string(),
            var_type: VariableType::Bool,
            initial_value: 0.0,
            index: 1,
        });
        topology.variables.push(crate::ir::VariableDef {
            name: "b".to_string(),
            var_type: VariableType::Float,
            initial_value: 0.0,
            index: 2,
        });

        let st = generate_st(
            &topology,
            &ConstraintSet::default(),
            &sm,
            &StCodegenConfig::default(),
        )
        .expect("codegen should succeed");
        assert!(
            st.contains("flag := NOT(a) OR (b=1);") || st.contains("flag := (NOT(a) OR (b=1));")
        );
        assert!(
            !st.contains("=="),
            "ST should not contain C-style comparison operator"
        );
    }

    #[test]
    fn only_simple_condition_expressions_are_allowed() {
        let s0 = state("main", "idle");
        let s1 = state("main", "done");
        let sm = StateMachine {
            states: vec![s0.clone(), s1.clone()],
            transitions: vec![Transition {
                from: s0,
                to: s1,
                guard: TransitionGuard::Condition {
                    expression: "sensor_a >= 1".to_string(),
                },
                actions: vec![],
                effects: vec![],
                timers: vec![],
            }],
            initial: state("main", "idle"),
            analog_regions: BTreeMap::new(),
            task_contexts: vec![],
        };

        let errors = generate_st(
            &empty_topology(),
            &ConstraintSet::default(),
            &sm,
            &StCodegenConfig::default(),
        )
        .expect_err("must fail");
        assert!(errors.iter().any(|e| {
            matches!(
                e,
                StCodegenError::ExpressionNotSupported { expr } if expr == "sensor_a >= 1"
            )
        }));
    }

    #[test]
    fn timer_calls_precede_case_and_cancel_operations_are_ignored() {
        let s0 = state("main", "wait");
        let s1 = state("main", "done");
        let sm = StateMachine {
            states: vec![s0.clone(), s1.clone()],
            transitions: vec![Transition {
                from: s0,
                to: s1,
                guard: TransitionGuard::Timeout { duration_ms: 500 },
                actions: vec![],
                effects: vec![],
                timers: vec![TimerOperation {
                    timer_name: "phase_timer".to_string(),
                    operation: TimerOperationKind::Cancel,
                    duration_ms: None,
                }],
            }],
            initial: state("main", "wait"),
            analog_regions: BTreeMap::new(),
            task_contexts: vec![],
        };

        let st = generate_st(
            &empty_topology(),
            &ConstraintSet::default(),
            &sm,
            &StCodegenConfig::default(),
        )
        .expect("codegen should succeed");
        let timer_pos = st
            .find("_timer_0(IN := _state = 0, PT := T#500ms);")
            .expect("timer call should exist");
        let case_pos = st.find("CASE _state OF").expect("CASE should exist");
        assert!(timer_pos < case_pos);
        assert!(!st.contains("phase_timer"));
    }

    #[test]
    fn var_block_covers_action_targets_and_wait_variables() {
        let s0 = state("main", "idle");
        let s1 = state("main", "run");
        let sm = StateMachine {
            states: vec![s0.clone(), s1.clone()],
            transitions: vec![
                Transition {
                    from: s0.clone(),
                    to: s1.clone(),
                    guard: TransitionGuard::Condition {
                        expression: "sensor_A_ext == true".to_string(),
                    },
                    actions: vec![TransitionAction::Extend {
                        target: "Valve-A".to_string(),
                        port: "self".to_string(),
                        timeout: None,
                        on_motion_fault: None,
                        on_safety_fault: None,
                    }],
                    effects: vec![],
                    timers: vec![],
                },
                Transition {
                    from: s1,
                    to: s0,
                    guard: TransitionGuard::Condition {
                        expression: "pressure_in > 5.0".to_string(),
                    },
                    actions: vec![TransitionAction::SetAnalog {
                        target: "pressure_out".to_string(),
                        port: "self".to_string(),
                        value_raw: "6.2".to_string(),
                    }],
                    effects: vec![],
                    timers: vec![],
                },
            ],
            initial: state("main", "idle"),
            analog_regions: BTreeMap::new(),
            task_contexts: vec![],
        };

        let st = generate_st(
            &empty_topology(),
            &ConstraintSet::default(),
            &sm,
            &StCodegenConfig::default(),
        )
        .expect("codegen should succeed");
        assert!(st.contains("valve_a: BOOL := FALSE;"));
        assert!(st.contains("sensor_a_ext: BOOL := FALSE;"));
        assert!(st.contains("pressure_in: REAL := 0.0;"));
        assert!(st.contains("pressure_out: REAL := 0.0;"));
    }

    #[test]
    fn parallel_and_race_synthetic_states_can_codegen() {
        let parallel = state("task", "step__parallel_1_fork");
        let race = state("task", "step__race_1_decision");
        let sm = StateMachine {
            states: vec![parallel.clone(), race.clone()],
            transitions: vec![Transition {
                from: parallel.clone(),
                to: race.clone(),
                guard: TransitionGuard::Always,
                actions: vec![],
                effects: vec![],
                timers: vec![],
            }],
            initial: parallel,
            analog_regions: BTreeMap::new(),
            task_contexts: vec![],
        };

        let st = generate_st(
            &empty_topology(),
            &ConstraintSet::default(),
            &sm,
            &StCodegenConfig::default(),
        )
        .expect("codegen should succeed");
        assert!(st.contains("CASE _state OF"));
    }

    #[test]
    fn generate_st_maps_guards_and_actions() {
        let s0 = state("main", "idle");
        let s1 = state("main", "run");
        let s2 = state("main", "fault");

        let sm = StateMachine {
            states: vec![s0.clone(), s1.clone(), s2.clone()],
            transitions: vec![
                Transition {
                    from: s0.clone(),
                    to: s1.clone(),
                    guard: TransitionGuard::Condition {
                        expression: "sensor_a_ext == true".to_string(),
                    },
                    actions: vec![TransitionAction::Extend {
                        target: "Valve-A".to_string(),
                        port: "self".to_string(),
                        timeout: None,
                        on_motion_fault: None,
                        on_safety_fault: None,
                    }],
                    effects: vec![],
                    timers: vec![],
                },
                Transition {
                    from: s0.clone(),
                    to: s2.clone(),
                    guard: TransitionGuard::Timeout { duration_ms: 500 },
                    actions: vec![],
                    effects: vec![],
                    timers: vec![],
                },
                Transition {
                    from: s1,
                    to: s2,
                    guard: TransitionGuard::Always,
                    actions: vec![TransitionAction::Log {
                        message: "trip".to_string(),
                    }],
                    effects: vec![],
                    timers: vec![],
                },
            ],
            initial: s0,
            analog_regions: BTreeMap::new(),
            task_contexts: vec![],
        };

        let st = generate_st(
            &empty_topology(),
            &ConstraintSet::default(),
            &sm,
            &StCodegenConfig::default(),
        )
        .expect("codegen should succeed");

        assert!(st.contains("PROGRAM Main"));
        assert!(st.contains("VAR"));
        assert!(st.contains("CASE _state OF"));
        assert!(st.contains("_timer_0(IN := _state = 0, PT := T#500ms);"));
        assert!(st.contains("IF sensor_a_ext THEN"));
        assert!(st.contains("ELSIF _timer_0.Q THEN"));
        assert!(st.contains("(* LOG: trip *)"));
        assert!(st.contains("valve_a := TRUE;"));
        assert!(st.contains("_state := 20;"));
        assert!(st.contains("CONFIGURATION Config0"));
        assert!(st.contains("TASK MainTask(INTERVAL := T#10ms, PRIORITY := 0);"));
    }

    #[test]
    fn generate_st_emits_noop_for_terminal_case_arm() {
        let s0 = state("main", "idle");
        let s1 = state("done", "halt");

        let sm = StateMachine {
            states: vec![s0.clone(), s1.clone()],
            transitions: vec![Transition {
                from: s0.clone(),
                to: s1.clone(),
                guard: TransitionGuard::Always,
                actions: vec![],
                effects: vec![],
                timers: vec![],
            }],
            initial: s0,
            analog_regions: BTreeMap::new(),
            task_contexts: vec![],
        };

        let st = generate_st(
            &empty_topology(),
            &ConstraintSet::default(),
            &sm,
            &StCodegenConfig::default(),
        )
        .expect("codegen should succeed");

        assert!(st.contains("10: (* done.halt *)"));
        assert!(st.contains("_state := _state;"));
    }

    #[test]
    fn generate_st_renders_axis_move_branches_and_fault_targets() {
        let s0 = state("motion", "run");
        let s1 = state("done", "halt");

        let sm = StateMachine {
            states: vec![s0.clone(), s1.clone()],
            transitions: vec![Transition {
                from: s0.clone(),
                to: s1.clone(),
                guard: TransitionGuard::Always,
                actions: vec![TransitionAction::AxisMoveRelative {
                    target: "axis_x".to_string(),
                    port: "self".to_string(),
                    distance_raw: "10".to_string(),
                    speed_raw: "2".to_string(),
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
                        error_code: Some("AXIS_MOTION".to_string()),
                    },
                    on_safety_fault: AxisFaultBranch {
                        target_task: "fault".to_string(),
                        target_step: Some("safety_fault".to_string()),
                        kind: AxisFaultKind::Safety,
                        category: AxisFaultCategory::Safety,
                        vendor_code: None,
                        error_code: Some("AXIS_SAFETY".to_string()),
                    },
                    on_reject_routes: vec![],
                    on_motion_fault_routes: vec![],
                    on_safety_fault_routes: vec![],
                }],
                effects: vec![],
                timers: vec![],
            }],
            initial: s0,
            analog_regions: BTreeMap::new(),
            task_contexts: vec![],
        };

        let st = generate_st(
            &empty_topology(),
            &ConstraintSet::default(),
            &sm,
            &StCodegenConfig::default(),
        )
        .expect("axis move codegen should succeed");
        assert!(st.contains("AXIS_MOVE_RELATIVE axis_x distance=10 speed=2"));
        assert!(st.contains("timeout=500ms->fault.timeout"));
        assert!(st.contains("on_reject->fault.reject[AXIS_REJECT]"));
        assert!(st.contains("on_motion_fault->fault.motion_fault[AXIS_MOTION]"));
        assert!(st.contains("on_safety_fault->fault.safety_fault[AXIS_SAFETY]"));
        assert!(st.contains("routes:-|-|-"));
    }

    #[test]
    fn generate_st_rejects_semantic_resource_interlock() {
        let s0 = state("motion", "run");
        let sm = StateMachine {
            states: vec![s0.clone()],
            transitions: vec![],
            initial: s0,
            analog_regions: BTreeMap::new(),
            task_contexts: vec![],
        };
        let constraints = ConstraintSet {
            safety: vec![],
            workpiece_types: vec![],
            workpiece_sites: vec![],
            workpiece_holders: vec![],
            workpiece_carriers: vec![],
            semantic_resources: vec![crate::ir::SemanticResource {
                name: "slide_pick_zone".to_string(),
                mode: crate::ir::SemanticResourceMode::Exclusive,
                purpose: None,
            }],
            resource_claims: vec![],
            timing: vec![],
            causality: vec![],
        };

        let errors = generate_st(
            &empty_topology(),
            &constraints,
            &sm,
            &StCodegenConfig::default(),
        )
        .expect_err("SRI should be rejected by ST backend");
        assert!(errors.iter().any(|error| {
            matches!(error, StCodegenError::SemanticResourceInterlockUnsupported)
        }));
    }

    #[test]
    fn generate_st_rejects_closed_loop_cylinder_semantics() {
        let s0 = state("main", "extend");
        let s1 = state("done", "halt");
        let sm = StateMachine {
            states: vec![s0.clone(), s1.clone()],
            transitions: vec![Transition {
                from: s0.clone(),
                to: s1.clone(),
                guard: TransitionGuard::Always,
                actions: vec![TransitionAction::Extend {
                    target: "cyl_a".to_string(),
                    port: "self".to_string(),
                    timeout: Some(crate::ir::MotionTimeoutBranch {
                        duration_ms: 500,
                        target_task: "fault".to_string(),
                        target_step: Some("timeout".to_string()),
                    }),
                    on_motion_fault: Some(crate::ir::MotionFaultBranch {
                        target_task: "fault".to_string(),
                        target_step: Some("motion_fault".to_string()),
                    }),
                    on_safety_fault: Some(crate::ir::MotionFaultBranch {
                        target_task: "fault".to_string(),
                        target_step: Some("safety_fault".to_string()),
                    }),
                }],
                effects: vec![],
                timers: vec![],
            }],
            initial: s0,
            analog_regions: BTreeMap::new(),
            task_contexts: vec![],
        };

        let errors = generate_st(
            &empty_topology(),
            &ConstraintSet::default(),
            &sm,
            &StCodegenConfig::default(),
        )
        .expect_err("ST backend should reject closed-loop cylinder semantics");

        assert!(errors.iter().any(|error| {
            matches!(
                error,
                StCodegenError::ClosedLoopCylinderSemanticsUnsupported { target }
                if target == "cyl_a"
            )
        }));
    }

    #[test]
    fn generate_st_erases_workpiece_semantics_before_codegen() {
        let s0 = state("transfer", "pick");
        let s1 = state("transfer", "place");
        let sm = StateMachine {
            states: vec![s0.clone(), s1.clone()],
            transitions: vec![Transition {
                from: s0.clone(),
                to: s1.clone(),
                guard: TransitionGuard::Always,
                actions: vec![],
                effects: vec![WorkpieceEffect::Acquire {
                    holder: "arm".to_string(),
                    from: "infeed".to_string(),
                }],
                timers: vec![],
            }],
            initial: s0,
            analog_regions: BTreeMap::new(),
            task_contexts: vec![],
        };
        let constraints = ConstraintSet {
            safety: vec![],
            workpiece_types: vec![WorkpieceTypeDef {
                name: "part".to_string(),
                properties: vec![WorkpiecePropertyDef {
                    name: "inspected".to_string(),
                    property_type: WorkpiecePropertyTypeDef::Bool,
                }],
                normal_terminal_states: vec!["finished".to_string()],
                abnormal_terminal_states: vec!["rejected".to_string()],
                ingress_sites: vec!["infeed".to_string()],
                normal_egress_sites: vec!["outfeed".to_string()],
                abnormal_egress_sites: vec!["reject_bin".to_string()],
                allows: vec![],
                derived_from: vec![],
            }],
            workpiece_sites: vec![WorkpieceSiteDef {
                name: "infeed".to_string(),
                kind: WorkpieceSiteKind::WorkpieceLocation,
                capacity: 1,
            }],
            workpiece_holders: vec![WorkpieceHolderDef {
                name: "arm".to_string(),
                capacity: 1,
            }],
            workpiece_carriers: vec![WorkpieceCarrierDef {
                name: "tray".to_string(),
                layout: WorkpieceCarrierLayoutDef::Slots { count: 4 },
            }],
            semantic_resources: vec![],
            resource_claims: vec![],
            timing: vec![],
            causality: vec![],
        };

        let st = generate_st(
            &empty_topology(),
            &constraints,
            &sm,
            &StCodegenConfig::default(),
        )
        .expect("workpiece verification semantics should be erased for ST codegen");

        assert!(st.contains("Workpiece verification semantics erased before ST codegen"));
        assert!(st.contains("transfer.pick = 0"));
        assert!(st.contains("transfer.place = 10"));
        assert!(!st.contains("acquire holder arm from infeed"));
    }
}
