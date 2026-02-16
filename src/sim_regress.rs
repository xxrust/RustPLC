use crate::parser::parse_plc;
use crate::runtime_bridge::state_machine_to_runtime_program;
use crate::semantic::{build_state_machine, build_topology_graph, preprocess_program};
use runtime_core::{Action, Instr, Program};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Failure {
    pub kind: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<u16>,
}

impl Failure {
    fn compile_error(message: String) -> Self {
        Self {
            kind: "compile_error".to_string(),
            message,
            at_ms: None,
            task: None,
            step: None,
        }
    }

    fn scenario_error(message: String) -> Self {
        Self {
            kind: "scenario_error".to_string(),
            message,
            at_ms: None,
            task: None,
            step: None,
        }
    }

    fn runtime_error(message: String) -> Self {
        Self {
            kind: "runtime_error".to_string(),
            message,
            at_ms: None,
            task: None,
            step: None,
        }
    }

    fn from_sim_failure(f: sim::SimFailure) -> Self {
        Self {
            kind: f.kind,
            message: f.message,
            at_ms: Some(f.at_ms),
            task: Some(f.task),
            step: Some(f.step),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SimRegressFailure {
    pub plc: String,
    pub scenario: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    pub artifact_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_path: Option<String>,
    pub failure: Failure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SimRegressSummary {
    pub total: usize,
    pub pass: usize,
    pub fail: usize,
    pub failures: Vec<SimRegressFailure>,
}

pub fn run_sim_regress(
    plc_dir: &Path,
    scenario_dir: &Path,
    artifacts_dir: &Path,
) -> Result<SimRegressSummary, String> {
    let mut plc_files = collect_files_recursive(plc_dir, &["plc"])?;
    let mut scenario_files = collect_files_recursive(scenario_dir, &["yaml", "yml"])?;

    plc_files.sort();
    scenario_files.sort();

    if plc_files.is_empty() {
        return Err(format!(
            "No .plc files found under directory: {}",
            plc_dir.display()
        ));
    }
    if scenario_files.is_empty() {
        return Err(format!(
            "No scenario .yaml/.yml files found under directory: {}",
            scenario_dir.display()
        ));
    }

    fs::create_dir_all(artifacts_dir)
        .map_err(|err| format!("Failed to create artifacts dir {artifacts_dir:?}: {err}"))?;

    let mut total = 0usize;
    let mut pass = 0usize;
    let mut fail = 0usize;
    let mut failures = Vec::<SimRegressFailure>::new();

    let mut case_idx = 0usize;
    for plc_path in &plc_files {
        for scenario_path in &scenario_files {
            total += 1;
            let artifact_dir = artifacts_dir.join(format!("case_{case_idx:04}"));
            case_idx += 1;
            fs::create_dir_all(&artifact_dir).map_err(|err| {
                format!("Failed to create case artifact dir {artifact_dir:?}: {err}")
            })?;

            let failure = run_one_case(plc_path, scenario_path, &artifact_dir)?;
            match failure {
                None => pass += 1,
                Some((failure, trace_path, report_path, seed)) => {
                    fail += 1;
                    failures.push(SimRegressFailure {
                        plc: plc_path.display().to_string(),
                        scenario: scenario_path.display().to_string(),
                        seed,
                        artifact_dir: artifact_dir.display().to_string(),
                        trace_path: trace_path.map(|p| p.display().to_string()),
                        report_path: report_path.map(|p| p.display().to_string()),
                        failure,
                    });
                }
            }
        }
    }

    Ok(SimRegressSummary {
        total,
        pass,
        fail,
        failures,
    })
}

fn run_one_case(
    plc_path: &Path,
    scenario_path: &Path,
    artifact_dir: &Path,
) -> Result<Option<(Failure, Option<PathBuf>, Option<PathBuf>, Option<u64>)>, String> {
    let plc_source = fs::read_to_string(plc_path).map_err(|err| {
        format!("Failed to read PLC file {}: {err}", plc_path.display())
    })?;

    let scenario_yaml = fs::read_to_string(scenario_path).map_err(|err| {
        format!(
            "Failed to read scenario YAML file {}: {err}",
            scenario_path.display()
        )
    })?;

    let scenario = match sim::Scenario::from_yaml_str(&scenario_yaml) {
        Ok(s) => s,
        Err(e) => {
            let failure = Failure::scenario_error(e.to_string());
            // Still write a report-like artifact so users can inspect failures uniformly.
            let report_path = artifact_dir.join("report.json");
            write_failure_report_json(&report_path, &failure)?;
            return Ok(Some((failure, None, Some(report_path), None)));
        }
    };

    let program = match compile_plc_to_runtime_program(&plc_source, scenario.tick_ms) {
        Ok(p) => p,
        Err(msg) => {
            let failure = Failure::compile_error(msg);
            let report_path = artifact_dir.join("report.json");
            write_failure_report_json(&report_path, &failure)?;
            return Ok(Some((
                failure,
                None,
                Some(report_path),
                scenario.seed,
            )));
        }
    };

    let (num_di, num_do, num_ai, num_ao) =
        io_sizes_for_program_and_scenario(&program, &scenario);
    let mut io = sim::SimIo::new(num_di, num_do, num_ai, num_ao);

    let run = match sim::run_program_for_scenario(&program, &scenario, &mut io) {
        Ok(run) => run,
        Err(e) => {
            let failure = Failure::runtime_error(e.to_string());
            let report_path = artifact_dir.join("report.json");
            write_failure_report_json(&report_path, &failure)?;
            return Ok(Some((
                failure,
                None,
                Some(report_path),
                scenario.seed,
            )));
        }
    };

    let trace_path = artifact_dir.join("trace.jsonl");
    fs::write(&trace_path, run.trace.into_string()).map_err(|err| {
        format!("Failed to write trace file {trace_path:?}: {err}")
    })?;

    let report_path = artifact_dir.join("report.json");
    let mut report_json = serde_json::to_string_pretty(&run.report)
        .map_err(|err| format!("Failed to serialize report JSON: {err}"))?;
    report_json.push('\n');
    fs::write(&report_path, report_json)
        .map_err(|err| format!("Failed to write report file {report_path:?}: {err}"))?;

    if let Some(f) = run.report.failure {
        return Ok(Some((
            Failure::from_sim_failure(f),
            Some(trace_path),
            Some(report_path),
            scenario.seed,
        )));
    }

    Ok(None)
}

fn write_failure_report_json(report_path: &Path, failure: &Failure) -> Result<(), String> {
    #[derive(Serialize)]
    struct FailureOnlyReport<'a> {
        failure: &'a Failure,
    }
    let mut json = serde_json::to_string_pretty(&FailureOnlyReport { failure })
        .map_err(|err| format!("Failed to serialize failure JSON: {err}"))?;
    json.push('\n');
    fs::write(report_path, json)
        .map_err(|err| format!("Failed to write report file {report_path:?}: {err}"))?;
    Ok(())
}

fn compile_plc_to_runtime_program(
    plc_source: &str,
    tick_ms: u64,
) -> Result<Program<'static>, String> {
    let program = parse_plc(plc_source).map_err(|e| e.to_string())?;
    let expanded = preprocess_program(&program)
        .map_err(|errors| errors.into_iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n"))?;

    let topology = build_topology_graph(&expanded)
        .map_err(|errors| errors.into_iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n"))?;
    let sm = build_state_machine(&expanded)
        .map_err(|errors| errors.into_iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n"))?;

    state_machine_to_runtime_program(&topology, &sm, tick_ms).map_err(|e| e.to_string())
}

fn io_sizes_for_program_and_scenario(program: &Program<'_>, scenario: &sim::Scenario) -> (usize, usize, usize, usize) {
    let mut max_di: Option<u16> = None;
    let mut max_do: Option<u16> = None;
    let mut max_ai: Option<u16> = None;
    let mut max_ao: Option<u16> = None;

    for task in program.tasks {
        for step in task.steps {
            match step.instr {
                Instr::WaitDigital { id, .. } => {
                    max_di = Some(max_di.map_or(id.0, |m| m.max(id.0)));
                }
                Instr::Action { actions, .. } => {
                    for a in actions {
                        match *a {
                            Action::SetDigital { id, .. }
                            | Action::Extend { output: id }
                            | Action::Retract { output: id } => {
                                max_do = Some(max_do.map_or(id.0, |m| m.max(id.0)));
                            }
                            Action::SetAnalog { id, .. } => {
                                max_ao = Some(max_ao.map_or(id.0, |m| m.max(id.0)));
                            }
                            Action::Log { .. } => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // Scenario inputs/faults may reference additional inputs beyond what the program reads.
    for ev in &scenario.inputs {
        for (&id, _) in &ev.set.digital_inputs {
            max_di = Some(max_di.map_or(id, |m| m.max(id)));
        }
        for (&id, _) in &ev.set.analog_inputs {
            max_ai = Some(max_ai.map_or(id, |m| m.max(id)));
        }
    }
    for f in &scenario.faults {
        let id = f.sensor_stuck.target;
        max_di = Some(max_di.map_or(id, |m| m.max(id)));
    }

    let num_di = max_di.map(|m| m as usize + 1).unwrap_or(0).max(1);
    let num_do = max_do.map(|m| m as usize + 1).unwrap_or(0).max(1);
    let num_ai = max_ai.map(|m| m as usize + 1).unwrap_or(0);
    let num_ao = max_ao.map(|m| m as usize + 1).unwrap_or(0);
    (num_di, num_do, num_ai, num_ao)
}

fn collect_files_recursive(root: &Path, exts: &[&str]) -> Result<Vec<PathBuf>, String> {
    if !root.exists() {
        return Err(format!("Directory does not exist: {}", root.display()));
    }

    let mut out = Vec::<PathBuf>::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir)
            .map_err(|err| format!("Failed to read directory {}: {err}", dir.display()))?;
        for entry in entries {
            let entry =
                entry.map_err(|err| format!("Failed to read directory entry: {err}"))?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if exts.iter().any(|wanted| ext.eq_ignore_ascii_case(wanted)) {
                out.push(path);
            }
        }
    }
    Ok(out)
}
