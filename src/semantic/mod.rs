use crate::ast::{
    ActionStatement, BinaryValue as AstBinaryValue, ComparisonOperator, ConditionExpression,
    ConstraintsSection, DeviceType, DurationValue, GotoDirective, LiteralValue,
    OnCompleteDirective, ParallelBlock, PlcProgram, RaceBlock, SafetyRelation as AstSafetyRelation,
    StepStatement, TaskDeclaration, TasksSection, TimeUnit, TimeoutDirective,
    TimingRelation as AstTimingRelation, TimingTarget, TopologySection, WaitStatement,
};
use crate::error::PlcError;
use crate::ir::{
    ActionKind, ActionRef, ActionTiming, BinaryValue as IrBinaryValue, CausalityChain,
    ConnectionType, ConstraintSet, Device, DeviceKind, SafetyRelation as IrSafetyRelation,
    SafetyRule, State, StateExpr, StateMachine, TimeInterval, TimerOperation, TimerOperationKind,
    TimingModel, TimingRelation as IrTimingRelation, TimingRule, TimingScope, TopologyGraph,
    Transition, TransitionAction, TransitionGuard,
};
use petgraph::graph::NodeIndex;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone)]
struct DeviceNode {
    index: NodeIndex,
    kind: DeviceKind,
}

pub fn build_topology_graph(program: &PlcProgram) -> Result<TopologyGraph, Vec<PlcError>> {
    build_topology_from_ast(&program.topology)
}

pub fn build_state_machine(program: &PlcProgram) -> Result<StateMachine, Vec<PlcError>> {
    build_state_machine_from_ast(&program.tasks)
}

pub fn build_constraint_set(program: &PlcProgram) -> Result<ConstraintSet, Vec<PlcError>> {
    build_constraint_set_from_ast(&program.topology, &program.constraints, &program.tasks)
}

pub fn build_timing_model(program: &PlcProgram) -> Result<TimingModel, Vec<PlcError>> {
    build_timing_model_from_ast(&program.topology, &program.tasks)
}

pub fn build_topology_from_ast(topology: &TopologySection) -> Result<TopologyGraph, Vec<PlcError>> {
    let mut topology_graph = TopologyGraph::new();
    let mut device_nodes = HashMap::<String, DeviceNode>::new();
    let mut errors = Vec::new();

    for device in &topology.devices {
        let kind = ast_type_to_ir_kind(&device.device_type);
        let index = topology_graph.add_device(Device {
            name: device.name.clone(),
            kind: kind.clone(),
        });

        device_nodes.insert(device.name.clone(), DeviceNode { index, kind });
    }

    for device in &topology.devices {
        let Some(target_name) = device.attributes.connected_to.as_deref() else {
            continue;
        };

        let Some(target_node) = device_nodes.get(target_name) else {
            errors.push(PlcError::undefined_reference_with_reason(
                device.line,
                "设备",
                target_name,
                format!(
                    "设备 {} 的 connected_to 引用了该名称，请先定义后再连接",
                    device.name
                ),
            ));
            continue;
        };

        let Some(current_node) = device_nodes.get(&device.name) else {
            continue;
        };

        let Some(connection_type) = connection_type_for(&target_node.kind, &current_node.kind)
        else {
            errors.push(PlcError::type_mismatch_with_reason(
                device.line,
                format!("可作为 {} 上游的设备", device_kind_name(&current_node.kind)),
                device_kind_name(&target_node.kind),
                format!("设备 {} 的 connected_to", device.name),
                format!(
                    "请检查 {} 与 {} 的连接方向，或调整为兼容设备类型",
                    target_name, device.name
                ),
            ));
            continue;
        };

        // `A connected_to B` means B provides upstream linkage into A.
        topology_graph.add_connection(target_node.index, current_node.index, connection_type);
    }

    if errors.is_empty() {
        Ok(topology_graph)
    } else {
        Err(errors)
    }
}

pub fn build_constraint_set_from_ast(
    topology: &TopologySection,
    constraints: &ConstraintsSection,
    tasks: &TasksSection,
) -> Result<ConstraintSet, Vec<PlcError>> {
    let mut errors = Vec::new();
    let mut constraint_set = ConstraintSet::default();

    let device_kinds = collect_device_kinds(topology);
    let known_states = collect_known_states(topology, &device_kinds);
    let task_steps = collect_task_steps(tasks);

    for safety in &constraints.safety {
        validate_state_reference(
            &safety.left,
            safety.line,
            "safety 左侧",
            &device_kinds,
            &known_states,
            &mut errors,
        );
        validate_state_reference(
            &safety.right,
            safety.line,
            "safety 右侧",
            &device_kinds,
            &known_states,
            &mut errors,
        );

        constraint_set.safety.push(SafetyRule {
            left: StateExpr {
                device: safety.left.device.clone(),
                state: safety.left.state.clone(),
            },
            relation: map_safety_relation(&safety.relation),
            right: StateExpr {
                device: safety.right.device.clone(),
                state: safety.right.state.clone(),
            },
            reason: safety.reason.clone(),
        });
    }

    for timing in &constraints.timing {
        validate_timing_target(&timing.target, timing.line, &task_steps, &mut errors);

        constraint_set.timing.push(TimingRule {
            scope: map_timing_scope(&timing.target),
            relation: map_timing_relation(&timing.relation),
            duration_ms: duration_value_to_ms(&timing.duration),
            reason: timing.reason.clone(),
        });
    }

    for causality in &constraints.causality {
        for node in &causality.chain {
            validate_device_reference(
                &node.device,
                causality.line,
                "causality",
                &device_kinds,
                &mut errors,
            );
        }

        constraint_set.causality.push(CausalityChain {
            devices: causality
                .chain
                .iter()
                .map(|node| node.device.clone())
                .collect(),
            reason: causality.reason.clone(),
        });
    }

    if errors.is_empty() {
        Ok(constraint_set)
    } else {
        Err(errors)
    }
}

