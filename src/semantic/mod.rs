use crate::ast::{
    ActionStatement, BinaryValue as AstBinaryValue, ComparisonOperator, ConditionExpression,
    ConstraintsSection, DeviceType, DurationValue, GotoDirective, LiteralValue,
    OnCompleteDirective, ParallelBlock, PlcProgram, RaceBlock, SafetyOperand,
    SafetyRelation as AstSafetyRelation, StepStatement, TaskDeclaration, TasksSection, TimeUnit,
    TimeoutDirective, TimingRelation as AstTimingRelation, TimingTarget, TopologySection,
    WaitCondition, WaitStatement,
};
use crate::error::PlcError;
use crate::ir::{
    ActionKind, ActionRef, ActionTiming, BinaryValue as IrBinaryValue, CausalityChain,
    ConnectionType, ConstraintSet, Device, DeviceKind, PidLoop as IrPidLoop, SafetyExpr,
    SafetyRelation as IrSafetyRelation, SafetyRule, State, StateExpr, StateMachine, TimeInterval,
    TimerOperation, TimerOperationKind, TimingModel,
    TimingRelation as IrTimingRelation, TimingRule, TimingScope, TopologyGraph, Transition,
    TransitionAction, TransitionGuard,
};
use petgraph::graph::NodeIndex;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone)]
struct DeviceNode {
    index: NodeIndex,
    kind: DeviceKind,
}

/// Expand syntax sugar in the AST before semantic lowering.
///
/// Currently this performs compile-time `repeat N:` expansion by rewriting it into `N` sequential
/// steps named with `_1.._N` suffixes.
pub fn preprocess_program(program: &PlcProgram) -> Result<PlcProgram, Vec<PlcError>> {
    let expanded_tasks = expand_repeat_blocks(&program.tasks)?;
    let mut rewritten = program.clone();
    rewritten.tasks = expanded_tasks;
    Ok(rewritten)
}

fn expand_repeat_blocks(tasks: &TasksSection) -> Result<TasksSection, Vec<PlcError>> {
    let mut rewritten_tasks = Vec::new();
    let mut errors = Vec::new();

    for task in &tasks.tasks {
        let mut expanded_steps = Vec::new();

        for step in &task.steps {
            let top_level_repeat_indices = step
                .statements
                .iter()
                .enumerate()
                .filter_map(|(idx, statement)| match statement {
                    StepStatement::Repeat { .. } => Some(idx),
                    _ => None,
                })
                .collect::<Vec<_>>();

            // Reject repeat blocks that appear in nested statement contexts (e.g., parallel/race).
            for statement in &step.statements {
                if contains_nested_repeat(statement) {
                    errors.push(PlcError::semantic(
                        step.line.max(1),
                        format!(
                            "step {}.{} 的 repeat 只能写在 step 顶层，不能嵌套在 parallel/race 等块内",
                            task.name, step.name
                        ),
                    ));
                    break;
                }
            }

            match top_level_repeat_indices.len() {
                0 => expanded_steps.push(step.clone()),
                1 => {
                    let repeat_index = top_level_repeat_indices[0];
                    let (prefix, repeat_statement, suffix) = split_repeat_step(step, repeat_index);

                    let StepStatement::Repeat { count, body } = repeat_statement else {
                        // split_repeat_step guarantees this index points at a repeat.
                        expanded_steps.push(step.clone());
                        continue;
                    };

                    if *count <= 1 {
                        errors.push(PlcError::semantic(
                            step.line.max(1),
                            format!(
                                "repeat 次数必须在 2..=100 之间，当前为 {count}（step {}.{}）",
                                task.name, step.name
                            ),
                        ));
                        continue;
                    }

                    if *count > 100 {
                        errors.push(PlcError::semantic(
                            step.line.max(1),
                            format!(
                                "repeat 次数超过上限 100，当前为 {count}（step {}.{}）",
                                task.name, step.name
                            ),
                        ));
                        continue;
                    }

                    if body.iter().any(statement_contains_repeat) {
                        errors.push(PlcError::semantic(
                            step.line.max(1),
                            format!(
                                "repeat 块内不允许嵌套 repeat（step {}.{}）",
                                task.name, step.name
                            ),
                        ));
                        continue;
                    }

                    for iteration in 1..=(*count as usize) {
                        let mut statements = Vec::new();
                        if iteration == 1 {
                            statements.extend_from_slice(prefix);
                        }
                        statements.extend(body.clone());
                        if iteration == *count as usize {
                            statements.extend_from_slice(suffix);
                        }

                        expanded_steps.push(crate::ast::StepDeclaration {
                            line: step.line,
                            name: format!("{}_{}", step.name, iteration),
                            statements,
                        });
                    }
                }
                _ => {
                    errors.push(PlcError::semantic(
                        step.line.max(1),
                        format!(
                            "step {}.{} 同时包含多个 repeat 块，当前版本只支持一个 repeat",
                            task.name, step.name
                        ),
                    ));
                }
            }
        }

        // Ensure step names remain unique inside the task after expansion.
        let mut seen = HashSet::<String>::new();
        for step in &expanded_steps {
            if !seen.insert(step.name.clone()) {
                errors.push(PlcError::duplicate_definition_with_reason(
                    step.line.max(1),
                    "step",
                    &format!("{}.{}", task.name, step.name),
                    "repeat 展开后产生了重复 step 名称，请重命名原始 step 或调整 repeat 使用方式",
                ));
            }
        }

        let mut rewritten_task = task.clone();
        rewritten_task.steps = expanded_steps;
        rewritten_tasks.push(rewritten_task);
    }

    if errors.is_empty() {
        Ok(TasksSection {
            tasks: rewritten_tasks,
        })
    } else {
        Err(errors)
    }
}

fn split_repeat_step(
    step: &crate::ast::StepDeclaration,
    repeat_index: usize,
) -> (&[StepStatement], &StepStatement, &[StepStatement]) {
    let prefix = &step.statements[..repeat_index];
    let repeat_statement = &step.statements[repeat_index];
    let suffix = &step.statements[repeat_index + 1..];
    (prefix, repeat_statement, suffix)
}

fn contains_nested_repeat(statement: &StepStatement) -> bool {
    match statement {
        // Top-level repeats are handled separately; nested repeats are rejected.
        StepStatement::Repeat { .. } => false,
        StepStatement::Parallel(block) => block
            .branches
            .iter()
            .any(|branch| branch.statements.iter().any(statement_contains_repeat)),
        StepStatement::Race(block) => block
            .branches
            .iter()
            .any(|branch| branch.statements.iter().any(statement_contains_repeat)),
        StepStatement::Action(_)
        | StepStatement::Wait(_)
        | StepStatement::IfElse { .. }
        | StepStatement::Delay { .. }
        | StepStatement::Timeout(_)
        | StepStatement::Goto(_)
        | StepStatement::AllowIndefiniteWait(_) => false,
    }
}

