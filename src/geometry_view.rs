use std::collections::{BTreeMap, BTreeSet, HashMap};

use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};

use crate::intent_alignment::{
    IntentAlignmentBlockerKind, IntentAlignmentReport, IntentAlignmentVerdict, IntentMismatch,
    IntentMismatchKind,
};
use crate::ir::{
    ActionKind, ConstraintSet, DeviceKind, ResourceClaimSource, SemanticResourceMode, State,
    StateMachine, TimingRelation, TimingScope, TopologyGraph, TransitionAction, TransitionGuard,
    WorkpieceSiteKind,
};
use crate::trace_diff::NormalizedTraceEvent;

pub const GEOMETRY_VIEW_SCHEMA_VERSION: u32 = 2;
pub const GEOMETRY_VIEW_ARTIFACT_KIND: &str = "semantic_twin_geometry";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum GeometryViewKind {
    Constellation,
    Orbit,
    Evidence,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GeometryLaneKind {
    Topology,
    Task,
    Evidence,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GeometryNodeKind {
    Device,
    Task,
    Step,
    SemanticResource,
    ClaimSource,
    TimingRule,
    CausalityChain,
    WorkpieceSite,
    WorkpieceHolder,
    WorkpieceCarrier,
    ExternalReference,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GeometryEdgeKind {
    Contains,
    TopologyLink,
    Transition,
    ResourceClaim,
    TimingScope,
    Causality,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GeometryEvidenceStatus {
    Authored,
    Derived,
    Verified,
    Observed,
    Warning,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeometryArtifact {
    pub schema_version: u32,
    pub artifact_kind: String,
    pub source_path: String,
    pub summary: GeometrySummary,
    pub lanes: Vec<GeometryLane>,
    pub nodes: Vec<GeometryNode>,
    pub edges: Vec<GeometryEdge>,
    pub overlays: GeometryOverlays,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub narrative: Option<GeometryNarrative>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeometrySummary {
    pub task_count: usize,
    pub step_count: usize,
    pub transition_count: usize,
    pub device_count: usize,
    pub resource_count: usize,
    pub timing_rule_count: usize,
    pub causality_chain_count: usize,
    pub observed_transition_count: usize,
    pub intent_mismatch_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeometryLane {
    pub id: String,
    pub kind: GeometryLaneKind,
    pub label: String,
    pub position: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeometryNode {
    pub id: String,
    pub kind: GeometryNodeKind,
    pub label: String,
    pub lane_id: String,
    pub views: Vec<GeometryViewKind>,
    pub evidence_status: GeometryEvidenceStatus,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeometryEdge {
    pub id: String,
    pub kind: GeometryEdgeKind,
    pub from: String,
    pub to: String,
    pub label: String,
    pub views: Vec<GeometryViewKind>,
    pub evidence_status: GeometryEvidenceStatus,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct GeometryOverlays {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<GeometryTraceOverlay>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<GeometryIntentOverlay>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeometryTraceOverlay {
    pub observed_transition_count: usize,
    pub resolution: String,
    pub transitions: Vec<GeometryObservedTransition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeometryObservedTransition {
    pub tick: u64,
    pub task_index: usize,
    pub from_step: u16,
    pub to_step: u16,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeometryIntentOverlay {
    pub verdict: IntentAlignmentVerdict,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_mismatch_kind: Option<IntentMismatchKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocker_kind: Option<IntentAlignmentBlockerKind>,
    pub mismatch_count: usize,
    pub warnings: Vec<String>,
    pub mismatches: Vec<IntentMismatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeometryNarrative {
    pub tasks: Vec<GeometryNarrativeTask>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeometryNarrativeTask {
    pub task_id: String,
    pub label: String,
    pub entry_step_id: String,
    pub current_step_id: String,
    pub blocking_state: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_actions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub main_path_step_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocking_points: Vec<GeometryNarrativeBlockingPoint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fault_exits: Vec<GeometryNarrativeExit>,
    pub coverage: GeometryNarrativeCoverage,
    pub steps: Vec<GeometryNarrativeStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeometryNarrativeCoverage {
    pub uncovered_step_count: usize,
    pub trace_available: bool,
    pub intent_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeometryNarrativeBlockingPoint {
    pub step_id: String,
    pub step_label: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub release_transitions: Vec<GeometryNarrativeTransitionRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timeout_transitions: Vec<GeometryNarrativeTransitionRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeometryNarrativeExit {
    pub from_step_id: String,
    pub from_step_label: String,
    pub via: GeometryNarrativeTransitionRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeometryNarrativeTransitionRef {
    pub transition_id: String,
    pub guard_kind: String,
    pub guard_label: String,
    pub to_step_id: String,
    pub to_step_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeometryNarrativeStep {
    pub step_id: String,
    pub label: String,
    pub index: usize,
    pub is_initial: bool,
    pub is_current: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub incoming_transition_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outgoing: Vec<GeometryNarrativeTransition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub device_chains: Vec<GeometryNarrativeDeviceChain>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_chain_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeometryNarrativeTransition {
    pub transition_id: String,
    pub to_step_id: String,
    pub to_step_label: String,
    pub guard_kind: String,
    pub guard_label: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timers: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<GeometryNarrativeAction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<String>,
    pub observed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeometryNarrativeAction {
    pub kind: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_device_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_port: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeometryNarrativeDeviceChain {
    pub source_kind: String,
    pub explanation: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub command_devices: Vec<GeometryNarrativeDeviceRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actuator_devices: Vec<GeometryNarrativeDeviceRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub feedback_devices: Vec<GeometryNarrativeDeviceRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub io_devices: Vec<GeometryNarrativeDeviceRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_chain_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeometryNarrativeDeviceRef {
    pub device_id: String,
    pub label: String,
    pub kind: String,
}

pub fn export_geometry_artifact(
    source_path: &str,
    topology: &TopologyGraph,
    constraints: &ConstraintSet,
    state_machine: &StateMachine,
    trace_events: Option<&[NormalizedTraceEvent]>,
    intent_report: Option<&IntentAlignmentReport>,
) -> GeometryArtifact {
    let mut lanes = Vec::new();
    lanes.push(GeometryLane {
        id: "topology".to_string(),
        kind: GeometryLaneKind::Topology,
        label: "Topology".to_string(),
        position: 0,
    });
    lanes.push(GeometryLane {
        id: "evidence".to_string(),
        kind: GeometryLaneKind::Evidence,
        label: "Evidence".to_string(),
        position: 1,
    });

    let mut task_names = BTreeSet::new();
    for task in &state_machine.task_contexts {
        task_names.insert(task.task_name.clone());
    }
    for state in &state_machine.states {
        task_names.insert(state.task_name.clone());
    }

    for (idx, task_name) in task_names.iter().enumerate() {
        lanes.push(GeometryLane {
            id: format!("task:{task_name}"),
            kind: GeometryLaneKind::Task,
            label: task_name.clone(),
            position: (idx as u32) + 2,
        });
    }

    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    let mut step_ids = BTreeSet::new();
    let task_contexts = state_machine
        .task_contexts
        .iter()
        .map(|context| (context.task_name.clone(), context))
        .collect::<HashMap<_, _>>();

    for task_name in &task_names {
        let mut attributes = BTreeMap::new();
        if let Some(context) = task_contexts.get(task_name) {
            attributes.insert("entry_state".to_string(), state_key(&context.entry_state));
            attributes.insert(
                "current_state".to_string(),
                state_key(&context.current_state),
            );
            attributes.insert(
                "blocking_state".to_string(),
                blocking_state_name(&context.blocking_state).to_string(),
            );
            if !context.pending_actions.is_empty() {
                let pending = context
                    .pending_actions
                    .iter()
                    .map(|action| {
                        let mut out = action_kind_name(&action.action_kind).to_string();
                        if let Some(target) = &action.target {
                            out.push(':');
                            out.push_str(target);
                        }
                        out
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                attributes.insert("pending_actions".to_string(), pending);
            }
            if !context.timers.is_empty() {
                let timers = context
                    .timers
                    .iter()
                    .map(|timer| {
                        let duration = timer
                            .duration_ms
                            .map(|value| format!("{value}ms"))
                            .unwrap_or_else(|| "open".to_string());
                        format!("{}:{duration}", timer.timer_name)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                attributes.insert("timers".to_string(), timers);
            }
        }
        nodes.push(GeometryNode {
            id: format!("task:{task_name}"),
            kind: GeometryNodeKind::Task,
            label: task_name.clone(),
            lane_id: format!("task:{task_name}"),
            views: vec![GeometryViewKind::Constellation, GeometryViewKind::Orbit],
            evidence_status: GeometryEvidenceStatus::Derived,
            attributes,
        });
    }

    for state in &state_machine.states {
        let step_id = format!("step:{}", state_key(state));
        if !step_ids.insert(step_id.clone()) {
            continue;
        }
        let mut attributes = BTreeMap::new();
        if state == &state_machine.initial {
            attributes.insert("initial".to_string(), "true".to_string());
        }
        nodes.push(GeometryNode {
            id: step_id.clone(),
            kind: GeometryNodeKind::Step,
            label: state.step_name.clone(),
            lane_id: format!("task:{}", state.task_name),
            views: vec![GeometryViewKind::Constellation, GeometryViewKind::Orbit],
            evidence_status: GeometryEvidenceStatus::Derived,
            attributes,
        });
        edges.push(GeometryEdge {
            id: format!("contains:{}:{}", state.task_name, state.step_name),
            kind: GeometryEdgeKind::Contains,
            from: format!("task:{}", state.task_name),
            to: step_id,
            label: "contains".to_string(),
            views: vec![GeometryViewKind::Constellation],
            evidence_status: GeometryEvidenceStatus::Derived,
            attributes: BTreeMap::new(),
        });
    }

    let mut devices = topology
        .graph
        .node_weights()
        .map(|device| (device.name.clone(), device.kind.clone()))
        .collect::<Vec<_>>();
    devices.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, kind) in devices {
        let mut attributes = BTreeMap::new();
        attributes.insert(
            "device_kind".to_string(),
            device_kind_name(&kind).to_string(),
        );
        nodes.push(GeometryNode {
            id: format!("device:{name}"),
            kind: GeometryNodeKind::Device,
            label: name,
            lane_id: "topology".to_string(),
            views: vec![GeometryViewKind::Constellation],
            evidence_status: GeometryEvidenceStatus::Authored,
            attributes,
        });
    }

    let mut topology_link_id = 0usize;
    if !topology.links.is_empty() {
        let mut links = topology.links.clone();
        links.sort_by(|a, b| {
            (&a.from, &a.to, &a.from_port, &a.to_port).cmp(&(
                &b.from,
                &b.to,
                &b.from_port,
                &b.to_port,
            ))
        });
        for link in links {
            topology_link_id += 1;
            let mut attributes = BTreeMap::new();
            attributes.insert(
                "link_kind".to_string(),
                connection_kind_name(&link.kind).to_string(),
            );
            if let Some(from_port) = &link.from_port {
                attributes.insert("from_port".to_string(), from_port.clone());
            }
            if let Some(to_port) = &link.to_port {
                attributes.insert("to_port".to_string(), to_port.clone());
            }
            edges.push(GeometryEdge {
                id: format!("topology-link:{topology_link_id}"),
                kind: GeometryEdgeKind::TopologyLink,
                from: format!("device:{}", link.from),
                to: format!("device:{}", link.to),
                label: connection_kind_name(&link.kind).to_string(),
                views: vec![GeometryViewKind::Constellation],
                evidence_status: GeometryEvidenceStatus::Authored,
                attributes,
            });
        }
    } else {
        let mut refs = topology.graph.edge_references().collect::<Vec<_>>();
        refs.sort_by(|a, b| {
            let a_from = &topology.graph[a.source()].name;
            let a_to = &topology.graph[a.target()].name;
            let b_from = &topology.graph[b.source()].name;
            let b_to = &topology.graph[b.target()].name;
            (a_from, a_to).cmp(&(b_from, b_to))
        });
        for edge in refs {
            topology_link_id += 1;
            let from_name = topology.graph[edge.source()].name.clone();
            let to_name = topology.graph[edge.target()].name.clone();
            edges.push(GeometryEdge {
                id: format!("topology-link:{topology_link_id}"),
                kind: GeometryEdgeKind::TopologyLink,
                from: format!("device:{from_name}"),
                to: format!("device:{to_name}"),
                label: connection_kind_name(edge.weight()).to_string(),
                views: vec![GeometryViewKind::Constellation],
                evidence_status: GeometryEvidenceStatus::Authored,
                attributes: BTreeMap::new(),
            });
        }
    }

    for site in &constraints.workpiece_sites {
        let mut attributes = BTreeMap::new();
        attributes.insert("capacity".to_string(), site.capacity.to_string());
        attributes.insert(
            "site_kind".to_string(),
            workpiece_site_kind_name(&site.kind).to_string(),
        );
        nodes.push(GeometryNode {
            id: format!("workpiece-site:{}", site.name),
            kind: GeometryNodeKind::WorkpieceSite,
            label: site.name.clone(),
            lane_id: "topology".to_string(),
            views: vec![GeometryViewKind::Constellation],
            evidence_status: GeometryEvidenceStatus::Authored,
            attributes,
        });
    }

    for holder in &constraints.workpiece_holders {
        let mut attributes = BTreeMap::new();
        attributes.insert("capacity".to_string(), holder.capacity.to_string());
        nodes.push(GeometryNode {
            id: format!("workpiece-holder:{}", holder.name),
            kind: GeometryNodeKind::WorkpieceHolder,
            label: holder.name.clone(),
            lane_id: "topology".to_string(),
            views: vec![GeometryViewKind::Constellation],
            evidence_status: GeometryEvidenceStatus::Authored,
            attributes,
        });
    }

    for carrier in &constraints.workpiece_carriers {
        let mut attributes = BTreeMap::new();
        attributes.insert(
            "layout".to_string(),
            match &carrier.layout {
                crate::ir::WorkpieceCarrierLayoutDef::Slots { count } => format!("slots:{count}"),
                crate::ir::WorkpieceCarrierLayoutDef::Grid { rows, cols } => {
                    format!("grid:{rows}x{cols}")
                }
            },
        );
        nodes.push(GeometryNode {
            id: format!("workpiece-carrier:{}", carrier.name),
            kind: GeometryNodeKind::WorkpieceCarrier,
            label: carrier.name.clone(),
            lane_id: "topology".to_string(),
            views: vec![GeometryViewKind::Constellation],
            evidence_status: GeometryEvidenceStatus::Authored,
            attributes,
        });
    }

    for resource in &constraints.semantic_resources {
        let mut attributes = BTreeMap::new();
        attributes.insert(
            "resource_mode".to_string(),
            semantic_resource_mode_name(&resource.mode).to_string(),
        );
        if let Some(purpose) = &resource.purpose {
            attributes.insert("purpose".to_string(), purpose.clone());
        }
        nodes.push(GeometryNode {
            id: format!("resource:{}", resource.name),
            kind: GeometryNodeKind::SemanticResource,
            label: resource.name.clone(),
            lane_id: "evidence".to_string(),
            views: vec![GeometryViewKind::Constellation, GeometryViewKind::Evidence],
            evidence_status: GeometryEvidenceStatus::Verified,
            attributes,
        });
    }

    let mut external_nodes = BTreeSet::new();
    for (idx, claim) in constraints.resource_claims.iter().enumerate() {
        let (from, label) = match &claim.source {
            ResourceClaimSource::State(expr) => {
                let source_id = format!(
                    "claim-source:state:{}:{}:{}",
                    expr.device, expr.port, expr.state
                );
                if external_nodes.insert(source_id.clone()) {
                    let mut attributes = BTreeMap::new();
                    attributes.insert("source_kind".to_string(), "state".to_string());
                    nodes.push(GeometryNode {
                        id: source_id.clone(),
                        kind: GeometryNodeKind::ClaimSource,
                        label: format!("{}.{} == {}", expr.device, expr.port, expr.state),
                        lane_id: "evidence".to_string(),
                        views: vec![GeometryViewKind::Evidence],
                        evidence_status: GeometryEvidenceStatus::Verified,
                        attributes,
                    });
                }
                (source_id, "claims".to_string())
            }
            ResourceClaimSource::ActionTag { tag } => {
                let source_id = format!("claim-source:action-tag:{tag}");
                if external_nodes.insert(source_id.clone()) {
                    let mut attributes = BTreeMap::new();
                    attributes.insert("source_kind".to_string(), "action_tag".to_string());
                    nodes.push(GeometryNode {
                        id: source_id.clone(),
                        kind: GeometryNodeKind::ClaimSource,
                        label: format!("action tag {tag}"),
                        lane_id: "evidence".to_string(),
                        views: vec![GeometryViewKind::Evidence],
                        evidence_status: GeometryEvidenceStatus::Verified,
                        attributes,
                    });
                }
                (source_id, "claims".to_string())
            }
        };

        let mut attributes = BTreeMap::new();
        if let Some(reason) = &claim.reason {
            attributes.insert("reason".to_string(), reason.clone());
        }
        edges.push(GeometryEdge {
            id: format!("resource-claim:{idx}"),
            kind: GeometryEdgeKind::ResourceClaim,
            from,
            to: format!("resource:{}", claim.resource),
            label,
            views: vec![GeometryViewKind::Evidence],
            evidence_status: GeometryEvidenceStatus::Verified,
            attributes,
        });
    }

    for (idx, rule) in constraints.timing.iter().enumerate() {
        let mut attributes = BTreeMap::new();
        attributes.insert(
            "relation".to_string(),
            timing_relation_name(&rule.relation).to_string(),
        );
        attributes.insert("duration_ms".to_string(), rule.duration_ms.to_string());
        if let Some(reason) = &rule.reason {
            attributes.insert("reason".to_string(), reason.clone());
        }
        let node_id = format!("timing-rule:{idx}");
        nodes.push(GeometryNode {
            id: node_id.clone(),
            kind: GeometryNodeKind::TimingRule,
            label: format!(
                "{} {}ms",
                timing_relation_name(&rule.relation),
                rule.duration_ms
            ),
            lane_id: "evidence".to_string(),
            views: vec![GeometryViewKind::Evidence],
            evidence_status: GeometryEvidenceStatus::Verified,
            attributes: attributes.clone(),
        });

        let scope_id = match &rule.scope {
            TimingScope::Task { task } => format!("task:{task}"),
            TimingScope::Step { task, step } => format!("step:{task}.{step}"),
        };
        edges.push(GeometryEdge {
            id: format!("timing-scope:{idx}"),
            kind: GeometryEdgeKind::TimingScope,
            from: node_id,
            to: scope_id,
            label: "scopes".to_string(),
            views: vec![GeometryViewKind::Evidence],
            evidence_status: GeometryEvidenceStatus::Verified,
            attributes,
        });
    }

    for (idx, chain) in constraints.causality.iter().enumerate() {
        let mut attributes = BTreeMap::new();
        if let Some(reason) = &chain.reason {
            attributes.insert("reason".to_string(), reason.clone());
        }
        let chain_id = format!("causality:{idx}");
        nodes.push(GeometryNode {
            id: chain_id.clone(),
            kind: GeometryNodeKind::CausalityChain,
            label: format!("causality {}", idx + 1),
            lane_id: "evidence".to_string(),
            views: vec![GeometryViewKind::Evidence],
            evidence_status: GeometryEvidenceStatus::Verified,
            attributes,
        });
        for device in &chain.devices {
            let target_id = ensure_external_device_node(&mut nodes, &mut external_nodes, device);
            edges.push(GeometryEdge {
                id: format!("causality:{idx}:{device}"),
                kind: GeometryEdgeKind::Causality,
                from: chain_id.clone(),
                to: target_id,
                label: "touches".to_string(),
                views: vec![GeometryViewKind::Evidence],
                evidence_status: GeometryEvidenceStatus::Verified,
                attributes: BTreeMap::new(),
            });
        }
    }

    for (idx, transition) in state_machine.transitions.iter().enumerate() {
        let mut attributes = BTreeMap::new();
        let action_labels = transition
            .actions
            .iter()
            .map(transition_action_label)
            .collect::<Vec<_>>();
        if !action_labels.is_empty() {
            attributes.insert("actions".to_string(), action_labels.join(" | "));
        }
        if !transition.effects.is_empty() {
            let effects = transition
                .effects
                .iter()
                .map(workpiece_effect_label)
                .collect::<Vec<_>>()
                .join(" | ");
            attributes.insert("effects".to_string(), effects);
        }
        if !transition.timers.is_empty() {
            let timers = transition
                .timers
                .iter()
                .map(|timer| {
                    let duration = timer
                        .duration_ms
                        .map(|value| format!("{value}ms"))
                        .unwrap_or_else(|| "open".to_string());
                    format!("{}:{duration}", timer.timer_name)
                })
                .collect::<Vec<_>>()
                .join(" | ");
            attributes.insert("timers".to_string(), timers);
        }

        edges.push(GeometryEdge {
            id: format!("transition:{idx}"),
            kind: GeometryEdgeKind::Transition,
            from: format!("step:{}", state_key(&transition.from)),
            to: format!("step:{}", state_key(&transition.to)),
            label: transition_guard_label(&transition.guard),
            views: vec![GeometryViewKind::Orbit, GeometryViewKind::Evidence],
            evidence_status: GeometryEvidenceStatus::Derived,
            attributes,
        });
    }

    let trace_overlay = trace_events.map(|events| GeometryTraceOverlay {
        observed_transition_count: events.len(),
        resolution: "best_effort_task_context_order".to_string(),
        transitions: map_trace_overlay(events, state_machine),
    });
    let intent_overlay = intent_report.map(|report| GeometryIntentOverlay {
        verdict: report.verdict,
        primary_mismatch_kind: report
            .primary_mismatch
            .as_ref()
            .map(|mismatch| mismatch.kind),
        blocker_kind: report.blocker_kind,
        mismatch_count: report.mismatches.len(),
        warnings: report.warnings.clone(),
        mismatches: report.mismatches.clone(),
    });
    let narrative = Some(build_geometry_narrative(
        topology,
        constraints,
        state_machine,
        trace_events,
        intent_report.is_some(),
    ));

    GeometryArtifact {
        schema_version: GEOMETRY_VIEW_SCHEMA_VERSION,
        artifact_kind: GEOMETRY_VIEW_ARTIFACT_KIND.to_string(),
        source_path: source_path.to_string(),
        summary: GeometrySummary {
            task_count: task_names.len(),
            step_count: state_machine.states.len(),
            transition_count: state_machine.transitions.len(),
            device_count: topology.graph.node_count(),
            resource_count: constraints.semantic_resources.len(),
            timing_rule_count: constraints.timing.len(),
            causality_chain_count: constraints.causality.len(),
            observed_transition_count: trace_events.map(|events| events.len()).unwrap_or(0),
            intent_mismatch_count: intent_report
                .map(|report| report.mismatches.len())
                .unwrap_or(0),
        },
        lanes,
        nodes,
        edges,
        overlays: GeometryOverlays {
            trace: trace_overlay,
            intent: intent_overlay,
        },
        narrative,
    }
}

#[derive(Debug, Clone)]
struct GeometryDeviceCatalog {
    by_name: HashMap<String, DeviceKind>,
    incoming: HashMap<String, Vec<String>>,
    outgoing: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone)]
struct GeometryCausalityEntry {
    id: String,
    reason: Option<String>,
    devices: Vec<String>,
}

fn build_geometry_narrative(
    topology: &TopologyGraph,
    constraints: &ConstraintSet,
    state_machine: &StateMachine,
    trace_events: Option<&[NormalizedTraceEvent]>,
    intent_available: bool,
) -> GeometryNarrative {
    let device_catalog = build_geometry_device_catalog(topology);
    let causality_catalog = build_geometry_causality_catalog(constraints);
    let observed_transition_ids = build_observed_transition_ids(trace_events, state_machine);
    let task_contexts = state_machine
        .task_contexts
        .iter()
        .map(|context| (context.task_name.clone(), context))
        .collect::<HashMap<_, _>>();
    let mut transitions_by_from = HashMap::<String, Vec<(usize, &crate::ir::Transition)>>::new();
    let mut transitions_by_to = HashMap::<String, Vec<(usize, &crate::ir::Transition)>>::new();
    for (idx, transition) in state_machine.transitions.iter().enumerate() {
        transitions_by_from
            .entry(format!("step:{}", state_key(&transition.from)))
            .or_default()
            .push((idx, transition));
        transitions_by_to
            .entry(format!("step:{}", state_key(&transition.to)))
            .or_default()
            .push((idx, transition));
    }

    let task_names = state_machine
        .states
        .iter()
        .map(|state| state.task_name.clone())
        .chain(
            state_machine
                .task_contexts
                .iter()
                .map(|context| context.task_name.clone()),
        )
        .collect::<BTreeSet<_>>();

    let mut tasks = Vec::new();
    for task_name in task_names {
        let step_states = state_machine
            .states
            .iter()
            .filter(|state| state.task_name == task_name)
            .cloned()
            .collect::<Vec<_>>();
        if step_states.is_empty() {
            continue;
        }

        let context = task_contexts.get(&task_name).copied();
        let entry_state = context
            .map(|ctx| ctx.entry_state.clone())
            .unwrap_or_else(|| step_states[0].clone());
        let current_state = context
            .map(|ctx| ctx.current_state.clone())
            .unwrap_or_else(|| entry_state.clone());
        let blocking_state = context
            .map(|ctx| blocking_state_name(&ctx.blocking_state).to_string())
            .unwrap_or_else(|| "ready".to_string());
        let pending_actions = context
            .map(|ctx| {
                ctx.pending_actions
                    .iter()
                    .map(|action| {
                        let mut out = action_kind_name(&action.action_kind).to_string();
                        if let Some(target) = &action.target {
                            out.push(':');
                            out.push_str(target);
                        }
                        out
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let ordered_states = order_task_states(&entry_state, &step_states, &transitions_by_from);
        let mut steps = Vec::new();
        for (index, state) in ordered_states.iter().enumerate() {
            let step_id = format!("step:{}", state_key(state));
            let outgoing_raw = transitions_by_from
                .get(&step_id)
                .cloned()
                .unwrap_or_default();
            let incoming_raw = transitions_by_to.get(&step_id).cloned().unwrap_or_default();
            let outgoing = outgoing_raw
                .iter()
                .map(|(transition_idx, transition)| {
                    build_narrative_transition(
                        *transition_idx,
                        transition,
                        &observed_transition_ids,
                    )
                })
                .collect::<Vec<_>>();
            let device_chains =
                build_step_device_chains(&outgoing_raw, &device_catalog, &causality_catalog);
            let evidence_chain_ids = device_chains
                .iter()
                .flat_map(|chain| chain.evidence_chain_ids.iter().cloned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let evidence_reasons = evidence_chain_ids
                .iter()
                .filter_map(|chain_id| {
                    causality_catalog
                        .iter()
                        .find(|entry| &entry.id == chain_id)
                        .and_then(|entry| entry.reason.clone())
                })
                .collect::<Vec<_>>();
            steps.push(GeometryNarrativeStep {
                step_id: step_id.clone(),
                label: state.step_name.clone(),
                index,
                is_initial: state == &entry_state,
                is_current: state == &current_state,
                incoming_transition_ids: incoming_raw
                    .iter()
                    .map(|(transition_idx, _)| format!("transition:{transition_idx}"))
                    .collect(),
                outgoing,
                device_chains,
                evidence_chain_ids,
                evidence_reasons,
            });
        }

        let main_path_step_ids = steps
            .iter()
            .map(|step| step.step_id.clone())
            .collect::<Vec<_>>();
        let (blocking_points, fault_exits) = build_task_headline(&steps);
        let uncovered_step_count = steps
            .iter()
            .filter(|step| step.device_chains.is_empty())
            .count();

        tasks.push(GeometryNarrativeTask {
            task_id: format!("task:{task_name}"),
            label: task_name,
            entry_step_id: format!("step:{}", state_key(&entry_state)),
            current_step_id: format!("step:{}", state_key(&current_state)),
            blocking_state,
            pending_actions,
            main_path_step_ids,
            blocking_points,
            fault_exits,
            coverage: GeometryNarrativeCoverage {
                uncovered_step_count,
                trace_available: trace_events.is_some(),
                intent_available,
            },
            steps,
        });
    }

    GeometryNarrative { tasks }
}

fn build_geometry_device_catalog(topology: &TopologyGraph) -> GeometryDeviceCatalog {
    let by_name = topology
        .graph
        .node_weights()
        .map(|device| (device.name.clone(), device.kind.clone()))
        .collect::<HashMap<_, _>>();
    let mut incoming = HashMap::<String, Vec<String>>::new();
    let mut outgoing = HashMap::<String, Vec<String>>::new();
    let links = if !topology.links.is_empty() {
        topology
            .links
            .iter()
            .map(|link| (link.from.clone(), link.to.clone()))
            .collect::<Vec<_>>()
    } else {
        topology
            .graph
            .edge_references()
            .map(|edge| {
                (
                    topology.graph[edge.source()].name.clone(),
                    topology.graph[edge.target()].name.clone(),
                )
            })
            .collect::<Vec<_>>()
    };

    for (from, to) in links {
        outgoing.entry(from.clone()).or_default().push(to.clone());
        incoming.entry(to).or_default().push(from);
    }

    GeometryDeviceCatalog {
        by_name,
        incoming,
        outgoing,
    }
}

fn build_geometry_causality_catalog(constraints: &ConstraintSet) -> Vec<GeometryCausalityEntry> {
    constraints
        .causality
        .iter()
        .enumerate()
        .map(|(idx, chain)| GeometryCausalityEntry {
            id: format!("causality:{idx}"),
            reason: chain.reason.clone(),
            devices: chain.devices.clone(),
        })
        .collect()
}

fn build_observed_transition_ids(
    trace_events: Option<&[NormalizedTraceEvent]>,
    state_machine: &StateMachine,
) -> BTreeSet<String> {
    let Some(events) = trace_events else {
        return BTreeSet::new();
    };

    map_trace_overlay(events, state_machine)
        .into_iter()
        .filter_map(|transition| {
            let from_state = transition.from_state?;
            let to_state = transition.to_state?;
            Some(format!("step:{from_state}->step:{to_state}"))
        })
        .collect()
}

fn order_task_states(
    entry_state: &State,
    step_states: &[State],
    transitions_by_from: &HashMap<String, Vec<(usize, &crate::ir::Transition)>>,
) -> Vec<State> {
    let mut ordered = Vec::new();
    let mut seen = BTreeSet::new();
    let states_by_id = step_states
        .iter()
        .map(|state| (format!("step:{}", state_key(state)), state.clone()))
        .collect::<HashMap<_, _>>();

    fn visit(
        step_id: &str,
        states_by_id: &HashMap<String, State>,
        transitions_by_from: &HashMap<String, Vec<(usize, &crate::ir::Transition)>>,
        seen: &mut BTreeSet<String>,
        ordered: &mut Vec<State>,
    ) {
        if !seen.insert(step_id.to_string()) {
            return;
        }
        let Some(state) = states_by_id.get(step_id) else {
            return;
        };
        ordered.push(state.clone());
        if let Some(transitions) = transitions_by_from.get(step_id) {
            for (_, transition) in transitions {
                visit(
                    &format!("step:{}", state_key(&transition.to)),
                    states_by_id,
                    transitions_by_from,
                    seen,
                    ordered,
                );
            }
        }
    }

    visit(
        &format!("step:{}", state_key(entry_state)),
        &states_by_id,
        transitions_by_from,
        &mut seen,
        &mut ordered,
    );
    for state in step_states {
        visit(
            &format!("step:{}", state_key(state)),
            &states_by_id,
            transitions_by_from,
            &mut seen,
            &mut ordered,
        );
    }
    ordered
}

fn build_narrative_transition(
    transition_idx: usize,
    transition: &crate::ir::Transition,
    observed_transition_ids: &BTreeSet<String>,
) -> GeometryNarrativeTransition {
    let timers = transition
        .timers
        .iter()
        .map(|timer| {
            let duration = timer
                .duration_ms
                .map(|value| format!("{value}ms"))
                .unwrap_or_else(|| "open".to_string());
            format!("{}:{duration}", timer.timer_name)
        })
        .collect::<Vec<_>>();
    let actions = transition
        .actions
        .iter()
        .map(build_narrative_action)
        .collect::<Vec<_>>();
    let effects = transition
        .effects
        .iter()
        .map(workpiece_effect_label)
        .collect::<Vec<_>>();
    let transition_key = format!(
        "step:{}->step:{}",
        state_key(&transition.from),
        state_key(&transition.to)
    );

    GeometryNarrativeTransition {
        transition_id: format!("transition:{transition_idx}"),
        to_step_id: format!("step:{}", state_key(&transition.to)),
        to_step_label: transition.to.step_name.clone(),
        guard_kind: transition_guard_kind(&transition.guard).to_string(),
        guard_label: transition_guard_label(&transition.guard),
        timers,
        actions,
        effects,
        observed: observed_transition_ids.contains(&transition_key),
    }
}

fn build_narrative_action(action: &TransitionAction) -> GeometryNarrativeAction {
    GeometryNarrativeAction {
        kind: action_kind_name(&action_kind_from_transition(action)).to_string(),
        label: transition_action_label(action),
        target_device_id: action_target_name(action).map(|target| format!("device:{target}")),
        target_port: action_target_port(action).map(str::to_string),
    }
}

fn build_step_device_chains(
    outgoing_raw: &[(usize, &crate::ir::Transition)],
    device_catalog: &GeometryDeviceCatalog,
    causality_catalog: &[GeometryCausalityEntry],
) -> Vec<GeometryNarrativeDeviceChain> {
    let mut chains = Vec::new();
    let mut seen = BTreeSet::new();
    let known_devices = device_catalog
        .by_name
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();

    for (_, transition) in outgoing_raw {
        for action in &transition.actions {
            if let Some(target) = action_target_name(action) {
                let key = format!("action:{target}");
                if seen.insert(key) {
                    if let Some(chain) = build_device_chain_for_focus(
                        target,
                        "action_target",
                        transition_action_label(action),
                        device_catalog,
                        causality_catalog,
                    ) {
                        chains.push(chain);
                    }
                }
            }
        }

        for device_name in extract_guard_device_names(&transition.guard, &known_devices) {
            let key = format!("guard:{device_name}");
            if seen.insert(key) {
                if let Some(chain) = build_device_chain_for_focus(
                    &device_name,
                    "guard_dependency",
                    transition_guard_label(&transition.guard),
                    device_catalog,
                    causality_catalog,
                ) {
                    chains.push(chain);
                }
            }
        }
    }

    chains
}

fn build_device_chain_for_focus(
    focal: &str,
    source_kind: &str,
    explanation: String,
    device_catalog: &GeometryDeviceCatalog,
    causality_catalog: &[GeometryCausalityEntry],
) -> Option<GeometryNarrativeDeviceChain> {
    if !device_catalog.by_name.contains_key(focal) {
        return None;
    }

    let upstream = traverse_device_neighbors(focal, &device_catalog.incoming);
    let downstream = traverse_device_neighbors(focal, &device_catalog.outgoing);
    let mut command_devices = Vec::new();
    let mut actuator_devices = Vec::new();
    let mut feedback_devices = Vec::new();
    let mut io_devices = Vec::new();

    push_device_ref_by_bucket(
        focal,
        device_catalog,
        &mut command_devices,
        &mut actuator_devices,
        &mut feedback_devices,
        &mut io_devices,
    );
    for device in upstream.iter().rev() {
        push_device_ref_by_bucket(
            device,
            device_catalog,
            &mut command_devices,
            &mut actuator_devices,
            &mut feedback_devices,
            &mut io_devices,
        );
    }
    for device in &downstream {
        push_device_ref_by_bucket(
            device,
            device_catalog,
            &mut command_devices,
            &mut actuator_devices,
            &mut feedback_devices,
            &mut io_devices,
        );
    }

    let chain_device_names = command_devices
        .iter()
        .chain(actuator_devices.iter())
        .chain(feedback_devices.iter())
        .chain(io_devices.iter())
        .map(|device| device.label.clone())
        .collect::<BTreeSet<_>>();
    let evidence_chain_ids = causality_catalog
        .iter()
        .filter(|entry| {
            entry
                .devices
                .iter()
                .any(|device| chain_device_names.contains(device))
        })
        .map(|entry| entry.id.clone())
        .collect::<Vec<_>>();

    Some(GeometryNarrativeDeviceChain {
        source_kind: source_kind.to_string(),
        explanation,
        command_devices,
        actuator_devices,
        feedback_devices,
        io_devices,
        evidence_chain_ids,
    })
}

fn push_device_ref_by_bucket(
    device_name: &str,
    device_catalog: &GeometryDeviceCatalog,
    command_devices: &mut Vec<GeometryNarrativeDeviceRef>,
    actuator_devices: &mut Vec<GeometryNarrativeDeviceRef>,
    feedback_devices: &mut Vec<GeometryNarrativeDeviceRef>,
    io_devices: &mut Vec<GeometryNarrativeDeviceRef>,
) {
    let Some(kind) = device_catalog.by_name.get(device_name) else {
        return;
    };
    let device_ref = GeometryNarrativeDeviceRef {
        device_id: format!("device:{device_name}"),
        label: device_name.to_string(),
        kind: device_kind_name(kind).to_string(),
    };
    match device_bucket(kind) {
        "command" => push_unique_device_ref(command_devices, device_ref),
        "actuator" => push_unique_device_ref(actuator_devices, device_ref),
        "feedback" => push_unique_device_ref(feedback_devices, device_ref),
        _ => push_unique_device_ref(io_devices, device_ref),
    }
}

fn push_unique_device_ref(
    target: &mut Vec<GeometryNarrativeDeviceRef>,
    device_ref: GeometryNarrativeDeviceRef,
) {
    if target
        .iter()
        .any(|existing| existing.device_id == device_ref.device_id)
    {
        return;
    }
    target.push(device_ref);
}

fn traverse_device_neighbors(start: &str, adjacency: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut out = Vec::new();
    let mut queue = adjacency.get(start).cloned().unwrap_or_default();
    let mut seen = BTreeSet::new();
    while let Some(device) = queue.first().cloned() {
        queue.remove(0);
        if !seen.insert(device.clone()) {
            continue;
        }
        out.push(device.clone());
        if let Some(next) = adjacency.get(&device) {
            queue.extend(next.iter().cloned());
        }
    }
    out
}

fn extract_guard_device_names(
    guard: &TransitionGuard,
    known_devices: &BTreeSet<String>,
) -> Vec<String> {
    let TransitionGuard::Condition { expression } = guard else {
        return Vec::new();
    };
    expression
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .filter(|token| !token.is_empty())
        .filter(|token| known_devices.contains(*token))
        .map(str::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn build_task_headline(
    steps: &[GeometryNarrativeStep],
) -> (
    Vec<GeometryNarrativeBlockingPoint>,
    Vec<GeometryNarrativeExit>,
) {
    let mut blocking_points = Vec::new();
    let mut fault_exits = Vec::new();

    for step in steps {
        let timeout_transitions = step
            .outgoing
            .iter()
            .filter(|transition| is_timeout_transition(transition))
            .map(narrative_transition_ref)
            .collect::<Vec<_>>();
        let release_transitions = step
            .outgoing
            .iter()
            .filter(|transition| !is_timeout_transition(transition))
            .map(narrative_transition_ref)
            .collect::<Vec<_>>();

        if !timeout_transitions.is_empty()
            || release_transitions
                .iter()
                .any(|transition| transition.guard_kind != "always")
        {
            blocking_points.push(GeometryNarrativeBlockingPoint {
                step_id: step.step_id.clone(),
                step_label: step.label.clone(),
                release_transitions: release_transitions.clone(),
                timeout_transitions: timeout_transitions.clone(),
            });
        }

        let preferred_transition_id = release_transitions
            .first()
            .map(|transition| transition.transition_id.clone())
            .or_else(|| {
                step.outgoing
                    .first()
                    .map(|transition| transition.transition_id.clone())
            });
        for transition in step.outgoing.iter().map(narrative_transition_ref) {
            if preferred_transition_id
                .as_ref()
                .is_some_and(|preferred| preferred == &transition.transition_id)
            {
                continue;
            }
            fault_exits.push(GeometryNarrativeExit {
                from_step_id: step.step_id.clone(),
                from_step_label: step.label.clone(),
                via: transition,
            });
        }
    }

    (blocking_points, fault_exits)
}

fn narrative_transition_ref(
    transition: &GeometryNarrativeTransition,
) -> GeometryNarrativeTransitionRef {
    GeometryNarrativeTransitionRef {
        transition_id: transition.transition_id.clone(),
        guard_kind: transition.guard_kind.clone(),
        guard_label: transition.guard_label.clone(),
        to_step_id: transition.to_step_id.clone(),
        to_step_label: transition.to_step_label.clone(),
    }
}

fn is_timeout_transition(transition: &GeometryNarrativeTransition) -> bool {
    transition.guard_kind == "timeout"
        || transition.guard_kind == "delay"
        || !transition.timers.is_empty()
}

fn ensure_external_device_node(
    nodes: &mut Vec<GeometryNode>,
    external_nodes: &mut BTreeSet<String>,
    device: &str,
) -> String {
    let direct_id = format!("device:{device}");
    if nodes.iter().any(|node| node.id == direct_id) {
        return direct_id;
    }

    let fallback_id = format!("external:{device}");
    if external_nodes.insert(fallback_id.clone()) {
        let mut attributes = BTreeMap::new();
        attributes.insert("reference_kind".to_string(), "device".to_string());
        nodes.push(GeometryNode {
            id: fallback_id.clone(),
            kind: GeometryNodeKind::ExternalReference,
            label: device.to_string(),
            lane_id: "evidence".to_string(),
            views: vec![GeometryViewKind::Evidence],
            evidence_status: GeometryEvidenceStatus::Derived,
            attributes,
        });
    }
    fallback_id
}

fn map_trace_overlay(
    events: &[NormalizedTraceEvent],
    state_machine: &StateMachine,
) -> Vec<GeometryObservedTransition> {
    let task_layouts = build_best_effort_task_layouts(state_machine);
    events
        .iter()
        .map(|event| {
            let task_name = task_layouts
                .get(event.task)
                .map(|layout| layout.task_name.clone());
            let from_state = task_layouts
                .get(event.task)
                .and_then(|layout| layout.step_keys.get(event.from_step as usize))
                .cloned();
            let to_state = task_layouts
                .get(event.task)
                .and_then(|layout| layout.step_keys.get(event.to_step as usize))
                .cloned();

            GeometryObservedTransition {
                tick: event.tick,
                task_index: event.task,
                from_step: event.from_step,
                to_step: event.to_step,
                reason: event.reason.clone(),
                task_name,
                from_state,
                to_state,
            }
        })
        .collect()
}

#[derive(Debug, Clone)]
struct BestEffortTaskLayout {
    task_name: String,
    step_keys: Vec<String>,
}

fn build_best_effort_task_layouts(state_machine: &StateMachine) -> Vec<BestEffortTaskLayout> {
    let mut layouts = Vec::new();
    let mut seen = BTreeSet::new();

    for context in &state_machine.task_contexts {
        if seen.insert(context.task_name.clone()) {
            let step_keys = state_machine
                .states
                .iter()
                .filter(|state| state.task_name == context.task_name)
                .map(state_key)
                .collect::<Vec<_>>();
            layouts.push(BestEffortTaskLayout {
                task_name: context.task_name.clone(),
                step_keys,
            });
        }
    }

    for state in &state_machine.states {
        if seen.insert(state.task_name.clone()) {
            let step_keys = state_machine
                .states
                .iter()
                .filter(|candidate| candidate.task_name == state.task_name)
                .map(state_key)
                .collect::<Vec<_>>();
            layouts.push(BestEffortTaskLayout {
                task_name: state.task_name.clone(),
                step_keys,
            });
        }
    }

    layouts
}

fn transition_guard_label(guard: &TransitionGuard) -> String {
    match guard {
        TransitionGuard::Always => "always".to_string(),
        TransitionGuard::Condition { expression } => format!("when {expression}"),
        TransitionGuard::Edge { edge, operand } => {
            format!("{}({operand})", transition_edge_name(*edge))
        }
        TransitionGuard::Timeout { duration_ms } => format!("timeout {duration_ms}ms"),
        TransitionGuard::Delay { duration_ms } => format!("delay {duration_ms}ms"),
    }
}

fn transition_guard_kind(guard: &TransitionGuard) -> &'static str {
    match guard {
        TransitionGuard::Always => "always",
        TransitionGuard::Condition { .. } => "condition",
        TransitionGuard::Edge { .. } => "edge",
        TransitionGuard::Timeout { .. } => "timeout",
        TransitionGuard::Delay { .. } => "delay",
    }
}

fn transition_edge_name(edge: crate::ir::EdgeKind) -> &'static str {
    match edge {
        crate::ir::EdgeKind::Rising => "rising_edge",
        crate::ir::EdgeKind::Falling => "falling_edge",
    }
}

fn transition_action_label(action: &TransitionAction) -> String {
    match action {
        TransitionAction::Extend { target, .. } => format!("extend {target}"),
        TransitionAction::Retract { target, .. } => format!("retract {target}"),
        TransitionAction::Set { target, value, .. } => {
            format!("set {target} {}", binary_value_name(value))
        }
        TransitionAction::SetAnalog {
            target, value_raw, ..
        } => {
            format!("set_analog {target} {value_raw}")
        }
        TransitionAction::SetAnalogExpr {
            target, expr_raw, ..
        } => {
            format!("set_analog_expr {target} {expr_raw}")
        }
        TransitionAction::Compute { target, expr_raw } => format!("compute {target} = {expr_raw}"),
        TransitionAction::CallExtern { function, .. } => format!("extern {function}"),
        TransitionAction::CamEngage { target } => format!("cam_engage {target}"),
        TransitionAction::CamDisengage { target } => format!("cam_disengage {target}"),
        TransitionAction::CamSwitch { target, new_table } => {
            format!("cam_switch {target} -> {new_table}")
        }
        TransitionAction::CamPhase {
            target,
            offset_expr_raw,
        } => {
            format!("cam_phase {target} {offset_expr_raw}")
        }
        TransitionAction::DeviceAction {
            family,
            action_name,
            target,
            ..
        } => format!("device_action {family}.{action_name} {target}"),
        TransitionAction::AxisMoveRelative {
            target,
            distance_raw,
            ..
        } => {
            format!("axis_move_relative {target} {distance_raw}")
        }
        TransitionAction::AxisMoveAbsolute {
            target,
            position_raw,
            ..
        } => {
            format!("axis_move_absolute {target} {position_raw}")
        }
        TransitionAction::Log { message } => format!("log {message}"),
    }
}

fn action_kind_from_transition(action: &TransitionAction) -> ActionKind {
    match action {
        TransitionAction::Extend { .. } => ActionKind::Extend,
        TransitionAction::Retract { .. } => ActionKind::Retract,
        TransitionAction::Set { .. } => ActionKind::Set,
        TransitionAction::SetAnalog { .. } => ActionKind::SetAnalog,
        TransitionAction::SetAnalogExpr { .. } => ActionKind::SetAnalogExpr,
        TransitionAction::Compute { .. } => ActionKind::Compute,
        TransitionAction::CallExtern { .. } => ActionKind::CallExtern,
        TransitionAction::CamEngage { .. } => ActionKind::CamEngage,
        TransitionAction::CamDisengage { .. } => ActionKind::CamDisengage,
        TransitionAction::CamSwitch { .. } => ActionKind::CamSwitch,
        TransitionAction::CamPhase { .. } => ActionKind::CamPhase,
        TransitionAction::DeviceAction { .. } => ActionKind::DeviceAction,
        TransitionAction::AxisMoveRelative { .. } => ActionKind::AxisMoveRelative,
        TransitionAction::AxisMoveAbsolute { .. } => ActionKind::AxisMoveAbsolute,
        TransitionAction::Log { .. } => ActionKind::Log,
    }
}

fn action_target_name(action: &TransitionAction) -> Option<&str> {
    match action {
        TransitionAction::Extend { target, .. }
        | TransitionAction::Retract { target, .. }
        | TransitionAction::Set { target, .. }
        | TransitionAction::SetAnalog { target, .. }
        | TransitionAction::SetAnalogExpr { target, .. }
        | TransitionAction::Compute { target, .. }
        | TransitionAction::CamEngage { target, .. }
        | TransitionAction::CamDisengage { target, .. }
        | TransitionAction::CamSwitch { target, .. }
        | TransitionAction::CamPhase { target, .. }
        | TransitionAction::DeviceAction { target, .. }
        | TransitionAction::AxisMoveRelative { target, .. }
        | TransitionAction::AxisMoveAbsolute { target, .. } => Some(target.as_str()),
        TransitionAction::CallExtern { .. } | TransitionAction::Log { .. } => None,
    }
}

fn action_target_port(action: &TransitionAction) -> Option<&str> {
    match action {
        TransitionAction::Extend { port, .. }
        | TransitionAction::Retract { port, .. }
        | TransitionAction::Set { port, .. }
        | TransitionAction::SetAnalog { port, .. }
        | TransitionAction::SetAnalogExpr { port, .. }
        | TransitionAction::AxisMoveRelative { port, .. }
        | TransitionAction::AxisMoveAbsolute { port, .. } => Some(port.as_str()),
        _ => None,
    }
}

fn workpiece_effect_label(effect: &crate::ir::WorkpieceEffect) -> String {
    match effect {
        crate::ir::WorkpieceEffect::Acquire { holder, from } => {
            format!("acquire {holder} <- {from}")
        }
        crate::ir::WorkpieceEffect::Transfer { from, to } => format!("transfer {from} -> {to}"),
        crate::ir::WorkpieceEffect::Finish { at, terminal_state } => {
            format!("finish {at} as {terminal_state}")
        }
        crate::ir::WorkpieceEffect::Mount {
            workpiece_type,
            slot,
        } => {
            format!("mount {workpiece_type} @ {slot}")
        }
        crate::ir::WorkpieceEffect::Unmount {
            workpiece_type,
            slot,
            to,
        } => {
            format!("unmount {workpiece_type} {slot} -> {to}")
        }
        crate::ir::WorkpieceEffect::Split {
            source_type,
            target_type,
            count,
            ..
        } => format!("split {source_type} -> {target_type} x{count}"),
        crate::ir::WorkpieceEffect::Merge {
            inputs,
            target_type,
            ..
        } => format!("merge {} -> {target_type}", inputs.join("+")),
        crate::ir::WorkpieceEffect::TransformCarrier { carrier, frame } => {
            format!("transform carrier {carrier} -> {frame}")
        }
    }
}

fn state_key(state: &State) -> String {
    format!("{}.{}", state.task_name, state.step_name)
}

fn action_kind_name(kind: &ActionKind) -> &'static str {
    match kind {
        ActionKind::Extend => "extend",
        ActionKind::Retract => "retract",
        ActionKind::Set => "set",
        ActionKind::SetAnalog => "set_analog",
        ActionKind::SetAnalogExpr => "set_analog_expr",
        ActionKind::Compute => "compute",
        ActionKind::CallExtern => "call_extern",
        ActionKind::CamEngage => "cam_engage",
        ActionKind::CamDisengage => "cam_disengage",
        ActionKind::CamSwitch => "cam_switch",
        ActionKind::CamPhase => "cam_phase",
        ActionKind::DeviceAction => "device_action",
        ActionKind::AxisMoveRelative => "axis_move_relative",
        ActionKind::AxisMoveAbsolute => "axis_move_absolute",
        ActionKind::Log => "log",
    }
}

fn device_kind_name(kind: &DeviceKind) -> &'static str {
    match kind {
        DeviceKind::DigitalOutput => "digital_output",
        DeviceKind::DigitalInput => "digital_input",
        DeviceKind::Plc => "plc",
        DeviceKind::SolenoidValve => "solenoid_valve",
        DeviceKind::Cylinder => "cylinder",
        DeviceKind::Sensor => "sensor",
        DeviceKind::Motor => "motor",
        DeviceKind::StepperMotor => "stepper_motor",
        DeviceKind::Vfd => "vfd",
        DeviceKind::ServoDrive => "servo_drive",
        DeviceKind::CamCoupling => "cam_coupling",
        DeviceKind::AnalogInput => "analog_input",
        DeviceKind::AnalogOutput => "analog_output",
        DeviceKind::Pid => "pid",
        DeviceKind::ProportionalValve => "proportional_valve",
        DeviceKind::Gripper => "gripper",
        DeviceKind::Conveyor => "conveyor",
        DeviceKind::Pump => "pump",
        DeviceKind::Heater => "heater",
        DeviceKind::VisionSensor => "vision_sensor",
    }
}

fn device_bucket(kind: &DeviceKind) -> &'static str {
    match kind {
        DeviceKind::DigitalOutput | DeviceKind::AnalogOutput | DeviceKind::Plc => "command",
        DeviceKind::SolenoidValve
        | DeviceKind::Cylinder
        | DeviceKind::Motor
        | DeviceKind::StepperMotor
        | DeviceKind::Vfd
        | DeviceKind::ServoDrive
        | DeviceKind::CamCoupling
        | DeviceKind::Pid
        | DeviceKind::ProportionalValve
        | DeviceKind::Gripper
        | DeviceKind::Conveyor
        | DeviceKind::Pump
        | DeviceKind::Heater
        | DeviceKind::VisionSensor => "actuator",
        DeviceKind::Sensor => "feedback",
        DeviceKind::DigitalInput | DeviceKind::AnalogInput => "io",
    }
}

fn connection_kind_name(kind: &crate::ir::ConnectionType) -> &'static str {
    match kind {
        crate::ir::ConnectionType::Electrical => "electrical",
        crate::ir::ConnectionType::Pneumatic => "pneumatic",
        crate::ir::ConnectionType::Logical => "logical",
        crate::ir::ConnectionType::Analog => "analog",
    }
}

fn semantic_resource_mode_name(mode: &SemanticResourceMode) -> &'static str {
    match mode {
        SemanticResourceMode::Exclusive => "exclusive",
    }
}

fn timing_relation_name(relation: &TimingRelation) -> &'static str {
    match relation {
        TimingRelation::MustCompleteWithin => "must_complete_within",
        TimingRelation::MustCompleteWithinWorstCase => "must_complete_within_worst_case",
        TimingRelation::MustStartAfter => "must_start_after",
    }
}

fn workpiece_site_kind_name(kind: &WorkpieceSiteKind) -> &'static str {
    match kind {
        WorkpieceSiteKind::WorkpieceLocation => "workpiece_location",
        WorkpieceSiteKind::CarrierLocation => "carrier_location",
    }
}

fn blocking_state_name(state: &crate::ir::TaskBlockingState) -> &'static str {
    match state {
        crate::ir::TaskBlockingState::Ready => "ready",
        crate::ir::TaskBlockingState::WaitingCondition => "waiting_condition",
        crate::ir::TaskBlockingState::WaitingDelay => "waiting_delay",
        crate::ir::TaskBlockingState::WaitingTimeout => "waiting_timeout",
        crate::ir::TaskBlockingState::WaitingPendingAction => "waiting_pending_action",
    }
}

fn binary_value_name(value: &crate::ir::BinaryValue) -> &'static str {
    match value {
        crate::ir::BinaryValue::On => "on",
        crate::ir::BinaryValue::Off => "off",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        BinaryValue, ConnectionType, Device, ResourceClaimRule, SafetyRule, StateExpr,
        TaskBlockingState, TaskExecutionContext, TimingRule, TopologyGraph, Transition,
        TransitionAction, TransitionGuard,
    };

    #[test]
    fn geometry_artifact_captures_semantics_and_optional_evidence() {
        let mut topology = TopologyGraph::new();
        let plc = topology.add_device(Device {
            name: "plc_main".to_string(),
            kind: DeviceKind::Plc,
        });
        let y0 = topology.add_device(Device {
            name: "Y0".to_string(),
            kind: DeviceKind::DigitalOutput,
        });
        topology.add_connection(plc, y0, ConnectionType::Electrical);

        let wait = State {
            task_name: "main".to_string(),
            step_name: "wait_start".to_string(),
        };
        let run = State {
            task_name: "main".to_string(),
            step_name: "run".to_string(),
        };
        let done = State {
            task_name: "done".to_string(),
            step_name: "halt".to_string(),
        };

        let state_machine = StateMachine {
            states: vec![wait.clone(), run.clone(), done.clone()],
            transitions: vec![
                Transition {
                    from: wait.clone(),
                    to: run.clone(),
                    guard: TransitionGuard::Condition {
                        expression: "X0 == true".to_string(),
                    },
                    actions: vec![],
                    effects: vec![],
                    timers: vec![],
                },
                Transition {
                    from: run.clone(),
                    to: done.clone(),
                    guard: TransitionGuard::Always,
                    actions: vec![TransitionAction::Set {
                        target: "Y0".to_string(),
                        port: "self".to_string(),
                        value: BinaryValue::On,
                    }],
                    effects: vec![],
                    timers: vec![],
                },
            ],
            initial: wait.clone(),
            analog_regions: BTreeMap::new(),
            task_contexts: vec![
                TaskExecutionContext {
                    task_name: "main".to_string(),
                    entry_state: wait.clone(),
                    current_state: run.clone(),
                    blocking_state: TaskBlockingState::WaitingCondition,
                    timers: vec![],
                    pending_actions: vec![],
                },
                TaskExecutionContext {
                    task_name: "done".to_string(),
                    entry_state: done.clone(),
                    current_state: done.clone(),
                    blocking_state: TaskBlockingState::Ready,
                    timers: vec![],
                    pending_actions: vec![],
                },
            ],
        };

        let constraints = ConstraintSet {
            safety: vec![SafetyRule {
                left: crate::ir::SafetyExpr::State(StateExpr {
                    device: "Y0".to_string(),
                    port: "self".to_string(),
                    state: "on".to_string(),
                }),
                relation: crate::ir::SafetyRelation::ConflictsWith,
                right: crate::ir::SafetyExpr::State(StateExpr {
                    device: "X0".to_string(),
                    port: "self".to_string(),
                    state: "fault".to_string(),
                }),
                reason: Some("fixture".to_string()),
                source: None,
            }],
            workpiece_types: vec![],
            workpiece_sites: vec![],
            workpiece_holders: vec![],
            workpiece_carriers: vec![],
            semantic_resources: vec![crate::ir::SemanticResource {
                name: "fixture_axis".to_string(),
                mode: SemanticResourceMode::Exclusive,
                purpose: Some("fixture".to_string()),
            }],
            resource_claims: vec![ResourceClaimRule {
                source: ResourceClaimSource::ActionTag {
                    tag: "axis.main".to_string(),
                },
                resource: "fixture_axis".to_string(),
                reason: Some("fixture".to_string()),
            }],
            timing: vec![TimingRule {
                scope: TimingScope::Task {
                    task: "main".to_string(),
                },
                relation: TimingRelation::MustCompleteWithin,
                duration_ms: 100,
                reason: Some("fixture".to_string()),
            }],
            causality: vec![crate::ir::CausalityChain {
                devices: vec!["plc_main".to_string(), "Y0".to_string()],
                reason: Some("fixture".to_string()),
            }],
        };

        let trace = vec![
            NormalizedTraceEvent {
                tick: 1,
                task: 0,
                from_step: 0,
                to_step: 1,
                reason: "wait_satisfied".to_string(),
            },
            NormalizedTraceEvent {
                tick: 2,
                task: 0,
                from_step: 1,
                to_step: 2,
                reason: "goto".to_string(),
            },
        ];
        let intent_report = IntentAlignmentReport {
            contract_identity: crate::intent_alignment::IntentAlignmentContractIdentity {
                contract_id: "fixture".to_string(),
                contract_version: "phase-2.v1".to_string(),
            },
            evidence_identity: crate::intent_alignment::IntentAlignmentEvidenceIdentity {
                kind: crate::intent_alignment::IntentAlignmentEvidenceKind::InlineTraceJsonl,
                label: "fixture".to_string(),
            },
            comparator_version: "phase-2.v1".to_string(),
            cycle_window: crate::intent_alignment::IntentAlignmentCycleWindow {
                first_cycle_index: 0,
                last_cycle_index: 0,
                cycle_count: 1,
            },
            verdict: IntentAlignmentVerdict::Aligned,
            primary_mismatch: None,
            mismatches: vec![],
            blocked_reason: None,
            blocker_kind: None,
            warnings: vec!["CrossCycle warning".to_string()],
        };

        let artifact = export_geometry_artifact(
            "examples/fixture.plc",
            &topology,
            &constraints,
            &state_machine,
            Some(&trace),
            Some(&intent_report),
        );

        assert_eq!(artifact.schema_version, GEOMETRY_VIEW_SCHEMA_VERSION);
        assert_eq!(artifact.summary.task_count, 2);
        assert_eq!(artifact.summary.step_count, 3);
        assert_eq!(artifact.summary.transition_count, 2);
        assert_eq!(artifact.summary.observed_transition_count, 2);
        assert!(
            artifact
                .nodes
                .iter()
                .any(|node| node.id == "task:main" && node.kind == GeometryNodeKind::Task)
        );
        assert!(artifact.nodes.iter().any(|node| {
            node.id == "step:main.wait_start" && node.kind == GeometryNodeKind::Step
        }));
        assert!(
            artifact.nodes.iter().any(|node| {
                node.id == "device:plc_main" && node.kind == GeometryNodeKind::Device
            })
        );
        assert!(artifact.edges.iter().any(|edge| {
            edge.kind == GeometryEdgeKind::Transition && edge.label == "when X0 == true"
        }));
        assert_eq!(
            artifact
                .overlays
                .trace
                .as_ref()
                .expect("trace overlay")
                .observed_transition_count,
            2
        );
        assert_eq!(
            artifact
                .overlays
                .intent
                .as_ref()
                .expect("intent overlay")
                .verdict,
            IntentAlignmentVerdict::Aligned
        );
        let narrative = artifact.narrative.as_ref().expect("narrative");
        assert_eq!(narrative.tasks.len(), 2);
        let main_task = narrative
            .tasks
            .iter()
            .find(|task| task.task_id == "task:main")
            .expect("main task narrative");
        assert_eq!(main_task.entry_step_id, "step:main.wait_start");
        assert_eq!(
            main_task.main_path_step_ids,
            vec![
                "step:main.wait_start".to_string(),
                "step:main.run".to_string(),
            ]
        );
        assert_eq!(main_task.current_step_id, "step:main.run");
        assert_eq!(main_task.coverage.trace_available, true);
        assert_eq!(main_task.coverage.intent_available, true);
        assert!(main_task.blocking_points.iter().any(|point| {
            point.step_id == "step:main.wait_start"
                && point.release_transitions.iter().any(|transition| {
                    transition.to_step_id == "step:main.run"
                        && transition.guard_label == "when X0 == true"
                })
        }));
        let run_step = main_task
            .steps
            .iter()
            .find(|step| step.step_id == "step:main.run")
            .expect("run step narrative");
        assert!(run_step.is_current);
        assert_eq!(main_task.steps.len(), 2);
        assert_eq!(
            main_task
                .steps
                .iter()
                .filter(|step| step.is_current)
                .count(),
            1
        );
    }
}
