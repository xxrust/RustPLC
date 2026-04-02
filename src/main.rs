use rust_plc::ir::{ConstraintSet, StateMachine, TimingModel, TopologyGraph};
use rust_plc::semantic::build_timing_model;
use rust_plc::verification::{VerificationSummary, WarningEntry, WarningLevel};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use io_traits::{AnalogInputId, AnalogOutputId, DigitalInputId, DigitalOutputId, Io};
use runtime_core::{Action, Instr, MAX_TRANSITIONS_PER_TASK_PER_TICK, Program, Step, StepId, Task};
use rust_plc::alarm_runtime::{
    AlarmBuildInput, AlarmDispatchConfig, AlarmDispatcher, AlarmSeverity, build_alarm_event,
};
use rust_plc::diagnostics::{
    DiagnosisInput, EvidenceSource, IoSnapshotArtifact, IoTickSnapshot, diagnose,
};
use rust_plc::io_map::{IoMap, IoMapError, IoUsage};
use rust_plc::runtime_bridge::state_machine_to_runtime_program;
use rust_plc::scenario_resolve::resolve_scenario_yaml_for_plc;
use rust_plc::sim_regress::{SimRegressOptions, SimRegressSummary, run_sim_regress_with_options};
use rust_plc::tick_timing::{TickTimingSample, parse_tick_timing_jsonl, to_tick_timing_jsonl};
use rust_plc::timing_report::build_timing_report;
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;

mod cli;
mod cli_support;

use crate::cli_support::common::{CliOutputMode, display_path_relative_to_cwd};
use crate::cli_support::diagnostics_common::evidence_source_label;
use crate::cli_support::help::command_usage;
use crate::cli_support::plc_pipeline::{
    collect_compiled_plc_warnings, compile_plc_semantics, compile_plc_to_runtime_program,
    preprocess_plc_source, verify_compiled_plc_semantics,
};
use crate::cli_support::runtime_probe::{io_sizes_for_program_and_scenario, is_halted};
use crate::cli_support::scenario_yaml::{
    format_resolve_scenario_yaml_error, parse_scenario_yaml, read_scenario_yaml_file,
    scenario_mismatch_hint_for_example,
};

#[derive(Debug, Serialize)]
struct IrBundle {
    topology: TopologyGraph,
    state_machine: StateMachine,
    constraints: ConstraintSet,
    timing_model: TimingModel,
    runtime_budget: RuntimeBudget,
    verification: VerificationSummary,
}

#[derive(Debug, Serialize)]
struct VerificationReportFile<'a> {
    schema_version: u32,
    tool_version: &'a str,
    source_plc: &'a str,
    generated_at: &'a str,
    runtime_budget: &'a RuntimeBudget,
    verification: &'a VerificationSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum TransitionBudgetScope {
    PerTaskPerTick,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RuntimeBudget {
    /// runtime-core hard cap per task per tick (see Runtime::tick_with_trace_and_logs).
    transition_budget_scope: TransitionBudgetScope,
    max_transitions_per_tick_cap: usize,
    active_task_count: usize,
    /// Global per-tick transition upper bound derived from active tasks.
    max_transitions_all_tasks_per_tick_upper_bound: usize,
    /// Upper bound on same-tick transition chaining within one task.
    max_transitions_same_tick_upper_bound: usize,
    max_actions_per_transition: usize,
    max_actions_per_tick_upper_bound: usize,
    max_parallel_branches: usize,
    max_race_branches: usize,
    has_same_tick_cycle: bool,
    budget_time_estimate: BudgetTimeEstimate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct BudgetTimeEstimate {
    action_cost_us: u64,
    transition_cost_us: u64,
    parallel_expand_cost_us: u64,
    action_component_us: u64,
    transition_component_us: u64,
    parallel_component_us: u64,
    total_estimate_us: u64,
    max_allowed_us: u64,
    exceeds_budget: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RuntimeBudgetThresholds {
    max_actions_per_transition: usize,
    max_actions_per_tick_upper_bound: usize,
    max_parallel_branches: usize,
    max_race_branches: usize,
    warn_on_same_tick_cycle: bool,
    action_cost_us: u64,
    transition_cost_us: u64,
    parallel_expand_cost_us: u64,
    max_budget_time_estimate_us: u64,
}

const AXIS_BLOCKING_MIGRATION_WARNING_CODE: &str = "MIG-AXIS-BLOCK-001";

impl Default for RuntimeBudgetThresholds {
    fn default() -> Self {
        Self {
            max_actions_per_transition: 16,
            max_actions_per_tick_upper_bound: 512,
            max_parallel_branches: 8,
            max_race_branches: 8,
            warn_on_same_tick_cycle: true,
            action_cost_us: 8,
            transition_cost_us: 5,
            parallel_expand_cost_us: 12,
            max_budget_time_estimate_us: 2_000,
        }
    }
}

impl RuntimeBudgetThresholds {
    fn from_env() -> Self {
        let mut out = Self::default();
        if let Ok(v) = env::var("RUST_PLC_BUDGET_MAX_ACTIONS_PER_TRANSITION") {
            if let Ok(n) = v.parse::<usize>() {
                out.max_actions_per_transition = n;
            }
        }
        if let Ok(v) = env::var("RUST_PLC_BUDGET_MAX_ACTIONS_PER_TICK") {
            if let Ok(n) = v.parse::<usize>() {
                out.max_actions_per_tick_upper_bound = n;
            }
        }
        if let Ok(v) = env::var("RUST_PLC_BUDGET_MAX_PARALLEL_BRANCHES") {
            if let Ok(n) = v.parse::<usize>() {
                out.max_parallel_branches = n;
            }
        }
        if let Ok(v) = env::var("RUST_PLC_BUDGET_MAX_RACE_BRANCHES") {
            if let Ok(n) = v.parse::<usize>() {
                out.max_race_branches = n;
            }
        }
        if let Ok(v) = env::var("RUST_PLC_BUDGET_WARN_ON_SAME_TICK_CYCLE") {
            if let Ok(b) = v.parse::<bool>() {
                out.warn_on_same_tick_cycle = b;
            }
        }
        if let Ok(v) = env::var("RUST_PLC_BUDGET_ACTION_COST_US") {
            if let Ok(n) = v.parse::<u64>() {
                out.action_cost_us = n;
            }
        }
        if let Ok(v) = env::var("RUST_PLC_BUDGET_TRANSITION_COST_US") {
            if let Ok(n) = v.parse::<u64>() {
                out.transition_cost_us = n;
            }
        }
        if let Ok(v) = env::var("RUST_PLC_BUDGET_PARALLEL_EXPAND_COST_US") {
            if let Ok(n) = v.parse::<u64>() {
                out.parallel_expand_cost_us = n;
            }
        }
        if let Ok(v) = env::var("RUST_PLC_BUDGET_MAX_TIME_ESTIMATE_US") {
            if let Ok(n) = v.parse::<u64>() {
                out.max_budget_time_estimate_us = n;
            }
        }
        out
    }
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

static SIM_PROGRAM: Program<'static> = Program {
    tasks: &SIM_TASKS,
    pid_loops: &[],
    var_init: &[],
    cam_configs: &[],
    cam_tables: &[],
    axis_fault_policies: &[],
    semantic_resources: &[],
    resource_claims: &[],
    workpiece_types: &[],
    workpiece_sites: &[],
    workpiece_holders: &[],
};

fn main() {
    cli::run();
}

fn run_compile_command(program: String, path: String, remaining: Vec<String>) {
    let mut args = remaining.into_iter();
    let mut report_path: Option<PathBuf> = None;
    let mut no_print_ir = false;
    let mut ir_out_path: Option<PathBuf> = None;
    let mut deny_warnings = false;
    let mut budget_thresholds = RuntimeBudgetThresholds::from_env();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--report" => {
                let value = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --report <file>");
                    std::process::exit(1);
                });
                report_path = Some(PathBuf::from(value));
            }
            "--no-print-ir" => {
                no_print_ir = true;
            }
            "--ir-out" => {
                let value = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --ir-out <file>");
                    std::process::exit(1);
                });
                ir_out_path = Some(PathBuf::from(value));
            }
            "--deny-warnings" => {
                deny_warnings = true;
            }
            "--budget-max-actions-per-transition" => {
                let value = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --budget-max-actions-per-transition <n>");
                    std::process::exit(1);
                });
                budget_thresholds.max_actions_per_transition =
                    value.parse::<usize>().unwrap_or_else(|_| {
                        eprintln!(
                            "Invalid integer for --budget-max-actions-per-transition: {value}"
                        );
                        std::process::exit(1);
                    });
            }
            "--budget-max-actions-per-tick" => {
                let value = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --budget-max-actions-per-tick <n>");
                    std::process::exit(1);
                });
                budget_thresholds.max_actions_per_tick_upper_bound =
                    value.parse::<usize>().unwrap_or_else(|_| {
                        eprintln!("Invalid integer for --budget-max-actions-per-tick: {value}");
                        std::process::exit(1);
                    });
            }
            "--budget-max-parallel-branches" => {
                let value = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --budget-max-parallel-branches <n>");
                    std::process::exit(1);
                });
                budget_thresholds.max_parallel_branches =
                    value.parse::<usize>().unwrap_or_else(|_| {
                        eprintln!("Invalid integer for --budget-max-parallel-branches: {value}");
                        std::process::exit(1);
                    });
            }
            "--budget-max-race-branches" => {
                let value = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --budget-max-race-branches <n>");
                    std::process::exit(1);
                });
                budget_thresholds.max_race_branches = value.parse::<usize>().unwrap_or_else(|_| {
                    eprintln!("Invalid integer for --budget-max-race-branches: {value}");
                    std::process::exit(1);
                });
            }
            "--budget-warn-on-same-tick-cycle" => {
                let value = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --budget-warn-on-same-tick-cycle <true|false>");
                    std::process::exit(1);
                });
                budget_thresholds.warn_on_same_tick_cycle =
                    value.parse::<bool>().unwrap_or_else(|_| {
                        eprintln!("Invalid boolean for --budget-warn-on-same-tick-cycle: {value}");
                        std::process::exit(1);
                    });
            }
            "--budget-action-cost-us" => {
                let value = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --budget-action-cost-us <n>");
                    std::process::exit(1);
                });
                budget_thresholds.action_cost_us = value.parse::<u64>().unwrap_or_else(|_| {
                    eprintln!("Invalid integer for --budget-action-cost-us: {value}");
                    std::process::exit(1);
                });
            }
            "--budget-transition-cost-us" => {
                let value = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --budget-transition-cost-us <n>");
                    std::process::exit(1);
                });
                budget_thresholds.transition_cost_us = value.parse::<u64>().unwrap_or_else(|_| {
                    eprintln!("Invalid integer for --budget-transition-cost-us: {value}");
                    std::process::exit(1);
                });
            }
            "--budget-parallel-expand-cost-us" => {
                let value = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --budget-parallel-expand-cost-us <n>");
                    std::process::exit(1);
                });
                budget_thresholds.parallel_expand_cost_us =
                    value.parse::<u64>().unwrap_or_else(|_| {
                        eprintln!("Invalid integer for --budget-parallel-expand-cost-us: {value}");
                        std::process::exit(1);
                    });
            }
            "--budget-max-time-estimate-us" => {
                let value = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --budget-max-time-estimate-us <n>");
                    std::process::exit(1);
                });
                budget_thresholds.max_budget_time_estimate_us =
                    value.parse::<u64>().unwrap_or_else(|_| {
                        eprintln!("Invalid integer for --budget-max-time-estimate-us: {value}");
                        std::process::exit(1);
                    });
            }
            "-h" | "--help" => {
                cli_support::help::print_command_help_and_exit(&program, "compile", 0);
            }
            other => {
                eprintln!("Unknown argument: {other}");
                cli_support::help::print_usage(&program);
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
    let mut ir_bundle = ir_bundle;
    apply_runtime_budget_warnings(
        &mut ir_bundle.verification,
        &mut ir_bundle.runtime_budget,
        budget_thresholds,
    );

    let report_path =
        report_path.unwrap_or_else(|| default_verification_report_path(Path::new(&path)));
    if let Err(err) = write_verification_report(
        &path,
        &report_path,
        &ir_bundle.runtime_budget,
        &ir_bundle.verification,
    ) {
        eprintln!("{err}");
        std::process::exit(1);
    }

    print_success_summary(&ir_bundle.verification);
    eprintln!("verification_report: {}", report_path.display());
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

    if let Some(ir_out_path) = ir_out_path {
        if let Some(parent) = ir_out_path.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(err) = fs::create_dir_all(parent) {
                    eprintln!("Failed to create output directory {parent:?}: {err}");
                    std::process::exit(1);
                }
            }
        }
        match serde_json::to_string_pretty(&ir_bundle) {
            Ok(mut json) => {
                json.push('\n');
                if let Err(err) = fs::write(&ir_out_path, json) {
                    eprintln!("Failed to write IR JSON file {ir_out_path:?}: {err}");
                    std::process::exit(1);
                }
                eprintln!("ir_bundle: {}", ir_out_path.display());
            }
            Err(err) => {
                eprintln!("Failed to serialize IR as JSON: {err}");
                std::process::exit(1);
            }
        }
    }

    if !no_print_ir {
        match serde_json::to_string_pretty(&ir_bundle) {
            Ok(json) => println!("{json}"),
            Err(err) => {
                eprintln!("Failed to serialize IR as JSON: {err}");
                std::process::exit(1);
            }
        }
    }
}
fn write_scaffold_file(path: &Path, content: &str, force: bool) -> Result<(), String> {
    if path.exists() && !force {
        return Err(format!(
            "Refusing to overwrite existing file {} (use --force to allow overwrite)",
            path.display()
        ));
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to create directory {}: {err}", parent.display()))?;
        }
    }
    fs::write(path, content).map_err(|err| format!("Failed to write {}: {err}", path.display()))
}

