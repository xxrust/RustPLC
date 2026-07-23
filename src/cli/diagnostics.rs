use crate::cli_support::common::{CliOutputMode, DispatchResult, display_path_relative_to_cwd};
use crate::cli_support::diagnostics_common::evidence_source_label;
use crate::cli_support::help::command_usage;
use crate::cli_support::plc_pipeline::build_loaded_runtime_semantics;
use crate::cli_support::scenario_yaml::{
    format_resolve_scenario_yaml_error, parse_scenario_yaml, read_scenario_yaml_file,
};
use rust_plc::diagnostics::{
    DiagnosisAnchor, DiagnosisCandidate, DiagnosisInput, DiagnosisReport, EvidenceInputKind,
    EvidenceSource, IoSnapshotArtifact, diagnose,
};
use rust_plc::intent_alignment::{
    BindingStability, IntentDoctorReport, IntentDoctorRuntimeTaskLayout,
    compile_expected_behavior_spec, diagnose_intent_alignment_with_layouts, read_intent_contract,
};
use rust_plc::runtime_bridge::state_machine_to_runtime_program;
use rust_plc::scenario_resolve::resolve_scenario_yaml_for_plc;
use rust_plc::source_bundle::load_plc_source;
use rust_plc::tick_timing::parse_tick_timing_jsonl;
use rust_plc::timing_report::{TimingReport, build_timing_report};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn try_dispatch(
    program: &str,
    command: &str,
    remaining: &[String],
) -> Option<DispatchResult> {
    let (error_prefix, result) = match command {
        "trace-diff" => (
            None,
            run_trace_diff_subcommand(program, remaining.iter().cloned()),
        ),
        "trace-doctor" => (
            Some("[AXF-000]"),
            run_trace_doctor_subcommand(program, remaining.iter().cloned()),
        ),
        "intent-doctor" => (
            Some("[IAD-000]"),
            run_intent_doctor_subcommand(program, remaining.iter().cloned()),
        ),
        "timing-report" => (
            None,
            run_timing_report_subcommand(program, remaining.iter().cloned()),
        ),
        "io-map-normalize" => (
            None,
            run_io_map_normalize_subcommand(program, remaining.iter().cloned()),
        ),
        _ => return None,
    };
    Some(DispatchResult {
        error_prefix,
        result,
    })
}

#[derive(Debug, Serialize)]
struct TraceDoctorSummary {
    anchor_count: usize,
    candidate_count: usize,
    top_issue_code: Option<String>,
    top_confidence: Option<f64>,
}

#[derive(Debug, Serialize)]
struct TraceDoctorArtifacts {
    plc: String,
    scenario: String,
    trace: Option<String>,
    diff: Option<String>,
    timing_report: Option<String>,
    io_snapshot: Option<String>,
}

#[derive(Debug, Serialize)]
struct TraceDoctorJsonReport {
    schema_version: u32,
    command: &'static str,
    output: &'static str,
    evidence_source: EvidenceSource,
    evidence_inputs: Vec<EvidenceInputKind>,
    anchors: Vec<DiagnosisAnchor>,
    candidates: Vec<DiagnosisCandidate>,
    summary: TraceDoctorSummary,
    artifacts: TraceDoctorArtifacts,
}

fn parse_evidence_source(raw: &str) -> Option<EvidenceSource> {
    match raw {
        "no_board" => Some(EvidenceSource::NoBoard),
        "hil_board" => Some(EvidenceSource::HilBoard),
        "runtime_live" => Some(EvidenceSource::RuntimeLive),
        "mixed" => Some(EvidenceSource::Mixed),
        _ => None,
    }
}

fn diagnosis_category_label(category: rust_plc::diagnostics::DiagnosisCategory) -> &'static str {
    match category {
        rust_plc::diagnostics::DiagnosisCategory::ExpectedInputNeverChanged => {
            "expected_input_never_changed"
        }
        rust_plc::diagnostics::DiagnosisCategory::ActuatorCommandMissing => {
            "actuator_command_missing"
        }
        rust_plc::diagnostics::DiagnosisCategory::InterlockOrRequiresBlocked => {
            "interlock_or_requires_blocked"
        }
        rust_plc::diagnostics::DiagnosisCategory::MappingOrAliasMismatch => {
            "mapping_or_alias_mismatch"
        }
        rust_plc::diagnostics::DiagnosisCategory::TimeoutBudgetTooShort => {
            "timeout_budget_too_short"
        }
    }
}

