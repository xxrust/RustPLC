use crate::cli_support::common::{
    CliOutputMode, DispatchResult, display_path_relative_to_cwd, write_json_pretty,
};
use crate::cli_support::help::command_usage;
use crate::cli_support::plc_pipeline::{
    compile_loaded_codegen_semantics, format_loaded_plc_errors,
    parse_loaded_plc_with_required_purpose,
};
use rust_plc::codegen::st::{StCodegenConfig, StCodegenError, generate_st};
use rust_plc::geometry_view::{GeometryArtifact, export_geometry_artifact};
use rust_plc::intent_alignment::{
    IntentAlignmentBlockerKind, IntentAlignmentVerdict, IntentContract, IntentMismatchKind,
    compare_trace_jsonl, compile_expected_behavior_spec, read_intent_contract,
    reduce_intent_alignment_report, verify_intent_contract_delivery_readiness,
    verify_intent_contract_source_binding,
};
use rust_plc::process_operation::{ProcessOperationModel, build_process_operation_model};
use rust_plc::process_operation::{
    ProcessOperationRefinementIssue, RefinementStatus, read_process_operation_model,
    verify_process_operation_refinement,
};
use rust_plc::semantic::{
    build_constraint_set, build_state_machine, preprocess_program, preprocess_program_with_library,
};
use rust_plc::sequence_lint::{
    CriticalWaitExemption, LintLevel, SequenceLintConfig, lint_critical_wait_recovery,
};
use rust_plc::source_bundle::{is_supported_plc_source_path, load_plc_source};
use rust_plc::state_proof::{
    StateProofIssue, StateProofSeverity, StateProofStatus, analyze_program,
    load_state_proof_config, should_auto_run_state_proof_check,
};
use rust_plc::trace_diff::parse_trace_jsonl;
use serde::Serialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) fn try_dispatch(
    program: &str,
    command: &str,
    remaining: &[String],
) -> Option<DispatchResult> {
    let (error_prefix, result) = match command {
        "geometry-export" => (
            Some("[GEOM-000]"),
            run_geometry_export_subcommand(program, remaining.iter().cloned()),
        ),
        "gen-st" => (
            Some("[STGEN-000]"),
            run_gen_st_subcommand(program, remaining.iter().cloned()),
        ),
        "sequence-lint" => (
            None,
            run_sequence_lint_subcommand(program, remaining.iter().cloned()),
        ),
        "operation-model" => (
            Some("[OPMODEL-000]"),
            run_operation_model_subcommand(program, remaining.iter().cloned()),
        ),
        "process-model-check" => (
            Some("[OPMODEL-010]"),
            run_process_model_check_subcommand(program, remaining.iter().cloned()),
        ),
        "state-proof-check" => (
            Some("[SPF-000]"),
            run_state_proof_check_subcommand(program, remaining.iter().cloned()),
        ),
        "project-check" => (
            Some("[PROJCHECK-000]"),
            run_project_check_subcommand(program, remaining.iter().cloned()),
        ),
        _ => return None,
    };
    Some(DispatchResult {
        error_prefix,
        result,
    })
}

#[derive(Debug, Serialize)]
struct OperationModelExportReport {
    schema_version: u32,
    command: &'static str,
    source_plc: String,
    output_path: String,
    operation_count: usize,
    diagnostic_count: usize,
}

#[derive(Debug, Serialize)]
struct ProcessModelCheckCliReport {
    schema_version: u32,
    command: &'static str,
    output: &'static str,
    source_plc: String,
    model_path: String,
    status: RefinementStatus,
    expected_operation_count: usize,
    actual_operation_count: usize,
    issue_count: usize,
    issues: Vec<ProcessOperationRefinementIssue>,
}

#[derive(Debug, Serialize)]
struct StateProofCheckCliReport {
    schema_version: u32,
    command: &'static str,
    output: &'static str,
    source_plc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    config_path: Option<String>,
    status: StateProofStatus,
    issue_count: usize,
    issues: Vec<StateProofIssue>,
}

fn run_operation_model_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "operation-model");

    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut out_path: Option<PathBuf> = None;
    let mut output_mode = CliOutputMode::Human;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => {
                let value = args.next().ok_or_else(|| {
                    "Missing value for --out <process_operation_model.toml|json>".to_string()
                })?;
                out_path = Some(PathBuf::from(value));
            }
            "--output" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --output <human|json>".to_string())?;
                output_mode = CliOutputMode::parse(&raw).ok_or_else(|| {
                    format!("Invalid value for --output `{raw}` (expected human or json)")
                })?;
            }
            "-h" | "--help" => {
                return Err(usage.clone());
            }
            other => {
                return Err(format!(
                    "Unknown argument for operation-model: {other}\n{usage}"
                ));
            }
        }
    }

    let out_path = out_path.ok_or_else(|| {
        "operation-model requires --out <process_operation_model.toml|json>".to_string()
    })?;

    if !is_supported_plc_source_path(Path::new(&plc_path)) {
        return Err(format!(
            "operation-model expects a supported PLC source path, got: {plc_path}"
        ));
    }

    let loaded = load_plc_source(Path::new(&plc_path))?;
    let model = build_process_operation_model_for_loaded_source(&loaded)?;
    write_operation_model(&out_path, &model)?;

    let report = OperationModelExportReport {
        schema_version: 1,
        command: "operation-model",
        source_plc: plc_path.clone(),
        output_path: display_path_relative_to_cwd(&out_path),
        operation_count: model.operations.len(),
        diagnostic_count: model.diagnostics.len(),
    };

    match output_mode {
        CliOutputMode::Json => {
            let mut body = serde_json::to_string_pretty(&report)
                .map_err(|err| format!("Failed to serialize operation-model JSON: {err}"))?;
            body.push('\n');
            print!("{body}");
        }
        CliOutputMode::Human => {
            eprintln!("operation-model: PASS");
            eprintln!("  source: {}", report.source_plc);
            eprintln!("  out: {}", report.output_path);
            eprintln!("  operations: {}", report.operation_count);
            eprintln!("  diagnostics: {}", report.diagnostic_count);
        }
    }

    Ok(())
}

