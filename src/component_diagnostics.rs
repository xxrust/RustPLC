use crate::component_faults::ComponentFaultKind;
use crate::component_sim::{ComponentFaultAuditEntry, ComponentSimReport};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComponentEvidenceSource {
    ProgramBehavior,
    FaultInjection,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentEvidenceEntry {
    pub source: ComponentEvidenceSource,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentFaultContext {
    pub component_id: String,
    pub fault_kind: ComponentFaultKind,
    pub start_ms: u64,
    pub end_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComponentDiagnosisCandidate {
    pub issue_code: String,
    pub rank: u32,
    pub confidence: f64,
    pub evidence: Vec<ComponentEvidenceEntry>,
    pub fault_context: Option<ComponentFaultContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComponentDiagnosisReport {
    pub schema_version: u32,
    pub candidates: Vec<ComponentDiagnosisCandidate>,
}

pub fn diagnose_component_sim(report: &ComponentSimReport) -> ComponentDiagnosisReport {
    let contexts = merge_fault_windows(&report.fault_audit);
    let mut candidates = Vec::new();
    for (idx, context) in contexts.into_iter().enumerate() {
        let issue_code = issue_code_for_fault(context.fault_kind).to_string();
        let behavior_msg = summarize_component_behavior(report, &context.component_id);
        candidates.push(ComponentDiagnosisCandidate {
            issue_code,
            rank: u32::try_from(idx + 1).unwrap_or(u32::MAX),
            confidence: confidence_for_fault(context.fault_kind),
            evidence: vec![
                ComponentEvidenceEntry {
                    source: ComponentEvidenceSource::FaultInjection,
                    message: format!(
                        "fault `{}` on `{}` active from {}ms to {}",
                        fault_kind_label(context.fault_kind),
                        context.component_id,
                        context.start_ms,
                        context
                            .end_ms
                            .map(|v| format!("{v}ms"))
                            .unwrap_or_else(|| "end_of_run".to_string())
                    ),
                },
                ComponentEvidenceEntry {
                    source: ComponentEvidenceSource::ProgramBehavior,
                    message: behavior_msg,
                },
            ],
            fault_context: Some(context),
        });
    }
    ComponentDiagnosisReport {
        schema_version: 1,
        candidates,
    }
}

fn summarize_component_behavior(report: &ComponentSimReport, component_id: &str) -> String {
    let mut states = Vec::<String>::new();
    for tick in &report.ticks {
        if let Some(component) = tick.components.get(component_id) {
            if states.last() != Some(&component.state) {
                states.push(component.state.clone());
            }
        }
    }
    if states.is_empty() {
        return "no runtime state observed for component".to_string();
    }
    if states.len() == 1 {
        return format!("component state stayed `{}` during sampled run", states[0]);
    }
    format!(
        "component state transitions observed: {}",
        states.join(" -> ")
    )
}

fn merge_fault_windows(audit: &[ComponentFaultAuditEntry]) -> Vec<ComponentFaultContext> {
    let mut starts = BTreeMap::<String, u64>::new();
    let mut contexts = Vec::<ComponentFaultContext>::new();
    let mut ordered = audit.to_vec();
    ordered.sort_by_key(|entry| (entry.tick, entry.event_index));
    for entry in ordered {
        let key = fault_window_key(
            &entry.target_component_id,
            entry.fault_kind,
            entry.event_index,
        );
        if entry.action == "activated" {
            starts.insert(key, entry.at_ms);
            continue;
        }
        if entry.action == "expired" {
            if let Some(start_ms) = starts.remove(&key) {
                contexts.push(ComponentFaultContext {
                    component_id: entry.target_component_id,
                    fault_kind: entry.fault_kind,
                    start_ms,
                    end_ms: Some(entry.at_ms),
                });
            }
        }
    }
    for (key, start_ms) in starts {
        let Some((component_id, fault_kind)) = parse_fault_window_key(&key) else {
            continue;
        };
        contexts.push(ComponentFaultContext {
            component_id,
            fault_kind,
            start_ms,
            end_ms: None,
        });
    }
    contexts.sort_by_key(|ctx| {
        (
            ctx.start_ms,
            ctx.component_id.clone(),
            issue_code_for_fault(ctx.fault_kind),
        )
    });
    contexts
}

fn fault_window_key(
    component_id: &str,
    fault_kind: ComponentFaultKind,
    event_index: usize,
) -> String {
    format!(
        "{}|{}|{}",
        component_id,
        fault_kind_label(fault_kind),
        event_index
    )
}

fn parse_fault_window_key(key: &str) -> Option<(String, ComponentFaultKind)> {
    let (component_id, rest) = key.split_once('|')?;
    let (fault_kind, _event_index) = rest.split_once('|')?;
    let fault_kind = match fault_kind {
        "jammed" => ComponentFaultKind::Jammed,
        "motion_timeout" => ComponentFaultKind::MotionTimeout,
        "stuck_on" => ComponentFaultKind::StuckOn,
        "stuck_off" => ComponentFaultKind::StuckOff,
        "chatter" => ComponentFaultKind::Chatter,
        "lost_step" => ComponentFaultKind::LostStep,
        "stall" => ComponentFaultKind::Stall,
        "direction_reversed" => ComponentFaultKind::DirectionReversed,
        _ => return None,
    };
    Some((component_id.to_string(), fault_kind))
}

fn issue_code_for_fault(kind: ComponentFaultKind) -> &'static str {
    match kind {
        ComponentFaultKind::Jammed => "DIAG-COMP-001",
        ComponentFaultKind::MotionTimeout => "DIAG-COMP-002",
        ComponentFaultKind::StuckOn => "DIAG-COMP-003",
        ComponentFaultKind::StuckOff => "DIAG-COMP-004",
        ComponentFaultKind::Chatter => "DIAG-COMP-005",
        ComponentFaultKind::LostStep => "DIAG-COMP-006",
        ComponentFaultKind::Stall => "DIAG-COMP-007",
        ComponentFaultKind::DirectionReversed => "DIAG-COMP-008",
    }
}

fn confidence_for_fault(kind: ComponentFaultKind) -> f64 {
    match kind {
        ComponentFaultKind::Jammed | ComponentFaultKind::Stall => 0.95,
        ComponentFaultKind::MotionTimeout
        | ComponentFaultKind::LostStep
        | ComponentFaultKind::DirectionReversed => 0.88,
        ComponentFaultKind::StuckOn
        | ComponentFaultKind::StuckOff
        | ComponentFaultKind::Chatter => 0.85,
    }
}

fn fault_kind_label(kind: ComponentFaultKind) -> &'static str {
    match kind {
        ComponentFaultKind::Jammed => "jammed",
        ComponentFaultKind::MotionTimeout => "motion_timeout",
        ComponentFaultKind::StuckOn => "stuck_on",
        ComponentFaultKind::StuckOff => "stuck_off",
        ComponentFaultKind::Chatter => "chatter",
        ComponentFaultKind::LostStep => "lost_step",
        ComponentFaultKind::Stall => "stall",
        ComponentFaultKind::DirectionReversed => "direction_reversed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component_scenario::parse_component_scenario_json;
    use crate::component_sim::run_component_simulation;
    use crate::component_topology::parse_component_topology_json;

    #[test]
    fn diagnosis_includes_fault_context_and_evidence_sources() {
        let topology = parse_component_topology_json(
            r#"{
  "schema_version": 1,
  "component_library": {
    "schema_version": 1,
    "components": [
      { "id": "sw", "name": "Start", "type": "switch", "params": {} },
      { "id": "stp", "name": "Axis", "type": "stepper_pd", "params": {} }
    ]
  },
  "components": [
    { "id": "s0", "component_id": "sw", "params": {} },
    { "id": "m0", "component_id": "stp", "params": {} }
  ],
  "connections": [
    { "from": "s0.state", "to": "m0.pulse" },
    { "from": "s0.state", "to": "m0.enable" },
    { "from": "s0.state", "to": "m0.direction" }
  ]
}"#,
        )
        .expect("topology");
        let scenario = parse_component_scenario_json(
            r#"{
  "schema_version": 1,
  "tick_ms": 10,
  "duration_ms": 50,
  "switch_events": [
    { "at_ms": 0, "target": "s0", "value": true },
    { "at_ms": 10, "target": "s0", "value": false }
  ],
  "component_faults": [
    { "at_ms": 10, "duration_ms": 20, "target_component_id": "m0", "fault_kind": "stall" }
  ]
}"#,
        )
        .expect("scenario");
        let sim = run_component_simulation(&topology, &scenario).expect("sim");
        let diagnosis = diagnose_component_sim(&sim);
        assert!(
            !diagnosis.candidates.is_empty(),
            "should have diagnosis candidate"
        );
        let first = &diagnosis.candidates[0];
        assert_eq!(first.issue_code, "DIAG-COMP-007");
        assert!(first
            .evidence
            .iter()
            .any(|entry| { entry.source == ComponentEvidenceSource::FaultInjection }));
        assert!(first
            .evidence
            .iter()
            .any(|entry| { entry.source == ComponentEvidenceSource::ProgramBehavior }));
        assert!(
            first.fault_context.is_some(),
            "fault context should be present"
        );
    }
}
