use runtime_core::MAX_TRANSITIONS_PER_TASK_PER_TICK;
use rust_plc::error::PlcError;
use rust_plc::ir::{ConstraintSet, StateMachine, TimingModel, TopologyGraph};
use rust_plc::parser::parse_plc;
use rust_plc::semantic::{
    build_constraint_set, build_state_machine, build_timing_model, build_topology_graph,
    preprocess_program_with_library,
};
use rust_plc::source_bundle::{LoadedPlcSource, remap_plc_error};
use rust_plc::topology_semantic_gate::{
    collect_topology_deprecation_warnings, validate_device_purpose_required,
    validate_removed_legacy_io_model, validate_topology_semantics,
};
use rust_plc::verification::{VerificationSummary, WarningEntry, WarningLevel, verify_all};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use time::format_description::well_known::Rfc3339;

const AXIS_BLOCKING_MIGRATION_WARNING_CODE: &str = "MIG-AXIS-BLOCK-001";

#[derive(Debug, Serialize)]
pub(in crate::cli) struct IrBundle {
    pub(in crate::cli) topology: TopologyGraph,
    pub(in crate::cli) state_machine: StateMachine,
    pub(in crate::cli) constraints: ConstraintSet,
    pub(in crate::cli) timing_model: TimingModel,
    pub(in crate::cli) runtime_budget: RuntimeBudget,
    pub(in crate::cli) verification: VerificationSummary,
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
pub(in crate::cli) enum TransitionBudgetScope {
    PerTaskPerTick,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(in crate::cli) struct RuntimeBudget {
    pub(in crate::cli) transition_budget_scope: TransitionBudgetScope,
    pub(in crate::cli) max_transitions_per_tick_cap: usize,
    pub(in crate::cli) active_task_count: usize,
    pub(in crate::cli) max_transitions_all_tasks_per_tick_upper_bound: usize,
    pub(in crate::cli) max_transitions_same_tick_upper_bound: usize,
    pub(in crate::cli) max_actions_per_transition: usize,
    pub(in crate::cli) max_actions_per_tick_upper_bound: usize,
    pub(in crate::cli) max_parallel_branches: usize,
    pub(in crate::cli) max_race_branches: usize,
    pub(in crate::cli) has_same_tick_cycle: bool,
    pub(in crate::cli) budget_time_estimate: BudgetTimeEstimate,
}

impl RuntimeBudget {
    pub(in crate::cli) fn recompute_time_estimate(
        &mut self,
        action_cost_us: u64,
        transition_cost_us: u64,
        parallel_expand_cost_us: u64,
        max_allowed_us: u64,
    ) {
        self.budget_time_estimate = estimate_budget_time_values(
            self,
            action_cost_us,
            transition_cost_us,
            parallel_expand_cost_us,
            max_allowed_us,
        );
    }