fn build_process_operation_model_for_loaded_source(
    loaded: &rust_plc::source_bundle::LoadedPlcSource,
) -> Result<ProcessOperationModel, String> {
    let parsed = parse_loaded_plc_with_required_purpose(loaded)?;
    let devices_dir = Path::new("devices");
    let device_library =
        rust_plc::device_library::DeviceLibrary::load(devices_dir).map_err(|errors| {
            errors
                .into_iter()
                .map(|err| err.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        })?;
    let expanded = preprocess_program_with_library(
        &parsed,
        if device_library.is_empty() {
            None
        } else {
            Some(&device_library)
        },
    )
    .map_err(|errors| format_loaded_plc_errors(errors, loaded).join("\n"))?;
    let state_machine = build_state_machine(&expanded)
        .map_err(|errors| format_loaded_plc_errors(errors, loaded).join("\n"))?;
    let constraints = build_constraint_set(&expanded)
        .map_err(|errors| format_loaded_plc_errors(errors, loaded).join("\n"))?;
    Ok(build_process_operation_model(&state_machine, &constraints))
}

fn default_process_model_path(plc_path: &Path) -> Option<PathBuf> {
    let parent = plc_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = plc_path.file_name()?.to_str()?;
    if file_name.ends_with(".bundle.toml") {
        return Some(
            parent
                .join("process_model")
                .join("process_operation_model.toml"),
        );
    }
    if plc_path.extension().and_then(|ext| ext.to_str()) == Some("plc") {
        if parent.file_name().and_then(|name| name.to_str()) == Some("plc") {
            return parent.parent().map(|root| {
                root.join("process_model")
                    .join("process_operation_model.toml")
            });
        }
        return Some(
            parent
                .join("process_model")
                .join("process_operation_model.toml"),
        );
    }
    None
}

fn run_process_model_check_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "process-model-check");

    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut model_path: Option<PathBuf> = None;
    let mut output_mode = CliOutputMode::Human;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--model" => {
                model_path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --model <process_operation_model.toml|json>".to_string()
                })?));
            }
            "--output" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --output <human|json>".to_string())?;
                output_mode = CliOutputMode::parse(&raw).ok_or_else(|| {
                    format!("Invalid value for --output `{raw}` (expected human or json)")
                })?;
            }
            "-h" | "--help" => return Err(usage.clone()),
            other => {
                return Err(format!(
                    "Unknown argument for process-model-check: {other}\n{usage}"
                ));
            }
        }
    }

    if !is_supported_plc_source_path(Path::new(&plc_path)) {
        return Err(format!(
            "process-model-check expects a supported PLC source path, got: {plc_path}"
        ));
    }

    let plc_path_buf = PathBuf::from(&plc_path);
    let model_path = model_path
        .or_else(|| default_process_model_path(&plc_path_buf))
        .ok_or_else(|| {
            "process-model-check requires --model <process_operation_model.toml|json>".to_string()
        })?;
    if !model_path.is_file() {
        return Err(format!(
            "process-model-check model file not found: {}",
            model_path.display()
        ));
    }

    let expected = read_process_operation_model(&model_path)?;
    let loaded = load_plc_source(&plc_path_buf)?;
    let actual = build_process_operation_model_for_loaded_source(&loaded)?;
    let refinement = verify_process_operation_refinement(&expected, &actual);
    let report = ProcessModelCheckCliReport {
        schema_version: 1,
        command: "process-model-check",
        output: output_mode.as_str(),
        source_plc: display_path_relative_to_cwd(&plc_path_buf),
        model_path: display_path_relative_to_cwd(&model_path),
        status: refinement.status,
        expected_operation_count: refinement.expected_operation_count,
        actual_operation_count: refinement.actual_operation_count,
        issue_count: refinement.issues.len(),
        issues: refinement.issues,
    };

    if output_mode == CliOutputMode::Json {
        let mut body = serde_json::to_string_pretty(&report)
            .map_err(|err| format!("Failed to serialize process-model-check JSON: {err}"))?;
        body.push('\n');
        print!("{body}");
    } else {
        eprintln!(
            "process-model-check: {} (issues={})",
            match report.status {
                RefinementStatus::Pass => "PASS",
                RefinementStatus::Fail => "FAIL",
            },
            report.issue_count
        );
        eprintln!("  source: {}", report.source_plc);
        eprintln!("  model: {}", report.model_path);
        for issue in report.issues.iter().take(10) {
            eprintln!(
                "  [{}] {}: {}",
                issue.code, issue.operation_id, issue.message
            );
        }
    }

    if report.status == RefinementStatus::Fail {
        return Err(format!(
            "process-model-check failed: {} issue(s)",
            report.issue_count
        ));
    }

    Ok(())
}

fn write_operation_model(path: &Path, model: &ProcessOperationModel) -> Result<(), String> {
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
    {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|err| {
                    format!(
                        "Failed to create output directory {}: {err}",
                        parent.display()
                    )
                })?;
            }
        }
        let mut body = toml::to_string_pretty(model).map_err(|err| {
            format!(
                "Failed to serialize process operation model TOML for {}: {err}",
                path.display()
            )
        })?;
        body.push('\n');
        fs::write(path, body).map_err(|err| format!("Failed to write {}: {err}", path.display()))
    } else {
        write_json_pretty(path, model)
    }
}