fn build_trace_doctor_json_report(
    diagnosis: DiagnosisReport,
    output_mode: CliOutputMode,
    evidence_source: EvidenceSource,
    artifacts: TraceDoctorArtifacts,
) -> TraceDoctorJsonReport {
    let anchor_count = diagnosis.anchors.len();
    let candidate_count = diagnosis.candidates.len();
    let top = diagnosis.candidates.first();
    let top_issue_code = top.map(|candidate| candidate.issue_code.clone());
    let top_confidence = top.map(|candidate| candidate.confidence);
    TraceDoctorJsonReport {
        schema_version: diagnosis.schema_version,
        command: "trace-doctor",
        output: output_mode.as_str(),
        evidence_source,
        evidence_inputs: diagnosis.evidence_inputs,
        anchors: diagnosis.anchors,
        candidates: diagnosis.candidates,
        summary: TraceDoctorSummary {
            anchor_count,
            candidate_count,
            top_issue_code,
            top_confidence,
        },
        artifacts,
    }
}

fn print_trace_doctor_human(report: &TraceDoctorJsonReport, top_n: usize) {
    eprintln!(
        "trace-doctor: PASS (evidence_source={}, anchors={}, candidates={})",
        evidence_source_label(report.evidence_source),
        report.summary.anchor_count,
        report.summary.candidate_count
    );
    eprintln!("  plc: {}", report.artifacts.plc);
    eprintln!("  scenario: {}", report.artifacts.scenario);
    if let Some(trace) = &report.artifacts.trace {
        eprintln!("  trace: {trace}");
    }
    if let Some(diff) = &report.artifacts.diff {
        eprintln!("  diff: {diff}");
    }
    if let Some(timing_report) = &report.artifacts.timing_report {
        eprintln!("  timing_report: {timing_report}");
    }
    if let Some(io_snapshot) = &report.artifacts.io_snapshot {
        eprintln!("  io_snapshot: {io_snapshot}");
    }

    let top_n = top_n.max(1).min(report.candidates.len());
    eprintln!("Top {top_n} candidate(s):");
    for candidate in report.candidates.iter().take(top_n) {
        eprintln!(
            "  {}. [{}] {} (confidence={:.3})",
            candidate.rank,
            candidate.issue_code,
            diagnosis_category_label(candidate.category),
            candidate.confidence
        );
        if let Some(first_evidence) = candidate.evidence.first() {
            eprintln!("     evidence: {first_evidence}");
        }
        if let Some(location) = &candidate.source_location {
            eprintln!(
                "     source: {}:{}:{}",
                location.file, location.line, location.column
            );
        }
        eprintln!("     next: {}", candidate.suggested_fix);
    }
}

