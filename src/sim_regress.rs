use crate::parser::parse_plc;
use crate::runtime_bridge::state_machine_to_runtime_program;
use crate::scenario_resolve::resolve_scenario_yaml_for_plc;
use crate::semantic::{build_state_machine, build_topology_graph, preprocess_program};
use runtime_core::{Action, Instr, Program};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

fn render_minimized_scenario_yaml(
    scenario: &sim::Scenario,
    plc_path: &Path,
    scenario_path: &Path,
    failure: &Failure,
    seed: Option<u64>,
    has_sugar: bool,
) -> Result<String, String> {
    let mut msg = String::new();
    msg.push_str("# Minimized by `rust_plc sim-regress --minimize-failure`.\n");
    msg.push_str(&format!("# Source PLC: {}\n", plc_path.display()));
    msg.push_str(&format!("# Source scenario: {}\n", scenario_path.display()));
    msg.push_str(&format!(
        "# Failure signature: kind={} task={:?} step={:?} at_ms={:?}\n",
        failure.kind, failure.task, failure.step, failure.at_ms
    ));
    msg.push_str(&format!("# Seed: {:?}\n", seed));
    if has_sugar {
        msg.push_str("# Note: source scenario uses pulse/hold sugar; this file is the expanded numeric-ID form.\n");
        msg.push_str("#       Use `rust_plc scenario-expand <file.plc> --scenario <scenario.yaml> --out <expanded.yaml>` to inspect expansions.\n");
    }
    msg.push_str("#\n");
    msg.push_str("# Feedback (what to try next):\n");
    match failure.kind.as_str() {
        "timeout" => {
            msg.push_str("# - The PLC likely waited for an input edge that never happened.\n");
            msg.push_str("# - Try scripting the relevant sensors/guards over time, or extend `duration_ms`.\n");
            msg.push_str("# - If you're starting from scratch: `rust_plc scenario-init <file.plc> --preset normal`.\n");
        }
        _ => {
            msg.push_str("# - Review the failure message and script the missing input edges.\n");
        }
    }
    msg.push_str("#\n");

    let mut yaml = serde_yaml::to_string(scenario)
        .map_err(|e| format!("Failed to serialize minimized scenario YAML: {e}"))?;
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    msg.push_str(&yaml);
    Ok(msg)
}

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimized_scenario_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimization: Option<FailureMinimizationSummary>,
    pub failure: Failure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FailureMinimizationSummary {
    pub original_duration_ms: u64,
    pub minimized_duration_ms: u64,
    pub original_inputs: usize,
    pub minimized_inputs: usize,
    pub original_input_assignments: usize,
    pub minimized_input_assignments: usize,
    pub original_faults: usize,
    pub minimized_faults: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SimRegressSummary {
    pub total: usize,
    pub pass: usize,
    pub fail: usize,
    pub failures: Vec<SimRegressFailure>,
}

#[derive(Debug, Clone, Copy)]
pub struct SimRegressOptions {
    pub minimize: bool,
}

impl Default for SimRegressOptions {
    fn default() -> Self {
        Self { minimize: false }
    }
}

pub fn run_sim_regress(
    plc_dir: &Path,
    scenario_dir: &Path,
    artifacts_dir: &Path,
) -> Result<SimRegressSummary, String> {
    run_sim_regress_with_options(
        plc_dir,
        scenario_dir,
        artifacts_dir,
        SimRegressOptions::default(),
    )
}

pub fn run_sim_regress_with_options(
    plc_dir: &Path,
    scenario_dir: &Path,
    artifacts_dir: &Path,
    options: SimRegressOptions,
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

            let failure = run_one_case(plc_path, scenario_path, &artifact_dir, options)?;
            match failure {
                None => pass += 1,
                Some((
                    failure,
                    trace_path,
                    report_path,
                    seed,
                    minimized_scenario_path,
                    minimization,
                )) => {
                    fail += 1;
                    failures.push(SimRegressFailure {
                        plc: plc_path.display().to_string(),
                        scenario: scenario_path.display().to_string(),
                        seed,
                        artifact_dir: artifact_dir.display().to_string(),
                        trace_path: trace_path.map(|p| p.display().to_string()),
                        report_path: report_path.map(|p| p.display().to_string()),
                        minimized_scenario_path: minimized_scenario_path
                            .map(|p| p.display().to_string()),
                        minimization,
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
    options: SimRegressOptions,
) -> Result<
    Option<(
        Failure,
        Option<PathBuf>,
        Option<PathBuf>,
        Option<u64>,
        Option<PathBuf>,
        Option<FailureMinimizationSummary>,
    )>,
    String,
> {
    let plc_source = fs::read_to_string(plc_path)
        .map_err(|err| format!("Failed to read PLC file {}: {err}", plc_path.display()))?;

    let raw_scenario_yaml = fs::read_to_string(scenario_path).map_err(|err| {
        format!(
            "Failed to read scenario YAML file {}: {err}",
            scenario_path.display()
        )
    })?;

    let has_sugar = raw_scenario_yaml.contains("\npulse:")
        || raw_scenario_yaml.contains("\nhold:")
        || raw_scenario_yaml.contains("\npulses:")
        || raw_scenario_yaml.contains("\nholds:");

    let scenario_yaml = match resolve_scenario_yaml_for_plc(&plc_source, &raw_scenario_yaml) {
        Ok(yaml) => yaml,
        Err(e) => {
            let failure = Failure::scenario_error(e);
            let report_path = artifact_dir.join("report.json");
            write_failure_report_json(&report_path, &failure)?;
            return Ok(Some((failure, None, Some(report_path), None, None, None)));
        }
    };

    let scenario = match sim::Scenario::from_yaml_str(&scenario_yaml) {
        Ok(s) => s,
        Err(e) => {
            let failure = Failure::scenario_error(e.to_string());
            // Still write a report-like artifact so users can inspect failures uniformly.
            let report_path = artifact_dir.join("report.json");
            write_failure_report_json(&report_path, &failure)?;
            return Ok(Some((failure, None, Some(report_path), None, None, None)));
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
                None,
                None,
            )));
        }
    };

    let (num_di, num_do, num_ai, num_ao) = io_sizes_for_program_and_scenario(&program, &scenario);
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
                None,
                None,
            )));
        }
    };

    let trace_path = artifact_dir.join("trace.jsonl");
    fs::write(&trace_path, run.trace.into_string())
        .map_err(|err| format!("Failed to write trace file {trace_path:?}: {err}"))?;

    let report_path = artifact_dir.join("report.json");
    let mut report_json = serde_json::to_string_pretty(&run.report)
        .map_err(|err| format!("Failed to serialize report JSON: {err}"))?;
    report_json.push('\n');
    fs::write(&report_path, report_json)
        .map_err(|err| format!("Failed to write report file {report_path:?}: {err}"))?;

    if let Some(f) = run.report.failure {
        let failure = Failure::from_sim_failure(f);

        if options.minimize {
            let (min_scenario, min_run, min_summary) =
                minimize_failure_case(&program, &scenario, &failure)?;
            let minimized_scenario_path = artifact_dir.join("minimized_scenario.yaml");
            let minimized_trace_path = artifact_dir.join("minimized_trace.jsonl");
            let minimized_report_path = artifact_dir.join("minimized_report.json");

            let yaml = render_minimized_scenario_yaml(
                &min_scenario,
                plc_path,
                scenario_path,
                &failure,
                scenario.seed,
                has_sugar,
            )?;
            fs::write(&minimized_scenario_path, yaml)
                .map_err(|err| format!("Failed to write {minimized_scenario_path:?}: {err}"))?;
            fs::write(&minimized_trace_path, min_run.trace.into_string()).map_err(|err| {
                format!("Failed to write trace file {minimized_trace_path:?}: {err}")
            })?;
            let mut report_json = serde_json::to_string_pretty(&min_run.report)
                .map_err(|err| format!("Failed to serialize report JSON: {err}"))?;
            report_json.push('\n');
            fs::write(&minimized_report_path, report_json).map_err(|err| {
                format!("Failed to write report file {minimized_report_path:?}: {err}")
            })?;

            return Ok(Some((
                failure,
                Some(trace_path),
                Some(report_path),
                scenario.seed,
                Some(minimized_scenario_path),
                Some(min_summary),
            )));
        }

        return Ok(Some((
            failure,
            Some(trace_path),
            Some(report_path),
            scenario.seed,
            None,
            None,
        )));
    }

    Ok(None)
}

