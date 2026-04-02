use crate::cli_support::common::DispatchResult;
use crate::cli_support::help::command_usage;
use rust_plc::codegen::st::{StCodegenConfig, StCodegenError, generate_st};
use rust_plc::semantic::preprocess_program;
use rust_plc::sequence_lint::{
    CriticalWaitExemption, LintLevel, SequenceLintConfig, lint_critical_wait_recovery,
};
use rust_plc::source_bundle::{is_supported_plc_source_path, load_plc_source};
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn try_dispatch(
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
        _ => return None,
    };
    Some(DispatchResult {
        error_prefix,
        result,
    })
}

fn run_gen_st_subcommand(program: &str, mut args: impl Iterator<Item = String>) -> Result<(), String> {
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
    let ir_bundle = crate::compile_pipeline(&loaded).map_err(|errors| errors.join("\n"))?;

    let config = StCodegenConfig {
        program_name,
        source_file: plc_path.clone(),
        include_verification_summary,
    };
    let st_text = generate_st(
        &ir_bundle.topology,
        &ir_bundle.constraints,
        &ir_bundle.state_machine,
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
    let parsed = crate::parse_plc_with_required_purpose(&loaded)?;
    let expanded = preprocess_program(&parsed)
        .map_err(|errors| crate::format_plc_errors(errors, &loaded).join("\n"))?;
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
