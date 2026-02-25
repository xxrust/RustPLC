use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Device {
    pub name: String,
    pub kind: DeviceKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    DigitalOutput,
    DigitalInput,
    Plc,
    SolenoidValve,
    Cylinder,
    Sensor,
    Motor,
    StepperMotor,
    Vfd,
    ServoDrive,
    AnalogInput,
    AnalogOutput,
    Pid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PidLoop {
    pub name: String,
    pub pv: String,
    pub sp: String,
    pub kp: String,
    pub ki: String,
    pub kd: String,
    pub out: String,
    pub period_ms: u64,
    pub limit_min: String,
    pub limit_max: String,
    pub anti_windup: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionType {
    Electrical,
    Pneumatic,
    Logical,
    Analog,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TopologyLink {
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_port: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_port: Option<String>,
    pub kind: ConnectionType,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TopologyGraph {
    pub graph: DiGraph<Device, ConnectionType>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pid_loops: Vec<PidLoop>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<TopologyLink>,
}

impl TopologyGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            pid_loops: Vec::new(),
            links: Vec::new(),
        }
    }

    pub fn add_device(&mut self, device: Device) -> NodeIndex {
        self.graph.add_node(device)
    }

    pub fn add_connection(&mut self, from: NodeIndex, to: NodeIndex, kind: ConnectionType) {
        self.graph.add_edge(from, to, kind);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct State {
    pub task_name: String,
    pub step_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TransitionGuard {
    Always,
    Condition {
        expression: String,
    },
    Timeout {
        duration_ms: u64,
    },
    /// Internal bounded wait used by `delay: <duration>` DSL statements.
    Delay {
        duration_ms: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TransitionAction {
    Extend { target: String, port: String },
    Retract { target: String, port: String },
    Set { target: String, port: String, value: BinaryValue },
    SetAnalog { target: String, port: String, value_raw: String },
    Log { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BinaryValue {
    On,
    Off,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimerOperationKind {
    Start,
    Cancel,
    Reset,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimerOperation {
    pub timer_name: String,
    pub operation: TimerOperationKind,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Transition {
    pub from: State,
    pub to: State,
    pub guard: TransitionGuard,
    pub actions: Vec<TransitionAction>,
    pub timers: Vec<TimerOperation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct StateMachine {
    pub states: Vec<State>,
    pub transitions: Vec<Transition>,
    pub initial: State,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub analog_regions: BTreeMap<String, Vec<(String, String)>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateExpr {
    pub device: String,
    #[serde(default = "default_self_port")]
    pub port: String,
    pub state: String,
}

fn default_self_port() -> String {
    "self".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SafetyRelation {
    ConflictsWith,
    Requires,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SafetyExpr {
    State(StateExpr),
    Threshold {
        device: String,
        operator: String,
        value: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SafetyRule {
    pub left: SafetyExpr,
    pub relation: SafetyRelation,
    pub right: SafetyExpr,
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimingScope {
    Task { task: String },
    Step { task: String, step: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimingRelation {
    MustCompleteWithin,
    MustCompleteWithinWorstCase,
    MustStartAfter,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimingRule {
    pub scope: TimingScope,
    pub relation: TimingRelation,
    pub duration_ms: u64,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CausalityChain {
    pub devices: Vec<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ConstraintSet {
    pub safety: Vec<SafetyRule>,
    pub timing: Vec<TimingRule>,
    pub causality: Vec<CausalityChain>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Extend,
    Retract,
    Set,
    SetAnalog,
    Log,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionRef {
    pub task_name: String,
    pub step_name: String,
    pub action_kind: ActionKind,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimeInterval {
    pub min_ms: u64,
    pub max_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionTiming {
    pub action: ActionRef,
    pub interval: TimeInterval,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TimingModel {
    pub intervals: BTreeMap<String, ActionTiming>,
}

pub fn to_pretty_json<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use petgraph::visit::EdgeRef;

    #[test]
    fn topology_graph_supports_device_nodes_and_connection_edges() {
        let mut topology = TopologyGraph::new();

        let y0 = topology.add_device(Device {
            name: "Y0".to_string(),
            kind: DeviceKind::DigitalOutput,
        });
        let valve = topology.add_device(Device {
            name: "valve_A".to_string(),
            kind: DeviceKind::SolenoidValve,
        });

        topology.add_connection(y0, valve, ConnectionType::Electrical);

        assert_eq!(topology.graph.node_count(), 2);
        assert_eq!(topology.graph.edge_count(), 1);

        let edge = topology
            .graph
            .edge_references()
            .next()
            .expect("expected one edge");
        assert_eq!(edge.source(), y0);
        assert_eq!(edge.target(), valve);
        assert_eq!(edge.weight(), &ConnectionType::Electrical);
    }

    #[test]
    fn ir_structures_are_serializable_to_pretty_json() {
        let mut topology = TopologyGraph::new();
        let y0 = topology.add_device(Device {
            name: "Y0".to_string(),
            kind: DeviceKind::DigitalOutput,
        });
        let valve = topology.add_device(Device {
            name: "valve_A".to_string(),
            kind: DeviceKind::SolenoidValve,
        });
        topology.add_connection(y0, valve, ConnectionType::Electrical);

        let state_machine = StateMachine {
            states: vec![
                State {
                    task_name: "init".to_string(),
                    step_name: "extend_A".to_string(),
                },
                State {
                    task_name: "ready".to_string(),
                    step_name: "idle".to_string(),
                },
            ],
            transitions: vec![Transition {
                from: State {
                    task_name: "init".to_string(),
                    step_name: "extend_A".to_string(),
                },
                to: State {
                    task_name: "ready".to_string(),
                    step_name: "idle".to_string(),
                },
                guard: TransitionGuard::Condition {
                    expression: "sensor_A_ext == true".to_string(),
                },
                actions: vec![TransitionAction::Extend {
                    target: "cyl_A".to_string(),
                    port: "self".to_string(),
                }],
                timers: vec![TimerOperation {
                    timer_name: "extend_A_timeout".to_string(),
                    operation: TimerOperationKind::Start,
                    duration_ms: Some(600),
                }],
            }],
            initial: State {
                task_name: "init".to_string(),
                step_name: "extend_A".to_string(),
            },
            analog_regions: BTreeMap::new(),
        };

        let constraints = ConstraintSet {
            safety: vec![SafetyRule {
                left: SafetyExpr::State(StateExpr {
                    device: "cyl_A".to_string(),
                    port: "self".to_string(),
                    state: "extended".to_string(),
                }),
                relation: SafetyRelation::ConflictsWith,
                right: SafetyExpr::State(StateExpr {
                    device: "cyl_B".to_string(),
                    port: "self".to_string(),
                    state: "extended".to_string(),
                }),
                reason: Some("避免机械冲突".to_string()),
                source: None,
            }],
            timing: vec![TimingRule {
                scope: TimingScope::Task {
                    task: "extend_cycle".to_string(),
                },
                relation: TimingRelation::MustCompleteWithin,
                duration_ms: 500,
                reason: None,
            }],
            causality: vec![CausalityChain {
                devices: vec!["Y0".to_string(), "valve_A".to_string(), "cyl_A".to_string()],
                reason: None,
            }],
        };

        let mut timing_model = TimingModel::default();
        timing_model.intervals.insert(
            "init.extend_A.extend.cyl_A".to_string(),
            ActionTiming {
                action: ActionRef {
                    task_name: "init".to_string(),
                    step_name: "extend_A".to_string(),
                    action_kind: ActionKind::Extend,
                    target: Some("cyl_A".to_string()),
                },
                interval: TimeInterval {
                    min_ms: 180,
                    max_ms: 240,
                },
            },
        );

        let topology_json = to_pretty_json(&topology).expect("topology should serialize");
        let sm_json = to_pretty_json(&state_machine).expect("state machine should serialize");
        let constraints_json = to_pretty_json(&constraints).expect("constraints should serialize");
        let timing_json = to_pretty_json(&timing_model).expect("timing model should serialize");

        assert!(topology_json.contains("graph"));
        assert!(sm_json.contains("transitions"));
        assert!(constraints_json.contains("conflicts_with"));
        assert!(timing_json.contains("intervals"));

        let decoded_topology: TopologyGraph =
            serde_json::from_str(&topology_json).expect("topology should deserialize");
        assert_eq!(decoded_topology.graph.node_count(), 2);
        assert_eq!(decoded_topology.graph.edge_count(), 1);
    }
}