pub fn build_timing_model_from_ast(
    topology: &TopologySection,
    tasks: &TasksSection,
) -> Result<TimingModel, Vec<PlcError>> {
    let device_profiles = collect_device_timing_profiles(topology);
    let mut intervals = BTreeMap::new();
    let mut errors = Vec::new();

    for task in &tasks.tasks {
        for step in &task.steps {
            let mut actions = Vec::new();
            collect_actions(&step.statements, &mut actions);

            for action in actions {
                if let Some(action_timing) = action_to_timing(
                    &task.name,
                    &step.name,
                    step.line,
                    &action,
                    &device_profiles,
                    &mut errors,
                ) {
                    insert_action_timing(&mut intervals, action_timing);
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(TimingModel { intervals })
    } else {
        Err(errors)
    }
}

pub fn build_state_machine_from_ast(tasks: &TasksSection) -> Result<StateMachine, Vec<PlcError>> {
    let mut builder = StateMachineBuilder::default();
    let mut errors = Vec::new();

    if tasks.tasks.is_empty() {
        errors.push(PlcError::semantic(1, "[tasks] 段至少需要一个 task"));
        return Err(errors);
    }

    let mut task_initial_states = HashMap::<String, State>::new();

    for task in &tasks.tasks {
        if task.steps.is_empty() {
            errors.push(PlcError::semantic(
                task.line,
                format!("task {} 至少需要一个 step", task.name),
            ));
            continue;
        }

        let initial_state = State {
            task_name: task.name.clone(),
            step_name: task.steps[0].name.clone(),
        };

        if task_initial_states
            .insert(task.name.clone(), initial_state)
            .is_some()
        {
            errors.push(PlcError::duplicate_definition_with_reason(
                task.line,
                "task",
                &task.name,
                "请确保每个 task 名称唯一",
            ));
        }

        for step in &task.steps {
            builder.add_state(&task.name, &step.name);
        }
    }

    let Some(initial) = tasks.tasks.iter().find_map(|task| {
        task.steps.first().map(|step| State {
            task_name: task.name.clone(),
            step_name: step.name.clone(),
        })
    }) else {
        errors.push(PlcError::semantic(1, "未找到可执行的 task/step 初始状态"));
        return Err(errors);
    };

    let mut task_on_complete_targets = HashMap::<String, Option<State>>::new();
    for task in &tasks.tasks {
        let on_complete_target = match &task.on_complete {
            Some(OnCompleteDirective::Goto { step }) => {
                let line = task.on_complete_line.unwrap_or(task.line);
                resolve_task_target(step, line, &task_initial_states, &mut errors, "on_complete")
            }
            _ => None,
        };
        task_on_complete_targets.insert(task.name.clone(), on_complete_target);
    }

    for task in &tasks.tasks {
        for (step_index, step) in task.steps.iter().enumerate() {
            let from_state = State {
                task_name: task.name.clone(),
                step_name: step.name.clone(),
            };
            let completion_target =
                completion_target_for_step(task, step_index, &task_on_complete_targets);

            let analyzed = analyze_statements(&step.statements);

            for (block_index, block) in analyzed.parallel_blocks.iter().enumerate() {
                build_parallel_block(
                    &mut builder,
                    task,
                    &step.name,
                    &from_state,
                    block_index,
                    block,
                    completion_target.clone(),
                    &task_initial_states,
                    &mut errors,
                    analyzed.actions.clone(),
                );
            }

            for (block_index, block) in analyzed.race_blocks.iter().enumerate() {
                build_race_block(
                    &mut builder,
                    task,
                    &step.name,
                    &from_state,
                    block_index,
                    block,
                    completion_target.clone(),
                    &task_initial_states,
                    &mut errors,
                    analyzed.actions.clone(),
                );
            }

            for goto in &analyzed.gotos {
                if let Some(target) = resolve_task_target(
                    &goto.step,
                    goto.line,
                    &task_initial_states,
                    &mut errors,
                    "goto",
                ) {
                    builder.add_transition(
                        from_state.clone(),
                        target,
                        TransitionGuard::Always,
                        analyzed.actions.clone(),
                        Vec::new(),
                    );
                }
            }

            for (delay_index, duration_ms) in analyzed.delays_ms.iter().enumerate() {
                if let Some(target) = completion_target.clone() {
                    builder.add_transition(
                        from_state.clone(),
                        target,
                        TransitionGuard::Delay {
                            duration_ms: *duration_ms,
                        },
                        Vec::new(),
                        vec![TimerOperation {
                            timer_name: format!(
                                "{}.{}.delay_{}",
                                task.name,
                                step.name,
                                delay_index + 1
                            ),
                            operation: TimerOperationKind::Start,
                            duration_ms: Some(*duration_ms),
                        }],
                    );
                }
            }

            for (timeout_index, timeout) in analyzed.timeouts.iter().enumerate() {
                if let Some(target) = resolve_task_target(
                    &timeout.target.step,
                    timeout.target.line,
                    &task_initial_states,
                    &mut errors,
                    "timeout -> goto",
                ) {
                    let duration_ms = duration_to_ms(timeout);
                    builder.add_transition(
                        from_state.clone(),
                        target,
                        TransitionGuard::Timeout { duration_ms },
                        Vec::new(),
                        vec![TimerOperation {
                            timer_name: format!(
                                "{}.{}.timeout_{}",
                                task.name,
                                step.name,
                                timeout_index + 1
                            ),
                            operation: TimerOperationKind::Start,
                            duration_ms: Some(duration_ms),
                        }],
                    );
                }
            }

            for wait_expression in &analyzed.waits {
                if let Some(target) = completion_target.clone() {
                    builder.add_transition(
                        from_state.clone(),
                        target,
                        TransitionGuard::Condition {
                            expression: wait_expression.clone(),
                        },
                        analyzed.actions.clone(),
                        Vec::new(),
                    );
                }
            }

            let has_control_flow = !analyzed.waits.is_empty()
                || !analyzed.delays_ms.is_empty()
                || !analyzed.gotos.is_empty()
                || !analyzed.parallel_blocks.is_empty()
                || !analyzed.race_blocks.is_empty();
            if !has_control_flow {
                if let Some(target) = completion_target {
                    builder.add_transition(
                        from_state,
                        target,
                        TransitionGuard::Always,
                        analyzed.actions,
                        Vec::new(),
                    );
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(StateMachine {
            states: builder.states,
            transitions: builder.transitions,
            initial,
        })
    } else {
        Err(errors)
    }
}

#[derive(Debug, Clone, Default)]
struct StateMachineBuilder {
    states: Vec<State>,
    transitions: Vec<Transition>,
    seen_states: HashSet<(String, String)>,
}

impl StateMachineBuilder {
    fn add_state(&mut self, task_name: &str, step_name: &str) -> State {
        let key = (task_name.to_string(), step_name.to_string());
        if self.seen_states.insert(key.clone()) {
            self.states.push(State {
                task_name: key.0.clone(),
                step_name: key.1.clone(),
            });
        }

        State {
            task_name: key.0,
            step_name: key.1,
        }
    }

    fn add_transition(
        &mut self,
        from: State,
        to: State,
        guard: TransitionGuard,
        actions: Vec<TransitionAction>,
        timers: Vec<TimerOperation>,
    ) {
        self.transitions.push(Transition {
            from,
            to,
            guard,
            actions,
            timers,
        });
    }
}

#[derive(Debug, Clone, Default)]
struct AnalyzedStatements {
    actions: Vec<TransitionAction>,
    waits: Vec<String>,
    delays_ms: Vec<u64>,
    gotos: Vec<GotoDirective>,
    timeouts: Vec<TimeoutDirective>,
    parallel_blocks: Vec<ParallelBlock>,
    race_blocks: Vec<RaceBlock>,
}

#[derive(Debug, Clone, Default)]
struct DeviceTimingProfile {
    response_ms: Option<u64>,
    stroke_ms: Option<u64>,
    retract_ms: Option<u64>,
    ramp_ms: Option<u64>,
}

fn collect_device_kinds(topology: &TopologySection) -> HashMap<String, DeviceKind> {
    topology
        .devices
        .iter()
        .map(|device| {
            (
                device.name.clone(),
                ast_type_to_ir_kind(&device.device_type),
            )
        })
        .collect()
}

fn collect_known_states(
    topology: &TopologySection,
    device_kinds: &HashMap<String, DeviceKind>,
) -> HashMap<String, HashSet<String>> {
    let mut known_states = HashMap::new();

    for (name, kind) in device_kinds {
        let mut states = HashSet::new();
        for state in default_states_for_kind(kind) {
            states.insert(state.to_string());
        }
        known_states.insert(name.clone(), states);
    }

    for device in &topology.devices {
        if let Some(detects) = &device.attributes.detects {
            known_states
                .entry(detects.device.clone())
                .or_default()
                .insert(detects.state.clone());
        }
    }

    known_states
}

fn collect_task_steps(tasks: &TasksSection) -> HashMap<String, HashSet<String>> {
    let mut task_steps = HashMap::new();

    for task in &tasks.tasks {
        let steps = task
            .steps
            .iter()
            .map(|step| step.name.clone())
            .collect::<HashSet<_>>();
        task_steps.insert(task.name.clone(), steps);
    }

    task_steps
}

fn validate_state_reference(
    state: &crate::ast::StateReference,
    line: usize,
    source: &str,
    device_kinds: &HashMap<String, DeviceKind>,
    known_states: &HashMap<String, HashSet<String>>,
    errors: &mut Vec<PlcError>,
) {
    let Some(_) = device_kinds.get(&state.device) else {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            "设备",
            &state.device,
            format!("{source} 使用前需要先在 [topology] 段定义设备"),
        ));
        return;
    };

    if state.state.is_empty() {
        errors.push(PlcError::semantic(
            line,
            format!("{source} 设备 {} 缺少状态名", state.device),
        ));
        return;
    }

    let Some(allowed_states) = known_states.get(&state.device) else {
        return;
    };

    if !allowed_states.is_empty() && !allowed_states.contains(&state.state) {
        errors.push(PlcError::semantic(
            line,
            format!(
                "{source} 引用了设备 {} 的未定义状态 {}",
                state.device, state.state
            ),
        ));
    }
}

fn validate_device_reference(
    device_name: &str,
    line: usize,
    source: &str,
    device_kinds: &HashMap<String, DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    if !device_kinds.contains_key(device_name) {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            "设备",
            device_name,
            format!("{source} 约束引用前需要定义该设备"),
        ));
    }
}

fn validate_timing_target(
    target: &TimingTarget,
    line: usize,
    task_steps: &HashMap<String, HashSet<String>>,
    errors: &mut Vec<PlcError>,
) {
    match target {
        TimingTarget::Task { task } => {
            if !task_steps.contains_key(task) {
                errors.push(PlcError::undefined_reference_with_reason(
                    line,
                    " task",
                    task,
                    "请先在 [tasks] 段定义该 task".to_string(),
                ));
            }
        }
        TimingTarget::Step { task, step } => {
            let Some(steps) = task_steps.get(task) else {
                errors.push(PlcError::undefined_reference_with_reason(
                    line,
                    " task",
                    task,
                    "请先在 [tasks] 段定义该 task".to_string(),
                ));
                return;
            };

            if !steps.contains(step) {
                errors.push(PlcError::semantic(
                    line,
                    format!("timing 约束引用了未定义 step {task}.{step}"),
                ));
            }
        }
    }
}

fn map_safety_relation(relation: &AstSafetyRelation) -> IrSafetyRelation {
    match relation {
        AstSafetyRelation::ConflictsWith => IrSafetyRelation::ConflictsWith,
        AstSafetyRelation::Requires => IrSafetyRelation::Requires,
    }
}

fn map_timing_scope(target: &TimingTarget) -> TimingScope {
    match target {
        TimingTarget::Task { task } => TimingScope::Task { task: task.clone() },
        TimingTarget::Step { task, step } => TimingScope::Step {
            task: task.clone(),
            step: step.clone(),
        },
    }
}

fn map_timing_relation(relation: &AstTimingRelation) -> IrTimingRelation {
    match relation {
        AstTimingRelation::MustCompleteWithin => IrTimingRelation::MustCompleteWithin,
        AstTimingRelation::MustStartAfter => IrTimingRelation::MustStartAfter,
    }
}

fn collect_device_timing_profiles(
    topology: &TopologySection,
) -> HashMap<String, DeviceTimingProfile> {
    topology
        .devices
        .iter()
        .map(|device| {
            (
                device.name.clone(),
                DeviceTimingProfile {
                    response_ms: device
                        .attributes
                        .response_time
                        .as_ref()
                        .map(duration_value_to_ms),
                    stroke_ms: device
                        .attributes
                        .stroke_time
                        .as_ref()
                        .map(duration_value_to_ms),
                    retract_ms: device
                        .attributes
                        .retract_time
                        .as_ref()
                        .map(duration_value_to_ms),
                    ramp_ms: device
                        .attributes
                        .ramp_time
                        .as_ref()
                        .map(duration_value_to_ms),
                },
            )
        })
        .collect()
}

fn collect_actions(statements: &[StepStatement], actions: &mut Vec<ActionStatement>) {
    for statement in statements {
        match statement {
            StepStatement::Action(action) => actions.push(action.clone()),
            StepStatement::Repeat { body, .. } => collect_actions(body, actions),
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    collect_actions(&branch.statements, actions);
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    collect_actions(&branch.statements, actions);
                }
            }
            StepStatement::Wait(_)
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
}

fn action_to_timing(
    task_name: &str,
    step_name: &str,
    line: usize,
    action: &ActionStatement,
    profiles: &HashMap<String, DeviceTimingProfile>,
    errors: &mut Vec<PlcError>,
) -> Option<ActionTiming> {
    let (action_kind, target) = match action {
        ActionStatement::Extend { target } => (ActionKind::Extend, Some(target.as_str())),
        ActionStatement::Retract { target } => (ActionKind::Retract, Some(target.as_str())),
        ActionStatement::Set { target, .. } => (ActionKind::Set, Some(target.as_str())),
        ActionStatement::Log { .. } => (ActionKind::Log, None),
    };

    let Some(target) = target else {
        return None;
    };

    let Some(profile) = profiles.get(target) else {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            "设备",
            target,
            "请先在 [topology] 段定义该设备并补充物理参数".to_string(),
        ));
        return None;
    };

    let duration_ms = match action_kind {
        ActionKind::Extend => profile
            .stroke_ms
            .or(profile.response_ms)
            .or(profile.ramp_ms),
        ActionKind::Retract => profile
            .retract_ms
            .or(profile.response_ms)
            .or(profile.ramp_ms),
        ActionKind::Set => profile.ramp_ms.or(profile.response_ms),
        ActionKind::Log => None,
    }?;

    Some(ActionTiming {
        action: ActionRef {
            task_name: task_name.to_string(),
            step_name: step_name.to_string(),
            action_kind,
            target: Some(target.to_string()),
        },
        interval: TimeInterval {
            min_ms: duration_ms,
            max_ms: duration_ms,
        },
    })
}

