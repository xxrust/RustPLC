use crate::ast::{
    ActionStatement, ComparisonOperator, ConditionExpression, DeviceType, ExternCallBinding,
    GotoDirective, LiteralValue, OnCompleteDirective, PlcProgram, StepDeclaration, StepStatement,
    WaitCondition, WaitStatement,
};
use crate::device_semantics::axis::move_action_view as axis_move_action_view;
use crate::device_semantics::cylinder::stroke_action_view as cylinder_stroke_action_view;
use crate::ir::{ConstraintSet, DeviceKind, TopologyGraph};
use petgraph::algo::has_path_connecting;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CausalityDiagnostic {
    pub line: usize,
    pub action: Option<String>,
    pub wait: Option<String>,
    pub broken_link: String,
    pub expected_chain: String,
    pub actual_chain: String,
    pub suggestion: String,
}

impl fmt::Display for CausalityDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "ERROR [causality] 因果链断裂")?;
        writeln!(f, "  位置: <input>:{}:1", self.line)?;

        if let Some(action) = &self.action {
            writeln!(f, "  动作: {action}")?;
        }
        if let Some(wait) = &self.wait {
            writeln!(f, "  等待: {wait}")?;
        }

        writeln!(f, "  断裂链路: {}", self.broken_link)?;
        writeln!(f, "  期望链路: {}", self.expected_chain)?;
        writeln!(f, "  实际链路: {}", self.actual_chain)?;
        write!(f, "  建议: {}", self.suggestion)
    }
}

pub fn verify_causality(
    program: &PlcProgram,
    topology: &TopologyGraph,
    constraints: &ConstraintSet,
) -> Result<(), Vec<CausalityDiagnostic>> {
    let runtime_graph = RuntimeGraph::from_inputs(program, topology);
    let mut diagnostics = Vec::new();

    let chain_line_map = collect_chain_line_map(program);
    for chain in &constraints.causality {
        if chain.devices.len() < 2 {
            continue;
        }

        if let Some((from, to)) = first_broken_link(&runtime_graph, &chain.devices) {
            let line = chain_line_map
                .get(&chain.devices)
                .copied()
                .unwrap_or(1)
                .max(1);

            diagnostics.push(CausalityDiagnostic {
                line,
                action: None,
                wait: None,
                broken_link: format!("{from} -> {to}"),
                expected_chain: chain.devices.join(" -> "),
                actual_chain: realized_prefix(&runtime_graph, &chain.devices),
                suggestion: suggestion_for_link(&from, &to),
            });
        }
    }

    let observed_names = collect_observed_names(program);
    let output_ports = collect_output_ports(topology);
    let declared_chains: Vec<Vec<String>> = constraints
        .causality
        .iter()
        .map(|chain| chain.devices.clone())
        .collect();

    for pair in collect_action_wait_pairs(program, &observed_names) {
        if let Some(expected_chain) =
            match_declared_chain(&declared_chains, &pair.action_target, &pair.wait_sensor)
        {
            if let Some((from, to)) = first_broken_link(&runtime_graph, &expected_chain) {
                diagnostics.push(CausalityDiagnostic {
                    line: pair.line,
                    action: Some(pair.action),
                    wait: Some(pair.wait),
                    broken_link: format!("{from} -> {to}"),
                    expected_chain: expected_chain.join(" -> "),
                    actual_chain: realized_prefix(&runtime_graph, &expected_chain),
                    suggestion: suggestion_for_link(&from, &to),
                });
            }
            continue;
        }

        // 推断型 pair（parallel 分支组合、completion target 组合）只在 action->wait 可达时做兜底检查，
        // 避免把“无因果关系的偶然组合”误报成链路断裂。
        if pair.requires_existing_inferred_path()
            && !runtime_graph.path_exists(&pair.action_target, &pair.wait_sensor)
        {
            continue;
        }

        let source_path =
            shortest_output_path_to_target(&runtime_graph, &output_ports, &pair.action_target);
        let feedback_path = shortest_path(&runtime_graph, &pair.action_target, &pair.wait_sensor);

        // 变量/外部函数等控制信号节点可能没有输出端口前缀，仅需验证 action->wait 可达性。
        if source_path.is_none()
            && let Some(feedback_path) = &feedback_path
            && first_broken_link(&runtime_graph, feedback_path).is_none()
        {
            continue;
        }

        if let (Some(source_path), Some(feedback_path)) = (&source_path, &feedback_path) {
            let full_path = join_paths(source_path, feedback_path);
            if first_broken_link(&runtime_graph, &full_path).is_none() {
                continue;
            }
        }

        let (broken_link, expected_chain, actual_chain, suggestion) =
            build_fallback_details(&pair, &source_path, &feedback_path, &output_ports);

        diagnostics.push(CausalityDiagnostic {
            line: pair.line,
            action: Some(pair.action),
            wait: Some(pair.wait),
            broken_link,
            expected_chain,
            actual_chain,
            suggestion,
        });
    }

    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

#[derive(Debug, Clone)]
struct ActionWaitPair {
    line: usize,
    action: String,
    action_target: String,
    wait: String,
    wait_sensor: String,
    action_origin: StatementOrigin,
    wait_origin: StatementOrigin,
}

#[derive(Debug, Clone)]
struct CollectedAction {
    text: String,
    target: String,
    origin: StatementOrigin,
}

#[derive(Debug, Clone)]
struct CollectedWait {
    line: usize,
    text: String,
    observed: String,
    origin: StatementOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StatementOrigin {
    StepLevel,
    CompletionTarget,
    ParallelBranch {
        block_id: usize,
        branch_index: usize,
    },
}

impl ActionWaitPair {
    fn requires_existing_inferred_path(&self) -> bool {
        !matches!(self.action_origin, StatementOrigin::StepLevel)
            || !matches!(self.wait_origin, StatementOrigin::StepLevel)
    }
}

#[derive(Debug, Clone)]
struct RuntimeGraph {
    graph: DiGraph<String, ()>,
    nodes: HashMap<String, NodeIndex>,
}

impl RuntimeGraph {
    fn from_inputs(program: &PlcProgram, topology: &TopologyGraph) -> Self {
        let mut graph = DiGraph::<String, ()>::new();
        let mut nodes = HashMap::<String, NodeIndex>::new();

        for node in topology.graph.node_indices() {
            let name = topology.graph[node].name.clone();
            let index = graph.add_node(name.clone());
            nodes.insert(name, index);
        }

        for edge in topology.graph.edge_references() {
            let source_name = topology.graph[edge.source()].name.as_str();
            let target_name = topology.graph[edge.target()].name.as_str();

            if let (Some(source), Some(target)) = (nodes.get(source_name), nodes.get(target_name)) {
                graph.add_edge(*source, *target, ());
            }
        }

        for device in &program.topology.devices {
            let Some(detects) = &device.attributes.detects else {
                continue;
            };

            let Some(source) = nodes.get(&detects.device) else {
                continue;
            };
            let Some(target) = nodes.get(&device.name) else {
                continue;
            };

            graph.add_edge(*source, *target, ());
        }

        let mut runtime_graph = Self { graph, nodes };
        runtime_graph.add_dataflow_edges(program);
        runtime_graph
    }

    fn path_exists(&self, from: &str, to: &str) -> bool {
        let Some(source) = self.nodes.get(from) else {
            return false;
        };
        let Some(target) = self.nodes.get(to) else {
            return false;
        };

        has_path_connecting(&self.graph, *source, *target, None)
    }

    fn ensure_node(&mut self, name: &str) -> NodeIndex {
        if let Some(index) = self.nodes.get(name) {
            return *index;
        }

        let owned = name.to_string();
        let index = self.graph.add_node(owned.clone());
        self.nodes.insert(owned, index);
        index
    }

    fn add_edge_by_name(&mut self, from: &str, to: &str) {
        let source = self.ensure_node(from);
        let target = self.ensure_node(to);
        self.graph.add_edge(source, target, ());
    }

    fn add_dataflow_edges(&mut self, program: &PlcProgram) {
        for variable in &program.topology.variables {
            self.ensure_node(&variable.name);
        }

        let pure_externs = program
            .topology
            .extern_functions
            .iter()
            .map(|func| (func.name.clone(), func.contract.pure))
            .collect::<HashMap<_, _>>();

        for task in &program.tasks.tasks {
            for step in &task.steps {
                collect_dataflow_edges_from_statements(&step.statements, &pure_externs, self);
            }
        }
    }
}

fn collect_dataflow_edges_from_statements(
    statements: &[StepStatement],
    pure_externs: &HashMap<String, bool>,
    runtime_graph: &mut RuntimeGraph,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(action) => match action {
                ActionStatement::Call {
                    function,
                    args,
                    binding,
                } => {
                    runtime_graph.ensure_node(function);

                    for arg in args {
                        for dep in expression_variables(arg) {
                            runtime_graph.add_edge_by_name(&dep, function);
                        }
                    }

                    if pure_externs.get(function).copied().unwrap_or(false) {
                        for target in extern_binding_targets(binding) {
                            runtime_graph.add_edge_by_name(function, target);
                        }
                    }
                }
                ActionStatement::Compute { target, expr } => {
                    runtime_graph.ensure_node(target);
                    for dep in expression_variables(expr) {
                        runtime_graph.add_edge_by_name(&dep, target);
                    }
                }
                ActionStatement::SetAnalogExpr { target, expr } => {
                    for dep in expression_variables(expr) {
                        runtime_graph.add_edge_by_name(&dep, &target.device);
                    }
                }
                _ => {}
            },
            StepStatement::Repeat { body, .. } => {
                collect_dataflow_edges_from_statements(body, pure_externs, runtime_graph);
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    collect_dataflow_edges_from_statements(
                        &branch.statements,
                        pure_externs,
                        runtime_graph,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    collect_dataflow_edges_from_statements(
                        &branch.statements,
                        pure_externs,
                        runtime_graph,
                    );
                }
            }
            _ => {}
        }
    }
}

