use crate::cli::shared::compile_pipeline::{
    RuntimeBudgetSummary, compile_pipeline, write_verification_report,
};
use crate::cli_support::common::{CliOutputMode, DispatchResult, display_path_relative_to_cwd};
use crate::cli_support::diagnostics_common::evidence_source_label;
use crate::cli_support::help::command_usage;
use crate::cli_support::plc_pipeline::{
    compile_plc_to_runtime_program, format_loaded_plc_errors,
    parse_loaded_plc_with_required_purpose,
};
use crate::cli_support::runtime_probe::{io_sizes_for_program_and_scenario, is_halted};
use crate::cli_support::scenario_yaml::{
    format_resolve_scenario_yaml_error, parse_scenario_yaml, read_scenario_yaml_file,
    scenario_mismatch_hint_for_example,
};
use io_traits::Io;
use runtime_core::{Action, Instr, Program};
use rust_plc::diagnostics::{DiagnosisInput, EvidenceSource, diagnose};
use rust_plc::io_map::{IoMap, IoMapError, IoUsage};
use rust_plc::runtime_bridge::state_machine_to_runtime_program;
use rust_plc::scenario_resolve::resolve_scenario_yaml_for_plc;
use rust_plc::semantic::preprocess_program;
use rust_plc::source_bundle::{LoadedPlcSource, is_supported_plc_source_path, load_plc_source};
use rust_plc::tick_timing::{TickTimingSample, parse_tick_timing_jsonl, to_tick_timing_jsonl};
use rust_plc::timing_report::build_timing_report;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use time::format_description::well_known::Rfc3339;

pub(super) fn try_dispatch(
    program: &str,
    command: &str,
    remaining: &[String],
) -> Option<DispatchResult> {
    let (error_prefix, result) = match command {
        "build-rp2040" => (
            Some("[BLD-000]"),
            run_build_rp2040_subcommand(program, remaining.iter().cloned()),
        ),
        "release-bundle" => (
            None,
            run_release_bundle_subcommand(program, remaining.iter().cloned()),
        ),
        "flash-rp2040" => (
            None,
            run_flash_rp2040_subcommand(program, remaining.iter().cloned()),
        ),
        "board-parse" => (
            None,
            run_board_parse_subcommand(program, remaining.iter().cloned()),
        ),
        "no-board-gate" => (
            Some("[GATE-000]"),
            run_no_board_gate_subcommand(program, remaining.iter().cloned()),
        ),
        "commissioning-run" => (
            None,
            run_commissioning_run_subcommand(program, remaining.iter().cloned()),
        ),
        "pil-run" => (
            None,
            run_pil_run_subcommand(program, remaining.iter().cloned()),
        ),
        "virtual-board" => (
            None,
            run_virtual_board_subcommand(program, remaining.iter().cloned()),
        ),
        _ => return None,
    };
    Some(DispatchResult {
        error_prefix,
        result,
    })
}

#[derive(Debug, Serialize)]
struct BuildMeta<'a> {
    plc_sha256: &'a str,
    generated_at: &'a str,
    tool_version: &'a str,
    runtime_semver: &'a str,
    git_commit: &'a str,
    git_dirty: bool,
    runtime_budget: RuntimeBudgetSummary,
    realtime_profile: RealtimeProfile,
    #[serde(skip_serializing_if = "Option::is_none")]
    io_map: Option<IoMap>,
}

#[derive(Debug, Clone, Serialize)]
struct RealtimeProfile {
    tick_ms: u64,
    thresholds: RealtimeThresholdConfig,
    overrun_count: u64,
    p99_exec_us: u64,
}

#[derive(Debug, Clone, Serialize)]
struct RealtimeThresholdConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    max_p99_exec_us: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_overrun_count: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
struct GateSummary {
    schema_version: u32,
    trace_match: bool,
    realtime_pass: bool,
    passed: bool,
    p99_exec_us: u64,
    overrun_count: u64,
    thresholds: RealtimeThresholdConfig,
    reasons: Vec<String>,
}

#[derive(Debug, Clone)]
struct GitMetadata {
    commit: String,
    dirty: bool,
}

#[derive(Debug, Serialize)]
struct ReleaseBundleManifest<'a> {
    schema_version: u32,
    tool_version: &'a str,
    generated_at: &'a str,
    git_commit: &'a str,
    git_dirty: bool,
    artifacts: Vec<ReleaseBundleArtifact>,
}

#[derive(Debug, Serialize)]
struct ReleaseBundleArtifact {
    path: String,
    sha256: String,
    size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
struct AnalogContract {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    analog_inputs: BTreeMap<String, AnalogInputContractEntry>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    analog_outputs: BTreeMap<String, AnalogOutputContractEntry>,
}

#[derive(Debug, Clone, Serialize)]
struct AnalogInputContractEntry {
    min: f32,
    max: f32,
    scale: f32,
    offset: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    unit: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct AnalogOutputContractEntry {
    min: f32,
    max: f32,
    ramp_ms: u64,
    scale: f32,
    offset: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    unit: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct AnalogCalibrationFile {
    #[serde(default)]
    analog_inputs: BTreeMap<String, AnalogCalibrationEntry>,
    #[serde(default)]
    analog_outputs: BTreeMap<String, AnalogCalibrationEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct AnalogCalibrationEntry {
    #[serde(default = "default_calibration_scale")]
    scale: f32,
    #[serde(default)]
    offset: f32,
}

fn default_calibration_scale() -> f32 {
    1.0
}

fn run_build_rp2040_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "build-rp2040");
    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut out_dir: Option<PathBuf> = None;
    let mut io_map_path: Option<PathBuf> = None;
    let mut analog_calibration_path: Option<PathBuf> = None;
    let mut emit_uf2: Option<PathBuf> = None;
    let mut output_mode = CliOutputMode::Human;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => {
                out_dir =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --out <dir>".to_string()
                    })?));
            }
            "--io-map" => {
                io_map_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --io-map <file>".to_string()
                    })?));
            }
            "--analog-calibration" => {
                analog_calibration_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --analog-calibration <file>".to_string()
                    })?));
            }
            "--emit-uf2" => {
                emit_uf2 =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --emit-uf2 <file.uf2>".to_string()
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
            other => return Err(format!("Unknown argument for build-rp2040: {other}")),
        }
    }

    let out_dir = out_dir.ok_or_else(|| usage.clone())?;
    fs::create_dir_all(&out_dir)
        .map_err(|err| format!("Failed to create out dir {out_dir:?}: {err}"))?;

    if !is_supported_plc_source_path(Path::new(&plc_path)) {
        return Err(format!(
            "Expected a .plc or .bundle.toml path, got: {plc_path}"
        ));
    }

    let loaded = load_plc_source(Path::new(&plc_path))?;
    let plc_source = loaded.source.clone();
    let plc_bytes = plc_source.as_bytes().to_vec();

    let sha256 = {
        let mut h = Sha256::new();
        h.update(&plc_bytes);
        hex::encode(h.finalize())
    };

    let ir_bundle = compile_pipeline(&loaded).map_err(|errors| errors.join("\n\n"))?;

    // For build artifacts we use 1ms ticks so ms-based DSL durations are always aligned.
    let runtime_program = state_machine_to_runtime_program(
        &ir_bundle.topology,
        &ir_bundle.constraints,
        &ir_bundle.state_machine,
        1,
    )
    .map_err(|err| format!("Failed to bridge to runtime Program: {err}"))?;

    let usage = io_usage_for_program(&runtime_program);
    let io_map = match io_map_path.as_ref() {
        None => None,
        Some(path) => {
            let toml_str = fs::read_to_string(&path)
                .map_err(|err| format!("Failed to read io map {path:?}: {err}"))?;
            let m = IoMap::from_toml_str(&toml_str)
                .map_err(|err| format!("Failed to parse io map TOML: {err}"))?;
            match m.validate_for_usage(usage) {
                Ok(()) => {}
                Err(IoMapError::MissingRequired { kind, id }) => {
                    return Err(format!(
                        "Invalid io map for this program: missing required mapping for {kind}{id}\n\
\n\
hint: the io map must contain a GPIO assignment for every DI/DO/AI/AO used by the program.\n\
Start from the generated `io_map.template.toml` under `--out <dir>` and fill in GPIO numbers."
                    ));
                }
                Err(err) => {
                    return Err(format!("Invalid io map for this program: {err}"));
                }
            }
            Some(m)
        }
    };

    let generated_src = codegen::generate_program_module(&runtime_program, "generated")
        .map_err(|err| format!("Codegen failed: {err:?}"))?;

    let mut generated_src = generated_src;
    if !generated_src.ends_with('\n') {
        generated_src.push('\n');
    }

    let generated_path = out_dir.join("generated_program.rs");
    fs::write(&generated_path, generated_src)
        .map_err(|err| format!("Failed to write {generated_path:?}: {err}"))?;

    let iomap_path = out_dir.join("io_map.template.toml");
    let iomap = io_map_template_for_program(&runtime_program);
    fs::write(&iomap_path, iomap)
        .map_err(|err| format!("Failed to write {iomap_path:?}: {err}"))?;

    let mut analog_contract = build_analog_contract(&loaded)?;
    if let Some(path) = analog_calibration_path.as_ref() {
        let calibration_toml = fs::read_to_string(path)
            .map_err(|err| format!("Failed to read analog calibration file {path:?}: {err}"))?;
        apply_analog_calibration(&mut analog_contract, &calibration_toml)?;
    }
    let analog_contract_toml = toml::to_string_pretty(&analog_contract)
        .map_err(|err| format!("Failed to serialize analog contract TOML: {err}"))?;
    let analog_contract_path = out_dir.join("analog_contract.toml");
    fs::write(&analog_contract_path, analog_contract_toml)
        .map_err(|err| format!("Failed to write {analog_contract_path:?}: {err}"))?;

    let analog_cal_template_path = out_dir.join("analog_calibration.template.toml");
    let analog_cal_template = analog_calibration_template_for_contract(&analog_contract);
    fs::write(&analog_cal_template_path, analog_cal_template)
        .map_err(|err| format!("Failed to write {analog_cal_template_path:?}: {err}"))?;

    let generated_at = time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string());
    let git_metadata = detect_git_metadata();

    let meta = BuildMeta {
        plc_sha256: &sha256,
        generated_at: &generated_at,
        tool_version: env!("CARGO_PKG_VERSION"),
        runtime_semver: runtime_core::VERSION,
        git_commit: &git_metadata.commit,
        git_dirty: git_metadata.dirty,
        runtime_budget: ir_bundle.runtime_budget.summary(),
        realtime_profile: RealtimeProfile {
            tick_ms: 1,
            thresholds: RealtimeThresholdConfig {
                max_p99_exec_us: None,
                max_overrun_count: None,
            },
            overrun_count: 0,
            p99_exec_us: 0,
        },
        io_map,
    };
    let mut meta_json = serde_json::to_string_pretty(&meta)
        .map_err(|err| format!("Failed to serialize build_meta.json: {err}"))?;
    meta_json.push('\n');
    let meta_path = out_dir.join("build_meta.json");
    fs::write(&meta_path, meta_json)
        .map_err(|err| format!("Failed to write {meta_path:?}: {err}"))?;

    if let Some(uf2_path) = emit_uf2 {
        let io_map_path = io_map_path.as_ref().ok_or_else(|| {
            "--emit-uf2 requires --io-map <file> so board pin mapping is explicit".to_string()
        })?;
        emit_rp2040_uf2(
            &generated_path,
            io_map_path,
            &analog_contract_path,
            &uf2_path,
        )?;
    }

    if output_mode == CliOutputMode::Json {
        #[derive(Serialize)]
        struct BuildRp2040Json {
            schema_version: u32,
            command: &'static str,
            output: &'static str,
            status: &'static str,
            out_dir: String,
            artifacts: BTreeMap<&'static str, String>,
        }
        let mut artifacts = BTreeMap::<&'static str, String>::new();
        artifacts.insert(
            "generated_program",
            display_path_relative_to_cwd(&generated_path),
        );
        artifacts.insert("io_map_template", display_path_relative_to_cwd(&iomap_path));
        artifacts.insert(
            "analog_contract",
            display_path_relative_to_cwd(&analog_contract_path),
        );
        artifacts.insert(
            "analog_calibration_template",
            display_path_relative_to_cwd(&analog_cal_template_path),
        );
        artifacts.insert("build_meta", display_path_relative_to_cwd(&meta_path));
        let payload = BuildRp2040Json {
            schema_version: 1,
            command: "build-rp2040",
            output: output_mode.as_str(),
            status: "pass",
            out_dir: display_path_relative_to_cwd(&out_dir),
            artifacts,
        };
        let mut json = serde_json::to_string_pretty(&payload)
            .map_err(|err| format!("Failed to serialize build-rp2040 JSON output: {err}"))?;
        json.push('\n');
        print!("{json}");
    }

    Ok(())
}