fn prettify_project_name(raw: &str) -> String {
    let parts: Vec<String> = raw
        .split(|c: char| c == '_' || c == '-' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            let mut out = String::new();
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
            out
        })
        .collect();
    if parts.is_empty() {
        "RustPLC Project".to_string()
    } else {
        parts.join(" ")
    }
}

fn run_new_subcommand(program: &str, mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let usage = command_usage(program, "new");
    let Some(project_dir) = args.next() else {
        return Err(usage);
    };
    let mut force = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--force" => force = true,
            "-h" | "--help" => return Err(usage.clone()),
            other => return Err(format!("Unknown argument for new: {other}")),
        }
    }

    let root = PathBuf::from(project_dir);
    if root.exists() {
        if !root.is_dir() {
            return Err(format!(
                "Target path exists but is not a directory: {}",
                root.display()
            ));
        }
        if !force {
            let mut entries = fs::read_dir(&root)
                .map_err(|err| format!("Failed to inspect {}: {err}", root.display()))?;
            if entries.next().is_some() {
                return Err(format!(
                    "Target directory {} is not empty (use --force to overwrite known files)",
                    root.display()
                ));
            }
        }
    } else {
        fs::create_dir_all(&root)
            .map_err(|err| format!("Failed to create {}: {err}", root.display()))?;
    }

    let project_slug = root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("rustplc_project")
        .to_string();
    let project_title = prettify_project_name(&project_slug);

    let readme = format!(
        "# {project_title}\n\n## Project Identity\n\n- Project slug: `{project_slug}`\n- Manifest: `rustplc.project.toml`\n\n## Project Layout\n\n- `plc/main.system.md`: human/AI confirmed system intent\n- `plc/main.plc`: executable RustPLC DSL\n- `scenarios/nominal/normal.yaml`: nominal regression scenario\n- `config/io_map.toml`: deployment I/O mapping\n- `config/retain.toml`: retain/persistence baseline\n- `out/`: all generated artifacts (sim/gate/codegen/build/release)\n\n## Quick Start Checklist\n\n1. Validate scenario contract:\n\n```bash\ncargo run --release --bin rust_plc -- scenario-validate plc/main.plc --scenario scenarios/nominal/normal.yaml --output human\n```\n\n2. Run diagnostic pre-check (`scenario-doctor`):\n\n```bash\ncargo run --release --bin rust_plc -- scenario-doctor plc/main.plc --scenario scenarios/nominal/normal.yaml --output human\n```\n\n3. Run no-board regression gate:\n\n```bash\ncargo run --release --bin rust_plc -- no-board-gate plc/main.plc --scenario scenarios/nominal/normal.yaml --out-dir out/gate/no_board/normal --output human\n```\n\n4. Generate ST code (optional):\n\n```bash\ncargo run --release --bin rust_plc -- gen-st plc/main.plc --out out/codegen/st/main.st\n```\n\n5. Optional RP2040 build baseline:\n\n```bash\ncargo run --release --bin rust_plc -- build-rp2040 plc/main.plc --out out/rp2040 --io-map config/io_map.toml\n```\n\n## VS Code\n\n- Open Command Palette and run `Tasks: Run Task`.\n- Use prefixed tasks (`RustPLC: ...`) from `.vscode/tasks.json`.\n- See `.vscode/README.md` for troubleshooting.\n"
    );
    let gitignore = "/out/**\n!/out/\n!/out/**/\n!/out/**/.gitkeep\n";
    let system = format!(
        "# {project_title} System Description\n\n## 项目身份\n- **项目名称**：{project_title}\n- **项目代号**：`{project_slug}`\n- **所属行业**：教学 / 样机\n- **部署场所**：实验台\n- **最终用户**：控制工程师\n- **监管要求**：无\n\n## 系统使命\n这是一套最小 RustPLC 启动项目，用于把系统意图、PLC 逻辑、场景、I/O 映射和交付产物组织成完整工程。\n\n## 安全与可靠性定位\n- **安全等级**：常规工业防护\n- **故障后果**：演示失败或设备误动作\n- **容错策略**：等待超时后进入 safe stop\n\n## 运行环境\n- **介质**：数字 I/O 演示\n- **电源**：DC 24V\n- **控制器**：RustPLC + RP2040 示例链路\n- **通信**：无\n- **环境条件**：室内调试环境\n\n## 核心工艺意图\n\n### 正常流程\n1. 等待启动输入 `X0`\n2. 启动后置位输出 `Y0`\n3. 延时 20ms 后关闭输出\n4. 流程结束并停机\n\n### 异常处理\n- 若启动信号在 100ms 内未到达，则进入 `fault` task 执行安全关闭。\n\n### 特殊工况\n- 当前骨架未启用手动模式、维护模式与并发 task。\n\n## 并发与阻塞语义假设\n- **并发 task 划分**：当前仅 `main` 与 `fault/done` 这类简单控制流\n- **blocking step 清单**：`wait`、`delay`\n- **阻塞隔离预期**：后续若拆分并发 task，某 task 阻塞不得阻塞其他 task\n- **共享资源边界**：`Y0` 为当前唯一执行输出\n\n## 启动与停机流程\n\n### 上电初始化\n- 上电后默认输出全关，等待启动信号。\n\n### 正常停止\n- 正常流程结束后关闭 `Y0` 并进入 `done`。\n\n### 急停流程\n- 本骨架未建模独立急停，实际项目应补充硬件急停与安全回路。\n\n## 调试与测试策略\n\n### 手动测试\n- 可直接修改 `scenarios/nominal/normal.yaml` 验证输入边沿。\n\n### 自动测试\n- 先跑 `scenario-validate`，再跑 `no-board-gate`。\n\n### 单步模式\n- 当前未提供，调试阶段可借助 trace 观察 step 推进。\n"
    );
    let plc = "[topology]\n\ndevice plc_main: plc {\n    purpose: \"控制器本体与数字I/O端口映射\",\n    model_ref: openplc_softplc\n}\n\n[constraints]\n\n[tasks]\n\ntask main:\n    step wait_start:\n        wait: X0 == true\n        timeout: 100ms -> goto fault\n\n    step run:\n        action: set Y0 on\n        delay: 20ms\n\n    step stop:\n        action: set Y0 off\n\n    on_complete: goto done\n\ntask fault:\n    step safe_stop:\n        action: set Y0 off\n    on_complete: goto done\n\ntask done:\n    step halt:\n";
    let scenario = "tick_ms: 10\nduration_ms: 300\ninputs:\n  - at_ms: 0\n    set:\n      digital_inputs:\n        0: true\n  - at_ms: 50\n    set:\n      digital_inputs:\n        0: false\nforces: []\n";
    let io_map = "schema_version = 1\n\n[digital_inputs]\ndi0 = { gpio = 2, pull = \"up\" }\n\n[digital_outputs]\ndo0 = { gpio = 10, active_low = false }\n\n[safe_state]\nmode = \"all_zero\"\non_exit_timeout_ms = 0\n";
    let retain =
        "schema_version = 1\n\n[retain]\nenabled = false\npath = \"out/sim/retain_state.json\"\n";
    let manifest = format!(
        "schema_version = 1\n\n[project]\nname = \"{project_title}\"\nslug = \"{project_slug}\"\n\n[entry]\nsystem = \"plc/main.system.md\"\nplc = \"plc/main.plc\"\nscenario = \"scenarios/nominal/normal.yaml\"\nio_map = \"config/io_map.toml\"\nretain = \"config/retain.toml\"\n\n[out]\nir = \"out/ir\"\nsim = \"out/sim\"\ngate = \"out/gate\"\ncodegen = \"out/codegen\"\nrp2040 = \"out/rp2040\"\nrelease = \"out/release\"\n"
    );
    let project_layout = format!(
        "# Project Layout\n\n这个脚手架采用固定的 RustPLC 项目目录约定：\n\n- `rustplc.project.toml`：项目清单，声明主入口与默认路径\n- `plc/`：系统语义与 DSL 源码\n- `scenarios/`：版本化场景输入\n- `config/`：I/O 与运行配置\n- `out/`：所有可重建产物\n\n当前项目：`{project_slug}` / `{project_title}`\n\n推荐命令：\n\n```bash\ncargo run --release --bin rust_plc -- scenario-validate \\\n  plc/main.plc --scenario scenarios/nominal/normal.yaml --output human\n\ncargo run --release --bin rust_plc -- sim-plc \\\n  plc/main.plc --scenario scenarios/nominal/normal.yaml --out out/sim/normal/trace.jsonl\n\ncargo run --release --bin rust_plc -- no-board-gate \\\n  plc/main.plc --scenario scenarios/nominal/normal.yaml \\\n  --out-dir out/gate/no_board/normal --output human\n\ncargo run --release --bin rust_plc -- gen-st \\\n  plc/main.plc --out out/codegen/st/main.st\n\ncargo run --release --bin rust_plc -- build-rp2040 \\\n  plc/main.plc --out out/rp2040 --io-map config/io_map.toml\n```\n"
    );
    let workflow = "name: rustplc-no-board-gate\n\non:\n  push:\n  pull_request:\n\njobs:\n  no-board-gate:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n      - uses: dtolnay/rust-toolchain@stable\n      - name: Scenario validate\n        run: cargo run --release --bin rust_plc -- scenario-validate plc/main.plc --scenario scenarios/nominal/normal.yaml --output json\n      - name: No-board gate\n        run: cargo run --release --bin rust_plc -- no-board-gate plc/main.plc --scenario scenarios/nominal/normal.yaml --out-dir out/gate/no_board/normal --output json\n";
    let vscode_tasks = "{\n  \"version\": \"2.0.0\",\n  \"tasks\": [\n    {\n      \"label\": \"RustPLC: scenario-init (normal)\",\n      \"type\": \"shell\",\n      \"command\": \"cargo run --release --bin rust_plc -- scenario-init plc/main.plc --preset normal --out scenarios/nominal/normal.yaml\",\n      \"problemMatcher\": []\n    },\n    {\n      \"label\": \"RustPLC: scenario-validate\",\n      \"type\": \"shell\",\n      \"command\": \"cargo run --release --bin rust_plc -- scenario-validate plc/main.plc --scenario scenarios/nominal/normal.yaml --output human\",\n      \"problemMatcher\": []\n    },\n    {\n      \"label\": \"RustPLC: scenario-doctor\",\n      \"type\": \"shell\",\n      \"command\": \"cargo run --release --bin rust_plc -- scenario-doctor plc/main.plc --scenario scenarios/nominal/normal.yaml --fix-preview --output human\",\n      \"problemMatcher\": []\n    },\n    {\n      \"label\": \"RustPLC: sim-plc\",\n      \"type\": \"shell\",\n      \"command\": \"cargo run --release --bin rust_plc -- sim-plc plc/main.plc --scenario scenarios/nominal/normal.yaml --out out/sim/normal/trace.jsonl\",\n      \"problemMatcher\": []\n    },\n    {\n      \"label\": \"RustPLC: no-board-gate\",\n      \"type\": \"shell\",\n      \"command\": \"cargo run --release --bin rust_plc -- no-board-gate plc/main.plc --scenario scenarios/nominal/normal.yaml --out-dir out/gate/no_board/normal --output human\",\n      \"problemMatcher\": []\n    },\n    {\n      \"label\": \"RustPLC: gen-st\",\n      \"type\": \"shell\",\n      \"command\": \"cargo run --release --bin rust_plc -- gen-st plc/main.plc --out out/codegen/st/main.st\",\n      \"problemMatcher\": []\n    },\n    {\n      \"label\": \"RustPLC: build-rp2040\",\n      \"type\": \"shell\",\n      \"command\": \"cargo run --release --bin rust_plc -- build-rp2040 plc/main.plc --out out/rp2040 --io-map config/io_map.toml\",\n      \"problemMatcher\": []\n    }\n  ]\n}\n";
    let vscode_settings = "{\n  \"files.associations\": {\n    \"*.plc\": \"ini\"\n  },\n  \"editor.tabSize\": 4,\n  \"editor.insertSpaces\": true,\n  \"editor.detectIndentation\": false\n}\n";
    let vscode_extensions = "{\n  \"recommendations\": [\n    \"rust-lang.rust-analyzer\",\n    \"redhat.vscode-yaml\",\n    \"tamasfe.even-better-toml\",\n    \"streetsidesoftware.code-spell-checker\"\n  ]\n}\n";
    let vscode_snippets = "{\n  \"RustPLC: PLC Skeleton\": {\n    \"scope\": \"ini\",\n    \"prefix\": \"plc-skeleton\",\n    \"body\": [\n      \"[topology]\",\n      \"\",\n      \"device plc_main: plc {\",\n      \"    purpose: \\\"控制器本体与数字I/O端口映射\\\",\",\n      \"    model_ref: openplc_softplc\",\n      \"}\",\n      \"\",\n      \"[constraints]\",\n      \"\",\n      \"[tasks]\",\n      \"\",\n      \"task main:\",\n      \"    step wait_start:\",\n      \"        wait: X0 == true\",\n      \"\",\n      \"    step run:\",\n      \"        action: set Y0 on\",\n      \"\",\n      \"    on_complete: goto done\",\n      \"\",\n      \"task done:\",\n      \"    step halt:\"\n    ],\n    \"description\": \"Insert a minimal RustPLC file skeleton\"\n  },\n  \"RustPLC: Wait With Timeout\": {\n    \"scope\": \"ini\",\n    \"prefix\": \"plc-wait-timeout\",\n    \"body\": [\n      \"wait: ${1:X0} == ${2:true}\",\n      \"timeout: ${3:100ms} -> goto ${4:fault}\"\n    ],\n    \"description\": \"Insert wait+timeout pair\"\n  }\n}\n";
    let vscode_readme = "# VS Code Day-1 Support for RustPLC\n\n## What this package provides\n\n- `settings.json`: associates `*.plc` with INI highlighting (fallback strategy)\n- `plc.code-snippets`: starter snippets for skeletons and wait/timeout patterns\n- `tasks.json`: one-click commands for scenario-init/doctor/sim/gate/gen-st/build\n- `extensions.json`: recommended extensions for Rust/YAML/TOML/spell-check\n\n## Highlight strategy\n\nRustPLC currently uses a lightweight no-extension strategy in scaffold projects:\n\n- `*.plc` -> `ini` language mode\n- snippets + tasks provide practical editing/iteration support\n\n## Troubleshooting\n\n1. If snippets do not appear:\n   - confirm file is `*.plc`\n   - run `Developer: Reload Window`\n2. If tasks fail with \"command not found\":\n   - ensure `cargo` is on PATH\n   - run tasks from workspace root\n3. If YAML/TOML diagnostics are missing:\n   - install recommended extensions from `.vscode/extensions.json`\n";

    write_scaffold_file(&root.join("README.md"), &readme, force)?;
    write_scaffold_file(&root.join(".gitignore"), gitignore, force)?;
    write_scaffold_file(&root.join("rustplc.project.toml"), &manifest, force)?;
    write_scaffold_file(&root.join("plc/main.system.md"), &system, force)?;
    write_scaffold_file(&root.join("plc/main.plc"), plc, force)?;
    write_scaffold_file(&root.join("scenarios/nominal/normal.yaml"), scenario, force)?;
    write_scaffold_file(&root.join("scenarios/faults/.gitkeep"), "", force)?;
    write_scaffold_file(&root.join("scenarios/generated/.gitkeep"), "", force)?;
    write_scaffold_file(&root.join("config/io_map.toml"), io_map, force)?;
    write_scaffold_file(&root.join("config/retain.toml"), retain, force)?;
    write_scaffold_file(&root.join("docs/project-layout.md"), &project_layout, force)?;
    write_scaffold_file(&root.join("out/ir/.gitkeep"), "", force)?;
    write_scaffold_file(&root.join("out/sim/.gitkeep"), "", force)?;
    write_scaffold_file(&root.join("out/gate/.gitkeep"), "", force)?;
    write_scaffold_file(&root.join("out/codegen/.gitkeep"), "", force)?;
    write_scaffold_file(&root.join("out/rp2040/.gitkeep"), "", force)?;
    write_scaffold_file(&root.join("out/release/.gitkeep"), "", force)?;
    write_scaffold_file(
        &root.join(".github/workflows/no_board_gate.yml"),
        workflow,
        force,
    )?;
    write_scaffold_file(&root.join(".vscode/tasks.json"), vscode_tasks, force)?;
    write_scaffold_file(&root.join(".vscode/settings.json"), vscode_settings, force)?;
    write_scaffold_file(
        &root.join(".vscode/extensions.json"),
        vscode_extensions,
        force,
    )?;
    write_scaffold_file(
        &root.join(".vscode/plc.code-snippets"),
        vscode_snippets,
        force,
    )?;
    write_scaffold_file(&root.join(".vscode/README.md"), vscode_readme, force)?;

    eprintln!("new: scaffold created at {}", root.display());
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OnlineForceChannelKind {
    Di,
    Ai,
    Do,
    Ao,
}

