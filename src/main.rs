use rust_plc::error::PlcError;
use rust_plc::ir::{ConstraintSet, DeviceKind, StateMachine, TimingModel, TopologyGraph};
use rust_plc::parser::parse_plc;
use rust_plc::semantic::{
    build_constraint_set, build_state_machine, build_timing_model, build_topology_graph,
    preprocess_program,
};
use rust_plc::verification::{verify_all, VerificationSummary, WarningEntry, WarningLevel};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use io_traits::{DigitalInputId, DigitalOutputId, Io};
use petgraph::Direction;
use runtime_core::{Action, Instr, Program, Step, StepId, Task};
use rust_plc::io_map::{IoMap, IoMapError, IoUsage};
use rust_plc::runtime_bridge::state_machine_to_runtime_program;
use rust_plc::scenario_resolve::resolve_scenario_yaml_for_plc;
use rust_plc::sequence_lint::{
    lint_critical_wait_recovery, CriticalWaitExemption, LintLevel, SequenceLintConfig,
};
use rust_plc::sim_regress::{run_sim_regress_with_options, SimRegressOptions, SimRegressSummary};
use rust_plc::tick_timing::{parse_tick_timing_jsonl, to_tick_timing_jsonl, TickTimingSample};
use rust_plc::timing_report::build_timing_report;
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;

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
struct RuntimeBudget {
    /// runtime-core hard cap (see Runtime::tick_with_trace_and_logs).
    max_transitions_per_tick_cap: usize,
    /// Upper bound on same-tick transition chaining in the current state machine.
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
};

const SCENARIO_YAML_MINIMAL_TEMPLATE: &str = r#"tick_ms: 10
duration_ms: 1000
inputs:
  - at_ms: 0
    set:
      digital_inputs:
        0: true
"#;

fn should_skip_suggest_walk_dir(name: &OsStr) -> bool {
    // Keep this list small and obvious: just skip the usual big/noisy directories.
    matches!(
        name.to_str(),
        Some(".git")
            | Some("target")
            | Some("out")
            | Some("archive")
            | Some(".codex")
            | Some(".claude")
            | Some(".ralph_logs")
            | Some("node_modules")
    )
}

fn display_path_relative_to_cwd(p: &Path) -> String {
    match env::current_dir() {
        Ok(cwd) => p
            .strip_prefix(&cwd)
            .map(|rel| rel.display().to_string())
            .unwrap_or_else(|_| p.display().to_string()),
        Err(_) => p.display().to_string(),
    }
}

fn find_similar_yaml_files_by_name(wanted_file_name: &OsStr, max_matches: usize) -> Vec<PathBuf> {
    let Ok(cwd) = env::current_dir() else {
        return Vec::new();
    };

    // Keep this bounded to avoid surprising slowdowns if the tool is run from an unexpected dir.
    let mut matches = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = vec![(cwd, 0)];
    let mut entries_seen: usize = 0;
    let max_entries: usize = 20_000;
    let max_depth: usize = 8;

    while let Some((dir, depth)) = stack.pop() {
        if depth > max_depth {
            continue;
        }
        if dir
            .file_name()
            .is_some_and(|n| should_skip_suggest_walk_dir(n))
        {
            continue;
        }
        let Ok(rd) = fs::read_dir(&dir) else {
            continue;
        };

        for entry in rd {
            entries_seen += 1;
            if entries_seen > max_entries {
                return matches;
            }

            let Ok(entry) = entry else {
                continue;
            };
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            let path = entry.path();

            if ft.is_dir() {
                stack.push((path, depth + 1));
                continue;
            }
            if !ft.is_file() {
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str());
            if !matches!(ext, Some("yaml") | Some("yml")) {
                continue;
            }
            if path.file_name() != Some(wanted_file_name) {
                continue;
            }

            matches.push(path);
            if matches.len() >= max_matches {
                return matches;
            }
        }
    }

    matches
}

fn scenario_yaml_help() -> String {
    let mut msg = String::new();
    msg.push_str("Minimal scenario template:\n");
    msg.push_str(SCENARIO_YAML_MINIMAL_TEMPLATE);
    msg.push('\n');
    msg.push_str("Tips:\n");
    msg.push_str("- `at_ms` must be < `duration_ms` and aligned to `tick_ms`.\n");
    msg.push_str("- IDs are numeric (0 => DI0/AI0, 10 => DI10, ...).\n");
    msg
}

fn read_scenario_yaml_file(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| {
        if err.kind() != std::io::ErrorKind::NotFound {
            return format!(
                "Failed to read scenario YAML file {}: {err}",
                path.display()
            );
        }

        let mut msg = format!("Scenario YAML file not found: {}\n", path.display());
        if let Ok(cwd) = env::current_dir() {
            msg.push_str(&format!("  cwd: {}\n", cwd.display()));
        }

        if let Some(wanted_name) = path.file_name() {
            let suggestions = find_similar_yaml_files_by_name(wanted_name, 6);
            if !suggestions.is_empty() {
                msg.push_str("  similarly named files found:\n");
                for s in suggestions {
                    msg.push_str(&format!("    - {}\n", display_path_relative_to_cwd(&s)));
                }
            }
        }

        msg.push('\n');
        msg.push_str(&scenario_yaml_help());
        msg
    })
}

