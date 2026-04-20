use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use super::contract::{ObservationCombination, ObservationSubject};
use super::expected_behavior::ExpectedBehaviorSpec;
use super::observed::{
    ObservedBehaviorSequence, ObservedComparisonDimension, ObservedEventSourceKind,
    ObservedEvidenceEntry, ObservedEvidenceGap, ObservedTraceParseError,
    extract_observed_behavior_sequence, parse_observed_trace_jsonl,
};
use super::report::{
    INTENT_ALIGNMENT_COMPARATOR_VERSION, IntentAlignmentBlockerKind,
    IntentAlignmentContractIdentity, IntentAlignmentCycleWindow, IntentAlignmentEvidenceIdentity,
    IntentAlignmentEvidenceKind, IntentAlignmentReport, IntentAlignmentVerdict, IntentMismatch,
    IntentMismatchKind,
};

#[derive(Debug, Error)]
pub enum IntentAlignmentCompareInputError {
    #[error(transparent)]
    Parse(#[from] ObservedTraceParseError),
    #[error(transparent)]
    Gap(#[from] ObservedEvidenceGap),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MatchedOccurrence {
    milestone_id: String,
    cycle_index: usize,
    tick: u64,
    evidence_indices: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RawMatchedOccurrence {
    milestone_id: String,
    cycle_index: usize,
    tick: u64,
    evidence_indices: Vec<usize>,
}

pub fn compare_trace_jsonl(
    spec: &ExpectedBehaviorSpec,
    trace_jsonl: &str,
) -> Result<IntentAlignmentReport, IntentAlignmentCompareInputError> {
    let raw_events = parse_observed_trace_jsonl(trace_jsonl)?;
    let observed = extract_observed_behavior_sequence(spec, &raw_events)?;
    Ok(compare_intent_alignment_with_identity(
        spec,
        &observed,
        IntentAlignmentEvidenceIdentity {
            kind: IntentAlignmentEvidenceKind::InlineTraceJsonl,
            label: "inline_trace_jsonl".to_string(),
        },
    ))
}

pub fn compare_intent_alignment(
    spec: &ExpectedBehaviorSpec,
    observed: &ObservedBehaviorSequence,
) -> IntentAlignmentReport {
    compare_intent_alignment_with_identity(
        spec,
        observed,
        IntentAlignmentEvidenceIdentity {
            kind: IntentAlignmentEvidenceKind::ObservedSequence,
            label: "observed_sequence".to_string(),
        },
    )
}

fn compare_intent_alignment_with_identity(
    spec: &ExpectedBehaviorSpec,
    observed: &ObservedBehaviorSequence,
    evidence_identity: IntentAlignmentEvidenceIdentity,
) -> IntentAlignmentReport {
    if let Some(blocked_reason) = blocked_reason(observed) {
        return build_report(
            spec,
            observed,
            observed.cycle_count.max(1),
            evidence_identity,
            IntentAlignmentVerdict::Blocked,
            None,
            Vec::new(),
            Some(blocked_reason),
            Some(IntentAlignmentBlockerKind::MissingEvidence),
            Vec::new(),
        );
    }

    let matches = matched_occurrences(spec, observed);
    let consumed_indices: BTreeSet<usize> = matches
        .iter()
        .flat_map(|matched| matched.evidence_indices.iter().copied())
        .collect();
    let milestone_order: BTreeMap<(usize, String), usize> = matches
        .iter()
        .enumerate()
        .map(|(order, matched)| ((matched.cycle_index, matched.milestone_id.clone()), order))
        .collect();
    let expected_ids: Vec<&str> = spec
        .expected_milestones
        .iter()
        .map(|milestone| milestone.milestone_id.as_str())
        .collect();
    let mut mismatches = Vec::new();

    let cycle_count = effective_cycle_count(observed, &matches).max(1);
    for cycle_index in 0..cycle_count {
        let cycle_matches: Vec<&MatchedOccurrence> = matches
            .iter()
            .filter(|matched| matched.cycle_index == cycle_index)
            .collect();

        let counts = cycle_matches
            .iter()
            .fold(BTreeMap::new(), |mut acc, matched| {
                *acc.entry(matched.milestone_id.as_str()).or_insert(0usize) += 1;
                acc
            });

        for expected_id in &expected_ids {
            match counts.get(expected_id) {
                None => mismatches.push(IntentMismatch {
                    kind: IntentMismatchKind::MissingRequiredStep,
                    subject: (*expected_id).to_string(),
                    detail: "required milestone did not appear in observed behavior".to_string(),
                    cycle_index: Some(cycle_index),
                }),
                Some(count) if *count > 1 => mismatches.push(IntentMismatch {
                    kind: IntentMismatchKind::DuplicatedRequiredStep,
                    subject: (*expected_id).to_string(),
                    detail: format!(
                        "required milestone appeared {count} times within the same cycle"
                    ),
                    cycle_index: Some(cycle_index),
                }),
                _ => {}
            }
        }

        for edge in &spec.required_edges {
            let predecessor = milestone_order
                .get(&(cycle_index, edge.predecessor.clone()))
                .copied();
            let successor = milestone_order
                .get(&(cycle_index, edge.successor.clone()))
                .copied();
            if let (Some(predecessor), Some(successor)) = (predecessor, successor) {
                if predecessor >= successor {
                    mismatches.push(IntentMismatch {
                        kind: IntentMismatchKind::WrongOrder,
                        subject: format!("{} -> {}", edge.predecessor, edge.successor),
                        detail: "required edge order was violated in observed behavior".to_string(),
                        cycle_index: Some(cycle_index),
                    });
                }
            }
        }

        if cycle_matches.iter().any(|matched| {
            matched.milestone_id == spec.cycle_semantics.restartability.restartable_milestone
        }) {
            let missing_before_restartable: Vec<&str> = expected_ids
                .iter()
                .copied()
                .filter(|expected_id| !counts.contains_key(expected_id))
                .filter(|expected_id| {
                    *expected_id != spec.cycle_semantics.restartability.restartable_milestone
                })
                .collect();
            if !missing_before_restartable.is_empty() {
                mismatches.push(IntentMismatch {
                    kind: IntentMismatchKind::PrematureReadiness,
                    subject: spec
                        .cycle_semantics
                        .restartability
                        .restartable_milestone
                        .clone(),
                    detail: format!(
                        "restartable milestone was observed before recovery path closed over required milestones: {}",
                        missing_before_restartable.join(", ")
                    ),
                    cycle_index: Some(cycle_index),
                });
            }
        }
    }

    for (index, entry) in observed.evidence.iter().enumerate() {
        if !consumed_indices.contains(&index) && should_flag_unexpected_entry(entry) {
            mismatches.push(IntentMismatch {
                kind: IntentMismatchKind::UnexpectedObservedStep,
                subject: format!("{}={}", entry.key, entry.expected),
                detail: "observed evidence did not match any declared observation binding"
                    .to_string(),
                cycle_index: Some(entry.cycle_index),
            });
        }
    }

    mismatches.extend(evaluate_postconditions(spec, observed));
    mismatches.extend(detect_cross_cycle_drift(spec, observed, &mismatches));

    mismatches.sort_by(|left, right| {
        left.kind
            .priority()
            .cmp(&right.kind.priority())
            .then_with(|| left.cycle_index.cmp(&right.cycle_index))
            .then_with(|| left.subject.cmp(&right.subject))
            .then_with(|| left.detail.cmp(&right.detail))
    });
    mismatches.dedup();

    let verdict = if mismatches.is_empty() {
        IntentAlignmentVerdict::Aligned
    } else {
        IntentAlignmentVerdict::Mismatch
    };

    let warnings = observed
        .readiness
        .iter()
        .filter(|readiness| !readiness.ready)
        .filter_map(|readiness| {
            readiness
                .gap
                .as_ref()
                .map(|gap| format!("{:?}: {}", readiness.dimension, gap.detail))
        })
        .collect();

    build_report(
        spec,
        observed,
        cycle_count,
        evidence_identity,
        verdict,
        mismatches.first().cloned(),
        mismatches,
        None,
        None,
        warnings,
    )
}

fn evaluate_postconditions(
    spec: &ExpectedBehaviorSpec,
    observed: &ObservedBehaviorSequence,
) -> Vec<IntentMismatch> {
    let groups = grouped_entries(observed);
    let mut mismatches = Vec::new();
    let cycle_count = observed.cycle_count.max(1);

    for cycle_index in 0..cycle_count {
        let cycle_window = observed
            .cycles
            .iter()
            .find(|cycle| cycle.cycle_index == cycle_index);
        for predicate in &spec.postcondition_predicates {
            let satisfied = match &predicate.predicate {
                super::expected_behavior::PredicateExpr::AllOf(facts) => {
                    exact_transition_fact_cycle_indices(observed, facts)
                        .is_some_and(|cycles| cycles.contains(&cycle_index))
                        || groups
                            .iter()
                            .filter(|((group_cycle_index, _), _)| *group_cycle_index == cycle_index)
                            .any(|(_, indices)| {
                                facts
                                    .iter()
                                    .all(|fact| group_has_fact(observed, indices, fact))
                            })
                        || cycle_window.is_some_and(|window| {
                            snapshot_has_all_facts(
                                window.successful_cycle_end_snapshot.as_ref(),
                                facts,
                            )
                        })
                }
                super::expected_behavior::PredicateExpr::AnyOf(facts) => {
                    exact_transition_fact_cycle_indices(observed, facts)
                        .is_some_and(|cycles| cycles.contains(&cycle_index))
                        || groups
                            .iter()
                            .filter(|((group_cycle_index, _), _)| *group_cycle_index == cycle_index)
                            .any(|(_, indices)| {
                                facts
                                    .iter()
                                    .any(|fact| group_has_fact(observed, indices, fact))
                            })
                        || cycle_window.is_some_and(|window| {
                            snapshot_has_any_fact(
                                window.successful_cycle_end_snapshot.as_ref(),
                                facts,
                            )
                        })
                }
                super::expected_behavior::PredicateExpr::OrderedAllOf(facts) => {
                    exact_transition_fact_cycle_indices(observed, facts)
                        .is_some_and(|cycles| cycles.contains(&cycle_index))
                        || {
                            let cycle_groups: Vec<(&(usize, u64), &Vec<usize>)> = groups
                                .iter()
                                .filter(|((group_cycle_index, _), _)| *group_cycle_index == cycle_index)
                                .collect();
                            let mut next_fact = 0usize;
                            for (_, indices) in cycle_groups {
                                if next_fact >= facts.len() {
                                    break;
                                }
                                if group_has_fact(observed, indices, &facts[next_fact]) {
                                    next_fact += 1;
                                }
                            }
                            next_fact == facts.len()
                        }
                }
            };

            if !satisfied {
                mismatches.push(IntentMismatch {
                    kind: IntentMismatchKind::PostconditionNotMet,
                    subject: predicate.postcondition_id.clone(),
                    detail: "cycle finished without satisfying required postcondition predicate"
                        .to_string(),
                    cycle_index: Some(cycle_index),
                });
            }
        }
    }

    mismatches
}

fn snapshot_has_all_facts(
    snapshot: Option<&BTreeMap<String, String>>,
    facts: &[super::expected_behavior::ObservedFact],
) -> bool {
    let Some(snapshot) = snapshot else {
        return false;
    };
    facts
        .iter()
        .all(|fact| snapshot.get(&fact.key) == Some(&fact.expected))
}

fn snapshot_has_any_fact(
    snapshot: Option<&BTreeMap<String, String>>,
    facts: &[super::expected_behavior::ObservedFact],
) -> bool {
    let Some(snapshot) = snapshot else {
        return false;
    };
    facts
        .iter()
        .any(|fact| snapshot.get(&fact.key) == Some(&fact.expected))
}

fn blocked_reason(observed: &ObservedBehaviorSequence) -> Option<String> {
    observed
        .readiness
        .iter()
        .find(|readiness| {
            !readiness.ready
                && matches!(
                    readiness.dimension,
                    ObservedComparisonDimension::RequiredStep
                        | ObservedComparisonDimension::Ordering
                )
        })
        .and_then(|readiness| readiness.gap.as_ref().map(|gap| gap.detail.clone()))
}

fn should_flag_unexpected_entry(entry: &ObservedEvidenceEntry) -> bool {
    !matches!(entry.source, ObservedEventSourceKind::TraceTransition)
}

fn exact_transition_fact_cycle_indices(
    observed: &ObservedBehaviorSequence,
    facts: &[super::expected_behavior::ObservedFact],
) -> Option<BTreeSet<usize>> {
    if facts.len() != 1 || facts[0].key != "transition" {
        return None;
    }
    Some(
        observed
            .evidence
            .iter()
            .filter(|entry| entry.key == "transition" && entry.expected == facts[0].expected)
            .enumerate()
            .map(|(cycle_index, _)| cycle_index)
            .collect(),
    )
}

fn detect_cross_cycle_drift(
    spec: &ExpectedBehaviorSpec,
    observed: &ObservedBehaviorSequence,
    single_cycle_mismatches: &[IntentMismatch],
) -> Vec<IntentMismatch> {
    if observed.cycles.len() < 2 {
        return Vec::new();
    }
    if !handoff_requires_snapshot_evidence(spec) {
        return Vec::new();
    }

    let mut mismatches = Vec::new();
    for window in observed.cycles.windows(2) {
        let previous = &window[0];
        let next = &window[1];
        if cycle_has_non_cross_cycle_mismatch(single_cycle_mismatches, previous.cycle_index)
            || cycle_has_non_cross_cycle_mismatch(single_cycle_mismatches, next.cycle_index)
        {
            continue;
        }

        let terminal_snapshot_facts = snapshot_evaluable_facts(
            &spec
                .cycle_semantics
                .handoff_invariant
                .required_terminal_facts,
        );
        let next_start_snapshot_facts = snapshot_evaluable_facts(
            &spec
                .cycle_semantics
                .handoff_invariant
                .required_next_cycle_start_facts,
        );
        let handoff_snapshot_evaluable =
            !terminal_snapshot_facts.is_empty() || !next_start_snapshot_facts.is_empty();
        if !handoff_snapshot_evaluable {
            continue;
        }

        let missing_terminal = previous
            .successful_cycle_end_snapshot
            .as_ref()
            .map(|snapshot| {
                missing_snapshot_facts(
                    snapshot,
                    &terminal_snapshot_facts,
                )
            })
            .unwrap_or_default();
        let missing_next_start = next
            .first_observed_snapshot
            .as_ref()
            .map(|snapshot| {
                missing_snapshot_facts(
                    snapshot,
                    &next_start_snapshot_facts,
                )
            })
            .unwrap_or_default();

        let handoff_tick_ready = previous
            .successful_cycle_end_tick
            .is_some_and(|tick| next.start_tick > tick);

        if previous.successful_cycle_end_snapshot.is_some()
            && next.first_observed_snapshot.is_some()
            && missing_terminal.is_empty()
            && missing_next_start.is_empty()
            && handoff_tick_ready
        {
            continue;
        }

        let mut detail_parts = Vec::new();
        if previous.successful_cycle_end_snapshot.is_none() {
            detail_parts.push(format!(
                "cycle {} missing successful-cycle-end snapshot",
                previous.cycle_index
            ));
        } else if !missing_terminal.is_empty() {
            detail_parts.push(format!(
                "cycle {} terminal facts missing [{}]",
                previous.cycle_index,
                missing_terminal.join(", ")
            ));
        }

        if next.first_observed_snapshot.is_none() {
            detail_parts.push(format!(
                "cycle {} missing first-observed handoff snapshot",
                next.cycle_index
            ));
        } else if !missing_next_start.is_empty() {
            detail_parts.push(format!(
                "cycle {} first handoff snapshot violated next-cycle start facts [{}]",
                next.cycle_index,
                missing_next_start.join(", ")
            ));
        }
        if let Some(previous_end_tick) = previous.successful_cycle_end_tick {
            if next.start_tick <= previous_end_tick {
                detail_parts.push(format!(
                    "cycle {} restarted at tick {} before handoff window advanced beyond previous terminal tick {}",
                    next.cycle_index, next.start_tick, previous_end_tick
                ));
            }
        }

        mismatches.push(IntentMismatch {
            kind: IntentMismatchKind::CrossCycleDrift,
            subject: format!("cycle {} -> {}", previous.cycle_index, next.cycle_index),
            detail: detail_parts.join("; "),
            cycle_index: Some(next.cycle_index),
        });
    }

    mismatches
}

fn handoff_requires_snapshot_evidence(spec: &ExpectedBehaviorSpec) -> bool {
    spec.cycle_semantics
        .handoff_invariant
        .required_terminal_facts
        .iter()
        .chain(
            spec.cycle_semantics
                .handoff_invariant
                .required_next_cycle_start_facts
                .iter(),
        )
        .any(|fact| fact.key != "transition")
}

fn cycle_has_non_cross_cycle_mismatch(mismatches: &[IntentMismatch], cycle_index: usize) -> bool {
    mismatches.iter().any(|mismatch| {
        mismatch.cycle_index == Some(cycle_index)
            && mismatch.kind != IntentMismatchKind::CrossCycleDrift
    })
}

fn missing_snapshot_facts(
    snapshot: &BTreeMap<String, String>,
    facts: &[super::expected_behavior::ObservedFact],
) -> Vec<String> {
    facts
        .iter()
        .filter(|fact| snapshot.get(&fact.key) != Some(&fact.expected))
        .map(|fact| format!("{}={}", fact.key, fact.expected))
        .collect()
}

fn snapshot_evaluable_facts(
    facts: &[super::expected_behavior::ObservedFact],
) -> Vec<super::expected_behavior::ObservedFact> {
    facts.iter()
        .filter(|fact| fact.key.starts_with("vars."))
        .cloned()
        .collect()
}

fn build_report(
    spec: &ExpectedBehaviorSpec,
    observed: &ObservedBehaviorSequence,
    effective_cycle_count: usize,
    evidence_identity: IntentAlignmentEvidenceIdentity,
    verdict: IntentAlignmentVerdict,
    primary_mismatch: Option<IntentMismatch>,
    mismatches: Vec<IntentMismatch>,
    blocked_reason: Option<String>,
    blocker_kind: Option<IntentAlignmentBlockerKind>,
    warnings: Vec<String>,
) -> IntentAlignmentReport {
    IntentAlignmentReport {
        contract_identity: IntentAlignmentContractIdentity {
            contract_id: spec.contract_id.clone(),
            contract_version: spec.contract_version.clone(),
        },
        evidence_identity,
        comparator_version: INTENT_ALIGNMENT_COMPARATOR_VERSION.to_string(),
        cycle_window: cycle_window(observed, effective_cycle_count),
        verdict,
        primary_mismatch,
        mismatches,
        blocked_reason,
        blocker_kind,
        warnings,
    }
}

fn cycle_window(
    observed: &ObservedBehaviorSequence,
    effective_cycle_count: usize,
) -> IntentAlignmentCycleWindow {
    let cycle_count = effective_cycle_count.max(observed.cycle_count);
    let first_cycle_index = if cycle_count > 0 { 0 } else { 0 };
    let last_cycle_index = cycle_count.saturating_sub(1);
    IntentAlignmentCycleWindow {
        first_cycle_index,
        last_cycle_index,
        cycle_count,
    }
}

fn matched_occurrences(
    spec: &ExpectedBehaviorSpec,
    observed: &ObservedBehaviorSequence,
) -> Vec<MatchedOccurrence> {
    let groups = grouped_entries(observed);
    let milestone_rank = milestone_rank(spec);
    let mut raw_matches = Vec::new();

    for binding in &spec.observation_bindings {
        let ObservationSubject::Milestone { milestone_id } = &binding.subject else {
            continue;
        };

        if milestone_id == &spec.cycle_semantics.cycle_start_milestone {
            let mut used_cycle_snapshots = false;
            for cycle in &observed.cycles {
                if cycle.cycle_start_snapshot.is_none() {
                    continue;
                }
                used_cycle_snapshots = true;
                if snapshot_matches_binding(cycle.cycle_start_snapshot.as_ref(), binding) {
                    raw_matches.push(RawMatchedOccurrence {
                        milestone_id: milestone_id.clone(),
                        cycle_index: cycle.cycle_index,
                        tick: cycle.start_tick,
                        evidence_indices: groups
                            .get(&(cycle.cycle_index, cycle.start_tick))
                            .cloned()
                            .unwrap_or_default(),
                    });
                }
            }
            if used_cycle_snapshots {
                continue;
            }
        }

        if milestone_id == &spec.cycle_semantics.successful_cycle_end_milestone {
            let mut used_cycle_snapshots = false;
            for cycle in &observed.cycles {
                let Some(tick) = cycle.successful_cycle_end_tick else {
                    continue;
                };
                if cycle.successful_cycle_end_snapshot.is_none() {
                    continue;
                }
                used_cycle_snapshots = true;
                if snapshot_matches_binding(cycle.successful_cycle_end_snapshot.as_ref(), binding) {
                    raw_matches.push(RawMatchedOccurrence {
                        milestone_id: milestone_id.clone(),
                        cycle_index: cycle.cycle_index,
                        tick,
                        evidence_indices: groups
                            .get(&(cycle.cycle_index, tick))
                            .cloned()
                            .unwrap_or_default(),
                    });
                }
            }
            if used_cycle_snapshots {
                continue;
            }
        }

        match binding.combination {
            ObservationCombination::AllOf => {
                if exact_transition_binding_uses_occurrence_order(binding, observed) {
                    let mut occurrence_index = 0usize;
                    for ((_, tick), indices) in &groups {
                        if binding
                            .evidence
                            .iter()
                            .all(|expected| group_has_evidence(observed, indices, expected))
                        {
                            raw_matches.push(RawMatchedOccurrence {
                                milestone_id: milestone_id.clone(),
                                cycle_index: occurrence_index,
                                tick: *tick,
                                evidence_indices: indices.clone(),
                            });
                            occurrence_index += 1;
                        }
                    }
                    continue;
                }

                for ((cycle_index, tick), indices) in &groups {
                    if binding
                        .evidence
                        .iter()
                        .all(|expected| group_has_evidence(observed, indices, expected))
                    {
                        raw_matches.push(RawMatchedOccurrence {
                            milestone_id: milestone_id.clone(),
                            cycle_index: *cycle_index,
                            tick: *tick,
                            evidence_indices: indices.clone(),
                        });
                    }
                }
            }
            ObservationCombination::AnyOf => {
                for ((cycle_index, tick), indices) in &groups {
                    if binding
                        .evidence
                        .iter()
                        .any(|expected| group_has_evidence(observed, indices, expected))
                    {
                        raw_matches.push(RawMatchedOccurrence {
                            milestone_id: milestone_id.clone(),
                            cycle_index: *cycle_index,
                            tick: *tick,
                            evidence_indices: indices.clone(),
                        });
                    }
                }
            }
            ObservationCombination::OrderedAllOf => {
                let cycle_indices: BTreeSet<usize> =
                    groups.keys().map(|(cycle_index, _)| *cycle_index).collect();
                for cycle_index in cycle_indices {
                    let cycle_groups: Vec<(&(usize, u64), &Vec<usize>)> = groups
                        .iter()
                        .filter(|((group_cycle_index, _), _)| *group_cycle_index == cycle_index)
                        .collect();
                    let mut matched_indices = Vec::new();
                    let mut next_expected = 0usize;
                    let mut last_tick = None;
                    for ((_, tick), indices) in cycle_groups {
                        if next_expected >= binding.evidence.len() {
                            break;
                        }
                        if group_has_evidence(observed, indices, &binding.evidence[next_expected]) {
                            matched_indices.extend(indices.iter().copied());
                            next_expected += 1;
                            last_tick = Some(*tick);
                        }
                    }
                    if next_expected == binding.evidence.len() {
                        raw_matches.push(RawMatchedOccurrence {
                            milestone_id: milestone_id.clone(),
                            cycle_index,
                            tick: last_tick.unwrap_or(0),
                            evidence_indices: matched_indices,
                        });
                    }
                }
            }
        }
    }

    raw_matches.sort_by(|left, right| {
        left.cycle_index
            .cmp(&right.cycle_index)
            .then_with(|| left.tick.cmp(&right.tick))
            .then_with(|| {
                milestone_rank
                    .get(&left.milestone_id)
                    .copied()
                    .unwrap_or(usize::MAX)
                    .cmp(
                        &milestone_rank
                            .get(&right.milestone_id)
                            .copied()
                            .unwrap_or(usize::MAX),
                    )
            })
            .then_with(|| left.milestone_id.cmp(&right.milestone_id))
    });

    let mut matches = raw_matches
        .into_iter()
        .map(|matched| {
            MatchedOccurrence {
                milestone_id: matched.milestone_id,
                cycle_index: matched.cycle_index,
                tick: matched.tick,
                evidence_indices: matched.evidence_indices,
            }
        })
        .collect::<Vec<_>>();

    matches.sort_by(|left, right| {
        left.cycle_index
            .cmp(&right.cycle_index)
            .then_with(|| left.tick.cmp(&right.tick))
            .then_with(|| left.milestone_id.cmp(&right.milestone_id))
    });
    matches
}

fn effective_cycle_count(
    observed: &ObservedBehaviorSequence,
    matches: &[MatchedOccurrence],
) -> usize {
    observed.cycle_count.max(
        matches
            .iter()
            .map(|matched| matched.cycle_index + 1)
            .max()
            .unwrap_or(0),
    )
}

fn milestone_rank(spec: &ExpectedBehaviorSpec) -> BTreeMap<String, usize> {
    spec.expected_milestones
        .iter()
        .enumerate()
        .map(|(idx, milestone)| (milestone.milestone_id.clone(), idx))
        .collect()
}

fn grouped_entries(observed: &ObservedBehaviorSequence) -> BTreeMap<(usize, u64), Vec<usize>> {
    let mut groups = BTreeMap::new();
    for (index, entry) in observed.evidence.iter().enumerate() {
        groups
            .entry((entry.cycle_index, entry.tick))
            .or_insert_with(Vec::new)
            .push(index);
    }
    groups
}

fn exact_transition_binding_uses_occurrence_order(
    binding: &super::contract::ObservationBinding,
    observed: &ObservedBehaviorSequence,
) -> bool {
    binding.evidence.len() == 1
        && binding.evidence[0].key == "transition"
        && matches!(binding.combination, ObservationCombination::AllOf)
        && observed_cycles_overlap(observed)
}

fn observed_cycles_overlap(observed: &ObservedBehaviorSequence) -> bool {
    observed
        .cycles
        .windows(2)
        .any(|pair| pair[1].start_tick <= pair[0].end_tick)
}

fn group_has_evidence(
    observed: &ObservedBehaviorSequence,
    indices: &[usize],
    expected: &super::contract::ObservedEvidence,
) -> bool {
    indices.iter().any(|index| {
        let entry: &ObservedEvidenceEntry = &observed.evidence[*index];
        entry.key == expected.key && entry.expected == expected.expected
    }) || group_snapshot_has_fact(observed, indices, &expected.key, &expected.expected)
}

fn group_has_fact(
    observed: &ObservedBehaviorSequence,
    indices: &[usize],
    expected: &super::expected_behavior::ObservedFact,
) -> bool {
    indices.iter().any(|index| {
        let entry: &ObservedEvidenceEntry = &observed.evidence[*index];
        entry.key == expected.key && entry.expected == expected.expected
    }) || group_snapshot_has_fact(observed, indices, &expected.key, &expected.expected)
}

fn group_snapshot_has_fact(
    observed: &ObservedBehaviorSequence,
    indices: &[usize],
    key: &str,
    expected: &str,
) -> bool {
    let Some(first_index) = indices.first() else {
        return false;
    };
    let entry = &observed.evidence[*first_index];
    observed
        .snapshots
        .iter()
        .find(|snapshot| snapshot.cycle_index == entry.cycle_index && snapshot.tick == entry.tick)
        .and_then(|snapshot| snapshot.facts.get(key))
        .is_some_and(|actual| actual == expected)
}

fn snapshot_matches_binding(
    snapshot: Option<&BTreeMap<String, String>>,
    binding: &super::contract::ObservationBinding,
) -> bool {
    let Some(snapshot) = snapshot else {
        return false;
    };
    binding
        .evidence
        .iter()
        .all(|evidence| snapshot.get(&evidence.key) == Some(&evidence.expected))
}
