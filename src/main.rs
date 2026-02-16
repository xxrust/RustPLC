use rust_plc::error::PlcError;
use rust_plc::ir::{ConstraintSet, StateMachine, TimingModel, TopologyGraph};
use rust_plc::parser::parse_plc;
use rust_plc::semantic::{
    build_constraint_set, build_state_machine, build_timing_model, build_topology_graph,
    preprocess_program,
};
use rust_plc::verification::{VerificationSummary, WarningLevel, verify_all};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use io_traits::{DigitalInputId, DigitalOutputId, Io};
use runtime_core::{Action, Instr, Program, Step, StepId, Task};
use rust_plc::io_map::{IoMap, IoUsage};
use rust_plc::runtime_bridge::state_machine_to_runtime_program;
use rust_plc::sim_regress::{SimRegressOptions, SimRegressSummary, run_sim_regress_with_options};
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;

#[derive(Debug, Serialize)]
struct IrBundle {
    topology: TopologyGraph,
    state_machine: StateMachine,
    constraints: ConstraintSet,
    timing_model: TimingModel,
    verification: VerificationSummary,
}

#[derive(Debug, Serialize)]
struct VerificationReportFile<'a> {
    schema_version: u32,
    tool_version: &'a str,
    source_plc: &'a str,
    generated_at: &'a str,
    verification: &'a VerificationSummary,
}

static SIM_STEP1_ACTIONS: [Action; 1] = [Action::SetDigital {
    id: DigitalOutputId(0),
    value: true,
}];

// A deliberately tiny runtime-core program used by the `sim` subcommand.
//
// wait di0 == true -> set do0 true -> halt
static SIM_STEPS: [Step<'static>; 3] = [
    Step {
        name: "wait_di0_true",
        instr: Instr::WaitDigital {
            id: DigitalInputId(0),
            equals: true,
            next: StepId(1),
            timeout: None,
        },
    },
    Step {
        name: "set_do0_true",
        instr: Instr::Action {
            actions: &SIM_STEP1_ACTIONS,
            next: StepId(2),
        },
    },
    Step {
        name: "halt",
        instr: Instr::Halt,
    },
];

static SIM_TASKS: [Task<'static>; 1] = [Task {
    name: "main",
    steps: &SIM_STEPS,
    entry: StepId(0),
}];

static SIM_PROGRAM: Program<'static> = Program { tasks: &SIM_TASKS };