fn statement_contains_repeat(statement: &StepStatement) -> bool {
    match statement {
        StepStatement::Repeat { .. } => true,
        StepStatement::Parallel(block) => block
            .branches
            .iter()
            .any(|branch| branch.statements.iter().any(statement_contains_repeat)),
        StepStatement::Race(block) => block
            .branches
            .iter()
            .any(|branch| branch.statements.iter().any(statement_contains_repeat)),
        StepStatement::Action(_)
        | StepStatement::Wait(_)
        | StepStatement::IfElse { .. }
        | StepStatement::Delay { .. }
        | StepStatement::Timeout(_)
        | StepStatement::Goto(_)
        | StepStatement::AllowIndefiniteWait(_) => false,
    }
}

pub fn build_topology_graph(program: &PlcProgram) -> Result<TopologyGraph, Vec<PlcError>> {
    build_topology_from_ast(&program.topology)
}

pub fn build_state_machine(program: &PlcProgram) -> Result<StateMachine, Vec<PlcError>> {
    let expanded = preprocess_program(program)?;
    let wait_ctx = WaitExpressionContext::for_program(&expanded);
    build_state_machine_from_ast_with_context(&expanded.tasks, &wait_ctx)
}

pub fn build_constraint_set(program: &PlcProgram) -> Result<ConstraintSet, Vec<PlcError>> {
    let expanded = preprocess_program(program)?;
    build_constraint_set_from_ast(&expanded.topology, &expanded.constraints, &expanded.tasks)
}

pub fn build_timing_model(program: &PlcProgram) -> Result<TimingModel, Vec<PlcError>> {
    let expanded = preprocess_program(program)?;
    build_timing_model_from_ast(&expanded.topology, &expanded.tasks)
}

#[derive(Debug, Clone, Default)]
struct WaitExpressionContext {
    analog_input_regions: HashMap<String, Vec<(f64, f64)>>,
}

impl WaitExpressionContext {
    fn for_program(program: &PlcProgram) -> Self {
        Self {
            analog_input_regions: compute_analog_input_regions(program),
        }
    }
}

fn compute_analog_input_regions(program: &PlcProgram) -> HashMap<String, Vec<(f64, f64)>> {
    let mut values_by_device: HashMap<String, Vec<f64>> = HashMap::new();

    for constraint in &program.constraints.safety {
        for operand in [&constraint.left, &constraint.right] {
            if let SafetyOperand::Threshold { device, value, .. } = operand {
                values_by_device
                    .entry(device.clone())
                    .or_default()
                    .push(*value);
            }
        }
    }

    for task in &program.tasks.tasks {
        for step in &task.steps {
            collect_threshold_values_from_statements(&step.statements, &mut values_by_device);
        }
    }

    let mut regions_by_device = HashMap::new();
    for device in &program.topology.devices {
        if !matches!(device.device_type, DeviceType::AnalogInput) {
            continue;
        }

        let Some(range) = &device.attributes.range else {
            continue;
        };

        let (min, max) = if range.min <= range.max {
            (range.min, range.max)
        } else {
            (range.max, range.min)
        };

        let mut bounds = vec![min, max];
        if let Some(values) = values_by_device.get(&device.name) {
            for value in values {
                if *value >= min && *value <= max {
                    bounds.push(*value);
                }
            }
        }

        bounds.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        bounds.dedup_by(|a, b| (*a - *b).abs() <= f64::EPSILON);

        let mut regions = Vec::new();
        for window in bounds.windows(2) {
            regions.push((window[0], window[1]));
        }
        if regions.is_empty() {
            regions.push((min, max));
        }

        regions_by_device.insert(device.name.clone(), regions);
    }

    regions_by_device
}

fn collect_threshold_values_from_statements(
    statements: &[StepStatement],
    values_by_device: &mut HashMap<String, Vec<f64>>,
) {
    for statement in statements {
        match statement {
            StepStatement::Wait(wait) => {
                let terms: Vec<&ConditionExpression> = match &wait.condition {
                    WaitCondition::Single(condition) => vec![condition],
                    WaitCondition::And(conditions) | WaitCondition::Or(conditions) => {
                        conditions.iter().collect()
                    }
                };

                for condition in terms {
                    if let LiteralValue::Number(value) = &condition.right {
                        if let Some(device_name) = wait_operand_device_name(&condition.left) {
                            values_by_device
                                .entry(device_name.to_string())
                                .or_default()
                                .push(*value);
                        }
                    }
                    if let LiteralValue::Measured(measured) = &condition.right {
                        if let Some(device_name) = wait_operand_device_name(&condition.left) {
                            values_by_device
                                .entry(device_name.to_string())
                                .or_default()
                                .push(measured.value);
                        }
                    }
                }
            }
            StepStatement::Repeat { body, .. } => {
                collect_threshold_values_from_statements(body, values_by_device);
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    collect_threshold_values_from_statements(&branch.statements, values_by_device);
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    collect_threshold_values_from_statements(&branch.statements, values_by_device);
                }
            }
            _ => {}
        }
    }
}

