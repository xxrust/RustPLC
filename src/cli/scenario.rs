use crate::cli_support::common::{CliOutputMode, DispatchResult, display_path_relative_to_cwd};
use crate::cli_support::help::command_usage;
use crate::cli_support::plc_pipeline::compile_plc_to_runtime_program;
use crate::cli_support::runtime_probe::{io_sizes_for_program_and_scenario, is_halted};
use crate::cli_support::scenario_init::{
    ScenarioInitInputHints, ScenarioInitPreset, aliases_contain_keyword,
    collect_scenario_init_hints,
    default_scenario_init_out_path, render_scenario_init_yaml,
};
use crate::cli_support::scenario_validate::{
    ScenarioValidateFinding, ScenarioValidateSeverity, collect_scenario_referenced_forced_outputs,
    print_scenario_validate_findings, validate_scenario_against_plc,
};
use crate::cli_support::scenario_yaml::{
    format_resolve_scenario_yaml_error, parse_scenario_yaml, read_scenario_yaml_file,
    scenario_mismatch_hint_for_example,
};
use rust_plc::scenario_resolve::resolve_scenario_yaml_for_plc;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn try_dispatch(
    program: &str,
    command: &str,
    remaining: &[String],
) -> Option<DispatchResult> {
    let result = match command {
        "scenario-init" => run_scenario_init_subcommand(program, remaining.iter().cloned()),
        "scenario-validate" => run_scenario_validate_subcommand(program, remaining.iter().cloned()),
        "scenario-doctor" => run_scenario_doctor_subcommand(program, remaining.iter().cloned()),
        "scenario-expand" => run_scenario_expand_subcommand(program, remaining.iter().cloned()),
        "scenario-gen" => run_scenario_gen_subcommand(program, remaining.iter().cloned()),
        _ => return None,
    };
    Some(DispatchResult {
        error_prefix: None,
        result,
    })
}

pub(crate) fn run_scenario_init_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "scenario-init");
    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut out_path: Option<PathBuf> = None;
    let mut preset = ScenarioInitPreset::Normal;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => {
                out_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --out <scenario.yaml>".to_string()
                    })?));
            }
            "--preset" => {
                let raw = args
                    .next()
                    .ok_or_else(|| {
                        format!(
                            "Missing value for --preset <{}>",
                            ScenarioInitPreset::expected_values()
                        )
                    })?;
                preset = ScenarioInitPreset::parse(&raw).ok_or_else(|| {
                    format!(
                        "Invalid preset `{raw}` (expected `{}`)",
                        ScenarioInitPreset::expected_values()
                    )
                })?;
            }
            "-h" | "--help" => {
                return Err(usage.clone());
            }
            other => {
                return Err(format!("Unknown argument for scenario-init: {other}"));
            }
        }
    }

    let plc_path = PathBuf::from(plc_path);
    let plc_source = fs::read_to_string(&plc_path)
        .map_err(|err| format!("Failed to read {plc_path:?}: {err}"))?;

    let out_path = out_path.unwrap_or_else(|| default_scenario_init_out_path(&plc_path));
    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "Failed to create output directory {}: {err}",
                    parent.display()
                )
            })?;
        }
    }

    let hints = collect_scenario_init_hints(&plc_source)?;
    let yaml = render_scenario_init_yaml(&plc_path, preset, &hints);
    fs::write(&out_path, yaml).map_err(|err| {
        format!(
            "Failed to write scenario YAML {}: {err}",
            out_path.display()
        )
    })?;

    eprintln!("scenario-init: wrote {}", out_path.display());
    Ok(())
}