fn insert_action_timing(intervals: &mut BTreeMap<String, ActionTiming>, timing: ActionTiming) {
    let action_name = action_kind_name(&timing.action.action_kind);
    let target = timing.action.target.as_deref().unwrap_or("_");
    let base_key = format!(
        "{}.{}.{}.{}",
        timing.action.task_name, timing.action.step_name, action_name, target
    );

    if !intervals.contains_key(&base_key) {
        intervals.insert(base_key, timing);
        return;
    }

    let mut duplicate_index = 2usize;
    loop {
        let key = format!("{base_key}.{duplicate_index}");
        if !intervals.contains_key(&key) {
            intervals.insert(key, timing);
            return;
        }
        duplicate_index += 1;
    }
}

fn action_kind_name(action_kind: &ActionKind) -> &'static str {
    match action_kind {
        ActionKind::Extend => "extend",
        ActionKind::Retract => "retract",
        ActionKind::Set => "set",
        ActionKind::Log => "log",
    }
}

fn default_states_for_kind(kind: &DeviceKind) -> &'static [&'static str] {
    match kind {
        DeviceKind::Cylinder => &["extended", "retracted"],
        DeviceKind::DigitalOutput
        | DeviceKind::DigitalInput
        | DeviceKind::SolenoidValve
        | DeviceKind::Sensor
        | DeviceKind::Motor => &["on", "off"],
    }
}