impl OnlineForceChannelKind {
    fn label(self) -> &'static str {
        match self {
            Self::Di => "digital_input",
            Self::Ai => "analog_input",
            Self::Do => "digital_output",
            Self::Ao => "analog_output",
        }
    }

    fn short(self) -> &'static str {
        match self {
            Self::Di => "di",
            Self::Ai => "ai",
            Self::Do => "do",
            Self::Ao => "ao",
        }
    }
}

#[derive(Debug, Clone)]
enum OnlineForceValue {
    Digital(bool),
    Analog(f32),
}

#[derive(Debug, Clone)]
struct OnlineForceCommand {
    at_ms: u64,
    actor: String,
    source: String,
    channel_kind: OnlineForceChannelKind,
    channel_id: u16,
    value: Option<OnlineForceValue>,
}

#[derive(Debug, Deserialize)]
struct OnlineForceScriptEntryRaw {
    at_ms: u64,
    actor: String,
    source: String,
    channel: String,
    #[serde(default)]
    value: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum ForceAuditValue {
    Digital(bool),
    Analog(f32),
}

#[derive(Debug, Serialize)]
struct OnlineForceAuditEntry {
    at_ms: u64,
    tick: u64,
    actor: String,
    source: String,
    channel: String,
    channel_kind: &'static str,
    channel_id: u16,
    operation: &'static str,
    from: Option<ForceAuditValue>,
    to: Option<ForceAuditValue>,
}

fn parse_online_force_channel(raw: &str) -> Result<(OnlineForceChannelKind, u16), String> {
    let token = raw.trim().to_ascii_lowercase();
    let (kind, tail) = if let Some(v) = token.strip_prefix("di") {
        (OnlineForceChannelKind::Di, v)
    } else if let Some(v) = token.strip_prefix("ai") {
        (OnlineForceChannelKind::Ai, v)
    } else if let Some(v) = token.strip_prefix("do") {
        (OnlineForceChannelKind::Do, v)
    } else if let Some(v) = token.strip_prefix("ao") {
        (OnlineForceChannelKind::Ao, v)
    } else {
        return Err(format!(
            "invalid channel `{raw}` (expected DI<n>/AI<n>/DO<n>/AO<n>)"
        ));
    };

    if tail.is_empty() {
        return Err(format!(
            "invalid channel `{raw}` (missing numeric id after kind prefix)"
        ));
    }
    let id = tail
        .parse::<u16>()
        .map_err(|_| format!("invalid channel `{raw}` (id must be u16)"))?;
    Ok((kind, id))
}

fn parse_online_force_value(
    raw: Option<serde_json::Value>,
    kind: OnlineForceChannelKind,
) -> Result<Option<OnlineForceValue>, String> {
    let Some(v) = raw else {
        return Ok(None);
    };
    match kind {
        OnlineForceChannelKind::Di | OnlineForceChannelKind::Do => match v {
            serde_json::Value::Bool(b) => Ok(Some(OnlineForceValue::Digital(b))),
            serde_json::Value::Null => Ok(None),
            other => Err(format!(
                "{} channel expects bool/null value, got {other}",
                kind.short()
            )),
        },
        OnlineForceChannelKind::Ai | OnlineForceChannelKind::Ao => match v {
            serde_json::Value::Number(n) => {
                let f = n.as_f64().ok_or_else(|| {
                    format!(
                        "{} channel expects numeric/null value, got non-finite number",
                        kind.short()
                    )
                })?;
                if !f.is_finite() {
                    return Err(format!(
                        "{} channel expects finite numeric/null value",
                        kind.short()
                    ));
                }
                Ok(Some(OnlineForceValue::Analog(f as f32)))
            }
            serde_json::Value::Null => Ok(None),
            other => Err(format!(
                "{} channel expects numeric/null value, got {other}",
                kind.short()
            )),
        },
    }
}

fn load_online_force_script(path: &Path, tick_ms: u64) -> Result<Vec<OnlineForceCommand>, String> {
    let body = fs::read_to_string(path).map_err(|err| {
        format!(
            "Failed to read online-force script {}: {err}",
            path.display()
        )
    })?;
    let mut commands = Vec::<OnlineForceCommand>::new();
    for (lineno, line) in body.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let raw: OnlineForceScriptEntryRaw = serde_json::from_str(trimmed)
            .map_err(|err| format!("Invalid JSONL at {}:{}: {err}", path.display(), lineno + 1))?;
        if tick_ms != 0 && raw.at_ms % tick_ms != 0 {
            return Err(format!(
                "at_ms={} is not aligned to tick_ms={} at {}:{}",
                raw.at_ms,
                tick_ms,
                path.display(),
                lineno + 1
            ));
        }
        let (kind, id) = parse_online_force_channel(&raw.channel)
            .map_err(|err| format!("{err} at {}:{}", path.display(), lineno + 1))?;
        let value = parse_online_force_value(raw.value, kind)
            .map_err(|err| format!("{err} at {}:{}", path.display(), lineno + 1))?;
        commands.push(OnlineForceCommand {
            at_ms: raw.at_ms,
            actor: raw.actor,
            source: raw.source,
            channel_kind: kind,
            channel_id: id,
            value,
        });
    }
    commands.sort_by(|a, b| a.at_ms.cmp(&b.at_ms));
    Ok(commands)
}

fn build_online_force_audit(
    commands: &[OnlineForceCommand],
    tick_ms: u64,
) -> Vec<OnlineForceAuditEntry> {
    let mut out = Vec::<OnlineForceAuditEntry>::new();
    let mut di = BTreeMap::<u16, bool>::new();
    let mut ai = BTreeMap::<u16, f32>::new();
    let mut do_ = BTreeMap::<u16, bool>::new();
    let mut ao = BTreeMap::<u16, f32>::new();

    for cmd in commands {
        let (from, to) = match cmd.channel_kind {
            OnlineForceChannelKind::Di => {
                let before = di
                    .get(&cmd.channel_id)
                    .copied()
                    .map(ForceAuditValue::Digital);
                match cmd.value.as_ref() {
                    Some(OnlineForceValue::Digital(v)) => {
                        di.insert(cmd.channel_id, *v);
                        (before, Some(ForceAuditValue::Digital(*v)))
                    }
                    None => {
                        di.remove(&cmd.channel_id);
                        (before, None)
                    }
                    Some(OnlineForceValue::Analog(_)) => continue,
                }
            }
            OnlineForceChannelKind::Ai => {
                let before = ai
                    .get(&cmd.channel_id)
                    .copied()
                    .map(ForceAuditValue::Analog);
                match cmd.value.as_ref() {
                    Some(OnlineForceValue::Analog(v)) => {
                        ai.insert(cmd.channel_id, *v);
                        (before, Some(ForceAuditValue::Analog(*v)))
                    }
                    None => {
                        ai.remove(&cmd.channel_id);
                        (before, None)
                    }
                    Some(OnlineForceValue::Digital(_)) => continue,
                }
            }
            OnlineForceChannelKind::Do => {
                let before = do_
                    .get(&cmd.channel_id)
                    .copied()
                    .map(ForceAuditValue::Digital);
                match cmd.value.as_ref() {
                    Some(OnlineForceValue::Digital(v)) => {
                        do_.insert(cmd.channel_id, *v);
                        (before, Some(ForceAuditValue::Digital(*v)))
                    }
                    None => {
                        do_.remove(&cmd.channel_id);
                        (before, None)
                    }
                    Some(OnlineForceValue::Analog(_)) => continue,
                }
            }
            OnlineForceChannelKind::Ao => {
                let before = ao
                    .get(&cmd.channel_id)
                    .copied()
                    .map(ForceAuditValue::Analog);
                match cmd.value.as_ref() {
                    Some(OnlineForceValue::Analog(v)) => {
                        ao.insert(cmd.channel_id, *v);
                        (before, Some(ForceAuditValue::Analog(*v)))
                    }
                    None => {
                        ao.remove(&cmd.channel_id);
                        (before, None)
                    }
                    Some(OnlineForceValue::Digital(_)) => continue,
                }
            }
        };

        out.push(OnlineForceAuditEntry {
            at_ms: cmd.at_ms,
            tick: if tick_ms == 0 { 0 } else { cmd.at_ms / tick_ms },
            actor: cmd.actor.clone(),
            source: cmd.source.clone(),
            channel: format!("{}{}", cmd.channel_kind.short(), cmd.channel_id),
            channel_kind: cmd.channel_kind.label(),
            channel_id: cmd.channel_id,
            operation: if cmd.value.is_some() { "set" } else { "clear" },
            from,
            to,
        });
    }

    out
}

fn inject_online_force_commands(
    scenario: &mut sim::Scenario,
    commands: &[OnlineForceCommand],
) -> Result<(), String> {
    let mut by_at = BTreeMap::<u64, sim::ForceSet>::new();
    for cmd in commands {
        let set = by_at.entry(cmd.at_ms).or_default();
        match (cmd.channel_kind, cmd.value.as_ref()) {
            (OnlineForceChannelKind::Di, Some(OnlineForceValue::Digital(v))) => {
                set.digital_inputs.insert(cmd.channel_id, Some(*v));
            }
            (OnlineForceChannelKind::Di, None) => {
                set.digital_inputs.insert(cmd.channel_id, None);
            }
            (OnlineForceChannelKind::Ai, Some(OnlineForceValue::Analog(v))) => {
                set.analog_inputs.insert(cmd.channel_id, Some(*v));
            }
            (OnlineForceChannelKind::Ai, None) => {
                set.analog_inputs.insert(cmd.channel_id, None);
            }
            (OnlineForceChannelKind::Do, Some(OnlineForceValue::Digital(v))) => {
                set.digital_outputs.insert(cmd.channel_id, Some(*v));
            }
            (OnlineForceChannelKind::Do, None) => {
                set.digital_outputs.insert(cmd.channel_id, None);
            }
            (OnlineForceChannelKind::Ao, Some(OnlineForceValue::Analog(v))) => {
                set.analog_outputs.insert(cmd.channel_id, Some(*v));
            }
            (OnlineForceChannelKind::Ao, None) => {
                set.analog_outputs.insert(cmd.channel_id, None);
            }
            _ => {
                return Err(format!(
                    "online-force value type mismatch at {}{}",
                    cmd.channel_kind.short(),
                    cmd.channel_id
                ));
            }
        }
    }

    for (at_ms, set) in by_at {
        scenario.forces.push(sim::ForceEvent { at_ms, set });
    }
    scenario.forces.sort_by_key(|event| event.at_ms);
    Ok(())
}

fn default_online_force_audit_path(trace_out: &Path) -> PathBuf {
    trace_out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join("online_force_audit.jsonl")
}