pub(crate) fn run_scenario_validate_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "scenario-validate");
    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut scenario_path: Option<PathBuf> = None;
    let mut output_mode = CliOutputMode::Human;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenario" => {
                scenario_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --scenario <scenario.yaml>".to_string()
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
            "-h" | "--help" => {
                return Err(usage.clone());
            }
            other => {
                return Err(format!("Unknown argument for scenario-validate: {other}"));
            }
        }
    }

    let Some(scenario_path) = scenario_path else {
        return Err("Missing required argument: --scenario <scenario.yaml>".to_string());
    };

    let plc_path = PathBuf::from(plc_path);
    let plc_source = fs::read_to_string(&plc_path)
        .map_err(|err| format!("Failed to read {plc_path:?}: {err}"))?;

    let raw_scenario_yaml = read_scenario_yaml_file(&scenario_path)?;
    let scenario_yaml =
        resolve_scenario_yaml_for_plc(&plc_source, &raw_scenario_yaml).map_err(|e| {
            format_resolve_scenario_yaml_error(
                plc_path.to_string_lossy().as_ref(),
                &scenario_path,
                "scenario-validate",
                &e,
            )
        })?;
    let scenario = parse_scenario_yaml(&scenario_yaml)?;

    let hints = collect_scenario_init_hints(&plc_source)?;
    let mut findings = validate_scenario_against_plc(&plc_path, &scenario_path, &scenario, &hints);

    // If the scenario was generated by `scenario-init`, it includes a header we can sanity-check.
    let header_source = raw_scenario_yaml.lines().take(40).find_map(|line| {
        line.strip_prefix("# Source PLC: ")
            .map(|s| s.trim().to_string())
    });
    if let (Some(expected), Some(actual)) = (
        header_source.as_deref(),
        plc_path.file_name().and_then(|s| s.to_str()),
    ) {
        if expected != actual {
            findings.push(ScenarioValidateFinding::warn(
                "risk.scenario_plc_mismatch",
                format!(
                    "scenario header says it was generated from `{expected}`, but you're validating against `{actual}`"
                ),
                Some(format!(
                    "Regenerate a PLC-matched skeleton:\n  rust_plc scenario-init {} --out {} --preset normal",
                    display_path_relative_to_cwd(&plc_path),
                    display_path_relative_to_cwd(&scenario_path)
                )),
            ));
        }
    }

    let has_error = findings
        .iter()
        .any(|f| f.severity == ScenarioValidateSeverity::Error);

    if !has_error {
        let runtime_program = compile_plc_to_runtime_program(&plc_source, scenario.tick_ms)
            .map_err(|e| {
                format!("scenario-validate: failed to compile PLC to runtime program: {e}")
            })?;
        let (num_di, num_do, num_ai, num_ao) =
            io_sizes_for_program_and_scenario(&runtime_program, &scenario);

        // Validate force output ids against program IO sizes (out-of-range forces are almost
        // always authoring mistakes and should fail early).
        let (forced_dos, forced_aos) = collect_scenario_referenced_forced_outputs(&scenario);
        for (path, id) in forced_dos {
            if id as usize >= num_do {
                findings.push(ScenarioValidateFinding::error(
                    path,
                    format!(
                        "DO{id} does not exist in `{}` (num_do={num_do})",
                        display_path_relative_to_cwd(&plc_path)
                    ),
                    Some(
                        "Fix the force id, or add the missing digital_output device in the PLC topology."
                            .to_string(),
                    ),
                ));
            }
        }
        for (path, id) in forced_aos {
            if id as usize >= num_ao {
                findings.push(ScenarioValidateFinding::error(
                    path,
                    format!(
                        "AO{id} does not exist in `{}` (num_ao={num_ao})",
                        display_path_relative_to_cwd(&plc_path)
                    ),
                    Some(
                        "Fix the force id, or add the missing analog_output device in the PLC topology."
                            .to_string(),
                    ),
                ));
            }
        }
        let mut io = sim::SimIo::new(num_di, num_do, num_ai, num_ao);

        // Re-apply the scenario onto the IO we will use for probing.
        if let Err(err) = scenario.apply_to_simio(&mut io) {
            findings.push(ScenarioValidateFinding::error(
                "scenario.apply",
                err.to_string(),
                Some("Fix the scenario YAML errors and retry.".to_string()),
            ));
        } else {
            let mut rt = runtime_core::Runtime::new(&runtime_program)
                .map_err(|err| format!("scenario-validate: runtime init failed: {err:?}"))?;

            // Probe the early ticks only; if a same-tick loop exists, it should surface quickly.
            let probe_ticks = scenario.duration_ticks().min(50);
            for _ in 0..probe_ticks {
                if let Err(err) = rt.tick_with_trace(&mut io, |_| {}) {
                    let sim_err = sim::SimRunError::from(err);
                    let mut suggestion = String::new();

                    if let Some(tip) = scenario_mismatch_hint_for_example(
                        plc_path.to_string_lossy().as_ref(),
                        &scenario_path,
                        &sim_err,
                        "scenario-validate",
                    ) {
                        suggestion.push_str(&tip);
                        suggestion.push('\n');
                    }

                    suggestion.push_str(
                        "If this is caused by inputs being satisfied immediately, try pulsing start_button and scripting sensor edges over time.\n\
Example:\n\
inputs:\n\
  - at_ms: 0\n\
    set:\n\
      digital_inputs:\n\
        10: true\n\
  - at_ms: 50\n\
    set:\n\
      digital_inputs:\n\
        10: false\n",
                    );

                    findings.push(ScenarioValidateFinding::error(
                        "runtime.probe",
                        sim_err.to_string(),
                        Some(suggestion),
                    ));
                    break;
                }
                if is_halted(&rt, &runtime_program) {
                    break;
                }
            }
        }
    }

    print_scenario_validate_findings(&findings, output_mode);

    if findings
        .iter()
        .any(|f| f.severity == ScenarioValidateSeverity::Error)
    {
        return Err("scenario-validate failed".to_string());
    }

    Ok(())
}