fn run_trace_diff_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "trace-diff");
    let mut sil: Option<PathBuf> = None;
    let mut board: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut context_window: usize = 3;
    let mut fail_on_mismatch = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--sil" => {
                sil = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --sil <trace.jsonl>".to_string()
                })?));
            }
            "--board" => {
                board = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --board <trace.jsonl>".to_string()
                })?));
            }
            "--out" => {
                out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --out <report.json>".to_string()
                })?));
            }
            "--context" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --context <n>".to_string())?;
                context_window = raw
                    .parse::<usize>()
                    .map_err(|_| format!("Invalid --context value (expected usize): {raw}"))?;
            }
            "--fail-on-mismatch" => fail_on_mismatch = true,
            "-h" | "--help" => return Err(usage.clone()),
            other => return Err(format!("Unknown argument for trace-diff: {other}")),
        }
    }

    let sil = sil.ok_or_else(|| usage.clone())?;
    let board = board.ok_or_else(|| usage.clone())?;
    let out = out.ok_or_else(|| usage.clone())?;

    let sil_text = fs::read_to_string(&sil)
        .map_err(|err| format!("Failed to read SIL trace {sil:?}: {err}"))?;
    let board_text = fs::read_to_string(&board)
        .map_err(|err| format!("Failed to read board trace {board:?}: {err}"))?;

    let sil_events = rust_plc::trace_diff::parse_trace_jsonl(&sil_text)
        .map_err(|err| format!("Failed to parse SIL trace JSONL: {err}"))?;
    let board_events = rust_plc::trace_diff::parse_trace_jsonl(&board_text)
        .map_err(|err| format!("Failed to parse board trace JSONL: {err}"))?;

    let report = rust_plc::trace_diff::diff_traces(&sil_events, &board_events, context_window);

    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to create output dir {parent:?}: {err}"))?;
        }
    }

    let mut json = serde_json::to_string_pretty(&report)
        .map_err(|err| format!("Failed to serialize report JSON: {err}"))?;
    json.push('\n');
    fs::write(&out, json).map_err(|err| format!("Failed to write {out:?}: {err}"))?;

    if fail_on_mismatch && !report.is_match {
        return Err(format!(
            "Trace mismatch detected (tick={:?}, type={:?}); see report {:?}",
            report.first_mismatch_tick, report.mismatch_type, out
        ));
    }
    Ok(())
}