fn expression_variables(expr: &crate::ast::Expression) -> HashSet<String> {
    let mut vars = HashSet::new();
    collect_expression_variables(expr, &mut vars);
    vars
}

fn collect_expression_variables(expr: &crate::ast::Expression, vars: &mut HashSet<String>) {
    match expr {
        crate::ast::Expression::Literal(_) | crate::ast::Expression::Boolean(_) => {}
        crate::ast::Expression::Variable(name) => {
            vars.insert(name.clone());
        }
        crate::ast::Expression::UnaryNeg(inner) => {
            collect_expression_variables(inner, vars);
        }
        crate::ast::Expression::UnaryNot(inner) => {
            collect_expression_variables(inner, vars);
        }
        crate::ast::Expression::BinaryOp { left, right, .. } => {
            collect_expression_variables(left, vars);
            collect_expression_variables(right, vars);
        }
        crate::ast::Expression::FunctionCall { args, .. } => {
            for arg in args {
                collect_expression_variables(arg, vars);
            }
        }
    }
}

fn extern_binding_targets(binding: &ExternCallBinding) -> Vec<&str> {
    match binding {
        ExternCallBinding::Single(target) => vec![target.as_str()],
        ExternCallBinding::Tuple(targets) => targets.iter().map(String::as_str).collect(),
    }
}

fn collect_chain_line_map(program: &PlcProgram) -> HashMap<Vec<String>, usize> {
    program
        .constraints
        .causality
        .iter()
        .map(|chain| {
            (
                chain
                    .chain
                    .iter()
                    .map(|node| node.device.clone())
                    .collect::<Vec<_>>(),
                chain.line.max(1),
            )
        })
        .collect()
}

fn first_broken_link(runtime_graph: &RuntimeGraph, chain: &[String]) -> Option<(String, String)> {
    for pair in chain.windows(2) {
        if !runtime_graph.path_exists(&pair[0], &pair[1]) {
            return Some((pair[0].clone(), pair[1].clone()));
        }
    }

    None
}

fn realized_prefix(runtime_graph: &RuntimeGraph, chain: &[String]) -> String {
    if chain.is_empty() {
        return "???".to_string();
    }

    let mut realized = vec![chain[0].clone()];
    for pair in chain.windows(2) {
        let Some(segment_path) = shortest_path(runtime_graph, &pair[0], &pair[1]) else {
            realized.push("???".to_string());
            break;
        };

        for node in segment_path.into_iter().skip(1) {
            if realized.last() != Some(&node) {
                realized.push(node);
            }
        }
    }

    realized.join(" -> ")
}

fn collect_observed_names(program: &PlcProgram) -> HashSet<String> {
    let mut names = program
        .topology
        .devices
        .iter()
        .filter_map(|device| match device.device_type {
            DeviceType::Sensor => Some(device.name.clone()),
            DeviceType::AnalogInput => {
                if device.attributes.external.unwrap_or(false) {
                    None
                } else {
                    Some(device.name.clone())
                }
            }
            _ => None,
        })
        .collect::<HashSet<_>>();
    names.extend(
        program
            .topology
            .variables
            .iter()
            .map(|var| var.name.clone()),
    );
    names
}

fn collect_output_ports(topology: &TopologyGraph) -> Vec<String> {
    topology
        .graph
        .node_indices()
        .filter(|index| {
            matches!(
                topology.graph[*index].kind,
                DeviceKind::DigitalOutput | DeviceKind::AnalogOutput
            )
        })
        .map(|index| topology.graph[index].name.clone())
        .collect()
}

fn collect_action_wait_pairs(
    program: &PlcProgram,
    observed_names: &HashSet<String>,
) -> Vec<ActionWaitPair> {
    let mut pairs = Vec::new();
    let mut next_parallel_block_id = 0usize;

    for (task_index, task) in program.tasks.tasks.iter().enumerate() {
        for (step_index, step) in task.steps.iter().enumerate() {
            collect_pairs_from_statements(
                &step.statements,
                step.line.max(1),
                observed_names,
                &mut next_parallel_block_id,
                &mut pairs,
                program,
            );
            collect_completion_target_pairs(
                program,
                task_index,
                step_index,
                observed_names,
                &mut next_parallel_block_id,
                &mut pairs,
            );
        }
    }

    pairs
}

fn collect_completion_target_pairs(
    program: &PlcProgram,
    task_index: usize,
    step_index: usize,
    observed_names: &HashSet<String>,
    next_parallel_block_id: &mut usize,
    pairs: &mut Vec<ActionWaitPair>,
) {
    let Some((target_step, target_label)) =
        completion_target_step_and_label(program, task_index, step_index)
    else {
        return;
    };

    let step = &program.tasks.tasks[task_index].steps[step_index];
    let actions = collect_completion_actions(&step.statements);
    if actions.is_empty() {
        return;
    }

    let mut waits = Vec::new();
    collect_wait_items_from_statements(
        &target_step.statements,
        target_step.line.max(1),
        observed_names,
        StatementOrigin::StepLevel,
        next_parallel_block_id,
        &mut waits,
    );
    if waits.is_empty() {
        return;
    }

    for (action_text, action_target) in actions {
        let action_text = format!("{action_text} [done -> {target_label}]");
        for wait in &waits {
            pairs.push(ActionWaitPair {
                line: wait.line,
                action: action_text.clone(),
                action_target: action_target.clone(),
                wait: wait.text.clone(),
                wait_sensor: wait.observed.clone(),
                action_origin: StatementOrigin::CompletionTarget,
                wait_origin: wait.origin.clone(),
            });
        }
    }
}

