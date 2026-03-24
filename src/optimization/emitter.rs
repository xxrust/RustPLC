use crate::ast::PlcProgram;
use crate::optimization::{CandidateLegality, OptimizationLegalityError, render_plc_errors};
use crate::semantic::{
    build_constraint_set, build_state_machine, build_topology_graph, preprocess_program,
};
use crate::verification::verify_all;

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

pub fn emit_optimized_plc(_original_source: &str, _program: &PlcProgram) -> Result<String, String> {
    Err("not implemented".to_string())
}

fn recheck_program(program: &PlcProgram) -> Result<(), OptimizationLegalityError> {
    let expanded =
        preprocess_program(program).map_err(|errors| OptimizationLegalityError::Semantic(render_plc_errors(errors)))?;
    let topology = build_topology_graph(&expanded)
        .map_err(|errors| OptimizationLegalityError::Semantic(render_plc_errors(errors)))?;
    let constraints = build_constraint_set(&expanded)
        .map_err(|errors| OptimizationLegalityError::Semantic(render_plc_errors(errors)))?;
    let state_machine = build_state_machine(&expanded)
        .map_err(|errors| OptimizationLegalityError::Semantic(render_plc_errors(errors)))?;
    verify_all(&expanded, &topology, &constraints, &state_machine)
        .map_err(|errors| OptimizationLegalityError::Verification(
            errors.into_iter().map(|error| error.to_string()).collect(),
        ))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::recheck_candidate_legality;
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
        expanded.tasks.tasks[0].steps[0].statements = vec![StepStatement::Parallel(ParallelBlock {
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
}
