use rust_plc::error::PlcError;
use rust_plc::ir::{ConstraintSet, StateMachine, TimingModel, TopologyGraph};
use rust_plc::parser::parse_plc;
use rust_plc::semantic::{
    build_constraint_set, build_state_machine, build_timing_model, build_topology_graph,
    preprocess_program,
};
use rust_plc::verification::{VerificationSummary, verify_all};
use serde::Serialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use io_traits::{DigitalInputId, DigitalOutputId};
use runtime_core::{Action, Instr, Program, Step, StepId, Task};

#[derive(Debug, Serialize)]
struct IrBundle {
    topology: TopologyGraph,
    state_machine: StateMachine,
    constraints: ConstraintSet,
    timing_model: TimingModel,
    verification: VerificationSummary,
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

    let path = first;
    if args.next().is_some() {
        print_usage(&program);
        std::process::exit(1);
    };

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

    print_success_summary(&ir_bundle.verification);

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
    eprintln!("  {program} <file.plc>");
    eprintln!("  {program} sim <scenario.yaml> [--out <trace.jsonl>] [--vcd-out <wave.vcd>] [--analog-out <analog.csv>] [--report-out <report.json>]");
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
                out_path = Some(args.next().ok_or_else(|| {
                    "Missing value for --out <trace.jsonl>".to_string()
                })?);
            }
            "--vcd-out" => {
                vcd_out_path = Some(args.next().ok_or_else(|| {
                    "Missing value for --vcd-out <wave.vcd>".to_string()
                })?);
            }
            "--analog-out" => {
                analog_out_path = Some(args.next().ok_or_else(|| {
                    "Missing value for --analog-out <analog.csv>".to_string()
                })?);
            }
            "--report-out" => {
                report_out_path = Some(args.next().ok_or_else(|| {
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

    let scenario_yaml = fs::read_to_string(&scenario_path).map_err(|err| {
        format!("Failed to read scenario YAML file {scenario_path}: {err}")
    })?;
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
        eprintln!("    {warning}");
    }

    eprintln!("  - Liveness: {}", summary.liveness.level);
    eprintln!("  - Timing: {}", summary.timing.level);
    eprintln!("  - Causality: {}", summary.causality.level);
}