#[derive(Debug, Serialize)]
struct ScenarioDoctorIssue {
    code: &'static str,
    severity: &'static str,
    category: &'static str,
    tag: String,
    message: String,
    suggestion: Option<String>,
}

#[derive(Debug, Serialize)]
struct ScenarioDoctorReport {
    schema_version: u32,
    command: &'static str,
    output: &'static str,
    plc: String,
    scenario: String,
    status: &'static str,
    error_count: usize,
    warn_count: usize,
    issues: Vec<ScenarioDoctorIssue>,
}

fn doctor_category_from_tag(tag: &str) -> &'static str {
    if tag.ends_with(".at_ms") || tag == "duration_ms" {
        return "tick_alignment";
    }
    if tag.contains("digital_inputs")
        || tag.contains("analog_inputs")
        || tag.contains("digital_outputs")
        || tag.contains("analog_outputs")
        || tag.contains("scenario_plc_mismatch")
    {
        return "device_mapping";
    }
    if tag.starts_with("risk.") || tag == "runtime.probe" {
        return "same_tick_risk";
    }
    "general"
}

fn finding_to_doctor_issue(
    f: &ScenarioValidateFinding,
    include_suggestion: bool,
) -> ScenarioDoctorIssue {
    ScenarioDoctorIssue {
        code: f.code(),
        severity: f.severity.json_label(),
        category: doctor_category_from_tag(&f.tag),
        tag: f.tag.clone(),
        message: f.message.clone(),
        suggestion: if include_suggestion {
            f.suggestion.clone()
        } else {
            None
        },
    }
}

fn print_scenario_doctor_report(report: &ScenarioDoctorReport, output: CliOutputMode) {
    if output == CliOutputMode::Json {
        match serde_json::to_string_pretty(report) {
            Ok(mut body) => {
                body.push('\n');
                print!("{body}");
            }
            Err(err) => eprintln!("Failed to serialize scenario-doctor JSON output: {err}"),
        }
        return;
    }

    if report.error_count == 0 && report.warn_count == 0 {
        eprintln!("scenario-doctor: PASS (no issues)");
        return;
    }
    if report.error_count == 0 {
        eprintln!("scenario-doctor: PASS ({} warning(s))", report.warn_count);
    } else {
        eprintln!(
            "scenario-doctor: FAIL ({} error(s), {} warning(s))",
            report.error_count, report.warn_count
        );
    }

    for issue in &report.issues {
        eprintln!(
            "{} [{}:{}] {}",
            issue.severity.to_ascii_uppercase(),
            issue.code,
            issue.tag,
            issue.message
        );
        if let Some(suggestion) = &issue.suggestion {
            eprintln!("  Fix:\n{suggestion}");
        }
    }
}