fn run_release_bundle_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "release-bundle");
    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut scenario_path: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut io_map_path: Option<PathBuf> = None;
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
            "--io-map" => {
                io_map_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --io-map <file>".to_string()
                    })?));
            }
            "--max-p99-exec-us" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --max-p99-exec-us <us>".to_string())?;
                max_p99_exec_us = Some(raw.parse::<u64>().map_err(|_| {
                    format!("Invalid --max-p99-exec-us value (expected u64): {raw}")
                })?);
            }
            "--max-overrun-count" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --max-overrun-count <n>".to_string())?;
                max_overrun_count = Some(raw.parse::<u64>().map_err(|_| {
                    format!("Invalid --max-overrun-count value (expected u64): {raw}")
                })?);
            }
            "-h" | "--help" => {
                return Err(usage.clone());
            }
            other => return Err(format!("Unknown argument for release-bundle: {other}")),
        }
    }

    let scenario_path = scenario_path.ok_or_else(|| usage.clone())?;
    let out_dir = out_dir.ok_or_else(|| usage.clone())?;

    fs::create_dir_all(&out_dir)
        .map_err(|err| format!("Failed to create out dir {out_dir:?}: {err}"))?;

    if !is_supported_plc_source_path(Path::new(&plc_path)) {
        return Err(format!(
            "Expected a .plc or .bundle.toml path, got: {plc_path}"
        ));
    }

    let loaded = load_plc_source(Path::new(&plc_path))?;
    let plc_source = loaded.source.clone();
    let plc_bytes = plc_source.as_bytes().to_vec();

    let plc_sha256 = sha256_hex(&plc_bytes);

    let scenario_yaml = read_scenario_yaml_file(&scenario_path)?;
    let scenario_yaml =
        resolve_scenario_yaml_for_plc(&plc_source, &scenario_yaml).map_err(|e| {
            format_resolve_scenario_yaml_error(&plc_path, &scenario_path, "release-bundle", &e)
        })?;
    let scenario = parse_scenario_yaml(&scenario_yaml)?;

    let ir_bundle = compile_pipeline(&loaded).map_err(|errors| errors.join("\n\n"))?;

    // Board-oriented program generation uses 1ms ticks to align with firmware build artifacts.
    let board_program = state_machine_to_runtime_program(
        &ir_bundle.topology,
        &ir_bundle.constraints,
        &ir_bundle.state_machine,
        1,
    )
    .map_err(|err| format!("Failed to bridge to runtime Program: {err}"))?;

    let usage = io_usage_for_program(&board_program);
    let io_map = match io_map_path.as_ref() {
        None => None,
        Some(path) => {
            let toml_str = fs::read_to_string(&path)
                .map_err(|err| format!("Failed to read io map {path:?}: {err}"))?;
            let m = IoMap::from_toml_str(&toml_str)
                .map_err(|err| format!("Failed to parse io map TOML: {err}"))?;
            match m.validate_for_usage(usage) {
                Ok(()) => {}
                Err(IoMapError::MissingRequired { kind, id }) => {
                    return Err(format!(
                        "Invalid io map for this program: missing required mapping for {kind}{id}\n\
\n\
hint: the io map must contain a GPIO assignment for every DI/DO/AI/AO used by the program.\n\
Start from the generated `io_map.template.toml` under `--out-dir <dir>` and fill in GPIO numbers."
                    ));
                }
                Err(err) => {
                    return Err(format!("Invalid io map for this program: {err}"));
                }
            }
            Some(m)
        }
    };

    // Write/copy core bundle artifacts.
    let bundled_plc_path = out_dir.join("program.plc");
    fs::write(&bundled_plc_path, &plc_bytes)
        .map_err(|err| format!("Failed to write {bundled_plc_path:?}: {err}"))?;

    let bundled_scenario_path = out_dir.join("scenario.yaml");
    fs::write(&bundled_scenario_path, &scenario_yaml)
        .map_err(|err| format!("Failed to write {bundled_scenario_path:?}: {err}"))?;

    let io_map_template_path = out_dir.join("io_map.template.toml");
    let io_map_template = io_map_template_for_program(&board_program);
    fs::write(&io_map_template_path, &io_map_template)
        .map_err(|err| format!("Failed to write {io_map_template_path:?}: {err}"))?;

    // Always include an io_map file in the bundle: either the user-provided map or a template.
    let bundled_io_map_path = out_dir.join("io_map.toml");
    if let Some(src) = io_map_path.as_ref() {
        fs::copy(src, &bundled_io_map_path).map_err(|err| {
            format!("Failed to copy io map {src:?} -> {bundled_io_map_path:?}: {err}")
        })?;
    } else {
        fs::write(&bundled_io_map_path, &io_map_template)
            .map_err(|err| format!("Failed to write {bundled_io_map_path:?}: {err}"))?;
    }

    let generated_program_path = out_dir.join("generated_program.rs");
    let mut generated_src = codegen::generate_program_module(&board_program, "generated")
        .map_err(|err| format!("Codegen failed: {err:?}"))?;
    if !generated_src.ends_with('\n') {
        generated_src.push('\n');
    }
    fs::write(&generated_program_path, generated_src)
        .map_err(|err| format!("Failed to write {generated_program_path:?}: {err}"))?;

    let verification_report_path = out_dir.join("verification_report.json");
    let plc_path_text = PathBuf::from(&plc_path).to_string_lossy().to_string();
    write_verification_report(
        &plc_path_text,
        &verification_report_path,
        &ir_bundle.runtime_budget,
        &ir_bundle.verification,
    )?;

    // SIL artifacts for trace/report packaging.
    let sil_program = state_machine_to_runtime_program(
        &ir_bundle.topology,
        &ir_bundle.constraints,
        &ir_bundle.state_machine,
        scenario.tick_ms,
    )
    .map_err(|err| format!("Failed to bridge to SIL runtime Program: {err}"))?;
    let sil_trace_path = out_dir.join("sil_trace.jsonl");
    let sim_report_path = out_dir.join("sim_report.json");
    let (num_di, num_do, num_ai, num_ao) =
        io_sizes_for_program_and_scenario(&sil_program, &scenario);
    let mut io = sim::SimIo::new(num_di, num_do, num_ai, num_ao);
    let run = sim::run_program_for_scenario(&sil_program, &scenario, &mut io)
        .map_err(|err| format!("SIL simulation failed: {err}"))?;
    fs::write(&sil_trace_path, run.trace.into_string())
        .map_err(|err| format!("Failed to write trace file {sil_trace_path:?}: {err}"))?;
    let mut sim_report_json = serde_json::to_string_pretty(&run.report)
        .map_err(|err| format!("Failed to serialize sim report JSON: {err}"))?;
    sim_report_json.push('\n');
    fs::write(&sim_report_path, sim_report_json)
        .map_err(|err| format!("Failed to write sim report {sim_report_path:?}: {err}"))?;

    let (_board_log_path, board_trace_path, _board_meta_path, tick_timing_path) =
        write_virtual_board_artifacts(
            Path::new(&plc_path),
            &scenario_path,
            &sil_program,
            &scenario,
            &out_dir,
        )?;

    let board_trace_text = fs::read_to_string(&board_trace_path)
        .map_err(|err| format!("Failed to read board trace {board_trace_path:?}: {err}"))?;
    let sil_trace_text = fs::read_to_string(&sil_trace_path)
        .map_err(|err| format!("Failed to read SIL trace {sil_trace_path:?}: {err}"))?;
    let sil_events = rust_plc::trace_diff::parse_trace_jsonl(&sil_trace_text)
        .map_err(|err| format!("Failed to parse SIL trace JSONL: {err}"))?;
    let board_events = rust_plc::trace_diff::parse_trace_jsonl(&board_trace_text)
        .map_err(|err| format!("Failed to parse board trace JSONL: {err}"))?;
    let diff_report = rust_plc::trace_diff::diff_traces(&sil_events, &board_events, 3);
    let diff_report_path = out_dir.join("diff_report.json");
    let mut diff_json = serde_json::to_string_pretty(&diff_report)
        .map_err(|err| format!("Failed to serialize diff report JSON: {err}"))?;
    diff_json.push('\n');
    fs::write(&diff_report_path, diff_json)
        .map_err(|err| format!("Failed to write diff report {diff_report_path:?}: {err}"))?;

    let tick_timing_text = fs::read_to_string(&tick_timing_path)
        .map_err(|err| format!("Failed to read tick timing {tick_timing_path:?}: {err}"))?;
    let tick_timing_rows = parse_tick_timing_jsonl(&tick_timing_text)
        .map_err(|err| format!("Failed to parse tick timing JSONL: {err}"))?;
    let timing_report = build_timing_report(&tick_timing_rows)
        .ok_or_else(|| "tick_timing.jsonl is empty; cannot build timing report".to_string())?;
    let timing_report_path = out_dir.join("timing_report.json");
    let mut timing_json = serde_json::to_string_pretty(&timing_report)
        .map_err(|err| format!("Failed to serialize timing report JSON: {err}"))?;
    timing_json.push('\n');
    fs::write(&timing_report_path, timing_json)
        .map_err(|err| format!("Failed to write timing report {timing_report_path:?}: {err}"))?;

    let mut gate_reasons = Vec::new();
    let mut realtime_pass = true;
    if !diff_report.is_match {
        gate_reasons.push(format!(
            "trace mismatch (tick={:?}, type={:?}, index={:?})",
            diff_report.first_mismatch_tick, diff_report.mismatch_type, diff_report.mismatch_index
        ));
    }
    if let Some(limit) = max_p99_exec_us {
        if timing_report.exec_us_p99 > limit {
            realtime_pass = false;
            gate_reasons.push(format!(
                "p99 exec_us={} exceeds threshold {}",
                timing_report.exec_us_p99, limit
            ));
        }
    }
    if let Some(limit) = max_overrun_count {
        if timing_report.overrun_count > limit {
            realtime_pass = false;
            gate_reasons.push(format!(
                "overrun_count={} exceeds threshold {}",
                timing_report.overrun_count, limit
            ));
        }
    }
    let gate_summary = GateSummary {
        schema_version: 1,
        trace_match: diff_report.is_match,
        realtime_pass,
        passed: gate_reasons.is_empty(),
        p99_exec_us: timing_report.exec_us_p99,
        overrun_count: timing_report.overrun_count,
        thresholds: RealtimeThresholdConfig {
            max_p99_exec_us,
            max_overrun_count,
        },
        reasons: gate_reasons,
    };
    let gate_summary_path = out_dir.join("gate_summary.json");
    let mut gate_summary_json = serde_json::to_string_pretty(&gate_summary)
        .map_err(|err| format!("Failed to serialize gate summary JSON: {err}"))?;
    gate_summary_json.push('\n');
    fs::write(&gate_summary_path, gate_summary_json)
        .map_err(|err| format!("Failed to write gate summary {gate_summary_path:?}: {err}"))?;

    let generated_at = time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string());
    let git_metadata = detect_git_metadata();

    let build_meta_path = out_dir.join("build_meta.json");
    let meta = BuildMeta {
        plc_sha256: &plc_sha256,
        generated_at: &generated_at,
        tool_version: env!("CARGO_PKG_VERSION"),
        runtime_semver: runtime_core::VERSION,
        git_commit: &git_metadata.commit,
        git_dirty: git_metadata.dirty,
        runtime_budget: ir_bundle.runtime_budget.summary(),
        realtime_profile: RealtimeProfile {
            tick_ms: scenario.tick_ms,
            thresholds: RealtimeThresholdConfig {
                max_p99_exec_us,
                max_overrun_count,
            },
            overrun_count: timing_report.overrun_count,
            p99_exec_us: timing_report.exec_us_p99,
        },
        io_map,
    };
    let mut meta_json = serde_json::to_string_pretty(&meta)
        .map_err(|err| format!("Failed to serialize build_meta.json: {err}"))?;
    meta_json.push('\n');
    fs::write(&build_meta_path, meta_json)
        .map_err(|err| format!("Failed to write {build_meta_path:?}: {err}"))?;

    let manifest_path = out_dir.join("manifest.json");
    let mut artifact_paths: Vec<PathBuf> = vec![
        bundled_plc_path,
        bundled_scenario_path,
        bundled_io_map_path,
        io_map_template_path,
        generated_program_path,
        verification_report_path,
        sil_trace_path,
        sim_report_path,
        tick_timing_path,
        timing_report_path,
        gate_summary_path,
        diff_report_path,
        build_meta_path,
    ];
    artifact_paths.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

    let mut artifacts = Vec::new();
    for p in &artifact_paths {
        let rel = p
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("Non-utf8 artifact filename: {p:?}"))?
            .to_string();
        let (sha, size) = sha256_file(p)?;
        artifacts.push(ReleaseBundleArtifact {
            path: rel,
            sha256: sha,
            size_bytes: size,
        });
    }

    let manifest = ReleaseBundleManifest {
        schema_version: 1,
        tool_version: env!("CARGO_PKG_VERSION"),
        generated_at: &generated_at,
        git_commit: &git_metadata.commit,
        git_dirty: git_metadata.dirty,
        artifacts,
    };
    let mut manifest_json = serde_json::to_string_pretty(&manifest)
        .map_err(|err| format!("Failed to serialize manifest.json: {err}"))?;
    manifest_json.push('\n');
    fs::write(&manifest_path, manifest_json)
        .map_err(|err| format!("Failed to write {manifest_path:?}: {err}"))?;

    Ok(())
}