fn run_state_proof_check_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "state-proof-check");

    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut config_path: Option<PathBuf> = None;
    let mut output_mode = CliOutputMode::Human;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                config_path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --config <config/state_proof.toml>".to_string()
                })?));
            }
            "--output" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --output <human|json>".to_string())?;
                output_mode = CliOutputMode::parse(&raw).ok_or_else(|| {
                    format!("Invalid value for --output `{raw}` (expected human or json)")
                })?;
            }
            "-h" | "--help" => return Err(usage.clone()),
            other => {
                return Err(format!(
                    "Unknown argument for state-proof-check: {other}\n{usage}"
                ));
            }
        }
    }

    if !is_supported_plc_source_path(Path::new(&plc_path)) {
        return Err(format!(
            "state-proof-check expects a supported PLC source path, got: {plc_path}"
        ));
    }

    let plc_path_buf = PathBuf::from(&plc_path);
    let loaded = load_plc_source(&plc_path_buf)?;
    let program = parse_loaded_plc_with_required_purpose(&loaded)?;
    let loaded_config = load_state_proof_config(&plc_path_buf, config_path.as_deref())?;
    let mut issues = analyze_program(&program, &loaded_config.config);
    remap_state_proof_issues(&mut issues, &loaded, &plc_path_buf);

    let error_count = issues
        .iter()
        .filter(|issue| issue.severity.is_error())
        .count();
    let report = StateProofCheckCliReport {
        schema_version: 1,
        command: "state-proof-check",
        output: output_mode.as_str(),
        source_plc: display_path_relative_to_cwd(&plc_path_buf),
        config_path: loaded_config
            .path
            .as_ref()
            .map(|path| display_path_relative_to_cwd(path)),
        status: if error_count == 0 {
            StateProofStatus::Pass
        } else {
            StateProofStatus::Fail
        },
        issue_count: issues.len(),
        issues,
    };

    match output_mode {
        CliOutputMode::Json => {
            let mut body = serde_json::to_string_pretty(&report)
                .map_err(|err| format!("Failed to serialize state-proof-check JSON: {err}"))?;
            body.push('\n');
            print!("{body}");
        }
        CliOutputMode::Human => {
            eprintln!(
                "state-proof-check: {} (issues={})",
                match report.status {
                    StateProofStatus::Pass => "PASS",
                    StateProofStatus::Fail => "FAIL",
                },
                report.issue_count
            );
            eprintln!("  source: {}", report.source_plc);
            eprintln!(
                "  config: {}",
                report.config_path.as_deref().unwrap_or("<none>")
            );
            for issue in &report.issues {
                eprintln!(
                    "  [{}] {}:{}{}{}{}",
                    match issue.severity {
                        StateProofSeverity::Error => "ERROR",
                        StateProofSeverity::Warning => "WARN",
                    },
                    issue.code,
                    issue
                        .source_file
                        .as_deref()
                        .unwrap_or(report.source_plc.as_str()),
                    ":",
                    issue.line,
                    issue
                        .task
                        .as_ref()
                        .zip(issue.step.as_ref())
                        .map(|(task, step)| format!(" ({task}.{step})"))
                        .unwrap_or_default(),
                );
                eprintln!("    reason: {}", issue.message);
                eprintln!("    fix: {}", issue.fix);
            }
        }
    }

    if error_count > 0 {
        return Err(format!(
            "state-proof-check failed: {} issue(s)",
            report.issue_count
        ));
    }

    Ok(())
}

fn remap_state_proof_issues(
    issues: &mut [StateProofIssue],
    loaded: &rust_plc::source_bundle::LoadedPlcSource,
    requested_path: &Path,
) {
    for issue in issues.iter_mut() {
        if let Some(location) = loaded.source_map.remap_location(issue.line.max(1), 1) {
            issue.line = location.line.max(1);
            issue.source_file = Some(display_path_relative_to_cwd(Path::new(&location.file)));
        } else if issue.source_file.is_none() {
            issue.source_file = Some(display_path_relative_to_cwd(requested_path));
        }
    }
    issues.sort_by(|left, right| {
        left.severity
            .is_error()
            .cmp(&right.severity.is_error())
            .reverse()
            .then(left.line.cmp(&right.line))
            .then(left.code.cmp(&right.code))
    });
}

#[derive(Debug, Serialize)]
struct GeometryExportReport {
    schema_version: u32,
    command: &'static str,
    source_plc: String,
    output_path: String,
    node_count: usize,
    edge_count: usize,
    observed_transition_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    trace_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    intent_report_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    intent_verdict: Option<IntentAlignmentVerdict>,
}