fn run_trace_doctor_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "trace-doctor");

    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut scenario_path: Option<PathBuf> = None;
    let mut trace_path: Option<PathBuf> = None;
    let mut diff_path: Option<PathBuf> = None;
    let mut timing_report_path: Option<PathBuf> = None;
    let mut io_snapshot_path: Option<PathBuf> = None;
    let mut evidence_source = EvidenceSource::Mixed;
    let mut top_n: usize = 3;
    let mut output_mode = CliOutputMode::Human;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenario" => {
                scenario_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --scenario <scenario.yaml>".to_string()
                    })?));
            }
            "--trace" => {
                trace_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --trace <trace.jsonl>".to_string()
                    })?));
            }
            "--diff" => {
                diff_path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --diff <diff_report.json>".to_string()
                })?));
            }
            "--timing-report" => {
                timing_report_path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --timing-report <timing_report.json>".to_string()
                })?));
            }
            "--io-snapshot" => {
                io_snapshot_path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --io-snapshot <io_snapshot.json>".to_string()
                })?));
            }
            "--evidence-source" => {
                let raw = args.next().ok_or_else(|| {
                    "Missing value for --evidence-source <no_board|hil_board|runtime_live|mixed>"
                        .to_string()
                })?;
                evidence_source = parse_evidence_source(&raw).ok_or_else(|| {
                    format!(
                        "Invalid --evidence-source value `{raw}` (expected `no_board`, `hil_board`, `runtime_live`, or `mixed`)"
                    )
                })?;
            }
            "--top" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --top <n>".to_string())?;
                top_n = raw
                    .parse::<usize>()
                    .map_err(|_| format!("Invalid --top value (expected usize): {raw}"))?;
                if top_n == 0 {
                    return Err("Invalid --top value (expected >= 1)".to_string());
                }
            }
            "--output" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --output <human|json>".to_string())?;
                output_mode = CliOutputMode::parse(&raw).ok_or_else(|| {
                    format!("Invalid --output value `{raw}` (expected `human` or `json`)")
                })?;
            }
            "-h" | "--help" => return Err(usage.clone()),
            other => return Err(format!("Unknown argument for trace-doctor: {other}")),
        }
    }

    let scenario_path = scenario_path.ok_or_else(|| usage.clone())?;
    if trace_path.is_none() && diff_path.is_none() {
        return Err(
            "trace-doctor requires at least one input artifact: --trace <trace.jsonl> or --diff <diff_report.json>"
                .to_string(),
        );
    }

    let plc_source =
        fs::read_to_string(&plc_path).map_err(|err| format!("Failed to read {plc_path}: {err}"))?;
    let scenario_yaml = read_scenario_yaml_file(&scenario_path)?;
    let scenario_yaml =
        resolve_scenario_yaml_for_plc(&plc_source, &scenario_yaml).map_err(|e| {
            format_resolve_scenario_yaml_error(&plc_path, &scenario_path, "trace-doctor", &e)
        })?;
    let scenario = parse_scenario_yaml(&scenario_yaml)?;

    let trace_events = if let Some(path) = &trace_path {
        let text = fs::read_to_string(path)
            .map_err(|err| format!("Failed to read trace JSONL {}: {err}", path.display()))?;
        Some(
            rust_plc::trace_diff::parse_trace_jsonl(&text)
                .map_err(|err| format!("Failed to parse trace JSONL {}: {err}", path.display()))?,
        )
    } else {
        None
    };

    let diff_report = if let Some(path) = &diff_path {
        let text = fs::read_to_string(path)
            .map_err(|err| format!("Failed to read diff report {}: {err}", path.display()))?;
        Some(
            serde_json::from_str::<rust_plc::trace_diff::TraceDiffReport>(&text).map_err(
                |err| format!("Failed to parse diff report JSON {}: {err}", path.display()),
            )?,
        )
    } else {
        None
    };

    let timing_report = if let Some(path) = &timing_report_path {
        let text = fs::read_to_string(path)
            .map_err(|err| format!("Failed to read timing report {}: {err}", path.display()))?;
        Some(serde_json::from_str::<TimingReport>(&text).map_err(|err| {
            format!(
                "Failed to parse timing report JSON {}: {err}",
                path.display()
            )
        })?)
    } else {
        None
    };

    let io_snapshot = if let Some(path) = &io_snapshot_path {
        let text = fs::read_to_string(path)
            .map_err(|err| format!("Failed to read io-snapshot {}: {err}", path.display()))?;
        Some(
            serde_json::from_str::<IoSnapshotArtifact>(&text).map_err(|err| {
                format!("Failed to parse io-snapshot JSON {}: {err}", path.display())
            })?,
        )
    } else {
        None
    };

    let diagnosis = diagnose(DiagnosisInput {
        plc_source: &plc_source,
        scenario: &scenario,
        trace_events: trace_events.as_deref(),
        diff_report: diff_report.as_ref(),
        timing_report: timing_report.as_ref(),
        evidence_source,
        io_snapshot: io_snapshot.as_ref(),
    })
    .map_err(|err| format!("trace-doctor failed: {err}"))?;

    let report = build_trace_doctor_json_report(
        diagnosis,
        output_mode,
        evidence_source,
        TraceDoctorArtifacts {
            plc: display_path_relative_to_cwd(Path::new(&plc_path)),
            scenario: display_path_relative_to_cwd(&scenario_path),
            trace: trace_path
                .as_ref()
                .map(|path| display_path_relative_to_cwd(path)),
            diff: diff_path
                .as_ref()
                .map(|path| display_path_relative_to_cwd(path)),
            timing_report: timing_report_path
                .as_ref()
                .map(|path| display_path_relative_to_cwd(path)),
            io_snapshot: io_snapshot_path
                .as_ref()
                .map(|path| display_path_relative_to_cwd(path)),
        },
    );

    if output_mode == CliOutputMode::Json {
        let mut json = serde_json::to_string_pretty(&report)
            .map_err(|err| format!("Failed to serialize trace-doctor JSON output: {err}"))?;
        json.push('\n');
        print!("{json}");
    } else {
        print_trace_doctor_human(&report, top_n);
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct IntentDoctorSummary {
    observed_transition_count: usize,
    unique_transition_count: usize,
    candidate_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    top_transition: Option<String>,
}

#[derive(Debug, Serialize)]
struct IntentDoctorArtifacts {
    plc: String,
    trace: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    intent_contract: Option<String>,
}

#[derive(Debug, Serialize)]
struct IntentDoctorCliReport {
    schema_version: u32,
    command: &'static str,
    output: &'static str,
    analysis: IntentDoctorReport,
    summary: IntentDoctorSummary,
    artifacts: IntentDoctorArtifacts,
}

fn build_intent_doctor_cli_report(
    report: IntentDoctorReport,
    output_mode: CliOutputMode,
    artifacts: IntentDoctorArtifacts,
) -> IntentDoctorCliReport {
    let top_transition = report
        .candidates
        .first()
        .map(|candidate| candidate.transition.clone());
    IntentDoctorCliReport {
        schema_version: report.schema_version,
        command: "intent-doctor",
        output: output_mode.as_str(),
        summary: IntentDoctorSummary {
            observed_transition_count: report.observed_transition_count,
            unique_transition_count: report.unique_transition_count,
            candidate_count: report.candidates.len(),
            top_transition,
        },
        analysis: report,
        artifacts,
    }
}

fn binding_stability_label(status: BindingStability) -> &'static str {
    match status {
        BindingStability::Stable => "stable",
        BindingStability::Partial => "partial",
        BindingStability::Repeated => "repeated",
        BindingStability::Missing => "missing",
        BindingStability::Unsupported => "unsupported",
    }
}