/// Build a `Command` for an external tool binary.
/// On Windows, `.bat` and `.ps1` files cannot be spawned directly:
///   - `.bat` 鈫?`cmd /C <path>`
///   - `.ps1` 鈫?`powershell -NonInteractive -File <path>`
/// This wrapper handles that transparently so callers can pass the raw path
/// from an environment variable.
fn tool_command(bin: &str) -> std::process::Command {
    #[cfg(windows)]
    {
        let lower = bin.to_ascii_lowercase();
        if lower.ends_with(".bat") {
            let mut cmd = std::process::Command::new("cmd");
            cmd.arg("/C").arg(bin);
            return cmd;
        }
        if lower.ends_with(".ps1") {
            let mut cmd = std::process::Command::new("powershell");
            cmd.arg("-NonInteractive").arg("-File").arg(bin);
            return cmd;
        }
    }
    std::process::Command::new(bin)
}

fn emit_rp2040_uf2(
    generated_program_rs: &Path,
    io_map_toml: &Path,
    analog_contract_toml: &Path,
    uf2_out: &Path,
) -> Result<(), String> {
    let generated_program_rs = absolutize_path(generated_program_rs)?;
    let io_map_toml = absolutize_path(io_map_toml)?;
    let analog_contract_toml = absolutize_path(analog_contract_toml)?;
    let uf2_out = absolutize_path(uf2_out)?;

    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(parent) = uf2_out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to create UF2 output dir {parent:?}: {err}"))?;
        }
    }

    let cargo_bin = env::var("RUST_PLC_CARGO_BIN").unwrap_or_else(|_| "cargo".to_string());
    let elf2uf2_bin = env::var("RUST_PLC_ELF2UF2_BIN").unwrap_or_else(|_| "elf2uf2-rs".to_string());

    let cargo = tool_command(&cargo_bin)
        .current_dir(&repo_root)
        .env("RUST_PLC_GENERATED_PROGRAM_RS", &generated_program_rs)
        .env("RUST_PLC_IO_MAP_TOML", &io_map_toml)
        .env("RUST_PLC_ANALOG_CONTRACT_TOML", &analog_contract_toml)
        .arg("build")
        .arg("-p")
        .arg("board-rp2040")
        .arg("--target")
        .arg("thumbv6m-none-eabi")
        .arg("--release")
        .output()
        .map_err(|err| {
            format!("Failed to run cargo for RP2040 firmware build (bin={cargo_bin}): {err}")
        })?;
    if !cargo.status.success() {
        return Err(format!(
            "RP2040 firmware build failed.\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&cargo.stdout),
            String::from_utf8_lossy(&cargo.stderr)
        ));
    }

    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                repo_root.join(path)
            }
        })
        .unwrap_or_else(|| repo_root.join("target"));
    let elf = target_dir.join("thumbv6m-none-eabi/release/board-rp2040");
    if !elf.exists() {
        return Err(format!(
            "Expected firmware ELF does not exist after build: {elf:?}"
        ));
    }

    let uf2 = tool_command(&elf2uf2_bin)
        .arg(&elf)
        .arg(&uf2_out)
        .output()
        .map_err(|err| {
            format!("Failed to run {elf2uf2_bin} (install with `cargo install elf2uf2-rs`): {err}")
        })?;
    if !uf2.status.success() {
        return Err(format!(
            "UF2 conversion failed.\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&uf2.stdout),
            String::from_utf8_lossy(&uf2.stderr)
        ));
    }

    Ok(())
}

fn build_analog_contract(input: &LoadedPlcSource) -> Result<AnalogContract, String> {
    let parsed = parse_loaded_plc_with_required_purpose(input)
        .map_err(|err| format!("Failed to parse PLC source: {err}"))?;
    let expanded = preprocess_program(&parsed)
        .map_err(|errors| format_loaded_plc_errors(errors, input).join("\n"))?;

    let mut analog_inputs = BTreeMap::<String, AnalogInputContractEntry>::new();
    let mut analog_outputs = BTreeMap::<String, AnalogOutputContractEntry>::new();
    for d in expanded.topology.devices {
        match d.device_type {
            rust_plc::ast::DeviceType::AnalogInput => {
                let Some(id) = parse_prefixed_numeric_id(&d.name, "AI") else {
                    continue;
                };
                let (min, max) = d
                    .attributes
                    .range
                    .map(|r| (r.min as f32, r.max as f32))
                    // Fallback keeps old projects buildable even when range is omitted.
                    .unwrap_or((0.0, 3.3));
                analog_inputs.insert(
                    format!("ai{id}"),
                    AnalogInputContractEntry {
                        min,
                        max,
                        scale: 1.0,
                        offset: 0.0,
                        unit: d.attributes.unit.clone(),
                    },
                );
            }
            rust_plc::ast::DeviceType::AnalogOutput => {
                let Some(id) = parse_prefixed_numeric_id(&d.name, "AO") else {
                    continue;
                };
                let (min, max) = d
                    .attributes
                    .range
                    .map(|r| (r.min as f32, r.max as f32))
                    .unwrap_or((0.0, 10.0));
                let ramp_ms = d
                    .attributes
                    .ramp_time
                    .as_ref()
                    .map(duration_to_ms)
                    .unwrap_or(0);
                analog_outputs.insert(
                    format!("ao{id}"),
                    AnalogOutputContractEntry {
                        min,
                        max,
                        ramp_ms,
                        scale: 1.0,
                        offset: 0.0,
                        unit: d.attributes.unit.clone(),
                    },
                );
            }
            _ => {}
        }
    }

    Ok(AnalogContract {
        analog_inputs,
        analog_outputs,
    })
}

fn parse_prefixed_numeric_id(name: &str, prefix: &str) -> Option<u16> {
    name.strip_prefix(prefix)?.parse::<u16>().ok()
}

fn duration_to_ms(duration: &rust_plc::ast::DurationValue) -> u64 {
    match duration.unit {
        rust_plc::ast::TimeUnit::Ms => duration.value,
        rust_plc::ast::TimeUnit::S => duration.value.saturating_mul(1000),
    }
}

fn apply_analog_calibration(
    contract: &mut AnalogContract,
    calibration_toml: &str,
) -> Result<(), String> {
    let cal: AnalogCalibrationFile =
        toml::from_str(calibration_toml).map_err(|e| format!("Invalid calibration TOML: {e}"))?;

    for (k, v) in &cal.analog_inputs {
        validate_calibration_entry(v, &format!("analog_inputs.{k}"))?;
        let entry = contract.analog_inputs.get_mut(k).ok_or_else(|| {
            format!("analog calibration key not found in contract: analog_inputs.{k}")
        })?;
        entry.scale = v.scale;
        entry.offset = v.offset;
    }
    for (k, v) in &cal.analog_outputs {
        validate_calibration_entry(v, &format!("analog_outputs.{k}"))?;
        let entry = contract.analog_outputs.get_mut(k).ok_or_else(|| {
            format!("analog calibration key not found in contract: analog_outputs.{k}")
        })?;
        entry.scale = v.scale;
        entry.offset = v.offset;
    }
    Ok(())
}

fn validate_calibration_entry(v: &AnalogCalibrationEntry, scope: &str) -> Result<(), String> {
    if !v.scale.is_finite() || v.scale.abs() < 1e-9 {
        return Err(format!("{scope}.scale must be finite and non-zero"));
    }
    if !v.offset.is_finite() {
        return Err(format!("{scope}.offset must be finite"));
    }
    Ok(())
}

fn analog_calibration_template_for_contract(contract: &AnalogContract) -> String {
    let mut out = String::new();
    out.push_str("# Analog calibration template (optional)\n");
    out.push_str("#\n");
    out.push_str("# The firmware applies calibration as:\n");
    out.push_str("#   eng_calibrated = eng_raw * scale + offset\n");
    out.push_str("#\n");
    out.push_str("# Notes:\n");
    out.push_str("# - Keys match analog_contract.toml sections: ai0/ao0/...\n");
    out.push_str("# - Only entries present here override defaults.\n\n");

    if !contract.analog_inputs.is_empty() {
        out.push_str("[analog_inputs]\n");
        for k in contract.analog_inputs.keys() {
            out.push_str(&format!("# {k} = {{ scale = 1.0, offset = 0.0 }}\n"));
        }
        out.push('\n');
    }

    if !contract.analog_outputs.is_empty() {
        out.push_str("[analog_outputs]\n");
        for k in contract.analog_outputs.keys() {
            out.push_str(&format!("# {k} = {{ scale = 1.0, offset = 0.0 }}\n"));
        }
        out.push('\n');
    }

    out
}

fn absolutize_path(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let cwd = env::current_dir().map_err(|err| format!("Failed to read current dir: {err}"))?;
    Ok(cwd.join(path))
}