fn completion_target_step_and_label<'a>(
    program: &'a PlcProgram,
    task_index: usize,
    step_index: usize,
) -> Option<(&'a StepDeclaration, String)> {
    let task = program.tasks.tasks.get(task_index)?;
    if step_index + 1 < task.steps.len() {
        let target = &task.steps[step_index + 1];
        return Some((target, format!("{}.{}", task.name, target.name)));
    }

    let OnCompleteDirective::Goto { target } = task.on_complete.as_ref()? else {
        return None;
    };
    Some((
        resolve_goto_step(program, target)?,
        format_branch_target(target),
    ))
}

fn collect_completion_actions(statements: &[StepStatement]) -> Vec<(String, String)> {
    let mut actions = Vec::new();
    for statement in statements {
        let StepStatement::Action(action) = statement else {
            continue;
        };
        if matches!(
            action,
            ActionStatement::AxisMoveRelative { .. } | ActionStatement::AxisMoveAbsolute { .. }
        ) && let Some((text, target)) = action_to_text_and_target(action)
        {
            actions.push((text, target));
        }
    }
    actions
}

fn collect_pairs_from_statements(
    statements: &[StepStatement],
    line: usize,
    observed_names: &HashSet<String>,
    next_parallel_block_id: &mut usize,
    pairs: &mut Vec<ActionWaitPair>,
    program: &PlcProgram,
) {
    let mut actions = Vec::new();
    let mut waits = Vec::new();

    collect_items_from_statements(
        statements,
        line,
        observed_names,
        StatementOrigin::StepLevel,
        next_parallel_block_id,
        &mut actions,
        &mut waits,
        pairs,
        program,
    );

    for action in &actions {
        for wait in &waits {
            pairs.push(ActionWaitPair {
                line: wait.line,
                action: action.text.clone(),
                action_target: action.target.clone(),
                wait: wait.text.clone(),
                wait_sensor: wait.observed.clone(),
                action_origin: action.origin.clone(),
                wait_origin: wait.origin.clone(),
            });
        }
    }
}

fn collect_items_from_statements(
    statements: &[StepStatement],
    line: usize,
    observed_names: &HashSet<String>,
    origin: StatementOrigin,
    next_parallel_block_id: &mut usize,
    actions: &mut Vec<CollectedAction>,
    waits: &mut Vec<CollectedWait>,
    pairs: &mut Vec<ActionWaitPair>,
    program: &PlcProgram,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(action) => {
                if let Some((action_text, target)) = action_to_text_and_target(action) {
                    actions.push(CollectedAction {
                        text: action_text,
                        target,
                        origin: origin.clone(),
                    });
                }
                collect_motion_branch_pairs(
                    program,
                    action,
                    observed_names,
                    &origin,
                    next_parallel_block_id,
                    pairs,
                );
            }
            StepStatement::Wait(wait) => {
                let wait_text = wait_to_text(wait);
                for observed in infer_wait_observed(wait, observed_names) {
                    waits.push(CollectedWait {
                        line,
                        text: wait_text.clone(),
                        observed,
                        origin: origin.clone(),
                    });
                }
            }
            StepStatement::IfElse { .. } => {}
            StepStatement::Repeat { body, .. } => {
                collect_items_from_statements(
                    body,
                    line,
                    observed_names,
                    origin.clone(),
                    next_parallel_block_id,
                    actions,
                    waits,
                    pairs,
                    program,
                );
            }
            StepStatement::Parallel(block) => {
                let block_id = *next_parallel_block_id;
                *next_parallel_block_id += 1;
                for (branch_index, branch) in block.branches.iter().enumerate() {
                    let branch_origin = StatementOrigin::ParallelBranch {
                        block_id,
                        branch_index,
                    };
                    collect_items_from_statements(
                        &branch.statements,
                        line,
                        observed_names,
                        branch_origin,
                        next_parallel_block_id,
                        actions,
                        waits,
                        pairs,
                        program,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    collect_pairs_from_statements(
                        &branch.statements,
                        line,
                        observed_names,
                        next_parallel_block_id,
                        pairs,
                        program,
                    );
                }
            }
            _ => {}
        }
    }
}

fn collect_motion_branch_pairs(
    program: &PlcProgram,
    action: &ActionStatement,
    observed_names: &HashSet<String>,
    action_origin: &StatementOrigin,
    next_parallel_block_id: &mut usize,
    pairs: &mut Vec<ActionWaitPair>,
) {
    let Some((action_text, action_target, branches)) = motion_action_branches(action) else {
        return;
    };

    for (fault_kind, target) in branches {
        let Some(step) = resolve_goto_step(program, target) else {
            continue;
        };

        let mut waits = Vec::new();
        collect_wait_items_from_statements(
            &step.statements,
            step.line.max(1),
            observed_names,
            StatementOrigin::StepLevel,
            next_parallel_block_id,
            &mut waits,
        );
        if waits.is_empty() {
            continue;
        }

        let action_text = format!(
            "{action_text} [{fault_kind} -> {}]",
            format_branch_target(target)
        );
        for wait in waits {
            pairs.push(ActionWaitPair {
                line: wait.line,
                action: action_text.clone(),
                action_target: action_target.clone(),
                wait: wait.text,
                wait_sensor: wait.observed,
                action_origin: action_origin.clone(),
                wait_origin: wait.origin,
            });
        }
    }
}

fn motion_action_branches(
    action: &ActionStatement,
) -> Option<(String, String, Vec<(&'static str, &GotoDirective)>)> {
    if let Some(view) = cylinder_stroke_action_view(action) {
        return Some((
            view.action_text(),
            view.target.device.clone(),
            view.branch_targets(),
        ));
    }

    if let Some(view) = axis_move_action_view(action) {
        let mut branches = view.branch_targets();
        branches.extend(
            view.on_reject_routes
                .iter()
                .map(|route| ("on_reject_route", &route.target)),
        );
        branches.extend(
            view.on_motion_fault_routes
                .iter()
                .map(|route| ("on_motion_fault_route", &route.target)),
        );
        branches.extend(
            view.on_safety_fault_routes
                .iter()
                .map(|route| ("on_safety_fault_route", &route.target)),
        );
        return Some((
            view.action_text(),
            view.target.device.clone(),
            branches,
        ));
    }

    None
}

fn resolve_goto_step<'a>(
    program: &'a PlcProgram,
    target: &GotoDirective,
) -> Option<&'a StepDeclaration> {
    let task = program
        .tasks
        .tasks
        .iter()
        .find(|task| task.name == target.task)?;
    match target.step.as_deref() {
        Some(step_name) => task.steps.iter().find(|step| step.name == step_name),
        None => task.steps.first(),
    }
}

fn format_branch_target(target: &GotoDirective) -> String {
    match target.step.as_deref() {
        Some(step) => format!("{}.{}", target.task, step),
        None => target.task.clone(),
    }
}