fn main() {
    let mut args = env::args();
    let program = args.next().unwrap_or_else(|| "rust_plc".to_string());

    let Some(first) = args.next() else {
        print_usage(&program);
        std::process::exit(1);
    };

    if first == "sim" {
        if let Err(msg) = run_sim_subcommand(&program, args) {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "sim-regress" {
        if let Err(msg) = run_sim_regress_subcommand(&program, args) {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "sim-plc" {
        if let Err(msg) = run_sim_plc_subcommand(&program, args) {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "build-rp2040" {
        if let Err(msg) = run_build_rp2040_subcommand(&program, args) {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "flash-rp2040" {
        if let Err(msg) = run_flash_rp2040_subcommand(&program, args) {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "trace-parse" {
        if let Err(msg) = run_trace_parse_subcommand(&program, args) {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "trace-diff" {
        if let Err(msg) = run_trace_diff_subcommand(&program, args) {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "pil-run" {
        if let Err(msg) = run_pil_run_subcommand(&program, args) {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }

    let path = first;
    let mut report_path: Option<PathBuf> = None;
    let mut deny_warnings = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--report" => {
                let value = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --report <file>");
                    std::process::exit(1);
                });
                report_path = Some(PathBuf::from(value));
            }
            "--deny-warnings" => {
                deny_warnings = true;
            }
            "-h" | "--help" => {
                print_usage(&program);
                std::process::exit(0);
            }
            other => {
                eprintln!("Unknown argument: {other}");
                print_usage(&program);
                std::process::exit(1);
            }
        }
    }

    if Path::new(&path).extension().and_then(|ext| ext.to_str()) != Some("plc") {
        eprintln!("Expected a .plc file path, got: {path}");
        std::process::exit(1);
    }

    let source = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(err) => {
            eprintln!("Failed to read PLC file {path}: {err}");
            std::process::exit(1);
        }
    };

    let ir_bundle = match compile_pipeline(&source) {
        Ok(ir_bundle) => ir_bundle,
        Err(errors) => {
            for (index, error) in errors.iter().enumerate() {
                if index > 0 {
                    eprintln!();
                }
                eprintln!("{error}");
            }
            std::process::exit(1);
        }
    };

    let report_path =
        report_path.unwrap_or_else(|| default_verification_report_path(Path::new(&path)));
    if let Err(err) = write_verification_report(&path, &report_path, &ir_bundle.verification) {
        eprintln!("{err}");
        std::process::exit(1);
    }

    print_success_summary(&ir_bundle.verification);
    if deny_warnings {
        let blocking_warnings = collect_blocking_warnings(&ir_bundle.verification);
        if !blocking_warnings.is_empty() {
            eprintln!("--deny-warnings 已启用，检测到阻断级告警：");
            for warning in blocking_warnings {
                eprintln!("  - {warning}");
            }
            std::process::exit(2);
        }
    }

    match serde_json::to_string_pretty(&ir_bundle) {
        Ok(json) => println!("{json}"),
        Err(err) => {
            eprintln!("Failed to serialize IR as JSON: {err}");
            std::process::exit(1);
        }
    }
}

fn print_usage(program: &str) {
    eprintln!("Usage:");
    eprintln!(
        "  {program} <file.plc> [--report <verification_report.json>] [--deny-warnings]"
    );
    eprintln!(
        "  {program} sim <scenario.yaml> [--out <trace.jsonl>] [--vcd-out <wave.vcd>] [--analog-out <analog.csv>] [--report-out <report.json>]"
    );
    eprintln!("  {program} sim-plc <file.plc> --scenario <scenario.yaml> --out <trace.jsonl>");
    eprintln!(
        "  {program} sim-regress --plc-dir <dir> --scenario-dir <dir> [--artifacts-dir <dir>] [--summary-out <summary.json>] [--minimize-failure]"
    );
    eprintln!(
        "  {program} build-rp2040 <file.plc> --out <dir> [--io-map <file>] [--analog-calibration <file>] [--emit-uf2 <file.uf2>]"
    );
    eprintln!("  {program} flash-rp2040 --uf2 <file.uf2> --mount <path> [--dry-run]");
    eprintln!("  {program} trace-parse --in <log.txt> --out <trace.jsonl>");
    eprintln!(
        "  {program} trace-diff --sil <trace.jsonl> --board <trace.jsonl> --out <report.json> [--context <n>] [--fail-on-mismatch]"
    );
    eprintln!("  {program} pil-run <file.plc> --scenario <scenario.yaml>");
}

fn run_sim_subcommand(program: &str, mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let Some(scenario_path) = args.next() else {
        return Err(format!(
            "Usage: {program} sim <scenario.yaml> [--out <trace.jsonl>] [--vcd-out <wave.vcd>] [--analog-out <analog.csv>] [--report-out <report.json>]"
        ));
    };

    let mut out_path: Option<String> = None;
    let mut vcd_out_path: Option<String> = None;
    let mut analog_out_path: Option<String> = None;
    let mut report_out_path: Option<String> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => {
                out_path = Some(
                    args.next()
                        .ok_or_else(|| "Missing value for --out <trace.jsonl>".to_string())?,
                );
            }
            "--vcd-out" => {
                vcd_out_path = Some(
                    args.next()
                        .ok_or_else(|| "Missing value for --vcd-out <wave.vcd>".to_string())?,
                );
            }
            "--analog-out" => {
                analog_out_path =
                    Some(args.next().ok_or_else(|| {
                        "Missing value for --analog-out <analog.csv>".to_string()
                    })?);
            }
            "--report-out" => {
                report_out_path =
                    Some(args.next().ok_or_else(|| {
                        "Missing value for --report-out <report.json>".to_string()
                    })?);
            }
            "-h" | "--help" => {
                return Err(format!(
                    "Usage: {program} sim <scenario.yaml> [--out <trace.jsonl>] [--vcd-out <wave.vcd>] [--analog-out <analog.csv>] [--report-out <report.json>]"
                ));
            }
            other => {
                return Err(format!("Unknown argument for sim: {other}"));
            }
        }
    }

    let scenario_yaml = fs::read_to_string(&scenario_path)
        .map_err(|err| format!("Failed to read scenario YAML file {scenario_path}: {err}"))?;
    let scenario = sim::Scenario::from_yaml_str(&scenario_yaml)
        .map_err(|err| format!("Failed to parse scenario YAML: {err}"))?;

    let out_path = out_path.map(PathBuf::from);
    let base_dir = out_path
        .as_deref()
        .and_then(|p| p.parent())
        .filter(|p| !p.as_os_str().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if out_path.is_some() {
                PathBuf::from(".")
            } else {
                PathBuf::from("out")
            }
        });

    let out_path = out_path.unwrap_or_else(|| base_dir.join("trace.jsonl"));
    let vcd_out_path = vcd_out_path
        .map(PathBuf::from)
        .unwrap_or_else(|| base_dir.join("wave.vcd"));
    let analog_out_path = analog_out_path
        .map(PathBuf::from)
        .unwrap_or_else(|| base_dir.join("analog.csv"));
    let report_out_path = report_out_path
        .map(PathBuf::from)
        .unwrap_or_else(|| base_dir.join("report.json"));

    for p in [&out_path, &vcd_out_path, &analog_out_path, &report_out_path] {
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|err| {
                    format!("Failed to create output directory {parent:?}: {err}")
                })?;
            }
        }
    }

    let mut io = sim::SimIo::new(1, 1, 0, 0);
    let run = sim::run_program_for_scenario(&SIM_PROGRAM, &scenario, &mut io)
        .map_err(|err| format!("Simulation failed: {err}"))?;

    fs::write(&out_path, run.trace.into_string())
        .map_err(|err| format!("Failed to write trace file {out_path:?}: {err}"))?;

    let vcd = sim::export_vcd_digital(&io, scenario.tick_ms);
    fs::write(&vcd_out_path, vcd)
        .map_err(|err| format!("Failed to write VCD file {vcd_out_path:?}: {err}"))?;

    let analog_csv = sim::export_analog_outputs_csv(&io, scenario.tick_ms);
    fs::write(&analog_out_path, analog_csv)
        .map_err(|err| format!("Failed to write analog CSV file {analog_out_path:?}: {err}"))?;

    let mut report_json = serde_json::to_string_pretty(&run.report)
        .map_err(|err| format!("Failed to serialize report JSON: {err}"))?;
    report_json.push('\n');
    fs::write(&report_out_path, report_json)
        .map_err(|err| format!("Failed to write report file {report_out_path:?}: {err}"))?;

    Ok(())
}

fn run_sim_plc_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let Some(plc_path) = args.next() else {
        return Err(format!(
            "Usage: {program} sim-plc <file.plc> --scenario <scenario.yaml> --out <trace.jsonl>"
        ));
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
                        "Missing value for --out <trace.jsonl>".to_string()
                    })?));
            }
            "-h" | "--help" => {
                return Err(format!(
                    "Usage: {program} sim-plc <file.plc> --scenario <scenario.yaml> --out <trace.jsonl>"
                ));
            }
            other => return Err(format!("Unknown argument for sim-plc: {other}")),
        }
    }

    let scenario_path = scenario_path.ok_or_else(|| {
        format!(
            "Usage: {program} sim-plc <file.plc> --scenario <scenario.yaml> --out <trace.jsonl>"
        )
    })?;
    let out_path = out_path.ok_or_else(|| {
        format!(
            "Usage: {program} sim-plc <file.plc> --scenario <scenario.yaml> --out <trace.jsonl>"
        )
    })?;

    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to create output directory {parent:?}: {err}"))?;
        }
    }

    let plc_source =
        fs::read_to_string(&plc_path).map_err(|err| format!("Failed to read {plc_path}: {err}"))?;
    let scenario_yaml = fs::read_to_string(&scenario_path).map_err(|err| {
        format!(
            "Failed to read scenario YAML file {}: {err}",
            scenario_path.display()
        )
    })?;
    let scenario = sim::Scenario::from_yaml_str(&scenario_yaml).map_err(|e| format!("{e}"))?;
    let program = compile_plc_to_runtime_program(&plc_source, scenario.tick_ms)?;

    let (num_di, num_do, num_ai, num_ao) = io_sizes_for_program_and_scenario(&program, &scenario);
    let mut io = sim::SimIo::new(num_di, num_do, num_ai, num_ao);
    let run =
        sim::run_program_for_scenario(&program, &scenario, &mut io).map_err(|e| format!("{e}"))?;
    fs::write(&out_path, run.trace.into_string())
        .map_err(|err| format!("Failed to write trace file {out_path:?}: {err}"))?;
    Ok(())
}