fn detect_git_metadata() -> GitMetadata {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let commit = std::process::Command::new("git")
        .current_dir(&repo_root)
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
            } else {
                None
            }
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

    let dirty = std::process::Command::new("git")
        .current_dir(&repo_root)
        .arg("status")
        .arg("--porcelain")
        .output()
        .ok()
        .map(|out| out.status.success() && !out.stdout.is_empty())
        .unwrap_or(false);

    GitMetadata { commit, dirty }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

fn sha256_file(path: &Path) -> Result<(String, u64), String> {
    let bytes = fs::read(path).map_err(|err| format!("Failed to read artifact {path:?}: {err}"))?;
    let size = bytes.len() as u64;
    Ok((sha256_hex(&bytes), size))
}

fn io_map_template_for_program(program: &Program<'_>) -> String {
    use std::collections::BTreeSet;

    let mut dis = BTreeSet::<u16>::new();
    let mut dos = BTreeSet::<u16>::new();
    let mut ais = BTreeSet::<u16>::new();
    let mut aos = BTreeSet::<u16>::new();
    for task in program.tasks {
        for step in task.steps {
            match step.instr {
                Instr::WaitAllDigital { conditions, .. } => {
                    for condition in conditions {
                        dis.insert(condition.id.0);
                    }
                }
                Instr::WaitDigital { id, .. } => {
                    dis.insert(id.0);
                }
                Instr::WaitAnalog { id, .. } => {
                    ais.insert(id.0);
                }
                Instr::Action { actions, .. } => {
                    for a in actions {
                        match *a {
                            Action::SetDigital { id, .. } => {
                                dos.insert(id.0);
                            }
                            Action::Extend { output }
                            | Action::Retract { output }
                            | Action::CylinderMotion { output, .. } => {
                                dos.insert(output.0);
                            }
                            Action::SetAnalog { id, .. } => {
                                aos.insert(id.0);
                            }
                            Action::SetAnalogExpr { id, .. } => {
                                aos.insert(id.0);
                            }
                            Action::Compute { .. }
                            | Action::CallExtern { .. }
                            | Action::AxisMove { .. }
                            | Action::CamEngage { .. }
                            | Action::CamDisengage { .. }
                            | Action::CamSwitch { .. }
                            | Action::CamPhase { .. }
                            | Action::WorkpieceAcquire { .. }
                            | Action::WorkpieceTransfer { .. }
                            | Action::WorkpieceFinish { .. }
                            | Action::WorkpieceMount { .. }
                            | Action::WorkpieceUnmount { .. }
                            | Action::WorkpieceTransformCarrier { .. }
                            | Action::WorkpieceSplit { .. }
                            | Action::WorkpieceMerge { .. } => {}
                            Action::Log { .. } => {}
                        }
                    }
                }
                Instr::WaitExpr { .. }
                | Instr::WaitCamDigital { .. }
                | Instr::WaitCamAnalog { .. }
                | Instr::Delay { .. }
                | Instr::Goto { .. }
                | Instr::Halt => {}
            }
        }
    }
    for cam in program.cam_configs {
        ais.insert(cam.master_input.0);
        ais.insert(cam.slave_feedback.0);
        aos.insert(cam.slave_output.0);
    }
    for pid in program.pid_loops {
        ais.insert(pid.pv.0);
        aos.insert(pid.out.0);
    }

    let mut out = String::new();
    out.push_str("# RP2040 I/O map template (fill in GPIO numbers for your wiring)\n");
    out.push_str("# This file is a template; it may be incomplete by design.\n\n");
    out.push_str("# GPIO mapping notes:\n");
    out.push_str("# - DI/DO/AO: 0..=29 or \"virtual\" (no physical GPIO binding)\n");
    out.push_str("# - AI: 26..=29 (ADC-capable) or \"virtual\" (board-provided synthetic)\n\n");

    out.push_str("[digital_inputs]\n");
    if dis.is_empty() {
        out.push_str("# di0 = 2\n");
    } else {
        for id in dis {
            out.push_str(&format!("# di{id} = 2\n"));
        }
    }
    out.push('\n');

    out.push_str("[digital_outputs]\n");
    if dos.is_empty() {
        out.push_str("# do0 = 16\n");
    } else {
        for id in dos {
            out.push_str(&format!("# do{id} = 16\n"));
        }
    }
    out.push('\n');

    out.push_str("[analog_inputs]\n");
    out.push_str("# RP2040 ADC-capable GPIO: 26, 27, 28, 29\n");
    if ais.is_empty() {
        out.push_str("# ai0 = 26\n");
    } else {
        for id in ais {
            out.push_str(&format!("# ai{id} = 26\n"));
        }
    }
    out.push('\n');

    out.push_str("[analog_outputs]\n");
    if aos.is_empty() {
        out.push_str("# ao0 = 26\n");
    } else {
        for id in aos {
            out.push_str(&format!("# ao{id} = 26\n"));
        }
    }

    out.push('\n');
    out.push_str("# Motion (optional): Pulse/Dir stepper + AB encoder (PIO-first).\n");
    out.push_str(
        "# These channels are NOT inferred from the PLC program. Fill in GPIO wiring and\n",
    );
    out.push_str("# axis parameters if you plan to use board-level motion feedback/commands.\n");
    out.push_str("#\n");
    out.push_str("# Note: if you include a [motion] section, it must not be empty.\n");
    out.push_str("#\n");
    out.push_str("# [motion.stepper.axis0]\n");
    out.push_str("# step_gpio = 2\n");
    out.push_str("# dir_gpio = 3\n");
    out.push_str("# en_gpio = 4\n");
    out.push_str("# dir_inverted = false\n");
    out.push_str("# v_max_sps = 20000  # steps per second\n");
    out.push_str("# acc_sps2 = 40000   # steps per second^2\n");
    out.push_str("# dec_sps2 = 40000   # steps per second^2\n");
    out.push_str("#\n");
    out.push_str("# [motion.stepper.axis1]\n");
    out.push_str("# step_gpio = 5\n");
    out.push_str("# dir_gpio = 6\n");
    out.push_str("# en_gpio = 7\n");
    out.push_str("# dir_inverted = false\n");
    out.push_str("# v_max_sps = 20000\n");
    out.push_str("# acc_sps2 = 40000\n");
    out.push_str("# dec_sps2 = 40000\n");
    out.push_str("#\n");
    out.push_str("# [motion.encoder.axis0]\n");
    out.push_str("# a_gpio = 8\n");
    out.push_str("# b_gpio = 9\n");
    out.push_str("# ppr = 1024\n");
    out.push_str("# quad = 4\n");
    out.push_str("# count_sign = \"normal\"  # normal|inverted\n");
    out.push_str("# scale = 1.0\n");
    out.push_str("#\n");
    out.push_str("# [motion.encoder.axis1]\n");
    out.push_str("# a_gpio = 10\n");
    out.push_str("# b_gpio = 11\n");
    out.push_str("# ppr = 1024\n");
    out.push_str("# quad = 4\n");
    out.push_str("# count_sign = \"normal\"\n");
    out.push_str("# scale = 1.0\n");

    out.push('\n');
    out.push_str("[safe_state]\n");
    out.push_str("# Default: all outputs -> 0 on exit (de-energize)\n");
    out.push_str("# mode = \"all_zero\"  # all_zero | profile\n");
    out.push_str("# on_exit_timeout_ms = 300\n");
    out.push_str("#\n");
    out.push_str("# If mode = \"profile\", define per-output safe values and ordering groups.\n");
    out.push_str("# Example (NC brake coil, 0=brake):\n");
    out.push_str("# [safe_state.do.Y2]\n");
    out.push_str("# safe_value = 0\n");
    out.push_str("# group = 10\n");
    out.push_str("#\n");
    out.push_str("# Example (disable stepper enable after brake):\n");
    out.push_str("# [safe_state.do.Y1]\n");
    out.push_str("# safe_value = 0\n");
    out.push_str("# group = 20\n");
    out.push_str("#\n");
    out.push_str("# Example (analog output safe value):\n");
    out.push_str("# [safe_state.ao.AO0]\n");
    out.push_str("# safe_value = 0.0\n");
    out.push_str("# group = 30\n");
    out
}

fn io_usage_for_program(program: &Program<'_>) -> IoUsage {
    use std::collections::BTreeSet;

    let mut dis = BTreeSet::<u16>::new();
    let mut dos = BTreeSet::<u16>::new();
    let mut ais = BTreeSet::<u16>::new();
    let mut aos = BTreeSet::<u16>::new();
    for task in program.tasks {
        for step in task.steps {
            match step.instr {
                Instr::WaitAllDigital { conditions, .. } => {
                    for condition in conditions {
                        dis.insert(condition.id.0);
                    }
                }
                Instr::WaitDigital { id, .. } => {
                    dis.insert(id.0);
                }
                Instr::WaitAnalog { id, .. } => {
                    ais.insert(id.0);
                }
                Instr::Action { actions, .. } => {
                    for a in actions {
                        match *a {
                            Action::SetDigital { id, .. } => {
                                dos.insert(id.0);
                            }
                            Action::Extend { output }
                            | Action::Retract { output }
                            | Action::CylinderMotion { output, .. } => {
                                dos.insert(output.0);
                            }
                            Action::SetAnalog { id, .. } => {
                                aos.insert(id.0);
                            }
                            Action::SetAnalogExpr { id, .. } => {
                                aos.insert(id.0);
                            }
                            Action::Compute { .. }
                            | Action::CallExtern { .. }
                            | Action::AxisMove { .. }
                            | Action::CamEngage { .. }
                            | Action::CamDisengage { .. }
                            | Action::CamSwitch { .. }
                            | Action::CamPhase { .. }
                            | Action::WorkpieceAcquire { .. }
                            | Action::WorkpieceTransfer { .. }
                            | Action::WorkpieceFinish { .. }
                            | Action::WorkpieceMount { .. }
                            | Action::WorkpieceUnmount { .. }
                            | Action::WorkpieceTransformCarrier { .. }
                            | Action::WorkpieceSplit { .. }
                            | Action::WorkpieceMerge { .. } => {}
                            Action::Log { .. } => {}
                        }
                    }
                }
                Instr::WaitExpr { .. }
                | Instr::WaitCamDigital { .. }
                | Instr::WaitCamAnalog { .. }
                | Instr::Delay { .. }
                | Instr::Goto { .. }
                | Instr::Halt => {}
            }
        }
    }
    for cam in program.cam_configs {
        ais.insert(cam.master_input.0);
        ais.insert(cam.slave_feedback.0);
        aos.insert(cam.slave_output.0);
    }
    for pid in program.pid_loops {
        ais.insert(pid.pv.0);
        aos.insert(pid.out.0);
    }

    // `IoUsage` is a tiny borrowed wrapper; we leak the sets to keep build-rp2040 code simple.
    let dis: &'static [u16] = Box::leak(dis.into_iter().collect::<Vec<_>>().into_boxed_slice());
    let dos: &'static [u16] = Box::leak(dos.into_iter().collect::<Vec<_>>().into_boxed_slice());
    let ais: &'static [u16] = Box::leak(ais.into_iter().collect::<Vec<_>>().into_boxed_slice());
    let aos: &'static [u16] = Box::leak(aos.into_iter().collect::<Vec<_>>().into_boxed_slice());
    IoUsage {
        digital_inputs: dis,
        digital_outputs: dos,
        analog_inputs: ais,
        analog_outputs: aos,
    }
}

