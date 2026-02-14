use crate::ast::{
    ActionStatement, BinaryValue, ComparisonOperator, ConditionExpression, LiteralValue,
    PlcProgram, StepStatement, WaitCondition, WaitStatement,
};
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

    let sensor_names = collect_sensor_names(program);
    let output_ports = collect_output_ports(topology);
    let declared_chains: Vec<Vec<String>> = constraints
        .causality
        .iter()
        .map(|chain| chain.devices.clone())
        .collect();

    for pair in collect_action_wait_pairs(program, &sensor_names) {
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

        // `parallel` 块会让不同分支的 action 与 step 级 wait 产生组合。
        // 对于这类组合，只对位于传感器上游集合（action -> sensor 可达）的 action→sensor 对做推断检查，
        // 以避免跨分支误报。
        if pair.involves_parallel()
            && !runtime_graph.path_exists(&pair.action_target, &pair.wait_sensor)
        {
            continue;
        }

        let source_path =
            shortest_output_path_to_target(&runtime_graph, &output_ports, &pair.action_target);
        let feedback_path = shortest_path(&runtime_graph, &pair.action_target, &pair.wait_sensor);

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
    text: String,
    sensor: String,
    origin: StatementOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StatementOrigin {
    StepLevel,
    ParallelBranch {
        block_id: usize,
        branch_index: usize,
    },
}

impl ActionWaitPair {
    fn involves_parallel(&self) -> bool {
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

        Self { graph, nodes }
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

fn collect_sensor_names(program: &PlcProgram) -> HashSet<String> {
    program
        .topology
        .devices
        .iter()
        .filter(|device| {
            matches!(
                device.device_type,
                crate::ast::DeviceType::Sensor | crate::ast::DeviceType::AnalogInput
            )
        })
        .map(|device| device.name.clone())
        .collect()
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
    sensor_names: &HashSet<String>,
) -> Vec<ActionWaitPair> {
    let mut pairs = Vec::new();
    let mut next_parallel_block_id = 0usize;

    for task in &program.tasks.tasks {
        for step in &task.steps {
            collect_pairs_from_statements(
                &step.statements,
                step.line.max(1),
                sensor_names,
                &mut next_parallel_block_id,
                &mut pairs,
            );
        }
    }

    pairs
}

fn collect_pairs_from_statements(
    statements: &[StepStatement],
    line: usize,
    sensor_names: &HashSet<String>,
    next_parallel_block_id: &mut usize,
    pairs: &mut Vec<ActionWaitPair>,
) {
    let mut actions = Vec::new();
    let mut waits = Vec::new();

    collect_items_from_statements(
        statements,
        line,
        sensor_names,
        StatementOrigin::StepLevel,
        next_parallel_block_id,
        &mut actions,
        &mut waits,
        pairs,
    );

    for action in &actions {
        for wait in &waits {
            pairs.push(ActionWaitPair {
                line,
                action: action.text.clone(),
                action_target: action.target.clone(),
                wait: wait.text.clone(),
                wait_sensor: wait.sensor.clone(),
                action_origin: action.origin.clone(),
                wait_origin: wait.origin.clone(),
            });
        }
    }
}

fn collect_items_from_statements(
    statements: &[StepStatement],
    line: usize,
    sensor_names: &HashSet<String>,
    origin: StatementOrigin,
    next_parallel_block_id: &mut usize,
    actions: &mut Vec<CollectedAction>,
    waits: &mut Vec<CollectedWait>,
    pairs: &mut Vec<ActionWaitPair>,
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
            }
            StepStatement::Wait(wait) => {
                let wait_text = wait_to_text(wait);
                for sensor in infer_wait_sensors(wait, sensor_names) {
                    waits.push(CollectedWait {
                        text: wait_text.clone(),
                        sensor,
                        origin: origin.clone(),
                    });
                }
            }
            StepStatement::IfElse { .. } => {}
            StepStatement::Repeat { body, .. } => {
                collect_items_from_statements(
                    body,
                    line,
                    sensor_names,
                    origin.clone(),
                    next_parallel_block_id,
                    actions,
                    waits,
                    pairs,
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
                        sensor_names,
                        branch_origin,
                        next_parallel_block_id,
                        actions,
                        waits,
                        pairs,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    collect_pairs_from_statements(
                        &branch.statements,
                        line,
                        sensor_names,
                        next_parallel_block_id,
                        pairs,
                    );
                }
            }
            _ => {}
        }
    }
}