pub fn build_topology_from_ast(topology: &TopologySection) -> Result<TopologyGraph, Vec<PlcError>> {
    let mut topology_graph = TopologyGraph::new();
    let mut device_nodes = HashMap::<String, DeviceNode>::new();
    let mut errors = Vec::new();
    let pid_loops = extract_pid_loops(topology, &mut errors);

    for device in &topology.devices {
        let kind = ast_type_to_ir_kind(&device.device_type);
        let index = topology_graph.add_device(Device {
            name: device.name.clone(),
            kind: kind.clone(),
        });

        device_nodes.insert(device.name.clone(), DeviceNode { index, kind });

        // Analog devices must declare range
        if matches!(
            device.device_type,
            DeviceType::AnalogInput | DeviceType::AnalogOutput
        ) && device.attributes.range.is_none()
        {
            errors.push(PlcError::semantic_with_reason(
                device.line,
                format!("模拟量设备 {} 必须声明 range 属性", device.name),
                "请添加 range: min..max 属性，例如 range: 0..100",
            ));
        }
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

    topology_graph.pid_loops = pid_loops;

    if errors.is_empty() {
        Ok(topology_graph)
    } else {
        Err(errors)
    }
}

fn extract_pid_loops(topology: &TopologySection, errors: &mut Vec<PlcError>) -> Vec<IrPidLoop> {
    let device_ranges = collect_device_ranges(topology);
    let device_units = collect_device_units(topology);
    let analog_inputs = topology
        .devices
        .iter()
        .filter(|d| matches!(d.device_type, DeviceType::AnalogInput))
        .map(|d| d.name.as_str())
        .collect::<HashSet<_>>();
    let analog_outputs = topology
        .devices
        .iter()
        .filter(|d| matches!(d.device_type, DeviceType::AnalogOutput))
        .map(|d| d.name.as_str())
        .collect::<HashSet<_>>();

    let mut pid_loops = Vec::new();
    for device in &topology.devices {
        if !matches!(device.device_type, DeviceType::Pid) {
            continue;
        }
        let line = device.line.max(1);
        let Some(pv) = device.attributes.pv.as_ref() else {
            errors.push(PlcError::semantic(line, format!("PID {} 缺少 pv 属性", device.name)));
            continue;
        };
        let Some(sp) = device.attributes.sp.as_ref() else {
            errors.push(PlcError::semantic(line, format!("PID {} 缺少 sp 属性", device.name)));
            continue;
        };
        let Some(sp_numeric) = format_numeric_literal_from_literal(sp) else {
            errors.push(PlcError::semantic(
                line,
                format!("PID {} 的 sp 必须是 number 或 measured_value", device.name),
            ));
            continue;
        };
        let Some(kp) = device.attributes.kp else {
            errors.push(PlcError::semantic(line, format!("PID {} 缺少 kp 属性", device.name)));
            continue;
        };
        let Some(ki) = device.attributes.ki else {
            errors.push(PlcError::semantic(line, format!("PID {} 缺少 ki 属性", device.name)));
            continue;
        };
        let Some(kd) = device.attributes.kd else {
            errors.push(PlcError::semantic(line, format!("PID {} 缺少 kd 属性", device.name)));
            continue;
        };
        let Some(out) = device.attributes.out.as_ref() else {
            errors.push(PlcError::semantic(line, format!("PID {} 缺少 out 属性", device.name)));
            continue;
        };
        let Some(period_ms) = device.attributes.period_ms else {
            errors.push(PlcError::semantic(
                line,
                format!("PID {} 缺少 period_ms 属性", device.name),
            ));
            continue;
        };
        let Some(limit) = device.attributes.limit.as_ref() else {
            errors.push(PlcError::semantic(
                line,
                format!("PID {} 缺少 limit 属性", device.name),
            ));
            continue;
        };
        if period_ms == 0 {
            errors.push(PlcError::semantic(
                line,
                format!("PID {} 的 period_ms 必须 > 0", device.name),
            ));
        }
        if !analog_inputs.contains(pv.as_str()) {
            errors.push(PlcError::semantic(
                line,
                format!("PID {} 的 pv={} 不是 analog_input", device.name, pv),
            ));
        }
        if !analog_outputs.contains(out.as_str()) {
            errors.push(PlcError::semantic(
                line,
                format!("PID {} 的 out={} 不是 analog_output", device.name, out),
            ));
        }

        let (limit_min, limit_max) = if limit.min <= limit.max {
            (limit.min, limit.max)
        } else {
            (limit.max, limit.min)
        };

        if let Some((out_min, out_max)) = device_ranges.get(out).copied() {
            if limit_min < out_min || limit_max > out_max {
                errors.push(PlcError::semantic_with_reason(
                    line,
                    format!(
                        "PID {} 的 limit {}..{} 超出了输出 {} 的 range {}..{}",
                        device.name, limit_min, limit_max, out, out_min, out_max
                    ),
                    "请将 limit 约束在 analog_output 的 range 之内（或调整输出 range）",
                ));
            }
        }

        // If pv declares a unit and sp is measured, require them to match.
        if let Some(pv_unit) = device_units.get(pv) {
            if let LiteralValue::Measured(measured) = sp {
                if measured.unit != *pv_unit {
                    errors.push(PlcError::semantic_with_reason(
                        line,
                        format!(
                            "PID {} 的 sp 单位 {} 与 pv={} 单位 {} 不一致",
                            device.name, measured.unit, pv, pv_unit
                        ),
                        "请确保 sp 与 pv 使用相同 unit（或调整 pv 的 unit）",
                    ));
                }
            }
        }

        pid_loops.push(IrPidLoop {
            name: device.name.clone(),
            pv: pv.clone(),
            sp: sp_numeric,
            kp: format_numeric_literal(kp),
            ki: format_numeric_literal(ki),
            kd: format_numeric_literal(kd),
            out: out.clone(),
            period_ms,
            limit_min: format_numeric_literal(limit_min),
            limit_max: format_numeric_literal(limit_max),
            anti_windup: "conditional_integration".to_string(),
        });
    }
    pid_loops
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
    let device_ranges = collect_device_ranges(topology);
    let device_units = collect_device_units(topology);

    for safety in &constraints.safety {
        validate_safety_operand(
            &safety.left,
            safety.line,
            "safety 左侧",
            &device_kinds,
            &known_states,
            &device_ranges,
            &device_units,
            &mut errors,
        );
        validate_safety_operand(
            &safety.right,
            safety.line,
            "safety 右侧",
            &device_kinds,
            &known_states,
            &device_ranges,
            &device_units,
            &mut errors,
        );

        constraint_set.safety.push(SafetyRule {
            left: map_safety_operand(&safety.left),
            relation: map_safety_relation(&safety.relation),
            right: map_safety_operand(&safety.right),
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

    for task in &tasks.tasks {
        for step in &task.steps {
            validate_wait_device_references_in_statements(
                &step.statements,
                step.line.max(1),
                &device_kinds,
                &device_ranges,
                &device_units,
                &mut errors,
            );
            validate_analog_actions_in_statements(
                &step.statements,
                step.line.max(1),
                &device_kinds,
                &device_ranges,
                &mut errors,
            );
        }
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
    build_state_machine_from_ast_with_context(tasks, &WaitExpressionContext::default())
}

fn build_state_machine_from_ast_with_context(
    tasks: &TasksSection,
    wait_ctx: &WaitExpressionContext,
) -> Result<StateMachine, Vec<PlcError>> {
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

    let task_defined_steps = collect_task_steps(tasks);

    let mut task_on_complete_targets = HashMap::<String, Option<State>>::new();
    for task in &tasks.tasks {
        let on_complete_target = match &task.on_complete {
            Some(OnCompleteDirective::Goto { target }) => resolve_task_target(
                target,
                &task_initial_states,
                &task_defined_steps,
                &mut errors,
                "on_complete",
            ),
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

            let analyzed = analyze_statements(&step.statements, wait_ctx);

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
                    &task_defined_steps,
                    &mut errors,
                    analyzed.actions.clone(),
                    wait_ctx,
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
                    &task_defined_steps,
                    &mut errors,
                    analyzed.actions.clone(),
                    wait_ctx,
                );
            }

            for goto in &analyzed.gotos {
                if let Some(target) = resolve_task_target(
                    goto,
                    &task_initial_states,
                    &task_defined_steps,
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

            for if_else in &analyzed.if_elses {
                let expr = condition_to_expression(&if_else.condition);

                if let Some(then_target) = resolve_task_target(
                    &if_else.then_goto,
                    &task_initial_states,
                    &task_defined_steps,
                    &mut errors,
                    "if/else then goto",
                ) {
                    builder.add_transition(
                        from_state.clone(),
                        then_target,
                        TransitionGuard::Condition {
                            expression: expr.clone(),
                        },
                        analyzed.actions.clone(),
                        Vec::new(),
                    );
                }

                if let Some(else_target) = resolve_task_target(
                    &if_else.else_goto,
                    &task_initial_states,
                    &task_defined_steps,
                    &mut errors,
                    "if/else else goto",
                ) {
                    builder.add_transition(
                        from_state.clone(),
                        else_target,
                        TransitionGuard::Condition {
                            expression: format!("NOT({expr})"),
                        },
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
                    &timeout.target,
                    &task_initial_states,
                    &task_defined_steps,
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
                || !analyzed.if_elses.is_empty()
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
        let analog_regions = wait_ctx
            .analog_input_regions
            .iter()
            .map(|(device, regions)| {
                (
                    device.clone(),
                    regions
                        .iter()
                        .map(|(min, max)| {
                            (format_numeric_literal(*min), format_numeric_literal(*max))
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        Ok(StateMachine {
            states: builder.states,
            transitions: builder.transitions,
            initial,
            analog_regions,
        })
    } else {
        Err(errors)
    }
}

fn format_numeric_literal(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.1}")
    } else {
        v.to_string()
    }
}

fn format_numeric_literal_from_literal(literal: &LiteralValue) -> Option<String> {
    match literal {
        LiteralValue::Number(v) => Some(format_numeric_literal(*v)),
        LiteralValue::Measured(measured) => Some(format_numeric_literal(measured.value)),
        LiteralValue::Boolean(_)
        | LiteralValue::String(_)
        | LiteralValue::State(_) => None,
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
    if_elses: Vec<IfElseSpec>,
    parallel_blocks: Vec<ParallelBlock>,
    race_blocks: Vec<RaceBlock>,
}

#[derive(Debug, Clone)]
struct IfElseSpec {
    condition: ConditionExpression,
    then_goto: GotoDirective,
    else_goto: GotoDirective,
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

    for device in &topology.devices {
        let Some(kind) = device_kinds.get(&device.name) else {
            continue;
        };

        let mut states = HashSet::new();
        if let Some(custom_states) = &device.attributes.custom_states {
            if custom_states.len() > 8 {
                eprintln!(
                    "WARNING [semantic] 设备 {} 声明了 {} 个 states（> 8），请确认状态空间规模合理",
                    device.name,
                    custom_states.len()
                );
            }

            for state in custom_states {
                states.insert(state.clone());
            }
        } else {
            for state in default_states_for_kind(kind) {
                states.insert(state.to_string());
            }
        }

        known_states.insert(device.name.clone(), states);
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

fn collect_device_ranges(topology: &TopologySection) -> HashMap<String, (f64, f64)> {
    topology
        .devices
        .iter()
        .filter_map(|device| {
            device.attributes.range.as_ref().map(|r| {
                let (min, max) = if r.min <= r.max { (r.min, r.max) } else { (r.max, r.min) };
                (device.name.clone(), (min, max))
            })
        })
        .collect()
}

fn collect_device_units(topology: &TopologySection) -> HashMap<String, String> {
    topology
        .devices
        .iter()
        .filter_map(|device| {
            device
                .attributes
                .unit
                .as_ref()
                .map(|unit| (device.name.clone(), unit.clone()))
        })
        .collect()
}

fn validate_analog_actions_in_statements(
    statements: &[StepStatement],
    line: usize,
    device_kinds: &HashMap<String, DeviceKind>,
    device_ranges: &HashMap<String, (f64, f64)>,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::SetAnalog { target, value }) => {
                if let Some(kind) = device_kinds.get(target) {
                    if *kind != DeviceKind::AnalogOutput && *kind != DeviceKind::Motor {
                        errors.push(PlcError::type_mismatch_with_reason(
                            line,
                            "analog_output 或 motor",
                            device_kind_name(kind),
                            format!("set_analog {target}"),
                            "set_analog 只能用于 analog_output 或 motor 类型设备",
                        ));
                    }
                }
                if let Some((min, max)) = device_ranges.get(target) {
                    if *value < *min || *value > *max {
                        errors.push(PlcError::semantic_with_reason(
                            line,
                            format!("set_analog {target} {value} 超出声明范围 {min}..{max}",),
                            "请确保 set_analog 值在设备声明的 range 范围内",
                        ));
                    }
                }
            }
            StepStatement::Action(ActionStatement::Set { target, .. }) => {
                if let Some(kind) = device_kinds.get(target) {
                    if *kind == DeviceKind::AnalogOutput || *kind == DeviceKind::AnalogInput {
                        errors.push(PlcError::type_mismatch_with_reason(
                            line,
                            "digital_output 或 solenoid_valve 等离散设备",
                            device_kind_name(kind),
                            format!("set {target} on/off"),
                            "模拟量设备请使用 set_analog 指令",
                        ));
                    }
                }
            }
            StepStatement::Repeat { body, .. } => {
                validate_analog_actions_in_statements(
                    body,
                    line,
                    device_kinds,
                    device_ranges,
                    errors,
                );
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_analog_actions_in_statements(
                        &branch.statements,
                        line,
                        device_kinds,
                        device_ranges,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_analog_actions_in_statements(
                        &branch.statements,
                        line,
                        device_kinds,
                        device_ranges,
                        errors,
                    );
                }
            }
            _ => {}
        }
    }
}

fn validate_wait_device_references_in_statements(
    statements: &[StepStatement],
    line: usize,
    device_kinds: &HashMap<String, DeviceKind>,
    device_ranges: &HashMap<String, (f64, f64)>,
    device_units: &HashMap<String, String>,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Wait(wait) => {
                let should_validate_references =
                    matches!(wait.condition, WaitCondition::And(_) | WaitCondition::Or(_));
                for condition in wait_condition_terms(&wait.condition) {
                    if should_validate_references {
                        validate_wait_operand_device(
                            &condition.left,
                            line,
                            "wait 条件左值",
                            device_kinds,
                            errors,
                        );
                        if let LiteralValue::State(state) = &condition.right {
                            validate_device_reference(
                                &state.device,
                                line,
                                "wait 条件右值",
                                device_kinds,
                                errors,
                            );
                        }
                    }
                    if let Some((value, unit)) = threshold_literal_value_and_unit(&condition.right) {
                        if let Some(device) = wait_operand_device_name(&condition.left) {
                            validate_analog_threshold_comparison(
                                device,
                                value,
                                unit,
                                line,
                                "wait 条件阈值比较",
                                device_kinds,
                                device_ranges,
                                device_units,
                                errors,
                            );
                        }
                    }
                }
            }
            StepStatement::IfElse { .. } => {}
            StepStatement::Repeat { body, .. } => {
                validate_wait_device_references_in_statements(
                    body,
                    line,
                    device_kinds,
                    device_ranges,
                    device_units,
                    errors,
                );
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_wait_device_references_in_statements(
                        &branch.statements,
                        line,
                        device_kinds,
                        device_ranges,
                        device_units,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_wait_device_references_in_statements(
                        &branch.statements,
                        line,
                        device_kinds,
                        device_ranges,
                        device_units,
                        errors,
                    );
                }
            }
            StepStatement::Action(_)
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
}

fn wait_condition_terms(condition: &WaitCondition) -> Vec<&ConditionExpression> {
    match condition {
        WaitCondition::Single(term) => vec![term],
        WaitCondition::And(terms) | WaitCondition::Or(terms) => terms.iter().collect(),
    }
}

fn validate_wait_operand_device(
    operand: &str,
    line: usize,
    source: &str,
    device_kinds: &HashMap<String, DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    if let Some(candidate) = wait_operand_device_name(operand) {
        validate_device_reference(candidate, line, source, device_kinds, errors);
    }
}

fn wait_operand_device_name(operand: &str) -> Option<&str> {
    let candidate = operand.split('.').next().unwrap_or(operand).trim();

    if candidate.is_empty() {
        None
    } else {
        Some(candidate)
    }
}

fn map_safety_relation(relation: &AstSafetyRelation) -> IrSafetyRelation {
    match relation {
        AstSafetyRelation::ConflictsWith => IrSafetyRelation::ConflictsWith,
        AstSafetyRelation::Requires => IrSafetyRelation::Requires,
    }
}

fn validate_safety_operand(
    operand: &SafetyOperand,
    line: usize,
    source: &str,
    device_kinds: &HashMap<String, DeviceKind>,
    known_states: &HashMap<String, HashSet<String>>,
    device_ranges: &HashMap<String, (f64, f64)>,
    device_units: &HashMap<String, String>,
    errors: &mut Vec<PlcError>,
) {
    match operand {
        SafetyOperand::State(state_ref) => {
            validate_state_reference(state_ref, line, source, device_kinds, known_states, errors);
        }
        SafetyOperand::Threshold {
            device,
            value,
            unit,
            ..
        } => {
            validate_device_reference(device, line, source, device_kinds, errors);
            validate_analog_threshold_comparison(
                device,
                *value,
                unit.as_deref(),
                line,
                "safety 阈值比较",
                device_kinds,
                device_ranges,
                device_units,
                errors,
            );
        }
    }
}

fn validate_analog_threshold_comparison(
    device: &str,
    value: f64,
    value_unit: Option<&str>,
    line: usize,
    source: &str,
    device_kinds: &HashMap<String, DeviceKind>,
    device_ranges: &HashMap<String, (f64, f64)>,
    device_units: &HashMap<String, String>,
    errors: &mut Vec<PlcError>,
) {
    let Some(kind) = device_kinds.get(device) else {
        return;
    };

    if *kind != DeviceKind::AnalogInput {
        errors.push(PlcError::type_mismatch_with_reason(
            line,
            "analog_input",
            device_kind_name(kind),
            format!("{source} {device}"),
            "阈值比较仅支持 analog_input 设备",
        ));
        return;
    }

    let Some((min, max)) = device_ranges.get(device) else {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!("模拟量输入 {device} 缺少 range，无法进行阈值比较"),
            "请在 [topology] 段为该设备声明 range: min..max",
        ));
        return;
    };

    if let Some(expected_unit) = device_units.get(device) {
        if let Some(got_unit) = value_unit
            && got_unit != expected_unit
        {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!(
                    "阈值单位不一致：{device} 声明 unit=\"{expected_unit}\"，但比较值使用单位 \"{got_unit}\"",
                ),
                "请统一单位（修改 unit 或比较值单位），或移除比较值的单位后缀",
            ));
        }
    }

    if value < *min || value > *max {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!("阈值 {value} 超出 {device} 的 range {min}..{max}"),
            "请调整阈值或更新 range 范围",
        ));
    }
}

fn map_safety_operand(operand: &SafetyOperand) -> SafetyExpr {
    match operand {
        SafetyOperand::State(state_ref) => SafetyExpr::State(StateExpr {
            device: state_ref.device.clone(),
            state: state_ref.state.clone(),
        }),
        SafetyOperand::Threshold {
            device,
            operator,
            value,
            unit: _,
        } => SafetyExpr::Threshold {
            device: device.clone(),
            operator: comparison_operator_to_string(operator).to_string(),
            value: value.to_string(),
        },
    }
}

fn comparison_operator_to_string(op: &ComparisonOperator) -> &'static str {
    match op {
        ComparisonOperator::Eq => "==",
        ComparisonOperator::Neq => "!=",
        ComparisonOperator::Gt => ">",
        ComparisonOperator::Lt => "<",
        ComparisonOperator::Gte => ">=",
        ComparisonOperator::Lte => "<=",
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
        AstTimingRelation::MustCompleteWithinWorstCase => {
            IrTimingRelation::MustCompleteWithinWorstCase
        }
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
            StepStatement::IfElse { .. } => {}
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
        ActionStatement::SetAnalog { target, .. } => (ActionKind::SetAnalog, Some(target.as_str())),
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
        ActionKind::Set | ActionKind::SetAnalog => profile.ramp_ms.or(profile.response_ms),
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
        ActionKind::SetAnalog => "set_analog",
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
        DeviceKind::AnalogInput | DeviceKind::AnalogOutput | DeviceKind::Pid => &[],
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

fn analyze_statements(
    statements: &[StepStatement],
    wait_ctx: &WaitExpressionContext,
) -> AnalyzedStatements {
    let mut analyzed = AnalyzedStatements::default();

    for statement in statements {
        match statement {
            StepStatement::Action(action) => {
                analyzed.actions.push(action_to_transition_action(action));
            }
            StepStatement::Wait(wait) => {
                analyzed
                    .waits
                    .push(wait_to_guard_expression(wait, wait_ctx));
            }
            StepStatement::IfElse {
                condition,
                then_goto,
                else_goto,
            } => analyzed.if_elses.push(IfElseSpec {
                condition: condition.clone(),
                then_goto: then_goto.clone(),
                else_goto: else_goto.clone(),
            }),
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
    task_defined_steps: &HashMap<String, HashSet<String>>,
    errors: &mut Vec<PlcError>,
    parent_actions: Vec<TransitionAction>,
    wait_ctx: &WaitExpressionContext,
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

        let analyzed = analyze_statements(&branch.statements, wait_ctx);

        for goto in &analyzed.gotos {
            if let Some(target) = resolve_task_target(
                goto,
                task_initial_states,
                task_defined_steps,
                errors,
                "goto",
            ) {
                builder.add_transition(
                    branch_state.clone(),
                    target,
                    TransitionGuard::Always,
                    analyzed.actions.clone(),
                    Vec::new(),
                );
            }
        }

        for if_else in &analyzed.if_elses {
            let expr = condition_to_expression(&if_else.condition);

            if let Some(then_target) = resolve_task_target(
                &if_else.then_goto,
                task_initial_states,
                task_defined_steps,
                errors,
                "if/else then goto",
            ) {
                builder.add_transition(
                    branch_state.clone(),
                    then_target,
                    TransitionGuard::Condition {
                        expression: expr.clone(),
                    },
                    analyzed.actions.clone(),
                    Vec::new(),
                );
            }

            if let Some(else_target) = resolve_task_target(
                &if_else.else_goto,
                task_initial_states,
                task_defined_steps,
                errors,
                "if/else else goto",
            ) {
                builder.add_transition(
                    branch_state.clone(),
                    else_target,
                    TransitionGuard::Condition {
                        expression: format!("NOT({expr})"),
                    },
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
                &timeout.target,
                task_initial_states,
                task_defined_steps,
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
                task_defined_steps,
                errors,
                analyzed.actions.clone(),
                wait_ctx,
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
                task_defined_steps,
                errors,
                analyzed.actions.clone(),
                wait_ctx,
            );
        }

        let has_control_flow = !analyzed.waits.is_empty()
            || !analyzed.delays_ms.is_empty()
            || !analyzed.gotos.is_empty()
            || !analyzed.if_elses.is_empty()
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
    task_defined_steps: &HashMap<String, HashSet<String>>,
    errors: &mut Vec<PlcError>,
    parent_actions: Vec<TransitionAction>,
    wait_ctx: &WaitExpressionContext,
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

        let analyzed = analyze_statements(&branch.statements, wait_ctx);
        let branch_completion_target = branch
            .then_goto
            .as_ref()
            .and_then(|goto| {
                resolve_task_target(
                    goto,
                    task_initial_states,
                    task_defined_steps,
                    errors,
                    "race then goto",
                )
            })
            .or_else(|| completion_target.clone());

        for goto in &analyzed.gotos {
            if let Some(target) = resolve_task_target(
                goto,
                task_initial_states,
                task_defined_steps,
                errors,
                "goto",
            ) {
                builder.add_transition(
                    branch_state.clone(),
                    target,
                    TransitionGuard::Always,
                    analyzed.actions.clone(),
                    Vec::new(),
                );
            }
        }

        for if_else in &analyzed.if_elses {
            let expr = condition_to_expression(&if_else.condition);

            if let Some(then_target) = resolve_task_target(
                &if_else.then_goto,
                task_initial_states,
                task_defined_steps,
                errors,
                "if/else then goto",
            ) {
                builder.add_transition(
                    branch_state.clone(),
                    then_target,
                    TransitionGuard::Condition {
                        expression: expr.clone(),
                    },
                    analyzed.actions.clone(),
                    Vec::new(),
                );
            }

            if let Some(else_target) = resolve_task_target(
                &if_else.else_goto,
                task_initial_states,
                task_defined_steps,
                errors,
                "if/else else goto",
            ) {
                builder.add_transition(
                    branch_state.clone(),
                    else_target,
                    TransitionGuard::Condition {
                        expression: format!("NOT({expr})"),
                    },
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
                &timeout.target,
                task_initial_states,
                task_defined_steps,
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
                task_defined_steps,
                errors,
                analyzed.actions.clone(),
                wait_ctx,
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
                task_defined_steps,
                errors,
                analyzed.actions.clone(),
                wait_ctx,
            );
        }

        let has_control_flow = !analyzed.waits.is_empty()
            || !analyzed.delays_ms.is_empty()
            || !analyzed.gotos.is_empty()
            || !analyzed.if_elses.is_empty()
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
    target: &GotoDirective,
    task_initial_states: &HashMap<String, State>,
    task_defined_steps: &HashMap<String, HashSet<String>>,
    errors: &mut Vec<PlcError>,
    source: &str,
) -> Option<State> {
    let line = target.line.max(1);
    let Some(initial_state) = task_initial_states.get(&target.task) else {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            " task",
            &target.task,
            format!("{source} 目标必须是已定义 task 名称"),
        ));
        return None;
    };

    let Some(step) = &target.step else {
        return Some(initial_state.clone());
    };

    let Some(steps) = task_defined_steps.get(&target.task) else {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            " task",
            &target.task,
            format!("{source} 目标必须是已定义 task 名称"),
        ));
        return None;
    };

    if !steps.contains(step) {
        let synthetic_hint = step.contains("__parallel_") || step.contains("__race_");
        if synthetic_hint {
            errors.push(PlcError::semantic(
                line,
                format!(
                    "{source} 不允许跳转到 parallel/race 内部合成 step {}.{step}",
                    target.task
                ),
            ));
        } else {
            errors.push(PlcError::semantic(
                line,
                format!("{source} 引用了未定义 step {}.{step}", target.task),
            ));
        }
        return None;
    }

    Some(State {
        task_name: target.task.clone(),
        step_name: step.clone(),
    })
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
        ActionStatement::SetAnalog { target, value } => TransitionAction::SetAnalog {
            target: target.clone(),
            value_raw: value.to_string(),
        },
        ActionStatement::Log { message } => TransitionAction::Log {
            message: message.clone(),
        },
    }
}

fn wait_to_guard_expression(wait: &WaitStatement, wait_ctx: &WaitExpressionContext) -> String {
    wait_condition_to_expression(&wait.condition, wait_ctx)
}

fn wait_condition_to_expression(
    condition: &WaitCondition,
    wait_ctx: &WaitExpressionContext,
) -> String {
    match condition {
        WaitCondition::Single(single) => wait_term_to_expression(single, wait_ctx),
        WaitCondition::And(conditions) => conditions
            .iter()
            .map(|condition| wait_term_to_expression(condition, wait_ctx))
            .collect::<Vec<_>>()
            .join(" AND "),
        WaitCondition::Or(conditions) => conditions
            .iter()
            .map(|condition| wait_term_to_expression(condition, wait_ctx))
            .collect::<Vec<_>>()
            .join(" OR "),
    }
}

fn analog_region_state_name(index: usize) -> String {
    format!("region_{index}")
}

#[derive(Clone, Copy)]
enum ComparisonOp {
    Eq,
    Neq,
    Gt,
    Lt,
    Gte,
    Lte,
}

fn comparison_op_from_ast(op: &ComparisonOperator) -> ComparisonOp {
    match op {
        ComparisonOperator::Eq => ComparisonOp::Eq,
        ComparisonOperator::Neq => ComparisonOp::Neq,
        ComparisonOperator::Gt => ComparisonOp::Gt,
        ComparisonOperator::Lt => ComparisonOp::Lt,
        ComparisonOperator::Gte => ComparisonOp::Gte,
        ComparisonOperator::Lte => ComparisonOp::Lte,
    }
}

fn region_intersects(op: ComparisonOp, value: f64, min: f64, max: f64) -> bool {
    match op {
        ComparisonOp::Eq => value >= min && value <= max,
        ComparisonOp::Neq => !(min == max && value == min),
        ComparisonOp::Gt => max > value,
        // For analog waits we need the selected region set to be a *sufficient* condition
        // (otherwise a wait may be satisfied even when the numeric predicate is false).
        //
        // Using intersection semantics for / becomes a tautology when regions overlap
        // at the split point (e.g. [0..T] and [T..MAX]), because both regions intersect.
        // So for non-strict comparisons we pick regions that are entirely within the predicate.
        ComparisonOp::Gte => min >= value,
        ComparisonOp::Lt => min < value,
        ComparisonOp::Lte => max <= value,
    }
}

fn wait_term_to_expression(
    condition: &ConditionExpression,
    wait_ctx: &WaitExpressionContext,
) -> String {
    if let Some((value, _unit)) = threshold_literal_value_and_unit(&condition.right) {
        if let Some(device_name) = wait_operand_device_name(&condition.left) {
            if let Some(regions) = wait_ctx.analog_input_regions.get(device_name) {
                let op = comparison_op_from_ast(&condition.operator);
                let mut matching = Vec::new();

                for (index, (min, max)) in regions.iter().enumerate() {
                    if region_intersects(op, value, *min, *max) {
                        matching.push(index);
                    }
                }

                if !matching.is_empty() {
                    let rendered = matching
                        .into_iter()
                        .map(analog_region_state_name)
                        .collect::<Vec<_>>()
                        .join(", ");
                    return format!("{device_name} in {{{rendered}}}");
                }
            }
        }
    }

    condition_to_expression(condition)
}

fn condition_to_expression(condition: &ConditionExpression) -> String {
    let operator = match condition.operator {
        ComparisonOperator::Eq => "==",
        ComparisonOperator::Neq => "!=",
        ComparisonOperator::Gt => ">",
        ComparisonOperator::Lt => "<",
        ComparisonOperator::Gte => ">=",
        ComparisonOperator::Lte => "<=",
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
        LiteralValue::Measured(measured) => format!("{}{}", measured.value, measured.unit),
        LiteralValue::String(value) => format!("\"{}\"", value),
        LiteralValue::State(state) => format!("{}.{}", state.device, state.state),
    }
}

fn threshold_literal_value_and_unit(literal: &LiteralValue) -> Option<(f64, Option<&str>)> {
    match literal {
        LiteralValue::Number(value) => Some((*value, None)),
        LiteralValue::Measured(measured) => Some((measured.value, Some(measured.unit.as_str()))),
        LiteralValue::Boolean(_) | LiteralValue::String(_) | LiteralValue::State(_) => None,
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
        DeviceType::AnalogInput => DeviceKind::AnalogInput,
        DeviceType::AnalogOutput => DeviceKind::AnalogOutput,
        DeviceType::Pid => DeviceKind::Pid,
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
        (DeviceKind::AnalogInput, DeviceKind::AnalogInput)
        | (DeviceKind::AnalogOutput, DeviceKind::AnalogOutput) => Some(ConnectionType::Logical),
        (DeviceKind::AnalogInput, DeviceKind::Sensor)
        | (DeviceKind::AnalogOutput, DeviceKind::SolenoidValve)
        | (DeviceKind::AnalogOutput, DeviceKind::Motor) => Some(ConnectionType::Analog),
        (DeviceKind::Pid, DeviceKind::Pid) => Some(ConnectionType::Logical),
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
        DeviceKind::AnalogInput => "analog_input",
        DeviceKind::AnalogOutput => "analog_output",
        DeviceKind::Pid => "pid",
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
    fn topology_extracts_pid_loop_with_conditional_integration_strategy() {
        let input = r#"
[topology]
device AI0: analog_input { range: 0..100, unit: "bar" }
device AO0: analog_output { range: 0..100, unit: "%" }
device loop_pressure: pid {
    pv: AI0,
    sp: 50bar,
    kp: 2.0,
    ki: 0.4,
    kd: 0.05,
    out: AO0,
    period_ms: 100,
    limit: 0..100
}

[constraints]

[tasks]
task main:
    step hold:
"#;

        let program = parse_plc(input).expect("parse");
        let topology = build_topology_graph(&program).expect("build topology");
        assert_eq!(topology.pid_loops.len(), 1);
        let pid = &topology.pid_loops[0];
        assert_eq!(pid.name, "loop_pressure");
        assert_eq!(pid.pv, "AI0");
        assert_eq!(pid.out, "AO0");
        assert_eq!(pid.period_ms, 100);
        assert_eq!(pid.anti_windup, "conditional_integration");
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
        match &constraints.safety[0].left {
            crate::ir::SafetyExpr::State(expr) => {
                assert_eq!(expr.device, "cyl_A");
                assert_eq!(expr.state, "extended");
            }
            other => panic!("期望 State 变体，实际为: {other:?}"),
        }

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
    fn builds_constraint_set_with_must_complete_within_worst_case_relation() {
        let input = r#"
[topology]

[constraints]

timing: task.init must_complete_within_worst_case 1000ms

[tasks]

task init:
    step start:
        action: log "ok"
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let constraints = build_constraint_set(&program).expect("应能构建约束集合");

        assert_eq!(constraints.timing.len(), 1);
        assert!(matches!(
            constraints.timing[0].relation,
            TimingRelation::MustCompleteWithinWorstCase
        ));
        assert_eq!(constraints.timing[0].duration_ms, 1000);
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
    fn reports_undefined_device_in_and_or_wait_conditions() {
        let input = r#"
[topology]

device sensor_A: sensor
device sensor_C: sensor

[constraints]

[tasks]

task main:
    step wait_combo:
        wait: sensor_A == true AND sensor_B == true
        wait: sensor_C == true OR sensor_D == true
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let errors = build_constraint_set(&program).expect_err("AND/OR wait 的未定义设备应报错");

        let rendered = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("未定义设备 sensor_B"),
            "应报告 AND 子条件中的未定义设备"
        );
        assert!(
            rendered.contains("未定义设备 sensor_D"),
            "应报告 OR 子条件中的未定义设备"
        );
    }

    #[test]
    fn reports_invalid_analog_thresholds_in_safety() {
        let input = r#"
[topology]

device pressure_ok: analog_input { range: 0..10 }
device pressure_missing: analog_input
device button: digital_input

[constraints]

safety: pressure_ok > 11 conflicts_with button.on
safety: pressure_missing > 5 conflicts_with button.on
safety: button > 1 conflicts_with button.on

[tasks]

task main:
    step start:
        wait: button == true
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let errors = build_constraint_set(&program).expect_err("无效阈值比较应报错");

        let rendered = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("pressure_ok") && rendered.contains("超出"),
            "应报告阈值超出范围"
        );
        assert!(
            rendered.contains("pressure_missing") && rendered.contains("缺少 range"),
            "应报告缺少 range 的模拟量输入"
        );
        assert!(
            rendered.contains("期望 analog_input"),
            "应报告非 analog_input 的阈值比较"
        );
    }

    #[test]
    fn reports_invalid_analog_thresholds_in_wait_conditions() {
        let input = r#"
[topology]

device temp_ok: analog_input { range: 0..100 }
device temp_missing: analog_input
device start_button: digital_input

[constraints]

[tasks]

task main:
    step check:
        wait: temp_ok > 120
        wait: temp_missing < 10
        wait: start_button > 1
        wait: start_button == true
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let errors = build_constraint_set(&program).expect_err("无效 wait 阈值比较应报错");

        let rendered = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("temp_ok") && rendered.contains("超出"),
            "应报告 wait 阈值超出范围"
        );
        assert!(
            rendered.contains("temp_missing") && rendered.contains("缺少 range"),
            "应报告 wait 条件缺少 range 的模拟量输入"
        );
        assert!(
            rendered.contains("期望 analog_input"),
            "应报告 wait 条件使用非 analog_input 设备"
        );
    }

    #[test]
    fn reports_unit_mismatch_for_analog_thresholds() {
        let input = r#"
[topology]

device pressure: analog_input { range: 0..10, unit: "bar" }
device button: digital_input

[constraints]

safety: pressure > 5psi conflicts_with button.on

[tasks]

task main:
    step check:
        wait: pressure > 5psi
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let errors = build_constraint_set(&program).expect_err("单位不一致应报错");
        let rendered = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("单位不一致") && rendered.contains("bar") && rendered.contains("psi"),
            "应报告阈值比较单位不一致"
        );
    }

    #[test]
    fn accepts_unit_matched_analog_thresholds() {
        let input = r#"
[topology]

device pressure: analog_input { range: 0..10, unit: "bar" }
device button: digital_input

[constraints]

safety: pressure > 5bar conflicts_with button.on

[tasks]

task main:
    step check:
        wait: pressure > 5bar
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let constraints = build_constraint_set(&program).expect("单位一致的阈值比较应通过语义检查");
        assert_eq!(constraints.safety.len(), 1);
    }

    #[test]
    fn maps_analog_wait_conditions_to_region_predicates() {
        let input = r#"
[topology]

device AI0: analog_input { range: 0..10 }

[constraints]

[tasks]

task main:
    step wait_pressure:
        wait: AI0 > 5
    step done:
        action: log "ok"
"#;

        let program = parse_plc(input).expect("测试输入应能解析为 AST");
        let state_machine = build_state_machine(&program).expect("应能构建状态机");

        let has_region_guard = state_machine.transitions.iter().any(|transition| {
            matches!(
                transition.guard,
                TransitionGuard::Condition { ref expression }
                    if expression.contains("AI0") && expression.contains("region_")
            )
        });
        assert!(has_region_guard, "模拟量 wait 应映射为 region 谓词表达式");
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

    #[test]
    fn rejects_goto_to_synthetic_parallel_step() {
        let input = r#"
[topology]

[constraints]

[tasks]

task main:
    step start:
        parallel:
            branch_A:
                action: log "A"
    step jump:
        goto main.start__parallel_1_fork
"#;

        let program = parse_plc(input).expect("测试输入应能成功解析为 AST");
        let errors = build_state_machine(&program).expect_err("跳转到合成 step 应报语义错误");

        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("不允许跳转到 parallel/race 内部合成 step"),
            "应提示不允许跳转到合成 step"
        );
    }

    #[test]
    fn expands_repeat_block_into_sequential_steps_with_suffixes() {
        let input = r#"
[topology]

[constraints]

[tasks]

task init:
    step glue_cycle:
        repeat 3:
            action: log "tick"
"#;

        let program = parse_plc(input).expect("repeat 示例应能成功解析为 AST");
        let state_machine = build_state_machine(&program).expect("repeat 应在语义阶段展开");

        for suffix in ["glue_cycle_1", "glue_cycle_2", "glue_cycle_3"] {
            assert!(
                state_machine
                    .states
                    .iter()
                    .any(|state| { state.task_name == "init" && state.step_name == suffix }),
                "repeat 展开后应包含 step {suffix}"
            );
        }

        let has_1_to_2 = state_machine.transitions.iter().any(|transition| {
            transition.from.task_name == "init"
                && transition.from.step_name == "glue_cycle_1"
                && transition.to.task_name == "init"
                && transition.to.step_name == "glue_cycle_2"
                && matches!(transition.guard, TransitionGuard::Always)
        });
        assert!(has_1_to_2, "glue_cycle_1 应顺序链接到 glue_cycle_2");

        let has_2_to_3 = state_machine.transitions.iter().any(|transition| {
            transition.from.task_name == "init"
                && transition.from.step_name == "glue_cycle_2"
                && transition.to.task_name == "init"
                && transition.to.step_name == "glue_cycle_3"
                && matches!(transition.guard, TransitionGuard::Always)
        });
        assert!(has_2_to_3, "glue_cycle_2 应顺序链接到 glue_cycle_3");
    }

    #[test]
    fn reports_semantic_error_for_repeat_count_zero_or_one() {
        for count in [0, 1] {
            let input = format!(
                r#"
[topology]

[constraints]

[tasks]

task init:
    step glue_cycle:
        repeat {count}:
            action: log "tick"
"#
            );

            let program = parse_plc(&input).expect("repeat 语法应能解析");
            let errors = build_state_machine(&program).expect_err("repeat 0/1 应报语义错误");
            let joined = errors
                .iter()
                .map(|err| err.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                joined.contains("repeat 次数必须在 2..=100 之间"),
                "应包含 repeat 次数范围错误提示"
            );
        }
    }

    #[test]
    fn reports_semantic_error_for_repeat_count_over_limit() {
        let input = r#"
[topology]

[constraints]

[tasks]

task init:
    step glue_cycle:
        repeat 101:
            action: log "tick"
"#;

        let program = parse_plc(input).expect("repeat 语法应能解析");
        let errors = build_state_machine(&program).expect_err("repeat > 100 应报语义错误");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("repeat 次数超过上限 100"),
            "应包含 repeat 次数上限错误提示"
        );
    }

    #[test]
    fn reports_semantic_error_for_nested_repeat_blocks() {
        let input = r#"
[topology]

[constraints]

[tasks]

task init:
    step glue_cycle:
        repeat 2:
            repeat 2:
                action: log "tick"
"#;

        let program = parse_plc(input).expect("嵌套 repeat 语法应能解析");
        let errors = build_state_machine(&program).expect_err("嵌套 repeat 应报语义错误");
        let joined = errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("不允许嵌套 repeat"),
            "应包含嵌套 repeat 错误提示"
        );
    }
}