fn run_sim_regress_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let mut plc_dir: Option<PathBuf> = None;
    let mut scenario_dir: Option<PathBuf> = None;
    let mut artifacts_dir: Option<PathBuf> = None;
    let mut summary_out: Option<PathBuf> = None;
    let mut minimize_failure = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--plc-dir" => {
                plc_dir =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --plc-dir <dir>".to_string()
                    })?));
            }
            "--scenario-dir" => {
                scenario_dir =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --scenario-dir <dir>".to_string()
                    })?));
            }
            "--artifacts-dir" => {
                artifacts_dir =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --artifacts-dir <dir>".to_string()
                    })?));
            }
            "--summary-out" => {
                summary_out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --summary-out <summary.json>".to_string()
                })?));
            }
            "--minimize-failure" => {
                minimize_failure = true;
            }
            "-h" | "--help" => {
                return Err(format!(
                    "Usage: {program} sim-regress --plc-dir <dir> --scenario-dir <dir> [--artifacts-dir <dir>] [--summary-out <summary.json>] [--minimize-failure]"
                ));
            }
            other => {
                return Err(format!("Unknown argument for sim-regress: {other}"));
            }
        }
    }

    let plc_dir = plc_dir.ok_or_else(|| {
        format!(
            "Usage: {program} sim-regress --plc-dir <dir> --scenario-dir <dir> [--artifacts-dir <dir>] [--summary-out <summary.json>] [--minimize-failure]"
        )
    })?;
    let scenario_dir = scenario_dir.ok_or_else(|| {
        format!(
            "Usage: {program} sim-regress --plc-dir <dir> --scenario-dir <dir> [--artifacts-dir <dir>] [--summary-out <summary.json>] [--minimize-failure]"
        )
    })?;

    let artifacts_dir = artifacts_dir.unwrap_or_else(|| PathBuf::from("out/sim-regress"));
    let summary_out = summary_out.unwrap_or_else(|| artifacts_dir.join("summary.json"));

    if let Some(parent) = summary_out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to create output directory {parent:?}: {err}"))?;
        }
    }

    let summary = run_sim_regress_with_options(
        &plc_dir,
        &scenario_dir,
        &artifacts_dir,
        SimRegressOptions {
            minimize: minimize_failure,
        },
    )
    .map_err(|e| format!("sim-regress failed: {e}"))?;
    write_sim_regress_summary(&summary_out, &summary)?;
    Ok(())
}