fn collect_wait_items_from_statements(
    statements: &[StepStatement],
    line: usize,
    observed_names: &HashSet<String>,
    origin: StatementOrigin,
    next_parallel_block_id: &mut usize,
    waits: &mut Vec<CollectedWait>,
) {
    for statement in statements {
        match statement {
            StepStatement::Wait(wait) => {
                let wait_text = wait_to_text(wait);
                for observed in infer_wait_observed(wait, observed_names) {
                    waits.push(CollectedWait {
                        line,
                        text: wait_text.clone(),
                        observed,
                        origin: origin.clone(),
                    });
                }
            }
            StepStatement::Repeat { body, .. } => {
                collect_wait_items_from_statements(
                    body,
                    line,
                    observed_names,
                    origin.clone(),
                    next_parallel_block_id,
                    waits,
                );
            }
            StepStatement::Parallel(block) => {
                let block_id = *next_parallel_block_id;
                *next_parallel_block_id += 1;
                for (branch_index, branch) in block.branches.iter().enumerate() {
                    collect_wait_items_from_statements(
                        &branch.statements,
                        line,
                        observed_names,
                        StatementOrigin::ParallelBranch {
                            block_id,
                            branch_index,
                        },
                        next_parallel_block_id,
                        waits,
                    );
                }
            }
            StepStatement::Race(block) => {
                let block_id = *next_parallel_block_id;
                *next_parallel_block_id += 1;
                for (branch_index, branch) in block.branches.iter().enumerate() {
                    collect_wait_items_from_statements(
                        &branch.statements,
                        line,
                        observed_names,
                        StatementOrigin::ParallelBranch {
                            block_id,
                            branch_index,
                        },
                        next_parallel_block_id,
                        waits,
                    );
                }
            }
            _ => {}
        }
    }
}

fn action_to_text_and_target(action: &ActionStatement) -> Option<(String, String)> {
    match action {
        ActionStatement::Extend { target, .. } => {
            Some((format!("extend {}", target.device), target.device.clone()))
        }
        ActionStatement::Retract { target, .. } => {
            Some((format!("retract {}", target.device), target.device.clone()))
        }
        ActionStatement::Set { target, value } => Some((
            format!("set {} {value}", target.device),
            target.device.clone(),
        )),
        ActionStatement::SetAnalog { target, value } => Some((
            format!("set_analog {} {value}", target.device),
            target.device.clone(),
        )),
        ActionStatement::SetAnalogExpr { target, .. } => Some((
            format!("set_analog {} <expr>", target.device),
            target.device.clone(),
        )),
        ActionStatement::AxisMoveRelative { target, .. } => Some((
            format!("axis.move_relative {}", target.device),
            target.device.clone(),
        )),
        ActionStatement::AxisMoveAbsolute { target, .. } => Some((
            format!("axis.move_absolute {}", target.device),
            target.device.clone(),
        )),
        ActionStatement::Compute { target, .. } => {
            Some((format!("compute {target}"), target.clone()))
        }
        ActionStatement::Call { .. } => None,
        ActionStatement::CamEngage { target }
        | ActionStatement::CamDisengage { target }
        | ActionStatement::CamPhase { target, .. } => {
            Some((format!("cam_action {target}"), target.clone()))
        }
        ActionStatement::CamSwitch { target, new_table } => {
            Some((format!("cam_switch {target} {new_table}"), target.clone()))
        }
        ActionStatement::Log { .. } => None,
    }
}

fn infer_wait_observed(wait: &WaitStatement, observed_names: &HashSet<String>) -> Vec<String> {
    let mut observed = Vec::new();
    let mut seen = HashSet::new();

    for condition in wait_conditions(wait) {
        if let Some((left, right)) = condition.expression_pair() {
            for candidate in expression_variables(left)
                .into_iter()
                .chain(expression_variables(right).into_iter())
            {
                if observed_names.contains(&candidate) && seen.insert(candidate.clone()) {
                    observed.push(candidate);
                }
            }
            continue;
        }
        if observed_names.contains(&condition.left) {
            if seen.insert(condition.left.clone()) {
                observed.push(condition.left.clone());
            }
            continue;
        }

        if let Some(candidate) = condition.left.split('.').next()
            && observed_names.contains(candidate)
        {
            let candidate = candidate.to_string();
            if seen.insert(candidate.clone()) {
                observed.push(candidate);
            }
            continue;
        }

        if let LiteralValue::State(state) = &condition.right
            && observed_names.contains(&state.device)
            && seen.insert(state.device.clone())
        {
            observed.push(state.device.clone());
        }
    }

    observed
}

fn wait_conditions(wait: &WaitStatement) -> Vec<&ConditionExpression> {
    match &wait.condition {
        WaitCondition::Single(condition) => vec![condition],
        WaitCondition::And(conditions) | WaitCondition::Or(conditions) => {
            conditions.iter().collect()
        }
    }
}

fn wait_to_text(wait: &WaitStatement) -> String {
    match &wait.condition {
        WaitCondition::Single(condition) => condition_to_text(condition),
        WaitCondition::And(conditions) => conditions
            .iter()
            .map(condition_to_text)
            .collect::<Vec<_>>()
            .join(" AND "),
        WaitCondition::Or(conditions) => conditions
            .iter()
            .map(condition_to_text)
            .collect::<Vec<_>>()
            .join(" OR "),
    }
}

fn condition_to_text(condition: &ConditionExpression) -> String {
    if let Some((left, right)) = condition.expression_pair() {
        return format!(
            "{} {} {}",
            render_expression(left),
            comparison_operator_text(&condition.operator),
            render_expression(right)
        );
    }

    format!(
        "{} {} {}",
        condition.left,
        comparison_operator_text(&condition.operator),
        literal_to_text(&condition.right)
    )
}

fn comparison_operator_text(operator: &ComparisonOperator) -> &'static str {
    match operator {
        ComparisonOperator::Eq => "==",
        ComparisonOperator::Neq => "!=",
        ComparisonOperator::Gt => ">",
        ComparisonOperator::Lt => "<",
        ComparisonOperator::Gte => ">=",
        ComparisonOperator::Lte => "<=",
    }
}

fn literal_to_text(literal: &LiteralValue) -> String {
    match literal {
        LiteralValue::Boolean(value) => value.to_string(),
        LiteralValue::Number(value) => value.to_string(),
        LiteralValue::Measured(measured) => format!("{}{}", measured.value, measured.unit),
        LiteralValue::String(value) => format!("\"{value}\""),
        LiteralValue::State(state) => format!("{}.{}", state.device, state.state),
    }
}

