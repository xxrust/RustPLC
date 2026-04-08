use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ir::{State, StateMachine, TransitionAction, WorkpieceEffect};
use crate::task_root_selection::select_root_task_contexts;
use crate::trace_diff::NormalizedTraceEvent;

use super::contract::{ObservationBinding, ObservationCombination, ObservationSubject};
use super::expected_behavior::ExpectedBehaviorSpec;
use super::observed::{
    ObservedBehaviorSequence, ObservedEvidenceEntry, adapt_normalized_trace_events,
    extract_observed_behavior_sequence,
};

const INTENT_DOCTOR_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingStability {
    Stable,
    Partial,
    Repeated,
    Missing,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentDoctorTransitionSummary {
    pub transition: String,
    pub reason: String,
    pub occurrence_count: usize,
    pub first_tick: u64,
    pub last_tick: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observed_cycles: Vec<usize>,
    pub root_task: String,
    pub from_state: String,
    pub to_state: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub action_kinds: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workpiece_effects: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentDoctorCandidate {
    pub rank: usize,
    pub transition: String,
    pub score: f64,
    pub occurrence_count: usize,
    pub first_tick: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observed_cycles: Vec<usize>,
    pub root_task: String,
    pub from_state: String,
    pub to_state: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workpiece_effects: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentDoctorBindingDiagnosis {
    pub binding_id: String,
    pub subject: String,
    pub status: BindingStability,
    pub expected_occurrences: usize,
    pub observed_occurrences: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observed_ticks: Vec<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observed_cycles: Vec<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggested_replacements: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentDoctorContractDiagnosis {
    pub contract_id: String,
    pub contract_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub milestone_bindings: Vec<IntentDoctorBindingDiagnosis>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentDoctorCycleDiagnosis {
    pub observed_cycle_count: usize,
    pub cross_cycle_ready: bool,
    pub trailing_partial_cycle: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycle_start_status: Option<BindingStability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycle_complete_status: Option<BindingStability>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentDoctorReport {
    pub schema_version: u32,
    pub observed_transition_count: usize,
    pub unique_transition_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transition_summaries: Vec<IntentDoctorTransitionSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<IntentDoctorCandidate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_diagnosis: Option<IntentDoctorContractDiagnosis>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycle_diagnosis: Option<IntentDoctorCycleDiagnosis>,
}

#[derive(Debug, Error)]
pub enum IntentDoctorError {
    #[error(
        "trace event references runtime task index {task_index}, but only {task_count} runtime task(s) were derived from the state machine"
    )]
    UnknownRuntimeTask {
        task_index: usize,
        task_count: usize,
    },
    #[error(
        "trace event references runtime step {step_id} in task `{root_task}`, but only {step_count} step(s) exist in that runtime task"
    )]
    UnknownRuntimeStep {
        root_task: String,
        step_id: u16,
        step_count: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentDoctorRuntimeTaskLayout {
    pub root_task: String,
    pub step_keys: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
struct BindingOccurrence {
    cycle_index: usize,
    tick: u64,
}

#[derive(Debug, Clone)]
struct TransitionSummaryBuilder {
    transition: String,
    reason: String,
    occurrence_count: usize,
    first_tick: u64,
    last_tick: u64,
    observed_cycles: BTreeSet<usize>,
    root_task: String,
    from_state: String,
    to_state: String,
    action_kinds: BTreeSet<String>,
    workpiece_effects: BTreeSet<String>,
}

pub fn diagnose_intent_alignment(
    state_machine: &StateMachine,
    trace_events: &[NormalizedTraceEvent],
    expected_behavior: Option<&ExpectedBehaviorSpec>,
) -> Result<IntentDoctorReport, IntentDoctorError> {
    let runtime_layouts = build_runtime_task_layouts(state_machine);
    diagnose_intent_alignment_with_layouts(
        state_machine,
        trace_events,
        expected_behavior,
        &runtime_layouts,
    )
}

pub fn diagnose_intent_alignment_with_layouts(
    state_machine: &StateMachine,
    trace_events: &[NormalizedTraceEvent],
    expected_behavior: Option<&ExpectedBehaviorSpec>,
    runtime_layouts: &[IntentDoctorRuntimeTaskLayout],
) -> Result<IntentDoctorReport, IntentDoctorError> {
    let transition_lookup = build_transition_lookup(state_machine);
    let observed_input = adapt_normalized_trace_events(trace_events);
    let observed = expected_behavior
        .and_then(|spec| extract_observed_behavior_sequence(spec, &observed_input).ok());
    let contract_blocked_reason = expected_behavior.and_then(|spec| {
        extract_observed_behavior_sequence(spec, &observed_input)
            .err()
            .map(|gap| gap.to_string())
    });
    let event_cycles = event_cycle_indices(trace_events, observed.as_ref());
    let mut summaries = BTreeMap::<String, TransitionSummaryBuilder>::new();

    for (idx, event) in trace_events.iter().enumerate() {
        let layout =
            runtime_layouts
                .get(event.task)
                .ok_or(IntentDoctorError::UnknownRuntimeTask {
                    task_index: event.task,
                    task_count: runtime_layouts.len(),
                })?;
        let from_key = layout
            .step_keys
            .get(event.from_step as usize)
            .ok_or_else(|| IntentDoctorError::UnknownRuntimeStep {
                root_task: layout.root_task.clone(),
                step_id: event.from_step,
                step_count: layout.step_keys.len(),
            })?;
        let to_key = layout
            .step_keys
            .get(event.to_step as usize)
            .ok_or_else(|| IntentDoctorError::UnknownRuntimeStep {
                root_task: layout.root_task.clone(),
                step_id: event.to_step,
                step_count: layout.step_keys.len(),
            })?;
        let summary_key = transition_key(event);
        let builder =
            summaries
                .entry(summary_key.clone())
                .or_insert_with(|| TransitionSummaryBuilder {
                    transition: summary_key.clone(),
                    reason: event.reason.clone(),
                    occurrence_count: 0,
                    first_tick: event.tick,
                    last_tick: event.tick,
                    observed_cycles: BTreeSet::new(),
                    root_task: layout.root_task.clone(),
                    from_state: format!("{}.{}", from_key.0, from_key.1),
                    to_state: format!("{}.{}", to_key.0, to_key.1),
                    action_kinds: BTreeSet::new(),
                    workpiece_effects: BTreeSet::new(),
                });

        builder.occurrence_count += 1;
        builder.first_tick = builder.first_tick.min(event.tick);
        builder.last_tick = builder.last_tick.max(event.tick);
        if let Some(cycle_index) = event_cycles.get(idx).and_then(|cycle| *cycle) {
            builder.observed_cycles.insert(cycle_index);
        }

        if let Some(transitions) = transition_lookup.get(&(
            from_key.0.clone(),
            from_key.1.clone(),
            to_key.0.clone(),
            to_key.1.clone(),
        )) {
            for transition in transitions {
                for action in &transition.actions {
                    builder.action_kinds.insert(render_action_kind(action));
                }
                for effect in &transition.effects {
                    builder
                        .workpiece_effects
                        .insert(render_workpiece_effect(effect));
                }
            }
        }
    }

    let mut transition_summaries = summaries
        .into_values()
        .map(|builder| IntentDoctorTransitionSummary {
            transition: builder.transition,
            reason: builder.reason,
            occurrence_count: builder.occurrence_count,
            first_tick: builder.first_tick,
            last_tick: builder.last_tick,
            observed_cycles: builder.observed_cycles.into_iter().collect(),
            root_task: builder.root_task,
            from_state: builder.from_state,
            to_state: builder.to_state,
            action_kinds: builder.action_kinds.into_iter().collect(),
            workpiece_effects: builder.workpiece_effects.into_iter().collect(),
        })
        .collect::<Vec<_>>();

    transition_summaries.sort_by(|left, right| {
        left.first_tick
            .cmp(&right.first_tick)
            .then_with(|| left.transition.cmp(&right.transition))
    });

    let mut candidates = transition_summaries
        .iter()
        .map(|summary| candidate_from_summary(summary, observed.as_ref()))
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.occurrence_count.cmp(&right.occurrence_count))
            .then_with(|| left.first_tick.cmp(&right.first_tick))
            .then_with(|| left.transition.cmp(&right.transition))
    });
    for (rank, candidate) in candidates.iter_mut().enumerate() {
        candidate.rank = rank + 1;
    }

    let contract_diagnosis = expected_behavior.map(|spec| {
        diagnose_contract(
            spec,
            observed.as_ref(),
            contract_blocked_reason.as_deref(),
            &candidates,
        )
    });
    let cycle_diagnosis = expected_behavior.map(|spec| {
        diagnose_cycles(
            spec,
            observed.as_ref(),
            contract_diagnosis.as_ref(),
            contract_blocked_reason,
        )
    });

    Ok(IntentDoctorReport {
        schema_version: INTENT_DOCTOR_SCHEMA_VERSION,
        observed_transition_count: trace_events.len(),
        unique_transition_count: transition_summaries.len(),
        transition_summaries,
        candidates,
        contract_diagnosis,
        cycle_diagnosis,
    })
}

fn diagnose_contract(
    spec: &ExpectedBehaviorSpec,
    observed: Option<&ObservedBehaviorSequence>,
    blocked_reason: Option<&str>,
    candidates: &[IntentDoctorCandidate],
) -> IntentDoctorContractDiagnosis {
    let expected_occurrences = observed
        .map(|sequence| sequence.cycle_count.max(1))
        .unwrap_or(1);
    let candidate_keys = candidates
        .iter()
        .map(|candidate| candidate.transition.as_str())
        .collect::<Vec<_>>();

    let milestone_bindings = spec
        .observation_bindings
        .iter()
        .filter(|binding| matches!(binding.subject, ObservationSubject::Milestone { .. }))
        .map(|binding| {
            let occurrences = binding_occurrences(binding, observed);
            let exact_transition = exact_transition_binding(binding);
            let observed_occurrences = if let Some(transition) = exact_transition.as_deref() {
                candidates
                    .iter()
                    .find(|candidate| candidate.transition == transition)
                    .map(|candidate| candidate.occurrence_count)
                    .unwrap_or(0)
            } else {
                occurrences.len()
            };
            let observed_ticks = occurrences.iter().map(|occurrence| occurrence.tick).collect();
            let observed_cycles = occurrences
                .iter()
                .map(|occurrence| occurrence.cycle_index)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let status = classify_binding_status(
                binding,
                observed.is_some(),
                expected_occurrences,
                observed_occurrences,
                &observed_cycles,
            );
            let note = if exact_transition.is_none() && observed.is_none() {
                Some(
                    "binding diagnosis fell back to exact transition counts because observed cycle groups were unavailable"
                        .to_string(),
                )
            } else if exact_transition.is_none() && observed.is_some() {
                Some(
                    "binding is not a single transition anchor; doctor matched grouped evidence instead"
                        .to_string(),
                )
            } else {
                None
            };
            let suggested_replacements = if matches!(
                status,
                BindingStability::Missing | BindingStability::Repeated | BindingStability::Partial
            ) {
                candidate_keys
                    .iter()
                    .filter(|candidate| Some(**candidate) != exact_transition.as_deref())
                    .take(3)
                    .map(|candidate| (*candidate).to_string())
                    .collect()
            } else {
                Vec::new()
            };

            IntentDoctorBindingDiagnosis {
                binding_id: binding.binding_id.clone(),
                subject: render_subject(&binding.subject),
                status,
                expected_occurrences,
                observed_occurrences,
                observed_ticks,
                observed_cycles,
                evidence: binding
                    .evidence
                    .iter()
                    .map(|evidence| format!("{}={}", evidence.key, evidence.expected))
                    .collect(),
                suggested_replacements,
                note,
            }
        })
        .collect();

    IntentDoctorContractDiagnosis {
        contract_id: spec.contract_id.clone(),
        contract_version: spec.contract_version.clone(),
        blocked_reason: blocked_reason.map(|reason| reason.to_string()),
        milestone_bindings,
    }
}

fn diagnose_cycles(
    spec: &ExpectedBehaviorSpec,
    observed: Option<&ObservedBehaviorSequence>,
    contract_diagnosis: Option<&IntentDoctorContractDiagnosis>,
    blocked_reason: Option<String>,
) -> IntentDoctorCycleDiagnosis {
    let observed_cycle_count = observed.map(|sequence| sequence.cycle_count).unwrap_or(0);
    let cross_cycle_ready = observed
        .and_then(|sequence| {
            sequence
                .readiness
                .iter()
                .find(|readiness| {
                    matches!(
                        readiness.dimension,
                        super::observed::ObservedComparisonDimension::CrossCycle
                    )
                })
                .map(|readiness| readiness.ready)
        })
        .unwrap_or(false);
    let trailing_partial_cycle = observed
        .and_then(|sequence| sequence.cycles.last())
        .is_some_and(|cycle| observed_cycle_count > 1 && cycle.successful_cycle_end_tick.is_none());
    let cycle_start_status = contract_diagnosis.and_then(|diagnosis| {
        diagnosis
            .milestone_bindings
            .iter()
            .find(|binding| {
                binding.subject
                    == format!("milestone:{}", spec.cycle_semantics.cycle_start_milestone)
            })
            .map(|binding| binding.status)
    });
    let cycle_complete_status = contract_diagnosis.and_then(|diagnosis| {
        diagnosis
            .milestone_bindings
            .iter()
            .find(|binding| {
                binding.subject
                    == format!(
                        "milestone:{}",
                        spec.cycle_semantics.successful_cycle_end_milestone
                    )
            })
            .map(|binding| binding.status)
    });

    let mut notes = Vec::new();
    if let Some(reason) = blocked_reason {
        notes.push(reason);
    }
    if trailing_partial_cycle {
        notes.push(
            "last observed cycle started but never hit the successful cycle-end anchor; treat it as a trailing partial cycle artifact"
                .to_string(),
        );
    }
    if !cross_cycle_ready {
        notes.push(
            "cross-cycle diagnosis is weak because observed evidence does not cover at least two complete cycles"
                .to_string(),
        );
    }

    IntentDoctorCycleDiagnosis {
        observed_cycle_count,
        cross_cycle_ready,
        trailing_partial_cycle,
        cycle_start_status,
        cycle_complete_status,
        notes,
    }
}

fn binding_occurrences(
    binding: &ObservationBinding,
    observed: Option<&ObservedBehaviorSequence>,
) -> Vec<BindingOccurrence> {
    let Some(observed) = observed else {
        return Vec::new();
    };

    let groups = grouped_entries(observed);
    match binding.combination {
        ObservationCombination::AllOf => groups
            .into_iter()
            .filter_map(|((cycle_index, tick), entries)| {
                binding_matches_entries(binding, &entries)
                    .then_some(BindingOccurrence { cycle_index, tick })
            })
            .collect(),
        ObservationCombination::AnyOf => groups
            .into_iter()
            .filter_map(|((cycle_index, tick), entries)| {
                binding_matches_any_entry(binding, &entries)
                    .then_some(BindingOccurrence { cycle_index, tick })
            })
            .collect(),
        ObservationCombination::OrderedAllOf => ordered_binding_occurrences(binding, &groups),
    }
}

fn grouped_entries(
    observed: &ObservedBehaviorSequence,
) -> BTreeMap<(usize, u64), Vec<&ObservedEvidenceEntry>> {
    let mut groups = BTreeMap::<(usize, u64), Vec<&ObservedEvidenceEntry>>::new();
    for entry in &observed.evidence {
        groups
            .entry((entry.cycle_index, entry.tick))
            .or_default()
            .push(entry);
    }
    groups
}

fn ordered_binding_occurrences(
    binding: &ObservationBinding,
    groups: &BTreeMap<(usize, u64), Vec<&ObservedEvidenceEntry>>,
) -> Vec<BindingOccurrence> {
    let mut out = Vec::new();
    let expected = binding
        .evidence
        .iter()
        .map(|evidence| (&evidence.key, &evidence.expected))
        .collect::<Vec<_>>();

    let cycle_ids = groups
        .keys()
        .map(|(cycle_index, _)| *cycle_index)
        .collect::<BTreeSet<_>>();
    for cycle_index in cycle_ids {
        let cycle_groups = groups
            .iter()
            .filter(|((group_cycle, _), _)| *group_cycle == cycle_index)
            .collect::<Vec<_>>();
        let mut next_idx = 0usize;
        let mut matched_tick = None;
        for ((_, tick), entries) in cycle_groups {
            if next_idx >= expected.len() {
                break;
            }
            if entries.iter().any(|entry| {
                entry.key == *expected[next_idx].0 && entry.expected == *expected[next_idx].1
            }) {
                matched_tick = Some(*tick);
                next_idx += 1;
            }
        }
        if next_idx == expected.len() {
            out.push(BindingOccurrence {
                cycle_index,
                tick: matched_tick.unwrap_or_default(),
            });
        }
    }
    out
}

fn binding_matches_entries(
    binding: &ObservationBinding,
    entries: &[&ObservedEvidenceEntry],
) -> bool {
    binding.evidence.iter().all(|evidence| {
        entries
            .iter()
            .any(|entry| entry.key == evidence.key && entry.expected == evidence.expected)
    })
}

fn binding_matches_any_entry(
    binding: &ObservationBinding,
    entries: &[&ObservedEvidenceEntry],
) -> bool {
    binding.evidence.iter().any(|evidence| {
        entries
            .iter()
            .any(|entry| entry.key == evidence.key && entry.expected == evidence.expected)
    })
}

fn classify_binding_status(
    binding: &ObservationBinding,
    has_grouped_observed: bool,
    expected_occurrences: usize,
    observed_occurrences: usize,
    observed_cycles: &[usize],
) -> BindingStability {
    let exact_transition = exact_transition_binding(binding);
    if !has_grouped_observed && exact_transition.is_none() {
        return BindingStability::Unsupported;
    }
    if observed_occurrences == 0 {
        return BindingStability::Missing;
    }
    if observed_occurrences > expected_occurrences {
        return BindingStability::Repeated;
    }
    if observed_cycles.len() != observed_occurrences {
        return BindingStability::Repeated;
    }
    if observed_occurrences < expected_occurrences {
        return BindingStability::Partial;
    }
    BindingStability::Stable
}

fn exact_transition_binding(binding: &ObservationBinding) -> Option<String> {
    if binding.evidence.len() != 1 || !matches!(binding.combination, ObservationCombination::AllOf)
    {
        return None;
    }
    let evidence = &binding.evidence[0];
    (evidence.key == "transition").then_some(evidence.expected.clone())
}

fn candidate_from_summary(
    summary: &IntentDoctorTransitionSummary,
    observed: Option<&ObservedBehaviorSequence>,
) -> IntentDoctorCandidate {
    let expected_cycles = observed
        .map(|sequence| sequence.cycle_count.max(1))
        .unwrap_or(1);
    let mut score = 0.0;
    let mut reasons = Vec::new();

    if summary.occurrence_count == 1 {
        score += 4.0;
        reasons.push("observed_once".to_string());
    } else if summary.occurrence_count == expected_cycles {
        score += 2.0;
        reasons.push("one_per_observed_cycle".to_string());
    } else {
        score -= (summary.occurrence_count.saturating_sub(expected_cycles)) as f64;
        reasons.push("repeated_housekeeping_risk".to_string());
    }

    if matches!(summary.reason.as_str(), "action" | "wait_satisfied") {
        score += 1.0;
        reasons.push(format!("reason={}", summary.reason));
    }

    if !summary.workpiece_effects.is_empty() {
        score += 6.0;
        reasons.push("has_workpiece_semantics".to_string());
        for effect in &summary.workpiece_effects {
            score += workpiece_effect_weight(effect);
            reasons.push(format!("effect={effect}"));
        }
    }

    if summary.from_state.split('.').next() != summary.to_state.split('.').next() {
        score += 0.5;
        reasons.push("cross_task_handoff".to_string());
    }

    IntentDoctorCandidate {
        rank: 0,
        transition: summary.transition.clone(),
        score,
        occurrence_count: summary.occurrence_count,
        first_tick: summary.first_tick,
        observed_cycles: summary.observed_cycles.clone(),
        root_task: summary.root_task.clone(),
        from_state: summary.from_state.clone(),
        to_state: summary.to_state.clone(),
        workpiece_effects: summary.workpiece_effects.clone(),
        reasons,
    }
}

fn workpiece_effect_weight(effect: &str) -> f64 {
    if effect.starts_with("finish ") {
        3.0
    } else if effect.starts_with("transfer ") || effect.starts_with("acquire ") {
        2.5
    } else if effect.starts_with("mount ") || effect.starts_with("unmount ") {
        1.5
    } else {
        1.0
    }
}

fn render_subject(subject: &ObservationSubject) -> String {
    match subject {
        ObservationSubject::Milestone { milestone_id } => format!("milestone:{milestone_id}"),
        ObservationSubject::Postcondition { postcondition_id } => {
            format!("postcondition:{postcondition_id}")
        }
    }
}

fn event_cycle_indices(
    trace_events: &[NormalizedTraceEvent],
    observed: Option<&ObservedBehaviorSequence>,
) -> Vec<Option<usize>> {
    let Some(observed) = observed else {
        return vec![None; trace_events.len()];
    };

    trace_events
        .iter()
        .map(|event| {
            observed
                .cycles
                .iter()
                .find(|cycle| event.tick >= cycle.start_tick && event.tick <= cycle.end_tick)
                .map(|cycle| cycle.cycle_index)
                .or_else(|| observed.cycles.last().map(|cycle| cycle.cycle_index))
        })
        .collect()
}

fn build_runtime_task_layouts(state_machine: &StateMachine) -> Vec<IntentDoctorRuntimeTaskLayout> {
    let task_entry_states = state_machine
        .task_contexts
        .iter()
        .map(|ctx| (ctx.task_name.clone(), ctx.entry_state.clone()))
        .collect::<HashMap<_, _>>();
    let known_state_keys = state_machine
        .states
        .iter()
        .map(|state| (state.task_name.clone(), state.step_name.clone()))
        .collect::<HashSet<_>>();
    let mut outgoing_indices: HashMap<(String, String), Vec<usize>> = HashMap::new();
    for (idx, transition) in state_machine.transitions.iter().enumerate() {
        outgoing_indices
            .entry((
                transition.from.task_name.clone(),
                transition.from.step_name.clone(),
            ))
            .or_default()
            .push(idx);
    }

    let roots = select_root_task_contexts(state_machine, motion_branch_target_tasks);
    roots
        .into_iter()
        .filter_map(|root_task| {
            let reachable = collect_runtime_task_state_keys(
                &root_task,
                &task_entry_states,
                &known_state_keys,
                &outgoing_indices,
                state_machine,
            );
            let step_keys = state_machine
                .states
                .iter()
                .filter(|state| {
                    reachable.contains(&(state.task_name.clone(), state.step_name.clone()))
                })
                .map(|state| (state.task_name.clone(), state.step_name.clone()))
                .collect::<Vec<_>>();
            (!step_keys.is_empty()).then_some(IntentDoctorRuntimeTaskLayout {
                root_task,
                step_keys,
            })
        })
        .collect()
}

fn build_transition_lookup(
    state_machine: &StateMachine,
) -> BTreeMap<(String, String, String, String), Vec<&crate::ir::Transition>> {
    let mut lookup =
        BTreeMap::<(String, String, String, String), Vec<&crate::ir::Transition>>::new();
    for transition in &state_machine.transitions {
        lookup
            .entry((
                transition.from.task_name.clone(),
                transition.from.step_name.clone(),
                transition.to.task_name.clone(),
                transition.to.step_name.clone(),
            ))
            .or_default()
            .push(transition);
    }
    lookup
}

fn collect_runtime_task_state_keys(
    root_task: &str,
    task_entry_states: &HashMap<String, State>,
    known_state_keys: &HashSet<(String, String)>,
    outgoing_indices: &HashMap<(String, String), Vec<usize>>,
    state_machine: &StateMachine,
) -> HashSet<(String, String)> {
    let Some(entry_state) = task_entry_states.get(root_task) else {
        return HashSet::new();
    };

    let mut reachable = HashSet::<(String, String)>::new();
    let mut queue = VecDeque::new();
    queue.push_back((entry_state.task_name.clone(), entry_state.step_name.clone()));

    while let Some(state_key) = queue.pop_front() {
        if !reachable.insert(state_key.clone()) {
            continue;
        }

        let Some(transition_indices) = outgoing_indices.get(&state_key) else {
            continue;
        };

        for transition_idx in transition_indices {
            let transition = &state_machine.transitions[*transition_idx];
            queue.push_back((
                transition.to.task_name.clone(),
                transition.to.step_name.clone(),
            ));
            for target in motion_branch_target_state_keys(
                &transition.actions,
                task_entry_states,
                known_state_keys,
            ) {
                queue.push_back(target);
            }
        }
    }

    reachable
}

fn motion_branch_target_tasks(actions: &[TransitionAction]) -> Vec<String> {
    let mut out = Vec::new();
    for action in actions {
        match action {
            TransitionAction::Extend {
                timeout,
                on_motion_fault,
                on_safety_fault,
                ..
            }
            | TransitionAction::Retract {
                timeout,
                on_motion_fault,
                on_safety_fault,
                ..
            } => {
                if let Some(timeout) = timeout {
                    out.push(timeout.target_task.clone());
                }
                if let Some(branch) = on_motion_fault {
                    out.push(branch.target_task.clone());
                }
                if let Some(branch) = on_safety_fault {
                    out.push(branch.target_task.clone());
                }
            }
            TransitionAction::AxisMoveRelative {
                timeout,
                on_reject,
                on_motion_fault,
                on_safety_fault,
                on_reject_routes,
                on_motion_fault_routes,
                on_safety_fault_routes,
                ..
            }
            | TransitionAction::AxisMoveAbsolute {
                timeout,
                on_reject,
                on_motion_fault,
                on_safety_fault,
                on_reject_routes,
                on_motion_fault_routes,
                on_safety_fault_routes,
                ..
            } => {
                out.push(timeout.target_task.clone());
                out.push(on_reject.target_task.clone());
                out.push(on_motion_fault.target_task.clone());
                out.push(on_safety_fault.target_task.clone());
                out.extend(
                    on_reject_routes
                        .iter()
                        .map(|route| route.target_task.clone()),
                );
                out.extend(
                    on_motion_fault_routes
                        .iter()
                        .map(|route| route.target_task.clone()),
                );
                out.extend(
                    on_safety_fault_routes
                        .iter()
                        .map(|route| route.target_task.clone()),
                );
            }
            _ => {}
        }
    }
    out
}

fn motion_branch_target_state_keys(
    actions: &[TransitionAction],
    task_entry_states: &HashMap<String, State>,
    known_state_keys: &HashSet<(String, String)>,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for action in actions {
        match action {
            TransitionAction::Extend {
                timeout,
                on_motion_fault,
                on_safety_fault,
                ..
            }
            | TransitionAction::Retract {
                timeout,
                on_motion_fault,
                on_safety_fault,
                ..
            } => {
                if let Some(timeout) = timeout {
                    push_target_state_key(
                        &mut out,
                        &timeout.target_task,
                        &timeout.target_step,
                        task_entry_states,
                        known_state_keys,
                    );
                }
                if let Some(branch) = on_motion_fault {
                    push_target_state_key(
                        &mut out,
                        &branch.target_task,
                        &branch.target_step,
                        task_entry_states,
                        known_state_keys,
                    );
                }
                if let Some(branch) = on_safety_fault {
                    push_target_state_key(
                        &mut out,
                        &branch.target_task,
                        &branch.target_step,
                        task_entry_states,
                        known_state_keys,
                    );
                }
            }
            TransitionAction::AxisMoveRelative {
                timeout,
                on_reject,
                on_motion_fault,
                on_safety_fault,
                on_reject_routes,
                on_motion_fault_routes,
                on_safety_fault_routes,
                ..
            }
            | TransitionAction::AxisMoveAbsolute {
                timeout,
                on_reject,
                on_motion_fault,
                on_safety_fault,
                on_reject_routes,
                on_motion_fault_routes,
                on_safety_fault_routes,
                ..
            } => {
                push_target_state_key(
                    &mut out,
                    &timeout.target_task,
                    &timeout.target_step,
                    task_entry_states,
                    known_state_keys,
                );
                push_target_state_key(
                    &mut out,
                    &on_reject.target_task,
                    &on_reject.target_step,
                    task_entry_states,
                    known_state_keys,
                );
                push_target_state_key(
                    &mut out,
                    &on_motion_fault.target_task,
                    &on_motion_fault.target_step,
                    task_entry_states,
                    known_state_keys,
                );
                push_target_state_key(
                    &mut out,
                    &on_safety_fault.target_task,
                    &on_safety_fault.target_step,
                    task_entry_states,
                    known_state_keys,
                );
                for route in on_reject_routes {
                    push_target_state_key(
                        &mut out,
                        &route.target_task,
                        &route.target_step,
                        task_entry_states,
                        known_state_keys,
                    );
                }
                for route in on_motion_fault_routes {
                    push_target_state_key(
                        &mut out,
                        &route.target_task,
                        &route.target_step,
                        task_entry_states,
                        known_state_keys,
                    );
                }
                for route in on_safety_fault_routes {
                    push_target_state_key(
                        &mut out,
                        &route.target_task,
                        &route.target_step,
                        task_entry_states,
                        known_state_keys,
                    );
                }
            }
            _ => {}
        }
    }
    out
}

fn push_target_state_key(
    out: &mut Vec<(String, String)>,
    target_task: &str,
    target_step: &Option<String>,
    task_entry_states: &HashMap<String, State>,
    known_state_keys: &HashSet<(String, String)>,
) {
    if let Some(step) = target_step {
        let key = (target_task.to_string(), step.clone());
        if known_state_keys.contains(&key) {
            out.push(key);
        }
        return;
    }

    if let Some(entry_state) = task_entry_states.get(target_task) {
        out.push((entry_state.task_name.clone(), entry_state.step_name.clone()));
    }
}

fn render_action_kind(action: &TransitionAction) -> String {
    match action {
        TransitionAction::Extend { .. } => "extend".to_string(),
        TransitionAction::Retract { .. } => "retract".to_string(),
        TransitionAction::Set { .. } => "set".to_string(),
        TransitionAction::SetAnalog { .. } => "set_analog".to_string(),
        TransitionAction::SetAnalogExpr { .. } => "set_analog_expr".to_string(),
        TransitionAction::Compute { .. } => "compute".to_string(),
        TransitionAction::CallExtern { .. } => "call_extern".to_string(),
        TransitionAction::CamEngage { .. } => "cam_engage".to_string(),
        TransitionAction::CamDisengage { .. } => "cam_disengage".to_string(),
        TransitionAction::CamSwitch { .. } => "cam_switch".to_string(),
        TransitionAction::CamPhase { .. } => "cam_phase".to_string(),
        TransitionAction::AxisMoveRelative { .. } => "axis_move_relative".to_string(),
        TransitionAction::AxisMoveAbsolute { .. } => "axis_move_absolute".to_string(),
        TransitionAction::Log { .. } => "log".to_string(),
    }
}

fn render_workpiece_effect(effect: &WorkpieceEffect) -> String {
    match effect {
        WorkpieceEffect::Acquire { holder, from } => format!("acquire {holder} from {from}"),
        WorkpieceEffect::Transfer { from, to } => format!("transfer {from} -> {to}"),
        WorkpieceEffect::Finish { at, terminal_state } => {
            format!("finish {at} as {terminal_state}")
        }
        WorkpieceEffect::Mount {
            workpiece_type,
            slot,
        } => format!("mount {workpiece_type} at {slot}"),
        WorkpieceEffect::Unmount {
            workpiece_type,
            slot,
            to,
        } => format!("unmount {workpiece_type} from {slot} to {to}"),
        WorkpieceEffect::Split {
            source_type,
            target_type,
            count,
            ..
        } => format!("split {source_type} -> {target_type} x{count}"),
        WorkpieceEffect::Merge {
            target_type,
            inputs,
            ..
        } => format!("merge [{}] -> {target_type}", inputs.join(",")),
        WorkpieceEffect::TransformCarrier { carrier, frame } => {
            format!("transform carrier {carrier} -> {frame}")
        }
    }
}

fn transition_key(event: &NormalizedTraceEvent) -> String {
    format!(
        "task={};from={};to={};reason={}",
        event.task, event.from_step, event.to_step, event.reason
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent_alignment::compile_expected_behavior_spec;
    use crate::intent_alignment::contract::parse_intent_contract_str;
    use crate::ir::{BinaryValue, TaskExecutionContext, Transition, TransitionGuard};

    fn state(task: &str, step: &str) -> State {
        State {
            task_name: task.to_string(),
            step_name: step.to_string(),
        }
    }

    fn task_ctx(task: &str, step: &str) -> TaskExecutionContext {
        TaskExecutionContext {
            task_name: task.to_string(),
            entry_state: state(task, step),
            current_state: state(task, step),
            ..Default::default()
        }
    }

    fn synthetic_state_machine() -> StateMachine {
        StateMachine {
            states: vec![
                state("cycle", "start"),
                state("cycle", "pick"),
                state("cycle", "handoff"),
                state("cycle", "done"),
            ],
            transitions: vec![
                Transition {
                    from: state("cycle", "start"),
                    to: state("cycle", "pick"),
                    guard: TransitionGuard::Always,
                    actions: vec![TransitionAction::Set {
                        target: "Y0".to_string(),
                        port: "self".to_string(),
                        value: BinaryValue::On,
                    }],
                    effects: Vec::new(),
                    timers: Vec::new(),
                },
                Transition {
                    from: state("cycle", "pick"),
                    to: state("cycle", "handoff"),
                    guard: TransitionGuard::Always,
                    actions: Vec::new(),
                    effects: vec![WorkpieceEffect::Acquire {
                        holder: "arm".to_string(),
                        from: "infeed".to_string(),
                    }],
                    timers: Vec::new(),
                },
                Transition {
                    from: state("cycle", "handoff"),
                    to: state("cycle", "done"),
                    guard: TransitionGuard::Always,
                    actions: Vec::new(),
                    effects: vec![
                        WorkpieceEffect::Transfer {
                            from: "arm".to_string(),
                            to: "outfeed".to_string(),
                        },
                        WorkpieceEffect::Finish {
                            at: "outfeed".to_string(),
                            terminal_state: "finished".to_string(),
                        },
                    ],
                    timers: Vec::new(),
                },
            ],
            initial: state("cycle", "start"),
            analog_regions: Default::default(),
            task_contexts: vec![task_ctx("cycle", "start")],
        }
    }

    #[test]
    fn doctor_prefers_unique_workpiece_transitions_as_anchor_candidates() {
        let report = diagnose_intent_alignment(
            &synthetic_state_machine(),
            &[
                NormalizedTraceEvent {
                    tick: 0,
                    task: 0,
                    from_step: 0,
                    to_step: 1,
                    reason: "action".to_string(),
                },
                NormalizedTraceEvent {
                    tick: 1,
                    task: 0,
                    from_step: 1,
                    to_step: 2,
                    reason: "wait_satisfied".to_string(),
                },
                NormalizedTraceEvent {
                    tick: 2,
                    task: 0,
                    from_step: 2,
                    to_step: 3,
                    reason: "action".to_string(),
                },
            ],
            None,
        )
        .expect("doctor report");

        assert_eq!(report.unique_transition_count, 3);
        let top = report.candidates.first().expect("top candidate");
        assert!(
            !top.workpiece_effects.is_empty(),
            "top candidate should expose workpiece semantics"
        );
        assert!(
            top.reasons
                .iter()
                .any(|reason| reason == "has_workpiece_semantics"),
            "top candidate should be promoted by workpiece semantics"
        );
    }

    #[test]
    fn doctor_flags_repeated_binding_and_trailing_partial_cycle() {
        let contract = parse_intent_contract_str(
            r#"{
  "contract_version": "phase-2.v1",
  "source_ref": { "kind": "authored_asset", "path": "tests/fake.system.md", "description": "synthetic" },
  "source_digest": { "algorithm": "sha256", "value": "deadbeef" },
  "metadata": {
    "contract_id": "synthetic-intent",
    "title": "synthetic",
    "business_owner": "tests",
    "authoritative_intent_source": { "kind": "authored_asset", "path": "tests/fake.system.md", "description": "synthetic" },
    "review_basis": [
      { "label": "synthetic", "source": { "kind": "authored_asset", "path": "tests/fake.system.md", "description": "synthetic" } }
    ]
  },
  "contract_core": {
    "expected_milestones": [
      { "milestone_id": "cycle_started", "business_milestone": { "label": "start", "description": "start" } },
      { "milestone_id": "wafer_handed_off", "business_milestone": { "label": "handoff", "description": "handoff" } }
    ],
    "required_edges": [{ "predecessor": "cycle_started", "successor": "wafer_handed_off" }],
    "postconditions": [],
    "cycle_semantics": {
      "cycle_start_milestone": "cycle_started",
      "cycle_complete_milestone": "wafer_handed_off",
      "restart_semantics": {
        "restartable_milestone": "wafer_handed_off",
        "next_cycle_start_milestone": "cycle_started",
        "required_postconditions": []
      }
    }
  },
  "observation_bindings": [
    {
      "binding_id": "cycle_started_binding",
      "subject": { "kind": "milestone", "milestone_id": "cycle_started" },
      "combination": "all_of",
      "evidence": [{ "source": "trace_event", "key": "transition", "expected": "task=0;from=0;to=1;reason=action" }]
    },
    {
      "binding_id": "handoff_binding",
      "subject": { "kind": "milestone", "milestone_id": "wafer_handed_off" },
      "combination": "all_of",
      "evidence": [{ "source": "trace_event", "key": "transition", "expected": "task=0;from=2;to=3;reason=action" }]
    }
  ]
}"#,
        )
        .expect("contract");
        let spec = compile_expected_behavior_spec(&contract).expect("spec");
        let report = diagnose_intent_alignment(
            &synthetic_state_machine(),
            &[
                NormalizedTraceEvent {
                    tick: 0,
                    task: 0,
                    from_step: 0,
                    to_step: 1,
                    reason: "action".to_string(),
                },
                NormalizedTraceEvent {
                    tick: 1,
                    task: 0,
                    from_step: 0,
                    to_step: 1,
                    reason: "action".to_string(),
                },
                NormalizedTraceEvent {
                    tick: 2,
                    task: 0,
                    from_step: 1,
                    to_step: 2,
                    reason: "wait_satisfied".to_string(),
                },
                NormalizedTraceEvent {
                    tick: 3,
                    task: 0,
                    from_step: 2,
                    to_step: 3,
                    reason: "action".to_string(),
                },
                NormalizedTraceEvent {
                    tick: 4,
                    task: 0,
                    from_step: 0,
                    to_step: 1,
                    reason: "action".to_string(),
                },
            ],
            Some(&spec),
        )
        .expect("doctor report");

        let contract = report.contract_diagnosis.expect("contract diagnosis");
        let start = contract
            .milestone_bindings
            .iter()
            .find(|binding| binding.binding_id == "cycle_started_binding")
            .expect("cycle start binding");
        assert_eq!(start.status, BindingStability::Repeated);

        let cycle = report.cycle_diagnosis.expect("cycle diagnosis");
        assert!(cycle.trailing_partial_cycle);
    }
}
