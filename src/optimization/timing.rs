use crate::ast::PlcProgram;
use crate::optimization::{CandidateTimingSummary, render_plc_errors, summarize_program_timing};
use crate::semantic::{build_state_machine, build_topology_graph};
use crate::verification::timing::estimate_program_timing;

pub fn evaluate_candidate_timing(
    program: &PlcProgram,
) -> Result<CandidateTimingSummary, Vec<String>> {
    let topology = build_topology_graph(program).map_err(render_plc_errors)?;
    let state_machine = build_state_machine(program).map_err(render_plc_errors)?;
    let estimate = estimate_program_timing(program, &topology, &state_machine);
    Ok(summarize_program_timing(&estimate))
}

#[cfg(test)]
mod tests {
    use super::evaluate_candidate_timing;
    use crate::parser::parse_plc;
    use crate::semantic::preprocess_program;

    #[test]
    fn evaluates_nominal_and_worst_case_from_existing_timing_rules() {
        let source = r#"
[topology]

device plc_main: plc { model_ref: openplc_softplc }

device valve_A: solenoid_valve { response_time: 10ms }
device valve_B: solenoid_valve { response_time: 10ms }

device cyl_A: cylinder { stroke_time: 200ms, retract_time: 180ms }
device cyl_B: cylinder { stroke_time: 250ms, retract_time: 220ms }

relation { from: plc_main.Y0, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: plc_main.Y1, to: valve_B.coil, via: driven_by }
relation { from: valve_B.out, to: cyl_B.cmd, via: driven_by }

[constraints]

[tasks]

task loader:
    step clamp:
        action: extend cyl_A
        delay: 20ms

task unloader:
    step clamp:
        action: extend cyl_B
        delay: 50ms
"#;

        let program = parse_plc(source).expect("parse");
        let expanded = preprocess_program(&program).expect("preprocess");
        let timing = evaluate_candidate_timing(&expanded).expect("timing");

        assert_eq!(timing.active_tasks.len(), 2);
        assert!(timing.global_nominal_ms > 0);
        assert!(timing.global_worst_case_ms >= timing.global_nominal_ms);
        assert!(timing.sequential_nominal_ms >= timing.global_nominal_ms);
        assert!(timing.sequential_worst_case_ms >= timing.global_worst_case_ms);
    }
}