fn print_intent_doctor_human(report: &IntentDoctorCliReport, top_n: usize) {
    eprintln!(
        "intent-doctor: PASS (observed={}, unique={}, candidates={})",
        report.summary.observed_transition_count,
        report.summary.unique_transition_count,
        report.summary.candidate_count
    );
    eprintln!("  plc: {}", report.artifacts.plc);
    eprintln!("  trace: {}", report.artifacts.trace);
    if let Some(contract) = &report.artifacts.intent_contract {
        eprintln!("  intent_contract: {contract}");
    }

    let top_n = top_n.max(1).min(report.analysis.candidates.len());
    eprintln!("Top {top_n} anchor candidate(s):");
    for candidate in report.analysis.candidates.iter().take(top_n) {
        eprintln!(
            "  {}. {} (score={:.2}, count={})",
            candidate.rank, candidate.transition, candidate.score, candidate.occurrence_count
        );
        eprintln!(
            "     states: {} -> {}",
            candidate.from_state, candidate.to_state
        );
        if !candidate.workpiece_effects.is_empty() {
            eprintln!(
                "     workpiece: {}",
                candidate.workpiece_effects.join(" | ")
            );
        }
        if let Some(reason) = candidate.reasons.first() {
            eprintln!("     why: {reason}");
        }
    }

    if let Some(contract) = &report.analysis.contract_diagnosis {
        eprintln!(
            "Contract bindings: {} ({})",
            contract.contract_id, contract.contract_version
        );
        if let Some(blocked_reason) = &contract.blocked_reason {
            eprintln!("  blocked: {blocked_reason}");
        }
        for binding in &contract.milestone_bindings {
            eprintln!(
                "  {} [{}] observed={}/{}",
                binding.subject,
                binding_stability_label(binding.status),
                binding.observed_occurrences,
                binding.expected_occurrences
            );
        }
    }

    if let Some(cycle) = &report.analysis.cycle_diagnosis {
        eprintln!(
            "Cycle diagnosis: cycles={}, cross_cycle_ready={}, trailing_partial_cycle={}",
            cycle.observed_cycle_count, cycle.cross_cycle_ready, cycle.trailing_partial_cycle
        );
        for note in &cycle.notes {
            eprintln!("  note: {note}");
        }
    }
}

fn default_intent_contract_path(plc_path: &Path) -> Option<PathBuf> {
    let file_name = plc_path.file_name()?.to_str()?;
    let candidate_name = if file_name.ends_with(".bundle.toml") {
        format!(
            "{}.intent_alignment.contract.json",
            file_name.trim_end_matches(".toml")
        )
    } else if file_name.ends_with(".plc") {
        format!(
            "{}.intent_alignment.contract.json",
            file_name.trim_end_matches(".plc")
        )
    } else {
        return None;
    };
    let sibling = plc_path.with_file_name(candidate_name);
    if sibling.is_file() {
        return Some(sibling);
    }

    let plc_dir = plc_path.parent()?;
    if plc_dir.file_name()?.to_str()? != "plc" {
        return None;
    }
    let docs_dir = plc_dir.parent()?.join("docs");
    let mut matches = fs::read_dir(&docs_dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.ends_with(".intent_alignment.contract.json"))
                .unwrap_or(false)
        });
    let first = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(first)
}

