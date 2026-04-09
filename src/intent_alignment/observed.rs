use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::trace_diff::NormalizedTraceEvent;

use super::contract::{ObservationBinding, ObservationSubject};
use super::expected_behavior::ExpectedBehaviorSpec;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RawObservedEvent {
    Transition {
        event: NormalizedTraceEvent,
    },
    VariableSnapshot {
        tick: u64,
        vars: BTreeMap<String, Value>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservedEventSourceKind {
    TraceTransition,
    TraceVariableSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservedComparisonDimension {
    RequiredStep,
    Ordering,
    Postcondition,
    CrossCycle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservedEvidenceGapCode {
    UnsupportedTraceRow,
    MissingCycleBoundaryEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{code:?}: {detail}")]
#[serde(deny_unknown_fields)]
pub struct ObservedEvidenceGap {
    pub code: ObservedEvidenceGapCode,
    pub detail: String,
    pub line: Option<usize>,
    pub tick: Option<u64>,
    pub dimension: Option<ObservedComparisonDimension>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedEvidenceEntry {
    pub tick: u64,
    pub cycle_index: usize,
    pub key: String,
    pub expected: String,
    pub source: ObservedEventSourceKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedEvidenceThresholds {
    pub required_step_min_events: usize,
    pub ordering_min_events: usize,
    pub postcondition_min_events: usize,
    pub cross_cycle_min_cycles: usize,
}

impl Default for ObservedEvidenceThresholds {
    fn default() -> Self {
        Self {
            required_step_min_events: 1,
            ordering_min_events: 2,
            postcondition_min_events: 1,
            cross_cycle_min_cycles: 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedDimensionReadiness {
    pub dimension: ObservedComparisonDimension,
    pub ready: bool,
    pub observed_count: usize,
    pub required_min: usize,
    pub gap: Option<ObservedEvidenceGap>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedBehaviorSequence {
    pub evidence: Vec<ObservedEvidenceEntry>,
    pub cycles: Vec<ObservedCycleWindow>,
    pub snapshots: Vec<ObservedSnapshot>,
    pub thresholds: ObservedEvidenceThresholds,
    pub readiness: Vec<ObservedDimensionReadiness>,
    pub cycle_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedSnapshot {
    pub tick: u64,
    pub cycle_index: usize,
    pub facts: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedCycleWindow {
    pub cycle_index: usize,
    pub start_tick: u64,
    pub end_tick: u64,
    pub first_observed_snapshot: Option<BTreeMap<String, String>>,
    pub cycle_start_snapshot: Option<BTreeMap<String, String>>,
    pub successful_cycle_end_tick: Option<u64>,
    pub successful_cycle_end_snapshot: Option<BTreeMap<String, String>>,
    pub aborted_cycle_end_tick: Option<u64>,
    pub aborted_cycle_end_snapshot: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone)]
struct ObservedCycleBoundary {
    tick: u64,
    snapshot: Option<BTreeMap<String, String>>,
}

#[derive(Debug, thiserror::Error)]
pub enum ObservedTraceParseError {
    #[error("line {line}: invalid JSON: {detail}")]
    InvalidJson { line: usize, detail: String },
    #[error("line {line}: unsupported trace row: {detail}")]
    UnsupportedRow { line: usize, detail: String },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum JsonlObservedRow {
    Transition {
        tick: u64,
        task: usize,
        from_step: u16,
        to_step: u16,
        reason: String,
        #[serde(default)]
        _timestamp_ms: Option<u64>,
    },
    VariableSnapshot {
        tick: u64,
        vars: BTreeMap<String, Value>,
    },
}

pub fn parse_observed_trace_jsonl(
    input: &str,
) -> Result<Vec<RawObservedEvent>, ObservedTraceParseError> {
    let mut out = Vec::new();
    for (idx, raw) in input.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }

        let json: Value =
            serde_json::from_str(line).map_err(|err| ObservedTraceParseError::InvalidJson {
                line: idx + 1,
                detail: err.to_string(),
            })?;
        let parsed: JsonlObservedRow = serde_json::from_value(json).map_err(|err| {
            ObservedTraceParseError::UnsupportedRow {
                line: idx + 1,
                detail: err.to_string(),
            }
        })?;

        match parsed {
            JsonlObservedRow::Transition {
                tick,
                task,
                from_step,
                to_step,
                reason,
                _timestamp_ms: _,
            } => out.push(RawObservedEvent::Transition {
                event: NormalizedTraceEvent {
                    tick,
                    task,
                    from_step,
                    to_step,
                    reason,
                },
            }),
            JsonlObservedRow::VariableSnapshot { tick, vars } => {
                if vars.is_empty() {
                    return Err(ObservedTraceParseError::UnsupportedRow {
                        line: idx + 1,
                        detail: "variable snapshot must contain at least one variable".to_string(),
                    });
                }
                out.push(RawObservedEvent::VariableSnapshot { tick, vars });
            }
        }
    }
    Ok(out)
}

pub fn adapt_normalized_trace_events(events: &[NormalizedTraceEvent]) -> Vec<RawObservedEvent> {
    events
        .iter()
        .cloned()
        .map(|event| RawObservedEvent::Transition { event })
        .collect()
}

pub fn extract_observed_behavior_sequence(
    spec: &ExpectedBehaviorSpec,
    raw_events: &[RawObservedEvent],
) -> Result<ObservedBehaviorSequence, ObservedEvidenceGap> {
    let thresholds = ObservedEvidenceThresholds::default();
    let cycle_start_binding = cycle_start_binding(spec);
    let successful_cycle_end_binding = successful_cycle_end_binding(spec);
    let mut evidence = Vec::new();
    let mut cycle_index = 0usize;
    let mut cycle_boundary_seen = false;
    let mut cycle_ready_for_handoff = false;
    let mut previous_vars: Option<BTreeMap<String, Value>> = None;
    let mut cycle_builders = vec![ObservedCycleWindowBuilder::new(0)];
    let mut snapshots = Vec::new();
    let mut cycle_start_boundaries = Vec::new();
    let mut successful_cycle_end_boundaries = Vec::new();
    let exact_transition_cycles = cycle_start_binding
        .zip(successful_cycle_end_binding)
        .is_some_and(|(start, end)| {
            is_exact_transition_binding(start) && is_exact_transition_binding(end)
        });

    for raw_event in raw_events {
        let tick = raw_event.tick();
        let mut event_entries =
            normalize_event_entries(raw_event, previous_vars.as_ref(), cycle_index);

        if let RawObservedEvent::VariableSnapshot { vars, .. } = raw_event {
            previous_vars = Some(vars.clone());
        }

        let matches_cycle_start = cycle_start_binding
            .is_some_and(|binding| binding_matches_event(binding, raw_event, &event_entries));
        let matches_successful_cycle_end = successful_cycle_end_binding
            .is_some_and(|binding| binding_matches_event(binding, raw_event, &event_entries));
        let starts_next_cycle_candidate = if cycle_start_binding.is_some() {
            cycle_boundary_seen && cycle_ready_for_handoff && matches_cycle_start
        } else {
            cycle_boundary_seen && cycle_ready_for_handoff && !event_entries.is_empty()
        };

        if starts_next_cycle_candidate {
            cycle_index += 1;
            cycle_ready_for_handoff = false;
            for entry in &mut event_entries {
                entry.cycle_index = cycle_index;
            }
            cycle_builders.push(ObservedCycleWindowBuilder::new(cycle_index));
        }

        if matches_cycle_start && !cycle_boundary_seen {
            cycle_boundary_seen = true;
        }
        if matches_cycle_start {
            cycle_start_boundaries.push(ObservedCycleBoundary {
                tick,
                snapshot: raw_event.snapshot_vars().map(canonical_snapshot),
            });
        }
        if matches_successful_cycle_end {
            successful_cycle_end_boundaries.push(ObservedCycleBoundary {
                tick,
                snapshot: raw_event.snapshot_vars().map(canonical_snapshot),
            });
        }

        let builder = cycle_builders
            .last_mut()
            .expect("cycle builder should exist for current cycle");
        builder.observe_tick(tick);
        builder.record_first_observed(raw_event);
        if matches_cycle_start {
            builder.record_cycle_start(raw_event);
        }
        if matches_successful_cycle_end {
            builder.record_successful_cycle_end(tick, raw_event);
            cycle_ready_for_handoff = true;
        }
        if let Some(vars) = raw_event.snapshot_vars() {
            snapshots.push(ObservedSnapshot {
                tick,
                cycle_index,
                facts: canonical_snapshot(vars),
            });
        }

        for entry in event_entries {
            if evidence.last().is_some_and(|previous| previous == &entry) {
                continue;
            }
            evidence.push(entry);
        }

        if cycle_start_binding.is_none() && raw_event.is_variable_snapshot() && tick == 0 {
            cycle_boundary_seen = true;
        }
    }

    if cycle_start_binding.is_some() && !cycle_boundary_seen {
        return Err(ObservedEvidenceGap {
            code: ObservedEvidenceGapCode::MissingCycleBoundaryEvidence,
            detail: format!(
                "no observed event satisfied cycle-start binding for milestone `{}`",
                spec.cycle_semantics.cycle_start_milestone
            ),
            line: None,
            tick: None,
            dimension: Some(ObservedComparisonDimension::CrossCycle),
        });
    }

    let last_tick = raw_events.last().map(RawObservedEvent::tick).unwrap_or(0);
    let (cycles, cycle_count) = if exact_transition_cycles && !cycle_start_boundaries.is_empty() {
        let cycles = build_exact_transition_cycles(
            &cycle_start_boundaries,
            &successful_cycle_end_boundaries,
            last_tick,
        );
        let cycle_count = cycles.len();
        (cycles, cycle_count)
    } else {
        let cycles: Vec<ObservedCycleWindow> = cycle_builders
            .into_iter()
            .filter(|builder| builder.seen_anything)
            .map(ObservedCycleWindowBuilder::finish)
            .collect();
        let cycle_count = if !cycles.is_empty() {
            cycles.len()
        } else if evidence.is_empty() {
            0
        } else {
            evidence
                .iter()
                .map(|entry| entry.cycle_index)
                .max()
                .unwrap_or(0)
                + 1
        };
        (cycles, cycle_count)
    };

    let readiness = vec![
        build_readiness(
            ObservedComparisonDimension::RequiredStep,
            evidence.len(),
            thresholds.required_step_min_events,
            cycle_count,
        ),
        build_readiness(
            ObservedComparisonDimension::Ordering,
            evidence.len(),
            thresholds.ordering_min_events,
            cycle_count,
        ),
        build_readiness(
            ObservedComparisonDimension::Postcondition,
            evidence.len(),
            thresholds.postcondition_min_events,
            cycle_count,
        ),
        build_readiness(
            ObservedComparisonDimension::CrossCycle,
            cycle_count,
            thresholds.cross_cycle_min_cycles,
            cycle_count,
        ),
    ];

    Ok(ObservedBehaviorSequence {
        evidence,
        cycles,
        snapshots,
        thresholds,
        readiness,
        cycle_count,
    })
}

fn is_exact_transition_binding(binding: &ObservationBinding) -> bool {
    binding.evidence.len() == 1
        && binding.evidence[0].key == "transition"
        && matches!(
            binding.combination,
            super::contract::ObservationCombination::AllOf
        )
}

fn build_exact_transition_cycles(
    cycle_start_boundaries: &[ObservedCycleBoundary],
    successful_cycle_end_boundaries: &[ObservedCycleBoundary],
    last_tick: u64,
) -> Vec<ObservedCycleWindow> {
    cycle_start_boundaries
        .iter()
        .enumerate()
        .map(|(cycle_index, start)| {
            let successful_end = successful_cycle_end_boundaries.get(cycle_index);
            ObservedCycleWindow {
                cycle_index,
                start_tick: start.tick,
                end_tick: successful_end.map(|end| end.tick).unwrap_or(last_tick),
                first_observed_snapshot: start.snapshot.clone(),
                cycle_start_snapshot: start.snapshot.clone(),
                successful_cycle_end_tick: successful_end.map(|end| end.tick),
                successful_cycle_end_snapshot: successful_end.and_then(|end| end.snapshot.clone()),
                aborted_cycle_end_tick: None,
                aborted_cycle_end_snapshot: None,
            }
        })
        .collect()
}

fn build_readiness(
    dimension: ObservedComparisonDimension,
    observed_count: usize,
    required_min: usize,
    cycle_count: usize,
) -> ObservedDimensionReadiness {
    let ready = observed_count >= required_min;
    let gap = if ready {
        None
    } else {
        Some(ObservedEvidenceGap {
            code: ObservedEvidenceGapCode::MissingCycleBoundaryEvidence,
            detail: match dimension {
                ObservedComparisonDimension::CrossCycle => format!(
                    "observed evidence only covers {cycle_count} cycle(s); cross-cycle comparison requires at least {required_min}"
                ),
                _ => format!(
                    "observed evidence count {observed_count} is below threshold {required_min} for {dimension:?}"
                ),
            },
            line: None,
            tick: None,
            dimension: Some(dimension),
        })
    };

    ObservedDimensionReadiness {
        dimension,
        ready,
        observed_count,
        required_min,
        gap,
    }
}

fn cycle_start_binding(spec: &ExpectedBehaviorSpec) -> Option<&ObservationBinding> {
    spec.observation_bindings.iter().find(|binding| {
        binding.subject
            == ObservationSubject::Milestone {
                milestone_id: spec.cycle_semantics.cycle_start_milestone.clone(),
            }
    })
}

fn successful_cycle_end_binding(spec: &ExpectedBehaviorSpec) -> Option<&ObservationBinding> {
    spec.observation_bindings.iter().find(|binding| {
        binding.subject
            == ObservationSubject::Milestone {
                milestone_id: spec.cycle_semantics.successful_cycle_end_milestone.clone(),
            }
    })
}

fn binding_matches_entries(
    binding: &ObservationBinding,
    entries: &[ObservedEvidenceEntry],
) -> bool {
    binding.evidence.iter().all(|evidence| {
        entries
            .iter()
            .any(|entry| entry.key == evidence.key && entry.expected == evidence.expected)
    })
}

fn binding_matches_event(
    binding: &ObservationBinding,
    raw_event: &RawObservedEvent,
    entries: &[ObservedEvidenceEntry],
) -> bool {
    if let Some(vars) = raw_event.snapshot_vars() {
        let snapshot = canonical_snapshot(vars);
        return binding
            .evidence
            .iter()
            .all(|evidence| snapshot.get(&evidence.key) == Some(&evidence.expected));
    }

    binding_matches_entries(binding, entries)
}

fn normalize_event_entries(
    raw_event: &RawObservedEvent,
    previous_vars: Option<&BTreeMap<String, Value>>,
    cycle_index: usize,
) -> Vec<ObservedEvidenceEntry> {
    match raw_event {
        RawObservedEvent::Transition { event } => {
            vec![ObservedEvidenceEntry {
                tick: event.tick,
                cycle_index,
                key: "transition".to_string(),
                expected: format!(
                    "task={};from={};to={};reason={}",
                    event.task, event.from_step, event.to_step, event.reason
                ),
                source: ObservedEventSourceKind::TraceTransition,
            }]
        }
        RawObservedEvent::VariableSnapshot { tick, vars } => vars
            .iter()
            .filter_map(|(name, value)| {
                let changed = previous_vars
                    .and_then(|previous| previous.get(name))
                    .is_none_or(|previous_value| previous_value != value);
                if !changed {
                    return None;
                }

                Some(ObservedEvidenceEntry {
                    tick: *tick,
                    cycle_index,
                    key: format!("vars.{name}"),
                    expected: canonical_json_value(value),
                    source: ObservedEventSourceKind::TraceVariableSnapshot,
                })
            })
            .collect(),
    }
}

fn canonical_json_value(value: &Value) -> String {
    serde_json::to_string(value).expect("JSON value should serialize")
}

fn canonical_snapshot(vars: &BTreeMap<String, Value>) -> BTreeMap<String, String> {
    vars.iter()
        .map(|(name, value)| (format!("vars.{name}"), canonical_json_value(value)))
        .collect()
}

impl RawObservedEvent {
    fn tick(&self) -> u64 {
        match self {
            Self::Transition { event } => event.tick,
            Self::VariableSnapshot { tick, .. } => *tick,
        }
    }

    fn is_variable_snapshot(&self) -> bool {
        matches!(self, Self::VariableSnapshot { .. })
    }
}

#[derive(Debug, Clone)]
struct ObservedCycleWindowBuilder {
    cycle_index: usize,
    start_tick: Option<u64>,
    end_tick: Option<u64>,
    first_observed_snapshot: Option<BTreeMap<String, String>>,
    cycle_start_snapshot: Option<BTreeMap<String, String>>,
    successful_cycle_end_tick: Option<u64>,
    successful_cycle_end_snapshot: Option<BTreeMap<String, String>>,
    seen_anything: bool,
}

impl ObservedCycleWindowBuilder {
    fn new(cycle_index: usize) -> Self {
        Self {
            cycle_index,
            start_tick: None,
            end_tick: None,
            first_observed_snapshot: None,
            cycle_start_snapshot: None,
            successful_cycle_end_tick: None,
            successful_cycle_end_snapshot: None,
            seen_anything: false,
        }
    }

    fn observe_tick(&mut self, tick: u64) {
        self.seen_anything = true;
        if self.start_tick.is_none() {
            self.start_tick = Some(tick);
        }
        self.end_tick = Some(tick);
    }

    fn record_first_observed(&mut self, raw_event: &RawObservedEvent) {
        if self.first_observed_snapshot.is_none() {
            self.first_observed_snapshot = raw_event.snapshot_vars().map(canonical_snapshot);
        }
    }

    fn record_cycle_start(&mut self, raw_event: &RawObservedEvent) {
        if self.cycle_start_snapshot.is_none() {
            self.cycle_start_snapshot = raw_event.snapshot_vars().map(canonical_snapshot);
        }
    }

    fn record_successful_cycle_end(&mut self, tick: u64, raw_event: &RawObservedEvent) {
        self.successful_cycle_end_tick = Some(tick);
        self.successful_cycle_end_snapshot = raw_event.snapshot_vars().map(canonical_snapshot);
    }

    fn finish(self) -> ObservedCycleWindow {
        ObservedCycleWindow {
            cycle_index: self.cycle_index,
            start_tick: self.start_tick.unwrap_or(0),
            end_tick: self.end_tick.unwrap_or(0),
            first_observed_snapshot: self.first_observed_snapshot,
            cycle_start_snapshot: self.cycle_start_snapshot,
            successful_cycle_end_tick: self.successful_cycle_end_tick,
            successful_cycle_end_snapshot: self.successful_cycle_end_snapshot,
            aborted_cycle_end_tick: None,
            aborted_cycle_end_snapshot: None,
        }
    }
}

impl RawObservedEvent {
    fn snapshot_vars(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Self::VariableSnapshot { vars, .. } => Some(vars),
            Self::Transition { .. } => None,
        }
    }
}
