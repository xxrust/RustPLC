use crate::cli_support::common::{
    CliOutputMode, DispatchResult, display_path_relative_to_cwd, write_json_pretty,
};
use crate::cli_support::help::command_usage;
use crate::cli_support::plc_pipeline::{
    compile_loaded_codegen_semantics, format_loaded_plc_errors,
    parse_loaded_plc_with_required_purpose,
};
use rust_plc::codegen::st::{StCodegenConfig, StCodegenError, generate_st};
use rust_plc::semantic::preprocess_program;
use rust_plc::sequence_lint::{
    CriticalWaitExemption, LintLevel, SequenceLintConfig, lint_critical_wait_recovery,
};
use rust_plc::source_bundle::{is_supported_plc_source_path, load_plc_source};
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
        "gen-st" => (
            Some("[STGEN-000]"),
            run_gen_st_subcommand(program, remaining.iter().cloned()),
        ),
        "sequence-lint" => (
            None,
            run_sequence_lint_subcommand(program, remaining.iter().cloned()),
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
    })
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
            "-h" | "--help" => return Err(usage.clone()),
            other => return Err(format!("Unknown argument for project-check: {other}")),
        }
    }

    let scenario_path = scenario_path
        .ok_or_else(|| "Missing required argument: --scenario <scenario.yaml>".to_string())?;
    let out_dir =
        out_dir.ok_or_else(|| "Missing required argument: --out-dir <dir>".to_string())?;

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

    let steps = vec![compile_step, lint_step, doctor_step, gate_step];
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