    pub(in crate::cli) fn summary(&self) -> RuntimeBudgetSummary {
        RuntimeBudgetSummary {
            transition_budget_scope: match self.transition_budget_scope {
                TransitionBudgetScope::PerTaskPerTick => "per_task_per_tick",
            },
            max_transitions_per_tick_cap: self.max_transitions_per_tick_cap,
            active_task_count: self.active_task_count,
            max_transitions_all_tasks_per_tick_upper_bound: self
                .max_transitions_all_tasks_per_tick_upper_bound,
            max_transitions_same_tick_upper_bound: self.max_transitions_same_tick_upper_bound,
            max_actions_per_transition: self.max_actions_per_transition,
            max_actions_per_tick_upper_bound: self.max_actions_per_tick_upper_bound,
            max_parallel_branches: self.max_parallel_branches,
            max_race_branches: self.max_race_branches,
            has_same_tick_cycle: self.has_same_tick_cycle,
            budget_time_estimate: RuntimeBudgetTimeSummary {
                action_cost_us: self.budget_time_estimate.action_cost_us,
                transition_cost_us: self.budget_time_estimate.transition_cost_us,
                parallel_expand_cost_us: self.budget_time_estimate.parallel_expand_cost_us,
                action_component_us: self.budget_time_estimate.action_component_us,
                transition_component_us: self.budget_time_estimate.transition_component_us,
                parallel_component_us: self.budget_time_estimate.parallel_component_us,
                total_estimate_us: self.budget_time_estimate.total_estimate_us,
                max_allowed_us: self.budget_time_estimate.max_allowed_us,
                exceeds_budget: self.budget_time_estimate.exceeds_budget,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(in crate::cli) struct BudgetTimeEstimate {
    pub(in crate::cli) action_cost_us: u64,
    pub(in crate::cli) transition_cost_us: u64,
    pub(in crate::cli) parallel_expand_cost_us: u64,
    pub(in crate::cli) action_component_us: u64,
    pub(in crate::cli) transition_component_us: u64,
    pub(in crate::cli) parallel_component_us: u64,
    pub(in crate::cli) total_estimate_us: u64,
    pub(in crate::cli) max_allowed_us: u64,
    pub(in crate::cli) exceeds_budget: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(in crate::cli) struct RuntimeBudgetSummary {
    pub(in crate::cli) transition_budget_scope: &'static str,
    pub(in crate::cli) max_transitions_per_tick_cap: usize,
    pub(in crate::cli) active_task_count: usize,
    pub(in crate::cli) max_transitions_all_tasks_per_tick_upper_bound: usize,
    pub(in crate::cli) max_transitions_same_tick_upper_bound: usize,
    pub(in crate::cli) max_actions_per_transition: usize,
    pub(in crate::cli) max_actions_per_tick_upper_bound: usize,
    pub(in crate::cli) max_parallel_branches: usize,
    pub(in crate::cli) max_race_branches: usize,
    pub(in crate::cli) has_same_tick_cycle: bool,
    pub(in crate::cli) budget_time_estimate: RuntimeBudgetTimeSummary,
}

#[derive(Debug, Clone, Serialize)]
pub(in crate::cli) struct RuntimeBudgetTimeSummary {
    pub(in crate::cli) action_cost_us: u64,
    pub(in crate::cli) transition_cost_us: u64,
    pub(in crate::cli) parallel_expand_cost_us: u64,
    pub(in crate::cli) action_component_us: u64,
    pub(in crate::cli) transition_component_us: u64,
    pub(in crate::cli) parallel_component_us: u64,
    pub(in crate::cli) total_estimate_us: u64,
    pub(in crate::cli) max_allowed_us: u64,
    pub(in crate::cli) exceeds_budget: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeBudgetThresholds {
    action_cost_us: u64,
    transition_cost_us: u64,
    parallel_expand_cost_us: u64,
    max_budget_time_estimate_us: u64,
}

impl Default for RuntimeBudgetThresholds {
    fn default() -> Self {
        Self {
            action_cost_us: 8,
            transition_cost_us: 5,
            parallel_expand_cost_us: 12,
            max_budget_time_estimate_us: 2_000,
        }
    }
}

pub(in crate::cli) fn compile_pipeline(input: &LoadedPlcSource) -> Result<IrBundle, Vec<String>> {
    let program = parse_plc(&input.source)
        .map_err(|err| vec![remap_plc_error(err, &input.source_map).to_string()])?;
    for warning in collect_topology_deprecation_warnings(&program.topology) {
        eprintln!("WARNING [deprecation] {warning}");
    }
    validate_removed_legacy_io_model(&program.topology)
        .map_err(|gate_error| vec![format_topology_gate_error(gate_error, input)])?;
    validate_device_purpose_required(&program.topology)
        .map_err(|gate_error| vec![format_topology_gate_error(gate_error, input)])?;

    let devices_dir = Path::new("devices");
    let device_library = match rust_plc::device_library::DeviceLibrary::load(devices_dir) {
        Ok(lib) => lib,
        Err(errors) => {
            return Err(errors.into_iter().map(|e| e.to_string()).collect());
        }
    };

    let expanded_program = preprocess_program_with_library(
        &program,
        if device_library.is_empty() {
            None
        } else {
            Some(&device_library)
        },
    )
    .map_err(|errors| format_plc_errors(errors, input))?;
    validate_topology_semantics(&expanded_program.topology)
        .map_err(|gate_error| vec![format_topology_gate_error(gate_error, input)])?;

    let mut errors = Vec::new();
    let topology = collect_stage(build_topology_graph(&expanded_program), &mut errors);
    let state_machine = collect_stage(build_state_machine(&expanded_program), &mut errors);
    let constraints = collect_stage(build_constraint_set(&expanded_program), &mut errors);
    let timing_model = collect_stage(build_timing_model(&expanded_program), &mut errors);

    if !errors.is_empty() {
        return Err(format_plc_errors(errors, input));
    }

    let topology = topology.expect("topology exists when semantic errors are empty");
    let state_machine = state_machine.expect("state machine exists when semantic errors are empty");
    let constraints = constraints.expect("constraints exist when semantic errors are empty");
    let timing_model = timing_model.expect("timing model exists when semantic errors are empty");

    let mut verification = verify_all(&expanded_program, &topology, &constraints, &state_machine)
        .map_err(|issues| {
        issues
            .into_iter()
            .map(|issue| issue.to_string())
            .collect::<Vec<_>>()
    })?;
    apply_axis_move_blocking_migration_warning(&program, &mut verification);

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

pub(in crate::cli) fn write_verification_report(
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

fn collect_stage<T>(result: Result<T, Vec<PlcError>>, errors: &mut Vec<PlcError>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(mut stage_errors) => {
            errors.append(&mut stage_errors);
            None
        }
    }
}

fn format_plc_errors(errors: Vec<PlcError>, input: &LoadedPlcSource) -> Vec<String> {
    errors
        .into_iter()
        .map(|error| remap_plc_error(error, &input.source_map).to_string())
        .collect()
}

fn format_topology_gate_error(
    gate_error: rust_plc::topology_semantic_gate::TopologySemanticGateError,
    input: &LoadedPlcSource,
) -> String {
    let mut rendered = format!(
        "ERROR [{}] Topology semantic gate rejected the program\n",
        gate_error.code
    );
    for issue in gate_error.issues {
        if let Some(location) = input.source_map.remap_location(issue.line.max(1), 1) {
            let _ = writeln!(
                rendered,
                "  - [{}] {}:{}:{}",
                issue.code.as_str(),
                location.file,
                location.line.max(1),
                location.column.max(1)
            );
        } else {
            let _ = writeln!(
                rendered,
                "  - [{}] {}:{}:{}",
                issue.code.as_str(),
                input.requested_path.display(),
                issue.line.max(1),
                1
            );
        }
        let _ = writeln!(rendered, "    cause: {}", issue.message);
        let _ = writeln!(rendered, "    suggestion: {}", issue.suggestion);
    }
    rendered.trim_end().to_string()
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

            let edge_id = edges.len();
            edges.push((from, to));
            outgoing[from].push(edge_id);
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
                for branch in &block.branches {
                    analyze_statements_budget_facts(
                        &branch.statements,
                        actions_in_step,
                        max_parallel,
                        max_race,
                    );
                }
            }
            rust_plc::ast::StepStatement::Race(block) => {
                *max_race = (*max_race).max(block.branches.len());
                for branch in &block.branches {
                    analyze_statements_budget_facts(
                        &branch.statements,
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
    let node_count = outgoing.len();
    let mut visiting = vec![false; node_count];
    let mut visited = vec![false; node_count];
    let mut memo = vec![0usize; node_count];
    let mut has_cycle = false;

    fn dfs(
        node: usize,
        outgoing: &[Vec<usize>],
        edges: &[(usize, usize)],
        visiting: &mut [bool],
        visited: &mut [bool],
        memo: &mut [usize],
        has_cycle: &mut bool,
    ) -> usize {
        if visiting[node] {
            *has_cycle = true;
            return 0;
        }
        if visited[node] {
            return memo[node];
        }
        visiting[node] = true;
        let mut best = 0usize;
        for &edge_id in &outgoing[node] {
            let (_from, to) = edges[edge_id];
            let candidate =
                1usize.saturating_add(dfs(to, outgoing, edges, visiting, visited, memo, has_cycle));
            best = best.max(candidate);
        }
        visiting[node] = false;
        visited[node] = true;
        memo[node] = best;
        best
    }

    let mut longest = 0usize;
    for node in 0..node_count {
        longest = longest.max(dfs(
            node,
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
        format!("{preview} and {} more step(s)", overflow)
    } else {
        format!("{preview} ({} step(s) total)", impacted_steps.len())
    };

    verification.liveness.warnings.push(WarningEntry {
        code: Some(AXIS_BLOCKING_MIGRATION_WARNING_CODE.to_string()),
        level: WarningLevel::Warn,
        message: format!(
            "migration notice: axis.move_* now executes with default blocking semantics. Detected {scope} mixing axis.move_* with other statements inside one step; runtime ordering may differ from older non-blocking assumptions."
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
    estimate_budget_time_values(
        budget,
        thresholds.action_cost_us,
        thresholds.transition_cost_us,
        thresholds.parallel_expand_cost_us,
        thresholds.max_budget_time_estimate_us,
    )
}

fn estimate_budget_time_values(
    budget: &RuntimeBudget,
    action_cost_us: u64,
    transition_cost_us: u64,
    parallel_expand_cost_us: u64,
    max_allowed_us: u64,
) -> BudgetTimeEstimate {
    let action_component_us =
        (budget.max_actions_per_tick_upper_bound as u64).saturating_mul(action_cost_us);
    let transition_component_us =
        (budget.max_transitions_same_tick_upper_bound as u64).saturating_mul(transition_cost_us);
    let parallel_expansion = budget
        .max_parallel_branches
        .saturating_sub(1)
        .saturating_add(budget.max_race_branches.saturating_sub(1))
        as u64;
    let parallel_component_us = parallel_expansion.saturating_mul(parallel_expand_cost_us);
    let total_estimate_us = action_component_us
        .saturating_add(transition_component_us)
        .saturating_add(parallel_component_us);

    BudgetTimeEstimate {
        action_cost_us,
        transition_cost_us,
        parallel_expand_cost_us,
        action_component_us,
        transition_component_us,
        parallel_component_us,
        total_estimate_us,
        max_allowed_us,
        exceeds_budget: total_estimate_us > max_allowed_us,
    }
}