fn run_flash_rp2040_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "flash-rp2040");
    let mut uf2: Option<PathBuf> = None;
    let mut mount: Option<PathBuf> = None;
    let mut dry_run = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--uf2" => {
                uf2 =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --uf2 <file.uf2>".to_string()
                    })?));
            }
            "--mount" => {
                mount =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --mount <path>".to_string()
                    })?));
            }
            "--dry-run" => dry_run = true,
            "-h" | "--help" => {
                return Err(usage.clone());
            }
            other => return Err(format!("Unknown argument for flash-rp2040: {other}")),
        }
    }

    let uf2 = uf2.ok_or_else(|| usage.clone())?;
    let mount = mount.ok_or_else(|| usage.clone())?;

    if !uf2.exists() {
        return Err(format!("UF2 file does not exist: {uf2:?}"));
    }
    if !mount.exists() {
        return Err(format!("Mount path does not exist: {mount:?}"));
    }
    if !mount.is_dir() {
        return Err(format!("Mount path is not a directory: {mount:?}"));
    }

    let file_name = uf2
        .file_name()
        .ok_or_else(|| format!("Invalid UF2 path (no file name): {uf2:?}"))?;
    let dest = mount.join(file_name);

    if dry_run {
        eprintln!("dry-run: would copy {uf2:?} -> {dest:?}");
        return Ok(());
    }

    fs::copy(&uf2, &dest).map_err(|err| {
        format!("Failed to copy UF2 to mount (src={uf2:?}, dest={dest:?}): {err}")
    })?;
    Ok(())
}

fn run_board_parse_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "board-parse");
    let mut input: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--in" => {
                input =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --in <board.log>".to_string()
                    })?));
            }
            "--out-dir" => {
                out_dir =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --out-dir <dir>".to_string()
                    })?));
            }
            "-h" | "--help" => {
                return Err(usage.clone());
            }
            other => return Err(format!("Unknown argument for board-parse: {other}")),
        }
    }

    let input = input.ok_or_else(|| usage.clone())?;
    let out_dir = out_dir.ok_or_else(|| usage.clone())?;

    let text = fs::read_to_string(&input)
        .map_err(|err| format!("Failed to read board log {input:?}: {err}"))?;
    let parsed = rust_plc::board_log::parse_board_log_text(&text)
        .map_err(|err| format!("Failed to parse board log: {err}"))?;

    fs::create_dir_all(&out_dir)
        .map_err(|err| format!("Failed to create output dir {out_dir:?}: {err}"))?;

    let mut board_trace_jsonl = String::new();
    for r in parsed.trace_rows {
        let mut line = serde_json::to_string(&r)
            .map_err(|err| format!("Failed to serialize trace row JSON: {err}"))?;
        line.push('\n');
        board_trace_jsonl.push_str(&line);
    }

    let board_trace_path = out_dir.join("board_trace.jsonl");
    fs::write(&board_trace_path, board_trace_jsonl)
        .map_err(|err| format!("Failed to write {board_trace_path:?}: {err}"))?;

    let tick_timing_jsonl = to_tick_timing_jsonl(&parsed.timing_rows)
        .map_err(|err| format!("Failed to serialize tick timing JSONL: {err}"))?;
    let tick_timing_path = out_dir.join("tick_timing.jsonl");
    fs::write(&tick_timing_path, tick_timing_jsonl)
        .map_err(|err| format!("Failed to write {tick_timing_path:?}: {err}"))?;

    Ok(())
}

#[derive(Debug, Serialize)]
struct CommissioningStepReport {
    id: &'static str,
    title: &'static str,
    command: String,
    status: &'static str,
    artifacts: Vec<String>,
    detail: Option<String>,
}

#[derive(Debug, Serialize)]
struct CommissioningArtifacts {
    nominal_scenario: String,
    doctor_nominal: String,
    retain_config: String,
    retain_state: String,
    nominal_trace: String,
    gate_nominal_json: String,
    gate_nominal_dir: String,
    gate_nominal_diagnosis: String,
    fault_scenario: String,
    doctor_fault: String,
    online_force_script: String,
    online_var_script: String,
    online_var_bindings: String,
    fault_trace: String,
    online_force_audit: String,
    online_var_audit: String,
    gate_fault_json: String,
    gate_fault_dir: String,
    gate_fault_diagnosis: String,
}

#[derive(Debug, Serialize)]
struct CommissioningRunReport {
    schema_version: u32,
    command: &'static str,
    output: &'static str,
    status: &'static str,
    plc: String,
    out_dir: String,
    artifact_index: String,
    steps: Vec<CommissioningStepReport>,
    artifacts: CommissioningArtifacts,
}

fn commissioning_command_display(program: &str, args: &[String]) -> String {
    if args.is_empty() {
        return program.to_string();
    }
    format!("{program} {}", args.join(" "))
}

fn run_commissioning_child(
    binary_path: &Path,
    args: &[String],
    stdout_capture: Option<&Path>,
) -> Result<(), String> {
    let output = Command::new(binary_path)
        .args(args)
        .output()
        .map_err(|err| format!("Failed to execute {}: {err}", binary_path.display()))?;

    if let Some(path) = stdout_capture {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "Failed to create stdout capture directory {}: {err}",
                    parent.display()
                )
            })?;
        }
        fs::write(path, &output.stdout)
            .map_err(|err| format!("Failed to write stdout capture {}: {err}", path.display()))?;
    }

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr_trimmed = stderr.trim();
    if stderr_trimmed.is_empty() {
        Err(format!(
            "Command failed (status {:?}): {}",
            output.status.code(),
            args.join(" ")
        ))
    } else {
        Err(format!(
            "Command failed (status {:?}): {}\n{}",
            output.status.code(),
            args.join(" "),
            stderr_trimmed
        ))
    }
}

fn read_status_from_json(path: &Path) -> Result<String, String> {
    let body = fs::read_to_string(path)
        .map_err(|err| format!("Failed to read JSON file {}: {err}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&body)
        .map_err(|err| format!("Failed to parse JSON file {}: {err}", path.display()))?;
    let status = value
        .get("status")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            format!(
                "JSON file {} is missing string field `status`",
                path.display()
            )
        })?;
    Ok(status.to_string())
}

fn commissioning_paths_to_relative(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|p| display_path_relative_to_cwd(p))
        .collect()
}

fn push_commissioning_step(
    steps: &mut Vec<CommissioningStepReport>,
    id: &'static str,
    title: &'static str,
    command: String,
    status: &'static str,
    artifacts: Vec<String>,
    detail: Option<String>,
) {
    steps.push(CommissioningStepReport {
        id,
        title,
        command,
        status,
        artifacts,
        detail,
    });
}

fn run_commissioning_step(
    steps: &mut Vec<CommissioningStepReport>,
    failure_reason: &mut Option<String>,
    program: &str,
    binary_path: &Path,
    id: &'static str,
    title: &'static str,
    cmd_args: Vec<String>,
    stdout_capture: Option<&Path>,
    artifact_paths: Vec<PathBuf>,
    checker: impl FnOnce() -> Result<(), String>,
) {
    let command = commissioning_command_display(program, &cmd_args);
    let artifacts_rel = commissioning_paths_to_relative(&artifact_paths);
    if failure_reason.is_some() {
        push_commissioning_step(
            steps,
            id,
            title,
            command,
            "skipped",
            artifacts_rel,
            Some("Skipped because an earlier commissioning step failed".to_string()),
        );
        return;
    }

    let result =
        run_commissioning_child(binary_path, &cmd_args, stdout_capture).and_then(|_| checker());
    match result {
        Ok(()) => push_commissioning_step(steps, id, title, command, "pass", artifacts_rel, None),
        Err(err) => {
            *failure_reason = Some(err.clone());
            push_commissioning_step(steps, id, title, command, "fail", artifacts_rel, Some(err));
        }
    }
}

