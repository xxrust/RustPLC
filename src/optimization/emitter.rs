use crate::ast::{
    ActionStatement, BinaryOperator, Branch, ComparisonOperator, ConditionExpression,
    DurationValue, EffectKind, EffectStatement, Expression, ExternCallBinding, GotoDirective,
    LiteralValue, OnCompleteDirective, ParallelBlock, PlcProgram, RaceBlock, RaceBranch,
    StateReference, StepStatement, TaskDeclaration, TimeUnit, TimeoutDirective, WaitCondition,
    WaitStatement,
};
use crate::optimization::{CandidateLegality, OptimizationLegalityError, render_plc_errors};
use crate::semantic::{
    build_constraint_set, build_state_machine, build_topology_graph, preprocess_program,
};
use crate::verification::verify_all;
use std::fmt::Write;

pub fn recheck_candidate_legality(program: &PlcProgram) -> CandidateLegality {
    match recheck_program(program) {
        Ok(()) => CandidateLegality {
            is_legal: true,
            diagnostics: Vec::new(),
        },
        Err(OptimizationLegalityError::Parse(errors))
        | Err(OptimizationLegalityError::Semantic(errors))
        | Err(OptimizationLegalityError::Verification(errors)) => CandidateLegality {
            is_legal: false,
            diagnostics: errors,
        },
    }
}

pub fn emit_optimized_plc(original_source: &str, program: &PlcProgram) -> Result<String, String> {
    let prefix = extract_non_task_prefix(original_source)?;
    let mut rendered = prefix;
    rendered.push_str("[tasks]\n\n");
    rendered.push_str(&render_tasks(&program.tasks.tasks));
    Ok(rendered)
}

fn recheck_program(program: &PlcProgram) -> Result<(), OptimizationLegalityError> {
    let expanded = preprocess_program(program)
        .map_err(|errors| OptimizationLegalityError::Semantic(render_plc_errors(errors)))?;
    let topology = build_topology_graph(&expanded)
        .map_err(|errors| OptimizationLegalityError::Semantic(render_plc_errors(errors)))?;
    let constraints = build_constraint_set(&expanded)
        .map_err(|errors| OptimizationLegalityError::Semantic(render_plc_errors(errors)))?;
    let state_machine = build_state_machine(&expanded)
        .map_err(|errors| OptimizationLegalityError::Semantic(render_plc_errors(errors)))?;
    verify_all(&expanded, &topology, &constraints, &state_machine).map_err(|errors| {
        OptimizationLegalityError::Verification(
            errors.into_iter().map(|error| error.to_string()).collect(),
        )
    })?;
    Ok(())
}

fn extract_non_task_prefix(source: &str) -> Result<String, String> {
    let marker = "[tasks]";
    let start = source
        .find(marker)
        .ok_or_else(|| "source does not contain [tasks] section".to_string())?;
    Ok(source[..start].to_string())
}

fn render_tasks(tasks: &[TaskDeclaration]) -> String {
    let mut out = String::new();
    for (task_index, task) in tasks.iter().enumerate() {
        if task_index > 0 {
            out.push('\n');
        }
        writeln!(&mut out, "task {}:", task.name).expect("write task");
        for step in &task.steps {
            writeln!(&mut out, "    step {}:", step.name).expect("write step");
            for statement in &step.statements {
                render_statement(statement, 2, &mut out);
            }
            if step.statements.is_empty() {
                out.push('\n');
            }
        }
        if let Some(on_complete) = &task.on_complete {
            match on_complete {
                OnCompleteDirective::Goto { target } => {
                    writeln!(
                        &mut out,
                        "    on_complete: goto {}",
                        render_goto_target(target)
                    )
                    .expect("write on_complete");
                }
                OnCompleteDirective::Unreachable => {
                    writeln!(&mut out, "    on_complete: unreachable").expect("write unreachable");
                }
            }
        }
    }
    out
}