fn runtime_layouts_from_program(
    program: &runtime_core::Program<'_>,
) -> Result<Vec<IntentDoctorRuntimeTaskLayout>, String> {
    program
        .tasks
        .iter()
        .map(|task| {
            let step_keys = task
                .steps
                .iter()
                .map(|step| {
                    let Some((task_name, step_name)) = step.name.split_once('.') else {
                        return Err(format!(
                            "runtime step `{}` is missing the expected `task.step` naming form",
                            step.name
                        ));
                    };
                    Ok((task_name.to_string(), step_name.to_string()))
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(IntentDoctorRuntimeTaskLayout {
                root_task: task.name.to_string(),
                step_keys,
            })
        })
        .collect()
}

fn run_intent_doctor_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "intent-doctor");
    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut trace_path: Option<PathBuf> = None;
    let mut intent_contract_path: Option<PathBuf> = None;
    let mut top_n: usize = 5;
    let mut output_mode = CliOutputMode::Human;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--trace" => {
                trace_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --trace <trace.jsonl>".to_string()
                    })?));
            }
            "--intent-contract" => {
                intent_contract_path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --intent-contract <contract.json>".to_string()
                })?));
            }
            "--top" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --top <n>".to_string())?;
                top_n = raw
                    .parse::<usize>()
                    .map_err(|_| format!("Invalid --top value (expected usize): {raw}"))?;
                if top_n == 0 {
                    return Err("Invalid --top value (expected >= 1)".to_string());
                }
            }
            "--output" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --output <human|json>".to_string())?;
                output_mode = CliOutputMode::parse(&raw).ok_or_else(|| {
                    format!("Invalid --output value `{raw}` (expected `human` or `json`)")
                })?;
            }
            "-h" | "--help" => return Err(usage.clone()),
            other => return Err(format!("Unknown argument for intent-doctor: {other}")),
        }
    }

    let plc_path = PathBuf::from(plc_path);
    let trace_path = trace_path.ok_or_else(|| usage.clone())?;
    let loaded = load_plc_source(&plc_path)?;
    let semantics = build_loaded_runtime_semantics(&loaded)
        .map_err(|err| format!("intent-doctor failed to compile PLC semantics: {err}"))?;
    let runtime_program = state_machine_to_runtime_program(
        &semantics.topology,
        &semantics.constraints,
        &semantics.state_machine,
        1,
    )
    .map_err(|err| format!("intent-doctor failed to lower runtime layout: {err}"))?;
    let runtime_layouts = runtime_layouts_from_program(runtime_program.program())?;
    let trace_text = fs::read_to_string(&trace_path)
        .map_err(|err| format!("Failed to read trace JSONL {}: {err}", trace_path.display()))?;
    let trace_events = rust_plc::trace_diff::parse_trace_jsonl(&trace_text).map_err(|err| {
        format!(
            "Failed to parse trace JSONL {}: {err}",
            trace_path.display()
        )
    })?;

    let resolved_contract_path = intent_contract_path.or_else(|| {
        default_intent_contract_path(&plc_path).filter(|candidate| candidate.is_file())
    });
    let expected_behavior = if let Some(contract_path) = &resolved_contract_path {
        let contract = read_intent_contract(contract_path).map_err(|err| {
            format!(
                "Failed to load intent contract {}: {err}",
                contract_path.display()
            )
        })?;
        Some(
            compile_expected_behavior_spec(&contract)
                .map_err(|err| format!("Failed to compile intent contract: {err}"))?,
        )
    } else {
        None
    };

    let report = diagnose_intent_alignment_with_layouts(
        &semantics.state_machine,
        &trace_events,
        expected_behavior.as_ref(),
        &runtime_layouts,
    )
    .map_err(|err| format!("intent-doctor failed: {err}"))?;
    let cli_report = build_intent_doctor_cli_report(
        report,
        output_mode,
        IntentDoctorArtifacts {
            plc: display_path_relative_to_cwd(&plc_path),
            trace: display_path_relative_to_cwd(&trace_path),
            intent_contract: resolved_contract_path
                .as_ref()
                .map(|path| display_path_relative_to_cwd(path)),
        },
    );

    if output_mode == CliOutputMode::Json {
        let mut json = serde_json::to_string_pretty(&cli_report)
            .map_err(|err| format!("Failed to serialize intent-doctor JSON output: {err}"))?;
        json.push('\n');
        print!("{json}");
    } else {
        print_intent_doctor_human(&cli_report, top_n);
    }
    Ok(())
}