fn write_online_force_audit(path: &Path, entries: &[OnlineForceAuditEntry]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "Failed to create online-force audit directory {}: {err}",
                    parent.display()
                )
            })?;
        }
    }

    let file = fs::File::create(path).map_err(|err| {
        format!(
            "Failed to create online-force audit {}: {err}",
            path.display()
        )
    })?;
    let mut writer = std::io::BufWriter::new(file);
    for entry in entries {
        let line = serde_json::to_string(entry)
            .map_err(|err| format!("Failed to serialize online-force audit entry: {err}"))?;
        writer.write_all(line.as_bytes()).map_err(|err| {
            format!(
                "Failed to write online-force audit {}: {err}",
                path.display()
            )
        })?;
        writer.write_all(b"\n").map_err(|err| {
            format!(
                "Failed to write online-force audit {}: {err}",
                path.display()
            )
        })?;
    }
    writer.flush().map_err(|err| {
        format!(
            "Failed to flush online-force audit {}: {err}",
            path.display()
        )
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OnlineVariableKind {
    Bool,
    Real,
}

impl OnlineVariableKind {
    fn label(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Real => "real",
        }
    }
}

#[derive(Debug, Clone)]
enum OnlineVariableValue {
    Bool(bool),
    Real(f32),
}

#[derive(Debug, Clone)]
struct OnlineVariableCommand {
    at_ms: u64,
    actor: String,
    source: String,
    variable_kind: OnlineVariableKind,
    variable_name: String,
    variable_key: String,
    value: Option<OnlineVariableValue>,
}

#[derive(Debug, Deserialize)]
struct OnlineVariableScriptEntryRaw {
    at_ms: u64,
    actor: String,
    source: String,
    variable: String,
    #[serde(default)]
    value: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum OnlineVariableAuditValue {
    Bool(bool),
    Real(f32),
}

#[derive(Debug, Serialize)]
struct OnlineVariableAuditEntry {
    at_ms: u64,
    tick: u64,
    actor: String,
    source: String,
    variable: String,
    variable_kind: &'static str,
    bound_channel: Option<String>,
    operation: &'static str,
    from: Option<OnlineVariableAuditValue>,
    to: Option<OnlineVariableAuditValue>,
}

#[derive(Debug, Clone, Default)]
struct OnlineVariableBindings {
    bool_to_di: BTreeMap<String, u16>,
    real_to_ai: BTreeMap<String, u16>,
}

#[derive(Debug, Deserialize)]
struct OnlineVariableBindingsFileRaw {
    #[serde(default = "online_var_binding_schema_version")]
    schema_version: u32,
    #[serde(default)]
    bool: BTreeMap<String, toml::Value>,
    #[serde(default)]
    real: BTreeMap<String, toml::Value>,
}

fn online_var_binding_schema_version() -> u32 {
    1
}

fn normalize_online_variable_name(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

fn parse_online_variable_binding_channel(
    raw: &toml::Value,
    kind: OnlineVariableKind,
    var_name: &str,
) -> Result<u16, String> {
    let prefixes = match kind {
        OnlineVariableKind::Bool => ["di", "x"].as_slice(),
        OnlineVariableKind::Real => ["ai"].as_slice(),
    };
    match raw {
        toml::Value::Integer(v) => {
            if *v < 0 || *v > u16::MAX as i64 {
                return Err(format!(
                    "invalid {} binding for `{}`: integer id out of range for u16",
                    kind.label(),
                    var_name
                ));
            }
            Ok(*v as u16)
        }
        toml::Value::String(s) => parse_retain_channel_id(s, prefixes)
            .map_err(|err| format!("invalid {} binding for `{}`: {err}", kind.label(), var_name)),
        _ => Err(format!(
            "invalid {} binding for `{}`: expected integer id or channel string",
            kind.label(),
            var_name
        )),
    }
}

fn load_online_variable_bindings(path: &Path) -> Result<OnlineVariableBindings, String> {
    let body = fs::read_to_string(path).map_err(|err| {
        format!(
            "Failed to read online-variable bindings {}: {err}",
            path.display()
        )
    })?;
    let raw: OnlineVariableBindingsFileRaw = toml::from_str(&body).map_err(|err| {
        format!(
            "Failed to parse online-variable bindings {}: {err}",
            path.display()
        )
    })?;
    if raw.schema_version != online_var_binding_schema_version() {
        return Err(format!(
            "online-variable bindings schema_version={} is unsupported (expected {})",
            raw.schema_version,
            online_var_binding_schema_version()
        ));
    }

    let mut out = OnlineVariableBindings::default();
    for (name, channel) in &raw.bool {
        let key = normalize_online_variable_name(name);
        if key.is_empty() {
            return Err("online-variable bool binding name cannot be empty".to_string());
        }
        let id = parse_online_variable_binding_channel(channel, OnlineVariableKind::Bool, name)?;
        if out.bool_to_di.insert(key.clone(), id).is_some() {
            return Err(format!(
                "duplicate BOOL binding for `{name}` after normalization"
            ));
        }
    }
    for (name, channel) in &raw.real {
        let key = normalize_online_variable_name(name);
        if key.is_empty() {
            return Err("online-variable real binding name cannot be empty".to_string());
        }
        let id = parse_online_variable_binding_channel(channel, OnlineVariableKind::Real, name)?;
        if out.real_to_ai.insert(key.clone(), id).is_some() {
            return Err(format!(
                "duplicate REAL binding for `{name}` after normalization"
            ));
        }
    }

    Ok(out)
}

fn parse_online_variable_target(raw: &str) -> Result<(OnlineVariableKind, String), String> {
    let token = raw.trim();
    let Some((kind_raw, name_raw)) = token.split_once(':') else {
        return Err(format!(
            "invalid variable `{raw}` (expected BOOL:<name> or REAL:<name>)"
        ));
    };
    let kind = match kind_raw.trim().to_ascii_lowercase().as_str() {
        "bool" | "boolean" => OnlineVariableKind::Bool,
        "real" | "float" | "f32" => OnlineVariableKind::Real,
        _ => {
            return Err(format!(
                "invalid variable `{raw}` (unknown type prefix `{kind_raw}`; expected BOOL or REAL)"
            ));
        }
    };
    let name = name_raw.trim();
    if name.is_empty() {
        return Err(format!("invalid variable `{raw}` (name cannot be empty)"));
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        return Err(format!(
            "invalid variable `{raw}` (name must contain only [A-Za-z0-9_.-])"
        ));
    }
    Ok((kind, name.to_string()))
}

fn parse_online_variable_value(
    raw: Option<serde_json::Value>,
    kind: OnlineVariableKind,
) -> Result<Option<OnlineVariableValue>, String> {
    let Some(v) = raw else {
        return Ok(None);
    };
    match kind {
        OnlineVariableKind::Bool => match v {
            serde_json::Value::Bool(value) => Ok(Some(OnlineVariableValue::Bool(value))),
            serde_json::Value::Null => Ok(None),
            other => Err(format!(
                "BOOL variable expects bool/null value, got {other}"
            )),
        },
        OnlineVariableKind::Real => match v {
            serde_json::Value::Number(value) => {
                let parsed = value
                    .as_f64()
                    .ok_or_else(|| "REAL variable expects finite numeric/null value".to_string())?;
                if !parsed.is_finite() {
                    return Err("REAL variable expects finite numeric/null value".to_string());
                }
                Ok(Some(OnlineVariableValue::Real(parsed as f32)))
            }
            serde_json::Value::Null => Ok(None),
            other => Err(format!(
                "REAL variable expects numeric/null value, got {other}"
            )),
        },
    }
}

fn load_online_variable_script(
    path: &Path,
    tick_ms: u64,
) -> Result<Vec<OnlineVariableCommand>, String> {
    let body = fs::read_to_string(path).map_err(|err| {
        format!(
            "Failed to read online-variable script {}: {err}",
            path.display()
        )
    })?;
    let mut commands = Vec::<OnlineVariableCommand>::new();
    for (lineno, line) in body.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let raw: OnlineVariableScriptEntryRaw = serde_json::from_str(trimmed)
            .map_err(|err| format!("Invalid JSONL at {}:{}: {err}", path.display(), lineno + 1))?;
        if tick_ms != 0 && raw.at_ms % tick_ms != 0 {
            return Err(format!(
                "at_ms={} is not aligned to tick_ms={} at {}:{}",
                raw.at_ms,
                tick_ms,
                path.display(),
                lineno + 1
            ));
        }
        let (kind, name) = parse_online_variable_target(&raw.variable)
            .map_err(|err| format!("{err} at {}:{}", path.display(), lineno + 1))?;
        let value = parse_online_variable_value(raw.value, kind)
            .map_err(|err| format!("{err} at {}:{}", path.display(), lineno + 1))?;
        commands.push(OnlineVariableCommand {
            at_ms: raw.at_ms,
            actor: raw.actor,
            source: raw.source,
            variable_kind: kind,
            variable_key: normalize_online_variable_name(&name),
            variable_name: name,
            value,
        });
    }
    commands.sort_by(|a, b| a.at_ms.cmp(&b.at_ms));
    Ok(commands)
}

fn parse_auto_online_variable_channel_id(
    kind: OnlineVariableKind,
    variable_key: &str,
) -> Option<u16> {
    let prefixes = match kind {
        OnlineVariableKind::Bool => ["di", "x"].as_slice(),
        OnlineVariableKind::Real => ["ai"].as_slice(),
    };
    parse_retain_channel_id(variable_key, prefixes).ok()
}

fn resolve_online_variable_channel(
    cmd: &OnlineVariableCommand,
    bindings: Option<&OnlineVariableBindings>,
) -> Result<u16, String> {
    let from_bindings = bindings.and_then(|defs| match cmd.variable_kind {
        OnlineVariableKind::Bool => defs.bool_to_di.get(&cmd.variable_key).copied(),
        OnlineVariableKind::Real => defs.real_to_ai.get(&cmd.variable_key).copied(),
    });
    if let Some(id) = from_bindings {
        return Ok(id);
    }
    if let Some(id) = parse_auto_online_variable_channel_id(cmd.variable_kind, &cmd.variable_key) {
        return Ok(id);
    }
    Err(format!(
        "missing {} binding for variable `{}`; add --online-var-bindings <bindings.toml> or use auto-mappable names (BOOL:DI<n>, REAL:AI<n>)",
        cmd.variable_kind.label().to_ascii_uppercase(),
        cmd.variable_name
    ))
}

fn inject_online_variable_commands(
    scenario: &mut sim::Scenario,
    commands: &[OnlineVariableCommand],
    bindings: Option<&OnlineVariableBindings>,
) -> Result<(), String> {
    let mut by_at = BTreeMap::<u64, sim::ForceSet>::new();
    for cmd in commands {
        let id = resolve_online_variable_channel(cmd, bindings)?;
        let set = by_at.entry(cmd.at_ms).or_default();
        match (cmd.variable_kind, cmd.value.as_ref()) {
            (OnlineVariableKind::Bool, Some(OnlineVariableValue::Bool(v))) => {
                set.digital_inputs.insert(id, Some(*v));
            }
            (OnlineVariableKind::Bool, None) => {
                set.digital_inputs.insert(id, None);
            }
            (OnlineVariableKind::Real, Some(OnlineVariableValue::Real(v))) => {
                set.analog_inputs.insert(id, Some(*v));
            }
            (OnlineVariableKind::Real, None) => {
                set.analog_inputs.insert(id, None);
            }
            _ => {
                return Err(format!(
                    "online-variable value type mismatch at {}:{}",
                    cmd.variable_kind.label(),
                    cmd.variable_name
                ));
            }
        }
    }
    for (at_ms, set) in by_at {
        scenario.forces.push(sim::ForceEvent { at_ms, set });
    }
    scenario.forces.sort_by_key(|event| event.at_ms);
    Ok(())
}

fn build_online_variable_audit(
    commands: &[OnlineVariableCommand],
    tick_ms: u64,
    bindings: Option<&OnlineVariableBindings>,
) -> Result<Vec<OnlineVariableAuditEntry>, String> {
    let mut out = Vec::<OnlineVariableAuditEntry>::new();
    let mut bool_values = BTreeMap::<String, bool>::new();
    let mut real_values = BTreeMap::<String, f32>::new();

    for cmd in commands {
        let bound_channel =
            resolve_online_variable_channel(cmd, bindings).map(|id| match cmd.variable_kind {
                OnlineVariableKind::Bool => format!("di{id}"),
                OnlineVariableKind::Real => format!("ai{id}"),
            })?;
        let (from, to) = match cmd.variable_kind {
            OnlineVariableKind::Bool => {
                let before = bool_values
                    .get(&cmd.variable_name)
                    .copied()
                    .map(OnlineVariableAuditValue::Bool);
                match cmd.value.as_ref() {
                    Some(OnlineVariableValue::Bool(v)) => {
                        bool_values.insert(cmd.variable_name.clone(), *v);
                        (before, Some(OnlineVariableAuditValue::Bool(*v)))
                    }
                    None => {
                        bool_values.remove(&cmd.variable_name);
                        (before, None)
                    }
                    Some(OnlineVariableValue::Real(_)) => continue,
                }
            }
            OnlineVariableKind::Real => {
                let before = real_values
                    .get(&cmd.variable_name)
                    .copied()
                    .map(OnlineVariableAuditValue::Real);
                match cmd.value.as_ref() {
                    Some(OnlineVariableValue::Real(v)) => {
                        real_values.insert(cmd.variable_name.clone(), *v);
                        (before, Some(OnlineVariableAuditValue::Real(*v)))
                    }
                    None => {
                        real_values.remove(&cmd.variable_name);
                        (before, None)
                    }
                    Some(OnlineVariableValue::Bool(_)) => continue,
                }
            }
        };

        out.push(OnlineVariableAuditEntry {
            at_ms: cmd.at_ms,
            tick: if tick_ms == 0 { 0 } else { cmd.at_ms / tick_ms },
            actor: cmd.actor.clone(),
            source: cmd.source.clone(),
            variable: format!("{}:{}", cmd.variable_kind.label(), cmd.variable_name),
            variable_kind: cmd.variable_kind.label(),
            bound_channel: Some(bound_channel),
            operation: if cmd.value.is_some() { "set" } else { "clear" },
            from,
            to,
        });
    }

    Ok(out)
}

fn default_online_variable_audit_path(trace_out: &Path) -> PathBuf {
    trace_out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join("online_var_audit.jsonl")
}

fn default_alarm_event_audit_path(trace_out: &Path) -> PathBuf {
    trace_out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join("alarm_events.ndjson")
}

fn capture_io_tick_snapshot(io: &sim::SimIo) -> IoTickSnapshot {
    let mut digital_inputs = Vec::with_capacity(io.num_digital_inputs());
    for idx in 0..io.num_digital_inputs() {
        let Ok(id) = u16::try_from(idx) else {
            break;
        };
        digital_inputs.push(io.read_digital_input(DigitalInputId(id)));
    }

    let mut analog_inputs = Vec::with_capacity(io.num_analog_inputs());
    for idx in 0..io.num_analog_inputs() {
        let Ok(id) = u16::try_from(idx) else {
            break;
        };
        analog_inputs.push(io.read_analog_input(AnalogInputId(id)));
    }

    let mut digital_outputs = Vec::with_capacity(io.num_digital_outputs());
    for idx in 0..io.num_digital_outputs() {
        let Ok(id) = u16::try_from(idx) else {
            break;
        };
        digital_outputs.push(io.read_digital_output_value(DigitalOutputId(id)));
    }

    let mut analog_outputs = Vec::with_capacity(io.num_analog_outputs());
    for idx in 0..io.num_analog_outputs() {
        let Ok(id) = u16::try_from(idx) else {
            break;
        };
        analog_outputs.push(io.read_analog_output_value(AnalogOutputId(id)));
    }

    IoTickSnapshot {
        tick: io.tick().0,
        digital_inputs,
        analog_inputs,
        digital_outputs,
        analog_outputs,
    }
}

fn write_io_snapshot_artifact(
    path: &Path,
    tick_ms: u64,
    ticks: Vec<IoTickSnapshot>,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "Failed to create io-snapshot artifact directory {}: {err}",
                    parent.display()
                )
            })?;
        }
    }

    let mut json = serde_json::to_string_pretty(&IoSnapshotArtifact {
        schema_version: 1,
        tick_ms,
        ticks,
    })
    .map_err(|err| format!("Failed to serialize io-snapshot artifact JSON: {err}"))?;
    json.push('\n');
    fs::write(path, json).map_err(|err| {
        format!(
            "Failed to write io-snapshot artifact {}: {err}",
            path.display()
        )
    })
}