fn render_statement(statement: &StepStatement, indent_level: usize, out: &mut String) {
    let indent = "    ".repeat(indent_level);
    match statement {
        StepStatement::Action(action) => {
            writeln!(out, "{indent}action: {}", render_action(action)).expect("write action");
        }
        StepStatement::Effect(effect) => {
            writeln!(out, "{indent}effect: {}", render_effect(effect)).expect("write effect");
        }
        StepStatement::Wait(wait) => {
            writeln!(out, "{indent}wait: {}", render_wait(wait)).expect("write wait");
        }
        StepStatement::IfElse {
            condition,
            then_goto,
            else_goto,
        } => {
            writeln!(
                out,
                "{indent}if: {} goto {} else: goto {}",
                render_condition(condition),
                render_goto_target(then_goto),
                render_goto_target(else_goto)
            )
            .expect("write if_else");
        }
        StepStatement::Delay { duration_ms } => {
            writeln!(out, "{indent}delay: {}ms", duration_ms).expect("write delay");
        }
        StepStatement::Repeat { count, body } => {
            writeln!(out, "{indent}repeat {count}:").expect("write repeat");
            for nested in body {
                render_statement(nested, indent_level + 1, out);
            }
        }
        StepStatement::Timeout(timeout) => {
            writeln!(
                out,
                "{indent}timeout: {} -> goto {}",
                render_duration(&timeout.duration),
                render_goto_target(&timeout.target)
            )
            .expect("write timeout");
        }
        StepStatement::Goto(target) => {
            writeln!(out, "{indent}goto {}", render_goto_target(target)).expect("write goto");
        }
        StepStatement::Parallel(block) => render_parallel(block, indent_level, out),
        StepStatement::Race(block) => render_race(block, indent_level, out),
        StepStatement::AllowIndefiniteWait(value) => {
            writeln!(out, "{indent}allow_indefinite_wait: {value}").expect("write aiw");
        }
    }
}

fn render_parallel(block: &ParallelBlock, indent_level: usize, out: &mut String) {
    let indent = "    ".repeat(indent_level);
    writeln!(out, "{indent}parallel:").expect("write parallel");
    for (index, branch) in block.branches.iter().enumerate() {
        render_branch(
            &format!("branch_{}", index + 1),
            branch,
            indent_level + 1,
            out,
        );
    }
}

fn render_branch(name: &str, branch: &Branch, indent_level: usize, out: &mut String) {
    let indent = "    ".repeat(indent_level);
    writeln!(out, "{indent}{name}:").expect("write branch");
    for statement in &branch.statements {
        render_statement(statement, indent_level + 1, out);
    }
}

fn render_race(block: &RaceBlock, indent_level: usize, out: &mut String) {
    let indent = "    ".repeat(indent_level);
    writeln!(out, "{indent}race:").expect("write race");
    for (index, branch) in block.branches.iter().enumerate() {
        render_race_branch(
            &format!("branch_{}", index + 1),
            branch,
            indent_level + 1,
            out,
        );
    }
}

fn render_race_branch(name: &str, branch: &RaceBranch, indent_level: usize, out: &mut String) {
    let indent = "    ".repeat(indent_level);
    writeln!(out, "{indent}{name}:").expect("write race branch");
    for statement in &branch.statements {
        render_statement(statement, indent_level + 1, out);
    }
    if let Some(target) = &branch.then_goto {
        writeln!(
            out,
            "{}then: goto {}",
            "    ".repeat(indent_level + 1),
            render_goto_target(target)
        )
        .expect("write race then");
    }
}