fn run_timing_report_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "timing-report");
    let mut input: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--in" => {
                input = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --in <tick_timing.jsonl>".to_string()
                })?));
            }
            "--out" => {
                out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --out <timing_report.json>".to_string()
                })?));
            }
            "-h" | "--help" => return Err(usage.clone()),
            other => return Err(format!("Unknown argument for timing-report: {other}")),
        }
    }

    let input = input.ok_or_else(|| usage.clone())?;
    let out = out.unwrap_or_else(|| {
        input
            .parent()
            .map(|path| path.join("timing_report.json"))
            .unwrap_or_else(|| PathBuf::from("timing_report.json"))
    });

    let text = fs::read_to_string(&input)
        .map_err(|err| format!("Failed to read timing input {input:?}: {err}"))?;
    let rows = parse_tick_timing_jsonl(&text)
        .map_err(|err| format!("Failed to parse tick_timing JSONL: {err}"))?;
    let report = build_timing_report(&rows)
        .ok_or_else(|| "tick_timing.jsonl is empty; cannot build timing report".to_string())?;

    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to create output dir {parent:?}: {err}"))?;
        }
    }

    let mut json = serde_json::to_string_pretty(&report)
        .map_err(|err| format!("Failed to serialize timing report JSON: {err}"))?;
    json.push('\n');
    fs::write(&out, json).map_err(|err| format!("Failed to write timing report {out:?}: {err}"))?;

    eprintln!(
        "timing-report: count={} overrun_count={} exec_us[min/p50/p95/p99/max/mean]={}/{}/{}/{}/{}/{:.2}",
        report.count,
        report.overrun_count,
        report.exec_us_min,
        report.exec_us_p50,
        report.exec_us_p95,
        report.exec_us_p99,
        report.exec_us_max,
        report.exec_us_mean
    );
    eprintln!("  timing_report: {}", out.display());
    Ok(())
}

fn run_io_map_normalize_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "io-map-normalize");
    let mut input: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--in" => {
                input =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --in <io_map.toml>".to_string()
                    })?));
            }
            "--out" => {
                out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --out <normalized.toml>".to_string()
                })?));
            }
            "-h" | "--help" => return Err(usage.clone()),
            other => return Err(format!("Unknown argument for io-map-normalize: {other}")),
        }
    }

    let input = input.ok_or_else(|| usage.clone())?;
    let out = out.ok_or_else(|| usage.clone())?;

    let text =
        fs::read_to_string(&input).map_err(|err| format!("Failed to read {input:?}: {err}"))?;
    let value: toml::Value =
        toml::from_str(&text).map_err(|err| format!("TOML parse error: {err}"))?;
    let normalized = normalize_io_map_toml(&value)?;
    let mut out_text = toml::to_string_pretty(&normalized)
        .map_err(|err| format!("TOML serialize error: {err}"))?;
    if !out_text.ends_with('\n') {
        out_text.push('\n');
    }

    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to create output dir {parent:?}: {err}"))?;
        }
    }
    fs::write(&out, out_text).map_err(|err| format!("Failed to write {out:?}: {err}"))?;
    Ok(())
}