fn run_commissioning_run_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "commissioning-run");
    let Some(plc_path_raw) = args.next() else {
        return Err(usage);
    };
    let plc_path = PathBuf::from(plc_path_raw);

    let mut out_dir: Option<PathBuf> = None;
    let mut output_mode = CliOutputMode::Human;

    while let Some(arg) = args.next() {
        match arg.as_str() {
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
            "-h" | "--help" => return Err(usage.clone()),
            other => return Err(format!("Unknown argument for commissioning-run: {other}")),
        }
    }

    let out_dir = out_dir.ok_or_else(|| usage.clone())?;
    fs::create_dir_all(&out_dir).map_err(|err| {
        format!(
            "Failed to create commissioning output directory {}: {err}",
            out_dir.display()
        )
    })?;

    let binary_path = env::current_exe()
        .map_err(|err| format!("Failed to resolve current binary path: {err}"))?;

    let nominal_yaml = out_dir.join("nominal.yaml");
    let doctor_nominal_json = out_dir.join("doctor_nominal.json");
    let retain_toml = out_dir.join("retain.toml");
    let retain_state_json = out_dir.join("retain_state.json");
    let nominal_trace_jsonl = out_dir.join("nominal_trace.jsonl");
    let gate_nominal_dir = out_dir.join("gate_nominal");
    let gate_nominal_json = out_dir.join("gate_nominal.json");
    let gate_nominal_diagnosis = gate_nominal_dir.join("diagnosis_report.json");

    let fault_yaml = out_dir.join("fault.yaml");
    let doctor_fault_json = out_dir.join("doctor_fault.json");
    let online_force_jsonl = out_dir.join("online_force.jsonl");
    let online_var_jsonl = out_dir.join("online_var.jsonl");
    let online_var_bindings_toml = out_dir.join("online_var_bindings.toml");
    let fault_trace_jsonl = out_dir.join("fault_trace.jsonl");
    let online_force_audit_jsonl = out_dir.join("online_force_audit.jsonl");
    let online_var_audit_jsonl = out_dir.join("online_var_audit.jsonl");
    let gate_fault_dir = out_dir.join("gate_fault");
    let gate_fault_json = out_dir.join("gate_fault.json");
    let gate_fault_diagnosis = gate_fault_dir.join("diagnosis_report.json");
    let artifact_index_path = out_dir.join("commissioning_index.json");

    let artifacts = CommissioningArtifacts {
        nominal_scenario: display_path_relative_to_cwd(&nominal_yaml),
        doctor_nominal: display_path_relative_to_cwd(&doctor_nominal_json),
        retain_config: display_path_relative_to_cwd(&retain_toml),
        retain_state: display_path_relative_to_cwd(&retain_state_json),
        nominal_trace: display_path_relative_to_cwd(&nominal_trace_jsonl),
        gate_nominal_json: display_path_relative_to_cwd(&gate_nominal_json),
        gate_nominal_dir: display_path_relative_to_cwd(&gate_nominal_dir),
        gate_nominal_diagnosis: display_path_relative_to_cwd(&gate_nominal_diagnosis),
        fault_scenario: display_path_relative_to_cwd(&fault_yaml),
        doctor_fault: display_path_relative_to_cwd(&doctor_fault_json),
        online_force_script: display_path_relative_to_cwd(&online_force_jsonl),
        online_var_script: display_path_relative_to_cwd(&online_var_jsonl),
        online_var_bindings: display_path_relative_to_cwd(&online_var_bindings_toml),
        fault_trace: display_path_relative_to_cwd(&fault_trace_jsonl),
        online_force_audit: display_path_relative_to_cwd(&online_force_audit_jsonl),
        online_var_audit: display_path_relative_to_cwd(&online_var_audit_jsonl),
        gate_fault_json: display_path_relative_to_cwd(&gate_fault_json),
        gate_fault_dir: display_path_relative_to_cwd(&gate_fault_dir),
        gate_fault_diagnosis: display_path_relative_to_cwd(&gate_fault_diagnosis),
    };

    let mut steps: Vec<CommissioningStepReport> = Vec::new();
    let mut failure_reason: Option<String> = None;

    run_commissioning_step(
        &mut steps,
        &mut failure_reason,
        program,
        &binary_path,
        "A1",
        "Nominal scenario-init",
        vec![
            "scenario-init".to_string(),
            plc_path.display().to_string(),
            "--preset".to_string(),
            "normal".to_string(),
            "--out".to_string(),
            nominal_yaml.display().to_string(),
        ],
        None,
        vec![nominal_yaml.clone()],
        || {
            if nominal_yaml.exists() {
                Ok(())
            } else {
                Err(format!(
                    "Expected nominal scenario output {}",
                    nominal_yaml.display()
                ))
            }
        },
    );

    run_commissioning_step(
        &mut steps,
        &mut failure_reason,
        program,
        &binary_path,
        "A2",
        "Nominal scenario-doctor",
        vec![
            "scenario-doctor".to_string(),
            plc_path.display().to_string(),
            "--scenario".to_string(),
            nominal_yaml.display().to_string(),
            "--output".to_string(),
            "json".to_string(),
        ],
        Some(&doctor_nominal_json),
        vec![doctor_nominal_json.clone()],
        || {
            let status = read_status_from_json(&doctor_nominal_json)?;
            if status == "pass" {
                Ok(())
            } else {
                Err(format!(
                    "doctor_nominal status must be `pass`, got `{status}`"
                ))
            }
        },
    );

    let retain_write_command = format!("write {}", retain_toml.display());
    if failure_reason.is_some() {
        push_commissioning_step(
            &mut steps,
            "A3",
            "Write retain config",
            retain_write_command,
            "skipped",
            vec![display_path_relative_to_cwd(&retain_toml)],
            Some("Skipped because an earlier commissioning step failed".to_string()),
        );
    } else {
        let retain_body = "schema_version = 1\n[digital_inputs]\ndi0 = false\n[digital_outputs]\ndo0 = false\n[analog_outputs]\nao0 = 0.0\n";
        let retain_result = fs::write(&retain_toml, retain_body).map_err(|err| {
            format!(
                "Failed to write retain config {}: {err}",
                retain_toml.display()
            )
        });
        match retain_result {
            Ok(()) => push_commissioning_step(
                &mut steps,
                "A3",
                "Write retain config",
                retain_write_command,
                "pass",
                vec![display_path_relative_to_cwd(&retain_toml)],
                None,
            ),
            Err(err) => {
                failure_reason = Some(err.clone());
                push_commissioning_step(
                    &mut steps,
                    "A3",
                    "Write retain config",
                    retain_write_command,
                    "fail",
                    vec![display_path_relative_to_cwd(&retain_toml)],
                    Some(err),
                );
            }
        }
    }

    run_commissioning_step(
        &mut steps,
        &mut failure_reason,
        program,
        &binary_path,
        "A4",
        "Nominal sim-plc with retain",
        vec![
            "sim-plc".to_string(),
            plc_path.display().to_string(),
            "--scenario".to_string(),
            nominal_yaml.display().to_string(),
            "--out".to_string(),
            nominal_trace_jsonl.display().to_string(),
            "--retain-config".to_string(),
            retain_toml.display().to_string(),
            "--retain-state".to_string(),
            retain_state_json.display().to_string(),
        ],
        None,
        vec![nominal_trace_jsonl.clone(), retain_state_json.clone()],
        || {
            if !nominal_trace_jsonl.exists() {
                return Err(format!(
                    "Expected nominal trace output {}",
                    nominal_trace_jsonl.display()
                ));
            }
            if !retain_state_json.exists() {
                return Err(format!(
                    "Expected retain state output {}",
                    retain_state_json.display()
                ));
            }
            Ok(())
        },
    );

    run_commissioning_step(
        &mut steps,
        &mut failure_reason,
        program,
        &binary_path,
        "A5",
        "Nominal no-board-gate",
        vec![
            "no-board-gate".to_string(),
            plc_path.display().to_string(),
            "--scenario".to_string(),
            nominal_yaml.display().to_string(),
            "--out-dir".to_string(),
            gate_nominal_dir.display().to_string(),
            "--output".to_string(),
            "json".to_string(),
        ],
        Some(&gate_nominal_json),
        vec![
            gate_nominal_json.clone(),
            gate_nominal_dir.join("sil_trace.jsonl"),
            gate_nominal_dir.join("board_trace.jsonl"),
            gate_nominal_dir.join("diff_report.json"),
            gate_nominal_dir.join("timing_report.json"),
            gate_nominal_diagnosis.clone(),
        ],
        || {
            let status = read_status_from_json(&gate_nominal_json)?;
            if status != "pass" {
                return Err(format!(
                    "gate_nominal status must be `pass`, got `{status}`"
                ));
            }
            for required in [
                gate_nominal_dir.join("sil_trace.jsonl"),
                gate_nominal_dir.join("board_trace.jsonl"),
                gate_nominal_dir.join("diff_report.json"),
                gate_nominal_dir.join("timing_report.json"),
            ] {
                if !required.exists() {
                    return Err(format!(
                        "Missing nominal gate artifact {}",
                        required.display()
                    ));
                }
            }
            Ok(())
        },
    );

    run_commissioning_step(
        &mut steps,
        &mut failure_reason,
        program,
        &binary_path,
        "B1",
        "Fault scenario-init",
        vec![
            "scenario-init".to_string(),
            plc_path.display().to_string(),
            "--preset".to_string(),
            "sensor_stuck".to_string(),
            "--out".to_string(),
            fault_yaml.display().to_string(),
        ],
        None,
        vec![fault_yaml.clone()],
        || {
            if fault_yaml.exists() {
                Ok(())
            } else {
                Err(format!(
                    "Expected fault scenario output {}",
                    fault_yaml.display()
                ))
            }
        },
    );

    run_commissioning_step(
        &mut steps,
        &mut failure_reason,
        program,
        &binary_path,
        "B2",
        "Fault scenario-doctor",
        vec![
            "scenario-doctor".to_string(),
            plc_path.display().to_string(),
            "--scenario".to_string(),
            fault_yaml.display().to_string(),
            "--fix-preview".to_string(),
            "--output".to_string(),
            "json".to_string(),
        ],
        Some(&doctor_fault_json),
        vec![doctor_fault_json.clone()],
        || {
            let status = read_status_from_json(&doctor_fault_json)?;
            if status == "pass" {
                Ok(())
            } else {
                Err(format!(
                    "doctor_fault status must be `pass`, got `{status}`"
                ))
            }
        },
    );

    let scripts_write_command = format!(
        "write {}, {}, {}",
        online_force_jsonl.display(),
        online_var_jsonl.display(),
        online_var_bindings_toml.display()
    );
    if failure_reason.is_some() {
        push_commissioning_step(
            &mut steps,
            "B3",
            "Write online control scripts",
            scripts_write_command,
            "skipped",
            vec![
                display_path_relative_to_cwd(&online_force_jsonl),
                display_path_relative_to_cwd(&online_var_jsonl),
                display_path_relative_to_cwd(&online_var_bindings_toml),
            ],
            Some("Skipped because an earlier commissioning step failed".to_string()),
        );
    } else {
        let force_script = concat!(
            "{\"at_ms\":0,\"actor\":\"commissioning\",\"source\":\"panel\",\"channel\":\"DI0\",\"value\":true}\n",
            "{\"at_ms\":40,\"actor\":\"commissioning\",\"source\":\"panel\",\"channel\":\"DI0\",\"value\":null}\n",
        );
        let var_script = concat!(
            "{\"at_ms\":0,\"actor\":\"commissioning\",\"source\":\"panel\",\"variable\":\"BOOL:diag_latch\",\"value\":true}\n",
            "{\"at_ms\":20,\"actor\":\"commissioning\",\"source\":\"panel\",\"variable\":\"REAL:gain_k\",\"value\":1.25}\n",
            "{\"at_ms\":40,\"actor\":\"commissioning\",\"source\":\"panel\",\"variable\":\"BOOL:diag_latch\",\"value\":null}\n",
            "{\"at_ms\":50,\"actor\":\"commissioning\",\"source\":\"panel\",\"variable\":\"REAL:gain_k\",\"value\":null}\n",
        );
        let bindings_body = concat!(
            "schema_version = 1\n",
            "[bool]\n",
            "diag_latch = \"DI0\"\n",
            "[real]\n",
            "gain_k = \"AI0\"\n",
        );

        let write_result = fs::write(&online_force_jsonl, force_script)
            .and_then(|_| fs::write(&online_var_jsonl, var_script))
            .and_then(|_| fs::write(&online_var_bindings_toml, bindings_body))
            .map_err(|err| format!("Failed to write online control scripts: {err}"));

        match write_result {
            Ok(()) => push_commissioning_step(
                &mut steps,
                "B3",
                "Write online control scripts",
                scripts_write_command,
                "pass",
                vec![
                    display_path_relative_to_cwd(&online_force_jsonl),
                    display_path_relative_to_cwd(&online_var_jsonl),
                    display_path_relative_to_cwd(&online_var_bindings_toml),
                ],
                None,
            ),
            Err(err) => {
                failure_reason = Some(err.clone());
                push_commissioning_step(
                    &mut steps,
                    "B3",
                    "Write online control scripts",
                    scripts_write_command,
                    "fail",
                    vec![
                        display_path_relative_to_cwd(&online_force_jsonl),
                        display_path_relative_to_cwd(&online_var_jsonl),
                        display_path_relative_to_cwd(&online_var_bindings_toml),
                    ],
                    Some(err),
                );
            }
        }
    }

    run_commissioning_step(
        &mut steps,
        &mut failure_reason,
        program,
        &binary_path,
        "B4",
        "Fault sim-plc with online controls",
        vec![
            "sim-plc".to_string(),
            plc_path.display().to_string(),
            "--scenario".to_string(),
            fault_yaml.display().to_string(),
            "--out".to_string(),
            fault_trace_jsonl.display().to_string(),
            "--retain-config".to_string(),
            retain_toml.display().to_string(),
            "--retain-state".to_string(),
            retain_state_json.display().to_string(),
            "--enable-online-force-dev".to_string(),
            "--online-force-script".to_string(),
            online_force_jsonl.display().to_string(),
            "--online-force-audit-out".to_string(),
            online_force_audit_jsonl.display().to_string(),
            "--online-var-script".to_string(),
            online_var_jsonl.display().to_string(),
            "--online-var-bindings".to_string(),
            online_var_bindings_toml.display().to_string(),
            "--online-var-audit-out".to_string(),
            online_var_audit_jsonl.display().to_string(),
        ],
        None,
        vec![
            fault_trace_jsonl.clone(),
            online_force_audit_jsonl.clone(),
            online_var_audit_jsonl.clone(),
        ],
        || {
            for required in [
                fault_trace_jsonl.clone(),
                online_force_audit_jsonl.clone(),
                online_var_audit_jsonl.clone(),
            ] {
                if !required.exists() {
                    return Err(format!(
                        "Missing fault simulation artifact {}",
                        required.display()
                    ));
                }
            }
            Ok(())
        },
    );

    run_commissioning_step(
        &mut steps,
        &mut failure_reason,
        program,
        &binary_path,
        "B5",
        "Fault no-board-gate",
        vec![
            "no-board-gate".to_string(),
            plc_path.display().to_string(),
            "--scenario".to_string(),
            fault_yaml.display().to_string(),
            "--out-dir".to_string(),
            gate_fault_dir.display().to_string(),
            "--output".to_string(),
            "json".to_string(),
        ],
        Some(&gate_fault_json),
        vec![
            gate_fault_json.clone(),
            gate_fault_dir.join("diff_report.json"),
            gate_fault_diagnosis.clone(),
        ],
        || {
            let status = read_status_from_json(&gate_fault_json)?;
            if status != "pass" {
                return Err(format!("gate_fault status must be `pass`, got `{status}`"));
            }
            let diff_report = gate_fault_dir.join("diff_report.json");
            if !diff_report.exists() {
                return Err(format!(
                    "Missing fault gate artifact {}",
                    diff_report.display()
                ));
            }
            Ok(())
        },
    );

    let report_status = if failure_reason.is_none() {
        "pass"
    } else {
        "fail"
    };

    let report = CommissioningRunReport {
        schema_version: 1,
        command: "commissioning-run",
        output: output_mode.as_str(),
        status: report_status,
        plc: display_path_relative_to_cwd(&plc_path),
        out_dir: display_path_relative_to_cwd(&out_dir),
        artifact_index: display_path_relative_to_cwd(&artifact_index_path),
        steps,
        artifacts,
    };

    let mut report_json = serde_json::to_string_pretty(&report)
        .map_err(|err| format!("Failed to serialize commissioning index JSON: {err}"))?;
    report_json.push('\n');
    fs::write(&artifact_index_path, &report_json).map_err(|err| {
        format!(
            "Failed to write commissioning index {}: {err}",
            artifact_index_path.display()
        )
    })?;

    if output_mode == CliOutputMode::Json {
        print!("{report_json}");
    } else {
        eprintln!(
            "commissioning-run: {}",
            if report_status == "pass" {
                "PASS"
            } else {
                "FAIL"
            }
        );
        eprintln!(
            "  commissioning_index: {}",
            display_path_relative_to_cwd(&artifact_index_path)
        );
    }

    if let Some(reason) = failure_reason {
        return Err(format!(
            "commissioning-run failed: {reason} (index: {})",
            artifact_index_path.display()
        ));
    }

    Ok(())
}