fn render_action(action: &ActionStatement) -> String {
    match action {
        ActionStatement::Extend { target, .. } => format!("extend {target}"),
        ActionStatement::Retract { target, .. } => format!("retract {target}"),
        ActionStatement::Set { target, value } => format!("set {target} {value}"),
        ActionStatement::SetAnalog { target, value } => format!("set_analog {target} {value}"),
        ActionStatement::SetAnalogExpr { target, expr } => {
            format!("set_analog {target} {}", render_expression(expr))
        }
        ActionStatement::Compute { target, expr } => {
            format!("compute {target} = {}", render_expression(expr))
        }
        ActionStatement::Call {
            function,
            args,
            binding,
        } => format!(
            "call {function}({}) -> {}",
            args.iter()
                .map(render_expression)
                .collect::<Vec<_>>()
                .join(", "),
            render_binding(binding)
        ),
        ActionStatement::CamEngage { target } => format!("cam_engage {target}"),
        ActionStatement::CamDisengage { target } => format!("cam_disengage {target}"),
        ActionStatement::CamSwitch { target, new_table } => {
            format!("cam_switch {target} {new_table}")
        }
        ActionStatement::CamPhase { target, offset } => {
            format!("cam_phase {target} {}", render_expression(offset))
        }
        ActionStatement::AxisMoveRelative {
            target,
            params,
            distance,
            speed,
            acceleration,
            deceleration,
            timeout,
            on_reject,
            on_motion_fault,
            on_safety_fault,
            on_reject_routes,
            on_motion_fault_routes,
            on_safety_fault_routes,
            semantic_tag,
        } => render_axis_move(
            "axis.move_relative",
            target.to_string(),
            vec![
                Some(format!("distance: {distance}")),
                speed.map(|value| format!("speed: {value}")),
                acceleration.map(|value| format!("acc: {value}")),
                deceleration.map(|value| format!("dec: {value}")),
                params.as_ref().map(|value| format!("params: {value}")),
            ],
            timeout.as_ref(),
            on_reject.as_ref(),
            on_motion_fault.as_ref(),
            on_safety_fault.as_ref(),
            on_reject_routes
                .iter()
                .map(render_axis_route)
                .collect::<Vec<_>>(),
            on_motion_fault_routes
                .iter()
                .map(render_axis_route)
                .collect::<Vec<_>>(),
            on_safety_fault_routes
                .iter()
                .map(render_axis_route)
                .collect::<Vec<_>>(),
            semantic_tag.as_ref(),
        ),
        ActionStatement::AxisMoveAbsolute {
            target,
            params,
            position,
            speed,
            acceleration,
            deceleration,
            timeout,
            on_reject,
            on_motion_fault,
            on_safety_fault,
            on_reject_routes,
            on_motion_fault_routes,
            on_safety_fault_routes,
            semantic_tag,
        } => render_axis_move(
            "axis.move_absolute",
            target.to_string(),
            vec![
                Some(format!("position: {position}")),
                speed.map(|value| format!("speed: {value}")),
                acceleration.map(|value| format!("acc: {value}")),
                deceleration.map(|value| format!("dec: {value}")),
                params.as_ref().map(|value| format!("params: {value}")),
            ],
            timeout.as_ref(),
            on_reject.as_ref(),
            on_motion_fault.as_ref(),
            on_safety_fault.as_ref(),
            on_reject_routes
                .iter()
                .map(render_axis_route)
                .collect::<Vec<_>>(),
            on_motion_fault_routes
                .iter()
                .map(render_axis_route)
                .collect::<Vec<_>>(),
            on_safety_fault_routes
                .iter()
                .map(render_axis_route)
                .collect::<Vec<_>>(),
            semantic_tag.as_ref(),
        ),
        ActionStatement::Log { message } => format!("log \"{}\"", escape_string(message)),
    }
}

fn render_axis_move(
    kind: &str,
    target: String,
    args: Vec<Option<String>>,
    timeout: Option<&TimeoutDirective>,
    on_reject: Option<&GotoDirective>,
    on_motion_fault: Option<&GotoDirective>,
    on_safety_fault: Option<&GotoDirective>,
    on_reject_routes: Vec<String>,
    on_motion_fault_routes: Vec<String>,
    on_safety_fault_routes: Vec<String>,
    semantic_tag: Option<&String>,
) -> String {
    let mut out = format!(
        "{kind}({})",
        std::iter::once(target)
            .chain(args.into_iter().flatten())
            .collect::<Vec<_>>()
            .join(", ")
    );
    if let Some(timeout) = timeout {
        out.push_str(&format!(
            " timeout: {} -> {}",
            render_duration(&timeout.duration),
            render_goto_target(&timeout.target)
        ));
    }
    if let Some(target) = on_reject {
        out.push_str(&format!(" on_reject -> {}", render_goto_target(target)));
    }
    if let Some(target) = on_motion_fault {
        out.push_str(&format!(
            " on_motion_fault -> {}",
            render_goto_target(target)
        ));
    }
    if let Some(target) = on_safety_fault {
        out.push_str(&format!(
            " on_safety_fault -> {}",
            render_goto_target(target)
        ));
    }
    for route in on_reject_routes {
        out.push(' ');
        out.push_str(&route);
    }
    for route in on_motion_fault_routes {
        out.push(' ');
        out.push_str(&route);
    }
    for route in on_safety_fault_routes {
        out.push(' ');
        out.push_str(&route);
    }
    if let Some(tag) = semantic_tag {
        out.push_str(&format!(" semantic_tag: {tag}"));
    }
    out
}

fn render_axis_route(route: &crate::ast::AxisFaultRouteDirective) -> String {
    let label = match route.kind {
        Some(crate::ast::AxisFaultRouteKind::Reject) => "on_reject",
        Some(crate::ast::AxisFaultRouteKind::Motion) => "on_motion_fault",
        Some(crate::ast::AxisFaultRouteKind::Safety) => "on_safety_fault",
        Some(crate::ast::AxisFaultRouteKind::Vendor) => "on_motion_fault",
        None => "on_motion_fault",
    };
    let mut matcher = Vec::new();
    if let Some(kind) = &route.kind {
        matcher.push(format!("kind: {}", render_axis_route_kind(kind)));
    }
    if let Some(code) = route.code {
        matcher.push(format!("code: {code}"));
    }
    if matcher.is_empty() {
        format!("{label} -> {}", render_goto_target(&route.target))
    } else {
        format!(
            "{label}({}) -> {}",
            matcher.join(", "),
            render_goto_target(&route.target)
        )
    }
}