fn parse_scenario_yaml(yaml: &str) -> Result<sim::Scenario, String> {
    sim::Scenario::from_yaml_str(yaml).map_err(|e| {
        format!(
            "Failed to parse scenario YAML: {e}\n\n{}",
            scenario_yaml_help()
        )
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScenarioInitPreset {
    Minimal,
    Normal,
    Timeout,
    SensorStuck,
    Bounce,
}

impl ScenarioInitPreset {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "minimal" => Some(Self::Minimal),
            "normal" => Some(Self::Normal),
            "timeout" => Some(Self::Timeout),
            "sensor_stuck" => Some(Self::SensorStuck),
            "bounce" => Some(Self::Bounce),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Normal => "normal",
            Self::Timeout => "timeout",
            Self::SensorStuck => "sensor_stuck",
            Self::Bounce => "bounce",
        }
    }
}

#[derive(Debug, Default)]
struct ScenarioInitInputHints {
    digital_ids: Vec<u16>,
    analog_ids: Vec<u16>,
    physical_digital_ids: Vec<u16>,
    physical_analog_ids: Vec<u16>,
    digital_aliases: BTreeMap<u16, Vec<String>>,
    analog_aliases: BTreeMap<u16, Vec<String>>,
}

fn default_scenario_init_out_path(plc_path: &Path) -> PathBuf {
    let parent = plc_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = plc_path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("scenario");
    parent.join(format!("{stem}.scenario.yaml"))
}

fn parse_prefixed_u16(name: &str, prefix: char) -> Option<u16> {
    let rest = name.strip_prefix(prefix)?;
    if rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    rest.parse::<u16>().ok()
}

fn parse_prefixed_token_u16(name: &str, prefix: &str) -> Option<u16> {
    let rest = name.strip_prefix(prefix)?;
    if rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    rest.parse::<u16>().ok()
}

fn collect_scenario_init_hints(plc_source: &str) -> Result<ScenarioInitInputHints, String> {
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
    let state_machine = build_state_machine(&expanded).map_err(|errors| {
        errors
            .into_iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    let runtime = state_machine_to_runtime_program(&topology, &state_machine, 10)
        .map_err(|e| e.to_string())?;

    let mut used_di = BTreeSet::<u16>::new();
    let mut used_ai = BTreeSet::<u16>::new();
    for task in runtime.tasks {
        for step in task.steps {
            match step.instr {
                Instr::WaitDigital { id, .. } => {
                    used_di.insert(id.0);
                }
                Instr::WaitAnalog { id, .. } => {
                    used_ai.insert(id.0);
                }
                Instr::Action { .. } | Instr::Delay { .. } | Instr::Goto { .. } | Instr::Halt => {}
            }
        }
    }
    for pid in runtime.pid_loops {
        used_ai.insert(pid.pv.0);
    }

    let mut digital_aliases = BTreeMap::<u16, Vec<String>>::new();
    let mut analog_aliases = BTreeMap::<u16, Vec<String>>::new();
    for node in topology.graph.node_indices() {
        let device = &topology.graph[node];
        match device.kind {
            DeviceKind::DigitalInput => {
                if let Some(id) = parse_prefixed_u16(&device.name, 'X') {
                    let aliases =
                        collect_downstream_aliases(&topology, node, is_physical_digital_input_name);
                    digital_aliases.insert(id, aliases);
                }
            }
            DeviceKind::AnalogInput => {
                if let Some(id) = parse_prefixed_token_u16(&device.name, "AI") {
                    let aliases =
                        collect_downstream_aliases(&topology, node, is_physical_analog_input_name);
                    analog_aliases.insert(id, aliases);
                }
            }
            _ => {}
        }
    }

    if used_di.is_empty() {
        used_di.extend(digital_aliases.keys().copied());
    }
    if used_ai.is_empty() {
        used_ai.extend(analog_aliases.keys().copied());
    }

    let physical_digital_ids = digital_aliases.keys().copied().collect::<Vec<_>>();
    let physical_analog_ids = analog_aliases.keys().copied().collect::<Vec<_>>();

    Ok(ScenarioInitInputHints {
        digital_ids: used_di.into_iter().collect(),
        analog_ids: used_ai.into_iter().collect(),
        physical_digital_ids,
        physical_analog_ids,
        digital_aliases,
        analog_aliases,
    })
}

fn collect_downstream_aliases(
    topology: &TopologyGraph,
    node: petgraph::graph::NodeIndex,
    is_physical_input_name: fn(&str) -> bool,
) -> Vec<String> {
    let mut aliases = topology
        .graph
        .neighbors_directed(node, Direction::Outgoing)
        .filter_map(|next| {
            let name = topology.graph[next].name.as_str();
            if is_physical_input_name(name) {
                return None;
            }
            Some(name.to_string())
        })
        .collect::<Vec<_>>();
    aliases.sort();
    aliases.dedup();
    aliases
}

fn is_physical_digital_input_name(name: &str) -> bool {
    parse_prefixed_u16(name, 'X').is_some()
}

fn is_physical_analog_input_name(name: &str) -> bool {
    parse_prefixed_token_u16(name, "AI").is_some()
}

fn render_input_alias_comment(aliases: &BTreeMap<u16, Vec<String>>, id: u16) -> String {
    let Some(names) = aliases.get(&id) else {
        return String::new();
    };
    if names.is_empty() {
        return String::new();
    }
    let shown = names.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
    if names.len() > 3 {
        format!(" # {shown}, ...")
    } else {
        format!(" # {shown}")
    }
}

fn render_scenario_init_yaml(
    plc_path: &Path,
    preset: ScenarioInitPreset,
    hints: &ScenarioInitInputHints,
) -> String {
    let source_name = plc_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("<unknown>");

    let mut out = String::new();
    out.push_str("# Generated by `rust_plc scenario-init`.\n");
    out.push_str(&format!("# Source PLC: {source_name}\n"));
    out.push_str(&format!("# Preset: {}\n", preset.as_str()));
    out.push_str("# Keep `at_ms` aligned to `tick_ms`, and keep `at_ms` < `duration_ms`.\n");
    out.push_str("tick_ms: 10\n");
    match preset {
        ScenarioInitPreset::Minimal => out.push_str("duration_ms: 1000\n\n"),
        ScenarioInitPreset::Normal => out.push_str("duration_ms: 6000\n\n"),
        ScenarioInitPreset::Timeout => out.push_str("duration_ms: 2000\n\n"),
        ScenarioInitPreset::SensorStuck => out.push_str("duration_ms: 3000\n\n"),
        ScenarioInitPreset::Bounce => out.push_str("duration_ms: 1000\n\n"),
    }

    if hints.digital_ids.is_empty() && hints.analog_ids.is_empty() {
        out.push_str("# No physical X*/AI* inputs were discovered from this PLC topology.\n");
    }

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

    match preset {
        ScenarioInitPreset::Minimal => {
            out.push_str("# Add input events under `inputs`, for example:\n");
            out.push_str("# - at_ms: 0\n");
            out.push_str("#   set:\n");
            out.push_str("#     digital_inputs:\n");
            out.push_str("#       0: true\n");
            out.push_str("#     analog_inputs:\n");
            out.push_str("#       0: 1.0\n");
            out.push_str("inputs: []\n");
        }
        ScenarioInitPreset::Normal
        | ScenarioInitPreset::Timeout
        | ScenarioInitPreset::SensorStuck
        | ScenarioInitPreset::Bounce => {
            out.push_str("inputs:\n");

            // Start button pulse/bounce if we can identify one from topology aliases.
            if let Some(start_id) = start_id {
                match preset {
                    ScenarioInitPreset::Bounce => {
                        // A few quick toggles to emulate a bouncy button, ending released.
                        let toggles = [
                            (0, true),
                            (10, false),
                            (20, true),
                            (30, false),
                            (40, true),
                            (50, false),
                        ];
                        for (at_ms, value) in toggles {
                            out.push_str(&format!("  - at_ms: {at_ms}\n"));
                            out.push_str("    set:\n");
                            out.push_str("      digital_inputs:\n");
                            let suffix =
                                render_input_alias_comment(&hints.digital_aliases, start_id);
                            out.push_str(&format!("        {start_id}: {value}{suffix}\n"));
                            if at_ms == 0 && !hints.analog_ids.is_empty() {
                                out.push_str("      analog_inputs:\n");
                                for id in &hints.analog_ids {
                                    let suffix =
                                        render_input_alias_comment(&hints.analog_aliases, *id);
                                    out.push_str(&format!("        {id}: 0.0{suffix}\n"));
                                }
                            }
                        }
                    }
                    _ => {
                        out.push_str("  - at_ms: 0\n");
                        out.push_str("    set:\n");
                        out.push_str("      digital_inputs:\n");
                        let suffix = render_input_alias_comment(&hints.digital_aliases, start_id);
                        out.push_str(&format!("        {start_id}: true{suffix}\n"));
                        if !hints.analog_ids.is_empty() {
                            out.push_str("      analog_inputs:\n");
                            for id in &hints.analog_ids {
                                let suffix = render_input_alias_comment(&hints.analog_aliases, *id);
                                out.push_str(&format!("        {id}: 0.0{suffix}\n"));
                            }
                        }
                        out.push_str("  - at_ms: 50\n");
                        out.push_str("    set:\n");
                        out.push_str("      digital_inputs:\n");
                        out.push_str(&format!("        {start_id}: false{suffix}\n"));
                    }
                }
            } else {
                // Keep a placeholder to guide first-time authors.
                out.push_str("  - at_ms: 0\n");
                out.push_str("    set:\n");
                out.push_str("      digital_inputs:\n");
                out.push_str("        0: true  # (example) press start button\n");
                if !hints.analog_ids.is_empty() {
                    out.push_str("      analog_inputs:\n");
                    for id in &hints.analog_ids {
                        let suffix = render_input_alias_comment(&hints.analog_aliases, *id);
                        out.push_str(&format!("        {id}: 0.0{suffix}\n"));
                    }
                }
                out.push_str("  - at_ms: 50\n");
                out.push_str("    set:\n");
                out.push_str("      digital_inputs:\n");
                out.push_str("        0: false # (example) release\n");
            }

            // For a "normal" preset, try to drive common sensor waits by turning sensors on later.
            if preset == ScenarioInitPreset::Normal {
                // Heuristic: script sensor edges in a stable order so waits don't satisfy immediately.
                let mut t = 100u64;
                for id in &sensor_ids {
                    out.push_str(&format!("  - at_ms: {t}\n"));
                    out.push_str("    set:\n");
                    out.push_str("      digital_inputs:\n");
                    let suffix = render_input_alias_comment(&hints.digital_aliases, *id);
                    out.push_str(&format!("        {id}: true{suffix}\n"));
                    t = t.saturating_add(20);
                    if t >= 1000 {
                        break;
                    }
                }
            }

            // Inject one representative stuck fault for the template.
            if preset == ScenarioInitPreset::SensorStuck {
                let target = sensor_ids.first().copied().or(start_id).unwrap_or(0);
                out.push_str("\n# Fault injection example:\n");
                out.push_str("faults:\n");
                out.push_str("  - sensor_stuck:\n");
                out.push_str("      at_ms: 200\n");
                let suffix = render_input_alias_comment(&hints.digital_aliases, target);
                out.push_str(&format!("      target: {target}{suffix}\n"));
                out.push_str("      value: true\n");
            }
        }
    }

    out.push_str("\n# Force/override (optional). Use YAML `null` to clear a forced value.\n");
    out.push_str("# Example:\n");
    out.push_str("# forces:\n");
    out.push_str("#   - at_ms: 0\n");
    out.push_str("#     set:\n");
    out.push_str("#       digital_inputs:\n");
    out.push_str("#         0: true\n");
    out.push_str("#   - at_ms: 100\n");
    out.push_str("#     set:\n");
    out.push_str("#       digital_inputs:\n");
    out.push_str("#         0: null\n");
    out.push_str("forces: []\n");
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScenarioValidateSeverity {
    Error,
    Warn,
}

impl ScenarioValidateSeverity {
    fn label(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN",
        }
    }

    fn json_label(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum CliOutputMode {
    Human,
    Json,
}

impl CliOutputMode {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "human" => Some(Self::Human),
            "json" => Some(Self::Json),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Json => "json",
        }
    }
}

#[derive(Debug, Clone)]
struct ScenarioValidateFinding {
    severity: ScenarioValidateSeverity,
    tag: String,
    message: String,
    suggestion: Option<String>,
}

impl ScenarioValidateFinding {
    fn error(
        tag: impl Into<String>,
        message: impl Into<String>,
        suggestion: Option<String>,
    ) -> Self {
        Self {
            severity: ScenarioValidateSeverity::Error,
            tag: tag.into(),
            message: message.into(),
            suggestion,
        }
    }

    fn warn(
        tag: impl Into<String>,
        message: impl Into<String>,
        suggestion: Option<String>,
    ) -> Self {
        Self {
            severity: ScenarioValidateSeverity::Warn,
            tag: tag.into(),
            message: message.into(),
            suggestion,
        }
    }

    fn code(&self) -> &'static str {
        match self.tag.as_str() {
            "duration_ms" => "SCN-VAL-001",
            "runtime.probe" => "SCN-VAL-002",
            "risk.start_button_held" => "SCN-RISK-001",
            "risk.sensors_all_true_at_start" => "SCN-RISK-002",
            "risk.scenario_plc_mismatch" => "SCN-MAP-001",
            tag if tag.ends_with(".at_ms") => "SCN-TICK-001",
            tag if tag.contains("digital_inputs") => "SCN-MAP-002",
            tag if tag.contains("analog_inputs") => "SCN-MAP-003",
            tag if tag.contains("digital_outputs") => "SCN-MAP-004",
            tag if tag.contains("analog_outputs") => "SCN-MAP-005",
            _ => match self.severity {
                ScenarioValidateSeverity::Error => "SCN-VAL-999",
                ScenarioValidateSeverity::Warn => "SCN-RISK-999",
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct ScenarioValidateIssueJson<'a> {
    code: &'static str,
    severity: &'static str,
    tag: &'a str,
    message: &'a str,
    suggestion: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct ScenarioValidateJsonReport<'a> {
    schema_version: u32,
    command: &'static str,
    output: &'static str,
    status: &'static str,
    error_count: usize,
    warn_count: usize,
    issues: Vec<ScenarioValidateIssueJson<'a>>,
}

fn print_scenario_validate_findings(findings: &[ScenarioValidateFinding], output: CliOutputMode) {
    let errors = findings
        .iter()
        .filter(|f| f.severity == ScenarioValidateSeverity::Error)
        .count();
    let warnings = findings
        .iter()
        .filter(|f| f.severity == ScenarioValidateSeverity::Warn)
        .count();

    if output == CliOutputMode::Json {
        let report = ScenarioValidateJsonReport {
            schema_version: 1,
            command: "scenario-validate",
            output: output.as_str(),
            status: if errors == 0 { "pass" } else { "fail" },
            error_count: errors,
            warn_count: warnings,
            issues: findings
                .iter()
                .map(|f| ScenarioValidateIssueJson {
                    code: f.code(),
                    severity: f.severity.json_label(),
                    tag: &f.tag,
                    message: &f.message,
                    suggestion: f.suggestion.as_deref(),
                })
                .collect(),
        };
        match serde_json::to_string_pretty(&report) {
            Ok(mut body) => {
                body.push('\n');
                print!("{body}");
            }
            Err(err) => eprintln!("Failed to serialize scenario-validate JSON output: {err}"),
        }
        return;
    }

    if errors == 0 && warnings == 0 {
        eprintln!("scenario-validate: PASS (no issues)");
        return;
    }
    if errors == 0 {
        eprintln!("scenario-validate: PASS ({warnings} warning(s))");
    } else {
        eprintln!("scenario-validate: FAIL ({errors} error(s), {warnings} warning(s))");
    }

    for finding in findings {
        eprintln!(
            "{} [{}:{}] {}",
            finding.severity.label(),
            finding.code(),
            finding.tag,
            finding.message
        );
        if let Some(suggestion) = &finding.suggestion {
            eprintln!("  Fix:\n{suggestion}");
        }
    }
}

fn collect_scenario_referenced_inputs(
    scenario: &sim::Scenario,
) -> (Vec<(String, u16)>, Vec<(String, u16)>) {
    let mut digital = Vec::<(String, u16)>::new();
    let mut analog = Vec::<(String, u16)>::new();

    for (event_idx, event) in scenario.inputs.iter().enumerate() {
        for (&id, _) in &event.set.digital_inputs {
            digital.push((format!("inputs[{event_idx}].set.digital_inputs.{id}"), id));
        }
        for (&id, _) in &event.set.analog_inputs {
            analog.push((format!("inputs[{event_idx}].set.analog_inputs.{id}"), id));
        }
    }
    for (idx, burst) in scenario.digital_bursts.iter().enumerate() {
        digital.push((format!("digital_bursts[{idx}].target"), burst.target));
    }
    for (idx, fault) in scenario.faults.iter().enumerate() {
        digital.push((
            format!("faults[{idx}].sensor_stuck.target"),
            fault.sensor_stuck.target,
        ));
    }

    for (event_idx, force) in scenario.forces.iter().enumerate() {
        for (&id, _) in &force.set.digital_inputs {
            digital.push((format!("forces[{event_idx}].set.digital_inputs.{id}"), id));
        }
        for (&id, _) in &force.set.analog_inputs {
            analog.push((format!("forces[{event_idx}].set.analog_inputs.{id}"), id));
        }
    }

    (digital, analog)
}

fn collect_scenario_referenced_forced_outputs(
    scenario: &sim::Scenario,
) -> (Vec<(String, u16)>, Vec<(String, u16)>) {
    let mut digital = Vec::<(String, u16)>::new();
    let mut analog = Vec::<(String, u16)>::new();

    for (event_idx, force) in scenario.forces.iter().enumerate() {
        for (&id, _) in &force.set.digital_outputs {
            digital.push((format!("forces[{event_idx}].set.digital_outputs.{id}"), id));
        }
        for (&id, _) in &force.set.analog_outputs {
            analog.push((format!("forces[{event_idx}].set.analog_outputs.{id}"), id));
        }
    }

    (digital, analog)
}

fn collect_initial_digital_values(scenario: &sim::Scenario) -> BTreeMap<u16, bool> {
    let mut values = BTreeMap::<u16, bool>::new();
    for event in &scenario.inputs {
        if event.at_ms != 0 {
            continue;
        }
        for (&id, &value) in &event.set.digital_inputs {
            values.insert(id, value);
        }
    }

    // Faults are applied after scripted inputs for the same tick.
    for fault in &scenario.faults {
        if fault.sensor_stuck.at_ms != 0 {
            continue;
        }
        values.insert(fault.sensor_stuck.target, fault.sensor_stuck.value);
    }
    values
}

fn aliases_contain_keyword(aliases: &[String], keyword: &str) -> bool {
    aliases
        .iter()
        .any(|name| name.to_ascii_lowercase().contains(keyword))
}

fn first_alias(aliases: &BTreeMap<u16, Vec<String>>, id: u16) -> Option<String> {
    aliases
        .get(&id)
        .and_then(|names| names.first())
        .map(|s| s.to_string())
}

fn has_later_digital_false(scenario: &sim::Scenario, id: u16) -> bool {
    scenario.inputs.iter().any(|event| {
        event.at_ms > 0
            && event
                .set
                .digital_inputs
                .get(&id)
                .copied()
                .map(|value| !value)
                .unwrap_or(false)
    }) || scenario.faults.iter().any(|fault| {
        fault.sensor_stuck.target == id && fault.sensor_stuck.at_ms > 0 && !fault.sensor_stuck.value
    })
}

fn validate_scenario_against_plc(
    plc_path: &Path,
    scenario_path: &Path,
    scenario: &sim::Scenario,
    hints: &ScenarioInitInputHints,
) -> Vec<ScenarioValidateFinding> {
    let mut findings = Vec::<ScenarioValidateFinding>::new();

    if scenario.duration_ms == 0 {
        findings.push(ScenarioValidateFinding::error(
            "duration_ms",
            "must be > 0",
            Some("duration_ms: 1000".to_string()),
        ));
    }

    let mut io = sim::SimIo::new(1, 1, 0, 0);
    if let Err(err) = scenario.apply_to_simio(&mut io) {
        if let sim::ScenarioError::Validation { path, message } = err {
            let tick_suggestion = if path.ends_with(".at_ms") {
                format!(
                    "Use multiples of tick_ms ({}), e.g. 0, {}, {}",
                    scenario.tick_ms,
                    scenario.tick_ms,
                    scenario.tick_ms.saturating_mul(2)
                )
            } else {
                "Check the scenario field value and retry".to_string()
            };
            findings.push(ScenarioValidateFinding::error(
                path,
                message,
                Some(tick_suggestion),
            ));
        }
    }

    let valid_di = if !hints.physical_digital_ids.is_empty() {
        hints
            .physical_digital_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
    } else {
        hints.digital_ids.iter().copied().collect::<BTreeSet<_>>()
    };
    let valid_ai = if !hints.physical_analog_ids.is_empty() {
        hints
            .physical_analog_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
    } else {
        hints.analog_ids.iter().copied().collect::<BTreeSet<_>>()
    };

    let (digital_refs, analog_refs) = collect_scenario_referenced_inputs(scenario);
    let plc_display = display_path_relative_to_cwd(plc_path);
    let scenario_display = display_path_relative_to_cwd(scenario_path);
    let skeleton_cmd = format!(
        "  rust_plc scenario-init {} --out {} --preset normal",
        plc_display, scenario_display
    );

    for (path, id) in digital_refs {
        if !valid_di.is_empty() && !valid_di.contains(&id) {
            let known = valid_di.iter().copied().collect::<Vec<_>>();
            let known_text = if known.is_empty() {
                "none".to_string()
            } else {
                known
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            findings.push(ScenarioValidateFinding::error(
                path,
                format!("DI{id} does not exist in `{plc_display}` (known DI ids: {known_text})"),
                Some(format!(
                    "Regenerate a PLC-matched scenario skeleton:\n{skeleton_cmd}"
                )),
            ));
        }
    }
    for (path, id) in analog_refs {
        if !valid_ai.is_empty() && !valid_ai.contains(&id) {
            let known = valid_ai.iter().copied().collect::<Vec<_>>();
            let known_text = if known.is_empty() {
                "none".to_string()
            } else {
                known
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            findings.push(ScenarioValidateFinding::error(
                path,
                format!("AI{id} does not exist in `{plc_display}` (known AI ids: {known_text})"),
                Some(format!(
                    "Regenerate a PLC-matched scenario skeleton:\n{skeleton_cmd}"
                )),
            ));
        }
    }

    let initial = collect_initial_digital_values(scenario);
    let mut start_ids = hints
        .digital_aliases
        .iter()
        .filter_map(|(&id, aliases)| {
            if aliases_contain_keyword(aliases, "start") {
                Some(id)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    start_ids.sort_unstable();
    start_ids.dedup();

    for id in start_ids {
        if initial.get(&id).copied().unwrap_or(false) && !has_later_digital_false(scenario, id) {
            let label = first_alias(&hints.digital_aliases, id)
                .map(|name| format!("{name} (DI{id})"))
                .unwrap_or_else(|| format!("DI{id}"));
            findings.push(ScenarioValidateFinding::warn(
                "risk.start_button_held",
                format!("{label} starts true and is never released; this can cause same-tick loops"),
                Some(format!(
                    "inputs:\n  - at_ms: 0\n    set:\n      digital_inputs:\n        {id}: true\n  - at_ms: {}\n    set:\n      digital_inputs:\n        {id}: false",
                    scenario.tick_ms
                )),
            ));
        }
    }

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

    if !sensor_ids.is_empty()
        && sensor_ids
            .iter()
            .all(|id| initial.get(id).copied().unwrap_or(false))
    {
        let preview = sensor_ids.iter().take(3).copied().collect::<Vec<_>>();
        let mut snippet = String::from("inputs:\n  - at_ms: 0\n    set:\n      digital_inputs:\n");
        for id in preview {
            snippet.push_str(&format!("        {id}: false\n"));
        }
        snippet.push_str("  # add later `at_ms` edges to set each sensor true when reached");
        findings.push(ScenarioValidateFinding::warn(
            "risk.sensors_all_true_at_start",
            "all known sensor inputs start true; waits/guards may be satisfied immediately"
                .to_string(),
            Some(snippet),
        ));
    }

    findings
}

fn scenario_mismatch_hint_for_example(
    plc_path: &str,
    scenario_path: &Path,
    err: &sim::SimRunError,
    subcommand: &str,
) -> Option<String> {
    if !matches!(
        err,
        sim::SimRunError::Runtime(runtime_core::RuntimeError::TooManyTransitionsInOneTick)
    ) {
        return None;
    }

    scenario_mismatch_hint_for_example_paths(plc_path, scenario_path, subcommand)
}

fn scenario_mismatch_hint_for_example_paths(
    plc_path: &str,
    scenario_path: &Path,
    subcommand: &str,
) -> Option<String> {
    let plc_name = Path::new(plc_path).file_name().and_then(|s| s.to_str())?;
    let scenario_name = scenario_path.file_name().and_then(|s| s.to_str())?;

    if plc_name == "two_cylinder.plc" && scenario_name == "normal.yaml" {
        let suggested_cmd = if subcommand == "sim-plc" {
            "cargo run --release -- sim-plc examples/two_cylinder.plc --scenario scenarios/two_cylinder.yaml --out trace.jsonl"
        } else {
            "cargo run --release -- no-board-gate examples/two_cylinder.plc --scenario scenarios/two_cylinder.yaml --out-dir out/no_board_gate"
        };
        return Some(format!(
            "Tip: `scenarios/normal.yaml` is tuned for `examples/assembly_station.plc`.\n\
For `examples/two_cylinder.plc`, use `scenarios/two_cylinder.yaml`:\n\
  {suggested_cmd}"
        ));
    }

    None
}

fn format_resolve_scenario_yaml_error(
    plc_path: &str,
    scenario_path: &Path,
    subcommand: &str,
    err: &str,
) -> String {
    let mut msg = format!(
        "Failed to resolve device-name inputs in scenario {}:\n{err}",
        scenario_path.display()
    );
    if let Some(hint) =
        scenario_mismatch_hint_for_example_paths(plc_path, scenario_path, subcommand)
    {
        msg.push_str("\n\n");
        msg.push_str(&hint);
    }
    msg
}

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
    if first == "sim-pid-kpi" {
        if let Err(msg) = run_sim_pid_kpi_subcommand(&program, args) {
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
            eprintln!("[BLD-000] {msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "release-bundle" {
        if let Err(msg) = run_release_bundle_subcommand(&program, args) {
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
    if first == "board-parse" {
        if let Err(msg) = run_board_parse_subcommand(&program, args) {
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
    if first == "timing-report" {
        if let Err(msg) = run_timing_report_subcommand(&program, args) {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "io-map-normalize" {
        if let Err(msg) = run_io_map_normalize_subcommand(&program, args) {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "no-board-gate" {
        if let Err(msg) = run_no_board_gate_subcommand(&program, args) {
            eprintln!("[GATE-000] {msg}");
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
    if first == "virtual-board" {
        if let Err(msg) = run_virtual_board_subcommand(&program, args) {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "sequence-lint" {
        if let Err(msg) = run_sequence_lint_subcommand(&program, args) {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "scenario-init" {
        if let Err(msg) = run_scenario_init_subcommand(&program, args) {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "scenario-validate" {
        if let Err(msg) = run_scenario_validate_subcommand(&program, args) {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "scenario-doctor" {
        if let Err(msg) = run_scenario_doctor_subcommand(&program, args) {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "scenario-expand" {
        if let Err(msg) = run_scenario_expand_subcommand(&program, args) {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "scenario-gen" {
        if let Err(msg) = run_scenario_gen_subcommand(&program, args) {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }
    if first == "new" {
        if let Err(msg) = run_new_subcommand(&program, args) {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        return;
    }

    let path = first;
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
            // Back-compat note: the CLI historically printed IR JSON to stdout by default.
            // Keep that as the default for tests/scripts, and offer a switch to suppress it.
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

fn print_usage(program: &str) {
    eprintln!("Usage:");
    eprintln!(
        "  {program} <file.plc> [--report <verification_report.json>] [--deny-warnings] [--no-print-ir] [--ir-out <ir_bundle.json>] [--budget-... <value>]"
    );
    eprintln!(
        "  {program} sim <scenario.yaml> [--out <trace.jsonl>] [--vcd-out <wave.vcd>] [--analog-out <analog.csv>] [--report-out <report.json>]"
    );
    eprintln!(
        "  {program} sim-plc <file.plc> --scenario <scenario.yaml> --out <trace.jsonl> [--retain-config <retain.toml>] [--retain-state <retain_state.json>] [--enable-online-force-dev] [--online-force-script <script.jsonl>] [--online-force-audit-out <audit.jsonl>] [--online-var-script <script.jsonl>] [--online-var-audit-out <audit.jsonl>]"
    );
    eprintln!(
        "  {program} sim-regress --plc-dir <dir> --scenario-dir <dir> [--artifacts-dir <dir>] [--summary-out <summary.json>] [--minimize-failure]"
    );
    eprintln!(
        "  {program} sim-pid-kpi <file.plc> --scenario <pid_scenario.yaml> [--out <kpi.json>]"
    );
    eprintln!(
        "  {program} build-rp2040 <file.plc> --out <dir> [--io-map <file>] [--analog-calibration <file>] [--emit-uf2 <file.uf2>] [--output <human|json>]"
    );
    eprintln!(
        "  {program} release-bundle <file.plc> --scenario <scenario.yaml> --out-dir <dir> [--io-map <file>] [--max-p99-exec-us <us>] [--max-overrun-count <n>]"
    );
    eprintln!("  {program} flash-rp2040 --uf2 <file.uf2> --mount <path> [--dry-run]");
    eprintln!("  {program} board-parse --in <board.log> --out-dir <dir>");
    eprintln!(
        "  {program} trace-diff --sil <trace.jsonl> --board <trace.jsonl> --out <report.json> [--context <n>] [--fail-on-mismatch]"
    );
    eprintln!("  {program} timing-report --in <tick_timing.jsonl> [--out <timing_report.json>]");
    eprintln!("  {program} io-map-normalize --in <io_map.toml> --out <normalized.toml>");
    eprintln!(
        "  {program} no-board-gate <file.plc> --scenario <scenario.yaml> --out-dir <dir> [--context <n>] [--sil-scenario <scenario.yaml>] [--board-scenario <scenario.yaml>] [--max-p99-exec-us <us>] [--max-overrun-count <n>] [--output <human|json>]"
    );
    eprintln!("  {program} new <project_dir> [--force]");
    eprintln!("  {program} pil-run <file.plc> --scenario <scenario.yaml>");
    eprintln!("  {program} virtual-board <file.plc> --scenario <scenario.yaml> --out-dir <dir>");
    eprintln!(
        "  {program} sequence-lint <file.plc> [--critical-wait-level <warn|error>] [--critical-wait-exempt <task.step|task.*>]"
    );
    eprintln!(
        "  {program} scenario-init <file.plc> [--out <scenario.yaml>] [--preset <minimal|normal|timeout|sensor_stuck|bounce>]"
    );
    eprintln!(
        "  {program} scenario-validate <file.plc> --scenario <scenario.yaml> [--output <human|json>]"
    );
    eprintln!(
        "  {program} scenario-doctor <file.plc> --scenario <scenario.yaml> [--fix-preview] [--output <human|json>]"
    );
    eprintln!(
        "  {program} scenario-expand <file.plc> --scenario <scenario.yaml> --out <expanded.yaml>"
    );
    eprintln!(
        "  {program} scenario-gen --plc <file.plc> --config <gen.yaml> --out-dir <dir> [--coverage-mode <pairwise|boundary-first|risk-first>] [--dry-run] [--template-library <metadata.json>]"
    );
    eprintln!();
    eprintln!("Budget options (also configurable via env vars):");
    eprintln!("  --budget-max-actions-per-transition <n>");
    eprintln!("  --budget-max-actions-per-tick <n>");
    eprintln!("  --budget-max-parallel-branches <n>");
    eprintln!("  --budget-max-race-branches <n>");
    eprintln!("  --budget-warn-on-same-tick-cycle <true|false>");
    eprintln!("  --budget-action-cost-us <n>");
    eprintln!("  --budget-transition-cost-us <n>");
    eprintln!("  --budget-parallel-expand-cost-us <n>");
    eprintln!("  --budget-max-time-estimate-us <n>");
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

fn run_new_subcommand(program: &str, mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let usage = format!("Usage: {program} new <project_dir> [--force]");
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

    let readme = "# RustPLC Bootstrap Project\n\n## Quick Start Checklist\n\n1. Validate scenario contract:\n\n```bash\ncargo run --release -- scenario-validate plc/main.plc --scenario scenarios/normal.yaml --output human\n```\n\n2. Run no-board regression gate:\n\n```bash\ncargo run --release -- no-board-gate plc/main.plc --scenario scenarios/normal.yaml --out-dir out/no_board_gate --output human\n```\n\n3. Optional RP2040 build baseline:\n\n```bash\ncargo run --release -- build-rp2040 plc/main.plc --out out/rp2040 --io-map io_map.toml\n```\n";
    let plc = "[topology]\n\ndevice X0: digital_input\ndevice Y0: digital_output\n\n[constraints]\n\n[tasks]\n\ntask main:\n    step wait_start:\n        wait: X0 == true\n        timeout: 100ms -> goto fault\n\n    step run:\n        action: set Y0 on\n        delay: 20ms\n\n    step stop:\n        action: set Y0 off\n\n    on_complete: goto done\n\ntask fault:\n    step safe_stop:\n        action: set Y0 off\n    on_complete: goto done\n\ntask done:\n    step halt:\n";
    let scenario = "tick_ms: 10\nduration_ms: 300\ninputs:\n  - at_ms: 0\n    set:\n      digital_inputs:\n        0: true\n  - at_ms: 50\n    set:\n      digital_inputs:\n        0: false\nforces: []\n";
    let io_map = "schema_version = 1\n\n[digital_inputs]\ndi0 = { gpio = 2, pull = \"up\" }\n\n[digital_outputs]\ndo0 = { gpio = 10, active_low = false }\n\n[safe_state]\nmode = \"all_zero\"\non_exit_timeout_ms = 0\n";
    let workflow = "name: rustplc-no-board-gate\n\non:\n  push:\n  pull_request:\n\njobs:\n  no-board-gate:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n      - uses: dtolnay/rust-toolchain@stable\n      - name: Scenario validate\n        run: cargo run --release -- scenario-validate plc/main.plc --scenario scenarios/normal.yaml --output json\n      - name: No-board gate\n        run: cargo run --release -- no-board-gate plc/main.plc --scenario scenarios/normal.yaml --out-dir out/no_board_gate --output json\n";
    let vscode_tasks = "{\n  \"version\": \"2.0.0\",\n  \"tasks\": [\n    {\n      \"label\": \"RustPLC: scenario-validate\",\n      \"type\": \"shell\",\n      \"command\": \"cargo run --release -- scenario-validate plc/main.plc --scenario scenarios/normal.yaml --output human\",\n      \"problemMatcher\": []\n    },\n    {\n      \"label\": \"RustPLC: no-board-gate\",\n      \"type\": \"shell\",\n      \"command\": \"cargo run --release -- no-board-gate plc/main.plc --scenario scenarios/normal.yaml --out-dir out/no_board_gate --output human\",\n      \"problemMatcher\": []\n    }\n  ]\n}\n";
    let vscode_settings = "{\n  \"files.associations\": {\n    \"*.plc\": \"ini\"\n  }\n}\n";
    let vscode_extensions = "{\n  \"recommendations\": [\n    \"rust-lang.rust-analyzer\",\n    \"redhat.vscode-yaml\",\n    \"tamasfe.even-better-toml\"\n  ]\n}\n";

    write_scaffold_file(&root.join("README.md"), readme, force)?;
    write_scaffold_file(&root.join("plc/main.plc"), plc, force)?;
    write_scaffold_file(&root.join("scenarios/normal.yaml"), scenario, force)?;
    write_scaffold_file(&root.join("io_map.toml"), io_map, force)?;
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

    eprintln!("new: scaffold created at {}", root.display());
    Ok(())
}

fn run_sequence_lint_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let Some(plc_path) = args.next() else {
        return Err(format!(
            "Usage: {program} sequence-lint <file.plc> [--critical-wait-level <warn|error>] [--critical-wait-exempt <task.step|task.*>]"
        ));
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
                return Err(format!(
                    "Usage: {program} sequence-lint <file.plc> [--critical-wait-level <warn|error>] [--critical-wait-exempt <task.step|task.*>]"
                ));
            }
            other => {
                return Err(format!("Unknown argument for sequence-lint: {other}"));
            }
        }
    }

    let source =
        fs::read_to_string(&plc_path).map_err(|err| format!("Failed to read {plc_path}: {err}"))?;
    let parsed = parse_plc(&source).map_err(|err| err.to_string())?;
    let expanded = preprocess_program(&parsed).map_err(|errors| {
        errors
            .into_iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    })?;

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

fn run_scenario_init_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let Some(plc_path) = args.next() else {
        return Err(format!(
            "Usage: {program} scenario-init <file.plc> [--out <scenario.yaml>] [--preset <minimal|normal|timeout|sensor_stuck|bounce>]"
        ));
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
                    .ok_or_else(|| "Missing value for --preset <minimal|normal>".to_string())?;
                preset = ScenarioInitPreset::parse(&raw).ok_or_else(|| {
                    format!("Invalid preset `{raw}` (expected `minimal` or `normal`)")
                })?;
            }
            "-h" | "--help" => {
                return Err(format!(
                    "Usage: {program} scenario-init <file.plc> [--out <scenario.yaml>] [--preset <minimal|normal|timeout|sensor_stuck|bounce>]"
                ));
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

fn run_scenario_validate_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let Some(plc_path) = args.next() else {
        return Err(format!(
            "Usage: {program} scenario-validate <file.plc> --scenario <scenario.yaml> [--output <human|json>]"
        ));
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
                return Err(format!(
                    "Usage: {program} scenario-validate <file.plc> --scenario <scenario.yaml> [--output <human|json>]"
                ));
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
                        "sim-plc",
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

fn run_scenario_doctor_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let Some(plc_path) = args.next() else {
        return Err(format!(
            "Usage: {program} scenario-doctor <file.plc> --scenario <scenario.yaml> [--fix-preview] [--output <human|json>]"
        ));
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
                return Err(format!(
                    "Usage: {program} scenario-doctor <file.plc> --scenario <scenario.yaml> [--fix-preview] [--output <human|json>]"
                ));
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

fn run_scenario_expand_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let Some(plc_path) = args.next() else {
        return Err(format!(
            "Usage: {program} scenario-expand <file.plc> --scenario <scenario.yaml> --out <expanded.yaml>"
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
                        "Missing value for --out <expanded.yaml>".to_string()
                    })?));
            }
            "-h" | "--help" => {
                return Err(format!(
                    "Usage: {program} scenario-expand <file.plc> --scenario <scenario.yaml> --out <expanded.yaml>"
                ));
            }
            other => return Err(format!("Unknown argument for scenario-expand: {other}")),
        }
    }

    let scenario_path = scenario_path.ok_or_else(|| {
        format!(
            "Usage: {program} scenario-expand <file.plc> --scenario <scenario.yaml> --out <expanded.yaml>"
        )
    })?;
    let out_path = out_path.ok_or_else(|| {
        format!(
            "Usage: {program} scenario-expand <file.plc> --scenario <scenario.yaml> --out <expanded.yaml>"
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

fn run_scenario_gen_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = format!(
        "Usage: {program} scenario-gen --plc <file.plc> --config <gen.yaml> --out-dir <dir> [--coverage-mode <pairwise|boundary-first|risk-first>] [--dry-run] [--template-library <metadata.json>]"
    );
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
    operation: &'static str,
    from: Option<OnlineVariableAuditValue>,
    to: Option<OnlineVariableAuditValue>,
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
            other => Err(format!("BOOL variable expects bool/null value, got {other}")),
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
            other => Err(format!("REAL variable expects numeric/null value, got {other}")),
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
            variable_name: name,
            value,
        });
    }
    commands.sort_by(|a, b| a.at_ms.cmp(&b.at_ms));
    Ok(commands)
}

fn build_online_variable_audit(
    commands: &[OnlineVariableCommand],
    tick_ms: u64,
) -> Vec<OnlineVariableAuditEntry> {
    let mut out = Vec::<OnlineVariableAuditEntry>::new();
    let mut bool_values = BTreeMap::<String, bool>::new();
    let mut real_values = BTreeMap::<String, f32>::new();

    for cmd in commands {
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
            operation: if cmd.value.is_some() { "set" } else { "clear" },
            from,
            to,
        });
    }

    out
}

fn default_online_variable_audit_path(trace_out: &Path) -> PathBuf {
    trace_out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join("online_var_audit.jsonl")
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
    let usage = format!(
        "Usage: {program} sim-plc <file.plc> --scenario <scenario.yaml> --out <trace.jsonl> [--retain-config <retain.toml>] [--retain-state <retain_state.json>] [--enable-online-force-dev] [--online-force-script <script.jsonl>] [--online-force-audit-out <audit.jsonl>] [--online-var-script <script.jsonl>] [--online-var-audit-out <audit.jsonl>]"
    );
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
    let mut online_var_audit_out: Option<PathBuf> = None;
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
            "--online-var-audit-out" => {
                online_var_audit_out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --online-var-audit-out <audit.jsonl>".to_string()
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
        || online_var_audit_out.is_some())
        && !enable_online_force_dev
    {
        return Err(
            "online-force/variable dev control plane is disabled by default; add --enable-online-force-dev to use --online-force-script/--online-force-audit-out/--online-var-script/--online-var-audit-out"
                .to_string(),
        );
    }

    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to create output directory {parent:?}: {err}"))?;
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
    if let Some(script_path) = &online_var_script {
        online_variable_commands = load_online_variable_script(script_path, scenario.tick_ms)?;
    }
    if let Some(path) = &variable_audit_path {
        let variable_audit = build_online_variable_audit(&online_variable_commands, scenario.tick_ms);
        write_online_variable_audit(path, &variable_audit)?;
    }

    let program = compile_plc_to_runtime_program(&plc_source, scenario.tick_ms)?;

    let (num_di, num_do, num_ai, num_ao) = io_sizes_for_program_and_scenario(&program, &scenario);
    let mut io = sim::SimIo::new(num_di, num_do, num_ai, num_ao);
    let run = sim::run_program_for_scenario(&program, &scenario, &mut io).map_err(|e| {
        let mut msg = format!("{e}");
        if let Some(hint) =
            scenario_mismatch_hint_for_example(&plc_path, &scenario_path, &e, "sim-plc")
        {
            msg.push_str("\n\n");
            msg.push_str(&hint);
        }
        msg
    })?;
    fs::write(&out_path, run.trace.into_string())
        .map_err(|err| format!("Failed to write trace file {out_path:?}: {err}"))?;
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
    let Some(plc_path) = args.next() else {
        return Err(format!(
            "Usage: {program} sim-pid-kpi <file.plc> --scenario <pid_scenario.yaml> [--out <kpi.json>]"
        ));
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
                return Err(format!(
                    "Usage: {program} sim-pid-kpi <file.plc> --scenario <pid_scenario.yaml> [--out <kpi.json>]"
                ));
            }
            other => return Err(format!("Unknown argument for sim-pid-kpi: {other}")),
        }
    }

    let scenario_path = scenario_path.ok_or_else(|| {
        format!(
            "Usage: {program} sim-pid-kpi <file.plc> --scenario <pid_scenario.yaml> [--out <kpi.json>]"
        )
    })?;
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
    let Some(plc_path) = args.next() else {
        return Err(format!(
            "Usage: {program} build-rp2040 <file.plc> --out <dir> [--io-map <file>] [--analog-calibration <file>] [--emit-uf2 <file.uf2>] [--output <human|json>]"
        ));
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
                return Err(format!(
                    "Usage: {program} build-rp2040 <file.plc> --out <dir> [--io-map <file>] [--analog-calibration <file>] [--emit-uf2 <file.uf2>] [--output <human|json>]"
                ));
            }
            other => return Err(format!("Unknown argument for build-rp2040: {other}")),
        }
    }

    let out_dir = out_dir.ok_or_else(|| {
        format!(
            "Usage: {program} build-rp2040 <file.plc> --out <dir> [--io-map <file>] [--analog-calibration <file>] [--emit-uf2 <file.uf2>] [--output <human|json>]"
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
    let Some(plc_path) = args.next() else {
        return Err(format!(
            "Usage: {program} release-bundle <file.plc> --scenario <scenario.yaml> --out-dir <dir> [--io-map <file>] [--max-p99-exec-us <us>] [--max-overrun-count <n>]"
        ));
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
                return Err(format!(
                    "Usage: {program} release-bundle <file.plc> --scenario <scenario.yaml> --out-dir <dir> [--io-map <file>] [--max-p99-exec-us <us>] [--max-overrun-count <n>]"
                ));
            }
            other => return Err(format!("Unknown argument for release-bundle: {other}")),
        }
    }

    let scenario_path = scenario_path.ok_or_else(|| {
        format!(
            "Usage: {program} release-bundle <file.plc> --scenario <scenario.yaml> --out-dir <dir> [--io-map <file>] [--max-p99-exec-us <us>] [--max-overrun-count <n>]"
        )
    })?;
    let out_dir = out_dir.ok_or_else(|| {
        format!(
            "Usage: {program} release-bundle <file.plc> --scenario <scenario.yaml> --out-dir <dir> [--io-map <file>] [--max-p99-exec-us <us>] [--max-overrun-count <n>]"
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

    let plc_sha256 = sha256_hex(&plc_bytes);

    let scenario_yaml = read_scenario_yaml_file(&scenario_path)?;
    let scenario_yaml =
        resolve_scenario_yaml_for_plc(&plc_source, &scenario_yaml).map_err(|e| {
            format_resolve_scenario_yaml_error(&plc_path, &scenario_path, "release-bundle", &e)
        })?;
    let scenario = parse_scenario_yaml(&scenario_yaml)?;

    let ir_bundle = compile_pipeline(&plc_source).map_err(|errors| errors.join("\n\n"))?;

    // Board-oriented program generation uses 1ms ticks to align with firmware build artifacts.
    let board_program =
        state_machine_to_runtime_program(&ir_bundle.topology, &ir_bundle.state_machine, 1)
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

fn run_board_parse_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
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
                return Err(format!(
                    "Usage: {program} board-parse --in <board.log> --out-dir <dir>"
                ));
            }
            other => return Err(format!("Unknown argument for board-parse: {other}")),
        }
    }

    let input = input
        .ok_or_else(|| format!("Usage: {program} board-parse --in <board.log> --out-dir <dir>"))?;
    let out_dir = out_dir
        .ok_or_else(|| format!("Usage: {program} board-parse --in <board.log> --out-dir <dir>"))?;

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

fn run_timing_report_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
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
            "-h" | "--help" => {
                return Err(format!(
                    "Usage: {program} timing-report --in <tick_timing.jsonl> [--out <timing_report.json>]"
                ));
            }
            other => return Err(format!("Unknown argument for timing-report: {other}")),
        }
    }

    let input = input.ok_or_else(|| {
        format!(
            "Usage: {program} timing-report --in <tick_timing.jsonl> [--out <timing_report.json>]"
        )
    })?;

    let out = out.unwrap_or_else(|| {
        input
            .parent()
            .map(|p| p.join("timing_report.json"))
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
            "-h" | "--help" => {
                return Err(format!(
                    "Usage: {program} io-map-normalize --in <io_map.toml> --out <normalized.toml>"
                ));
            }
            other => return Err(format!("Unknown argument for io-map-normalize: {other}")),
        }
    }

    let input = input.ok_or_else(|| {
        format!("Usage: {program} io-map-normalize --in <io_map.toml> --out <normalized.toml>")
    })?;
    let out = out.ok_or_else(|| {
        format!("Usage: {program} io-map-normalize --in <io_map.toml> --out <normalized.toml>")
    })?;

    let text =
        fs::read_to_string(&input).map_err(|err| format!("Failed to read {input:?}: {err}"))?;
    let v: toml::Value = toml::from_str(&text).map_err(|err| format!("TOML parse error: {err}"))?;
    let normalized = normalize_io_map_toml(&v)?;
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

fn normalize_io_map_toml(v: &toml::Value) -> Result<toml::Value, String> {
    use rust_plc::iec_address::{parse_iec_address, LogicalChannelKind};
    use toml::value::Table;

    let root = v
        .as_table()
        .ok_or_else(|| "io_map.toml must be a TOML table at the root".to_string())?;
    let mut out_root: Table = root.clone();

    fn section_table<'a>(root: &'a Table, name: &str) -> Result<&'a Table, String> {
        root.get(name)
            .and_then(|v| v.as_table())
            .ok_or_else(|| format!("Missing or invalid [{name}] (expected a table)"))
    }

    fn opt_section_table<'a>(root: &'a Table, name: &str) -> Result<Option<&'a Table>, String> {
        match root.get(name) {
            None => Ok(None),
            Some(v) => v
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
        t: &Table,
    ) -> Result<Table, String> {
        use std::collections::BTreeMap;
        let mut by_id: BTreeMap<u16, i64> = BTreeMap::new();

        for (k, v) in t.iter() {
            let gpio = parse_gpio_int(section, k, v)?;

            let (kind, id) = if let Some(id) = parse_native_key(k, expected_native_prefix) {
                (expected_kind, id)
            } else if k.trim_start().starts_with('%') {
                let parsed = parse_iec_address(k).map_err(|e| e.to_string())?;
                (parsed.kind, parsed.id)
            } else {
                return Err(format!(
                    "Invalid key {k:?} in [{section}] (expected {expected_native_prefix}<n> or a quoted IEC key like \"%IX0.0\")"
                ));
            };

            if kind != expected_kind {
                return Err(format!(
                    "Invalid key {k:?} in [{section}] (IEC kind {:?} does not match section kind {:?})",
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

fn run_no_board_gate_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let Some(plc_path) = args.next() else {
        return Err(format!(
            "Usage: {program} no-board-gate <file.plc> --scenario <scenario.yaml> --out-dir <dir> [--context <n>] [--sil-scenario <scenario.yaml>] [--board-scenario <scenario.yaml>] [--max-p99-exec-us <us>] [--max-overrun-count <n>] [--output <human|json>]"
        ));
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
                return Err(format!(
                    "Usage: {program} no-board-gate <file.plc> --scenario <scenario.yaml> --out-dir <dir> [--context <n>] [--sil-scenario <scenario.yaml>] [--board-scenario <scenario.yaml>] [--max-p99-exec-us <us>] [--max-overrun-count <n>] [--output <human|json>]"
                ));
            }
            other => return Err(format!("Unknown argument for no-board-gate: {other}")),
        }
    }

    let out_dir = out_dir.ok_or_else(|| {
        format!("Usage: {program} no-board-gate <file.plc> --scenario <scenario.yaml> --out-dir <dir> [--context <n>] [--sil-scenario <scenario.yaml>] [--board-scenario <scenario.yaml>] [--max-p99-exec-us <us>] [--max-overrun-count <n>] [--output <human|json>]")
    })?;

    let sil_scenario_path = sil_scenario_path.or_else(|| scenario_path.clone()).ok_or_else(|| {
        format!("Usage: {program} no-board-gate <file.plc> --scenario <scenario.yaml> --out-dir <dir> [--context <n>] [--sil-scenario <scenario.yaml>] [--board-scenario <scenario.yaml>] [--max-p99-exec-us <us>] [--max-overrun-count <n>] [--output <human|json>]")
    })?;
    let board_scenario_path =
        board_scenario_path.or_else(|| scenario_path.clone()).ok_or_else(|| {
            format!("Usage: {program} no-board-gate <file.plc> --scenario <scenario.yaml> --out-dir <dir> [--context <n>] [--sil-scenario <scenario.yaml>] [--board-scenario <scenario.yaml>] [--max-p99-exec-us <us>] [--max-overrun-count <n>] [--output <human|json>]")
        })?;

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
        }
        let payload = NoBoardGateJson {
            schema_version: 1,
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
    let Some(plc_path) = args.next() else {
        return Err(format!(
            "Usage: {program} virtual-board <file.plc> --scenario <scenario.yaml> --out-dir <dir>"
        ));
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
                return Err(format!(
                    "Usage: {program} virtual-board <file.plc> --scenario <scenario.yaml> --out-dir <dir>"
                ));
            }
            other => return Err(format!("Unknown argument for virtual-board: {other}")),
        }
    }

    let scenario_path = scenario_path.ok_or_else(|| {
        format!(
            "Usage: {program} virtual-board <file.plc> --scenario <scenario.yaml> --out-dir <dir>"
        )
    })?;
    let out_dir = out_dir.ok_or_else(|| {
        format!(
            "Usage: {program} virtual-board <file.plc> --scenario <scenario.yaml> --out-dir <dir>"
        )
    })?;
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
    for pid in program.pid_loops {
        max_ai = Some(max_ai.map_or(pid.pv.0, |m| m.max(pid.pv.0)));
        max_ao = Some(max_ao.map_or(pid.out.0, |m| m.max(pid.out.0)));
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

    let runtime_budget = analyze_runtime_budget(&expanded_program, &state_machine);

    Ok(IrBundle {
        topology,
        state_machine,
        constraints,
        timing_model,
        runtime_budget,
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

    // Edges that may fire within the same tick if inputs match.
    let mut outgoing: Vec<Vec<usize>> = vec![Vec::new(); state_machine.states.len()];
    let mut edges: Vec<(usize, usize)> = Vec::new();
    let mut state_index: HashMap<(String, String), usize> = HashMap::new();
    for (idx, state) in state_machine.states.iter().enumerate() {
        state_index.insert((state.task_name.clone(), state.step_name.clone()), idx);
    }

    for tr in &state_machine.transitions {
        let from = state_index
            .get(&(tr.from.task_name.clone(), tr.from.step_name.clone()))
            .copied();
        let to = state_index
            .get(&(tr.to.task_name.clone(), tr.to.step_name.clone()))
            .copied();
        let (Some(from), Some(to)) = (from, to) else {
            continue;
        };

        if !guard_can_fire_same_tick(&tr.guard) {
            continue;
        }

        let eid = edges.len();
        edges.push((from, to));
        outgoing[from].push(eid);
    }

    let (has_cycle, longest_chain) = analyze_longest_chain(&outgoing, &edges);
    let max_transitions_per_tick_cap = 64;
    let max_transitions_same_tick_upper_bound = if has_cycle {
        max_transitions_per_tick_cap
    } else {
        longest_chain.min(max_transitions_per_tick_cap)
    };

    let max_actions_per_tick_upper_bound = max_actions_per_transition
        .saturating_mul(max_transitions_per_tick_cap)
        .max(max_actions_per_transition);

    let mut budget = RuntimeBudget {
        max_transitions_per_tick_cap,
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
            level: WarningLevel::Warn,
            message: format!(
                "runtime budget: max_actions_per_transition={} exceeds threshold {}",
                budget.max_actions_per_transition, thresholds.max_actions_per_transition
            ),
        });
    }
    if budget.max_actions_per_tick_upper_bound > thresholds.max_actions_per_tick_upper_bound {
        warnings.push(WarningEntry {
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
            level: WarningLevel::Warn,
            message: format!(
                "runtime budget: max_parallel_branches={} exceeds threshold {}",
                budget.max_parallel_branches, thresholds.max_parallel_branches
            ),
        });
    }
    if budget.max_race_branches > thresholds.max_race_branches {
        warnings.push(WarningEntry {
            level: WarningLevel::Warn,
            message: format!(
                "runtime budget: max_race_branches={} exceeds threshold {}",
                budget.max_race_branches, thresholds.max_race_branches
            ),
        });
    }
    if thresholds.warn_on_same_tick_cycle && budget.has_same_tick_cycle {
        warnings.push(WarningEntry {
            level: WarningLevel::Warn,
            message: "runtime budget: same-tick transition subgraph contains a cycle; runtime-core will cap chaining per tick".to_string(),
        });
    }
    if budget.budget_time_estimate.exceeds_budget {
        warnings.push(WarningEntry {
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