fn run_geometry_export_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "geometry-export");

    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut out_path: Option<PathBuf> = None;
    let mut trace_path: Option<PathBuf> = None;
    let mut intent_report_path: Option<PathBuf> = None;
    let mut output_mode = CliOutputMode::Human;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => {
                let value = args.next().ok_or_else(|| {
                    "Missing value for --out <geometry.json> in geometry-export".to_string()
                })?;
                out_path = Some(PathBuf::from(value));
            }
            "--trace" => {
                let value = args.next().ok_or_else(|| {
                    "Missing value for --trace <trace.jsonl> in geometry-export".to_string()
                })?;
                trace_path = Some(PathBuf::from(value));
            }
            "--intent-report" => {
                let value = args.next().ok_or_else(|| {
                    "Missing value for --intent-report <report.json> in geometry-export".to_string()
                })?;
                intent_report_path = Some(PathBuf::from(value));
            }
            "--output" => {
                let raw = args.next().ok_or_else(|| {
                    "Missing value for --output <human|json> in geometry-export".to_string()
                })?;
                output_mode = CliOutputMode::parse(&raw).ok_or_else(|| {
                    format!(
                        "Invalid value for --output `{raw}` in geometry-export (expected human or json)"
                    )
                })?;
            }
            "-h" | "--help" => return Err(usage.clone()),
            other => {
                return Err(format!(
                    "Unknown argument for geometry-export: {other}\n{usage}"
                ));
            }
        }
    }

    let out_path = out_path.ok_or_else(|| {
        "geometry-export requires --out <geometry.json> to persist the artifact".to_string()
    })?;

    if !is_supported_plc_source_path(Path::new(&plc_path)) {
        return Err(format!(
            "geometry-export expects a supported PLC source path, got: {plc_path}"
        ));
    }

    let loaded = load_plc_source(Path::new(&plc_path))?;
    let semantics =
        compile_loaded_codegen_semantics(&loaded).map_err(|errors| errors.join("\n"))?;

    let trace_events = if let Some(path) = &trace_path {
        let text = fs::read_to_string(path)
            .map_err(|err| format!("Failed to read trace {}: {err}", path.display()))?;
        Some(parse_trace_jsonl(&text).map_err(|err| {
            format!(
                "Failed to parse trace JSONL {} for geometry-export: {err}",
                path.display()
            )
        })?)
    } else {
        None
    };

    let intent_report = if let Some(path) = &intent_report_path {
        let text = fs::read_to_string(path)
            .map_err(|err| format!("Failed to read intent report {}: {err}", path.display()))?;
        Some(serde_json::from_str(&text).map_err(|err| {
            format!(
                "Failed to parse intent report {} for geometry-export: {err}",
                path.display()
            )
        })?)
    } else {
        None
    };

    let artifact: GeometryArtifact = export_geometry_artifact(
        &plc_path,
        &semantics.topology,
        &semantics.constraints,
        &semantics.state_machine,
        trace_events.as_deref(),
        intent_report.as_ref(),
    );
    write_json_pretty(&out_path, &artifact)?;

    let report = GeometryExportReport {
        schema_version: 1,
        command: "geometry-export",
        source_plc: plc_path,
        output_path: display_path_relative_to_cwd(&out_path),
        node_count: artifact.nodes.len(),
        edge_count: artifact.edges.len(),
        observed_transition_count: artifact.summary.observed_transition_count,
        trace_path: trace_path
            .as_ref()
            .map(|path| display_path_relative_to_cwd(path.as_path())),
        intent_report_path: intent_report_path
            .as_ref()
            .map(|path| display_path_relative_to_cwd(path.as_path())),
        intent_verdict: artifact
            .overlays
            .intent
            .as_ref()
            .map(|overlay| overlay.verdict),
    };

    match output_mode {
        CliOutputMode::Json => {
            let mut body = serde_json::to_string_pretty(&report)
                .map_err(|err| format!("Failed to serialize geometry-export JSON: {err}"))?;
            body.push('\n');
            print!("{body}");
        }
        CliOutputMode::Human => {
            eprintln!("geometry-export: PASS");
            eprintln!("  source: {}", report.source_plc);
            eprintln!("  out: {}", report.output_path);
            eprintln!("  nodes: {}", report.node_count);
            eprintln!("  edges: {}", report.edge_count);
            if let Some(trace_path) = &report.trace_path {
                eprintln!("  trace: {trace_path}");
                eprintln!(
                    "  observed_transitions: {}",
                    report.observed_transition_count
                );
            }
            if let Some(verdict) = report.intent_verdict {
                eprintln!("  intent_verdict: {}", intent_verdict_name(verdict));
            }
        }
    }

    Ok(())
}

fn intent_verdict_name(verdict: IntentAlignmentVerdict) -> &'static str {
    match verdict {
        IntentAlignmentVerdict::Aligned => "aligned",
        IntentAlignmentVerdict::Mismatch => "mismatch",
        IntentAlignmentVerdict::Blocked => "blocked",
    }
}

fn run_gen_st_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "gen-st");

    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut out_path: Option<PathBuf> = None;
    let mut program_name = "Main".to_string();
    let mut include_verification_summary = true;
    let mut task_interval_ms: u64 = 10;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => {
                let value = args.next().ok_or_else(|| {
                    "Missing value for --out <output.st> in gen-st subcommand".to_string()
                })?;
                out_path = Some(PathBuf::from(value));
            }
            "--program-name" => {
                program_name = args
                    .next()
                    .ok_or_else(|| "Missing value for --program-name <Main>".to_string())?;
                if program_name.trim().is_empty() {
                    return Err("--program-name cannot be empty".to_string());
                }
            }
            "--no-verification-summary" => {
                include_verification_summary = false;
            }
            "--task-interval-ms" => {
                let raw = args.next().ok_or_else(|| {
                    "Missing value for --task-interval-ms <ms> in gen-st subcommand".to_string()
                })?;
                task_interval_ms = raw.parse::<u64>().map_err(|_| {
                    format!("Invalid value for --task-interval-ms `{raw}` (expected integer)")
                })?;
                if task_interval_ms == 0 {
                    return Err("--task-interval-ms must be greater than zero".to_string());
                }
            }
            "-h" | "--help" => return Err(usage.clone()),
            other => return Err(format!("Unknown argument for gen-st: {other}\n{usage}")),
        }
    }

    if !is_supported_plc_source_path(Path::new(&plc_path)) {
        return Err(format!(
            "gen-st expects a supported PLC source path, got: {plc_path}"
        ));
    }

    let loaded = load_plc_source(Path::new(&plc_path))?;
    let semantics =
        compile_loaded_codegen_semantics(&loaded).map_err(|errors| errors.join("\n"))?;

    let config = StCodegenConfig {
        program_name,
        source_file: plc_path.clone(),
        include_verification_summary,
        task_interval_ms,
    };
    let st_text = generate_st(
        &semantics.topology,
        &semantics.constraints,
        &semantics.state_machine,
        &config,
    )
    .map_err(format_st_codegen_errors)?;

    if let Some(path) = out_path {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|err| {
                    format!("Failed to create output directory {parent:?}: {err}")
                })?;
            }
        }
        fs::write(&path, st_text)
            .map_err(|err| format!("Failed to write ST file {}: {err}", path.display()))?;
        eprintln!("st_output: {}", path.display());
        return Ok(());
    }

    print!("{st_text}");
    Ok(())
}

fn format_st_codegen_errors(errors: Vec<StCodegenError>) -> String {
    let mut out = String::from("ST code generation failed:\n");
    for error in errors {
        out.push_str(&format!("  - {error}\n"));
    }
    out.trim_end().to_string()
}