#[derive(Debug, Serialize)]
struct BuildMeta<'a> {
    plc_sha256: &'a str,
    generated_at: &'a str,
    tool_version: &'a str,
    runtime_semver: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    io_map: Option<IoMap>,
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
    let Some(plc_path) = args.next() else {
        return Err(format!(
            "Usage: {program} build-rp2040 <file.plc> --out <dir> [--io-map <file>] [--analog-calibration <file>] [--emit-uf2 <file.uf2>]"
        ));
    };

    let mut out_dir: Option<PathBuf> = None;
    let mut io_map_path: Option<PathBuf> = None;
    let mut analog_calibration_path: Option<PathBuf> = None;
    let mut emit_uf2: Option<PathBuf> = None;
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
            "-h" | "--help" => {
                return Err(format!(
                    "Usage: {program} build-rp2040 <file.plc> --out <dir> [--io-map <file>] [--analog-calibration <file>] [--emit-uf2 <file.uf2>]"
                ));
            }
            other => return Err(format!("Unknown argument for build-rp2040: {other}")),
        }
    }

    let out_dir = out_dir.ok_or_else(|| {
        format!(
            "Usage: {program} build-rp2040 <file.plc> --out <dir> [--io-map <file>] [--analog-calibration <file>] [--emit-uf2 <file.uf2>]"
        )
    })?;
    fs::create_dir_all(&out_dir)
        .map_err(|err| format!("Failed to create out dir {out_dir:?}: {err}"))?;

    if Path::new(&plc_path)
        .extension()
        .and_then(|ext| ext.to_str())
        != Some("plc")
    {
        return Err(format!("Expected a .plc file path, got: {plc_path}"));
    }

    let plc_bytes =
        fs::read(&plc_path).map_err(|err| format!("Failed to read PLC file {plc_path}: {err}"))?;
    let plc_source = String::from_utf8(plc_bytes.clone())
        .map_err(|err| format!("PLC file is not valid UTF-8: {err}"))?;

    let sha256 = {
        let mut h = Sha256::new();
        h.update(&plc_bytes);
        hex::encode(h.finalize())
    };

    let ir_bundle = compile_pipeline(&plc_source).map_err(|errors| errors.join("\n\n"))?;

    // For build artifacts we use 1ms ticks so ms-based DSL durations are always aligned.
    let runtime_program =
        state_machine_to_runtime_program(&ir_bundle.topology, &ir_bundle.state_machine, 1)
            .map_err(|err| format!("Failed to bridge to runtime Program: {err}"))?;

    let usage = io_usage_for_program(&runtime_program);
    let io_map = match io_map_path.as_ref() {
        None => None,
        Some(path) => {
            let toml_str = fs::read_to_string(&path)
                .map_err(|err| format!("Failed to read io map {path:?}: {err}"))?;
            let m = IoMap::from_toml_str(&toml_str)
                .map_err(|err| format!("Failed to parse io map TOML: {err}"))?;
            m.validate_for_usage(usage)
                .map_err(|err| format!("Invalid io map for this program: {err}"))?;
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

    let mut analog_contract = build_analog_contract(&plc_source)?;
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

    let meta = BuildMeta {
        plc_sha256: &sha256,
        generated_at: &generated_at,
        tool_version: env!("CARGO_PKG_VERSION"),
        runtime_semver: runtime_core::VERSION,
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

    Ok(())
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

    let cargo = std::process::Command::new(&cargo_bin)
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

    let uf2 = std::process::Command::new(&elf2uf2_bin)
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

fn build_analog_contract(plc_source: &str) -> Result<AnalogContract, String> {
    let parsed =
        parse_plc(plc_source).map_err(|err| format!("Failed to parse PLC source: {err}"))?;
    let expanded = preprocess_program(&parsed).map_err(|errors| {
        errors
            .into_iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    })?;

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

fn io_map_template_for_program(program: &Program<'_>) -> String {
    use std::collections::BTreeSet;

    let mut dis = BTreeSet::<u16>::new();
    let mut dos = BTreeSet::<u16>::new();
    let mut ais = BTreeSet::<u16>::new();
    let mut aos = BTreeSet::<u16>::new();
    for task in program.tasks {
        for step in task.steps {
            match step.instr {
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
                            Action::Extend { output } | Action::Retract { output } => {
                                dos.insert(output.0);
                            }
                            Action::SetAnalog { id, .. } => {
                                aos.insert(id.0);
                            }
                            Action::Log { .. } => {}
                        }
                    }
                }
                Instr::Delay { .. } | Instr::Goto { .. } | Instr::Halt => {}
            }
        }
    }

    let mut out = String::new();
    out.push_str("# RP2040 I/O map template (fill in GPIO numbers for your wiring)\n");
    out.push_str("# This file is a template; it may be incomplete by design.\n\n");

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
                            Action::Extend { output } | Action::Retract { output } => {
                                dos.insert(output.0);
                            }
                            Action::SetAnalog { id, .. } => {
                                aos.insert(id.0);
                            }
                            Action::Log { .. } => {}
                        }
                    }
                }
                Instr::Delay { .. } | Instr::Goto { .. } | Instr::Halt => {}
            }
        }
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