fn minimize_failure_case(
    program: &Program<'static>,
    scenario: &sim::Scenario,
    failure: &Failure,
) -> Result<(sim::Scenario, sim::SimRunOutput, FailureMinimizationSummary), String> {
    let target_signature = FailureSignature {
        kind: failure.kind.as_str(),
        task: failure.task,
        step: failure.step,
    };
    let original_duration_ms = scenario.duration_ms;
    let original_inputs = scenario.inputs.len();
    let original_input_assignments = input_assignment_count(scenario);
    let original_faults = scenario.faults.len();

    let (num_di, num_do, num_ai, num_ao) = io_sizes_for_program_and_scenario(program, scenario);

    let mut best = scenario.clone();

    // 1) Try to shrink duration using binary search while preserving failure signature.
    let max_ticks = best.duration_ticks().max(1);
    let mut hi_ticks = max_ticks;
    if let Some(at_ms) = failure.at_ms {
        let at_ticks = at_ms / best.tick_ms;
        hi_ticks = hi_ticks.min(at_ticks.saturating_add(1).max(1));
    }

    if scenario_matches_failure_signature(
        program,
        &scenario_with_ticks(&best, hi_ticks),
        &target_signature,
        num_di,
        num_do,
        num_ai,
        num_ao,
    )? {
        let mut lo = 1u64;
        let mut hi = hi_ticks;
        while lo < hi {
            let mid = lo + ((hi - lo) / 2);
            let cand = scenario_with_ticks(&best, mid);
            if scenario_matches_failure_signature(
                program,
                &cand,
                &target_signature,
                num_di,
                num_do,
                num_ai,
                num_ao,
            )? {
                hi = mid;
            } else {
                lo = mid.saturating_add(1);
            }
        }
        best = scenario_with_ticks(&best, lo);
    }

    // 2) Delta-debugging: remove whole input events if the failure persists.
    let mut i = 0usize;
    while i < best.inputs.len() {
        let mut cand = best.clone();
        cand.inputs.remove(i);
        if scenario_matches_failure_signature(
            program,
            &cand,
            &target_signature,
            num_di,
            num_do,
            num_ai,
            num_ao,
        )? {
            best = cand;
        } else {
            i += 1;
        }
    }

    // 3) Remove individual input assignments inside events (keeps timing but drops unrelated inputs).
    let mut ev_idx = 0usize;
    while ev_idx < best.inputs.len() {
        let mut event_changed = false;

        // Digital inputs
        let keys: Vec<u16> = best.inputs[ev_idx]
            .set
            .digital_inputs
            .keys()
            .copied()
            .collect();
        for k in keys {
            let mut cand = best.clone();
            cand.inputs[ev_idx].set.digital_inputs.remove(&k);
            if cand.inputs[ev_idx].set.digital_inputs.is_empty()
                && cand.inputs[ev_idx].set.analog_inputs.is_empty()
            {
                cand.inputs.remove(ev_idx);
            }
            if scenario_matches_failure_signature(
                program,
                &cand,
                &target_signature,
                num_di,
                num_do,
                num_ai,
                num_ao,
            )? {
                best = cand;
                event_changed = true;
                if ev_idx >= best.inputs.len() {
                    break;
                }
            }
        }

        // Analog inputs
        if ev_idx >= best.inputs.len() {
            break;
        }
        let keys: Vec<u16> = best.inputs[ev_idx]
            .set
            .analog_inputs
            .keys()
            .copied()
            .collect();
        for k in keys {
            let mut cand = best.clone();
            cand.inputs[ev_idx].set.analog_inputs.remove(&k);
            if cand.inputs[ev_idx].set.digital_inputs.is_empty()
                && cand.inputs[ev_idx].set.analog_inputs.is_empty()
            {
                cand.inputs.remove(ev_idx);
            }
            if scenario_matches_failure_signature(
                program,
                &cand,
                &target_signature,
                num_di,
                num_do,
                num_ai,
                num_ao,
            )? {
                best = cand;
                event_changed = true;
                if ev_idx >= best.inputs.len() {
                    break;
                }
            }
        }

        if !event_changed {
            ev_idx += 1;
        }
    }

    // 4) Remove faults (when failure persists without them, they were irrelevant noise).
    let mut fi = 0usize;
    while fi < best.faults.len() {
        let mut cand = best.clone();
        cand.faults.remove(fi);
        if scenario_matches_failure_signature(
            program,
            &cand,
            &target_signature,
            num_di,
            num_do,
            num_ai,
            num_ao,
        )? {
            best = cand;
        } else {
            fi += 1;
        }
    }

    // Final run artifacts for minimized scenario.
    let min_run = run_program_for_options(program, &best, num_di, num_do, num_ai, num_ao)?;
    if !run_matches_failure_signature(&min_run, &target_signature) {
        return Err("minimization produced a non-failing scenario; this is a bug".to_string());
    }

    Ok((
        best.clone(),
        min_run,
        FailureMinimizationSummary {
            original_duration_ms,
            minimized_duration_ms: best.duration_ms,
            original_inputs,
            minimized_inputs: best.inputs.len(),
            original_input_assignments,
            minimized_input_assignments: input_assignment_count(&best),
            original_faults,
            minimized_faults: best.faults.len(),
        },
    ))
}