fn normalize_io_map_toml(value: &toml::Value) -> Result<toml::Value, String> {
    use rust_plc::iec_address::{LogicalChannelKind, parse_iec_address};
    use toml::value::Table;

    let root = value
        .as_table()
        .ok_or_else(|| "io_map.toml must be a TOML table at the root".to_string())?;
    let mut out_root: Table = root.clone();

    fn section_table<'a>(root: &'a Table, name: &str) -> Result<&'a Table, String> {
        root.get(name)
            .and_then(|value| value.as_table())
            .ok_or_else(|| format!("Missing or invalid [{name}] (expected a table)"))
    }

    fn opt_section_table<'a>(root: &'a Table, name: &str) -> Result<Option<&'a Table>, String> {
        match root.get(name) {
            None => Ok(None),
            Some(value) => value
                .as_table()
                .map(Some)
                .ok_or_else(|| format!("Invalid [{name}] (expected a table)")),
        }
    }

    fn parse_native_key(key: &str, expected_prefix: &str) -> Option<u16> {
        let rest = key.strip_prefix(expected_prefix)?;
        if rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        rest.parse::<u16>().ok()
    }

    fn parse_gpio_int(section: &str, key: &str, value: &toml::Value) -> Result<i64, String> {
        value.as_integer().ok_or_else(|| {
            format!(
                "Invalid value for {key:?} in [{section}] (expected integer gpio), got {value:?}"
            )
        })
    }

    fn normalize_section(
        section: &str,
        expected_native_prefix: &'static str,
        expected_kind: LogicalChannelKind,
        table: &Table,
    ) -> Result<Table, String> {
        use std::collections::BTreeMap;
        let mut by_id: BTreeMap<u16, i64> = BTreeMap::new();

        for (key, value) in table.iter() {
            let gpio = parse_gpio_int(section, key, value)?;

            let (kind, id) = if let Some(id) = parse_native_key(key, expected_native_prefix) {
                (expected_kind, id)
            } else if key.trim_start().starts_with('%') {
                let parsed = parse_iec_address(key).map_err(|e| e.to_string())?;
                (parsed.kind, parsed.id)
            } else {
                return Err(format!(
                    "Invalid key {key:?} in [{section}] (expected {expected_native_prefix}<n> or a quoted IEC key like \"%IX0.0\")"
                ));
            };

            if kind != expected_kind {
                return Err(format!(
                    "Invalid key {key:?} in [{section}] (IEC kind {:?} does not match section kind {:?})",
                    kind, expected_kind
                ));
            }

            if let Some(prev) = by_id.insert(id, gpio) {
                if prev != gpio {
                    return Err(format!(
                        "Conflict for {expected_native_prefix}{id} in [{section}]: {prev} vs {gpio}"
                    ));
                }
            }
        }

        let mut out = Table::new();
        for (id, gpio) in by_id {
            out.insert(
                format!("{expected_native_prefix}{id}"),
                toml::Value::Integer(gpio),
            );
        }
        Ok(out)
    }

    let di = section_table(root, "digital_inputs")?;
    let do_ = section_table(root, "digital_outputs")?;
    let ai = opt_section_table(root, "analog_inputs")?;
    let ao = opt_section_table(root, "analog_outputs")?;

    out_root.insert(
        "digital_inputs".to_string(),
        toml::Value::Table(normalize_section(
            "digital_inputs",
            "di",
            LogicalChannelKind::DigitalInput,
            di,
        )?),
    );
    out_root.insert(
        "digital_outputs".to_string(),
        toml::Value::Table(normalize_section(
            "digital_outputs",
            "do",
            LogicalChannelKind::DigitalOutput,
            do_,
        )?),
    );
    if let Some(ai) = ai {
        out_root.insert(
            "analog_inputs".to_string(),
            toml::Value::Table(normalize_section(
                "analog_inputs",
                "ai",
                LogicalChannelKind::AnalogInput,
                ai,
            )?),
        );
    }
    if let Some(ao) = ao {
        out_root.insert(
            "analog_outputs".to_string(),
            toml::Value::Table(normalize_section(
                "analog_outputs",
                "ao",
                LogicalChannelKind::AnalogOutput,
                ao,
            )?),
        );
    }

    Ok(toml::Value::Table(out_root))
}