fn completion_target_for_step(
    task: &TaskDeclaration,
    step_index: usize,
    task_on_complete_targets: &HashMap<String, Option<State>>,
) -> Option<State> {
    if step_index + 1 < task.steps.len() {
        return Some(State {
            task_name: task.name.clone(),
            step_name: task.steps[step_index + 1].name.clone(),
        });
    }

    task_on_complete_targets
        .get(&task.name)
        .cloned()
        .unwrap_or(None)
}

fn analyze_statements(statements: &[StepStatement]) -> AnalyzedStatements {
    let mut analyzed = AnalyzedStatements::default();

    for statement in statements {
        match statement {
            StepStatement::Action(action) => {
                analyzed.actions.push(action_to_transition_action(action));
            }
            StepStatement::Wait(wait) => {
                analyzed.waits.push(wait_to_guard_expression(wait));
            }
            StepStatement::Delay { duration_ms } => analyzed.delays_ms.push(*duration_ms),
            StepStatement::Repeat { .. } => {}
            StepStatement::Timeout(timeout) => analyzed.timeouts.push(timeout.clone()),
            StepStatement::Goto(goto) => analyzed.gotos.push(goto.clone()),
            StepStatement::Parallel(block) => analyzed.parallel_blocks.push(block.clone()),
            StepStatement::Race(block) => analyzed.race_blocks.push(block.clone()),
            StepStatement::AllowIndefiniteWait(_) => {}
        }
    }

    analyzed
}

fn build_parallel_block(
    builder: &mut StateMachineBuilder,
    task: &TaskDeclaration,
    step_name: &str,
    source_state: &State,
    block_index: usize,
    block: &ParallelBlock,
    completion_target: Option<State>,
    task_initial_states: &HashMap<String, State>,
    errors: &mut Vec<PlcError>,
    parent_actions: Vec<TransitionAction>,
) {
    let fork_state_name = format!("{step_name}__parallel_{}_fork", block_index + 1);
    let join_state_name = format!("{step_name}__parallel_{}_join", block_index + 1);

    let fork_state = builder.add_state(&task.name, &fork_state_name);
    let join_state = builder.add_state(&task.name, &join_state_name);

    builder.add_transition(
        source_state.clone(),
        fork_state.clone(),
        TransitionGuard::Always,
        parent_actions,
        Vec::new(),
    );

    for (branch_index, branch) in block.branches.iter().enumerate() {
        let branch_state_name = format!(
            "{step_name}__parallel_{}_branch_{}",
            block_index + 1,
            branch_index + 1
        );
        let branch_state = builder.add_state(&task.name, &branch_state_name);

        builder.add_transition(
            fork_state.clone(),
            branch_state.clone(),
            TransitionGuard::Always,
            Vec::new(),
            Vec::new(),
        );

        let analyzed = analyze_statements(&branch.statements);

        for goto in &analyzed.gotos {
            if let Some(target) =
                resolve_task_target(&goto.step, goto.line, task_initial_states, errors, "goto")
            {
                builder.add_transition(
                    branch_state.clone(),
                    target,
                    TransitionGuard::Always,
                    analyzed.actions.clone(),
                    Vec::new(),
                );
            }
        }

        for (delay_index, duration_ms) in analyzed.delays_ms.iter().enumerate() {
            builder.add_transition(
                branch_state.clone(),
                join_state.clone(),
                TransitionGuard::Delay {
                    duration_ms: *duration_ms,
                },
                Vec::new(),
                vec![TimerOperation {
                    timer_name: format!(
                        "{}.{}.parallel_{}_branch_{}.delay_{}",
                        task.name,
                        step_name,
                        block_index + 1,
                        branch_index + 1,
                        delay_index + 1
                    ),
                    operation: TimerOperationKind::Start,
                    duration_ms: Some(*duration_ms),
                }],
            );
        }

        for (timeout_index, timeout) in analyzed.timeouts.iter().enumerate() {
            if let Some(target) = resolve_task_target(
                &timeout.target.step,
                timeout.target.line,
                task_initial_states,
                errors,
                "timeout -> goto",
            ) {
                let duration_ms = duration_to_ms(timeout);
                builder.add_transition(
                    branch_state.clone(),
                    target,
                    TransitionGuard::Timeout { duration_ms },
                    Vec::new(),
                    vec![TimerOperation {
                        timer_name: format!(
                            "{}.{}.parallel_{}_branch_{}.timeout_{}",
                            task.name,
                            step_name,
                            block_index + 1,
                            branch_index + 1,
                            timeout_index + 1
                        ),
                        operation: TimerOperationKind::Start,
                        duration_ms: Some(duration_ms),
                    }],
                );
            }
        }

        for wait_expression in &analyzed.waits {
            builder.add_transition(
                branch_state.clone(),
                join_state.clone(),
                TransitionGuard::Condition {
                    expression: wait_expression.clone(),
                },
                analyzed.actions.clone(),
                Vec::new(),
            );
        }

        for (nested_parallel_index, nested_parallel) in analyzed.parallel_blocks.iter().enumerate()
        {
            build_parallel_block(
                builder,
                task,
                &format!(
                    "{step_name}__parallel_{}_branch_{}",
                    block_index + 1,
                    branch_index + 1
                ),
                &branch_state,
                nested_parallel_index,
                nested_parallel,
                Some(join_state.clone()),
                task_initial_states,
                errors,
                analyzed.actions.clone(),
            );
        }

        for (nested_race_index, nested_race) in analyzed.race_blocks.iter().enumerate() {
            build_race_block(
                builder,
                task,
                &format!(
                    "{step_name}__parallel_{}_branch_{}",
                    block_index + 1,
                    branch_index + 1
                ),
                &branch_state,
                nested_race_index,
                nested_race,
                Some(join_state.clone()),
                task_initial_states,
                errors,
                analyzed.actions.clone(),
            );
        }

        let has_control_flow = !analyzed.waits.is_empty()
            || !analyzed.delays_ms.is_empty()
            || !analyzed.gotos.is_empty()
            || !analyzed.parallel_blocks.is_empty()
            || !analyzed.race_blocks.is_empty();
        if !has_control_flow {
            builder.add_transition(
                branch_state,
                join_state.clone(),
                TransitionGuard::Always,
                analyzed.actions,
                Vec::new(),
            );
        }
    }

    if let Some(target) = completion_target {
        builder.add_transition(
            join_state,
            target,
            TransitionGuard::Always,
            Vec::new(),
            Vec::new(),
        );
    }
}