fn render_axis_route_kind(kind: &crate::ast::AxisFaultRouteKind) -> &'static str {
    match kind {
        crate::ast::AxisFaultRouteKind::Reject => "reject",
        crate::ast::AxisFaultRouteKind::Motion => "motion",
        crate::ast::AxisFaultRouteKind::Safety => "safety",
        crate::ast::AxisFaultRouteKind::Vendor => "vendor",
    }
}

fn render_binding(binding: &ExternCallBinding) -> String {
    match binding {
        ExternCallBinding::Single(target) => target.clone(),
        ExternCallBinding::Tuple(targets) => format!("({})", targets.join(", ")),
    }
}

fn render_effect(effect: &EffectStatement) -> String {
    match &effect.kind {
        EffectKind::Acquire { holder, from } => format!("acquire holder {holder} from {from}"),
        EffectKind::Transfer { from, to } => format!("transfer from {from} to {to}"),
        EffectKind::Finish { at, terminal_state } => {
            format!("finish workpiece at {at} as {terminal_state}")
        }
        EffectKind::Mount {
            workpiece_type,
            slot,
        } => format!("mount {workpiece_type} on {slot}"),
        EffectKind::Unmount {
            workpiece_type,
            slot,
            to,
        } => format!("unmount {workpiece_type} from {slot} to {to}"),
        EffectKind::Split {
            source_type,
            target_type,
            count,
            consumed,
        } => {
            let mut out = format!("split {source_type} into {target_type} count {count}");
            if *consumed {
                out.push_str(" consumed");
            }
            out
        }
        EffectKind::Merge {
            inputs,
            target_type,
            consumed_inputs,
        } => {
            let mut out = format!("merge [{}] into {target_type}", inputs.join(", "));
            if *consumed_inputs {
                out.push_str(" consumed_inputs");
            }
            out
        }
        EffectKind::TransformCarrier { carrier, frame } => {
            format!("transform carrier {carrier} to frame {frame}")
        }
    }
}

fn render_wait(wait: &WaitStatement) -> String {
    match &wait.condition {
        WaitCondition::Single(condition) => render_condition(condition),
        WaitCondition::And(conditions) => conditions
            .iter()
            .map(render_condition)
            .collect::<Vec<_>>()
            .join(" AND "),
        WaitCondition::Or(conditions) => conditions
            .iter()
            .map(render_condition)
            .collect::<Vec<_>>()
            .join(" OR "),
    }
}

fn render_condition(condition: &ConditionExpression) -> String {
    if let Some((left, right)) = condition.expression_pair() {
        return format!(
            "{} {} {}",
            render_expression(left),
            render_cmp_operator(&condition.operator),
            render_expression(right)
        );
    }
    format!(
        "{} {} {}",
        condition.left,
        render_cmp_operator(&condition.operator),
        render_literal(&condition.right)
    )
}