#[derive(Debug, Clone, Copy)]
struct FailureSignature<'a> {
    kind: &'a str,
    task: Option<usize>,
    step: Option<u16>,
}

fn scenario_with_ticks(base: &sim::Scenario, ticks: u64) -> sim::Scenario {
    let mut out = base.clone();
    out.duration_ms = ticks.saturating_mul(out.tick_ms);

    // When shrinking duration, drop any scripted changes that would become invalid
    // (Scenario::apply_to_simio requires at_ms < duration_ms).
    let dur = out.duration_ms;
    out.inputs.retain(|ev| ev.at_ms < dur);
    out.faults.retain(|f| f.sensor_stuck.at_ms < dur);
    out.digital_bursts.retain(|b| {
        if b.at_ms >= dur {
            return false;
        }
        // Conservative check: ensure the final active window ends before duration.
        if b.count == 0 || b.period_ms == 0 {
            return false;
        }
        let last_start = b.at_ms.saturating_add(
            b.period_ms
                .saturating_mul((b.count.saturating_sub(1)) as u64),
        );
        let last_end = last_start.saturating_add(b.active_ms);
        last_end < dur
    });
    out
}

fn run_matches_failure_signature(
    run: &sim::SimRunOutput,
    signature: &FailureSignature<'_>,
) -> bool {
    let Some(f) = run.report.failure.as_ref() else {
        return false;
    };
    if f.kind != signature.kind {
        return false;
    }
    if let Some(task) = signature.task {
        if f.task != task {
            return false;
        }
    }
    if let Some(step) = signature.step {
        if f.step != step {
            return false;
        }
    }
    true
}