fn default_alarm_scenario_or_recipe_id(scenario_path: &Path) -> String {
    scenario_path
        .file_stem()
        .and_then(OsStr::to_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("scenario")
        .to_string()
}

fn write_online_variable_audit(
    path: &Path,
    entries: &[OnlineVariableAuditEntry],
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "Failed to create online-variable audit directory {}: {err}",
                    parent.display()
                )
            })?;
        }
    }

    let file = fs::File::create(path).map_err(|err| {
        format!(
            "Failed to create online-variable audit {}: {err}",
            path.display()
        )
    })?;
    let mut writer = std::io::BufWriter::new(file);
    for entry in entries {
        let line = serde_json::to_string(entry)
            .map_err(|err| format!("Failed to serialize online-variable audit entry: {err}"))?;
        writer.write_all(line.as_bytes()).map_err(|err| {
            format!(
                "Failed to write online-variable audit {}: {err}",
                path.display()
            )
        })?;
        writer.write_all(b"\n").map_err(|err| {
            format!(
                "Failed to write online-variable audit {}: {err}",
                path.display()
            )
        })?;
    }
    writer.flush().map_err(|err| {
        format!(
            "Failed to flush online-variable audit {}: {err}",
            path.display()
        )
    })
}

#[derive(Debug, Clone)]
struct RetainConfig {
    digital_inputs: BTreeMap<u16, bool>,
    analog_inputs: BTreeMap<u16, f32>,
    digital_outputs: BTreeMap<u16, bool>,
    analog_outputs: BTreeMap<u16, f32>,
}

impl RetainConfig {
    fn is_empty(&self) -> bool {
        self.digital_inputs.is_empty()
            && self.analog_inputs.is_empty()
            && self.digital_outputs.is_empty()
            && self.analog_outputs.is_empty()
    }
}

#[derive(Debug, Deserialize)]
struct RetainConfigFileRaw {
    #[serde(default = "retain_schema_version")]
    schema_version: u32,
    #[serde(default)]
    digital_inputs: BTreeMap<String, bool>,
    #[serde(default)]
    analog_inputs: BTreeMap<String, f32>,
    #[serde(default)]
    digital_outputs: BTreeMap<String, bool>,
    #[serde(default)]
    analog_outputs: BTreeMap<String, f32>,
}

fn retain_schema_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RetainStatePayload {
    schema_version: u32,
    #[serde(default)]
    digital_inputs: BTreeMap<u16, bool>,
    #[serde(default)]
    analog_inputs: BTreeMap<u16, f32>,
    #[serde(default)]
    digital_outputs: BTreeMap<u16, bool>,
    #[serde(default)]
    analog_outputs: BTreeMap<u16, f32>,
}

impl RetainStatePayload {
    fn from_config_defaults(config: &RetainConfig) -> Self {
        Self {
            schema_version: retain_schema_version(),
            digital_inputs: config.digital_inputs.clone(),
            analog_inputs: config.analog_inputs.clone(),
            digital_outputs: config.digital_outputs.clone(),
            analog_outputs: config.analog_outputs.clone(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct RetainStateEnvelope {
    schema_version: u32,
    checksum_sha256: String,
    payload: RetainStatePayload,
}

fn parse_retain_channel_id(raw: &str, prefixes: &[&str]) -> Result<u16, String> {
    let token = raw.trim();
    if token.is_empty() {
        return Err("channel id key cannot be empty".to_string());
    }
    if let Ok(id) = token.parse::<u16>() {
        return Ok(id);
    }
    let lower = token.to_ascii_lowercase();
    for prefix in prefixes {
        if let Some(rest) = lower.strip_prefix(prefix) {
            return rest.parse::<u16>().map_err(|_| {
                format!(
                    "invalid retain channel key `{raw}` (expected <id> or {}<id>)",
                    prefix
                )
            });
        }
    }
    Err(format!(
        "invalid retain channel key `{raw}` (expected prefixes {:?} + integer id)",
        prefixes
    ))
}

fn normalize_retain_bool_map(
    raw: &BTreeMap<String, bool>,
    prefixes: &[&str],
    label: &str,
) -> Result<BTreeMap<u16, bool>, String> {
    let mut out = BTreeMap::<u16, bool>::new();
    for (k, v) in raw {
        let id = parse_retain_channel_id(k, prefixes)
            .map_err(|err| format!("invalid {label} key `{k}`: {err}"))?;
        if out.insert(id, *v).is_some() {
            return Err(format!(
                "duplicate retain {label} id {id} after key normalization"
            ));
        }
    }
    Ok(out)
}

fn normalize_retain_f32_map(
    raw: &BTreeMap<String, f32>,
    prefixes: &[&str],
    label: &str,
) -> Result<BTreeMap<u16, f32>, String> {
    let mut out = BTreeMap::<u16, f32>::new();
    for (k, v) in raw {
        if !v.is_finite() {
            return Err(format!("retain {label}.{k} must be finite"));
        }
        let id = parse_retain_channel_id(k, prefixes)
            .map_err(|err| format!("invalid {label} key `{k}`: {err}"))?;
        if out.insert(id, *v).is_some() {
            return Err(format!(
                "duplicate retain {label} id {id} after key normalization"
            ));
        }
    }
    Ok(out)
}

fn load_retain_config(path: &Path) -> Result<RetainConfig, String> {
    let body = fs::read_to_string(path)
        .map_err(|err| format!("Failed to read retain config {}: {err}", path.display()))?;
    let raw: RetainConfigFileRaw = toml::from_str(&body)
        .map_err(|err| format!("Failed to parse retain config {}: {err}", path.display()))?;
    if raw.schema_version != retain_schema_version() {
        return Err(format!(
            "retain config schema_version={} is unsupported (expected {})",
            raw.schema_version,
            retain_schema_version()
        ));
    }

    Ok(RetainConfig {
        digital_inputs: normalize_retain_bool_map(
            &raw.digital_inputs,
            &["di", "x"],
            "digital_inputs",
        )?,
        analog_inputs: normalize_retain_f32_map(&raw.analog_inputs, &["ai"], "analog_inputs")?,
        digital_outputs: normalize_retain_bool_map(
            &raw.digital_outputs,
            &["do", "y"],
            "digital_outputs",
        )?,
        analog_outputs: normalize_retain_f32_map(&raw.analog_outputs, &["ao"], "analog_outputs")?,
    })
}

fn default_retain_state_path(trace_out: &Path) -> PathBuf {
    trace_out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join("retain_state.json")
}

fn compute_retain_checksum(payload: &RetainStatePayload) -> Result<String, String> {
    let bytes = serde_json::to_vec(payload)
        .map_err(|err| format!("Failed to serialize retain payload for checksum: {err}"))?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

fn load_retain_state(path: &Path, config: &RetainConfig) -> (RetainStatePayload, Option<String>) {
    if !path.exists() {
        return (
            RetainStatePayload::from_config_defaults(config),
            Some(format!(
                "retain state {} does not exist; using config defaults",
                path.display()
            )),
        );
    }
    let body = match fs::read_to_string(path) {
        Ok(v) => v,
        Err(err) => {
            return (
                RetainStatePayload::from_config_defaults(config),
                Some(format!(
                    "failed to read retain state {} ({err}); using config defaults",
                    path.display()
                )),
            );
        }
    };

    let envelope: RetainStateEnvelope = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(err) => {
            return (
                RetainStatePayload::from_config_defaults(config),
                Some(format!(
                    "retain state {} is invalid JSON ({err}); using config defaults",
                    path.display()
                )),
            );
        }
    };
    if envelope.schema_version != retain_schema_version() {
        return (
            RetainStatePayload::from_config_defaults(config),
            Some(format!(
                "retain state {} schema_version={} is unsupported; using config defaults",
                path.display(),
                envelope.schema_version
            )),
        );
    }
    if envelope.payload.schema_version != retain_schema_version() {
        return (
            RetainStatePayload::from_config_defaults(config),
            Some(format!(
                "retain payload schema_version={} is unsupported in {}; using config defaults",
                envelope.payload.schema_version,
                path.display()
            )),
        );
    }
    let checksum = match compute_retain_checksum(&envelope.payload) {
        Ok(v) => v,
        Err(err) => {
            return (
                RetainStatePayload::from_config_defaults(config),
                Some(format!(
                    "failed to verify retain checksum for {} ({err}); using config defaults",
                    path.display()
                )),
            );
        }
    };
    if checksum != envelope.checksum_sha256 {
        return (
            RetainStatePayload::from_config_defaults(config),
            Some(format!(
                "retain checksum mismatch for {}; using config defaults",
                path.display()
            )),
        );
    }

    let mut payload = RetainStatePayload::from_config_defaults(config);
    for id in config.digital_inputs.keys() {
        if let Some(v) = envelope.payload.digital_inputs.get(id) {
            payload.digital_inputs.insert(*id, *v);
        }
    }
    for id in config.analog_inputs.keys() {
        if let Some(v) = envelope.payload.analog_inputs.get(id) {
            payload.analog_inputs.insert(*id, *v);
        }
    }
    for id in config.digital_outputs.keys() {
        if let Some(v) = envelope.payload.digital_outputs.get(id) {
            payload.digital_outputs.insert(*id, *v);
        }
    }
    for id in config.analog_outputs.keys() {
        if let Some(v) = envelope.payload.analog_outputs.get(id) {
            payload.analog_outputs.insert(*id, *v);
        }
    }
    (payload, None)
}

fn write_retain_state(path: &Path, payload: &RetainStatePayload) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "Failed to create retain state directory {}: {err}",
                    parent.display()
                )
            })?;
        }
    }
    let envelope = RetainStateEnvelope {
        schema_version: retain_schema_version(),
        checksum_sha256: compute_retain_checksum(payload)?,
        payload: payload.clone(),
    };
    let mut json = serde_json::to_string_pretty(&envelope)
        .map_err(|err| format!("Failed to serialize retain state JSON: {err}"))?;
    json.push('\n');
    fs::write(path, json)
        .map_err(|err| format!("Failed to write retain state {}: {err}", path.display()))
}

fn apply_retain_payload_to_scenario(scenario: &mut sim::Scenario, payload: &RetainStatePayload) {
    if !payload.digital_inputs.is_empty() || !payload.analog_inputs.is_empty() {
        let mut set = sim::InputSet::default();
        for (id, value) in &payload.digital_inputs {
            set.digital_inputs.insert(*id, *value);
        }
        for (id, value) in &payload.analog_inputs {
            set.analog_inputs.insert(*id, *value);
        }
        // Place retain bootstrap first so explicit scenario scripting at the same tick can override it.
        scenario.inputs.insert(0, sim::InputEvent { at_ms: 0, set });
        scenario.inputs.sort_by_key(|event| event.at_ms);
    }

    if !payload.digital_outputs.is_empty() || !payload.analog_outputs.is_empty() {
        let mut set = sim::ForceSet::default();
        for (id, value) in &payload.digital_outputs {
            set.digital_outputs.insert(*id, Some(*value));
        }
        for (id, value) in &payload.analog_outputs {
            set.analog_outputs.insert(*id, Some(*value));
        }
        scenario.forces.insert(0, sim::ForceEvent { at_ms: 0, set });

        // Outputs use a one-tick bootstrap force so runtime writes can take over afterwards.
        if scenario.tick_ms > 0
            && (scenario.duration_ms == 0 || scenario.tick_ms < scenario.duration_ms)
        {
            let mut clear = sim::ForceSet::default();
            for id in payload.digital_outputs.keys() {
                clear.digital_outputs.insert(*id, None);
            }
            for id in payload.analog_outputs.keys() {
                clear.analog_outputs.insert(*id, None);
            }
            scenario.forces.push(sim::ForceEvent {
                at_ms: scenario.tick_ms,
                set: clear,
            });
        }

        scenario.forces.sort_by_key(|event| event.at_ms);
    }
}