fn write_sim_regress_summary(path: &Path, summary: &SimRegressSummary) -> Result<(), String> {
    let mut json = serde_json::to_string_pretty(summary)
        .map_err(|err| format!("Failed to serialize summary JSON: {err}"))?;
    json.push('\n');
    fs::write(path, json).map_err(|err| format!("Failed to write summary file {path:?}: {err}"))?;
    Ok(())
}

fn run_flash_rp2040_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
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
                return Err(format!(
                    "Usage: {program} flash-rp2040 --uf2 <file.uf2> --mount <path> [--dry-run]"
                ));
            }
            other => return Err(format!("Unknown argument for flash-rp2040: {other}")),
        }
    }

    let uf2 = uf2.ok_or_else(|| {
        format!("Usage: {program} flash-rp2040 --uf2 <file.uf2> --mount <path> [--dry-run]")
    })?;
    let mount = mount.ok_or_else(|| {
        format!("Usage: {program} flash-rp2040 --uf2 <file.uf2> --mount <path> [--dry-run]")
    })?;

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

fn run_trace_parse_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let mut input: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--in" => {
                input =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --in <log.txt>".to_string()
                    })?));
            }
            "--out" => {
                out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --out <trace.jsonl>".to_string()
                })?));
            }
            "-h" | "--help" => {
                return Err(format!(
                    "Usage: {program} trace-parse --in <log.txt> --out <trace.jsonl>"
                ));
            }
            other => return Err(format!("Unknown argument for trace-parse: {other}")),
        }
    }

    let input = input.ok_or_else(|| {
        format!("Usage: {program} trace-parse --in <log.txt> --out <trace.jsonl>")
    })?;
    let out = out.ok_or_else(|| {
        format!("Usage: {program} trace-parse --in <log.txt> --out <trace.jsonl>")
    })?;

    let text = fs::read_to_string(&input)
        .map_err(|err| format!("Failed to read trace log {input:?}: {err}"))?;
    let rows = rust_plc::board_trace::parse_trace_text(&text)
        .map_err(|err| format!("Failed to parse trace log: {err}"))?;

    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to create output dir {parent:?}: {err}"))?;
        }
    }

    let mut jsonl = String::new();
    for r in rows {
        let mut line = serde_json::to_string(&r)
            .map_err(|err| format!("Failed to serialize trace row JSON: {err}"))?;
        line.push('\n');
        jsonl.push_str(&line);
    }
    fs::write(&out, jsonl).map_err(|err| format!("Failed to write {out:?}: {err}"))?;
    Ok(())
}