fn scenario_matches_failure_signature(
    program: &Program<'static>,
    scenario: &sim::Scenario,
    target_signature: &FailureSignature<'_>,
    num_di: usize,
    num_do: usize,
    num_ai: usize,
    num_ao: usize,
) -> Result<bool, String> {
    let run = run_program_for_options(program, scenario, num_di, num_do, num_ai, num_ao)?;
    Ok(run_matches_failure_signature(&run, target_signature))
}

fn input_assignment_count(scenario: &sim::Scenario) -> usize {
    scenario
        .inputs
        .iter()
        .map(|ev| ev.set.digital_inputs.len() + ev.set.analog_inputs.len())
        .sum()
}

fn run_program_for_options(
    program: &Program<'static>,
    scenario: &sim::Scenario,
    num_di: usize,
    num_do: usize,
    num_ai: usize,
    num_ao: usize,
) -> Result<sim::SimRunOutput, String> {
    let mut io = sim::SimIo::new(num_di, num_do, num_ai, num_ao);
    sim::run_program_for_scenario(program, scenario, &mut io)
        .map_err(|e| format!("Simulation failed: {e}"))
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
    let expanded = preprocess_program(&program).map_err(|errors| {
        errors
            .into_iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    })?;

    let topology = build_topology_graph(&expanded).map_err(|errors| {
        errors
            .into_iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    let sm = build_state_machine(&expanded).map_err(|errors| {
        errors
            .into_iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    })?;

    state_machine_to_runtime_program(&topology, &sm, tick_ms).map_err(|e| e.to_string())
}

fn io_sizes_for_program_and_scenario(
    program: &Program<'_>,
    scenario: &sim::Scenario,
) -> (usize, usize, usize, usize) {
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
                Instr::WaitAnalog { id, .. } => {
                    max_ai = Some(max_ai.map_or(id.0, |m| m.max(id.0)));
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
                            Action::SetAnalogExpr { id, .. } => {
                                max_ao = Some(max_ao.map_or(id.0, |m| m.max(id.0)));
                            }
                            Action::Compute { .. } | Action::CallExtern { .. } => {}
                            Action::CamEngage { .. }
                            | Action::CamDisengage { .. }
                            | Action::CamSwitch { .. }
                            | Action::CamPhase { .. } => {}
                            Action::Log { .. } => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }
    for cam in program.cam_configs {
        max_ai = Some(max_ai.map_or(cam.master_input.0, |m| m.max(cam.master_input.0)));
        max_ai = Some(max_ai.map_or(cam.slave_feedback.0, |m| m.max(cam.slave_feedback.0)));
        max_ao = Some(max_ao.map_or(cam.slave_output.0, |m| m.max(cam.slave_output.0)));
    }
    for pid in program.pid_loops {
        max_ai = Some(max_ai.map_or(pid.pv.0, |m| m.max(pid.pv.0)));
        max_ao = Some(max_ao.map_or(pid.out.0, |m| m.max(pid.out.0)));
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
            let entry = entry.map_err(|err| format!("Failed to read directory entry: {err}"))?;
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
