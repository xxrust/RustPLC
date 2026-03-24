use core::fmt;

use runtime_core::{Instr, Program, Runtime, RuntimeError, TransitionReason};

use crate::{
    JsonlTraceRecorder, Scenario, ScenarioError, ScenarioSummary, SimFailure, SimIo, SimReport,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimRunError {
    Scenario(ScenarioError),
    Runtime(RuntimeError),
}

impl fmt::Display for SimRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SimRunError::Scenario(e) => write!(f, "{e}"),
            SimRunError::Runtime(e) => match e {
                RuntimeError::TooManyTransitionsInOneTick {
                    task,
                    attempted,
                    per_task_cap,
                    active_tasks,
                } => {
                    write!(
                        f,
                        "runtime error: TooManyTransitionsInOneTick(task={task}, attempted={attempted}, per_task_cap={per_task_cap}, active_tasks={active_tasks})\n\
hint: a same-tick transition loop likely occurred (e.g. a task completes immediately and re-enters within the same tick).\n\
common causes:\n\
  - scenario sets start_button/sensors to values that make waits/guards satisfied instantly\n\
  - start_button is held true, so `ready` -> `init` loops without time advancing\n\
fix:\n\
  - pulse start_button (set true then false)\n\
  - script sensor edges over time (set false at t=0, then set true at later `at_ms`)\n"
                    )
                }
                other => write!(f, "runtime error: {other:?}"),
            },
        }
    }
}

impl std::error::Error for SimRunError {}

impl From<ScenarioError> for SimRunError {
    fn from(value: ScenarioError) -> Self {
        SimRunError::Scenario(value)
    }
}

impl From<RuntimeError> for SimRunError {
    fn from(value: RuntimeError) -> Self {
        SimRunError::Runtime(value)
    }
}

#[derive(Debug)]
pub struct SimRunOutput {
    pub trace: JsonlTraceRecorder,
    pub report: SimReport,
}

pub fn run_program_for_scenario<'a>(
    program: &'a Program<'a>,
    scenario: &Scenario,
    io: &mut SimIo,
) -> Result<SimRunOutput, SimRunError> {
    run_program_for_scenario_with_tick_observer(program, scenario, io, |_| {})
}

pub fn run_program_for_scenario_with_tick_observer<'a>(
    program: &'a Program<'a>,
    scenario: &Scenario,
    io: &mut SimIo,
    mut on_tick_start: impl FnMut(&SimIo),
) -> Result<SimRunOutput, SimRunError> {
    scenario.apply_to_simio(io)?;

    let mut rt = Runtime::new(program)?;
    let mut trace = JsonlTraceRecorder::new();

    let mut failure: Option<SimFailure> = None;
    for _ in 0..scenario.duration_ticks() {
        on_tick_start(io);
        rt.tick_with_trace(io, |e| {
            trace.record(e);
            if failure.is_none() && e.reason == TransitionReason::Timeout {
                failure = Some(SimFailure {
                    kind: "timeout".to_string(),
                    message: format!(
                        "timeout transition at task {} step {} -> {}",
                        e.task, e.from.0, e.to.0
                    ),
                    at_ms: e.tick.0.saturating_mul(scenario.tick_ms),
                    task: e.task,
                    step: e.from.0,
                });
            }
        })?;

        if is_halted(&rt, program) {
            break;
        }
    }

    Ok(SimRunOutput {
        trace,
        report: SimReport {
            seed: scenario.seed,
            scenario: ScenarioSummary::from_scenario(scenario),
            failure,
        },
    })
}

fn is_halted<'a>(rt: &Runtime<'a>, program: &'a Program<'a>) -> bool {
    let loc = rt.location();
    let Ok(task) = program.task(loc.task) else {
        return false;
    };
    let Some(step) = task.step(loc.step) else {
        return false;
    };
    matches!(step.instr, Instr::Halt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use io_traits::DigitalInputId;
    use runtime_core::{Instr, Program, Step, StepId, Task, Timeout};

    #[test]
    fn sensor_stuck_causes_timeout_and_report_includes_step_and_time() {
        static STEPS: [Step<'static>; 3] = [
            Step {
                name: "wait_di0_true",
                instr: Instr::WaitDigital {
                    id: DigitalInputId(0),
                    equals: true,
                    next: StepId(1),
                    timeout: Some(Timeout {
                        after_ticks: 2,
                        target: StepId(2),
                    }),
                },
            },
            Step {
                name: "ok_halt",
                instr: Instr::Halt,
            },
            Step {
                name: "timeout_halt",
                instr: Instr::Halt,
            },
        ];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &[],
            workpiece_sites: &[],
            workpiece_holders: &[],
        };

        let yaml = r#"
seed: 123
tick_ms: 10
duration_ms: 50
inputs:
  - at_ms: 10
    set:
      digital_inputs:
        0: true
faults:
  - sensor_stuck:
      at_ms: 0
      target: 0
      value: false
"#;
        let scenario = Scenario::from_yaml_str(yaml).unwrap();
        let mut io = SimIo::new(1, 0, 0, 0);

        let out = run_program_for_scenario(&PROGRAM, &scenario, &mut io).unwrap();

        let failure = out.report.failure.expect("expected a timeout failure");
        assert_eq!(failure.kind, "timeout");
        assert_eq!(failure.task, 0);
        assert_eq!(failure.step, 0);
        assert_eq!(failure.at_ms, 20);

        // Ensure the trace contains a timeout transition at tick 2.
        assert!(
            out.trace
                .lines()
                .iter()
                .any(|l| l.contains("\"reason\":\"timeout\"") && l.contains("\"tick\":2")),
            "expected timeout event in trace, got: {:?}",
            out.trace.lines()
        );
    }
}