pub(crate) fn run_scenario_doctor_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "scenario-doctor");
    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut scenario_path: Option<PathBuf> = None;
    let mut include_fix_preview = false;
    let mut output_mode = CliOutputMode::Human;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenario" => {
                scenario_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --scenario <scenario.yaml>".to_string()
                    })?));
            }
            "--fix-preview" => {
                include_fix_preview = true;
            }
            "--output" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --output <human|json>".to_string())?;
                output_mode = CliOutputMode::parse(&raw).ok_or_else(|| {
                    format!("Invalid --output value `{raw}` (expected `human` or `json`)")
                })?;
            }
            "-h" | "--help" => {
                return Err(usage.clone());
            }
            other => return Err(format!("Unknown argument for scenario-doctor: {other}")),
        }
    }

    let Some(scenario_path) = scenario_path else {
        return Err("Missing required argument: --scenario <scenario.yaml>".to_string());
    };

    let plc_path = PathBuf::from(plc_path);
    let plc_source = fs::read_to_string(&plc_path)
        .map_err(|err| format!("[SCN-DOCTOR-001] Failed to read {plc_path:?}: {err}"))?;
    let raw_scenario_yaml =
        read_scenario_yaml_file(&scenario_path).map_err(|err| format!("[SCN-DOCTOR-002] {err}"))?;

    let mut issues = Vec::<ScenarioDoctorIssue>::new();
    let header_source = raw_scenario_yaml.lines().take(40).find_map(|line| {
        line.strip_prefix("# Source PLC: ")
            .map(|s| s.trim().to_string())
    });
    if let (Some(expected), Some(actual)) = (
        header_source.as_deref(),
        plc_path.file_name().and_then(|s| s.to_str()),
    ) {
        if expected != actual {
            issues.push(ScenarioDoctorIssue {
                code: "SCN-MAP-001",
                severity: "warn",
                category: "path_mismatch",
                tag: "risk.scenario_plc_mismatch".to_string(),
                message: format!(
                    "scenario header says `{expected}`, but doctor is running against `{actual}`"
                ),
                suggestion: if include_fix_preview {
                    Some(format!(
                        "Regenerate with matched source:\n  rust_plc scenario-init {} --out {} --preset normal",
                        display_path_relative_to_cwd(&plc_path),
                        display_path_relative_to_cwd(&scenario_path)
                    ))
                } else {
                    None
                },
            });
        }
    }

    let resolved = match resolve_scenario_yaml_for_plc(&plc_source, &raw_scenario_yaml) {
        Ok(v) => Some(v),
        Err(err) => {
            issues.push(ScenarioDoctorIssue {
                code: "SCN-MAP-010",
                severity: "error",
                category: "device_mapping",
                tag: "resolve.device_name".to_string(),
                message: format_resolve_scenario_yaml_error(
                    plc_path.to_string_lossy().as_ref(),
                    &scenario_path,
                    "scenario-doctor",
                    &err,
                ),
                suggestion: if include_fix_preview {
                    Some("Fix device aliases/paths first, then rerun scenario-doctor.".to_string())
                } else {
                    None
                },
            });
            None
        }
    };

    if let Some(resolved_yaml) = resolved {
        match parse_scenario_yaml(&resolved_yaml) {
            Ok(scenario) => {
                let hints = collect_scenario_init_hints(&plc_source)
                    .map_err(|err| format!("[SCN-DOCTOR-003] {err}"))?;
                let findings =
                    validate_scenario_against_plc(&plc_path, &scenario_path, &scenario, &hints);
                issues.extend(
                    findings
                        .iter()
                        .map(|f| finding_to_doctor_issue(f, include_fix_preview)),
                );
            }
            Err(err) => {
                issues.push(ScenarioDoctorIssue {
                    code: "SCN-TICK-010",
                    severity: "error",
                    category: "tick_alignment",
                    tag: "parse.scenario_yaml".to_string(),
                    message: err,
                    suggestion: if include_fix_preview {
                        Some(
                            "Ensure all `at_ms` fields align to `tick_ms`, then rerun scenario-doctor."
                                .to_string(),
                        )
                    } else {
                        None
                    },
                });
            }
        }
    }

    let error_count = issues.iter().filter(|i| i.severity == "error").count();
    let warn_count = issues.iter().filter(|i| i.severity == "warn").count();
    let report = ScenarioDoctorReport {
        schema_version: 1,
        command: "scenario-doctor",
        output: output_mode.as_str(),
        plc: display_path_relative_to_cwd(&plc_path),
        scenario: display_path_relative_to_cwd(&scenario_path),
        status: if error_count == 0 { "pass" } else { "fail" },
        error_count,
        warn_count,
        issues,
    };
    print_scenario_doctor_report(&report, output_mode);

    if error_count > 0 {
        return Err(format!(
            "[SCN-DOCTOR-900] scenario-doctor found {error_count} blocking issue(s)"
        ));
    }

    Ok(())
}

pub(crate) fn run_scenario_expand_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "scenario-expand");
    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut scenario_path: Option<PathBuf> = None;
    let mut out_path: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenario" => {
                scenario_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --scenario <scenario.yaml>".to_string()
                    })?));
            }
            "--out" => {
                out_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --out <expanded.yaml>".to_string()
                    })?));
            }
            "-h" | "--help" => {
                return Err(usage.clone());
            }
            other => return Err(format!("Unknown argument for scenario-expand: {other}")),
        }
    }

    let scenario_path = scenario_path.ok_or_else(|| usage.clone())?;
    let out_path = out_path.ok_or_else(|| usage.clone())?;

    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to create output directory {parent:?}: {err}"))?;
        }
    }

    let plc_source =
        fs::read_to_string(&plc_path).map_err(|err| format!("Failed to read {plc_path}: {err}"))?;
    let scenario_yaml = read_scenario_yaml_file(&scenario_path)?;
    let resolved = resolve_scenario_yaml_for_plc(&plc_source, &scenario_yaml).map_err(|e| {
        format!(
            "Failed to resolve/expand scenario {}:\n{e}",
            scenario_path.display()
        )
    })?;
    let scenario = parse_scenario_yaml(&resolved)?;

    let mut out = serde_yaml::to_string(&scenario)
        .map_err(|err| format!("Failed to serialize scenario: {err}"))?;
    if !out.ends_with('\n') {
        out.push('\n');
    }
    fs::write(&out_path, out).map_err(|err| {
        format!(
            "Failed to write expanded scenario {}: {err}",
            out_path.display()
        )
    })?;
    eprintln!("scenario-expand: wrote {}", out_path.display());
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
struct ScenarioGenConfig {
    #[serde(default)]
    seed_base: Option<u64>,
    #[serde(default = "scenario_gen_default_tick_ms")]
    tick_ms: u64,
    #[serde(default)]
    duration_ms: Vec<u64>,
    #[serde(default)]
    start_pulse_ms: Vec<u64>,
    #[serde(default)]
    sensor_window_ms: Vec<u64>,
    #[serde(default)]
    inject_sensor_stuck: Vec<bool>,
    #[serde(default = "scenario_gen_default_max_cases")]
    max_cases: usize,
}