fn render_expression(expr: &crate::ast::Expression) -> String {
    match expr {
        crate::ast::Expression::Literal(value) => value.to_string(),
        crate::ast::Expression::Boolean(value) => value.to_string(),
        crate::ast::Expression::Variable(name) => name.clone(),
        crate::ast::Expression::UnaryNeg(inner) => format!("-({})", render_expression(inner)),
        crate::ast::Expression::UnaryNot(inner) => format!("NOT({})", render_expression(inner)),
        crate::ast::Expression::BinaryOp { op, left, right } => format!(
            "({}{}{})",
            render_expression(left),
            match op {
                crate::ast::BinaryOperator::Add => "+",
                crate::ast::BinaryOperator::Sub => "-",
                crate::ast::BinaryOperator::Mul => "*",
                crate::ast::BinaryOperator::Div => "/",
                crate::ast::BinaryOperator::Mod => "%",
                crate::ast::BinaryOperator::Eq => "==",
                crate::ast::BinaryOperator::Neq => "!=",
                crate::ast::BinaryOperator::Gt => ">",
                crate::ast::BinaryOperator::Lt => "<",
                crate::ast::BinaryOperator::Gte => ">=",
                crate::ast::BinaryOperator::Lte => "<=",
                crate::ast::BinaryOperator::And => "AND",
                crate::ast::BinaryOperator::Or => "OR",
            },
            render_expression(right)
        ),
        crate::ast::Expression::FunctionCall { name, args } => format!(
            "{}({})",
            name,
            args.iter()
                .map(render_expression)
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

fn match_declared_chain(
    chains: &[Vec<String>],
    action_target: &str,
    wait_sensor: &str,
) -> Option<Vec<String>> {
    let mut best: Option<Vec<String>> = None;

    for chain in chains {
        let Some(wait_index) = chain.iter().position(|node| node == wait_sensor) else {
            continue;
        };

        let Some(action_index) = chain
            .iter()
            .take(wait_index + 1)
            .position(|node| node == action_target)
        else {
            continue;
        };

        if action_index >= wait_index {
            continue;
        }

        let candidate = chain[..=wait_index].to_vec();
        let is_better = best
            .as_ref()
            .map(|existing| candidate.len() < existing.len())
            .unwrap_or(true);

        if is_better {
            best = Some(candidate);
        }
    }

    best
}

fn shortest_output_path_to_target(
    runtime_graph: &RuntimeGraph,
    output_ports: &[String],
    target: &str,
) -> Option<Vec<String>> {
    let mut best: Option<Vec<String>> = None;

    for output in output_ports {
        let Some(path) = shortest_path(runtime_graph, output, target) else {
            continue;
        };

        let is_better = best
            .as_ref()
            .map(|existing| path.len() < existing.len())
            .unwrap_or(true);

        if is_better {
            best = Some(path);
        }
    }

    best
}

fn shortest_path(runtime_graph: &RuntimeGraph, from: &str, to: &str) -> Option<Vec<String>> {
    let source = *runtime_graph.nodes.get(from)?;
    let target = *runtime_graph.nodes.get(to)?;

    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();
    let mut previous = HashMap::<NodeIndex, NodeIndex>::new();

    queue.push_back(source);
    visited.insert(source);

    while let Some(node) = queue.pop_front() {
        if node == target {
            break;
        }

        for neighbor in runtime_graph.graph.neighbors(node) {
            if visited.insert(neighbor) {
                previous.insert(neighbor, node);
                queue.push_back(neighbor);
            }
        }
    }

    if !visited.contains(&target) {
        return None;
    }

    let mut path_indices = vec![target];
    let mut cursor = target;
    while cursor != source {
        let parent = *previous.get(&cursor)?;
        path_indices.push(parent);
        cursor = parent;
    }
    path_indices.reverse();

    Some(
        path_indices
            .into_iter()
            .map(|index| runtime_graph.graph[index].clone())
            .collect(),
    )
}

fn join_paths(left: &[String], right: &[String]) -> Vec<String> {
    let mut joined = left.to_vec();

    for node in right {
        if joined.last() != Some(node) {
            joined.push(node.clone());
        }
    }

    joined
}

fn build_fallback_details(
    pair: &ActionWaitPair,
    source_path: &Option<Vec<String>>,
    feedback_path: &Option<Vec<String>>,
    output_ports: &[String],
) -> (String, String, String, String) {
    if source_path.is_none() {
        if feedback_path.is_some() {
            return (
                format!("{} -> {}", pair.action_target, pair.wait_sensor),
                format!("{} -> {}", pair.action_target, pair.wait_sensor),
                format!("{} -> ???", pair.action_target),
                format!(
                    "请补充 {} 到 {} 的因果链定义（设备关系、变量赋值或外部函数传播）",
                    pair.action_target, pair.wait_sensor
                ),
            );
        }
        let output = output_ports
            .first()
            .cloned()
            .unwrap_or_else(|| "<输出端口>".to_string());
        return (
            format!("{output} -> {}", pair.action_target),
            format!("{output} -> {} -> {}", pair.action_target, pair.wait_sensor),
            format!("{output} -> ???"),
            format!(
                "请检查 {} 的 driven_by/reports_to 链路，确保它可由输出端口驱动",
                pair.action_target
            ),
        );
    }

    let source_path = source_path.as_ref().expect("source path exists above");

    if feedback_path.is_none() {
        return (
            format!("{} -> {}", pair.action_target, pair.wait_sensor),
            format!("{} -> {}", source_path.join(" -> "), pair.wait_sensor),
            format!("{} -> ???", source_path.join(" -> ")),
            format!(
                "请补充 {} 的 detects/driven_by/reports_to 声明，确保动作后能反馈到 {}",
                pair.wait_sensor, pair.wait_sensor
            ),
        );
    }

    (
        format!("{} -> {}", pair.action_target, pair.wait_sensor),
        format!("{} -> {}", source_path.join(" -> "), pair.wait_sensor),
        format!("{} -> ???", source_path.join(" -> ")),
        format!(
            "请检查 {} 与 {} 之间的物理连接定义",
            pair.action_target, pair.wait_sensor
        ),
    )
}

fn suggestion_for_link(from: &str, to: &str) -> String {
    format!(
        "请在 [topology] 中检查 {to} 的 driven_by/reports_to/detects 配置，确保链路 {from} -> {to} 可达"
    )
}

#[cfg(test)]
mod tests {
    use super::verify_causality;
    use crate::parser::parse_plc;
    use crate::semantic::{build_constraint_set, build_topology_graph};

    #[test]
    fn verifies_prd_5_4_causality_chains() {
        let source = r#"
[topology]

device Y0: digital_output
device Y1: digital_output
device X0: digital_input
device X1: digital_input

device valve_A: solenoid_valve {
    response_time: 20ms
}

device valve_B: solenoid_valve {
    response_time: 20ms
}

device cyl_A: cylinder {
    stroke_time: 300ms,
    retract_time: 300ms
}

device cyl_B: cylinder {
    stroke_time: 300ms,
    retract_time: 300ms
}

device sensor_A_ext: sensor
device sensor_B_ext: sensor

relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: Y1.out, to: valve_B.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: valve_B.out, to: cyl_B.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_A_ext.sense, via: detects }
relation { from: sensor_A_ext.out, to: X0.in, via: reports_to }
relation { from: cyl_B.extended, to: sensor_B_ext.sense, via: detects }
relation { from: sensor_B_ext.out, to: X1.in, via: reports_to }

[constraints]

causality: Y0 -> valve_A -> cyl_A -> sensor_A_ext
causality: Y1 -> valve_B -> cyl_B -> sensor_B_ext

[tasks]

task init:
    step extend_A:
        action: extend cyl_A
        wait: sensor_A_ext == true
    step extend_B:
        action: extend cyl_B
        wait: sensor_B_ext == true
"#;

        let program = parse_plc(source).expect("PRD 5.4 示例应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应能构建");
        let constraints = build_constraint_set(&program).expect("约束应能构建");

        verify_causality(&program, &topology, &constraints)
            .expect("PRD 5.4 示例中的因果链应全部通过");
    }

    #[test]
    fn reports_broken_chain_when_valve_is_not_connected_to_cylinder() {
        let source = r#"
[topology]

device Y0: digital_output
device X0: digital_input

device valve_A: solenoid_valve {
    response_time: 20ms
}

device cyl_A: cylinder {
    stroke_time: 300ms,
    retract_time: 300ms
}

device sensor_A_ext: sensor

relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: cyl_A.extended, to: sensor_A_ext.sense, via: detects }
relation { from: sensor_A_ext.out, to: X0.in, via: reports_to }

[constraints]

causality: Y0 -> valve_A -> cyl_A -> sensor_A_ext

[tasks]

task init:
    step extend_A:
        action: extend cyl_A
        wait: sensor_A_ext == true
"#;

        let program = parse_plc(source).expect("测试输入应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应能构建");
        let constraints = build_constraint_set(&program).expect("约束应能构建");

        let errors = verify_causality(&program, &topology, &constraints)
            .expect_err("缺失 valve_A -> cyl_A 链路时应报错");

        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("ERROR [causality] 因果链断裂")),
            "错误应包含因果链断裂标题"
        );
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("断裂链路: valve_A -> cyl_A")),
            "错误应指出断裂的链路"
        );
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("动作: extend cyl_A")),
            "错误应包含 action+wait 推断得到的动作信息"
        );
        assert!(
            errors.iter().all(|error| error.line > 0),
            "所有错误都应包含有效行号"
        );
    }

    #[test]
    fn skips_cross_branch_false_positive_for_parallel_action_and_step_wait() {
        let source = r#"
[topology]

device Y0: digital_output
device Y1: digital_output
device X0: digital_input
device X1: digital_input

device motor_left: motor
device motor_right: motor
device sensor_left: sensor
device sensor_right: sensor

relation { from: Y0.out, to: motor_left.cmd, via: driven_by }
relation { from: Y1.out, to: motor_right.cmd, via: driven_by }
relation { from: motor_left.on, to: sensor_left.sense, via: detects }
relation { from: sensor_left.out, to: X0.in, via: reports_to }
relation { from: motor_right.on, to: sensor_right.sense, via: detects }
relation { from: sensor_right.out, to: X1.in, via: reports_to }

[constraints]

causality: Y0 -> motor_left -> sensor_left
causality: Y1 -> motor_right -> sensor_right

[tasks]

task main:
    step start_both:
        parallel:
            branch_left:
                action: set motor_left.run on
            branch_right:
                action: set motor_right.run on
        wait: sensor_left == true
"#;

        let program = parse_plc(source).expect("测试输入应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应能构建");
        let constraints = build_constraint_set(&program).expect("约束应能构建");

        verify_causality(&program, &topology, &constraints)
            .expect("parallel 分支跨设备组合不应误报 motor_right -> sensor_left 因果链错误");
    }

    #[test]
    fn still_reports_real_broken_chain_inside_parallel_branch() {
        let source = r#"
[topology]

device Y0: digital_output
device Y1: digital_output
device Y2: digital_output
device X0: digital_input

device motor_left: motor
device motor_right: motor
device motor_aux: motor
device sensor_left: sensor

relation { from: Y0.out, to: motor_left.cmd, via: driven_by }
relation { from: Y1.out, to: motor_right.cmd, via: driven_by }
relation { from: Y2.out, to: motor_aux.cmd, via: driven_by }
relation { from: motor_aux.on, to: sensor_left.sense, via: detects }
relation { from: sensor_left.out, to: X0.in, via: reports_to }

[constraints]

causality: Y0 -> motor_left -> sensor_left

[tasks]

task main:
    step run_parallel:
        parallel:
            branch_left:
                action: set motor_left.run on
                wait: sensor_left == true
            branch_right:
                action: set motor_right.run on
"#;

        let program = parse_plc(source).expect("测试输入应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应能构建");
        let constraints = build_constraint_set(&program).expect("约束应能构建");

        let errors = verify_causality(&program, &topology, &constraints)
            .expect_err("parallel 分支内真实链路断裂应被检出");

        assert!(
            errors
                .iter()
                .any(|error| error.broken_link == "motor_left -> sensor_left"),
            "错误应包含 parallel 分支内真实断裂链路"
        );
    }

    #[test]
    fn reports_causality_error_when_one_sensor_in_and_wait_lacks_chain() {
        let source = r#"
[topology]

device Y0: digital_output
device Y1: digital_output
device X0: digital_input
device X1: digital_input

device valve_A: solenoid_valve
device valve_B: solenoid_valve

device cyl_A: cylinder {
    stroke_time: 200ms
    retract_time: 200ms
}

device cyl_B: cylinder {
    stroke_time: 200ms
    retract_time: 200ms
}

device sensor_A_ext: sensor
device sensor_A_ext2: sensor

relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: Y1.out, to: valve_B.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: valve_B.out, to: cyl_B.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_A_ext.sense, via: detects }
relation { from: sensor_A_ext.out, to: X0.in, via: reports_to }
relation { from: cyl_B.extended, to: sensor_A_ext2.sense, via: detects }
relation { from: sensor_A_ext2.out, to: X1.in, via: reports_to }