fn capture_retain_payload(config: &RetainConfig, io: &sim::SimIo) -> RetainStatePayload {
    let mut payload = RetainStatePayload::from_config_defaults(config);
    for id in config.digital_inputs.keys() {
        payload
            .digital_inputs
            .insert(*id, io.read_digital_input(io_traits::DigitalInputId(*id)));
    }
    for id in config.analog_inputs.keys() {
        payload
            .analog_inputs
            .insert(*id, io.read_analog_input(io_traits::AnalogInputId(*id)));
    }
    for id in config.digital_outputs.keys() {
        let value = io
            .digital_output_edges()
            .iter()
            .rev()
            .find(|edge| edge.id.0 == *id)
            .map(|edge| edge.value)
            .unwrap_or(false);
        payload.digital_outputs.insert(*id, value);
    }
    for id in config.analog_outputs.keys() {
        let value = io
            .analog_output_edges()
            .iter()
            .rev()
            .find(|edge| edge.id.0 == *id)
            .map(|edge| edge.value)
            .unwrap_or(0.0);
        payload.analog_outputs.insert(*id, value);
    }
    payload
}

fn run_sim_subcommand(program: &str, mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let usage = command_usage(program, "sim");
    let Some(scenario_path) = args.next() else {
        return Err(usage);
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
                return Err(usage.clone());
            }
            other => {
                return Err(format!("Unknown argument for sim: {other}"));
            }
        }
    }

    let scenario_path = PathBuf::from(&scenario_path);
    let scenario_yaml = read_scenario_yaml_file(&scenario_path)?;
    let scenario = parse_scenario_yaml(&scenario_yaml)?;

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
    let usage = command_usage(program, "sim-plc");
    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut scenario_path: Option<PathBuf> = None;
    let mut out_path: Option<PathBuf> = None;
    let mut retain_config_path: Option<PathBuf> = None;
    let mut retain_state_path: Option<PathBuf> = None;
    let mut enable_online_force_dev = false;
    let mut online_force_script: Option<PathBuf> = None;
    let mut online_force_audit_out: Option<PathBuf> = None;
    let mut online_var_script: Option<PathBuf> = None;
    let mut online_var_bindings_path: Option<PathBuf> = None;
    let mut online_var_audit_out: Option<PathBuf> = None;
    let mut alarm_options_seen = false;
    let mut alarm_audit_out: Option<PathBuf> = None;
    let mut alarm_hmi_ws: Option<String> = None;
    let mut alarm_scenario_id: Option<String> = None;
    let mut alarm_top_n: usize = 3;
    let mut alarm_dedup_window_ms: u64 = 1_000;
    let mut alarm_min_interval_ms: u64 = 200;
    let mut io_snapshot_out: Option<PathBuf> = None;
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
            "--retain-config" => {
                retain_config_path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --retain-config <retain.toml>".to_string()
                })?));
            }
            "--retain-state" => {
                retain_state_path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --retain-state <retain_state.json>".to_string()
                })?));
            }
            "--enable-online-force-dev" => {
                enable_online_force_dev = true;
            }
            "--online-force-script" => {
                online_force_script = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --online-force-script <script.jsonl>".to_string()
                })?));
            }
            "--online-force-audit-out" => {
                online_force_audit_out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --online-force-audit-out <audit.jsonl>".to_string()
                })?));
            }
            "--online-var-script" => {
                online_var_script = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --online-var-script <script.jsonl>".to_string()
                })?));
            }
            "--online-var-bindings" => {
                online_var_bindings_path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --online-var-bindings <bindings.toml>".to_string()
                })?));
            }
            "--online-var-audit-out" => {
                online_var_audit_out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --online-var-audit-out <audit.jsonl>".to_string()
                })?));
            }
            "--alarm-audit-out" => {
                alarm_options_seen = true;
                alarm_audit_out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --alarm-audit-out <alarm_events.ndjson>".to_string()
                })?));
            }
            "--alarm-hmi-ws" => {
                alarm_options_seen = true;
                alarm_hmi_ws = Some(args.next().ok_or_else(|| {
                    "Missing value for --alarm-hmi-ws <ws://host:port/path>".to_string()
                })?);
            }
            "--alarm-scenario-id" => {
                alarm_options_seen = true;
                let value = args
                    .next()
                    .ok_or_else(|| "Missing value for --alarm-scenario-id <id>".to_string())?;
                if value.trim().is_empty() {
                    return Err("--alarm-scenario-id cannot be empty".to_string());
                }
                alarm_scenario_id = Some(value);
            }
            "--alarm-top" => {
                alarm_options_seen = true;
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --alarm-top <n>".to_string())?;
                alarm_top_n = raw
                    .parse::<usize>()
                    .map_err(|_| format!("Invalid --alarm-top value (expected usize): {raw}"))?;
                if alarm_top_n == 0 {
                    return Err("Invalid --alarm-top value (expected >= 1)".to_string());
                }
            }
            "--alarm-dedup-window-ms" => {
                alarm_options_seen = true;
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --alarm-dedup-window-ms <ms>".to_string())?;
                alarm_dedup_window_ms = raw.parse::<u64>().map_err(|_| {
                    format!("Invalid --alarm-dedup-window-ms value (expected u64): {raw}")
                })?;
            }
            "--alarm-min-interval-ms" => {
                alarm_options_seen = true;
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --alarm-min-interval-ms <ms>".to_string())?;
                alarm_min_interval_ms = raw.parse::<u64>().map_err(|_| {
                    format!("Invalid --alarm-min-interval-ms value (expected u64): {raw}")
                })?;
            }
            "--io-snapshot-out" => {
                io_snapshot_out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --io-snapshot-out <io_snapshot.json>".to_string()
                })?));
            }
            "-h" | "--help" => {
                return Err(usage.clone());
            }
            other => return Err(format!("Unknown argument for sim-plc: {other}")),
        }
    }

    let scenario_path = scenario_path.ok_or_else(|| usage.clone())?;
    let out_path = out_path.ok_or_else(|| usage.clone())?;

    if retain_state_path.is_some() && retain_config_path.is_none() {
        return Err("--retain-state requires --retain-config".to_string());
    }
    if (online_force_script.is_some()
        || online_force_audit_out.is_some()
        || online_var_script.is_some()
        || online_var_bindings_path.is_some()
        || online_var_audit_out.is_some())
        && !enable_online_force_dev
    {
        return Err(
            "online-force/variable dev control plane is disabled by default; add --enable-online-force-dev to use --online-force-script/--online-force-audit-out/--online-var-script/--online-var-bindings/--online-var-audit-out"
                .to_string(),
        );
    }

    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to create output directory {parent:?}: {err}"))?;
        }
    }
    if let Some(path) = &io_snapshot_out {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|err| {
                    format!("Failed to create io-snapshot output directory {parent:?}: {err}")
                })?;
            }
        }
    }

    let plc_source =
        fs::read_to_string(&plc_path).map_err(|err| format!("Failed to read {plc_path}: {err}"))?;
    let scenario_yaml = read_scenario_yaml_file(&scenario_path)?;
    let scenario_yaml =
        resolve_scenario_yaml_for_plc(&plc_source, &scenario_yaml).map_err(|e| {
            format_resolve_scenario_yaml_error(&plc_path, &scenario_path, "sim-plc", &e)
        })?;
    let mut scenario = parse_scenario_yaml(&scenario_yaml)?;

    let mut retain_session: Option<(RetainConfig, PathBuf)> = None;
    if let Some(config_path) = retain_config_path {
        let config = load_retain_config(&config_path)?;
        if config.is_empty() {
            return Err(format!(
                "retain config {} has no retained channels configured",
                config_path.display()
            ));
        }
        let state_path = retain_state_path
            .clone()
            .unwrap_or_else(|| default_retain_state_path(&out_path));
        let (payload, warning) = load_retain_state(&state_path, &config);
        if let Some(msg) = warning {
            eprintln!("[RET-201] {msg}");
        }
        apply_retain_payload_to_scenario(&mut scenario, &payload);
        retain_session = Some((config, state_path));
    }

    let audit_path = if enable_online_force_dev {
        Some(
            online_force_audit_out
                .clone()
                .unwrap_or_else(|| default_online_force_audit_path(&out_path)),
        )
    } else {
        None
    };
    let variable_audit_path = if enable_online_force_dev
        && (online_var_script.is_some() || online_var_audit_out.is_some())
    {
        Some(
            online_var_audit_out
                .clone()
                .unwrap_or_else(|| default_online_variable_audit_path(&out_path)),
        )
    } else {
        None
    };
    let alarm_audit_path = if alarm_options_seen {
        Some(
            alarm_audit_out
                .clone()
                .unwrap_or_else(|| default_alarm_event_audit_path(&out_path)),
        )
    } else {
        None
    };
    let alarm_scenario_or_recipe_id = if alarm_options_seen {
        alarm_scenario_id
            .clone()
            .unwrap_or_else(|| default_alarm_scenario_or_recipe_id(&scenario_path))
    } else {
        String::new()
    };
    let alarm_hmi_ws_display = alarm_hmi_ws.clone();
    let alarm_dispatcher = if let Some(path) = &alarm_audit_path {
        Some(
            AlarmDispatcher::new(AlarmDispatchConfig {
                audit_path: path.clone(),
                websocket_url: alarm_hmi_ws.clone(),
                dedup_window_ms: alarm_dedup_window_ms,
                min_emit_interval_ms: alarm_min_interval_ms,
                queue_capacity: 64,
            })
            .map_err(|err| format!("Failed to initialize alarm dispatcher: {err}"))?,
        )
    } else {
        None
    };

    let mut online_commands = Vec::new();
    if let Some(script_path) = &online_force_script {
        online_commands = load_online_force_script(script_path, scenario.tick_ms)?;
        inject_online_force_commands(&mut scenario, &online_commands)?;
    }

    if let Some(path) = &audit_path {
        let audit_entries = build_online_force_audit(&online_commands, scenario.tick_ms);
        write_online_force_audit(path, &audit_entries)?;
    }
    let mut online_variable_commands = Vec::new();
    let online_variable_bindings = if let Some(path) = &online_var_bindings_path {
        Some(load_online_variable_bindings(path)?)
    } else {
        None
    };
    if let Some(script_path) = &online_var_script {
        online_variable_commands = load_online_variable_script(script_path, scenario.tick_ms)?;
        inject_online_variable_commands(
            &mut scenario,
            &online_variable_commands,
            online_variable_bindings.as_ref(),
        )?;
    }
    if let Some(path) = &variable_audit_path {
        let variable_audit = build_online_variable_audit(
            &online_variable_commands,
            scenario.tick_ms,
            online_variable_bindings.as_ref(),
        )?;
        write_online_variable_audit(path, &variable_audit)?;
    }

    let program = compile_plc_to_runtime_program(&plc_source, scenario.tick_ms)?;

    let (num_di, num_do, num_ai, num_ao) = io_sizes_for_program_and_scenario(&program, &scenario);
    let mut io = sim::SimIo::new(num_di, num_do, num_ai, num_ao);
    let mut io_snapshots = Vec::new();
    let run = if io_snapshot_out.is_some() {
        sim::run_program_for_scenario_with_tick_observer(&program, &scenario, &mut io, |io| {
            io_snapshots.push(capture_io_tick_snapshot(io));
        })
    } else {
        sim::run_program_for_scenario(&program, &scenario, &mut io)
    }
    .map_err(|e| {
        let mut msg = format!("{e}");
        if let Some(hint) =
            scenario_mismatch_hint_for_example(&plc_path, &scenario_path, &e, "sim-plc")
        {
            msg.push_str("\n\n");
            msg.push_str(&hint);
        }
        msg
    })?;
    if let Some(path) = &io_snapshot_out {
        write_io_snapshot_artifact(path, scenario.tick_ms, io_snapshots)?;
    }
    let trace_text = run.trace.into_string();
    fs::write(&out_path, &trace_text)
        .map_err(|err| format!("Failed to write trace file {out_path:?}: {err}"))?;
    if let Some(dispatcher) = alarm_dispatcher {
        let trace_events = rust_plc::trace_diff::parse_trace_jsonl(&trace_text)
            .map_err(|err| format!("Failed to parse generated trace for alarm events: {err}"))?;
        let timeout_events = trace_events
            .iter()
            .filter(|event| event.reason == "timeout")
            .collect::<Vec<_>>();
        if !timeout_events.is_empty() {
            let diagnosis = diagnose(DiagnosisInput {
                plc_source: &plc_source,
                scenario: &scenario,
                trace_events: Some(trace_events.as_slice()),
                diff_report: None,
                timing_report: None,
                evidence_source: EvidenceSource::RuntimeLive,
                io_snapshot: None,
            })
            .map_err(|err| format!("Failed to build runtime alarm diagnosis: {err}"))?;
            let evidence_ref = display_path_relative_to_cwd(&out_path);
            for timeout in timeout_events {
                let alarm_event = build_alarm_event(AlarmBuildInput {
                    diagnosis: &diagnosis,
                    severity: AlarmSeverity::Critical,
                    first_seen_ms: timeout.tick.saturating_mul(scenario.tick_ms),
                    top_n: alarm_top_n,
                    evidence_ref: &evidence_ref,
                    evidence_source: EvidenceSource::RuntimeLive,
                    scenario_or_recipe_id: &alarm_scenario_or_recipe_id,
                });
                let _ = dispatcher.publish(alarm_event).map_err(|err| {
                    format!("Failed to enqueue runtime alarm event for publishing: {err}")
                })?;
            }
        }
        dispatcher
            .close()
            .map_err(|err| format!("Failed to finalize runtime alarm dispatcher: {err}"))?;
    }
    if let Some((config, state_path)) = retain_session {
        let payload = capture_retain_payload(&config, &io);
        write_retain_state(&state_path, &payload)?;
        eprintln!("sim-plc: retain state {}", state_path.display());
    }
    if let Some(path) = audit_path {
        eprintln!("sim-plc: online-force audit {}", path.display());
    }
    if let Some(path) = variable_audit_path {
        eprintln!("sim-plc: online-variable audit {}", path.display());
    }
    if let Some(path) = alarm_audit_path {
        eprintln!("sim-plc: alarm-event audit {}", path.display());
    }
    if let Some(ws_url) = alarm_hmi_ws_display {
        eprintln!("sim-plc: alarm-event realtime {}", ws_url);
    }
    if let Some(path) = io_snapshot_out {
        eprintln!("sim-plc: io-snapshot {}", path.display());
    }
    Ok(())
}