fn scenario_gen_default_tick_ms() -> u64 {
    10
}

fn scenario_gen_default_max_cases() -> usize {
    16
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScenarioCoverageMode {
    Pairwise,
    BoundaryFirst,
    RiskFirst,
}

impl ScenarioCoverageMode {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "pairwise" => Ok(Self::Pairwise),
            "boundary-first" => Ok(Self::BoundaryFirst),
            "risk-first" => Ok(Self::RiskFirst),
            other => Err(format!(
                "Invalid --coverage-mode `{other}` (expected pairwise|boundary-first|risk-first)"
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Pairwise => "pairwise",
            Self::BoundaryFirst => "boundary-first",
            Self::RiskFirst => "risk-first",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ScenarioGenCombo {
    duration_ms: u64,
    start_pulse_ms: u64,
    sensor_window_ms: u64,
    inject_sensor_stuck: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct ScenarioTemplateLibrary {
    schema_version: u32,
    templates: Vec<ScenarioTemplateMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScenarioTemplateMeta {
    id: String,
    path: String,
    kind: String,
    description: String,
    #[serde(default)]
    parameters: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScenarioGenCase {
    name: String,
    path: String,
    seed: Option<u64>,
    duration_ms: u64,
    start_pulse_ms: u64,
    sensor_window_ms: u64,
    inject_sensor_stuck: bool,
    template_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ScenarioGenSummary {
    schema_version: u32,
    plc: String,
    config: String,
    coverage_mode: String,
    dry_run: bool,
    template_library: String,
    count: usize,
    #[serde(default)]
    templates: Vec<ScenarioTemplateMeta>,
    cases: Vec<ScenarioGenCase>,
}

impl ScenarioGenConfig {
    fn validate(&self) -> Result<(), String> {
        if self.tick_ms == 0 {
            return Err("tick_ms must be > 0".to_string());
        }
        if self.max_cases == 0 {
            return Err("max_cases must be > 0".to_string());
        }

        for (i, duration) in self.duration_ms.iter().enumerate() {
            if *duration == 0 {
                return Err(format!("duration_ms[{i}] must be > 0"));
            }
            if *duration < self.tick_ms {
                return Err(format!(
                    "duration_ms[{i}] ({duration}) must be >= tick_ms ({})",
                    self.tick_ms
                ));
            }
        }
        for (i, pulse) in self.start_pulse_ms.iter().enumerate() {
            if *pulse == 0 {
                return Err(format!("start_pulse_ms[{i}] must be > 0"));
            }
        }
        for (i, window) in self.sensor_window_ms.iter().enumerate() {
            if *window == 0 {
                return Err(format!("sensor_window_ms[{i}] must be > 0"));
            }
        }
        Ok(())
    }

    fn seed_base_value(&self) -> u64 {
        self.seed_base.unwrap_or(42)
    }

    fn duration_values(&self) -> Vec<u64> {
        dedup_u64_preserve_order_with_default(&self.duration_ms, &[1000, 2000, 3000])
    }

    fn start_pulse_values(&self) -> Vec<u64> {
        dedup_u64_preserve_order_with_default(&self.start_pulse_ms, &[30, 50])
    }

    fn sensor_window_values(&self) -> Vec<u64> {
        dedup_u64_preserve_order_with_default(&self.sensor_window_ms, &[20, 40])
    }

    fn fault_values(&self) -> Vec<bool> {
        dedup_bool_preserve_order_with_default(&self.inject_sensor_stuck, &[false, true])
    }
}

fn dedup_u64_preserve_order_with_default(values: &[u64], default: &[u64]) -> Vec<u64> {
    let src = if values.is_empty() { default } else { values };
    let mut out = Vec::new();
    let mut seen = BTreeSet::<u64>::new();
    for v in src {
        if seen.insert(*v) {
            out.push(*v);
        }
    }
    out
}

fn dedup_bool_preserve_order_with_default(values: &[bool], default: &[bool]) -> Vec<bool> {
    let src = if values.is_empty() { default } else { values };
    let mut out = Vec::new();
    let mut seen = BTreeSet::<bool>::new();
    for v in src {
        if seen.insert(*v) {
            out.push(*v);
        }
    }
    out
}

fn scenario_gen_default_template_library_path() -> PathBuf {
    PathBuf::from("scenarios/templates/metadata.json")
}

fn load_scenario_template_library(path: &Path) -> Result<ScenarioTemplateLibrary, String> {
    let body = fs::read_to_string(path)
        .map_err(|err| format!("Failed to read template library {}: {err}", path.display()))?;
    let lib: ScenarioTemplateLibrary = serde_json::from_str(&body)
        .map_err(|err| format!("Failed to parse template library {}: {err}", path.display()))?;
    if lib.schema_version != 1 {
        return Err(format!(
            "template library schema_version={} is unsupported (expected 1)",
            lib.schema_version
        ));
    }
    if lib.templates.is_empty() {
        return Err(format!(
            "template library {} has no templates",
            path.display()
        ));
    }
    Ok(lib)
}

fn select_template_id(combo: &ScenarioGenCombo, lib: &ScenarioTemplateLibrary) -> String {
    let preferred_kind = if combo.inject_sensor_stuck {
        "fault"
    } else {
        "nominal"
    };
    lib.templates
        .iter()
        .find(|tpl| tpl.kind.eq_ignore_ascii_case(preferred_kind))
        .or_else(|| lib.templates.first())
        .map(|tpl| tpl.id.clone())
        .unwrap_or_else(|| "unassigned".to_string())
}

fn build_scenario_gen_combos(
    durations: &[u64],
    start_pulses: &[u64],
    sensor_windows: &[u64],
    fault_values: &[bool],
    mode: ScenarioCoverageMode,
) -> Vec<ScenarioGenCombo> {
    let mut combos = Vec::<ScenarioGenCombo>::new();
    for duration in durations {
        for pulse in start_pulses {
            for window in sensor_windows {
                for inject_fault in fault_values {
                    combos.push(ScenarioGenCombo {
                        duration_ms: *duration,
                        start_pulse_ms: *pulse,
                        sensor_window_ms: *window,
                        inject_sensor_stuck: *inject_fault,
                    });
                }
            }
        }
    }

    let duration_min = durations.iter().copied().min().unwrap_or(0);
    let duration_max = durations.iter().copied().max().unwrap_or(0);
    let pulse_min = start_pulses.iter().copied().min().unwrap_or(0);
    let pulse_max = start_pulses.iter().copied().max().unwrap_or(0);
    let window_min = sensor_windows.iter().copied().min().unwrap_or(0);
    let window_max = sensor_windows.iter().copied().max().unwrap_or(0);

    let boundary_score = |combo: &ScenarioGenCombo| -> u32 {
        let mut score = 0u32;
        if combo.duration_ms == duration_min || combo.duration_ms == duration_max {
            score += 2;
        }
        if combo.start_pulse_ms == pulse_min || combo.start_pulse_ms == pulse_max {
            score += 2;
        }
        if combo.sensor_window_ms == window_min || combo.sensor_window_ms == window_max {
            score += 2;
        }
        if combo.inject_sensor_stuck {
            score += 1;
        }
        score
    };

    let risk_score = |combo: &ScenarioGenCombo| -> u32 {
        let mut score = 0u32;
        if combo.inject_sensor_stuck {
            score += 100;
        }
        if combo.duration_ms == duration_min {
            score += 30;
        }
        if combo.start_pulse_ms == pulse_max {
            score += 20;
        }
        if combo.sensor_window_ms == window_max {
            score += 10;
        }
        score
    };

    match mode {
        ScenarioCoverageMode::Pairwise => {}
        ScenarioCoverageMode::BoundaryFirst => {
            combos.sort_by(|a, b| {
                boundary_score(b)
                    .cmp(&boundary_score(a))
                    .then_with(|| a.duration_ms.cmp(&b.duration_ms))
                    .then_with(|| a.start_pulse_ms.cmp(&b.start_pulse_ms))
                    .then_with(|| a.sensor_window_ms.cmp(&b.sensor_window_ms))
                    .then_with(|| b.inject_sensor_stuck.cmp(&a.inject_sensor_stuck))
            });
        }
        ScenarioCoverageMode::RiskFirst => {
            combos.sort_by(|a, b| {
                risk_score(b)
                    .cmp(&risk_score(a))
                    .then_with(|| a.duration_ms.cmp(&b.duration_ms))
                    .then_with(|| b.inject_sensor_stuck.cmp(&a.inject_sensor_stuck))
                    .then_with(|| b.start_pulse_ms.cmp(&a.start_pulse_ms))
                    .then_with(|| b.sensor_window_ms.cmp(&a.sensor_window_ms))
            });
        }
    }

    combos
}

fn round_up_to_tick(ms: u64, tick_ms: u64) -> u64 {
    if tick_ms == 0 {
        return ms;
    }
    ((ms + tick_ms - 1) / tick_ms) * tick_ms
}

fn round_down_to_tick(ms: u64, tick_ms: u64) -> u64 {
    if tick_ms == 0 {
        return ms;
    }
    (ms / tick_ms) * tick_ms
}

fn discover_start_and_sensor_ids(hints: &ScenarioInitInputHints) -> (Option<u16>, Vec<u16>) {
    let start_id = hints.digital_aliases.iter().find_map(|(&id, aliases)| {
        if aliases_contain_keyword(aliases, "start") {
            Some(id)
        } else {
            None
        }
    });
    let mut sensor_ids = hints
        .digital_aliases
        .iter()
        .filter_map(|(&id, aliases)| {
            if aliases_contain_keyword(aliases, "sensor") {
                Some(id)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    sensor_ids.sort_unstable();
    sensor_ids.dedup();
    (start_id, sensor_ids)
}

pub(crate) fn run_scenario_gen_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "scenario-gen");
    let mut plc_path: Option<PathBuf> = None;
    let mut config_path: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut coverage_mode = ScenarioCoverageMode::Pairwise;
    let mut dry_run = false;
    let mut template_library_path: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--plc" => {
                plc_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --plc <file.plc>".to_string()
                    })?));
            }
            "--config" => {
                config_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --config <gen.yaml>".to_string()
                    })?));
            }
            "--out-dir" => {
                out_dir =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --out-dir <dir>".to_string()
                    })?));
            }
            "--coverage-mode" => {
                let raw = args.next().ok_or_else(|| {
                    "Missing value for --coverage-mode <pairwise|boundary-first|risk-first>"
                        .to_string()
                })?;
                coverage_mode = ScenarioCoverageMode::parse(&raw)?;
            }
            "--dry-run" => {
                dry_run = true;
            }
            "--template-library" => {
                template_library_path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --template-library <metadata.json>".to_string()
                })?));
            }
            "-h" | "--help" => {
                return Err(usage.clone());
            }
            other => return Err(format!("Unknown argument for scenario-gen: {other}")),
        }
    }

    let plc_path = plc_path.ok_or_else(|| usage.clone())?;
    let config_path = config_path.ok_or_else(|| usage.clone())?;
    let out_dir = out_dir.ok_or_else(|| usage.clone())?;
    let template_library_path =
        template_library_path.unwrap_or_else(scenario_gen_default_template_library_path);

    let plc_source = fs::read_to_string(&plc_path)
        .map_err(|err| format!("Failed to read {}: {err}", plc_path.display()))?;
    let config_yaml = fs::read_to_string(&config_path)
        .map_err(|err| format!("Failed to read {}: {err}", config_path.display()))?;
    let config: ScenarioGenConfig = serde_yaml::from_str(&config_yaml).map_err(|err| {
        format!(
            "Failed to parse scenario-gen config {}: {err}",
            config_path.display()
        )
    })?;
    config.validate()?;
    let template_library = load_scenario_template_library(&template_library_path)?;

    fs::create_dir_all(&out_dir).map_err(|err| {
        format!(
            "Failed to create output directory {}: {err}",
            out_dir.display()
        )
    })?;

    let hints = collect_scenario_init_hints(&plc_source)?;
    let (start_id, sensor_ids) = discover_start_and_sensor_ids(&hints);

    let seed_base = config.seed_base_value();
    let durations = config.duration_values();
    let start_pulses = config.start_pulse_values();
    let sensor_windows = config.sensor_window_values();
    let fault_values = config.fault_values();
    let combos = build_scenario_gen_combos(
        &durations,
        &start_pulses,
        &sensor_windows,
        &fault_values,
        coverage_mode,
    );

    let mut cases = Vec::<ScenarioGenCase>::new();
    for (case_idx, combo) in combos.into_iter().take(config.max_cases).enumerate() {
        let duration = combo.duration_ms;
        let pulse = combo.start_pulse_ms;
        let window = combo.sensor_window_ms;
        let inject_fault = combo.inject_sensor_stuck;

        let mut inputs = Vec::<sim::InputEvent>::new();
        let primary_start = start_id.or_else(|| hints.digital_ids.first().copied());
        if let Some(start) = primary_start {
            let mut set = sim::InputSet::default();
            set.digital_inputs.insert(start, true);
            inputs.push(sim::InputEvent { at_ms: 0, set });

            let mut release_ms = round_up_to_tick(pulse.max(config.tick_ms), config.tick_ms);
            if release_ms >= duration {
                let latest = duration.saturating_sub(config.tick_ms);
                release_ms = round_down_to_tick(latest, config.tick_ms);
            }
            if release_ms > 0 && release_ms < duration {
                let mut set = sim::InputSet::default();
                set.digital_inputs.insert(start, false);
                inputs.push(sim::InputEvent {
                    at_ms: release_ms,
                    set,
                });
            }
        }

        let sensor_spacing = round_up_to_tick(window.max(config.tick_ms), config.tick_ms);
        if !sensor_ids.is_empty() {
            let sensor_targets = sensor_ids
                .iter()
                .copied()
                .filter(|id| Some(*id) != start_id)
                .take(8)
                .collect::<Vec<_>>();
            if !sensor_targets.is_empty() {
                let mut baseline = sim::InputSet::default();
                for id in &sensor_targets {
                    baseline.digital_inputs.insert(*id, false);
                }
                inputs.push(sim::InputEvent {
                    at_ms: 0,
                    set: baseline,
                });

                let sensor_start =
                    round_up_to_tick(pulse.saturating_add(sensor_spacing), config.tick_ms);
                let mut at_ms = sensor_start.max(config.tick_ms);
                for id in sensor_targets {
                    if at_ms >= duration {
                        break;
                    }
                    let mut set = sim::InputSet::default();
                    set.digital_inputs.insert(id, true);
                    inputs.push(sim::InputEvent { at_ms, set });
                    at_ms = at_ms.saturating_add(sensor_spacing);
                }
            }
        }

        inputs.sort_by_key(|e| e.at_ms);

        let faults = if inject_fault {
            let target = sensor_ids
                .first()
                .copied()
                .or(start_id)
                .or_else(|| hints.digital_ids.first().copied())
                .unwrap_or(0);
            let latest = duration.saturating_sub(config.tick_ms);
            let mut at_ms = round_up_to_tick(200, config.tick_ms);
            if at_ms >= duration {
                at_ms = round_down_to_tick(latest, config.tick_ms);
            }
            vec![sim::FaultEvent {
                sensor_stuck: sim::SensorStuckFault {
                    at_ms,
                    target,
                    value: true,
                },
            }]
        } else {
            Vec::new()
        };

        let scenario = sim::Scenario {
            seed: Some(seed_base + case_idx as u64),
            tick_ms: config.tick_ms,
            duration_ms: duration,
            inputs,
            digital_bursts: Vec::new(),
            faults,
            forces: Vec::new(),
        };
        let mut io = sim::SimIo::new(32, 32, 8, 8);
        scenario.apply_to_simio(&mut io).map_err(|e| {
            format!(
                "Generated scenario failed validation (duration_ms={duration}, start_pulse_ms={pulse}, sensor_window_ms={window}, inject_sensor_stuck={inject_fault}): {e}"
            )
        })?;

        let name = format!("scenario_{:04}.yaml", case_idx + 1);
        let path = out_dir.join(&name);
        if !dry_run {
            let mut yaml = serde_yaml::to_string(&scenario)
                .map_err(|err| format!("Failed to serialize generated scenario: {err}"))?;
            if !yaml.ends_with('\n') {
                yaml.push('\n');
            }
            fs::write(&path, yaml)
                .map_err(|err| format!("Failed to write {}: {err}", path.display()))?;
        }

        cases.push(ScenarioGenCase {
            name: format!("case_{:04}", case_idx + 1),
            path: name,
            seed: scenario.seed,
            duration_ms: duration,
            start_pulse_ms: pulse,
            sensor_window_ms: window,
            inject_sensor_stuck: inject_fault,
            template_id: select_template_id(&combo, &template_library),
        });
    }

    let summary = ScenarioGenSummary {
        schema_version: 1,
        plc: display_path_relative_to_cwd(&plc_path),
        config: display_path_relative_to_cwd(&config_path),
        coverage_mode: coverage_mode.as_str().to_string(),
        dry_run,
        template_library: display_path_relative_to_cwd(&template_library_path),
        count: cases.len(),
        templates: template_library.templates.clone(),
        cases,
    };
    let summary_path = out_dir.join("summary.json");
    let mut json = serde_json::to_string_pretty(&summary)
        .map_err(|err| format!("Failed to serialize summary JSON: {err}"))?;
    json.push('\n');
    fs::write(&summary_path, json)
        .map_err(|err| format!("Failed to write {}: {err}", summary_path.display()))?;

    if dry_run {
        eprintln!(
            "scenario-gen: dry-run planned {} scenarios under {} (summary: {})",
            summary.count,
            out_dir.display(),
            summary_path.display()
        );
    } else {
        eprintln!(
            "scenario-gen: wrote {} scenarios under {} (summary: {})",
            summary.count,
            out_dir.display(),
            summary_path.display()
        );
    }
    Ok(())
}