[constraints]

causality: Y0 -> valve_A -> cyl_A -> sensor_A_ext

[tasks]

task main:
    step extend:
        action: extend cyl_A
        wait: sensor_A_ext == true AND sensor_A_ext2 == true
"#;

        let program = parse_plc(source).expect("测试输入应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应能构建");
        let constraints = build_constraint_set(&program).expect("约束应能构建");

        let errors = verify_causality(&program, &topology, &constraints)
            .expect_err("AND wait 中某个传感器无链路时应报 causality 错误");

        assert!(
            errors.iter().any(|error| error.wait.as_deref()
                == Some("sensor_A_ext == true AND sensor_A_ext2 == true")),
            "错误应关联 AND wait 条件"
        );
        assert!(
            errors
                .iter()
                .any(|error| error.broken_link == "cyl_A -> sensor_A_ext2"),
            "错误应定位到缺失链路的传感器"
        );
    }

    #[test]
    fn infers_causality_for_analog_input_by_default() {
        let source = r#"
[topology]

device Y0: digital_output

device motor_A: motor
relation { from: Y0.out, to: motor_A.cmd, via: driven_by }

device pressure_in: analog_input {
    range: 0..10
}

[constraints]

[tasks]

task main:
    step run:
        action: set motor_A.run on
        wait: pressure_in > 5
"#;

        let program = parse_plc(source).expect("测试输入应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应能构建");
        let constraints = build_constraint_set(&program).expect("约束应能构建");

        let errors = verify_causality(&program, &topology, &constraints)
            .expect_err("analog_input 默认参与因果推断，应报缺失链路错误");

        assert!(
            errors
                .iter()
                .any(|error| error.broken_link == "motor_A -> pressure_in"),
            "错误应包含 analog_input 缺失链路"
        );
    }

    #[test]
    fn skips_external_analog_input_from_causality_inference() {
        let source = r#"
[topology]

device Y0: digital_output

device motor_A: motor
relation { from: Y0.out, to: motor_A.cmd, via: driven_by }

device pressure_in: analog_input {
    range: 0..10,
    external: true
}

[constraints]

[tasks]

task main:
    step run:
        action: set motor_A.run on
        wait: pressure_in > 5
"#;

        let program = parse_plc(source).expect("测试输入应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应能构建");
        let constraints = build_constraint_set(&program).expect("约束应能构建");

        verify_causality(&program, &topology, &constraints)
            .expect("external analog_input 应跳过因果推断");
    }

    #[test]
    fn accepts_encoder_cam_servo_chain_with_cam_actions() {
        let source = r#"
[topology]

device encoder_main: analog_input { range: 0..360 }
device servo_axis: motor
device sensor_sync: sensor

device cam_xy: cam_coupling {
    master: encoder_main,
    slave: servo_axis,
    table: cam_a,
}

cam_table cam_a: periodic [
    (0, 0),
    (180, 90),
    (360, 0),
]
cam_table cam_b: periodic [
    (0, 40),
    (180, 120),
    (360, 40),
]

relation { from: servo_axis.on, to: sensor_sync.sense, via: detects }

[constraints]

causality: encoder_main -> cam_xy -> servo_axis -> sensor_sync
causality: cam_xy -> servo_axis -> sensor_sync

[tasks]

task main:
    step engage_cam:
        action: cam_engage cam_xy
        wait: sensor_sync == true
    step switch_cam:
        action: cam_switch cam_xy cam_b
        wait: sensor_sync == true
    step phase_cam:
        action: cam_phase cam_xy 15.0
        wait: sensor_sync == true
"#;

        let program = parse_plc(source).expect("测试输入应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应能构建");
        let constraints = build_constraint_set(&program).expect("约束应能构建");

        verify_causality(&program, &topology, &constraints)
            .expect("encoder -> cam -> servo 因果链与 cam 动作关联路径应通过");
    }

    #[test]
    fn reports_broken_encoder_cam_servo_chain_when_master_disconnects() {
        let source = r#"
[topology]

device encoder_main: analog_input { range: 0..360 }
device encoder_aux: analog_input { range: 0..360 }
device servo_axis: motor
device sensor_sync: sensor

device cam_xy: cam_coupling {
    master: encoder_aux,
    slave: servo_axis,
    table: cam_a,
}

cam_table cam_a: periodic [
    (0, 0),
    (180, 90),
    (360, 0),
]
cam_table cam_b: periodic [
    (0, 40),
    (180, 120),
    (360, 40),
]

relation { from: servo_axis.on, to: sensor_sync.sense, via: detects }

[constraints]

causality: encoder_main -> cam_xy -> servo_axis -> sensor_sync
causality: cam_xy -> servo_axis -> sensor_sync

[tasks]

task main:
    step engage_cam:
        action: cam_engage cam_xy
        wait: sensor_sync == true
    step switch_cam:
        action: cam_switch cam_xy cam_b
        wait: sensor_sync == true
    step phase_cam:
        action: cam_phase cam_xy 15.0
        wait: sensor_sync == true
"#;

        let program = parse_plc(source).expect("测试输入应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应能构建");
        let constraints = build_constraint_set(&program).expect("约束应能构建");

        let errors = verify_causality(&program, &topology, &constraints)
            .expect_err("encoder -> cam 链路断裂应报 causality 错误");
        assert!(
            errors
                .iter()
                .any(|error| error.broken_link == "encoder_main -> cam_xy"),
            "错误应定位 encoder -> cam 断链"
        );
    }

    #[test]
    fn accepts_cross_task_variable_chain_with_compute_dataflow() {
        let source = r#"
[topology]

variable upstream_ready: bool = false
variable release_ok: bool = false

[constraints]

causality: upstream_ready -> release_ok

[tasks]

task feeder:
    step publish:
        action: compute upstream_ready = true
task picker:
    step capture:
        action: compute release_ok = upstream_ready
    step wait_release:
        wait: release_ok == true
"#;

        let program = parse_plc(source).expect("测试输入应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应能构建");
        let constraints = build_constraint_set(&program).expect("约束应能构建");

        verify_causality(&program, &topology, &constraints)
            .expect("跨 task 变量传播链应被因果图识别并通过");
    }

    #[test]
    fn reports_missing_cross_task_variable_chain_when_compute_dependency_absent() {
        let source = r#"
[topology]

variable upstream_ready: bool = false
variable release_ok: bool = false

[constraints]

causality: upstream_ready -> release_ok

[tasks]

task feeder:
    step publish:
        action: compute upstream_ready = true
task picker:
    step capture:
        action: compute release_ok = true
    step wait_release:
        wait: release_ok == true
"#;

        let program = parse_plc(source).expect("测试输入应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应能构建");
        let constraints = build_constraint_set(&program).expect("约束应能构建");

        let errors = verify_causality(&program, &topology, &constraints)
            .expect_err("缺失跨 task 变量依赖时应报 causality 错误");
        assert!(
            errors
                .iter()
                .any(|error| error.broken_link == "upstream_ready -> release_ok"),
            "错误应定位到跨 task 变量链缺失"
        );
    }

    #[test]
    fn verifies_axis_done_branch_wait_causality_when_links_exist() {
        let source = r#"
[topology]

device Y0: digital_output
device X0: digital_input
device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default, motion_param_set: stepper_default_fast }
device sensor_done: sensor