fn build_race_block(
    builder: &mut StateMachineBuilder,
    task: &TaskDeclaration,
    step_name: &str,
    source_state: &State,
    block_index: usize,
    block: &RaceBlock,
    completion_target: Option<State>,
    task_initial_states: &HashMap<String, State>,
    errors: &mut Vec<PlcError>,
    parent_actions: Vec<TransitionAction>,
) {
    let decision_state_name = format!("{step_name}__race_{}_decision", block_index + 1);
    let decision_state = builder.add_state(&task.name, &decision_state_name);

    builder.add_transition(
        source_state.clone(),
        decision_state.clone(),
        TransitionGuard::Always,
        parent_actions,
        Vec::new(),
    );

    for (branch_index, branch) in block.branches.iter().enumerate() {
        let branch_state_name = format!(
            "{step_name}__race_{}_branch_{}",
            block_index + 1,
            branch_index + 1
        );
        let branch_state = builder.add_state(&task.name, &branch_state_name);

        builder.add_transition(
            decision_state.clone(),
            branch_state.clone(),
            TransitionGuard::Always,
            Vec::new(),
            Vec::new(),
        );

        let analyzed = analyze_statements(&branch.statements);
        let branch_completion_target = branch
            .then_goto
            .as_ref()
            .and_then(|goto| {
                resolve_task_target(
                    &goto.step,
                    goto.line,
                    task_initial_states,
                    errors,
                    "race then goto",
                )
            })
            .or_else(|| completion_target.clone());

        for goto in &analyzed.gotos {
            if let Some(target) =
                resolve_task_target(&goto.step, goto.line, task_initial_states, errors, "goto")
            {
                builder.add_transition(
                    branch_state.clone(),
                    target,
                    TransitionGuard::Always,
                    analyzed.actions.clone(),
                    Vec::new(),
                );
            }
        }

        for (delay_index, duration_ms) in analyzed.delays_ms.iter().enumerate() {
            if let Some(target) = branch_completion_target.clone() {
                builder.add_transition(
                    branch_state.clone(),
                    target,
                    TransitionGuard::Delay {
                        duration_ms: *duration_ms,
                    },
                    Vec::new(),
                    vec![TimerOperation {
                        timer_name: format!(
                            "{}.{}.race_{}_branch_{}.delay_{}",
                            task.name,
                            step_name,
                            block_index + 1,
                            branch_index + 1,
                            delay_index + 1
                        ),
                        operation: TimerOperationKind::Start,
                        duration_ms: Some(*duration_ms),
                    }],
                );
            }
        }

        for (timeout_index, timeout) in analyzed.timeouts.iter().enumerate() {
            if let Some(target) = resolve_task_target(
                &timeout.target.step,
                timeout.target.line,
                task_initial_states,
                errors,
                "timeout -> goto",
            ) {
                let duration_ms = duration_to_ms(timeout);
                builder.add_transition(
                    branch_state.clone(),
                    target,
                    TransitionGuard::Timeout { duration_ms },
                    Vec::new(),
                    vec![TimerOperation {
                        timer_name: format!(
                            "{}.{}.race_{}_branch_{}.timeout_{}",
                            task.name,
                            step_name,
                            block_index + 1,
                            branch_index + 1,
                            timeout_index + 1
                        ),
                        operation: TimerOperationKind::Start,
                        duration_ms: Some(duration_ms),
                    }],
                );
            }
        }

        for wait_expression in &analyzed.waits {
            if let Some(target) = branch_completion_target.clone() {
                builder.add_transition(
                    branch_state.clone(),
                    target,
                    TransitionGuard::Condition {
                        expression: wait_expression.clone(),
                    },
                    analyzed.actions.clone(),
                    Vec::new(),
                );
            }
        }

        for (nested_parallel_index, nested_parallel) in analyzed.parallel_blocks.iter().enumerate()
        {
            build_parallel_block(
                builder,
                task,
                &format!(
                    "{step_name}__race_{}_branch_{}",
                    block_index + 1,
                    branch_index + 1
                ),
                &branch_state,
                nested_parallel_index,
                nested_parallel,
                branch_completion_target.clone(),
                task_initial_states,
                errors,
                analyzed.actions.clone(),
            );
        }

        for (nested_race_index, nested_race) in analyzed.race_blocks.iter().enumerate() {
            build_race_block(
                builder,
                task,
                &format!(
                    "{step_name}__race_{}_branch_{}",
                    block_index + 1,
                    branch_index + 1
                ),
                &branch_state,
                nested_race_index,
                nested_race,
                branch_completion_target.clone(),
                task_initial_states,
                errors,
                analyzed.actions.clone(),
            );
        }

        let has_control_flow = !analyzed.waits.is_empty()
            || !analyzed.delays_ms.is_empty()
            || !analyzed.gotos.is_empty()
            || !analyzed.parallel_blocks.is_empty()
            || !analyzed.race_blocks.is_empty();
        if !has_control_flow {
            if let Some(target) = branch_completion_target {
                builder.add_transition(
                    branch_state,
                    target,
                    TransitionGuard::Always,
                    analyzed.actions,
                    Vec::new(),
                );
            }
        }
    }
}

fn resolve_task_target(
    target_task: &str,
    line: usize,
    task_initial_states: &HashMap<String, State>,
    errors: &mut Vec<PlcError>,
    source: &str,
) -> Option<State> {
    let Some(state) = task_initial_states.get(target_task) else {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            " task",
            target_task,
            format!("{source} 目标必须是已定义 task 名称"),
        ));
        return None;
    };

    Some(state.clone())
}

fn action_to_transition_action(action: &ActionStatement) -> TransitionAction {
    match action {
        ActionStatement::Extend { target } => TransitionAction::Extend {
            target: target.clone(),
        },
        ActionStatement::Retract { target } => TransitionAction::Retract {
            target: target.clone(),
        },
        ActionStatement::Set { target, value } => TransitionAction::Set {
            target: target.clone(),
            value: match value {
                AstBinaryValue::On => IrBinaryValue::On,
                AstBinaryValue::Off => IrBinaryValue::Off,
            },
        },
        ActionStatement::Log { message } => TransitionAction::Log {
            message: message.clone(),
        },
    }
}