fn run_trace_diff_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
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
            "-h" | "--help" => {
                return Err(format!(
                    "Usage: {program} trace-diff --sil <trace.jsonl> --board <trace.jsonl> --out <report.json> [--context <n>] [--fail-on-mismatch]"
                ));
            }
            other => return Err(format!("Unknown argument for trace-diff: {other}")),
        }
    }

    let sil = sil.ok_or_else(|| format!(
        "Usage: {program} trace-diff --sil <trace.jsonl> --board <trace.jsonl> --out <report.json> [--context <n>] [--fail-on-mismatch]"
    ))?;
    let board = board.ok_or_else(|| format!(
        "Usage: {program} trace-diff --sil <trace.jsonl> --board <trace.jsonl> --out <report.json> [--context <n>] [--fail-on-mismatch]"
    ))?;
    let out = out.ok_or_else(|| format!(
        "Usage: {program} trace-diff --sil <trace.jsonl> --board <trace.jsonl> --out <report.json> [--context <n>] [--fail-on-mismatch]"
    ))?;

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

fn run_pil_run_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let Some(plc_path) = args.next() else {
        return Err(format!(
            "Usage: {program} pil-run <file.plc> --scenario <scenario.yaml>"
        ));
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
                return Err(format!(
                    "Usage: {program} pil-run <file.plc> --scenario <scenario.yaml>"
                ));
            }
            other => return Err(format!("Unknown argument for pil-run: {other}")),
        }
    }

    let scenario_path = scenario_path
        .ok_or_else(|| format!("Usage: {program} pil-run <file.plc> --scenario <scenario.yaml>"))?;

    let plc_source =
        fs::read_to_string(&plc_path).map_err(|err| format!("Failed to read {plc_path}: {err}"))?;
    let scenario_yaml = fs::read_to_string(&scenario_path).map_err(|err| {
        format!(
            "Failed to read scenario YAML file {}: {err}",
            scenario_path.display()
        )
    })?;
    let scenario = sim::Scenario::from_yaml_str(&scenario_yaml).map_err(|e| format!("{e}"))?;

    let program = compile_plc_to_runtime_program(&plc_source, scenario.tick_ms)?;

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
                            Action::Log { .. } => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }

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