relation { from: Y0.out, to: axis_x.enable, via: driven_by }
relation { from: axis_x.fault, to: sensor_done.sense, via: detects }
relation { from: sensor_done.out, to: X0.in, via: reports_to }

[constraints]

causality: axis_x -> sensor_done

[tasks]

task main:
    step move:
        action: axis.move_relative(axis_x, distance: 5, speed: 10)
            timeout: 100ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
    step confirm:
        wait: sensor_done == true
task fault:
    step timeout:
        action: log "timeout"
    step reject:
        action: log "reject"
    step motion_fault:
        action: log "motion"
    step safety_fault:
        action: log "safety"
"#;

        let program = parse_plc(source).expect("测试输入应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应能构建");
        let constraints = build_constraint_set(&program).expect("约束应能构建");

        verify_causality(&program, &topology, &constraints)
            .expect("axis done 分支后的等待应参与因果验证并通过");
    }

    #[test]
    fn reports_missing_axis_done_branch_causality_path() {
        let source = r#"
[topology]

device Y0: digital_output
device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default, motion_param_set: stepper_default_fast }
device sensor_done: sensor

relation { from: Y0.out, to: axis_x.enable, via: driven_by }

[constraints]

causality: axis_x -> sensor_done

[tasks]

task main:
    step move:
        action: axis.move_relative(axis_x, distance: 5, speed: 10)
            timeout: 100ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
    step confirm:
        wait: sensor_done == true
task fault:
    step timeout:
        action: log "timeout"
    step reject:
        action: log "reject"
    step motion_fault:
        action: log "motion"
    step safety_fault:
        action: log "safety"
"#;

        let program = parse_plc(source).expect("测试输入应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应能构建");
        let constraints = build_constraint_set(&program).expect("约束应能构建");

        let errors = verify_causality(&program, &topology, &constraints)
            .expect_err("axis done 分支缺失因果链应报错");
        assert!(
            errors.iter().any(|error| error
                .action
                .as_deref()
                .unwrap_or_default()
                .contains("[done -> main.confirm]")),
            "诊断动作文本应标注 done -> main.confirm 分支"
        );
        assert!(
            errors
                .iter()
                .any(|error| error.broken_link == "axis_x -> sensor_done"),
            "应定位 axis done 分支缺失的 axis->sensor 链路"
        );
    }

    #[test]
    fn verifies_axis_fault_branch_wait_causality_when_links_exist() {
        let source = r#"
[topology]

device Y0: digital_output
device X0: digital_input
device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default, motion_param_set: stepper_default_fast }
device sensor_fault: sensor

relation { from: Y0.out, to: axis_x.enable, via: driven_by }
relation { from: axis_x.fault, to: sensor_fault.sense, via: detects }
relation { from: sensor_fault.out, to: X0.in, via: reports_to }

[constraints]

[tasks]

task main:
    step move:
        action: axis.move_relative(axis_x, distance: 5, speed: 10)
            timeout: 100ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
task fault:
    step timeout:
        action: log "timeout"
    step reject:
        action: log "reject"
    step motion_fault:
        wait: sensor_fault == true
    step safety_fault:
        action: log "safety"
"#;

        let program = parse_plc(source).expect("测试输入应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应能构建");
        let constraints = build_constraint_set(&program).expect("约束应能构建");

        verify_causality(&program, &topology, &constraints)
            .expect("axis motion 故障分支等待应参与因果验证并通过");
    }

    #[test]
    fn verifies_axis_timeout_branch_wait_causality_when_links_exist() {
        let source = r#"
[topology]

device Y0: digital_output
device X0: digital_input
device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default, motion_param_set: stepper_default_fast }
device sensor_fault: sensor

relation { from: Y0.out, to: axis_x.enable, via: driven_by }
relation { from: axis_x.fault, to: sensor_fault.sense, via: detects }
relation { from: sensor_fault.out, to: X0.in, via: reports_to }

[constraints]

[tasks]

