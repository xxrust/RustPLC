use crate::ast::{
    ActionStatement, ComparisonOperator, ConditionExpression, DeviceType, EffectKind, Expression,
    LiteralValue, PlcProgram, StepStatement, TaskDeclaration, TasksSection, WaitCondition,
};
use crate::source_bundle::is_bundle_path;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const PROJECT_MANIFEST_FILE: &str = "rustplc.project.toml";
const DEFAULT_STATE_PROOF_CONFIG_FILE: &str = "config/state_proof.toml";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StateProofSeverity {
    Error,
    Warning,
}

impl StateProofSeverity {
    pub fn is_error(self) -> bool {
        matches!(self, Self::Error)
    }

    fn rank(self) -> u8 {
        match self {
            Self::Error => 0,
            Self::Warning => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StateProofStatus {
    Pass,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateProofIssue {
    pub code: String,
    pub severity: StateProofSeverity,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    pub message: String,
    pub fix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NoFeedbackStepException {
    pub task: String,
    pub step: String,
    pub reason: String,
    pub proof_basis: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustedInitialStateException {
    pub symbol: String,
    pub reason: String,
    pub proof_basis: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelfCheckDeviceException {
    pub device: String,
    pub reason: String,
    pub proof_basis: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StateProofConfig {
    pub no_feedback_steps: Vec<NoFeedbackStepException>,
    pub trusted_initial_state: Vec<TrustedInitialStateException>,
    pub self_check_exempt_devices: Vec<SelfCheckDeviceException>,
}

impl StateProofConfig {
    pub fn no_feedback_matches(&self, task: &str, step: &str) -> bool {
        self.no_feedback_steps
            .iter()
            .any(|entry| entry.task == task && entry.step == step)
    }

    pub fn trusts_symbol(&self, symbol: &str) -> bool {
        self.trusted_initial_state
            .iter()
            .any(|entry| entry.symbol.eq_ignore_ascii_case(symbol))
    }

    pub fn exempts_self_check(&self, device: &str) -> bool {
        self.self_check_exempt_devices
            .iter()
            .any(|entry| entry.device.eq_ignore_ascii_case(device))
    }
}

#[derive(Debug, Clone)]
pub struct LoadedStateProofConfig {
    pub path: Option<PathBuf>,
    pub config: StateProofConfig,
}

#[derive(Debug, Deserialize)]
struct StateProofConfigFile {
    #[serde(default = "default_state_proof_schema_version")]
    schema_version: u32,
    #[serde(default)]
    no_feedback_steps: Vec<NoFeedbackStepException>,
    #[serde(default)]
    trusted_initial_state: Vec<TrustedInitialStateException>,
    #[serde(default)]
    self_check_exempt_devices: Vec<SelfCheckDeviceException>,
}

fn default_state_proof_schema_version() -> u32 {
    1
}

#[derive(Debug, Clone)]
struct WaitUse {
    line: usize,
    task: String,
    step: String,
}

#[derive(Debug, Clone)]
struct AssignmentEvidence {
    var_refs: Vec<String>,
    direct_physical_ref: bool,
    prior_workpiece_effect: bool,
    no_feedback_override: bool,
    init_like_context: bool,
}

pub fn default_state_proof_config_path(source_path: &Path) -> Option<PathBuf> {
    if let Some(project_root) = find_project_root(source_path) {
        return Some(project_root.join(DEFAULT_STATE_PROOF_CONFIG_FILE));
    }

    let parent = source_path.parent()?;
    if source_path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("plc"))
        && parent.file_name().and_then(|name| name.to_str()) == Some("plc")
    {
        return parent
            .parent()
            .map(|root| root.join(DEFAULT_STATE_PROOF_CONFIG_FILE));
    }

    Some(parent.join(DEFAULT_STATE_PROOF_CONFIG_FILE))
}

pub fn load_state_proof_config(
    source_path: &Path,
    explicit_path: Option<&Path>,
) -> Result<LoadedStateProofConfig, String> {
    let resolved_path = match explicit_path {
        Some(path) => Some(path.to_path_buf()),
        None => default_state_proof_config_path(source_path).filter(|path| path.is_file()),
    };

    let Some(path) = resolved_path else {
        return Ok(LoadedStateProofConfig {
            path: None,
            config: StateProofConfig::default(),
        });
    };

    if !path.is_file() {
        return Err(format!(
            "state-proof-check config file not found: {}",
            path.display()
        ));
    }

    Ok(LoadedStateProofConfig {
        config: parse_state_proof_config(&path)?,
        path: Some(path),
    })
}

pub fn should_auto_run_state_proof_check(program: &PlcProgram, source_path: &Path) -> bool {
    if is_bundle_path(source_path) {
        return true;
    }
    if !program.topology.variables.is_empty() {
        return true;
    }
    has_workpiece_assets(program) || tasks_use_workpiece_effects(&program.tasks)
}

pub fn analyze_program(program: &PlcProgram, config: &StateProofConfig) -> Vec<StateProofIssue> {
    let variable_names = program
        .topology
        .variables
        .iter()
        .map(|variable| variable.name.clone())
        .collect::<HashSet<_>>();
    let variables = program
        .topology
        .variables
        .iter()
        .map(|variable| (variable.name.clone(), variable))
        .collect::<HashMap<_, _>>();
    let physical_symbols = collect_physical_symbols(program);
    let wait_uses = collect_wait_uses(program, &variable_names);
    let assignments = collect_assignments(program, &variable_names, &physical_symbols, config);
    let ingress_sites = collect_ingress_sites(program);
    let critical_residual_symbols = collect_critical_residual_symbols(program);
    let action_targets = collect_action_targets(program);

    let mut proof_cache = HashMap::<String, bool>::new();
    let mut startup_proof_cache = HashMap::<String, bool>::new();
    let mut issues = Vec::new();

    issues.extend(find_uncommanded_home_waits(program));
    issues.extend(find_unbounded_local_sensor_waits(program));
    issues.extend(find_unclosed_residual_manual_routes(program));
    issues.extend(find_unproven_vacuum_releases(program));
    issues.extend(find_missing_self_checks(program, config, &action_targets));

    for (variable_name, declaration) in &variables {
        let Some(uses) = wait_uses.get(variable_name) else {
            continue;
        };
        if uses.is_empty() || !looks_like_physical_flag(variable_name) {
            continue;
        }

        if is_initial_true_bool(declaration)
            && !variable_has_startup_or_initial_proof(
                variable_name,
                config,
                &assignments,
                &mut proof_cache,
                &mut startup_proof_cache,
            )
        {
            let first_use = &uses[0];
            issues.push(StateProofIssue {
                code: "SPF-001".to_string(),
                severity: StateProofSeverity::Error,
                line: declaration.line.max(first_use.line).max(1),
                source_file: None,
                task: Some(first_use.task.clone()),
                step: Some(first_use.step.clone()),
                symbol: Some(variable_name.clone()),
                message: format!(
                    "bool variable `{variable_name}` starts as `true` and is later used to prove a physical/production state without startup proof"
                ),
                fix: "derive this flag from a sensor/operator/workpiece proof, or add `trusted_initial_state` with explicit reason and proof_basis".to_string(),
            });
        } else if !variable_has_any_proof(variable_name, config, &assignments, &mut proof_cache) {
            let first_use = &uses[0];
            issues.push(StateProofIssue {
                code: "SPF-002".to_string(),
                severity: StateProofSeverity::Error,
                line: first_use.line.max(1),
                source_file: None,
                task: Some(first_use.task.clone()),
                step: Some(first_use.step.clone()),
                symbol: Some(variable_name.clone()),
                message: format!(
                    "flag `{variable_name}` is used like a physical readiness/state proof, but its assignment chain contains only constants or internal compute"
                ),
                fix: "feed this flag from physical inputs/workpiece effects, or document an explicit `no_feedback_steps` / `trusted_initial_state` exception".to_string(),
            });
        }

        if looks_like_ingress_stock_flag(variable_name, &ingress_sites)
            && !variable_has_any_proof(variable_name, config, &assignments, &mut proof_cache)
        {
            let first_use = &uses[0];
            issues.push(StateProofIssue {
                code: "SPF-003".to_string(),
                severity: StateProofSeverity::Error,
                line: first_use.line.max(1),
                source_file: None,
                task: Some(first_use.task.clone()),
                step: Some(first_use.step.clone()),
                symbol: Some(variable_name.clone()),
                message: format!(
                    "`{variable_name}` appears to treat `ingress_sites` as stock proof; ingress declarations define possible sources, not confirmed inventory"
                ),
                fix: "prove stock with sensors/operator confirmation/workpiece effects instead of presetting an ingress-backed flag".to_string(),
            });
        }
    }

    if has_workpiece_assets(program) || tasks_use_workpiece_effects(&program.tasks) {
        let has_init_layer = program
            .tasks
            .tasks
            .iter()
            .any(|task| is_init_like(&task.name));
        let trusted_residual_baseline = critical_residual_symbols
            .iter()
            .all(|symbol| config.trusts_symbol(symbol));
        let startup_residue_proof = program
            .tasks
            .tasks
            .iter()
            .filter(|task| is_init_like(&task.name))
            .any(|task| task_has_startup_residue_proof(task, &physical_symbols));

        if !has_init_layer && !trusted_residual_baseline {
            issues.push(StateProofIssue {
                code: "SPF-020".to_string(),
                severity: StateProofSeverity::Error,
                line: first_workpiece_line(program),
                source_file: None,
                task: None,
                step: None,
                symbol: None,
                message: "workpiece project has locations/holders/carriers but no startup/init layer or trusted residual-state declaration".to_string(),
                fix: "add an init/startup task that establishes the empty baseline, or declare a reviewed `trusted_initial_state` exception for each critical residual endpoint".to_string(),
            });
        }

        if first_auto_workpiece_step(program).is_some()
            && !trusted_residual_baseline
            && !startup_residue_proof
        {
            let (line, task, step) = first_auto_workpiece_step(program)
                .expect("checked first_auto_workpiece_step is some");
            issues.push(StateProofIssue {
                code: "SPF-021".to_string(),
                severity: StateProofSeverity::Error,
                line: line.max(1),
                source_file: None,
                task: Some(task),
                step: Some(step),
                symbol: None,
                message: "automatic workpiece flow starts without any proven startup check/cleanup/manual confirmation for residual parts".to_string(),
                fix: "before auto flow, add startup inspection/cleanup/operator confirmation for critical workpiece endpoints, or encode deliberate trusted_initial_state exceptions".to_string(),
            });
        }

        if let Some((line, target_task, target_step)) = find_recovery_jump_to_auto_flow(program) {
            if !trusted_residual_baseline && !startup_residue_proof {
                issues.push(StateProofIssue {
                    code: "SPF-022".to_string(),
                    severity: StateProofSeverity::Error,
                    line: line.max(1),
                    source_file: None,
                    task: Some(target_task),
                    step: Some(target_step),
                    symbol: None,
                    message: "fault/recovery path jumps back into automatic flow without re-establishing a controlled workpiece baseline".to_string(),
                    fix: "route recovery back through init/startup residue handling, or explicitly prove the residual baseline before re-entering auto flow".to_string(),
                });
            }
        }
    }

    issues.sort_by(|left, right| {
        left.severity
            .rank()
            .cmp(&right.severity.rank())
            .then(left.line.cmp(&right.line))
            .then(left.code.cmp(&right.code))
            .then(left.symbol.cmp(&right.symbol))
    });
    issues
}

fn parse_state_proof_config(path: &Path) -> Result<StateProofConfig, String> {
    let text = fs::read_to_string(path)
        .map_err(|err| format!("Failed to read {}: {err}", path.display()))?;
    let parsed: StateProofConfigFile = toml::from_str(&text)
        .map_err(|err| format!("Failed to parse {}: {err}", path.display()))?;

    if parsed.schema_version != 1 {
        return Err(format!(
            "Unsupported state-proof config schema_version {} in {} (expected 1)",
            parsed.schema_version,
            path.display()
        ));
    }

    for entry in &parsed.no_feedback_steps {
        if entry.task.trim().is_empty()
            || entry.step.trim().is_empty()
            || entry.reason.trim().is_empty()
            || entry.proof_basis.trim().is_empty()
        {
            return Err(format!(
                "Invalid no_feedback_steps entry in {}: task, step, reason, and proof_basis are all required",
                path.display()
            ));
        }
    }
    for entry in &parsed.trusted_initial_state {
        if entry.symbol.trim().is_empty()
            || entry.reason.trim().is_empty()
            || entry.proof_basis.trim().is_empty()
        {
            return Err(format!(
                "Invalid trusted_initial_state entry in {}: symbol, reason, and proof_basis are all required",
                path.display()
            ));
        }
    }
    for entry in &parsed.self_check_exempt_devices {
        if entry.device.trim().is_empty()
            || entry.reason.trim().is_empty()
            || entry.proof_basis.trim().is_empty()
        {
            return Err(format!(
                "Invalid self_check_exempt_devices entry in {}: device, reason, and proof_basis are all required",
                path.display()
            ));
        }
    }

    Ok(StateProofConfig {
        no_feedback_steps: parsed.no_feedback_steps,
        trusted_initial_state: parsed.trusted_initial_state,
        self_check_exempt_devices: parsed.self_check_exempt_devices,
    })
}

fn find_uncommanded_home_waits(program: &PlcProgram) -> Vec<StateProofIssue> {
    let mut issues = Vec::new();

    for task in &program.tasks.tasks {
        if !is_init_like(&task.name) {
            continue;
        }

        let mut prior_axis_motion = false;
        for step in &task.steps {
            if step_waits_on_home_sensor(&step.statements) && !prior_axis_motion {
                issues.push(StateProofIssue {
                    code: "SPF-030".to_string(),
                    severity: StateProofSeverity::Error,
                    line: step.line.max(task.line).max(1),
                    source_file: None,
                    task: Some(task.name.clone()),
                    step: Some(step.name.clone()),
                    symbol: None,
                    message: "startup/init task waits for a home sensor before any axis homing or motion command is issued".to_string(),
                    fix: "issue a real axis homing/motion command before waiting for home feedback, or document a trusted initial home proof explicitly".to_string(),
                });
            }
            prior_axis_motion |= step_has_axis_motion(&step.statements);
        }
    }

    issues
}

fn find_unbounded_local_sensor_waits(program: &PlcProgram) -> Vec<StateProofIssue> {
    let mut issues = Vec::new();

    for task in &program.tasks.tasks {
        for step in &task.steps {
            if !step_allows_indefinite_wait(&step.statements) {
                continue;
            }
            let Some(symbol) = first_local_controlled_wait_symbol(&step.statements) else {
                continue;
            };
            issues.push(StateProofIssue {
                code: "SPF-031".to_string(),
                severity: StateProofSeverity::Error,
                line: step.line.max(task.line).max(1),
                source_file: None,
                task: Some(task.name.clone()),
                step: Some(step.name.clone()),
                symbol: Some(symbol.clone()),
                message: format!(
                    "step uses allow_indefinite_wait while waiting on local controlled feedback `{symbol}`"
                ),
                fix: "only use allow_indefinite_wait for uncontrolled external actors or upstream tasks; local sensors need timeout, recovery, or explicit fault routing".to_string(),
            });
        }
    }

    issues
}

fn find_unclosed_residual_manual_routes(program: &PlcProgram) -> Vec<StateProofIssue> {
    let mut issues = Vec::new();

    for task in &program.tasks.tasks {
        if !is_init_like(&task.name) {
            continue;
        }

        for step in &task.steps {
            if !looks_like_residual_check_step(&step.name, &step.statements) {
                continue;
            }
            if step_has_recovery_action_or_effect(&step.statements) {
                continue;
            }

            let Some(target) = first_timeout_target(&step.statements) else {
                continue;
            };
            if route_is_explicit_manual_or_recovery(target) {
                continue;
            }

            issues.push(StateProofIssue {
                code: "SPF-032".to_string(),
                severity: StateProofSeverity::Error,
                line: step.line.max(task.line).max(1),
                source_file: None,
                task: Some(task.name.clone()),
                step: Some(step.name.clone()),
                symbol: None,
                message: "residual/workpiece baseline check routes failure to a generic fault without explicit manual-assist or automatic recovery semantics".to_string(),
                fix: "for recoverable residue, add a recovery path; if human handling is required, route to an explicitly named manual/operator-assist task and document the boundary".to_string(),
            });
        }
    }

    issues
}

fn find_unproven_vacuum_releases(program: &PlcProgram) -> Vec<StateProofIssue> {
    let holder_names = program
        .topology
        .workpiece_holders
        .iter()
        .map(|holder| holder.name.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    if holder_names.is_empty() {
        return Vec::new();
    }

    let mut device_types = HashMap::<String, DeviceType>::new();
    let mut device_purposes = HashMap::<String, String>::new();
    for device in &program.topology.devices {
        device_types.insert(device.name.to_ascii_lowercase(), device.device_type.clone());
        device_purposes.insert(
            device.name.to_ascii_lowercase(),
            device
                .attributes
                .purpose
                .clone()
                .unwrap_or_default()
                .to_ascii_lowercase(),
        );
    }

    let mut issues = Vec::new();
    for task in &program.tasks.tasks {
        for step in &task.steps {
            if !step_turns_vacuum_off(&step.statements, &device_types, &device_purposes) {
                continue;
            }
            let Some((from, to)) =
                first_holder_to_non_holder_transfer(&step.statements, &holder_names)
            else {
                continue;
            };
            issues.push(StateProofIssue {
                code: "SPF-033".to_string(),
                severity: StateProofSeverity::Error,
                line: step.line.max(task.line).max(1),
                source_file: None,
                task: Some(task.name.clone()),
                step: Some(step.name.clone()),
                symbol: Some(from.clone()),
                message: format!(
                    "vacuum is released while transferring workpiece from holder `{from}` to non-holder `{to}` without receiver ownership proof"
                ),
                fix: "keep vacuum until a receiving holder/stage has proven ownership, or model a passive support/receiver proof before releasing the holder".to_string(),
            });
        }
    }

    issues
}

fn find_missing_self_checks(
    program: &PlcProgram,
    config: &StateProofConfig,
    action_targets: &HashSet<String>,
) -> Vec<StateProofIssue> {
    let mut issues = Vec::new();

    for device in &program.topology.devices {
        if !action_targets.contains(&device.name.to_ascii_lowercase()) {
            continue;
        }
        if !device_requires_self_check(&device.device_type) {
            continue;
        }
        if config.exempts_self_check(&device.name) {
            continue;
        }
        if program_has_self_check_for_device(program, &device.name) {
            continue;
        }

        issues.push(StateProofIssue {
            code: "SPF-040".to_string(),
            severity: StateProofSeverity::Error,
            line: device.line.max(1),
            source_file: None,
            task: None,
            step: None,
            symbol: Some(device.name.clone()),
            message: format!(
                "actuated device `{}` is used by tasks but has no maintenance/self-check path",
                device.name
            ),
            fix: "add a maintenance/self-check task that exercises the device and proves feedback, or add self_check_exempt_devices with reason and proof_basis".to_string(),
        });
    }

    issues
}

fn variable_has_any_proof(
    variable_name: &str,
    config: &StateProofConfig,
    assignments: &HashMap<String, Vec<AssignmentEvidence>>,
    cache: &mut HashMap<String, bool>,
) -> bool {
    let mut stack = HashSet::new();
    variable_has_any_proof_inner(variable_name, config, assignments, cache, &mut stack)
}

fn variable_has_any_proof_inner(
    variable_name: &str,
    config: &StateProofConfig,
    assignments: &HashMap<String, Vec<AssignmentEvidence>>,
    cache: &mut HashMap<String, bool>,
    stack: &mut HashSet<String>,
) -> bool {
    if config.trusts_symbol(variable_name) {
        cache.insert(variable_name.to_string(), true);
        return true;
    }

    if let Some(cached) = cache.get(variable_name) {
        return *cached;
    }
    if !stack.insert(variable_name.to_string()) {
        return false;
    }

    let proven = assignments.get(variable_name).is_some_and(|items| {
        items.iter().any(|item| {
            let refs_are_proven = item.var_refs.iter().all(|dependency| {
                variable_has_any_proof_inner(dependency, config, assignments, cache, stack)
            });
            let has_anchor = item.direct_physical_ref
                || item.prior_workpiece_effect
                || item.no_feedback_override;
            if item.var_refs.is_empty() {
                return has_anchor;
            }
            refs_are_proven && (has_anchor || !item.var_refs.is_empty())
        })
    });

    stack.remove(variable_name);
    cache.insert(variable_name.to_string(), proven);
    proven
}

fn variable_has_startup_or_initial_proof(
    variable_name: &str,
    config: &StateProofConfig,
    assignments: &HashMap<String, Vec<AssignmentEvidence>>,
    proof_cache: &mut HashMap<String, bool>,
    startup_cache: &mut HashMap<String, bool>,
) -> bool {
    if config.trusts_symbol(variable_name) {
        startup_cache.insert(variable_name.to_string(), true);
        return true;
    }
    if let Some(cached) = startup_cache.get(variable_name) {
        return *cached;
    }

    let proven = assignments.get(variable_name).is_some_and(|items| {
        items.iter().any(|item| {
            item.init_like_context
                && (item.direct_physical_ref
                    || item.prior_workpiece_effect
                    || item.no_feedback_override
                    || item.var_refs.iter().all(|dependency| {
                        variable_has_any_proof(dependency, config, assignments, proof_cache)
                    }))
        })
    });
    startup_cache.insert(variable_name.to_string(), proven);
    proven
}

fn collect_wait_uses(
    program: &PlcProgram,
    variable_names: &HashSet<String>,
) -> HashMap<String, Vec<WaitUse>> {
    let mut uses = HashMap::<String, Vec<WaitUse>>::new();
    for task in &program.tasks.tasks {
        for step in &task.steps {
            collect_wait_uses_in_statements(
                &step.statements,
                variable_names,
                step.line.max(task.line).max(1),
                &task.name,
                &step.name,
                &mut uses,
            );
        }
    }
    uses
}

fn collect_wait_uses_in_statements(
    statements: &[StepStatement],
    variable_names: &HashSet<String>,
    line: usize,
    task: &str,
    step: &str,
    uses: &mut HashMap<String, Vec<WaitUse>>,
) {
    for statement in statements {
        match statement {
            StepStatement::Wait(wait) => match &wait.condition {
                WaitCondition::Single(condition) => {
                    if let Some(variable) = condition_waits_on_variable(condition, variable_names) {
                        uses.entry(variable).or_default().push(WaitUse {
                            line,
                            task: task.to_string(),
                            step: step.to_string(),
                        });
                    }
                }
                WaitCondition::And(conditions) | WaitCondition::Or(conditions) => {
                    for condition in conditions {
                        if let Some(variable) =
                            condition_waits_on_variable(condition, variable_names)
                        {
                            uses.entry(variable).or_default().push(WaitUse {
                                line,
                                task: task.to_string(),
                                step: step.to_string(),
                            });
                        }
                    }
                }
                WaitCondition::Edge(edge) => {
                    if variable_names.contains(&edge.operand) {
                        uses.entry(edge.operand.clone()).or_default().push(WaitUse {
                            line,
                            task: task.to_string(),
                            step: step.to_string(),
                        });
                    }
                }
            },
            StepStatement::Repeat { body, .. } => {
                collect_wait_uses_in_statements(body, variable_names, line, task, step, uses);
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    collect_wait_uses_in_statements(
                        &branch.statements,
                        variable_names,
                        line,
                        task,
                        step,
                        uses,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    collect_wait_uses_in_statements(
                        &branch.statements,
                        variable_names,
                        line,
                        task,
                        step,
                        uses,
                    );
                }
            }
            StepStatement::Action(_)
            | StepStatement::Effect(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
}

fn condition_waits_on_variable(
    condition: &ConditionExpression,
    variable_names: &HashSet<String>,
) -> Option<String> {
    if let Some((left, right)) = condition.expression_pair() {
        return expression_bool_wait_variable(left, right, variable_names)
            .or_else(|| expression_bool_wait_variable(right, left, variable_names));
    }

    if !variable_names.contains(&condition.left) {
        return None;
    }
    match (&condition.operator, &condition.right) {
        (ComparisonOperator::Eq, LiteralValue::Boolean(_))
        | (ComparisonOperator::Neq, LiteralValue::Boolean(_)) => Some(condition.left.clone()),
        _ => None,
    }
}

fn expression_bool_wait_variable(
    variable_expr: &Expression,
    boolean_expr: &Expression,
    variable_names: &HashSet<String>,
) -> Option<String> {
    match (variable_expr, boolean_expr) {
        (Expression::Variable(name), Expression::Boolean(_)) if variable_names.contains(name) => {
            Some(name.clone())
        }
        _ => None,
    }
}

fn collect_assignments(
    program: &PlcProgram,
    variable_names: &HashSet<String>,
    physical_symbols: &HashSet<String>,
    config: &StateProofConfig,
) -> HashMap<String, Vec<AssignmentEvidence>> {
    let mut assignments = HashMap::<String, Vec<AssignmentEvidence>>::new();
    for task in &program.tasks.tasks {
        let init_like_context = is_init_like(&task.name);
        let mut task_scan_state = StepScanState::default();
        for step in &task.steps {
            task_scan_state = collect_assignments_in_statements(
                &step.statements,
                variable_names,
                physical_symbols,
                config,
                &task.name,
                &step.name,
                step.line.max(task.line).max(1),
                init_like_context,
                task_scan_state,
                &mut assignments,
            );
        }
    }
    assignments
}

#[derive(Debug, Clone, Copy, Default)]
struct StepScanState {
    prior_physical_action: bool,
    prior_physical_wait: bool,
    prior_workpiece_effect: bool,
}

fn collect_assignments_in_statements(
    statements: &[StepStatement],
    variable_names: &HashSet<String>,
    physical_symbols: &HashSet<String>,
    config: &StateProofConfig,
    task: &str,
    step: &str,
    line: usize,
    init_like_context: bool,
    mut scan_state: StepScanState,
    assignments: &mut HashMap<String, Vec<AssignmentEvidence>>,
) -> StepScanState {
    for statement in statements {
        match statement {
            StepStatement::Action(action) => match action {
                ActionStatement::Compute { target, expr } if variable_names.contains(target) => {
                    let (var_refs, direct_physical_ref) =
                        collect_expression_dependencies(expr, variable_names, physical_symbols);
                    let evidence = AssignmentEvidence {
                        var_refs,
                        direct_physical_ref: direct_physical_ref || scan_state.prior_physical_wait,
                        prior_workpiece_effect: scan_state.prior_workpiece_effect,
                        no_feedback_override: config.no_feedback_matches(task, step)
                            && scan_state.prior_physical_action,
                        init_like_context,
                    };
                    assignments
                        .entry(target.clone())
                        .or_default()
                        .push(evidence);
                }
                other => {
                    if is_physical_action(other) {
                        scan_state.prior_physical_action = true;
                    }
                }
            },
            StepStatement::Effect(_) => {
                scan_state.prior_workpiece_effect = true;
            }
            StepStatement::Wait(wait) => {
                if wait_condition_has_physical_operand(&wait.condition, physical_symbols) {
                    scan_state.prior_physical_wait = true;
                }
            }
            StepStatement::Repeat { body, .. } => {
                let nested_state = collect_assignments_in_statements(
                    body,
                    variable_names,
                    physical_symbols,
                    config,
                    task,
                    step,
                    line,
                    init_like_context,
                    scan_state,
                    assignments,
                );
                scan_state.prior_physical_action |= nested_state.prior_physical_action;
                scan_state.prior_physical_wait |= nested_state.prior_physical_wait;
                scan_state.prior_workpiece_effect |= nested_state.prior_workpiece_effect;
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    let nested_state = collect_assignments_in_statements(
                        &branch.statements,
                        variable_names,
                        physical_symbols,
                        config,
                        task,
                        step,
                        line,
                        init_like_context,
                        scan_state,
                        assignments,
                    );
                    scan_state.prior_physical_action |= nested_state.prior_physical_action;
                    scan_state.prior_physical_wait |= nested_state.prior_physical_wait;
                    scan_state.prior_workpiece_effect |= nested_state.prior_workpiece_effect;
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    let nested_state = collect_assignments_in_statements(
                        &branch.statements,
                        variable_names,
                        physical_symbols,
                        config,
                        task,
                        step,
                        line,
                        init_like_context,
                        scan_state,
                        assignments,
                    );
                    scan_state.prior_physical_action |= nested_state.prior_physical_action;
                    scan_state.prior_physical_wait |= nested_state.prior_physical_wait;
                    scan_state.prior_workpiece_effect |= nested_state.prior_workpiece_effect;
                }
            }
            StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
    scan_state
}

fn collect_expression_dependencies(
    expr: &Expression,
    variable_names: &HashSet<String>,
    physical_symbols: &HashSet<String>,
) -> (Vec<String>, bool) {
    let mut var_refs = HashSet::<String>::new();
    let mut direct_physical_ref = false;
    collect_expression_dependencies_inner(
        expr,
        variable_names,
        physical_symbols,
        &mut var_refs,
        &mut direct_physical_ref,
    );
    (
        var_refs.into_iter().collect::<Vec<_>>(),
        direct_physical_ref,
    )
}

fn collect_expression_dependencies_inner(
    expr: &Expression,
    variable_names: &HashSet<String>,
    physical_symbols: &HashSet<String>,
    var_refs: &mut HashSet<String>,
    direct_physical_ref: &mut bool,
) {
    match expr {
        Expression::Variable(name) => {
            if variable_names.contains(name) {
                var_refs.insert(name.clone());
            } else if symbol_is_physical(name, physical_symbols) {
                *direct_physical_ref = true;
            }
        }
        Expression::UnaryNeg(inner) | Expression::UnaryNot(inner) => {
            collect_expression_dependencies_inner(
                inner,
                variable_names,
                physical_symbols,
                var_refs,
                direct_physical_ref,
            );
        }
        Expression::BinaryOp { left, right, .. } => {
            collect_expression_dependencies_inner(
                left,
                variable_names,
                physical_symbols,
                var_refs,
                direct_physical_ref,
            );
            collect_expression_dependencies_inner(
                right,
                variable_names,
                physical_symbols,
                var_refs,
                direct_physical_ref,
            );
        }
        Expression::FunctionCall { args, .. } => {
            for arg in args {
                collect_expression_dependencies_inner(
                    arg,
                    variable_names,
                    physical_symbols,
                    var_refs,
                    direct_physical_ref,
                );
            }
        }
        Expression::Literal(_) | Expression::Boolean(_) => {}
    }
}

fn collect_physical_symbols(program: &PlcProgram) -> HashSet<String> {
    let mut physical = HashSet::<String>::new();

    for controller in &program.topology.controller_io {
        for alias in &controller.aliases {
            if alias.direction == crate::ast::ControllerIoDirection::Input {
                physical.insert(alias.alias.to_ascii_lowercase());
                physical.insert(
                    format!("{}.{}", controller.controller, alias.alias).to_ascii_lowercase(),
                );
                physical.insert(alias.port.to_ascii_lowercase());
                physical.insert(
                    format!("{}.{}", controller.controller, alias.port).to_ascii_lowercase(),
                );
            }
        }
    }

    for connection in &program.topology.connections {
        if connection.relation == crate::ast::TopologyRelation::ReportsTo {
            if let Some(port) = &connection.to_port {
                physical.insert(port.to_ascii_lowercase());
                physical.insert(format!("{}.{}", connection.to, port).to_ascii_lowercase());
            }
        }
    }

    for device in &program.topology.devices {
        if device_is_physical_signal_source(&device.device_type) {
            physical.insert(device.name.to_ascii_lowercase());
        }
    }

    physical
}

fn device_is_physical_signal_source(device_type: &DeviceType) -> bool {
    !matches!(
        device_type,
        DeviceType::Plc
            | DeviceType::DigitalOutput
            | DeviceType::AnalogOutput
            | DeviceType::SolenoidValve
            | DeviceType::Pid
    )
}

fn symbol_is_physical(symbol: &str, physical_symbols: &HashSet<String>) -> bool {
    physical_symbols.contains(&symbol.to_ascii_lowercase())
        || symbol.split_once('.').is_some_and(|(device, port)| {
            physical_symbols.contains(&format!("{}.{}", device, port).to_ascii_lowercase())
                || (physical_symbols.contains(&device.to_ascii_lowercase())
                    && !looks_like_output_port(port))
        })
}

fn looks_like_output_port(port: &str) -> bool {
    let lower = port.to_ascii_lowercase();
    lower.starts_with('y')
        || lower.starts_with("ao")
        || lower == "coil"
        || lower == "cmd"
        || lower == "command"
}

fn is_physical_action(action: &ActionStatement) -> bool {
    !matches!(
        action,
        ActionStatement::Compute { .. } | ActionStatement::Log { .. }
    )
}

fn is_initial_true_bool(variable: &crate::ast::VariableDeclaration) -> bool {
    variable.var_type == crate::ast::VariableType::Bool
        && variable.initial_value.trim().eq_ignore_ascii_case("true")
}

fn looks_like_physical_flag(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "_has_seed",
        "_ready",
        "_done",
        "_available",
        "_present",
        "_loaded",
        "_empty",
        "_clear",
        "_homed",
        "_home",
    ]
    .iter()
    .any(|suffix| lower.ends_with(suffix))
}

fn looks_like_ingress_stock_flag(name: &str, ingress_sites: &HashSet<String>) -> bool {
    let lower = name.to_ascii_lowercase();
    looks_like_physical_flag(name)
        && ingress_sites
            .iter()
            .any(|site| lower.contains(site) || site.contains(&lower))
}

fn collect_ingress_sites(program: &PlcProgram) -> HashSet<String> {
    program
        .topology
        .workpiece_types
        .iter()
        .flat_map(|workpiece| workpiece.ingress_sites.iter())
        .map(|site| site.to_ascii_lowercase())
        .collect()
}

fn collect_critical_residual_symbols(program: &PlcProgram) -> HashSet<String> {
    let ingress_sites = program
        .topology
        .workpiece_types
        .iter()
        .flat_map(|workpiece| workpiece.ingress_sites.iter())
        .map(|site| site.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let abnormal_egress_sites = program
        .topology
        .workpiece_types
        .iter()
        .flat_map(|workpiece| workpiece.abnormal_egress_sites.iter())
        .map(|site| site.to_ascii_lowercase())
        .collect::<HashSet<_>>();

    let mut symbols = HashSet::<String>::new();
    for site in &program.topology.workpiece_sites {
        let lower = site.name.to_ascii_lowercase();
        if !ingress_sites.contains(&lower) && !abnormal_egress_sites.contains(&lower) {
            symbols.insert(lower);
        }
    }
    for holder in &program.topology.workpiece_holders {
        symbols.insert(holder.name.to_ascii_lowercase());
    }
    for carrier in &program.topology.workpiece_carriers {
        symbols.insert(carrier.name.to_ascii_lowercase());
    }
    symbols
}

fn first_auto_workpiece_step(program: &PlcProgram) -> Option<(usize, String, String)> {
    for task in &program.tasks.tasks {
        if is_non_auto_task(&task.name) {
            continue;
        }
        for step in &task.steps {
            if step_contains_workpiece_effects(&step.statements) {
                return Some((
                    step.line.max(task.line).max(1),
                    task.name.clone(),
                    step.name.clone(),
                ));
            }
        }
    }
    None
}

fn first_workpiece_line(program: &PlcProgram) -> usize {
    program
        .topology
        .workpiece_types
        .first()
        .map(|item| item.line.max(1))
        .or_else(|| {
            program
                .topology
                .workpiece_sites
                .first()
                .map(|item| item.line.max(1))
        })
        .or_else(|| {
            program
                .topology
                .workpiece_holders
                .first()
                .map(|item| item.line.max(1))
        })
        .or_else(|| {
            program
                .topology
                .workpiece_carriers
                .first()
                .map(|item| item.line.max(1))
        })
        .unwrap_or(1)
}

fn find_recovery_jump_to_auto_flow(program: &PlcProgram) -> Option<(usize, String, String)> {
    for task in &program.tasks.tasks {
        if !is_recovery_like(&task.name) {
            continue;
        }

        if let Some(on_complete) = &task.on_complete {
            if let crate::ast::OnCompleteDirective::Goto { target } = on_complete {
                if is_auto_task_name(&target.task) {
                    return Some((
                        task.on_complete_line.unwrap_or(task.line).max(1),
                        target.task.clone(),
                        target.step.clone().unwrap_or_else(|| "<entry>".to_string()),
                    ));
                }
            }
        }

        let mut targets = Vec::<(usize, &crate::ast::GotoDirective)>::new();
        for step in &task.steps {
            collect_goto_targets(
                &step.statements,
                step.line.max(task.line).max(1),
                &mut targets,
            );
        }
        for (line, target) in targets {
            if is_auto_task_name(&target.task) {
                return Some((
                    line.max(1),
                    target.task.clone(),
                    target.step.clone().unwrap_or_else(|| "<entry>".to_string()),
                ));
            }
        }
    }
    None
}

fn collect_goto_targets<'a>(
    statements: &'a [StepStatement],
    line: usize,
    targets: &mut Vec<(usize, &'a crate::ast::GotoDirective)>,
) {
    for statement in statements {
        match statement {
            StepStatement::IfElse {
                then_goto,
                else_goto,
                ..
            } => {
                targets.push((then_goto.line.max(line), then_goto));
                targets.push((else_goto.line.max(line), else_goto));
            }
            StepStatement::Timeout(timeout) => {
                targets.push((timeout.target.line.max(line), &timeout.target));
            }
            StepStatement::Goto(goto) => {
                targets.push((goto.line.max(line), goto));
            }
            StepStatement::Repeat { body, .. } => collect_goto_targets(body, line, targets),
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    collect_goto_targets(&branch.statements, line, targets);
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    collect_goto_targets(&branch.statements, line, targets);
                    if let Some(target) = &branch.then_goto {
                        targets.push((target.line.max(line), target));
                    }
                }
            }
            StepStatement::Action(_)
            | StepStatement::Effect(_)
            | StepStatement::Wait(_)
            | StepStatement::Delay { .. }
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
}

fn has_workpiece_assets(program: &PlcProgram) -> bool {
    !program.topology.workpiece_types.is_empty()
        || !program.topology.workpiece_sites.is_empty()
        || !program.topology.workpiece_holders.is_empty()
        || !program.topology.workpiece_carriers.is_empty()
}

fn tasks_use_workpiece_effects(tasks: &TasksSection) -> bool {
    tasks.tasks.iter().any(|task| {
        task.steps
            .iter()
            .any(|step| step_contains_workpiece_effects(&step.statements))
    })
}

fn step_contains_workpiece_effects(statements: &[StepStatement]) -> bool {
    statements.iter().any(|statement| match statement {
        StepStatement::Effect(_) => true,
        StepStatement::Repeat { body, .. } => step_contains_workpiece_effects(body),
        StepStatement::Parallel(block) => block
            .branches
            .iter()
            .any(|branch| step_contains_workpiece_effects(&branch.statements)),
        StepStatement::Race(block) => block
            .branches
            .iter()
            .any(|branch| step_contains_workpiece_effects(&branch.statements)),
        StepStatement::Action(_)
        | StepStatement::Wait(_)
        | StepStatement::IfElse { .. }
        | StepStatement::Delay { .. }
        | StepStatement::Timeout(_)
        | StepStatement::Goto(_)
        | StepStatement::AllowIndefiniteWait(_) => false,
    })
}

fn task_has_startup_residue_proof(
    task: &TaskDeclaration,
    physical_symbols: &HashSet<String>,
) -> bool {
    task.steps.iter().any(|step| {
        let keyword_hit = step_name_or_logs_contain_keywords(&step.name, &step.statements);
        keyword_hit
            && (step_has_physical_wait(&step.statements, physical_symbols)
                || step_contains_workpiece_effects(&step.statements))
    })
}

fn step_name_or_logs_contain_keywords(step_name: &str, statements: &[StepStatement]) -> bool {
    let lower = step_name.to_ascii_lowercase();
    if contains_residual_keyword(&lower) {
        return true;
    }

    statements.iter().any(|statement| match statement {
        StepStatement::Action(ActionStatement::Log { message }) => {
            contains_residual_keyword(&message.to_ascii_lowercase())
        }
        StepStatement::Repeat { body, .. } => step_name_or_logs_contain_keywords(step_name, body),
        StepStatement::Parallel(block) => block
            .branches
            .iter()
            .any(|branch| step_name_or_logs_contain_keywords(step_name, &branch.statements)),
        StepStatement::Race(block) => block
            .branches
            .iter()
            .any(|branch| step_name_or_logs_contain_keywords(step_name, &branch.statements)),
        StepStatement::Action(_)
        | StepStatement::Effect(_)
        | StepStatement::Wait(_)
        | StepStatement::IfElse { .. }
        | StepStatement::Delay { .. }
        | StepStatement::Timeout(_)
        | StepStatement::Goto(_)
        | StepStatement::AllowIndefiniteWait(_) => false,
    })
}

fn contains_residual_keyword(text: &str) -> bool {
    [
        "confirm", "check", "inspect", "clear", "cleanup", "empty", "baseline", "residue",
        "recover",
    ]
    .iter()
    .any(|keyword| text.contains(keyword))
}

fn step_has_physical_wait(
    statements: &[StepStatement],
    physical_symbols: &HashSet<String>,
) -> bool {
    statements.iter().any(|statement| match statement {
        StepStatement::Wait(wait) => {
            wait_condition_has_physical_operand(&wait.condition, physical_symbols)
        }
        StepStatement::Repeat { body, .. } => step_has_physical_wait(body, physical_symbols),
        StepStatement::Parallel(block) => block
            .branches
            .iter()
            .any(|branch| step_has_physical_wait(&branch.statements, physical_symbols)),
        StepStatement::Race(block) => block
            .branches
            .iter()
            .any(|branch| step_has_physical_wait(&branch.statements, physical_symbols)),
        StepStatement::Action(_)
        | StepStatement::Effect(_)
        | StepStatement::IfElse { .. }
        | StepStatement::Delay { .. }
        | StepStatement::Timeout(_)
        | StepStatement::Goto(_)
        | StepStatement::AllowIndefiniteWait(_) => false,
    })
}

fn wait_condition_has_physical_operand(
    condition: &WaitCondition,
    physical_symbols: &HashSet<String>,
) -> bool {
    match condition {
        WaitCondition::Single(expr) => condition_has_physical_operand(expr, physical_symbols),
        WaitCondition::And(items) | WaitCondition::Or(items) => items
            .iter()
            .any(|expr| condition_has_physical_operand(expr, physical_symbols)),
        WaitCondition::Edge(edge) => symbol_is_physical(&edge.operand, physical_symbols),
    }
}

fn condition_has_physical_operand(
    condition: &ConditionExpression,
    physical_symbols: &HashSet<String>,
) -> bool {
    if let Some((left, right)) = condition.expression_pair() {
        return expression_has_physical_operand(left, physical_symbols)
            || expression_has_physical_operand(right, physical_symbols);
    }
    symbol_is_physical(&condition.left, physical_symbols)
}

fn expression_has_physical_operand(expr: &Expression, physical_symbols: &HashSet<String>) -> bool {
    match expr {
        Expression::Variable(name) => symbol_is_physical(name, physical_symbols),
        Expression::UnaryNeg(inner) | Expression::UnaryNot(inner) => {
            expression_has_physical_operand(inner, physical_symbols)
        }
        Expression::BinaryOp { left, right, .. } => {
            expression_has_physical_operand(left, physical_symbols)
                || expression_has_physical_operand(right, physical_symbols)
        }
        Expression::FunctionCall { args, .. } => args
            .iter()
            .any(|arg| expression_has_physical_operand(arg, physical_symbols)),
        Expression::Literal(_) | Expression::Boolean(_) => false,
    }
}

fn is_init_like(task_name: &str) -> bool {
    let lower = task_name.to_ascii_lowercase();
    lower.contains("init") || lower.contains("startup")
}

fn is_recovery_like(task_name: &str) -> bool {
    let lower = task_name.to_ascii_lowercase();
    lower.contains("fault")
        || lower.contains("recovery")
        || lower.contains("estop")
        || lower.contains("alarm")
}

fn is_done_like(task_name: &str) -> bool {
    let lower = task_name.to_ascii_lowercase();
    lower.contains("done") || lower.contains("halt")
}

fn is_non_auto_task(task_name: &str) -> bool {
    is_init_like(task_name)
        || is_recovery_like(task_name)
        || is_done_like(task_name)
        || task_name.to_ascii_lowercase().contains("manual")
        || task_name.to_ascii_lowercase().contains("service")
        || task_name.to_ascii_lowercase().contains("hmi")
}

fn is_auto_task_name(task_name: &str) -> bool {
    !is_non_auto_task(task_name)
}

fn step_waits_on_home_sensor(statements: &[StepStatement]) -> bool {
    wait_symbols_in_statements(statements)
        .into_iter()
        .any(|symbol| symbol.to_ascii_lowercase().contains("home"))
}

fn step_has_axis_motion(statements: &[StepStatement]) -> bool {
    statements.iter().any(|statement| match statement {
        StepStatement::Action(
            ActionStatement::AxisMoveRelative { .. } | ActionStatement::AxisMoveAbsolute { .. },
        ) => true,
        StepStatement::Repeat { body, .. } => step_has_axis_motion(body),
        StepStatement::Parallel(block) => block
            .branches
            .iter()
            .any(|branch| step_has_axis_motion(&branch.statements)),
        StepStatement::Race(block) => block
            .branches
            .iter()
            .any(|branch| step_has_axis_motion(&branch.statements)),
        StepStatement::Action(_)
        | StepStatement::Effect(_)
        | StepStatement::Wait(_)
        | StepStatement::IfElse { .. }
        | StepStatement::Delay { .. }
        | StepStatement::Timeout(_)
        | StepStatement::Goto(_)
        | StepStatement::AllowIndefiniteWait(_) => false,
    })
}

fn step_allows_indefinite_wait(statements: &[StepStatement]) -> bool {
    statements.iter().any(|statement| match statement {
        StepStatement::AllowIndefiniteWait(true) => true,
        StepStatement::Repeat { body, .. } => step_allows_indefinite_wait(body),
        StepStatement::Parallel(block) => block
            .branches
            .iter()
            .any(|branch| step_allows_indefinite_wait(&branch.statements)),
        StepStatement::Race(block) => block
            .branches
            .iter()
            .any(|branch| step_allows_indefinite_wait(&branch.statements)),
        StepStatement::Action(_)
        | StepStatement::Effect(_)
        | StepStatement::Wait(_)
        | StepStatement::IfElse { .. }
        | StepStatement::Delay { .. }
        | StepStatement::Timeout(_)
        | StepStatement::Goto(_)
        | StepStatement::AllowIndefiniteWait(false) => false,
    })
}

fn first_local_controlled_wait_symbol(statements: &[StepStatement]) -> Option<String> {
    wait_symbols_in_statements(statements)
        .into_iter()
        .find(|symbol| is_local_controlled_sensor_symbol(symbol))
}

fn wait_symbols_in_statements(statements: &[StepStatement]) -> Vec<String> {
    let mut symbols = Vec::new();
    for statement in statements {
        match statement {
            StepStatement::Wait(wait) => {
                collect_wait_condition_symbols(&wait.condition, &mut symbols)
            }
            StepStatement::Repeat { body, .. } => symbols.extend(wait_symbols_in_statements(body)),
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    symbols.extend(wait_symbols_in_statements(&branch.statements));
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    symbols.extend(wait_symbols_in_statements(&branch.statements));
                }
            }
            StepStatement::Action(_)
            | StepStatement::Effect(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
    symbols
}

fn collect_wait_condition_symbols(condition: &WaitCondition, symbols: &mut Vec<String>) {
    match condition {
        WaitCondition::Single(expr) => collect_condition_symbols(expr, symbols),
        WaitCondition::And(items) | WaitCondition::Or(items) => {
            for expr in items {
                collect_condition_symbols(expr, symbols);
            }
        }
        WaitCondition::Edge(edge) => symbols.push(edge.operand.clone()),
    }
}

fn collect_condition_symbols(condition: &ConditionExpression, symbols: &mut Vec<String>) {
    if let Some((left, right)) = condition.expression_pair() {
        collect_expression_symbols(left, symbols);
        collect_expression_symbols(right, symbols);
        return;
    }
    if !condition.left.trim().is_empty() {
        symbols.push(condition.left.clone());
    }
}

fn collect_expression_symbols(expr: &Expression, symbols: &mut Vec<String>) {
    match expr {
        Expression::Variable(name) => symbols.push(name.clone()),
        Expression::UnaryNeg(inner) | Expression::UnaryNot(inner) => {
            collect_expression_symbols(inner, symbols)
        }
        Expression::BinaryOp { left, right, .. } => {
            collect_expression_symbols(left, symbols);
            collect_expression_symbols(right, symbols);
        }
        Expression::FunctionCall { args, .. } => {
            for arg in args {
                collect_expression_symbols(arg, symbols);
            }
        }
        Expression::Literal(_) | Expression::Boolean(_) => {}
    }
}

fn is_local_controlled_sensor_symbol(symbol: &str) -> bool {
    let lower = symbol.to_ascii_lowercase();
    if lower.contains("start")
        || lower.contains("stop")
        || lower.contains("reset")
        || lower.contains("ack")
        || lower.contains("button")
        || lower.contains("mode")
        || lower.contains("manual")
        || lower.contains("operator")
        || lower.contains("hmi")
        || lower.contains("upstream")
        || lower.contains("downstream")
        || lower.contains("handoff")
        || lower.contains("request")
        || lower.contains("allow")
        || lower.contains("ready")
    {
        return false;
    }

    lower.starts_with("sensor_")
        || lower.contains("_sensor")
        || lower.contains("wafer_on")
        || lower.contains("vac")
        || lower.contains("home")
        || lower.contains("drop")
        || lower.contains("limit")
        || lower.contains("_empty")
}

fn looks_like_residual_check_step(step_name: &str, statements: &[StepStatement]) -> bool {
    let lower = step_name.to_ascii_lowercase();
    (lower.contains("residual")
        || lower.contains("empty")
        || lower.contains("clear")
        || lower.contains("baseline"))
        && wait_symbols_in_statements(statements)
            .into_iter()
            .any(|symbol| is_local_controlled_sensor_symbol(&symbol))
}

fn step_has_recovery_action_or_effect(statements: &[StepStatement]) -> bool {
    statements.iter().any(|statement| match statement {
        StepStatement::Action(action) => is_physical_action(action),
        StepStatement::Effect(_) => true,
        StepStatement::Repeat { body, .. } => step_has_recovery_action_or_effect(body),
        StepStatement::Parallel(block) => block
            .branches
            .iter()
            .any(|branch| step_has_recovery_action_or_effect(&branch.statements)),
        StepStatement::Race(block) => block
            .branches
            .iter()
            .any(|branch| step_has_recovery_action_or_effect(&branch.statements)),
        StepStatement::Wait(_)
        | StepStatement::IfElse { .. }
        | StepStatement::Delay { .. }
        | StepStatement::Timeout(_)
        | StepStatement::Goto(_)
        | StepStatement::AllowIndefiniteWait(_) => false,
    })
}

fn first_timeout_target(statements: &[StepStatement]) -> Option<&crate::ast::GotoDirective> {
    for statement in statements {
        match statement {
            StepStatement::Timeout(timeout) => return Some(&timeout.target),
            StepStatement::Action(action) => {
                if let Some(target) = action_timeout_target(action) {
                    return Some(target);
                }
            }
            StepStatement::Repeat { body, .. } => {
                if let Some(target) = first_timeout_target(body) {
                    return Some(target);
                }
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    if let Some(target) = first_timeout_target(&branch.statements) {
                        return Some(target);
                    }
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    if let Some(target) = first_timeout_target(&branch.statements) {
                        return Some(target);
                    }
                }
            }
            StepStatement::Effect(_)
            | StepStatement::Wait(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
    None
}

fn action_timeout_target(action: &ActionStatement) -> Option<&crate::ast::GotoDirective> {
    match action {
        ActionStatement::Extend { timeout, .. }
        | ActionStatement::Retract { timeout, .. }
        | ActionStatement::AxisMoveRelative { timeout, .. }
        | ActionStatement::AxisMoveAbsolute { timeout, .. } => {
            timeout.as_ref().map(|timeout| &timeout.target)
        }
        ActionStatement::Set { .. }
        | ActionStatement::SetAnalog { .. }
        | ActionStatement::SetAnalogExpr { .. }
        | ActionStatement::Compute { .. }
        | ActionStatement::Call { .. }
        | ActionStatement::CamEngage { .. }
        | ActionStatement::CamDisengage { .. }
        | ActionStatement::CamSwitch { .. }
        | ActionStatement::CamPhase { .. }
        | ActionStatement::DeviceAction { .. }
        | ActionStatement::Log { .. } => None,
    }
}

fn route_is_explicit_manual_or_recovery(target: &crate::ast::GotoDirective) -> bool {
    let target_text = format!(
        "{}.{}",
        target.task.to_ascii_lowercase(),
        target.step.clone().unwrap_or_default().to_ascii_lowercase()
    );
    target_text.contains("manual")
        || target_text.contains("operator")
        || target_text.contains("assist")
        || target_text.contains("ack")
        || target_text.contains("recover")
        || target_text.contains("cleanup")
        || target_text.contains("clear")
        || target_text.contains("reject")
        || target_text.contains("rehome")
}

fn step_turns_vacuum_off(
    statements: &[StepStatement],
    device_types: &HashMap<String, DeviceType>,
    device_purposes: &HashMap<String, String>,
) -> bool {
    statements.iter().any(|statement| match statement {
        StepStatement::Action(ActionStatement::Set { target, value }) => {
            value.eq_ignore_ascii_case("off")
                && target.port.eq_ignore_ascii_case("coil")
                && target_is_vacuum_device(&target.device, device_types, device_purposes)
        }
        StepStatement::Repeat { body, .. } => {
            step_turns_vacuum_off(body, device_types, device_purposes)
        }
        StepStatement::Parallel(block) => block
            .branches
            .iter()
            .any(|branch| step_turns_vacuum_off(&branch.statements, device_types, device_purposes)),
        StepStatement::Race(block) => block
            .branches
            .iter()
            .any(|branch| step_turns_vacuum_off(&branch.statements, device_types, device_purposes)),
        StepStatement::Action(_)
        | StepStatement::Effect(_)
        | StepStatement::Wait(_)
        | StepStatement::IfElse { .. }
        | StepStatement::Delay { .. }
        | StepStatement::Timeout(_)
        | StepStatement::Goto(_)
        | StepStatement::AllowIndefiniteWait(_) => false,
    })
}

fn target_is_vacuum_device(
    device: &str,
    device_types: &HashMap<String, DeviceType>,
    device_purposes: &HashMap<String, String>,
) -> bool {
    let lower = device.to_ascii_lowercase();
    lower.contains("vac")
        || lower.contains("vacuum")
        || (matches!(
            device_types.get(&lower),
            Some(DeviceType::SolenoidValve | DeviceType::Pump)
        ) && device_purposes
            .get(&lower)
            .is_some_and(|purpose| purpose.contains("vac")))
}

fn first_holder_to_non_holder_transfer(
    statements: &[StepStatement],
    holder_names: &HashSet<String>,
) -> Option<(String, String)> {
    for statement in statements {
        match statement {
            StepStatement::Effect(effect) => {
                if let EffectKind::Transfer { from, to } = &effect.kind {
                    if holder_names.contains(&from.to_ascii_lowercase())
                        && !holder_names.contains(&to.to_ascii_lowercase())
                    {
                        return Some((from.clone(), to.clone()));
                    }
                }
            }
            StepStatement::Repeat { body, .. } => {
                if let Some(found) = first_holder_to_non_holder_transfer(body, holder_names) {
                    return Some(found);
                }
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    if let Some(found) =
                        first_holder_to_non_holder_transfer(&branch.statements, holder_names)
                    {
                        return Some(found);
                    }
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    if let Some(found) =
                        first_holder_to_non_holder_transfer(&branch.statements, holder_names)
                    {
                        return Some(found);
                    }
                }
            }
            StepStatement::Action(_)
            | StepStatement::Wait(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
    None
}

fn collect_action_targets(program: &PlcProgram) -> HashSet<String> {
    let mut targets = HashSet::new();
    for task in &program.tasks.tasks {
        for step in &task.steps {
            collect_action_targets_in_statements(&step.statements, &mut targets);
        }
    }
    targets
}

fn collect_action_targets_in_statements(
    statements: &[StepStatement],
    targets: &mut HashSet<String>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(action) => {
                if let Some(device) = action_target_device(action) {
                    targets.insert(device.to_ascii_lowercase());
                }
            }
            StepStatement::Repeat { body, .. } => {
                collect_action_targets_in_statements(body, targets)
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    collect_action_targets_in_statements(&branch.statements, targets);
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    collect_action_targets_in_statements(&branch.statements, targets);
                }
            }
            StepStatement::Effect(_)
            | StepStatement::Wait(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
}

fn action_target_device(action: &ActionStatement) -> Option<&str> {
    match action {
        ActionStatement::Extend { target, .. }
        | ActionStatement::Retract { target, .. }
        | ActionStatement::Set { target, .. }
        | ActionStatement::SetAnalog { target, .. }
        | ActionStatement::SetAnalogExpr { target, .. }
        | ActionStatement::DeviceAction { target, .. }
        | ActionStatement::AxisMoveRelative { target, .. }
        | ActionStatement::AxisMoveAbsolute { target, .. } => Some(target.device.as_str()),
        ActionStatement::CamEngage { target }
        | ActionStatement::CamDisengage { target }
        | ActionStatement::CamSwitch { target, .. }
        | ActionStatement::CamPhase { target, .. } => Some(target.as_str()),
        ActionStatement::Compute { .. }
        | ActionStatement::Call { .. }
        | ActionStatement::Log { .. } => None,
    }
}

fn device_requires_self_check(device_type: &DeviceType) -> bool {
    matches!(
        device_type,
        DeviceType::SolenoidValve
            | DeviceType::Cylinder
            | DeviceType::Motor
            | DeviceType::StepperMotor
            | DeviceType::Vfd
            | DeviceType::ServoDrive
            | DeviceType::CamCoupling
            | DeviceType::Pid
            | DeviceType::ProportionalValve
            | DeviceType::Gripper
            | DeviceType::Conveyor
            | DeviceType::Pump
            | DeviceType::Heater
            | DeviceType::VisionSensor
    )
}

fn program_has_self_check_for_device(program: &PlcProgram, device: &str) -> bool {
    let needle = device.to_ascii_lowercase();
    program.tasks.tasks.iter().any(|task| {
        let task_self_check = looks_like_self_check_context(&task.name);
        task.steps.iter().any(|step| {
            (task_self_check || looks_like_self_check_context(&step.name))
                && step_actions_target_device(&step.statements, &needle)
        })
    })
}

fn looks_like_self_check_context(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("self_check")
        || lower.contains("selfcheck")
        || lower.contains("maintenance")
        || lower.contains("commission")
        || lower.contains("test")
        || lower.contains("diagnostic")
}

fn step_actions_target_device(statements: &[StepStatement], device: &str) -> bool {
    statements.iter().any(|statement| match statement {
        StepStatement::Action(action) => {
            action_target_device(action).is_some_and(|target| target.eq_ignore_ascii_case(device))
        }
        StepStatement::Repeat { body, .. } => step_actions_target_device(body, device),
        StepStatement::Parallel(block) => block
            .branches
            .iter()
            .any(|branch| step_actions_target_device(&branch.statements, device)),
        StepStatement::Race(block) => block
            .branches
            .iter()
            .any(|branch| step_actions_target_device(&branch.statements, device)),
        StepStatement::Effect(_)
        | StepStatement::Wait(_)
        | StepStatement::IfElse { .. }
        | StepStatement::Delay { .. }
        | StepStatement::Timeout(_)
        | StepStatement::Goto(_)
        | StepStatement::AllowIndefiniteWait(_) => false,
    })
}

fn find_project_root(source_path: &Path) -> Option<PathBuf> {
    let start = if source_path.is_dir() {
        source_path
    } else {
        source_path.parent()?
    };
    for dir in start.ancestors() {
        if dir.join(PROJECT_MANIFEST_FILE).exists() {
            return Some(dir.to_path_buf());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        NoFeedbackStepException, SelfCheckDeviceException, StateProofConfig, analyze_program,
    };
    use crate::parser::parse_plc;
    use crate::semantic::preprocess_program;

    fn analyze(source: &str, config: StateProofConfig) -> Vec<String> {
        let parsed = parse_plc(source).expect("fixture should parse");
        let expanded = preprocess_program(&parsed).expect("fixture should preprocess");
        analyze_program(&expanded, &config)
            .into_iter()
            .map(|issue| issue.code)
            .collect()
    }

    #[test]
    fn seeded_bool_wait_flag_is_rejected() {
        let source = r#"
[topology]
variable feed_cassette_has_seed: bool = true

[constraints]

[tasks]
task main:
    step wait_seed:
        wait: feed_cassette_has_seed == true
"#;
        let codes = analyze(source, StateProofConfig::default());
        assert!(codes.iter().any(|code| code == "SPF-001"));
    }

    #[test]
    fn sensor_derived_readiness_flag_is_allowed() {
        let source = r#"
[topology]
device cassette_present: sensor { purpose: "Cassette present sensor" }
variable cassette_ready: bool = false

[constraints]

[tasks]
task startup_init:
    step derive_ready:
        action: compute cassette_ready = cassette_present

task main:
    step wait_ready:
        wait: cassette_ready == true
"#;
        let codes = analyze(source, StateProofConfig::default());
        assert!(
            codes.is_empty(),
            "sensor-derived readiness should pass, got {codes:?}"
        );
    }

    #[test]
    fn no_feedback_exception_can_whitelist_a_startup_assumption() {
        let source = r#"
[topology]
device lamp: solenoid_valve { purpose: "Minimal output" }
variable cassette_ready: bool = false

[constraints]

[tasks]
task startup_init:
    step assume_ready:
        action: set lamp.coil on
        action: compute cassette_ready = true

task main:
    step wait_ready:
        wait: cassette_ready == true
"#;
        let codes = analyze(
            source,
            StateProofConfig {
                no_feedback_steps: vec![NoFeedbackStepException {
                    task: "startup_init".to_string(),
                    step: "assume_ready".to_string(),
                    reason: "No physical feedback exists on this minimal fixture".to_string(),
                    proof_basis: "Reviewed startup checklist".to_string(),
                }],
                trusted_initial_state: Vec::new(),
                self_check_exempt_devices: vec![SelfCheckDeviceException {
                    device: "lamp".to_string(),
                    reason: "Minimal no-feedback fixture device".to_string(),
                    proof_basis: "Unit test only exercises state proof assignment logic"
                        .to_string(),
                }],
            },
        );
        assert!(
            codes.is_empty(),
            "no_feedback_steps should whitelist the reviewed startup assumption, got {codes:?}"
        );
    }

    #[test]
    fn workpiece_project_without_residual_strategy_is_rejected() {
        let source = r#"
[topology]
workpiece part: workpiece_type {
    normal_terminal_states: [finished]
    ingress_sites: [infeed]
    normal_egress_sites: [outfeed]
}
location infeed: workpiece_location { capacity: 1 }
location outfeed: workpiece_location { capacity: 1 }
holder arm: workpiece_holder { capacity: 1 }

[constraints]

[tasks]
task main:
    step pick:
        effect: acquire holder arm from infeed
    step place:
        effect: transfer from arm to outfeed
    step finish:
        effect: finish workpiece at outfeed as finished
"#;
        let codes = analyze(source, StateProofConfig::default());
        assert!(codes.iter().any(|code| code == "SPF-020"));
        assert!(codes.iter().any(|code| code == "SPF-021"));
    }

    #[test]
    fn startup_operator_confirmation_satisfies_residual_strategy() {
        let source = r#"
[topology]
device operator_empty_confirm: sensor { purpose: "Operator confirms machine is empty" }
workpiece part: workpiece_type {
    normal_terminal_states: [finished]
    ingress_sites: [infeed]
    normal_egress_sites: [outfeed]
}
location infeed: workpiece_location { capacity: 1 }
location outfeed: workpiece_location { capacity: 1 }
holder arm: workpiece_holder { capacity: 1 }

[constraints]

[tasks]
task startup_init:
    step confirm_empty_machine:
        wait: operator_empty_confirm == true
        allow_indefinite_wait: true

task main:
    step pick:
        effect: acquire holder arm from infeed
    step place:
        effect: transfer from arm to outfeed
    step finish:
        effect: finish workpiece at outfeed as finished
"#;
        let codes = analyze(source, StateProofConfig::default());
        assert!(
            !codes
                .iter()
                .any(|code| code == "SPF-020" || code == "SPF-021"),
            "startup operator confirmation should satisfy the residual strategy, got {codes:?}"
        );
    }

    #[test]
    fn init_home_wait_before_axis_command_is_rejected() {
        let source = r#"
[topology]
device sensor_arm_home: sensor { purpose: "Arm home sensor" }

[constraints]

[tasks]
task startup_init:
    step wait_arm_home:
        wait: sensor_arm_home == true
        timeout: 5000ms -> goto startup_fault.init_pose_timeout

task startup_fault:
    step init_pose_timeout:
        action: log "startup fault"
"#;
        let codes = analyze(source, StateProofConfig::default());
        assert!(
            codes.iter().any(|code| code == "SPF-030"),
            "uncommanded home wait should be rejected, got {codes:?}"
        );
    }

    #[test]
    fn allow_indefinite_wait_on_local_sensor_is_rejected() {
        let source = r#"
[topology]
device wafer_on_slide: sensor { purpose: "Slide occupancy sensor" }

[constraints]

[tasks]
task startup_init:
    step wait_slide_empty:
        wait: wafer_on_slide == false
        allow_indefinite_wait: true
"#;
        let codes = analyze(source, StateProofConfig::default());
        assert!(
            codes.iter().any(|code| code == "SPF-031"),
            "local controlled sensor must not use indefinite wait, got {codes:?}"
        );
    }

    #[test]
    fn allow_indefinite_wait_on_uncontrolled_upstream_signal_is_allowed() {
        let source = r#"
[topology]

[constraints]

[tasks]
task station_b:
    step wait_station_a:
        wait: upstream_ready == true
        allow_indefinite_wait: true
"#;
        let codes = analyze(source, StateProofConfig::default());
        assert!(
            !codes.iter().any(|code| code == "SPF-031"),
            "upstream/other-task waits may be indefinite from this task boundary, got {codes:?}"
        );
    }

    #[test]
    fn residual_check_to_generic_fault_is_rejected() {
        let source = r#"
[topology]
device wafer_on_slide: sensor { purpose: "Slide occupancy sensor" }

[constraints]

[tasks]
task startup_init:
    step check_slide_empty:
        wait: wafer_on_slide == false
        timeout: 5000ms -> goto startup_fault.init_pose_timeout

task startup_fault:
    step init_pose_timeout:
        action: log "startup fault"
"#;
        let codes = analyze(source, StateProofConfig::default());
        assert!(
            codes.iter().any(|code| code == "SPF-032"),
            "residual check must route to explicit recovery/manual semantics, got {codes:?}"
        );
    }

    #[test]
    fn vacuum_release_to_passive_location_is_rejected() {
        let source = r#"
[topology]
device vac_feed_valve: solenoid_valve { purpose: "Feed vacuum valve" }
workpiece wafer: workpiece_type {
    normal_terminal_states: [finished]
    ingress_sites: [feed_ejector]
    normal_egress_sites: [slide_pick_site]
}
holder feed_ejector: workpiece_holder { capacity: 1 }
location slide_pick_site: workpiece_location { capacity: 1 }

[constraints]

[tasks]
task feed:
    step release_to_slide:
        action: set vac_feed_valve.coil off
        effect: transfer from feed_ejector to slide_pick_site
"#;
        let codes = analyze(source, StateProofConfig::default());
        assert!(
            codes.iter().any(|code| code == "SPF-033"),
            "vacuum release before receiver ownership proof should be rejected, got {codes:?}"
        );
    }

    #[test]
    fn actuated_device_without_self_check_is_rejected() {
        let source = r#"
[topology]
device feed_motor: motor { purpose: "Feed motor" }

[constraints]

[tasks]
task main:
    step run_feed:
        action: set feed_motor.run on
"#;
        let codes = analyze(source, StateProofConfig::default());
        assert!(
            codes.iter().any(|code| code == "SPF-040"),
            "actuated devices need self-check or explicit exemption, got {codes:?}"
        );
    }

    #[test]
    fn actuated_device_self_check_task_satisfies_self_check_gate() {
        let source = r#"
[topology]
device feed_motor: motor { purpose: "Feed motor" }

[constraints]

[tasks]
task maintenance_self_check:
    step jog_feed_motor:
        action: set feed_motor.run on

task main:
    step run_feed:
        action: set feed_motor.run on
"#;
        let codes = analyze(source, StateProofConfig::default());
        assert!(
            !codes.iter().any(|code| code == "SPF-040"),
            "self-check task should satisfy actuated device gate, got {codes:?}"
        );
    }
}