fn run_sequence_lint_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "sequence-lint");
    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut config = SequenceLintConfig::default();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--critical-wait-level" => {
                let raw_level = args.next().ok_or_else(|| {
                    "Missing value for --critical-wait-level <warn|error>".to_string()
                })?;
                config.critical_wait_level = raw_level.parse::<LintLevel>()?;
            }
            "--critical-wait-exempt" => {
                let spec = args.next().ok_or_else(|| {
                    "Missing value for --critical-wait-exempt <task.step|task.*>".to_string()
                })?;
                let exemption = CriticalWaitExemption::parse(&spec)
                    .map_err(|err| format!("Invalid exemption `{spec}`: {err}"))?;
                config.critical_wait_exemptions.push(exemption);
            }
            "-h" | "--help" => {
                return Err(usage.clone());
            }
            other => {
                return Err(format!("Unknown argument for sequence-lint: {other}"));
            }
        }
    }

    if !is_supported_plc_source_path(Path::new(&plc_path)) {
        return Err(format!(
            "sequence-lint expects a supported PLC source path, got: {plc_path}"
        ));
    }

    let loaded = load_plc_source(Path::new(&plc_path))?;
    let parsed = parse_loaded_plc_with_required_purpose(&loaded)?;
    let expanded = preprocess_program(&parsed)
        .map_err(|errors| format_loaded_plc_errors(errors, &loaded).join("\n"))?;
    let findings = lint_critical_wait_recovery(&expanded, &config);
    if findings.is_empty() {
        eprintln!("sequence-lint: PASS (critical_wait_recovery)");
        return Ok(());
    }

    for finding in &findings {
        eprintln!("{finding}");
    }

    match config.critical_wait_level {
        LintLevel::Warn => Ok(()),
        LintLevel::Error => Err(format!(
            "sequence-lint failed: {} critical wait finding(s)",
            findings.len()
        )),
    }
}

#[derive(Debug, Serialize)]
struct ProjectCheckStepReport {
    name: &'static str,
    command: Vec<String>,
    status: &'static str,
    exit_code: Option<i32>,
    stdout_log: String,
    stderr_log: String,
    report_json: Option<String>,
    artifacts_dir: Option<String>,
    intent_alignment_verdict: Option<IntentAlignmentVerdict>,
    intent_alignment_primary_mismatch_kind: Option<IntentMismatchKind>,
    intent_alignment_blocker_kind: Option<IntentAlignmentBlockerKind>,
    intent_alignment_comparator_version: Option<String>,
}

#[derive(Debug, Serialize)]
struct ProjectCheckReport {
    schema_version: u32,
    command: &'static str,
    output: &'static str,
    source_plc: String,
    scenario: String,
    out_dir: String,
    status: &'static str,
    failed_steps: usize,
    steps: Vec<ProjectCheckStepReport>,
}

#[derive(Debug, Serialize)]
struct ProjectCheckIntentAlignmentStepOutput {
    verdict: IntentAlignmentVerdict,
    summary: rust_plc::intent_alignment::IntentAlignmentPipelineSummary,
    report: rust_plc::intent_alignment::IntentAlignmentReport,
}

fn run_project_check_child(
    executable: &Path,
    step_name: &'static str,
    args: &[String],
    step_dir: &Path,
    stdout_report_name: Option<&str>,
) -> Result<ProjectCheckStepReport, String> {
    fs::create_dir_all(step_dir).map_err(|err| {
        format!(
            "Failed to create project-check step directory {}: {err}",
            step_dir.display()
        )
    })?;

    let output = Command::new(executable)
        .args(args)
        .output()
        .map_err(|err| format!("Failed to run project-check step `{step_name}`: {err}"))?;

    let stdout_log_path = step_dir.join("stdout.log");
    fs::write(&stdout_log_path, &output.stdout).map_err(|err| {
        format!(
            "Failed to write project-check stdout log {}: {err}",
            stdout_log_path.display()
        )
    })?;
    let stderr_log_path = step_dir.join("stderr.log");
    fs::write(&stderr_log_path, &output.stderr).map_err(|err| {
        format!(
            "Failed to write project-check stderr log {}: {err}",
            stderr_log_path.display()
        )
    })?;

    let report_json = stdout_report_name.and_then(|name| {
        let report_path = step_dir.join(name);
        if output.stdout.is_empty() {
            return None;
        }
        fs::write(&report_path, &output.stdout).ok()?;
        Some(display_path_relative_to_cwd(&report_path))
    });

    let mut command = Vec::with_capacity(args.len() + 1);
    command.push(display_path_relative_to_cwd(executable));
    command.extend(args.iter().cloned());

    Ok(ProjectCheckStepReport {
        name: step_name,
        command,
        status: if output.status.success() {
            "pass"
        } else {
            "fail"
        },
        exit_code: output.status.code(),
        stdout_log: display_path_relative_to_cwd(&stdout_log_path),
        stderr_log: display_path_relative_to_cwd(&stderr_log_path),
        report_json,
        artifacts_dir: None,
        intent_alignment_verdict: None,
        intent_alignment_primary_mismatch_kind: None,
        intent_alignment_blocker_kind: None,
        intent_alignment_comparator_version: None,
    })
}