fn run_sim_regress_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "sim-regress");
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
                return Err(usage.clone());
            }
            other => {
                return Err(format!("Unknown argument for sim-regress: {other}"));
            }
        }
    }

    let plc_dir = plc_dir.ok_or_else(|| usage.clone())?;
    let scenario_dir = scenario_dir.ok_or_else(|| usage.clone())?;

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
    if minimize_failure {
        let feedback_path = artifacts_dir.join("feedback.json");
        write_sim_regress_feedback(&feedback_path, &summary)?;
    }
    Ok(())
}

fn run_sim_pid_kpi_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "sim-pid-kpi");
    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut scenario_path: Option<PathBuf> = None;
    let mut out_path: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenario" => {
                scenario_path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --scenario <pid_scenario.yaml>".to_string()
                })?));
            }
            "--out" => {
                out_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --out <kpi.json>".to_string()
                    })?));
            }
            "-h" | "--help" => {
                return Err(usage.clone());
            }
            other => return Err(format!("Unknown argument for sim-pid-kpi: {other}")),
        }
    }

    let scenario_path = scenario_path.ok_or_else(|| usage.clone())?;
    let out_path = out_path.unwrap_or_else(|| PathBuf::from("out/pid_kpi.json"));

    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to create output directory {parent:?}: {err}"))?;
        }
    }

    let plc_source =
        fs::read_to_string(&plc_path).map_err(|err| format!("Failed to read {plc_path}: {err}"))?;
    let pid_example = "Example:\n\
tick_ms: 100\n\
duration_ms: 10000\n\
loop_index: 0\n\
initial_pv: 0.0\n\
model:\n\
  kind: first_order\n\
  gain: 1.0\n\
  tau_ms: 500\n";
    let scenario_yaml = fs::read_to_string(&scenario_path).map_err(|err| {
        format!(
            "Failed to read PID scenario YAML {}: {err}\n\n{pid_example}",
            scenario_path.display()
        )
    })?;
    let scenario = sim::PidControlScenario::from_yaml_str(&scenario_yaml)
        .map_err(|err| format!("Failed to parse PID scenario YAML: {err}\n\n{pid_example}"))?;
    let runtime_program = compile_plc_to_runtime_program(&plc_source, scenario.tick_ms)?;
    let report = sim::run_pid_kpi(&runtime_program, &scenario)
        .map_err(|err| format!("Failed to run PID KPI simulation: {err}"))?;

    let mut json = serde_json::to_string_pretty(&report)
        .map_err(|err| format!("Failed to serialize KPI JSON: {err}"))?;
    json.push('\n');
    fs::write(&out_path, json)
        .map_err(|err| format!("Failed to write KPI file {out_path:?}: {err}"))?;

    Ok(())
}