fn run_no_board_gate_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "no-board-gate");
    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut scenario_path: Option<PathBuf> = None;
    let mut sil_scenario_path: Option<PathBuf> = None;
    let mut board_scenario_path: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut context_window: usize = 3;
    let mut max_p99_exec_us: Option<u64> = None;
    let mut max_overrun_count: Option<u64> = None;
    let mut output_mode = CliOutputMode::Human;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenario" => {
                scenario_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --scenario <scenario.yaml>".to_string()
                    })?));
            }
            "--sil-scenario" => {
                sil_scenario_path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --sil-scenario <scenario.yaml>".to_string()
                })?));
            }
            "--board-scenario" => {
                board_scenario_path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --board-scenario <scenario.yaml>".to_string()
                })?));
            }
            "--out-dir" => {
                out_dir =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --out-dir <dir>".to_string()
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
            "--max-p99-exec-us" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --max-p99-exec-us <us>".to_string())?;
                max_p99_exec_us = Some(raw.parse::<u64>().map_err(|_| {
                    format!("Invalid --max-p99-exec-us value (expected u64): {raw}")
                })?);
            }
            "--max-overrun-count" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --max-overrun-count <n>".to_string())?;
                max_overrun_count = Some(raw.parse::<u64>().map_err(|_| {
                    format!("Invalid --max-overrun-count value (expected u64): {raw}")
                })?);
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
            other => return Err(format!("Unknown argument for no-board-gate: {other}")),
        }
    }

    let out_dir = out_dir.ok_or_else(|| usage.clone())?;

    let sil_scenario_path = sil_scenario_path
        .or_else(|| scenario_path.clone())
        .ok_or_else(|| usage.clone())?;
    let board_scenario_path = board_scenario_path
        .or_else(|| scenario_path.clone())
        .ok_or_else(|| usage.clone())?;

    fs::create_dir_all(&out_dir)
        .map_err(|err| format!("Failed to create output directory {out_dir:?}: {err}"))?;

    let loaded = load_plc_source(Path::new(&plc_path))?;

    let sil_yaml = read_scenario_yaml_file(&sil_scenario_path)?;
    let board_yaml = read_scenario_yaml_file(&board_scenario_path)?;
    let sil_yaml = resolve_scenario_yaml_for_plc(&loaded.source, &sil_yaml).map_err(|e| {
        format_resolve_scenario_yaml_error(&plc_path, &sil_scenario_path, "no-board-gate", &e)
    })?;
    let board_yaml = resolve_scenario_yaml_for_plc(&loaded.source, &board_yaml).map_err(|e| {
        format_resolve_scenario_yaml_error(&plc_path, &board_scenario_path, "no-board-gate", &e)
    })?;

    let sil_scenario = parse_scenario_yaml(&sil_yaml)?;
    let board_scenario = parse_scenario_yaml(&board_yaml)?;

    if sil_scenario.tick_ms != board_scenario.tick_ms {
        return Err(format!(
            "SIL tick_ms ({}) must match board tick_ms ({}) for no-board-gate",
            sil_scenario.tick_ms, board_scenario.tick_ms
        ));
    }

    let program = compile_plc_to_runtime_program(&loaded.source, sil_scenario.tick_ms)?;

    let sil_trace_path = out_dir.join("sil_trace.jsonl");
    let (num_di, num_do, num_ai, num_ao) =
        io_sizes_for_program_and_scenario(&program, &sil_scenario);
    let mut sil_io = sim::SimIo::new(num_di, num_do, num_ai, num_ao);
    let sil_run =
        sim::run_program_for_scenario(&program, &sil_scenario, &mut sil_io).map_err(|e| {
            let mut msg = format!("SIL simulation failed: {e}");
            if let Some(hint) = scenario_mismatch_hint_for_example(
                &plc_path,
                &sil_scenario_path,
                &e,
                "no-board-gate",
            ) {
                msg.push_str("\n\n");
                msg.push_str(&hint);
            }
            msg
        })?;

    fs::write(&sil_trace_path, sil_run.trace.into_string())
        .map_err(|err| format!("Failed to write SIL trace file {sil_trace_path:?}: {err}"))?;

    let (_, board_trace_path, _, tick_timing_path) = write_virtual_board_artifacts(
        Path::new(&plc_path),
        &board_scenario_path,
        &program,
        &board_scenario,
        &out_dir,
    )?;

    let board_trace_text = fs::read_to_string(&board_trace_path)
        .map_err(|err| format!("Failed to read board trace {board_trace_path:?}: {err}"))?;
    let sil_trace_text = fs::read_to_string(&sil_trace_path)
        .map_err(|err| format!("Failed to read SIL trace {sil_trace_path:?}: {err}"))?;

    let sil_events = rust_plc::trace_diff::parse_trace_jsonl(&sil_trace_text)
        .map_err(|err| format!("Failed to parse SIL trace JSONL: {err}"))?;
    let board_events = rust_plc::trace_diff::parse_trace_jsonl(&board_trace_text)
        .map_err(|err| format!("Failed to parse board trace JSONL: {err}"))?;

    let report = rust_plc::trace_diff::diff_traces(&sil_events, &board_events, context_window);
    let diff_report_path = out_dir.join("diff_report.json");
    let mut json = serde_json::to_string_pretty(&report)
        .map_err(|err| format!("Failed to serialize diff report JSON: {err}"))?;
    json.push('\n');
    fs::write(&diff_report_path, json)
        .map_err(|err| format!("Failed to write diff report {diff_report_path:?}: {err}"))?;

    let tick_timing_text = fs::read_to_string(&tick_timing_path)
        .map_err(|err| format!("Failed to read tick timing {tick_timing_path:?}: {err}"))?;
    let tick_timing_rows = parse_tick_timing_jsonl(&tick_timing_text)
        .map_err(|err| format!("Failed to parse tick timing JSONL: {err}"))?;
    let timing_report = build_timing_report(&tick_timing_rows)
        .ok_or_else(|| "tick_timing.jsonl is empty; cannot evaluate realtime gate".to_string())?;
    let timing_report_path = out_dir.join("timing_report.json");
    let mut timing_json = serde_json::to_string_pretty(&timing_report)
        .map_err(|err| format!("Failed to serialize timing report JSON: {err}"))?;
    timing_json.push('\n');
    fs::write(&timing_report_path, timing_json)
        .map_err(|err| format!("Failed to write timing report {timing_report_path:?}: {err}"))?;

    let mut realtime_failures = Vec::new();
    if let Some(limit) = max_p99_exec_us {
        if timing_report.exec_us_p99 > limit {
            realtime_failures.push(format!(
                "p99 exec_us={} exceeds --max-p99-exec-us={limit}",
                timing_report.exec_us_p99
            ));
        }
    }
    if let Some(limit) = max_overrun_count {
        if timing_report.overrun_count > limit {
            realtime_failures.push(format!(
                "overrun_count={} exceeds --max-overrun-count={limit}",
                timing_report.overrun_count
            ));
        }
    }

    let gate_failed = !report.is_match || !realtime_failures.is_empty();
    let diagnosis_report_path = out_dir.join("diagnosis_report.json");
    let mut diagnosis_report_rel: Option<String> = None;
    let mut diagnosis_top_candidate_code: Option<String> = None;
    let mut diagnosis_evidence_source: Option<String> = None;

    if gate_failed {
        let diagnosis = diagnose(DiagnosisInput {
            plc_source: &loaded.source,
            scenario: &sil_scenario,
            trace_events: Some(&sil_events),
            diff_report: Some(&report),
            timing_report: Some(&timing_report),
            evidence_source: EvidenceSource::NoBoard,
            io_snapshot: None,
        })
        .map_err(|err| format!("Failed to build no-board diagnosis report: {err}"))?;
        diagnosis_top_candidate_code = diagnosis
            .candidates
            .first()
            .map(|candidate| candidate.issue_code.clone());
        diagnosis_evidence_source =
            Some(evidence_source_label(EvidenceSource::NoBoard).to_string());
        let mut diagnosis_json = serde_json::to_string_pretty(&diagnosis)
            .map_err(|err| format!("Failed to serialize diagnosis report JSON: {err}"))?;
        diagnosis_json.push('\n');
        fs::write(&diagnosis_report_path, diagnosis_json).map_err(|err| {
            format!(
                "Failed to write diagnosis report {}: {err}",
                diagnosis_report_path.display()
            )
        })?;
        diagnosis_report_rel = Some(display_path_relative_to_cwd(&diagnosis_report_path));
    }

    if output_mode == CliOutputMode::Human {
        if report.is_match {
            eprintln!(
                "no-board-gate: PASS (sil_events={}, board_events={})",
                report.sil_events, report.board_events
            );
        } else {
            eprintln!(
                "no-board-gate: FAIL (tick={:?}, type={:?}, index={:?})",
                report.first_mismatch_tick, report.mismatch_type, report.mismatch_index
            );
        }
        eprintln!("  sil_trace: {}", sil_trace_path.display());
        eprintln!("  board_trace: {}", board_trace_path.display());
        eprintln!("  diff_report: {}", diff_report_path.display());
        eprintln!(
            "  timing_report: {} (p99_exec_us={}, overrun_count={})",
            timing_report_path.display(),
            timing_report.exec_us_p99,
            timing_report.overrun_count
        );

        for reason in &realtime_failures {
            eprintln!("  realtime-gate: {reason}");
        }
        if let Some(path) = &diagnosis_report_rel {
            eprintln!("  diagnosis_report: {path}");
        }
    } else {
        #[derive(Serialize)]
        struct NoBoardGateJson<'a> {
            schema_version: u32,
            command: &'static str,
            output: &'static str,
            status: &'static str,
            trace_match: bool,
            realtime_failures: &'a [String],
            sil_trace: String,
            board_trace: String,
            diff_report: String,
            timing_report: String,
            p99_exec_us: u64,
            overrun_count: u64,
            diagnosis_report: Option<String>,
            diagnosis_top_candidate_code: Option<String>,
            diagnosis_evidence_source: Option<String>,
        }
        let payload = NoBoardGateJson {
            schema_version: 2,
            command: "no-board-gate",
            output: output_mode.as_str(),
            status: if report.is_match && realtime_failures.is_empty() {
                "pass"
            } else {
                "fail"
            },
            trace_match: report.is_match,
            realtime_failures: &realtime_failures,
            sil_trace: display_path_relative_to_cwd(&sil_trace_path),
            board_trace: display_path_relative_to_cwd(&board_trace_path),
            diff_report: display_path_relative_to_cwd(&diff_report_path),
            timing_report: display_path_relative_to_cwd(&timing_report_path),
            p99_exec_us: timing_report.exec_us_p99,
            overrun_count: timing_report.overrun_count,
            diagnosis_report: diagnosis_report_rel.clone(),
            diagnosis_top_candidate_code: diagnosis_top_candidate_code.clone(),
            diagnosis_evidence_source: diagnosis_evidence_source.clone(),
        };
        let mut json = serde_json::to_string_pretty(&payload)
            .map_err(|err| format!("Failed to serialize no-board-gate JSON output: {err}"))?;
        json.push('\n');
        print!("{json}");
    }

    if !report.is_match || !realtime_failures.is_empty() {
        let mut reasons = Vec::new();
        if !report.is_match {
            reasons.push(format!(
                "trace mismatch (see {})",
                diff_report_path.display()
            ));
        }
        if !realtime_failures.is_empty() {
            reasons.push(format!(
                "realtime threshold exceeded ({})",
                realtime_failures.join("; ")
            ));
        }
        return Err(format!("no-board-gate failed: {}", reasons.join(", ")));
    }
    Ok(())
}