fn find_intent_alignment_contract(plc_path: &Path) -> Option<PathBuf> {
    let stem = plc_path.file_stem()?.to_string_lossy();
    let candidate = plc_path.with_file_name(format!("{stem}.intent_alignment.contract.json"));
    if candidate.exists() {
        return Some(candidate);
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

fn resolve_intent_contract_workspace_root(
    contract_path: &Path,
    contract: &IntentContract,
) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    if let Ok(current_dir) = env::current_dir() {
        candidates.push(current_dir);
    }
    candidates.extend(contract_path.ancestors().skip(1).map(Path::to_path_buf));

    candidates
        .into_iter()
        .find(|root| {
            root.join(&contract.source_ref.path).is_file()
                && contract
                    .metadata
                    .review_basis
                    .iter()
                    .all(|review| root.join(&review.source.path).is_file())
        })
        .ok_or_else(|| {
            format!(
                "Failed to resolve intent contract workspace root for {}; none of the candidate roots expose `{}` and all review basis sources",
                contract_path.display(),
                contract.source_ref.path
            )
        })
}

fn failed_intent_alignment_step(
    command: Vec<String>,
    stdout_log_path: &Path,
    stderr_log_path: &Path,
    error_message: &str,
    blocker_kind: IntentAlignmentBlockerKind,
) -> Result<ProjectCheckStepReport, String> {
    fs::write(stdout_log_path, b"").map_err(|write_err| {
        format!(
            "Failed to write project-check stdout log {}: {write_err}",
            stdout_log_path.display()
        )
    })?;
    fs::write(stderr_log_path, format!("{error_message}\n").as_bytes()).map_err(|write_err| {
        format!(
            "Failed to write project-check stderr log {}: {write_err}",
            stderr_log_path.display()
        )
    })?;

    Ok(ProjectCheckStepReport {
        name: "intent_alignment",
        command,
        status: "fail",
        exit_code: Some(1),
        stdout_log: display_path_relative_to_cwd(stdout_log_path),
        stderr_log: display_path_relative_to_cwd(stderr_log_path),
        report_json: None,
        artifacts_dir: None,
        intent_alignment_verdict: Some(IntentAlignmentVerdict::Blocked),
        intent_alignment_primary_mismatch_kind: None,
        intent_alignment_blocker_kind: Some(blocker_kind),
        intent_alignment_comparator_version: None,
    })
}

fn run_project_check_intent_alignment_step(
    step_dir: &Path,
    contract_path: &Path,
    evidence_path: &Path,
) -> Result<ProjectCheckStepReport, String> {
    fs::create_dir_all(step_dir).map_err(|err| {
        format!(
            "Failed to create project-check step directory {}: {err}",
            step_dir.display()
        )
    })?;

    let stdout_log_path = step_dir.join("stdout.log");
    let stderr_log_path = step_dir.join("stderr.log");
    let report_path = step_dir.join("report.json");
    let summary_path = step_dir.join("summary.json");

    let command = vec![
        "project-check.intent-alignment".to_string(),
        "--intent-contract".to_string(),
        display_path_relative_to_cwd(contract_path),
        "--intent-evidence".to_string(),
        display_path_relative_to_cwd(evidence_path),
    ];

    let contract = match read_intent_contract(contract_path) {
        Ok(contract) => contract,
        Err(err) => {
            return failed_intent_alignment_step(
                command,
                &stdout_log_path,
                &stderr_log_path,
                &format!("Failed to load intent contract: {err}"),
                IntentAlignmentBlockerKind::InvalidContract,
            );
        }
    };
    if let Err(err) = verify_intent_contract_delivery_readiness(&contract) {
        return failed_intent_alignment_step(
            command,
            &stdout_log_path,
            &stderr_log_path,
            &format!("Intent contract is still a scaffold placeholder: {err}"),
            IntentAlignmentBlockerKind::InvalidContract,
        );
    }
    let workspace_root = match resolve_intent_contract_workspace_root(contract_path, &contract) {
        Ok(root) => root,
        Err(err) => {
            return failed_intent_alignment_step(
                command,
                &stdout_log_path,
                &stderr_log_path,
                &err,
                IntentAlignmentBlockerKind::InvalidContract,
            );
        }
    };
    if let Err(err) = verify_intent_contract_source_binding(&contract, &workspace_root) {
        return failed_intent_alignment_step(
            command,
            &stdout_log_path,
            &stderr_log_path,
            &format!("Intent contract source binding is invalid: {err}"),
            IntentAlignmentBlockerKind::InvalidContract,
        );
    }

    let result = (|| -> Result<ProjectCheckIntentAlignmentStepOutput, String> {
        let spec = compile_expected_behavior_spec(&contract)
            .map_err(|err| format!("Failed to compile intent contract: {err}"))?;
        let evidence = fs::read_to_string(evidence_path).map_err(|err| {
            format!(
                "Failed to read intent evidence {}: {err}",
                evidence_path.display()
            )
        })?;
        let report = compare_trace_jsonl(&spec, &evidence)
            .map_err(|err| format!("Intent alignment compare failed: {err}"))?;
        let summary = reduce_intent_alignment_report(&report);

        Ok(ProjectCheckIntentAlignmentStepOutput {
            verdict: summary.verdict,
            summary,
            report,
        })
    })();

    match result {
        Ok(output) => {
            write_json_pretty(&report_path, &output.report)?;
            write_json_pretty(&summary_path, &output.summary)?;
            let mut stdout_json = serde_json::to_string_pretty(&output).map_err(|err| {
                format!("Failed to serialize intent-alignment step output: {err}")
            })?;
            stdout_json.push('\n');
            fs::write(&stdout_log_path, stdout_json.as_bytes()).map_err(|err| {
                format!(
                    "Failed to write project-check stdout log {}: {err}",
                    stdout_log_path.display()
                )
            })?;
            let stderr_message = if output.verdict == IntentAlignmentVerdict::Aligned {
                String::new()
            } else {
                format!("intent-alignment verdict: {:?}\n", output.verdict)
            };
            fs::write(&stderr_log_path, stderr_message.as_bytes()).map_err(|err| {
                format!(
                    "Failed to write project-check stderr log {}: {err}",
                    stderr_log_path.display()
                )
            })?;

            Ok(ProjectCheckStepReport {
                name: "intent_alignment",
                command,
                status: if output.verdict == IntentAlignmentVerdict::Aligned {
                    "pass"
                } else {
                    "fail"
                },
                exit_code: Some(if output.verdict == IntentAlignmentVerdict::Aligned {
                    0
                } else {
                    1
                }),
                stdout_log: display_path_relative_to_cwd(&stdout_log_path),
                stderr_log: display_path_relative_to_cwd(&stderr_log_path),
                report_json: Some(display_path_relative_to_cwd(&report_path)),
                artifacts_dir: Some(display_path_relative_to_cwd(step_dir)),
                intent_alignment_verdict: Some(output.summary.verdict),
                intent_alignment_primary_mismatch_kind: output.summary.primary_mismatch_kind,
                intent_alignment_blocker_kind: output.summary.blocker_kind,
                intent_alignment_comparator_version: Some(
                    output.summary.comparator_version.clone(),
                ),
            })
        }
        Err(err) => failed_intent_alignment_step(
            command,
            &stdout_log_path,
            &stderr_log_path,
            &err,
            IntentAlignmentBlockerKind::MissingEvidence,
        ),
    }
}

fn run_project_check_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "project-check");
    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut scenario_path: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut output_mode = CliOutputMode::Human;
    let mut max_p99_exec_us: Option<u64> = None;
    let mut max_overrun_count: Option<u64> = None;
    let mut intent_contract_path: Option<PathBuf> = None;
    let mut intent_evidence_path: Option<PathBuf> = None;
    let mut require_process_model = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenario" => {
                scenario_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --scenario <scenario.yaml>".to_string()
                    })?));
            }
            "--out-dir" => {
                out_dir =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --out-dir <dir>".to_string()
                    })?));
            }
            "--output" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --output <human|json>".to_string())?;
                output_mode = CliOutputMode::parse(&raw).ok_or_else(|| {
                    format!("Invalid --output value `{raw}` (expected `human` or `json`)")
                })?;
            }
            "--max-p99-exec-us" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --max-p99-exec-us <us>".to_string())?;
                max_p99_exec_us = Some(
                    raw.parse::<u64>()
                        .map_err(|_| format!("Invalid integer for --max-p99-exec-us: {raw}"))?,
                );
            }
            "--max-overrun-count" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --max-overrun-count <n>".to_string())?;
                max_overrun_count = Some(
                    raw.parse::<u64>()
                        .map_err(|_| format!("Invalid integer for --max-overrun-count: {raw}"))?,
                );
            }
            "--intent-contract" => {
                intent_contract_path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --intent-contract <contract.json>".to_string()
                })?));
            }
            "--intent-evidence" => {
                intent_evidence_path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --intent-evidence <trace.jsonl>".to_string()
                })?));
            }
            "--require-process-model" => {
                require_process_model = true;
            }
            "-h" | "--help" => return Err(usage.clone()),
            other => return Err(format!("Unknown argument for project-check: {other}")),
        }
    }

    let scenario_path = scenario_path
        .ok_or_else(|| "Missing required argument: --scenario <scenario.yaml>".to_string())?;
    let out_dir =
        out_dir.ok_or_else(|| "Missing required argument: --out-dir <dir>".to_string())?;
    if intent_evidence_path.is_some() && intent_contract_path.is_none() {
        return Err(
            "project-check requires --intent-contract when --intent-evidence is provided"
                .to_string(),
        );
    }

    fs::create_dir_all(&out_dir).map_err(|err| {
        format!(
            "Failed to create project-check output directory {}: {err}",
            out_dir.display()
        )
    })?;

    let executable =
        env::current_exe().map_err(|err| format!("Failed to resolve current executable: {err}"))?;
    let plc_path_buf = PathBuf::from(&plc_path);
    let plc_arg = plc_path_buf.to_string_lossy().into_owned();
    let scenario_arg = scenario_path.to_string_lossy().into_owned();

    let compile_dir = out_dir.join("compile_verify");
    let compile_report_path = compile_dir.join("verification_report.json");
    let compile_args = vec![
        plc_arg.clone(),
        "--report".to_string(),
        compile_report_path.to_string_lossy().into_owned(),
        "--no-print-ir".to_string(),
    ];
    let mut compile_step = run_project_check_child(
        &executable,
        "compile_verify",
        &compile_args,
        &compile_dir,
        None,
    )?;
    if compile_report_path.exists() {
        compile_step.report_json = Some(display_path_relative_to_cwd(&compile_report_path));
    }

    let lint_dir = out_dir.join("sequence_lint");
    let lint_args = vec![
        "sequence-lint".to_string(),
        plc_arg.clone(),
        "--critical-wait-level".to_string(),
        "error".to_string(),
    ];
    let lint_step =
        run_project_check_child(&executable, "sequence_lint", &lint_args, &lint_dir, None)?;

    let state_proof_step = load_plc_source(&plc_path_buf)
        .ok()
        .and_then(|loaded| parse_loaded_plc_with_required_purpose(&loaded).ok())
        .filter(|program| should_auto_run_state_proof_check(program, &plc_path_buf))
        .map(|_| {
            let state_proof_dir = out_dir.join("state_proof_check");
            let state_proof_args = vec![
                "state-proof-check".to_string(),
                plc_arg.clone(),
                "--output".to_string(),
                "json".to_string(),
            ];
            run_project_check_child(
                &executable,
                "state_proof_check",
                &state_proof_args,
                &state_proof_dir,
                Some("report.json"),
            )
        })
        .transpose()?;

    let default_process_model = default_process_model_path(&plc_path_buf);
    let process_model_step = match default_process_model.filter(|path| path.is_file()) {
        Some(model_path) => {
            let process_model_dir = out_dir.join("process_model_check");
            let process_model_args = vec![
                "process-model-check".to_string(),
                plc_arg.clone(),
                "--model".to_string(),
                model_path.to_string_lossy().into_owned(),
                "--output".to_string(),
                "json".to_string(),
            ];
            run_project_check_child(
                &executable,
                "process_model_check",
                &process_model_args,
                &process_model_dir,
                Some("report.json"),
            )
            .map(Some)?
        }
        None if require_process_model => {
            let process_model_dir = out_dir.join("process_model_check");
            fs::create_dir_all(&process_model_dir).map_err(|err| {
                format!(
                    "Failed to create project-check step directory {}: {err}",
                    process_model_dir.display()
                )
            })?;
            let stdout_log = process_model_dir.join("stdout.log");
            let stderr_log = process_model_dir.join("stderr.log");
            fs::write(&stdout_log, b"").map_err(|err| {
                format!(
                    "Failed to write project-check stdout log {}: {err}",
                    stdout_log.display()
                )
            })?;
            fs::write(
                &stderr_log,
                b"process_model/process_operation_model.toml is required but was not found\n",
            )
            .map_err(|err| {
                format!(
                    "Failed to write project-check stderr log {}: {err}",
                    stderr_log.display()
                )
            })?;
            Some(ProjectCheckStepReport {
                name: "process_model_check",
                command: vec![
                    "project-check.process-model-check".to_string(),
                    "--require-process-model".to_string(),
                ],
                status: "fail",
                exit_code: Some(1),
                stdout_log: display_path_relative_to_cwd(&stdout_log),
                stderr_log: display_path_relative_to_cwd(&stderr_log),
                report_json: None,
                artifacts_dir: None,
                intent_alignment_verdict: None,
                intent_alignment_primary_mismatch_kind: None,
                intent_alignment_blocker_kind: None,
                intent_alignment_comparator_version: None,
            })
        }
        None => None,
    };

    let doctor_dir = out_dir.join("scenario_doctor");
    let doctor_args = vec![
        "scenario-doctor".to_string(),
        plc_arg.clone(),
        "--scenario".to_string(),
        scenario_arg.clone(),
        "--output".to_string(),
        "json".to_string(),
    ];
    let doctor_step = run_project_check_child(
        &executable,
        "scenario_doctor",
        &doctor_args,
        &doctor_dir,
        Some("report.json"),
    )?;

    let gate_dir = out_dir.join("no_board_gate");
    let gate_artifacts_dir = gate_dir.join("artifacts");
    let mut gate_args = vec![
        "no-board-gate".to_string(),
        plc_arg,
        "--scenario".to_string(),
        scenario_arg,
        "--out-dir".to_string(),
        gate_artifacts_dir.to_string_lossy().into_owned(),
        "--output".to_string(),
        "json".to_string(),
    ];
    if let Some(limit) = max_p99_exec_us {
        gate_args.push("--max-p99-exec-us".to_string());
        gate_args.push(limit.to_string());
    }
    if let Some(limit) = max_overrun_count {
        gate_args.push("--max-overrun-count".to_string());
        gate_args.push(limit.to_string());
    }
    let mut gate_step = run_project_check_child(
        &executable,
        "no_board_gate",
        &gate_args,
        &gate_dir,
        Some("report.json"),
    )?;
    gate_step.artifacts_dir = Some(display_path_relative_to_cwd(&gate_artifacts_dir));

    let mut steps = vec![compile_step, lint_step];
    if let Some(step) = state_proof_step {
        steps.push(step);
    }
    if let Some(step) = process_model_step {
        steps.push(step);
    }
    steps.extend([doctor_step, gate_step]);
    let resolved_intent_contract = intent_contract_path
        .clone()
        .or_else(|| find_intent_alignment_contract(&plc_path_buf));
    let resolved_intent_evidence = intent_evidence_path
        .clone()
        .or_else(|| Some(gate_artifacts_dir.join("sil_trace.jsonl")));
    if let (Some(contract_path), Some(evidence_path)) = (
        resolved_intent_contract.as_ref(),
        resolved_intent_evidence.as_ref(),
    ) {
        let intent_dir = out_dir.join("intent_alignment");
        let intent_step =
            run_project_check_intent_alignment_step(&intent_dir, contract_path, evidence_path)?;
        steps.push(intent_step);
    }
    let failed_steps = steps.iter().filter(|step| step.status == "fail").count();
    let status = if failed_steps == 0 { "pass" } else { "fail" };
    let report = ProjectCheckReport {
        schema_version: 1,
        command: "project-check",
        output: output_mode.as_str(),
        source_plc: display_path_relative_to_cwd(&plc_path_buf),
        scenario: display_path_relative_to_cwd(&scenario_path),
        out_dir: display_path_relative_to_cwd(&out_dir),
        status,
        failed_steps,
        steps,
    };

    let report_path = out_dir.join("project_check_report.json");
    write_json_pretty(&report_path, &report)?;

    if output_mode == CliOutputMode::Json {
        let mut json = serde_json::to_string_pretty(&report)
            .map_err(|err| format!("Failed to serialize project-check JSON output: {err}"))?;
        json.push('\n');
        print!("{json}");
    } else {
        if failed_steps == 0 {
            eprintln!("project-check: PASS ({} steps)", report.steps.len());
        } else {
            eprintln!(
                "project-check: FAIL ({} of {} steps failed)",
                failed_steps,
                report.steps.len()
            );
        }
        eprintln!("  report: {}", report_path.display());
        for step in &report.steps {
            eprintln!("  [{}] {}", step.status.to_ascii_uppercase(), step.name);
            eprintln!("    stderr_log: {}", step.stderr_log);
            if let Some(path) = &step.report_json {
                eprintln!("    report_json: {path}");
            }
            if let Some(path) = &step.artifacts_dir {
                eprintln!("    artifacts_dir: {path}");
            }
        }
    }

    if failed_steps > 0 {
        return Err(format!(
            "project-check failed: {failed_steps} step(s) failed (see {})",
            report_path.display()
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::find_intent_alignment_contract;
    use std::fs;
    use std::path::PathBuf;

    fn temp_dir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "{prefix}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock works")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn finds_delivery_asset_contract_in_sibling_docs_dir() {
        let base = temp_dir("intent_contract_autodiscovery");
        let plc_dir = base.join("plc");
        let docs_dir = base.join("docs");
        fs::create_dir_all(&plc_dir).expect("create plc dir");
        fs::create_dir_all(&docs_dir).expect("create docs dir");

        let plc_path = plc_dir.join("main.bundle.toml");
        let contract_path = docs_dir.join("station.intent_alignment.contract.json");
        fs::write(&plc_path, "sources = []\n").expect("write bundle");
        fs::write(&contract_path, "{}\n").expect("write contract");

        assert_eq!(
            find_intent_alignment_contract(&plc_path),
            Some(contract_path)
        );
    }
}