fn render_expression(expression: &Expression) -> String {
    match expression {
        Expression::Literal(value) => format!("{value}"),
        Expression::Boolean(value) => value.to_string(),
        Expression::Variable(value) => value.clone(),
        Expression::UnaryNeg(inner) => format!("-{}", render_expression(inner)),
        Expression::UnaryNot(inner) => format!("NOT {}", render_expression(inner)),
        Expression::BinaryOp { op, left, right } => format!(
            "{} {} {}",
            render_expression(left),
            render_binary_operator(op),
            render_expression(right)
        ),
        Expression::FunctionCall { name, args } => format!(
            "{name}({})",
            args.iter()
                .map(render_expression)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn render_binary_operator(operator: &BinaryOperator) -> &'static str {
    match operator {
        BinaryOperator::Add => "+",
        BinaryOperator::Sub => "-",
        BinaryOperator::Mul => "*",
        BinaryOperator::Div => "/",
        BinaryOperator::Mod => "%",
        BinaryOperator::Eq => "==",
        BinaryOperator::Neq => "!=",
        BinaryOperator::Gt => ">",
        BinaryOperator::Lt => "<",
        BinaryOperator::Gte => ">=",
        BinaryOperator::Lte => "<=",
        BinaryOperator::And => "AND",
        BinaryOperator::Or => "OR",
    }
}

fn render_cmp_operator(operator: &ComparisonOperator) -> &'static str {
    match operator {
        ComparisonOperator::Eq => "==",
        ComparisonOperator::Neq => "!=",
        ComparisonOperator::Gt => ">",
        ComparisonOperator::Lt => "<",
        ComparisonOperator::Gte => ">=",
        ComparisonOperator::Lte => "<=",
    }
}

fn render_literal(literal: &LiteralValue) -> String {
    match literal {
        LiteralValue::Boolean(value) => value.to_string(),
        LiteralValue::Number(value) => format!("{value}"),
        LiteralValue::Measured(measured) => format!("{}{}", measured.value, measured.unit),
        LiteralValue::String(value) => format!("\"{}\"", escape_string(value)),
        LiteralValue::State(state) => render_state_reference(state),
    }
}

fn render_state_reference(state: &StateReference) -> String {
    if state.port == "self" {
        format!("{}.{}", state.device, state.state)
    } else {
        format!("{}.{}.{}", state.device, state.port, state.state)
    }
}

fn render_duration(duration: &DurationValue) -> String {
    match duration.unit {
        TimeUnit::Ms => format!("{}ms", duration.value),
        TimeUnit::S => format!("{}s", duration.value),
    }
}

fn render_goto_target(target: &GotoDirective) -> String {
    match &target.step {
        Some(step) => format!("{}.{}", target.task, step),
        None => target.task.clone(),
    }
}

fn escape_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::{emit_optimized_plc, recheck_candidate_legality};
    use crate::ast::{Branch, ParallelBlock, StepStatement};
    use crate::parser::parse_plc;
    use crate::semantic::preprocess_program;

    #[test]
    fn passes_legal_candidate_back_through_existing_pipeline() {
        let source = r#"
[topology]

[constraints]

[tasks]

task main:
    step prep_a:
        action: set Y0 on
    step prep_b:
        action: set Y1 on
"#;

        let program = parse_plc(source).expect("parse");
        let expanded = preprocess_program(&program).expect("preprocess");
        let legality = recheck_candidate_legality(&expanded);
        assert!(legality.is_legal, "expected legal candidate");
        assert!(legality.diagnostics.is_empty());
    }

    #[test]
    fn rejects_candidate_via_existing_safety_verifier() {
        let source = r#"
[topology]

device Y0: digital_output
device Y1: digital_output

device valve_A: solenoid_valve { response_time: 15ms }
device valve_B: solenoid_valve { response_time: 15ms }

device cyl_A: cylinder { stroke_time: 200ms, retract_time: 180ms }
device cyl_B: cylinder { stroke_time: 250ms, retract_time: 220ms }

relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: Y1.out, to: valve_B.coil, via: driven_by }
relation { from: valve_B.out, to: cyl_B.cmd, via: driven_by }

[constraints]

safety: cyl_A.extended conflicts_with cyl_B.extended
    reason: "A缸和B缸同时伸出会导致机械碰撞"

[tasks]

task main:
    step serial_a:
        action: extend cyl_A
    step serial_b:
        action: extend cyl_B
"#;

        let program = parse_plc(source).expect("parse");
        let mut expanded = preprocess_program(&program).expect("preprocess");
        expanded.tasks.tasks[0].steps[0].statements =
            vec![StepStatement::Parallel(ParallelBlock {
                branches: vec![
                    Branch {
                        statements: vec![expanded.tasks.tasks[0].steps[0].statements[0].clone()],
                    },
                    Branch {
                        statements: vec![expanded.tasks.tasks[0].steps[1].statements[0].clone()],
                    },
                ],
            })];
        expanded.tasks.tasks[0].steps.remove(1);

        let legality = recheck_candidate_legality(&expanded);
        assert!(!legality.is_legal, "parallel conflict should be rejected");
        assert!(
            legality
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.contains("ERROR [safety]")),
            "expected safety verifier diagnostics"
        );
    }

    #[test]
    fn emits_tasks_section_while_preserving_prefix_sections() {
        let source = r#"[topology]

device Y0: digital_output

[constraints]

[tasks]

task main:
    step prep_a:
        action: set Y0 on
    step prep_b:
        action: set Y0 off
"#;
        let program = parse_plc(source).expect("parse");
        let expanded = preprocess_program(&program).expect("preprocess");
        let emitted = emit_optimized_plc(source, &expanded).expect("emit");

        assert!(emitted.contains("[topology]"));
        assert!(emitted.contains("[constraints]"));
        assert!(emitted.contains("[tasks]"));
        assert!(emitted.contains("task main:"));
        assert!(
            parse_plc(&emitted).is_ok(),
            "emitted PLC should remain parseable"
        );
    }
}