fn is_halted<'a>(rt: &runtime_core::Runtime<'a>, program: &'a Program<'a>) -> bool {
    let loc = rt.location();
    let Ok(task) = program.task(loc.task) else {
        return false;
    };
    let Some(step) = task.step(loc.step) else {
        return false;
    };
    matches!(step.instr, Instr::Halt)
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

fn compile_pipeline(source: &str) -> Result<IrBundle, Vec<String>> {
    let program = parse_plc(source).map_err(|err| vec![err.to_string()])?;
    let expanded_program = preprocess_program(&program).map_err(|errors| {
        errors
            .into_iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
    })?;

    let mut errors = Vec::new();
    let topology = collect_stage(build_topology_graph(&expanded_program), &mut errors);
    let state_machine = collect_stage(build_state_machine(&expanded_program), &mut errors);
    let constraints = collect_stage(build_constraint_set(&expanded_program), &mut errors);
    let timing_model = collect_stage(build_timing_model(&expanded_program), &mut errors);

    if !errors.is_empty() {
        return Err(errors.into_iter().map(|error| error.to_string()).collect());
    }

    let topology = topology.expect("topology exists when semantic errors are empty");
    let state_machine = state_machine.expect("state machine exists when semantic errors are empty");
    let constraints = constraints.expect("constraints exist when semantic errors are empty");
    let timing_model = timing_model.expect("timing model exists when semantic errors are empty");

    let verification = verify_all(&expanded_program, &topology, &constraints, &state_machine)
        .map_err(|issues| {
            issues
                .into_iter()
                .map(|issue| issue.to_string())
                .collect::<Vec<_>>()
        })?;

    Ok(IrBundle {
        topology,
        state_machine,
        constraints,
        timing_model,
        verification,
    })
}

fn collect_stage<T>(result: Result<T, Vec<PlcError>>, errors: &mut Vec<PlcError>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(mut stage_errors) => {
            errors.append(&mut stage_errors);
            None
        }
    }
}

fn print_success_summary(summary: &VerificationSummary) {
    eprintln!("验证通过：");
    eprintln!(
        "  - Safety: {}（深度 {}）",
        summary.safety.level, summary.safety.explored_depth
    );

    for warning in &summary.safety.warnings {
        eprintln!(
            "    [{}] {}",
            warning_level_label(&warning.level),
            warning.message
        );
    }

    eprintln!("  - Liveness: {}", summary.liveness.level);
    eprintln!("  - Timing: {}", summary.timing.level);
    eprintln!("  - Causality: {}", summary.causality.level);
}

fn warning_level_label(level: &WarningLevel) -> &'static str {
    match level {
        WarningLevel::Error => "ERROR",
        WarningLevel::Warn => "WARN",
        WarningLevel::Info => "INFO",
    }
}

fn collect_blocking_warnings(summary: &VerificationSummary) -> Vec<String> {
    let mut warnings = Vec::new();
    collect_checker_blocking_warnings("safety", &summary.safety.warnings, &mut warnings);
    collect_checker_blocking_warnings("liveness", &summary.liveness.warnings, &mut warnings);
    collect_checker_blocking_warnings("timing", &summary.timing.warnings, &mut warnings);
    collect_checker_blocking_warnings("causality", &summary.causality.warnings, &mut warnings);
    warnings
}

fn collect_checker_blocking_warnings(
    checker: &str,
    entries: &[rust_plc::verification::WarningEntry],
    output: &mut Vec<String>,
) {
    for entry in entries {
        if matches!(entry.level, WarningLevel::Warn | WarningLevel::Error) {
            output.push(format!(
                "[{checker}] {}: {}",
                warning_level_label(&entry.level),
                entry.message
            ));
        }
    }
}

fn default_verification_report_path(plc_path: &Path) -> PathBuf {
    let stem = plc_path
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("input");
    PathBuf::from("out").join(format!("{stem}.verification_report.json"))
}

fn write_verification_report(
    source_plc: &str,
    report_path: &Path,
    verification: &VerificationSummary,
) -> Result<(), String> {
    if let Some(parent) = report_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to create report directory {parent:?}: {err}"))?;
        }
    }

    let generated_at = time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string());
    let report = VerificationReportFile {
        schema_version: 1,
        tool_version: env!("CARGO_PKG_VERSION"),
        source_plc,
        generated_at: &generated_at,
        verification,
    };

    let mut report_json = serde_json::to_string_pretty(&report)
        .map_err(|err| format!("Failed to serialize verification report JSON: {err}"))?;
    report_json.push('\n');
    fs::write(report_path, report_json)
        .map_err(|err| format!("Failed to write verification report {report_path:?}: {err}"))?;

    Ok(())
}
