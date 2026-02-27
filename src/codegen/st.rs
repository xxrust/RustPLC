use crate::ir::{
    BinaryValue, State, StateMachine, TimerOperationKind, TopologyGraph, Transition,
    TransitionAction, TransitionGuard, VariableType,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

const STATE_ID_STRIDE: i32 = 10;
const RESERVED_INTERNAL_ERROR_STATE_ID: i32 = 9999;
const INTERNAL_STATE_VAR: &str = "_state";
const INTERNAL_TIMER_PREFIX: &str = "_timer_";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StCodegenConfig {
    pub program_name: String,
    pub source_file: String,
    pub include_verification_summary: bool,
}

impl Default for StCodegenConfig {
    fn default() -> Self {
        Self {
            program_name: "Main".to_string(),
            source_file: String::new(),
            include_verification_summary: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StCodegenError {
    ParallelNotSupported {
        task: String,
        step: String,
    },
    RaceNotSupported {
        task: String,
        step: String,
    },
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
}

impl fmt::Display for StCodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StCodegenError::ParallelNotSupported { task, step } => {
                write!(f, "parallel is not supported in phase 1: {task}.{step}")
            }
            StCodegenError::RaceNotSupported { task, step } => {
                write!(f, "race is not supported in phase 1: {task}.{step}")
            }
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
    state_machine: &StateMachine,
    config: &StCodegenConfig,
) -> Result<String, Vec<StCodegenError>> {
    let mut errors = collect_unsupported_constructs(state_machine);
    if state_machine.states.is_empty() {
        errors.push(StCodegenError::EmptyStateMachine);
    }

    let state_ids = assign_state_ids(state_machine);
    validate_state_references(state_machine, &state_ids, &mut errors);

    let mut variable_candidates = collect_variable_candidates_from_topology(topology);
    collect_variable_candidates_from_transitions(
        state_machine,
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

    let initial_id = state_id_of(&state_machine.initial, &state_ids)
        .expect("initial state must exist when all state references are valid");
    let transitions_by_state = collect_transitions_by_state(state_machine, &state_ids);
    let timers = collect_timers(state_machine, &state_ids);

    let mut out = String::new();
    emit_header(&mut out, config);
    out.push_str(&format!("PROGRAM {}\n", config.program_name));
    emit_var_block(
        &mut out,
        initial_id,
        &timers,
        &resolved_variables.declarations,
    );
    emit_state_constants_comment(&mut out, state_machine, &state_ids);
    emit_timer_calls(&mut out, &timers);
    emit_case_block(
        &mut out,
        state_machine,
        &state_ids,
        &transitions_by_state,
        &resolved_variables,
    );
    out.push_str("END_PROGRAM\n");

    Ok(out)
}

fn collect_unsupported_constructs(state_machine: &StateMachine) -> Vec<StCodegenError> {
    let mut errors = Vec::new();
    let mut seen_parallel = HashSet::new();
    let mut seen_race = HashSet::new();

    for state in &state_machine.states {
        let key = format!("{}.{}", state.task_name, state.step_name);
        if state.step_name.contains("__parallel_") && seen_parallel.insert(key.clone()) {
            errors.push(StCodegenError::ParallelNotSupported {
                task: state.task_name.clone(),
                step: state.step_name.clone(),
            });
        }
        if state.step_name.contains("__race_") && seen_race.insert(key) {
            errors.push(StCodegenError::RaceNotSupported {
                task: state.task_name.clone(),
                step: state.step_name.clone(),
            });
        }
    }

    errors
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
                | TransitionAction::SetAnalogExpr { target, .. }
                | TransitionAction::Compute { target, .. } => {
                    candidates.push(VariableCandidate {
                        original: target.clone(),
                        var_type: StVarType::Real,
                    });
                }
                TransitionAction::CamEngage { .. }
                | TransitionAction::CamDisengage { .. }
                | TransitionAction::CamSwitch { .. }
                | TransitionAction::CamPhase { .. }
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

fn emit_header(out: &mut String, config: &StCodegenConfig) {
    out.push_str("(* Generated by RustPLC ST Codegen *)\n");
    if !config.source_file.is_empty() {
        out.push_str(&format!("(* Source: {} *)\n", config.source_file));
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
                expr_raw.trim()
            )
        }
        TransitionAction::Compute {
            target, expr_raw, ..
        } => {
            format!(
                "{} := {};",
                resolved_variables.resolve_identifier(target),
                expr_raw.trim()
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
    }
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
    use crate::ir::{StateMachine, TimerOperation, Transition, TransitionAction, TransitionGuard};
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
                timers: vec![],
            }],
            initial: s1.clone(),
            analog_regions: BTreeMap::new(),
        };

        let ids = assign_state_ids(&sm);
        assert_eq!(state_id_of(&s0, &ids), Some(0));
        assert_eq!(state_id_of(&s1, &ids), Some(10));
        assert_eq!(state_id_of(&s2, &ids), Some(20));
        assert!(
            ids.values()
                .all(|id| *id != RESERVED_INTERNAL_ERROR_STATE_ID)
        );

        let st = generate_st(&empty_topology(), &sm, &StCodegenConfig::default())
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
                timers: vec![],
            }],
            initial: state("main", "idle"),
            analog_regions: BTreeMap::new(),
        };

        let st = generate_st(&empty_topology(), &sm, &StCodegenConfig::default())
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
                timers: vec![],
            }],
            initial: state("main", "idle"),
            analog_regions: BTreeMap::new(),
        };

        let errors = generate_st(&empty_topology(), &sm, &StCodegenConfig::default())
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
                timers: vec![],
            }],
            initial: state("main", "idle"),
            analog_regions: BTreeMap::new(),
        };

        let errors = generate_st(&empty_topology(), &sm, &StCodegenConfig::default())
            .expect_err("must fail");
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, StCodegenError::TypeConflict { name } if name == "mix"))
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
                timers: vec![],
            }],
            initial: state("main", "idle"),
            analog_regions: BTreeMap::new(),
        };

        let errors = generate_st(&empty_topology(), &sm, &StCodegenConfig::default())
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
                timers: vec![TimerOperation {
                    timer_name: "phase_timer".to_string(),
                    operation: TimerOperationKind::Cancel,
                    duration_ms: None,
                }],
            }],
            initial: state("main", "wait"),
            analog_regions: BTreeMap::new(),
        };

        let st = generate_st(&empty_topology(), &sm, &StCodegenConfig::default())
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
                    }],
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
                    timers: vec![],
                },
            ],
            initial: state("main", "idle"),
            analog_regions: BTreeMap::new(),
        };

        let st = generate_st(&empty_topology(), &sm, &StCodegenConfig::default())
            .expect("codegen should succeed");
        assert!(st.contains("valve_a: BOOL := FALSE;"));
        assert!(st.contains("sensor_a_ext: BOOL := FALSE;"));
        assert!(st.contains("pressure_in: REAL := 0.0;"));
        assert!(st.contains("pressure_out: REAL := 0.0;"));
    }

    #[test]
    fn parallel_and_race_states_are_rejected() {
        let parallel = state("task", "step__parallel_1_fork");
        let race = state("task", "step__race_1_decision");
        let sm = StateMachine {
            states: vec![parallel.clone(), race],
            transitions: vec![],
            initial: parallel,
            analog_regions: BTreeMap::new(),
        };

        let errors = generate_st(&empty_topology(), &sm, &StCodegenConfig::default())
            .expect_err("must fail");

        assert!(
            errors
                .iter()
                .any(|e| matches!(e, StCodegenError::ParallelNotSupported { .. }))
        );
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, StCodegenError::RaceNotSupported { .. }))
        );
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
                    }],
                    timers: vec![],
                },
                Transition {
                    from: s0.clone(),
                    to: s2.clone(),
                    guard: TransitionGuard::Timeout { duration_ms: 500 },
                    actions: vec![],
                    timers: vec![],
                },
                Transition {
                    from: s1,
                    to: s2,
                    guard: TransitionGuard::Always,
                    actions: vec![TransitionAction::Log {
                        message: "trip".to_string(),
                    }],
                    timers: vec![],
                },
            ],
            initial: s0,
            analog_regions: BTreeMap::new(),
        };

        let st = generate_st(&empty_topology(), &sm, &StCodegenConfig::default())
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
    }
}