fn action_to_text_and_target(action: &ActionStatement) -> Option<(String, String)> {
    match action {
        ActionStatement::Extend { target } => Some((format!("extend {target}"), target.clone())),
        ActionStatement::Retract { target } => Some((format!("retract {target}"), target.clone())),
        ActionStatement::Set { target, value } => Some((
            format!("set {target} {}", binary_value_text(value)),
            target.clone(),
        )),
        ActionStatement::SetAnalog { target, value } => {
            Some((format!("set_analog {target} {value}"), target.clone()))
        }
        ActionStatement::Log { .. } => None,
    }
}

fn binary_value_text(value: &BinaryValue) -> &'static str {
    match value {
        BinaryValue::On => "on",
        BinaryValue::Off => "off",
    }
}

fn infer_wait_sensors(wait: &WaitStatement, sensor_names: &HashSet<String>) -> Vec<String> {
    let mut sensors = Vec::new();
    let mut seen = HashSet::new();

    for condition in wait_conditions(wait) {
        if sensor_names.contains(&condition.left) {
            if seen.insert(condition.left.clone()) {
                sensors.push(condition.left.clone());
            }
            continue;
        }

        if let Some(candidate) = condition.left.split('.').next()
            && sensor_names.contains(candidate)
        {
            let sensor = candidate.to_string();
            if seen.insert(sensor.clone()) {
                sensors.push(sensor);
            }
            continue;
        }

        if let LiteralValue::State(state) = &condition.right
            && sensor_names.contains(&state.device)
            && seen.insert(state.device.clone())
        {
            sensors.push(state.device.clone());
        }
    }

    sensors
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
        LiteralValue::String(value) => format!("\"{value}\""),
        LiteralValue::State(state) => format!("{}.{}", state.device, state.state),
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
        let output = output_ports
            .first()
            .cloned()
            .unwrap_or_else(|| "<输出端口>".to_string());
        return (
            format!("{output} -> {}", pair.action_target),
            format!("{output} -> {} -> {}", pair.action_target, pair.wait_sensor),
            format!("{output} -> ???"),
            format!(
                "请检查 {} 的 connected_to 链路，确保它可由输出端口驱动",
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
                "请补充 {} 的 detects/connected_to 声明，确保动作后能反馈到 {}",
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
        "请在 [topology] 中检查 {to} 的 connected_to / detects 配置，确保链路 {from} -> {to} 可达"
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
    connected_to: Y0,
    response_time: 20ms
}

device valve_B: solenoid_valve {
    connected_to: Y1,
    response_time: 20ms
}

device cyl_A: cylinder {
    connected_to: valve_A,
    stroke_time: 300ms,
    retract_time: 300ms
}

device cyl_B: cylinder {
    connected_to: valve_B,
    stroke_time: 300ms,
    retract_time: 300ms
}

device sensor_A_ext: sensor {
    connected_to: X0,
    detects: cyl_A.extended
}

device sensor_B_ext: sensor {
    connected_to: X1,
    detects: cyl_B.extended
}

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
    connected_to: Y0,
    response_time: 20ms
}

device cyl_A: cylinder {
    stroke_time: 300ms,
    retract_time: 300ms
}

device sensor_A_ext: sensor {
    connected_to: X0,
    detects: cyl_A.extended
}

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

device motor_left: motor {
    connected_to: Y0
}

device motor_right: motor {
    connected_to: Y1
}

device sensor_left: sensor {
    connected_to: X0
    detects: motor_left.on
}

device sensor_right: sensor {
    connected_to: X1
    detects: motor_right.on
}

[constraints]

causality: Y0 -> motor_left -> sensor_left
causality: Y1 -> motor_right -> sensor_right

[tasks]

task main:
    step start_both:
        parallel:
            branch_left:
                action: set motor_left on
            branch_right:
                action: set motor_right on
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

device motor_left: motor {
    connected_to: Y0
}

device motor_right: motor {
    connected_to: Y1
}

device motor_aux: motor {
    connected_to: Y2
}

device sensor_left: sensor {
    connected_to: X0
    detects: motor_aux.on
}

[constraints]

causality: Y0 -> motor_left -> sensor_left

[tasks]

task main:
    step run_parallel:
        parallel:
            branch_left:
                action: set motor_left on
                wait: sensor_left == true
            branch_right:
                action: set motor_right on
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

device valve_A: solenoid_valve {
    connected_to: Y0
}

device valve_B: solenoid_valve {
    connected_to: Y1
}

device cyl_A: cylinder {
    connected_to: valve_A
    stroke_time: 200ms
    retract_time: 200ms
}

device cyl_B: cylinder {
    connected_to: valve_B
    stroke_time: 200ms
    retract_time: 200ms
}

device sensor_A_ext: sensor {
    connected_to: X0
    detects: cyl_A.extended
}

device sensor_A_ext2: sensor {
    connected_to: X1
    detects: cyl_B.extended
}

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
}