fn wait_to_guard_expression(wait: &WaitStatement) -> String {
    condition_to_expression(&wait.condition)
}

fn condition_to_expression(condition: &ConditionExpression) -> String {
    let operator = match condition.operator {
        ComparisonOperator::Eq => "==",
        ComparisonOperator::Neq => "!=",
    };

    format!(
        "{} {} {}",
        condition.left,
        operator,
        literal_to_expression(&condition.right)
    )
}

fn literal_to_expression(literal: &LiteralValue) -> String {
    match literal {
        LiteralValue::Boolean(value) => value.to_string(),
        LiteralValue::Number(value) => value.to_string(),
        LiteralValue::String(value) => format!("\"{}\"", value),
        LiteralValue::State(state) => format!("{}.{}", state.device, state.state),
    }
}

fn duration_to_ms(timeout: &TimeoutDirective) -> u64 {
    duration_value_to_ms(&timeout.duration)
}

fn duration_value_to_ms(duration: &DurationValue) -> u64 {
    match duration.unit {
        TimeUnit::Ms => duration.value,
        TimeUnit::S => duration.value.saturating_mul(1000),
    }
}

fn ast_type_to_ir_kind(device_type: &DeviceType) -> DeviceKind {
    match device_type {
        DeviceType::DigitalOutput => DeviceKind::DigitalOutput,
        DeviceType::DigitalInput => DeviceKind::DigitalInput,
        DeviceType::SolenoidValve => DeviceKind::SolenoidValve,
        DeviceType::Cylinder => DeviceKind::Cylinder,
        DeviceType::Sensor => DeviceKind::Sensor,
        DeviceType::Motor => DeviceKind::Motor,
    }
}

fn connection_type_for(from: &DeviceKind, to: &DeviceKind) -> Option<ConnectionType> {
    match (from, to) {
        (DeviceKind::DigitalOutput, DeviceKind::SolenoidValve)
        | (DeviceKind::DigitalOutput, DeviceKind::Motor)
        | (DeviceKind::DigitalInput, DeviceKind::Sensor) => Some(ConnectionType::Electrical),
        (DeviceKind::SolenoidValve, DeviceKind::Cylinder) => Some(ConnectionType::Pneumatic),
        (DeviceKind::DigitalInput, DeviceKind::DigitalInput)
        | (DeviceKind::DigitalOutput, DeviceKind::DigitalOutput) => Some(ConnectionType::Logical),
        _ => None,
    }
}