fn run_pil_run_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "pil-run");
    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut scenario_path: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenario" => {
                scenario_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --scenario <scenario.yaml>".to_string()
                    })?));
            }
            "-h" | "--help" => {
                return Err(usage.clone());
            }
            other => return Err(format!("Unknown argument for pil-run: {other}")),
        }
    }

    let scenario_path = scenario_path.ok_or_else(|| usage.clone())?;

    let loaded = load_plc_source(Path::new(&plc_path))?;
    let scenario_yaml = read_scenario_yaml_file(&scenario_path)?;
    let scenario_yaml =
        resolve_scenario_yaml_for_plc(&loaded.source, &scenario_yaml).map_err(|e| {
            format_resolve_scenario_yaml_error(&plc_path, &scenario_path, "pil-run", &e)
        })?;
    let scenario = parse_scenario_yaml(&scenario_yaml)?;

    let program = compile_plc_to_runtime_program(&loaded.source, scenario.tick_ms)?;

    let (num_di, num_do, num_ai, num_ao) = io_sizes_for_program_and_scenario(&program, &scenario);
    let mut io = sim::SimIo::new(num_di, num_do, num_ai, num_ao);
    scenario
        .apply_to_simio(&mut io)
        .map_err(|e| format!("scenario apply failed: {e}"))?;

    let mut rt =
        runtime_core::Runtime::new(&program).map_err(|e| format!("runtime init failed: {e:?}"))?;

    println!("boot ok");
    for _ in 0..scenario.duration_ticks() {
        let tick = io.tick().0;
        let ts_ms = tick.saturating_mul(scenario.tick_ms);
        println!("TICK tick={tick} ts_ms={ts_ms}");

        rt.tick_with_trace_and_logs(
            &mut io,
            |e| {
                let ts_ms = e.tick.0.saturating_mul(scenario.tick_ms);
                println!(
                    "TRACE tick={} task={} from={} to={} reason={} ts_ms={}",
                    e.tick.0,
                    e.task,
                    e.from.0,
                    e.to.0,
                    reason_str(e.reason),
                    ts_ms
                );
            },
            |log| {
                let ts_ms = log.tick.0.saturating_mul(scenario.tick_ms);
                println!(
                    "LOG tick={} task={} step={} msg_id={} msg={} ts_ms={}",
                    log.tick.0, log.task, log.step.0, log.message_id, log.message, ts_ms
                );
            },
        )
        .map_err(|e| format!("runtime tick failed: {e:?}"))?;

        if is_halted(&rt, &program) {
            break;
        }
    }

    Ok(())
}

#[derive(Debug, Serialize)]
struct VirtualBoardMeta<'a> {
    schema_version: u32,
    source_plc: &'a str,
    scenario_path: &'a str,
    generated_at: &'a str,
    tick_ms: u64,
    duration_ticks: u64,
}

fn write_virtual_board_artifacts(
    plc_path: &Path,
    scenario_path: &Path,
    program: &Program<'_>,
    scenario: &sim::Scenario,
    out_dir: &Path,
) -> Result<(PathBuf, PathBuf, PathBuf, PathBuf), String> {
    let (num_di, num_do, num_ai, num_ao) = io_sizes_for_program_and_scenario(program, scenario);
    let mut io = sim::SimIo::new(num_di, num_do, num_ai, num_ao);
    scenario
        .apply_to_simio(&mut io)
        .map_err(|e| format!("scenario apply failed: {e}"))?;
    let mut rt =
        runtime_core::Runtime::new(program).map_err(|e| format!("runtime init failed: {e:?}"))?;
    let tick_period_us = scenario.tick_ms.saturating_mul(1000);

    let board_log = std::cell::RefCell::new(String::new());
    board_log.borrow_mut().push_str("boot ok\n");
    let mut tick_timing_rows: Vec<TickTimingSample> = Vec::new();

    for _ in 0..scenario.duration_ticks() {
        let tick = io.tick().0;
        let ts_ms = tick.saturating_mul(scenario.tick_ms);
        let ts_start_us = tick.saturating_mul(tick_period_us);
        let transition_count = std::cell::Cell::new(0u64);
        let log_count = std::cell::Cell::new(0u64);
        board_log
            .borrow_mut()
            .push_str(&format!("TICK tick={tick} ts_ms={ts_ms}\n"));

        rt.tick_with_trace_and_logs(
            &mut io,
            |e| {
                transition_count.set(transition_count.get().saturating_add(1));
                let ts_ms = e.tick.0.saturating_mul(scenario.tick_ms);
                board_log.borrow_mut().push_str(&format!(
                    "TRACE tick={} task={} from={} to={} reason={} ts_ms={}\n",
                    e.tick.0,
                    e.task,
                    e.from.0,
                    e.to.0,
                    reason_str(e.reason),
                    ts_ms
                ));
            },
            |log| {
                log_count.set(log_count.get().saturating_add(1));
                let ts_ms = log.tick.0.saturating_mul(scenario.tick_ms);
                board_log.borrow_mut().push_str(&format!(
                    "LOG tick={} task={} step={} msg_id={} msg={} ts_ms={}\n",
                    log.tick.0, log.task, log.step.0, log.message_id, log.message, ts_ms
                ));
            },
        )
        .map_err(|e| format!("runtime tick failed: {e:?}"))?;

        // Keep virtual-board timing deterministic for stable no-board regressions.
        let exec_us = transition_count
            .get()
            .saturating_mul(40)
            .saturating_add(log_count.get().saturating_mul(15))
            .saturating_add(10);
        let overrun = exec_us > tick_period_us;
        let slack_us = if overrun {
            0
        } else {
            tick_period_us.saturating_sub(exec_us)
        };
        let ts_end_us = ts_start_us.saturating_add(exec_us);
        tick_timing_rows.push(TickTimingSample {
            tick,
            ts_start_us,
            ts_end_us,
            exec_us,
            slack_us,
            overrun,
        });
        board_log.borrow_mut().push_str(&format!(
            "TIMING tick={tick} ts_start_us={ts_start_us} ts_end_us={ts_end_us} exec_us={exec_us} slack_us={slack_us} overrun={overrun}\n"
        ));

        if is_halted(&rt, program) {
            break;
        }
    }

    let board_log = board_log.into_inner();
    let board_log_path = out_dir.join("board.log");
    fs::write(&board_log_path, &board_log)
        .map_err(|err| format!("Failed to write board log {board_log_path:?}: {err}"))?;

    let rows = rust_plc::board_trace::parse_trace_text(&board_log)
        .map_err(|err| format!("Failed to parse generated board trace: {err}"))?;
    let mut board_trace_jsonl = String::new();
    for row in rows {
        let mut line = serde_json::to_string(&row)
            .map_err(|err| format!("Failed to serialize trace row: {err}"))?;
        line.push('\n');
        board_trace_jsonl.push_str(&line);
    }
    let board_trace_path = out_dir.join("board_trace.jsonl");
    fs::write(&board_trace_path, board_trace_jsonl)
        .map_err(|err| format!("Failed to write board trace {board_trace_path:?}: {err}"))?;

    let tick_timing_jsonl = to_tick_timing_jsonl(&tick_timing_rows)
        .map_err(|err| format!("Failed to serialize tick timing JSONL: {err}"))?;
    let tick_timing_path = out_dir.join("tick_timing.jsonl");
    fs::write(&tick_timing_path, tick_timing_jsonl)
        .map_err(|err| format!("Failed to write tick timing {tick_timing_path:?}: {err}"))?;

    let generated_at = time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string());
    let plc_path_text = plc_path.to_string_lossy().to_string();
    let scenario_path_text = scenario_path.to_string_lossy().to_string();
    let meta = VirtualBoardMeta {
        schema_version: 1,
        source_plc: &plc_path_text,
        scenario_path: &scenario_path_text,
        generated_at: &generated_at,
        tick_ms: scenario.tick_ms,
        duration_ticks: scenario.duration_ticks(),
    };
    let mut meta_json = serde_json::to_string_pretty(&meta)
        .map_err(|err| format!("Failed to serialize virtual board meta JSON: {err}"))?;
    meta_json.push('\n');
    let meta_path = out_dir.join("virtual_board_meta.json");
    fs::write(&meta_path, meta_json)
        .map_err(|err| format!("Failed to write virtual board meta {meta_path:?}: {err}"))?;

    Ok((
        board_log_path,
        board_trace_path,
        meta_path,
        tick_timing_path,
    ))
}

fn run_virtual_board_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "virtual-board");
    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut scenario_path: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;
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
            "-h" | "--help" => {
                return Err(usage.clone());
            }
            other => return Err(format!("Unknown argument for virtual-board: {other}")),
        }
    }

    let scenario_path = scenario_path.ok_or_else(|| usage.clone())?;
    let out_dir = out_dir.ok_or_else(|| usage.clone())?;
    fs::create_dir_all(&out_dir)
        .map_err(|err| format!("Failed to create output directory {out_dir:?}: {err}"))?;

    let loaded = load_plc_source(Path::new(&plc_path))?;
    let scenario_yaml = read_scenario_yaml_file(&scenario_path)?;
    let scenario_yaml =
        resolve_scenario_yaml_for_plc(&loaded.source, &scenario_yaml).map_err(|e| {
            format_resolve_scenario_yaml_error(&plc_path, &scenario_path, "virtual-board", &e)
        })?;
    let scenario = parse_scenario_yaml(&scenario_yaml)?;

    let program = compile_plc_to_runtime_program(&loaded.source, scenario.tick_ms)?;
    write_virtual_board_artifacts(
        Path::new(&plc_path),
        &scenario_path,
        &program,
        &scenario,
        &out_dir,
    )?;

    Ok(())
}

fn reason_str(r: runtime_core::TransitionReason) -> &'static str {
    match r {
        runtime_core::TransitionReason::Action => "action",
        runtime_core::TransitionReason::DelayElapsed => "delay_elapsed",
        runtime_core::TransitionReason::WaitSatisfied => "wait_satisfied",
        runtime_core::TransitionReason::Timeout => "timeout",
        runtime_core::TransitionReason::Goto => "goto",
    }
}
