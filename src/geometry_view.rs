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

pub const GEOMETRY_VIEW_SCHEMA_VERSION: u32 = 1;
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
    }
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
        TransitionGuard::Timeout { duration_ms } => format!("timeout {duration_ms}ms"),
        TransitionGuard::Delay { duration_ms } => format!("delay {duration_ms}ms"),
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
        assert!(artifact
            .nodes
            .iter()
            .any(|node| node.id == "task:main" && node.kind == GeometryNodeKind::Task));
        assert!(artifact.nodes.iter().any(|node| {
            node.id == "step:main.wait_start" && node.kind == GeometryNodeKind::Step
        }));
        assert!(artifact
            .nodes
            .iter()
            .any(|node| { node.id == "device:plc_main" && node.kind == GeometryNodeKind::Device }));
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
    }
}