fn device_kind_name(kind: &DeviceKind) -> &'static str {
    match kind {
        DeviceKind::DigitalOutput => "digital_output",
        DeviceKind::DigitalInput => "digital_input",
        DeviceKind::SolenoidValve => "solenoid_valve",
        DeviceKind::Cylinder => "cylinder",
        DeviceKind::Sensor => "sensor",
        DeviceKind::Motor => "motor",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_constraint_set, build_state_machine, build_timing_model, build_topology_graph,
    };
    use crate::ir::{
        ConnectionType, SafetyRelation, TimerOperationKind, TimingRelation, TimingScope,
        TransitionGuard,
    };
    use crate::parser::parse_plc;
    use petgraph::visit::EdgeRef;

    #[test]
    fn builds_topology_graph_from_prd_5_3_topology() {
        let input = r#"
[topology]

# ===== controller ports =====
device Y0: digital_output
device Y1: digital_output
device Y2: digital_output
device X0: digital_input
device X1: digital_input
device X2: digital_input
device X3: digital_input
device X4: digital_input

# ===== operator panel =====
device start_button: digital_input {
    connected_to: X4,
    debounce: 20ms
}

device alarm_light: digital_output {
    connected_to: Y2
}

# ===== solenoid valves =====
device valve_A: solenoid_valve {
    type: "5/2",
    connected_to: Y0,
    response_time: 15ms
}

device valve_B: solenoid_valve {
    type: "5/2",
    connected_to: Y1,
    response_time: 15ms
}

# ===== cylinders =====
device cyl_A: cylinder {
    type: double_acting,
    connected_to: valve_A,
    stroke: 100mm,
    stroke_time: 200ms,
    retract_time: 180ms
}

device cyl_B: cylinder {
    type: double_acting,
    connected_to: valve_B,
    stroke: 150mm,
    stroke_time: 300ms,
    retract_time: 250ms
}

# ===== sensors =====
device sensor_A_ext: sensor {
    type: magnetic,
    connected_to: X0,
    detects: cyl_A.extended
}

device sensor_A_ret: sensor {
    type: magnetic,
    connected_to: X1,
    detects: cyl_A.retracted
}

device sensor_B_ext: sensor {
    type: magnetic,
    connected_to: X2,
    detects: cyl_B.extended
}

device sensor_B_ret: sensor {
    type: magnetic,
    connected_to: X3,
    detects: cyl_B.retracted
}

[constraints]

[tasks]
"#;

        let program = parse_plc(input).expect("PRD 5.3 示例应能成功解析为 AST");
        let topology = build_topology_graph(&program).expect("PRD 5.3 示例应能成功构建拓扑图");

        assert_eq!(topology.graph.node_count(), 18);
        assert_eq!(topology.graph.edge_count(), 10);

        let has_pneumatic_edge = topology.graph.edge_references().any(|edge| {
            let source = &topology.graph[edge.source()].name;
            let target = &topology.graph[edge.target()].name;
            source == "valve_A" && target == "cyl_A" && edge.weight() == &ConnectionType::Pneumatic
        });
        assert!(has_pneumatic_edge, "应包含 valve_A -> cyl_A 气路连接");

        let has_electrical_edge = topology.graph.edge_references().any(|edge| {
            let source = &topology.graph[edge.source()].name;
            let target = &topology.graph[edge.target()].name;
            source == "Y0" && target == "valve_A" && edge.weight() == &ConnectionType::Electrical
        });
        assert!(has_electrical_edge, "应包含 Y0 -> valve_A 电气连接");
    }

    #[test]
    fn reports_error_when_connected_to_references_undefined_device() {
        let input = r#"
[topology]
device Y0: digital_output

device valve_A: solenoid_valve {
    connected_to: Y9,
    response_time: 15ms
}

[constraints]

[tasks]
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let errors = build_topology_graph(&program).expect_err("未定义 connected_to 引用应报错");

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].line(), 5);
        assert!(
            errors[0].to_string().contains("未定义设备 Y9"),
            "错误消息应包含未定义设备名"
        );
    }

    #[test]
    fn reports_error_when_connection_types_are_incompatible() {
        let input = r#"
[topology]
device cyl_A: cylinder {
    connected_to: valve_A,
    stroke_time: 200ms,
    retract_time: 180ms
}

device valve_A: solenoid_valve {
    connected_to: Y0,
    response_time: 15ms
}

device sensor_bad: sensor {
    connected_to: cyl_A,
    detects: cyl_A.extended
}

device Y0: digital_output

[constraints]

[tasks]
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let errors = build_topology_graph(&program).expect_err("不兼容连接类型应报错");

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].line(), 14);
        assert!(
            errors[0].to_string().contains("sensor") && errors[0].to_string().contains("cylinder"),
            "错误消息应包含不兼容的设备类型"
        );
    }

    #[test]
    fn builds_constraint_set_and_timing_model_from_prd_5_4_example() {
        let input = r#"
[topology]

device Y0: digital_output
device Y1: digital_output
device motor_ctrl: motor {
    connected_to: Y0,
    ramp_time: 50ms
}

device valve_A: solenoid_valve {
    connected_to: Y0,
    response_time: 15ms
}

device valve_B: solenoid_valve {
    connected_to: Y1,
    response_time: 15ms
}

device cyl_A: cylinder {
    connected_to: valve_A,
    stroke_time: 200ms,
    retract_time: 180ms
}

device cyl_B: cylinder {
    connected_to: valve_B,
    stroke_time: 300ms,
    retract_time: 250ms
}

device sensor_A_ext: sensor {
    connected_to: Y0,
    detects: cyl_A.extended
}

device sensor_B_ext: sensor {
    connected_to: Y1,
    detects: cyl_B.extended
}

[constraints]

safety: cyl_A.extended conflicts_with cyl_B.extended
    reason: "A缸和B缸同时伸出会导致机械碰撞"

safety: valve_A.on conflicts_with valve_B.on
    reason: "气源压力不足以同时驱动两个阀"

timing: task.init must_complete_within 5000ms
    reason: "初始化超过5秒视为异常"

timing: task.init.step_extend_A must_complete_within 500ms
    reason: "单步动作不应超过500ms"

causality: Y0 -> valve_A -> cyl_A -> sensor_A_ext
    reason: "Y0 驱动 valve_A 推动 cyl_A 由 sensor_A_ext 检测"

causality: Y1 -> valve_B -> cyl_B -> sensor_B_ext
    reason: "Y1 驱动 valve_B 推动 cyl_B 由 sensor_B_ext 检测"

[tasks]

task init:
    step step_extend_A:
        action: extend cyl_A
    step step_retract_A:
        action: retract cyl_A

task ready:
    step start_motor:
        action: set motor_ctrl on
"#;

        let program = parse_plc(input).expect("PRD 5.4 示例应能成功解析为 AST");
        let constraints = build_constraint_set(&program).expect("应能构建约束集合");
        let timing_model = build_timing_model(&program).expect("应能构建设备时序模型");

        assert_eq!(constraints.safety.len(), 2);
        assert_eq!(constraints.timing.len(), 2);
        assert_eq!(constraints.causality.len(), 2);

        assert!(matches!(
            constraints.safety[0].relation,
            SafetyRelation::ConflictsWith
        ));
        assert_eq!(constraints.safety[0].left.device, "cyl_A");
        assert_eq!(constraints.safety[0].left.state, "extended");

        assert!(matches!(
            constraints.timing[0].scope,
            TimingScope::Task { ref task } if task == "init"
        ));
        assert!(matches!(
            constraints.timing[0].relation,
            TimingRelation::MustCompleteWithin
        ));
        assert_eq!(constraints.timing[0].duration_ms, 5000);

        assert!(matches!(
            constraints.timing[1].scope,
            TimingScope::Step { ref task, ref step } if task == "init" && step == "step_extend_A"
        ));
        assert_eq!(constraints.causality[0].devices.len(), 4);
        assert_eq!(constraints.causality[0].devices[0], "Y0");
        assert_eq!(constraints.causality[0].devices[3], "sensor_A_ext");

        let extend_key = "init.step_extend_A.extend.cyl_A";
        let retract_key = "init.step_retract_A.retract.cyl_A";
        let motor_key = "ready.start_motor.set.motor_ctrl";

        assert_eq!(timing_model.intervals[extend_key].interval.min_ms, 200);
        assert_eq!(timing_model.intervals[extend_key].interval.max_ms, 200);
        assert_eq!(timing_model.intervals[retract_key].interval.min_ms, 180);
        assert_eq!(timing_model.intervals[motor_key].interval.min_ms, 50);
    }

    #[test]
    fn reports_constraint_reference_errors_for_undefined_device_state_and_task() {
        let input = r#"
[topology]

device cyl_A: cylinder {
    stroke_time: 200ms,
    retract_time: 180ms
}

[constraints]

safety: cyl_A.invalid_state conflicts_with missing_device.on
timing: task.unknown must_complete_within 100ms
causality: cyl_A -> missing_device

[tasks]

task init:
    step start:
        action: extend cyl_A
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let errors = build_constraint_set(&program).expect_err("未定义引用应报错");

        assert_eq!(errors.len(), 4);
        assert!(
            errors
                .iter()
                .any(|err| err.to_string().contains("未定义状态 invalid_state")),
            "应报告未定义状态"
        );
        assert!(
            errors
                .iter()
                .any(|err| err.to_string().contains("未定义设备 missing_device")),
            "应报告未定义设备"
        );
        assert!(
            errors
                .iter()
                .any(|err| err.to_string().contains("未定义 task unknown")),
            "应报告未定义 task"
        );
    }

    #[test]
    fn builds_state_machine_from_prd_5_5_1_sequence_example() {
        let input = r#"
[topology]

[constraints]

[tasks]

task init:
    step extend_A:
        action: extend cyl_A
        wait: sensor_A_ext == true
        timeout: 600ms -> goto fault_handler

    step retract_A:
        action: retract cyl_A
        wait: sensor_A_ret == true
        timeout: 500ms -> goto fault_handler

    step extend_B:
        action: extend cyl_B
        wait: sensor_B_ext == true
        timeout: 800ms -> goto fault_handler

    step retract_B:
        action: retract cyl_B
        wait: sensor_B_ret == true
        timeout: 700ms -> goto fault_handler

    on_complete: goto ready

task fault_handler:
    step safe_position:
        action: retract cyl_A
    on_complete: goto ready

task ready:
    step wait_start:
        wait: start_button == true
        allow_indefinite_wait: true
    on_complete: goto init
"#;

        let program = parse_plc(input).expect("PRD 5.5.1 示例应能成功解析为 AST");
        let state_machine = build_state_machine(&program).expect("应能从 5.5.1 示例构建状态机");

        assert!(
            state_machine
                .states
                .iter()
                .any(|state| state.task_name == "init" && state.step_name == "extend_A")
        );
        assert!(
            state_machine
                .states
                .iter()
                .any(|state| state.task_name == "init" && state.step_name == "retract_B")
        );

        let has_wait_transition = state_machine.transitions.iter().any(|transition| {
            transition.from.task_name == "init"
                && transition.from.step_name == "extend_A"
                && transition.to.task_name == "init"
                && transition.to.step_name == "retract_A"
                && matches!(
                    transition.guard,
                    TransitionGuard::Condition { ref expression }
                        if expression == "sensor_A_ext == true"
                )
        });
        assert!(has_wait_transition, "应存在 wait 条件驱动的顺序转移");

        let has_timeout_transition = state_machine.transitions.iter().any(|transition| {
            transition.from.task_name == "init"
                && transition.from.step_name == "extend_A"
                && transition.to.task_name == "fault_handler"
                && transition.to.step_name == "safe_position"
                && matches!(
                    transition.guard,
                    TransitionGuard::Timeout { duration_ms } if duration_ms == 600
                )
        });
        assert!(has_timeout_transition, "timeout 应创建带定时守卫的跳转");

        let has_on_complete_goto = state_machine.transitions.iter().any(|transition| {
            transition.from.task_name == "init"
                && transition.from.step_name == "retract_B"
                && transition.to.task_name == "ready"
                && transition.to.step_name == "wait_start"
                && matches!(
                    transition.guard,
                    TransitionGuard::Condition { ref expression }
                        if expression == "sensor_B_ret == true"
                )
        });
        assert!(
            has_on_complete_goto,
            "最后一步应能够通过 on_complete 跳转到 ready"
        );
    }

    #[test]
    fn lowers_delay_statement_into_bounded_transition_to_next_step() {
        let input = r#"
[topology]

[constraints]

[tasks]

task init:
    step warmup:
        delay: 2000ms
    step work:
        action: log "start"
"#;

        let program = parse_plc(input).expect("测试输入应能成功解析为 AST");
        let state_machine = build_state_machine(&program).expect("delay 应能降级为状态机转移");

        let delay_transition = state_machine
            .transitions
            .iter()
            .find(|transition| {
                transition.from.task_name == "init"
                    && transition.from.step_name == "warmup"
                    && transition.to.task_name == "init"
                    && transition.to.step_name == "work"
                    && matches!(transition.guard, TransitionGuard::Delay { duration_ms } if duration_ms == 2000)
            })
            .expect("delay 应生成到下一个 step 的有界等待转移");

        assert!(
            delay_transition.actions.is_empty(),
            "delay 转移不应重复执行动作"
        );
        assert_eq!(delay_transition.timers.len(), 1);
        assert_eq!(
            delay_transition.timers[0].operation,
            TimerOperationKind::Start
        );
        assert_eq!(delay_transition.timers[0].duration_ms, Some(2000));
    }

    #[test]
    fn keeps_timeout_as_protective_upper_bound_when_delay_and_timeout_coexist() {
        let input = r#"
[topology]

[constraints]

[tasks]

task init:
    step wait_heat:
        delay: 300ms
        timeout: 1200ms -> goto fault_handler
    step run:
        action: log "running"

task fault_handler:
    step safe_stop:
        action: log "fault"
"#;

        let program = parse_plc(input).expect("测试输入应能成功解析为 AST");
        let state_machine = build_state_machine(&program).expect("delay + timeout 应可共存");

        let has_delay_to_next = state_machine.transitions.iter().any(|transition| {
            transition.from.task_name == "init"
                && transition.from.step_name == "wait_heat"
                && transition.to.task_name == "init"
                && transition.to.step_name == "run"
                && matches!(transition.guard, TransitionGuard::Delay { duration_ms } if duration_ms == 300)
        });
        assert!(has_delay_to_next, "delay 应指向当前 task 的下一个 step");

        let has_timeout_escape = state_machine.transitions.iter().any(|transition| {
            transition.from.task_name == "init"
                && transition.from.step_name == "wait_heat"
                && transition.to.task_name == "fault_handler"
                && transition.to.step_name == "safe_stop"
                && matches!(transition.guard, TransitionGuard::Timeout { duration_ms } if duration_ms == 1200)
        });
        assert!(has_timeout_escape, "timeout 应保留为保护性上界跳转");
    }

    #[test]
    fn builds_state_machine_race_branches_from_prd_9_example() {
        let input = r#"
[topology]

[constraints]

[tasks]

task search:
    step start_motor:
        action: set motor_ctrl on
    step detect:
        race:
            branch_A:
                wait: sensor_A == true
                then: goto process_A
            branch_B:
                wait: sensor_B == true
                then: goto process_B
        timeout: 800ms -> goto motor_fault

task process_A:
    step stop_motor:
        action: set motor_ctrl off
    on_complete: goto ready

task process_B:
    step stop_motor:
        action: set motor_ctrl off
    on_complete: goto ready

task motor_fault:
    step emergency_stop:
        action: set motor_ctrl off
    on_complete: goto ready

task ready:
    step wait_start:
        wait: start_button == true
        allow_indefinite_wait: true
    on_complete: goto search
"#;

        let program = parse_plc(input).expect("PRD 9 示例应能成功解析为 AST");
        let state_machine = build_state_machine(&program).expect("应能构建 race 状态机");

        assert!(state_machine.states.iter().any(
            |state| state.task_name == "search" && state.step_name == "detect__race_1_decision"
        ));
        assert!(state_machine.states.iter().any(
            |state| state.task_name == "search" && state.step_name == "detect__race_1_branch_1"
        ));

        let has_branch_a_transition = state_machine.transitions.iter().any(|transition| {
            transition.from.task_name == "search"
                && transition.from.step_name == "detect__race_1_branch_1"
                && transition.to.task_name == "process_A"
                && transition.to.step_name == "stop_motor"
                && matches!(
                    transition.guard,
                    TransitionGuard::Condition { ref expression }
                        if expression == "sensor_A == true"
                )
        });
        assert!(has_branch_a_transition, "race 分支 A 应创建条件跳转");

        let has_branch_b_transition = state_machine.transitions.iter().any(|transition| {
            transition.from.task_name == "search"
                && transition.from.step_name == "detect__race_1_branch_2"
                && transition.to.task_name == "process_B"
                && transition.to.step_name == "stop_motor"
                && matches!(
                    transition.guard,
                    TransitionGuard::Condition { ref expression }
                        if expression == "sensor_B == true"
                )
        });
        assert!(has_branch_b_transition, "race 分支 B 应创建条件跳转");

        let has_timeout_transition = state_machine.transitions.iter().any(|transition| {
            transition.from.task_name == "search"
                && transition.from.step_name == "detect"
                && transition.to.task_name == "motor_fault"
                && transition.to.step_name == "emergency_stop"
                && matches!(
                    transition.guard,
                    TransitionGuard::Timeout { duration_ms } if duration_ms == 800
                )
        });
        assert!(
            has_timeout_transition,
            "race 所在 step 应保留 timeout 守卫跳转"
        );
    }

    #[test]
    fn reports_undefined_goto_target_with_line_number() {
        let input = r#"
[topology]

[constraints]

[tasks]

task init:
    step start:
        goto missing_task
"#;

        let program = parse_plc(input).expect("测试输入应能成功解析为 AST");
        let errors = build_state_machine(&program).expect_err("未定义 goto 目标应返回语义错误");

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].line(), 10);
        assert!(
            errors[0].to_string().contains("未定义 task missing_task"),
            "错误消息应包含未定义 task 名称"
        );
    }
}