#[derive(Debug, Serialize)]
struct BuildMeta<'a> {
    plc_sha256: &'a str,
    generated_at: &'a str,
    tool_version: &'a str,
    runtime_semver: &'a str,
    git_commit: &'a str,
    git_dirty: bool,
    runtime_budget: RuntimeBudget,
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
    let git_metadata = detect_git_metadata();

    let meta = BuildMeta {
        plc_sha256: &sha256,
        generated_at: &generated_at,
        tool_version: env!("CARGO_PKG_VERSION"),
        runtime_semver: runtime_core::VERSION,
        git_commit: &git_metadata.commit,
        git_dirty: git_metadata.dirty,
        runtime_budget: ir_bundle.runtime_budget.clone(),
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

    let plc_sha256 = sha256_hex(&plc_bytes);

    let scenario_yaml = read_scenario_yaml_file(&scenario_path)?;
    let scenario_yaml =
        resolve_scenario_yaml_for_plc(&plc_source, &scenario_yaml).map_err(|e| {
            format_resolve_scenario_yaml_error(&plc_path, &scenario_path, "release-bundle", &e)
        })?;
    let scenario = parse_scenario_yaml(&scenario_yaml)?;

    let ir_bundle = compile_pipeline(&plc_source).map_err(|errors| errors.join("\n\n"))?;

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
        runtime_budget: ir_bundle.runtime_budget.clone(),
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
///   - `.bat` → `cmd /C <path>`
///   - `.ps1` → `powershell -NonInteractive -File <path>`
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

fn build_analog_contract(plc_source: &str) -> Result<AnalogContract, String> {
    let expanded = preprocess_plc_source(plc_source)
        .map_err(|err| format!("Failed to prepare PLC source: {err}"))?;

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
                Instr::WaitDigital { id, .. } => {
                    dis.insert(id.0);
                }
                Instr::WaitAnalog { id, .. } => {
                    ais.insert(id.0);
                }
                Instr::WaitAllDigital { conditions, .. } => {
                    for condition in conditions {
                        dis.insert(condition.id.0);
                    }
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
                            Action::CylinderMotion {
                                output,
                                confirm_inputs,
                                opposing_inputs,
                                ..
                            } => {
                                dos.insert(output.0);
                                for id in confirm_inputs {
                                    dis.insert(id.0);
                                }
                                for id in opposing_inputs {
                                    dis.insert(id.0);
                                }
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
                Instr::WaitDigital { id, .. } => {
                    dis.insert(id.0);
                }
                Instr::WaitAnalog { id, .. } => {
                    ais.insert(id.0);
                }
                Instr::WaitAllDigital { conditions, .. } => {
                    for condition in conditions {
                        dis.insert(condition.id.0);
                    }
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
                            Action::CylinderMotion {
                                output,
                                confirm_inputs,
                                opposing_inputs,
                                ..
                            } => {
                                dos.insert(output.0);
                                for id in confirm_inputs {
                                    dis.insert(id.0);
                                }
                                for id in opposing_inputs {
                                    dis.insert(id.0);
                                }
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

fn write_sim_regress_summary(path: &Path, summary: &SimRegressSummary) -> Result<(), String> {
    let mut json = serde_json::to_string_pretty(summary)
        .map_err(|err| format!("Failed to serialize summary JSON: {err}"))?;
    json.push('\n');
    fs::write(path, json).map_err(|err| format!("Failed to write summary file {path:?}: {err}"))?;
    Ok(())
}

#[derive(Debug, Serialize)]
struct SimRegressFeedbackFile {
    schema_version: u32,
    total_failures: usize,
    feedback: Vec<SimRegressFeedbackEntry>,
}

#[derive(Debug, Serialize)]
struct SimRegressFeedbackEntry {
    plc: String,
    scenario: String,
    failure_kind: String,
    template_hint: String,
    parameter_hints: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    minimized_scenario_path: Option<String>,
}

fn feedback_template_hint_for_failure_kind(kind: &str) -> &'static str {
    match kind {
        "timeout" => "fault_sensor_stuck",
        "compile_error" | "scenario_error" => "nominal_cycle",
        _ => "risk_gate_probe",
    }
}

fn feedback_parameter_hints_for_failure(
    failure: &rust_plc::sim_regress::SimRegressFailure,
) -> Vec<String> {
    let mut hints = Vec::<String>::new();
    match failure.failure.kind.as_str() {
        "timeout" => {
            hints.push("increase duration_ms to keep timeout windows observable".to_string());
            hints.push(
                "tune start_pulse_ms to align start signal release with task waits".to_string(),
            );
            hints.push("adjust sensor_window_ms to control sensor-edge spacing".to_string());
        }
        "scenario_error" => {
            hints.push(
                "run scenario-validate and fix mapping/tick alignment issues first".to_string(),
            );
        }
        "compile_error" => {
            hints
                .push("fix PLC semantic/verification errors before scenario expansion".to_string());
        }
        _ => {
            hints.push(
                "re-run with --minimize-failure and inspect minimized_scenario.yaml".to_string(),
            );
        }
    }
    if let Some(mini) = &failure.minimization {
        hints.push(format!(
            "duration_ms near {} reproduces this failure signature with lower noise",
            mini.minimized_duration_ms
        ));
    }
    hints
}

fn write_sim_regress_feedback(path: &Path, summary: &SimRegressSummary) -> Result<(), String> {
    let feedback = summary
        .failures
        .iter()
        .map(|failure| SimRegressFeedbackEntry {
            plc: failure.plc.clone(),
            scenario: failure.scenario.clone(),
            failure_kind: failure.failure.kind.clone(),
            template_hint: feedback_template_hint_for_failure_kind(&failure.failure.kind)
                .to_string(),
            parameter_hints: feedback_parameter_hints_for_failure(failure),
            minimized_scenario_path: failure.minimized_scenario_path.clone(),
        })
        .collect::<Vec<_>>();
    let file = SimRegressFeedbackFile {
        schema_version: 1,
        total_failures: summary.failures.len(),
        feedback,
    };
    let mut json = serde_json::to_string_pretty(&file)
        .map_err(|err| format!("Failed to serialize feedback JSON: {err}"))?;
    json.push('\n');
    fs::write(path, json).map_err(|err| format!("Failed to write feedback file {path:?}: {err}"))
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

    let plc_source =
        fs::read_to_string(&plc_path).map_err(|err| format!("Failed to read {plc_path}: {err}"))?;

    let sil_yaml = read_scenario_yaml_file(&sil_scenario_path)?;
    let board_yaml = read_scenario_yaml_file(&board_scenario_path)?;
    let sil_yaml = resolve_scenario_yaml_for_plc(&plc_source, &sil_yaml).map_err(|e| {
        format_resolve_scenario_yaml_error(&plc_path, &sil_scenario_path, "no-board-gate", &e)
    })?;
    let board_yaml = resolve_scenario_yaml_for_plc(&plc_source, &board_yaml).map_err(|e| {
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

    let program = compile_plc_to_runtime_program(&plc_source, sil_scenario.tick_ms)?;

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
            plc_source: &plc_source,
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

    let plc_source =
        fs::read_to_string(&plc_path).map_err(|err| format!("Failed to read {plc_path}: {err}"))?;
    let scenario_yaml = read_scenario_yaml_file(&scenario_path)?;
    let scenario_yaml =
        resolve_scenario_yaml_for_plc(&plc_source, &scenario_yaml).map_err(|e| {
            format_resolve_scenario_yaml_error(&plc_path, &scenario_path, "pil-run", &e)
        })?;
    let scenario = parse_scenario_yaml(&scenario_yaml)?;

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

    let plc_source =
        fs::read_to_string(&plc_path).map_err(|err| format!("Failed to read {plc_path}: {err}"))?;
    let scenario_yaml = read_scenario_yaml_file(&scenario_path)?;
    let scenario_yaml =
        resolve_scenario_yaml_for_plc(&plc_source, &scenario_yaml).map_err(|e| {
            format_resolve_scenario_yaml_error(&plc_path, &scenario_path, "virtual-board", &e)
        })?;
    let scenario = parse_scenario_yaml(&scenario_yaml)?;

    let program = compile_plc_to_runtime_program(&plc_source, scenario.tick_ms)?;
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

fn compile_pipeline(source: &str) -> Result<IrBundle, Vec<String>> {
    let compiled = compile_plc_semantics(source)?;
    for warning in collect_compiled_plc_warnings(&compiled) {
        eprintln!("WARNING [deprecation] {warning}");
    }
    let timing_model = build_timing_model(&compiled.expanded_program)
        .map_err(|errors| errors.into_iter().map(|error| error.to_string()).collect::<Vec<_>>())?;
    let mut verification = verify_compiled_plc_semantics(&compiled)?;
    apply_axis_move_blocking_migration_warning(&compiled.source_program, &mut verification);

    let runtime_budget = analyze_runtime_budget(&compiled.expanded_program, &compiled.state_machine);

    Ok(IrBundle {
        topology: compiled.topology,
        state_machine: compiled.state_machine,
        constraints: compiled.constraints,
        timing_model,
        runtime_budget,
        verification,
    })
}

fn print_success_summary(summary: &VerificationSummary) {
    eprintln!("验证通过：");
    eprintln!(
        "  - Safety: {}（深度 {}）",
        summary.safety.level, summary.safety.explored_depth
    );
    eprintln!(
        "    覆盖: bound {}/{}，degraded {}，skipped {}",
        summary.safety.coverage.bound_rules,
        summary.safety.coverage.total_rules,
        summary.safety.coverage.degraded_rules,
        summary.safety.coverage.skipped_rules
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
    entries: &[WarningEntry],
    output: &mut Vec<String>,
) {
    for entry in entries {
        if matches!(entry.level, WarningLevel::Warn | WarningLevel::Error) {
            let code_suffix = entry
                .code
                .as_ref()
                .map(|code| format!(" ({code})"))
                .unwrap_or_default();
            output.push(format!(
                "[{checker}] {}{}: {}",
                warning_level_label(&entry.level),
                code_suffix,
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
    runtime_budget: &RuntimeBudget,
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
        runtime_budget,
        verification,
    };

    let mut report_json = serde_json::to_string_pretty(&report)
        .map_err(|err| format!("Failed to serialize verification report JSON: {err}"))?;
    report_json.push('\n');
    fs::write(report_path, report_json)
        .map_err(|err| format!("Failed to write verification report {report_path:?}: {err}"))?;

    Ok(())
}

fn analyze_runtime_budget(
    program: &rust_plc::ast::PlcProgram,
    state_machine: &StateMachine,
) -> RuntimeBudget {
    let (max_actions_per_transition, max_parallel_branches, max_race_branches) =
        analyze_program_budget_facts(program);

    let mut task_names = state_machine
        .task_contexts
        .iter()
        .map(|ctx| ctx.task_name.clone())
        .collect::<BTreeSet<_>>();
    if task_names.is_empty() {
        for state in &state_machine.states {
            task_names.insert(state.task_name.clone());
        }
    }
    let active_task_count = task_names.len().max(1);

    // Edges that may fire within the same tick if inputs match.
    let mut state_index: HashMap<(String, String), usize> = HashMap::new();
    for (idx, state) in state_machine.states.iter().enumerate() {
        state_index.insert((state.task_name.clone(), state.step_name.clone()), idx);
    }

    let mut has_cycle = false;
    let mut longest_chain = 0usize;
    for task_name in task_names {
        let mut outgoing: Vec<Vec<usize>> = vec![Vec::new(); state_machine.states.len()];
        let mut edges: Vec<(usize, usize)> = Vec::new();
        for tr in &state_machine.transitions {
            if tr.from.task_name != task_name || tr.to.task_name != task_name {
                continue;
            }
            if !guard_can_fire_same_tick(&tr.guard) {
                continue;
            }

            let from = state_index
                .get(&(tr.from.task_name.clone(), tr.from.step_name.clone()))
                .copied();
            let to = state_index
                .get(&(tr.to.task_name.clone(), tr.to.step_name.clone()))
                .copied();
            let (Some(from), Some(to)) = (from, to) else {
                continue;
            };

            let eid = edges.len();
            edges.push((from, to));
            outgoing[from].push(eid);
        }

        let (task_has_cycle, task_longest_chain) = analyze_longest_chain(&outgoing, &edges);
        has_cycle |= task_has_cycle;
        longest_chain = longest_chain.max(task_longest_chain);
    }

    let max_transitions_per_tick_cap = MAX_TRANSITIONS_PER_TASK_PER_TICK;
    let max_transitions_all_tasks_per_tick_upper_bound =
        max_transitions_per_tick_cap.saturating_mul(active_task_count);
    let max_transitions_same_tick_upper_bound = if has_cycle {
        max_transitions_per_tick_cap
    } else {
        longest_chain.min(max_transitions_per_tick_cap)
    };

    let max_actions_per_tick_upper_bound = max_actions_per_transition
        .saturating_mul(max_transitions_all_tasks_per_tick_upper_bound)
        .max(max_actions_per_transition);

    let mut budget = RuntimeBudget {
        transition_budget_scope: TransitionBudgetScope::PerTaskPerTick,
        max_transitions_per_tick_cap,
        active_task_count,
        max_transitions_all_tasks_per_tick_upper_bound,
        max_transitions_same_tick_upper_bound,
        max_actions_per_transition,
        max_actions_per_tick_upper_bound,
        max_parallel_branches,
        max_race_branches,
        has_same_tick_cycle: has_cycle,
        budget_time_estimate: BudgetTimeEstimate {
            action_cost_us: 0,
            transition_cost_us: 0,
            parallel_expand_cost_us: 0,
            action_component_us: 0,
            transition_component_us: 0,
            parallel_component_us: 0,
            total_estimate_us: 0,
            max_allowed_us: 0,
            exceeds_budget: false,
        },
    };
    budget.budget_time_estimate =
        estimate_budget_time(&budget, &RuntimeBudgetThresholds::default());
    budget
}

fn analyze_program_budget_facts(program: &rust_plc::ast::PlcProgram) -> (usize, usize, usize) {
    let mut max_actions_in_step = 0usize;
    let mut max_parallel = 0usize;
    let mut max_race = 0usize;

    for task in &program.tasks.tasks {
        for step in &task.steps {
            let mut action_count = 0usize;
            analyze_statements_budget_facts(
                &step.statements,
                &mut action_count,
                &mut max_parallel,
                &mut max_race,
            );
            max_actions_in_step = max_actions_in_step.max(action_count);
        }
    }

    (max_actions_in_step, max_parallel, max_race)
}

fn analyze_statements_budget_facts(
    statements: &[rust_plc::ast::StepStatement],
    actions_in_step: &mut usize,
    max_parallel: &mut usize,
    max_race: &mut usize,
) {
    for stmt in statements {
        match stmt {
            rust_plc::ast::StepStatement::Action(_) => {
                *actions_in_step = actions_in_step.saturating_add(1);
            }
            rust_plc::ast::StepStatement::Repeat { body, .. } => {
                analyze_statements_budget_facts(body, actions_in_step, max_parallel, max_race);
            }
            rust_plc::ast::StepStatement::Parallel(block) => {
                *max_parallel = (*max_parallel).max(block.branches.len());
                for b in &block.branches {
                    analyze_statements_budget_facts(
                        &b.statements,
                        actions_in_step,
                        max_parallel,
                        max_race,
                    );
                }
            }
            rust_plc::ast::StepStatement::Race(block) => {
                *max_race = (*max_race).max(block.branches.len());
                for b in &block.branches {
                    analyze_statements_budget_facts(
                        &b.statements,
                        actions_in_step,
                        max_parallel,
                        max_race,
                    );
                }
            }
            _ => {}
        }
    }
}

fn guard_can_fire_same_tick(guard: &rust_plc::ir::TransitionGuard) -> bool {
    match guard {
        rust_plc::ir::TransitionGuard::Always => true,
        rust_plc::ir::TransitionGuard::Condition { .. } => true,
        rust_plc::ir::TransitionGuard::Timeout { duration_ms } => *duration_ms == 0,
        rust_plc::ir::TransitionGuard::Delay { duration_ms } => *duration_ms == 0,
    }
}

fn analyze_longest_chain(outgoing: &[Vec<usize>], edges: &[(usize, usize)]) -> (bool, usize) {
    let n = outgoing.len();
    let mut visiting = vec![false; n];
    let mut visited = vec![false; n];
    let mut memo = vec![0usize; n];
    let mut has_cycle = false;

    fn dfs(
        u: usize,
        outgoing: &[Vec<usize>],
        edges: &[(usize, usize)],
        visiting: &mut [bool],
        visited: &mut [bool],
        memo: &mut [usize],
        has_cycle: &mut bool,
    ) -> usize {
        if visiting[u] {
            *has_cycle = true;
            return 0;
        }
        if visited[u] {
            return memo[u];
        }
        visiting[u] = true;
        let mut best = 0usize;
        for &eid in &outgoing[u] {
            let (_from, to) = edges[eid];
            let candidate =
                1usize.saturating_add(dfs(to, outgoing, edges, visiting, visited, memo, has_cycle));
            best = best.max(candidate);
        }
        visiting[u] = false;
        visited[u] = true;
        memo[u] = best;
        best
    }

    let mut longest = 0usize;
    for u in 0..n {
        longest = longest.max(dfs(
            u,
            outgoing,
            edges,
            &mut visiting,
            &mut visited,
            &mut memo,
            &mut has_cycle,
        ));
    }

    (has_cycle, longest)
}

fn apply_runtime_budget_warnings(
    verification: &mut VerificationSummary,
    budget: &mut RuntimeBudget,
    thresholds: RuntimeBudgetThresholds,
) {
    let mut warnings: Vec<WarningEntry> = Vec::new();

    budget.budget_time_estimate = estimate_budget_time(budget, &thresholds);

    if budget.max_actions_per_transition > thresholds.max_actions_per_transition {
        warnings.push(WarningEntry {
            code: None,
            level: WarningLevel::Warn,
            message: format!(
                "runtime budget: max_actions_per_transition={} exceeds threshold {}",
                budget.max_actions_per_transition, thresholds.max_actions_per_transition
            ),
        });
    }
    if budget.max_actions_per_tick_upper_bound > thresholds.max_actions_per_tick_upper_bound {
        warnings.push(WarningEntry {
            code: None,
            level: WarningLevel::Warn,
            message: format!(
                "runtime budget: max_actions_per_tick_upper_bound={} exceeds threshold {}",
                budget.max_actions_per_tick_upper_bound,
                thresholds.max_actions_per_tick_upper_bound
            ),
        });
    }
    if budget.max_parallel_branches > thresholds.max_parallel_branches {
        warnings.push(WarningEntry {
            code: None,
            level: WarningLevel::Warn,
            message: format!(
                "runtime budget: max_parallel_branches={} exceeds threshold {}",
                budget.max_parallel_branches, thresholds.max_parallel_branches
            ),
        });
    }
    if budget.max_race_branches > thresholds.max_race_branches {
        warnings.push(WarningEntry {
            code: None,
            level: WarningLevel::Warn,
            message: format!(
                "runtime budget: max_race_branches={} exceeds threshold {}",
                budget.max_race_branches, thresholds.max_race_branches
            ),
        });
    }
    if thresholds.warn_on_same_tick_cycle && budget.has_same_tick_cycle {
        warnings.push(WarningEntry {
            code: None,
            level: WarningLevel::Warn,
            message: format!(
                "runtime budget: same-tick transition subgraph contains a cycle; runtime-core caps chaining to {} transitions per task per tick (active_tasks={})",
                budget.max_transitions_per_tick_cap, budget.active_task_count
            ),
        });
    }
    if budget.budget_time_estimate.exceeds_budget {
        warnings.push(WarningEntry {
            code: None,
            level: WarningLevel::Warn,
            message: format!(
                "runtime budget time estimate: total_estimate_us={} exceeds threshold {}",
                budget.budget_time_estimate.total_estimate_us,
                budget.budget_time_estimate.max_allowed_us
            ),
        });
    }

    verification.timing.warnings.extend(warnings);
}

fn apply_axis_move_blocking_migration_warning(
    program: &rust_plc::ast::PlcProgram,
    verification: &mut VerificationSummary,
) {
    let impacted_steps: Vec<String> = program
        .tasks
        .tasks
        .iter()
        .flat_map(|task| {
            task.steps.iter().filter_map(move |step| {
                let statement_count = step
                    .statements
                    .iter()
                    .filter(|stmt| {
                        !matches!(stmt, rust_plc::ast::StepStatement::AllowIndefiniteWait(_))
                    })
                    .count();
                if statement_count <= 1 || !statements_include_axis_move(&step.statements) {
                    return None;
                }
                Some(format!("{}.{}", task.name, step.name))
            })
        })
        .collect();

    if impacted_steps.is_empty() {
        return;
    }

    let preview = impacted_steps
        .iter()
        .take(3)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let overflow = impacted_steps.len().saturating_sub(3);
    let scope = if overflow > 0 {
        format!("{preview} 等 {} 个 step", impacted_steps.len())
    } else {
        format!("{preview}（共 {} 个 step）", impacted_steps.len())
    };

    verification.liveness.warnings.push(WarningEntry {
        code: Some(AXIS_BLOCKING_MIGRATION_WARNING_CODE.to_string()),
        level: WarningLevel::Warn,
        message: format!(
            "迁移提示：axis.move_* 现按默认阻塞语义执行。检测到 {scope} 在同一 step 内混合了 axis.move_* 与其它语句，执行时序会与旧非阻塞假设不同。"
        ),
    });
}

fn statements_include_axis_move(statements: &[rust_plc::ast::StepStatement]) -> bool {
    statements.iter().any(statement_includes_axis_move)
}

fn statement_includes_axis_move(statement: &rust_plc::ast::StepStatement) -> bool {
    match statement {
        rust_plc::ast::StepStatement::Action(
            rust_plc::ast::ActionStatement::AxisMoveRelative { .. },
        )
        | rust_plc::ast::StepStatement::Action(
            rust_plc::ast::ActionStatement::AxisMoveAbsolute { .. },
        ) => true,
        rust_plc::ast::StepStatement::Repeat { body, .. } => statements_include_axis_move(body),
        rust_plc::ast::StepStatement::Parallel(block) => block
            .branches
            .iter()
            .any(|branch| statements_include_axis_move(&branch.statements)),
        rust_plc::ast::StepStatement::Race(block) => block
            .branches
            .iter()
            .any(|branch| statements_include_axis_move(&branch.statements)),
        _ => false,
    }
}

fn estimate_budget_time(
    budget: &RuntimeBudget,
    thresholds: &RuntimeBudgetThresholds,
) -> BudgetTimeEstimate {
    let action_component_us =
        (budget.max_actions_per_tick_upper_bound as u64).saturating_mul(thresholds.action_cost_us);
    let transition_component_us = (budget.max_transitions_same_tick_upper_bound as u64)
        .saturating_mul(thresholds.transition_cost_us);
    let parallel_expansion = budget
        .max_parallel_branches
        .saturating_sub(1)
        .saturating_add(budget.max_race_branches.saturating_sub(1))
        as u64;
    let parallel_component_us =
        parallel_expansion.saturating_mul(thresholds.parallel_expand_cost_us);
    let total_estimate_us = action_component_us
        .saturating_add(transition_component_us)
        .saturating_add(parallel_component_us);

    BudgetTimeEstimate {
        action_cost_us: thresholds.action_cost_us,
        transition_cost_us: thresholds.transition_cost_us,
        parallel_expand_cost_us: thresholds.parallel_expand_cost_us,
        action_component_us,
        transition_component_us,
        parallel_component_us,
        total_estimate_us,
        max_allowed_us: thresholds.max_budget_time_estimate_us,
        exceeds_budget: total_estimate_us > thresholds.max_budget_time_estimate_us,
    }
}