task main:
    step move:
        action: axis.move_relative(axis_x, distance: 5, speed: 10)
            timeout: 100ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
task fault:
    step timeout:
        wait: sensor_fault == true
    step reject:
        action: log "reject"
    step motion_fault:
        action: log "motion"
    step safety_fault:
        action: log "safety"
"#;

        let program = parse_plc(source).expect("测试输入应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应能构建");
        let constraints = build_constraint_set(&program).expect("约束应能构建");

        verify_causality(&program, &topology, &constraints)
            .expect("axis timeout 分支等待应参与因果验证并通过");
    }

    #[test]
    fn reports_missing_axis_fault_branch_causality_path() {
        let source = r#"
[topology]

device Y0: digital_output
device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default, motion_param_set: stepper_default_fast }
device sensor_fault: sensor

relation { from: Y0.out, to: axis_x.enable, via: driven_by }

[constraints]

[tasks]

task main:
    step move:
        action: axis.move_relative(axis_x, distance: 5, speed: 10)
            timeout: 100ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
task fault:
    step timeout:
        action: log "timeout"
    step reject:
        action: log "reject"
    step motion_fault:
        wait: sensor_fault == true
    step safety_fault:
        action: log "safety"
"#;

        let program = parse_plc(source).expect("测试输入应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应能构建");
        let constraints = build_constraint_set(&program).expect("约束应能构建");

        let errors = verify_causality(&program, &topology, &constraints)
            .expect_err("axis motion 故障分支缺失因果链应报错");

        assert!(
            errors.iter().any(|error| error
                .action
                .as_deref()
                .unwrap_or_default()
                .contains("on_motion_fault")),
            "诊断动作文本应标注 on_motion_fault 分支"
        );
        assert!(
            errors
                .iter()
                .any(|error| error.broken_link == "axis_x -> sensor_fault"),
            "应定位轴故障分支缺失的 axis->sensor 链路"
        );
    }

    #[test]
    fn reports_missing_axis_timeout_branch_causality_path() {
        let source = r#"
[topology]

device Y0: digital_output
device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default, motion_param_set: stepper_default_fast }
device sensor_fault: sensor

relation { from: Y0.out, to: axis_x.enable, via: driven_by }

[constraints]

[tasks]

task main:
    step move:
        action: axis.move_relative(axis_x, distance: 5, speed: 10)
            timeout: 100ms -> fault.timeout
            on_reject -> fault.reject
            on_motion_fault -> fault.motion_fault
            on_safety_fault -> fault.safety_fault
task fault:
    step timeout:
        wait: sensor_fault == true
    step reject:
        action: log "reject"
    step motion_fault:
        action: log "motion"
    step safety_fault:
        action: log "safety"
"#;

        let program = parse_plc(source).expect("测试输入应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应能构建");
        let constraints = build_constraint_set(&program).expect("约束应能构建");

        let errors = verify_causality(&program, &topology, &constraints)
            .expect_err("axis timeout 分支缺失因果链应报错");

        assert!(
            errors.iter().any(|error| error
                .action
                .as_deref()
                .unwrap_or_default()
                .contains("timeout")),
            "诊断动作文本应标注 timeout 分支"
        );
        assert!(
            errors
                .iter()
                .any(|error| error.broken_link == "axis_x -> sensor_fault"),
            "应定位轴 timeout 分支缺失的 axis->sensor 链路"
        );
    }

    #[test]
    fn verifies_cylinder_fault_branch_wait_causality_when_links_exist() {
        let source = r#"
[topology]

device Y0: digital_output
device X0: digital_input
device valve_a: solenoid_valve
device cyl_a: cylinder
device sensor_fault: sensor

relation { from: Y0.out, to: valve_a.coil, via: driven_by }
relation { from: valve_a.out, to: cyl_a.cmd, via: driven_by }
relation { from: cyl_a.extended, to: sensor_fault.sense, via: detects }
relation { from: sensor_fault.out, to: X0.in, via: reports_to }

[constraints]

[tasks]

task main:
    step move:
        action: extend cyl_a
        on_motion_fault -> fault.motion_fault
        on_safety_fault -> fault.safety_fault
task fault:
    step motion_fault:
        wait: sensor_fault == true
    step safety_fault:
        action: log "safety"
"#;

        let program = parse_plc(source).expect("测试输入应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应能构建");
        let constraints = build_constraint_set(&program).expect("约束应能构建");

        verify_causality(&program, &topology, &constraints)
            .expect("cylinder motion fault branch wait should participate in causality");
    }

    #[test]
    fn reports_missing_cylinder_fault_branch_causality_path() {
        let source = r#"
[topology]

device Y0: digital_output
device valve_a: solenoid_valve
device cyl_a: cylinder
device sensor_fault: sensor

relation { from: Y0.out, to: valve_a.coil, via: driven_by }
relation { from: valve_a.out, to: cyl_a.cmd, via: driven_by }

[constraints]

[tasks]

task main:
    step move:
        action: extend cyl_a
        on_motion_fault -> fault.motion_fault
        on_safety_fault -> fault.safety_fault
task fault:
    step motion_fault:
        wait: sensor_fault == true
    step safety_fault:
        action: log "safety"
"#;

        let program = parse_plc(source).expect("测试输入应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应能构建");
        let constraints = build_constraint_set(&program).expect("约束应能构建");

        let errors = verify_causality(&program, &topology, &constraints)
            .expect_err("missing cylinder -> sensor path should fail causality");

        assert!(
            errors.iter().any(|error| error
                .action
                .as_deref()
                .unwrap_or_default()
                .contains("on_motion_fault")),
            "诊断动作文本应标注 on_motion_fault 分支"
        );
        assert!(
            errors
                .iter()
                .any(|error| error.broken_link == "cyl_a -> sensor_fault"),
            "应定位气缸分支缺失的 cylinder->sensor 链路"
        );
    }

    #[test]
    fn accepts_causality_chains_with_pure_extern_call_nodes() {
        let source = r#"
[topology]

device pressure_in: analog_input {
    range: 0..10
}
variable normalized: float = 0.0
extern function normalize(v: float) -> float {
    rust_module: "math::normalize"
    pure: true
    time_bound_us: 80
}

[constraints]

causality: pressure_in -> normalize -> normalized
causality: pressure_in -> normalized

[tasks]

task main:
    step run:
        action: call normalize(pressure_in) -> normalized
"#;

        let program = parse_plc(source).expect("测试输入应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应能构建");
        let constraints = build_constraint_set(&program).expect("约束应能构建");

        verify_causality(&program, &topology, &constraints)
            .expect("pure extern 应在因果图中作为确定性变换节点参与传播");
    }

    #[test]
    fn reports_broken_chain_when_non_pure_extern_is_used_for_propagation() {
        let source = r#"
[topology]

device pressure_in: analog_input {
    range: 0..10
}
variable normalized: float = 0.0
extern function normalize(v: float) -> float {
    rust_module: "math::normalize"
    pure: false
    time_bound_us: 80
}

[constraints]

causality: pressure_in -> normalized

[tasks]

task main:
    step run:
        action: call normalize(pressure_in) -> normalized
"#;

        let program = parse_plc(source).expect("测试输入应能解析");
        let topology = build_topology_graph(&program).expect("拓扑应能构建");
        let constraints = build_constraint_set(&program).expect("约束应能构建");

        let errors = verify_causality(&program, &topology, &constraints)
            .expect_err("non-pure extern 不应通过因果传播");

        assert!(
            errors
                .iter()
                .any(|error| error.broken_link == "pressure_in -> normalized"),
            "错误应报告 non-pure extern 链路无法传播"
        );
    }
}
