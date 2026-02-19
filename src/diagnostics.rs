use crate::ast::{
    ActionStatement, DurationValue, PlcProgram, StepStatement, TimeUnit, WaitCondition,
};
use crate::parser::parse_plc;
use crate::timing_report::TimingReport;
use crate::trace_diff::{NormalizedTraceEvent, TraceDiffReport, TraceMismatchType};
use serde::{Deserialize, Serialize};
use sim::Scenario;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    NoBoard,
    HilBoard,
    RuntimeLive,
    Mixed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceInputKind {
    Trace,
    Diff,
    TimingReport,
    IoSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IoTickSnapshot {
    pub tick: u64,
    pub digital_inputs: Vec<bool>,
    pub analog_inputs: Vec<f32>,
    pub digital_outputs: Vec<bool>,
    pub analog_outputs: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IoSnapshotArtifact {
    pub schema_version: u32,
    pub tick_ms: u64,
    pub ticks: Vec<IoTickSnapshot>,
}

impl IoSnapshotArtifact {
    fn unchanged_digital_inputs_until(
        &self,
        ids: &BTreeSet<u16>,
        until_tick: u64,
    ) -> BTreeSet<u16> {
        let mut unchanged = BTreeSet::new();
        for id in ids {
            let idx = usize::from(*id);
            let mut seen_any = false;
            let mut baseline: Option<bool> = None;
            let mut changed = false;
            for row in self.ticks.iter().filter(|row| row.tick <= until_tick) {
                let Some(value) = row.digital_inputs.get(idx).copied() else {
                    continue;
                };
                seen_any = true;
                match baseline {
                    None => baseline = Some(value),
                    Some(first) if first != value => {
                        changed = true;
                        break;
                    }
                    Some(_) => {}
                }
            }
            if seen_any && !changed {
                unchanged.insert(*id);
            }
        }
        unchanged
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosisCategory {
    ExpectedInputNeverChanged,
    ActuatorCommandMissing,
    InterlockOrRequiresBlocked,
    MappingOrAliasMismatch,
    TimeoutBudgetTooShort,
}

impl DiagnosisCategory {
    fn issue_code(self) -> &'static str {
        match self {
            Self::ExpectedInputNeverChanged => "DIAG-IN-001",
            Self::ActuatorCommandMissing => "DIAG-ACT-001",
            Self::InterlockOrRequiresBlocked => "DIAG-INT-001",
            Self::MappingOrAliasMismatch => "DIAG-MAP-001",
            Self::TimeoutBudgetTooShort => "DIAG-TIME-001",
        }
    }

    fn suggested_fix(self) -> &'static str {
        match self {
            Self::ExpectedInputNeverChanged => {
                "Inject or wire expected DI/AI changes earlier, then re-run trace verification."
            }
            Self::ActuatorCommandMissing => {
                "Check transition-to-action path and actuator command emission around mismatch anchor."
            }
            Self::InterlockOrRequiresBlocked => {
                "Review safety requires/conflicts constraints and verify all preconditions are satisfiable."
            }
            Self::MappingOrAliasMismatch => {
                "Verify PLC/scenario channel mapping (physical IDs, aliases, and topology connections)."
            }
            Self::TimeoutBudgetTooShort => {
                "Increase timeout budget or reduce cycle load so wait conditions can settle in time."
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnchorKind {
    Timeout,
    FirstTraceMismatch,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosisAnchor {
    pub kind: AnchorKind,
    pub tick: Option<u64>,
    pub trace_index: Option<usize>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiagnosisCandidate {
    pub issue_code: String,
    pub category: DiagnosisCategory,
    pub rank: u32,
    pub confidence: f64,
    pub evidence: Vec<String>,
    pub suggested_fix: String,
    pub evidence_source: EvidenceSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiagnosisReport {
    pub schema_version: u32,
    pub anchors: Vec<DiagnosisAnchor>,
    pub candidates: Vec<DiagnosisCandidate>,
    pub evidence_inputs: Vec<EvidenceInputKind>,
}

#[derive(Debug, Clone, Copy)]
pub struct DiagnosisInput<'a> {
    pub plc_source: &'a str,
    pub scenario: &'a Scenario,
    pub trace_events: Option<&'a [NormalizedTraceEvent]>,
    pub diff_report: Option<&'a TraceDiffReport>,
    pub timing_report: Option<&'a TimingReport>,
    pub evidence_source: EvidenceSource,
    pub io_snapshot: Option<&'a IoSnapshotArtifact>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DiagnosisError {
    #[error("diagnosis requires at least one trace or diff artifact")]
    MissingTraceOrDiff,

    #[error("failed to parse plc source for diagnosis: {0}")]
    InvalidPlc(String),
}

pub fn diagnose(input: DiagnosisInput<'_>) -> Result<DiagnosisReport, DiagnosisError> {
    if input.trace_events.is_none() && input.diff_report.is_none() {
        return Err(DiagnosisError::MissingTraceOrDiff);
    }

    let parsed =
        parse_plc(input.plc_source).map_err(|e| DiagnosisError::InvalidPlc(e.to_string()))?;
    let plc = PlcEvidence::collect(&parsed, input.scenario.tick_ms);
    let scenario = ScenarioEvidence::collect(input.scenario);
    let anchors = collect_anchors(input.trace_events, input.diff_report);
    let evidence_inputs = collect_evidence_inputs(&input);

    let timeout_anchor = anchors.iter().find(|a| a.kind == AnchorKind::Timeout);
    let mismatch_anchor = anchors
        .iter()
        .find(|a| a.kind == AnchorKind::FirstTraceMismatch);

    let mut drafts = vec![
        score_expected_input_never_changed(timeout_anchor, &plc, &scenario, input.io_snapshot),
        score_actuator_command_missing(mismatch_anchor, input.diff_report, &plc),
        score_interlock_or_requires_blocked(timeout_anchor, &plc),
        score_mapping_or_alias_mismatch(mismatch_anchor, input.diff_report, &plc, &scenario),
        score_timeout_budget_too_short(timeout_anchor, input.timing_report, &plc, input.scenario),
    ];

    drafts.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.category.cmp(&b.category))
    });

    let candidates = drafts
        .into_iter()
        .enumerate()
        .map(|(idx, draft)| DiagnosisCandidate {
            issue_code: draft.category.issue_code().to_string(),
            category: draft.category,
            rank: u32::try_from(idx + 1).unwrap_or(u32::MAX),
            confidence: round_confidence(draft.confidence),
            evidence: if draft.evidence.is_empty() {
                vec![
                    "insufficient direct evidence; candidate retained as deterministic fallback"
                        .to_string(),
                ]
            } else {
                draft.evidence
            },
            suggested_fix: draft.category.suggested_fix().to_string(),
            evidence_source: input.evidence_source,
        })
        .collect();

    Ok(DiagnosisReport {
        schema_version: 1,
        anchors,
        candidates,
        evidence_inputs,
    })
}

fn collect_evidence_inputs(input: &DiagnosisInput<'_>) -> Vec<EvidenceInputKind> {
    let mut out = Vec::new();
    if input.trace_events.is_some() {
        out.push(EvidenceInputKind::Trace);
    }
    if input.diff_report.is_some() {
        out.push(EvidenceInputKind::Diff);
    }
    if input.timing_report.is_some() {
        out.push(EvidenceInputKind::TimingReport);
    }
    if input.io_snapshot.is_some() {
        out.push(EvidenceInputKind::IoSnapshot);
    }
    out
}

#[derive(Debug)]
struct CandidateDraft {
    category: DiagnosisCategory,
    confidence: f64,
    evidence: Vec<String>,
}

#[derive(Debug, Default)]
struct PlcEvidence {
    wait_input_names: BTreeSet<String>,
    wait_input_ids: BTreeSet<u16>,
    known_input_ids: BTreeSet<u16>,
    action_targets: BTreeSet<String>,
    interlock_constraints: usize,
    min_timeout_ticks: Option<u64>,
}

impl PlcEvidence {
    fn collect(program: &PlcProgram, tick_ms: u64) -> Self {
        let mut out = PlcEvidence {
            interlock_constraints: program.constraints.safety.len(),
            ..Self::default()
        };

        for device in &program.topology.devices {
            let maybe_id = match device.device_type {
                crate::ast::DeviceType::DigitalInput => parse_channel_id(&device.name, true),
                crate::ast::DeviceType::AnalogInput => parse_channel_id(&device.name, false),
                _ => None,
            };
            if let Some(id) = maybe_id {
                out.known_input_ids.insert(id);
            }
        }

        for task in &program.tasks.tasks {
            for step in &task.steps {
                for statement in &step.statements {
                    out.collect_from_statement(statement, tick_ms);
                }
            }
        }

        out
    }

    fn collect_from_statement(&mut self, statement: &StepStatement, tick_ms: u64) {
        match statement {
            StepStatement::Action(action) => match action {
                ActionStatement::Extend { target }
                | ActionStatement::Retract { target }
                | ActionStatement::Set { target, .. }
                | ActionStatement::SetAnalog { target, .. } => {
                    self.action_targets.insert(target.clone());
                }
                ActionStatement::Log { .. } => {}
            },
            StepStatement::Wait(wait) => self.collect_wait_condition(&wait.condition),
            StepStatement::Timeout(timeout) => {
                let ticks = duration_to_ticks(&timeout.duration, tick_ms);
                self.min_timeout_ticks = match self.min_timeout_ticks {
                    Some(existing) => Some(existing.min(ticks)),
                    None => Some(ticks),
                };
            }
            StepStatement::Repeat { body, .. } => {
                for nested in body {
                    self.collect_from_statement(nested, tick_ms);
                }
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    for nested in &branch.statements {
                        self.collect_from_statement(nested, tick_ms);
                    }
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    for nested in &branch.statements {
                        self.collect_from_statement(nested, tick_ms);
                    }
                }
            }
            StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }

    fn collect_wait_condition(&mut self, wait: &WaitCondition) {
        match wait {
            WaitCondition::Single(expr) => self.collect_wait_left_operand(&expr.left),
            WaitCondition::And(exprs) | WaitCondition::Or(exprs) => {
                for expr in exprs {
                    self.collect_wait_left_operand(&expr.left);
                }
            }
        }
    }

    fn collect_wait_left_operand(&mut self, left: &str) {
        self.wait_input_names.insert(left.to_string());
        if let Some(id) = parse_channel_id(left, true).or_else(|| parse_channel_id(left, false)) {
            self.wait_input_ids.insert(id);
        }
    }
}

#[derive(Debug, Default)]
struct ScenarioEvidence {
    all_input_ids: BTreeSet<u16>,
    first_change_tick_by_input: BTreeMap<u16, u64>,
}

impl ScenarioEvidence {
    fn collect(scenario: &Scenario) -> Self {
        let mut out = Self::default();

        for ev in &scenario.inputs {
            let tick = ms_to_tick(ev.at_ms, scenario.tick_ms);
            for id in ev.set.digital_inputs.keys() {
                out.record_change(*id, tick);
            }
            for id in ev.set.analog_inputs.keys() {
                out.record_change(*id, tick);
            }
        }

        for burst in &scenario.digital_bursts {
            out.record_change(burst.target, ms_to_tick(burst.at_ms, scenario.tick_ms));
        }

        for fault in &scenario.faults {
            out.record_change(
                fault.sensor_stuck.target,
                ms_to_tick(fault.sensor_stuck.at_ms, scenario.tick_ms),
            );
        }

        for force in &scenario.forces {
            let tick = ms_to_tick(force.at_ms, scenario.tick_ms);
            for id in force.set.digital_inputs.keys() {
                out.record_change(*id, tick);
            }
            for id in force.set.analog_inputs.keys() {
                out.record_change(*id, tick);
            }
        }

        out
    }

    fn record_change(&mut self, id: u16, tick: u64) {
        self.all_input_ids.insert(id);
        let entry = self.first_change_tick_by_input.entry(id).or_insert(tick);
        *entry = (*entry).min(tick);
    }

    fn changed_inputs_until(&self, tick: u64) -> BTreeSet<u16> {
        self.first_change_tick_by_input
            .iter()
            .filter_map(|(id, first_tick)| (*first_tick <= tick).then_some(*id))
            .collect()
    }
}

fn collect_anchors(
    trace_events: Option<&[NormalizedTraceEvent]>,
    diff_report: Option<&TraceDiffReport>,
) -> Vec<DiagnosisAnchor> {
    let mut anchors = Vec::new();

    if let Some(timeout_anchor) = collect_timeout_anchor(trace_events, diff_report) {
        anchors.push(timeout_anchor);
    }

    if let Some(diff) = diff_report {
        if !diff.is_match {
            anchors.push(DiagnosisAnchor {
                kind: AnchorKind::FirstTraceMismatch,
                tick: diff.first_mismatch_tick,
                trace_index: diff.mismatch_index,
                detail: format!(
                    "first trace mismatch type={} (context_window={})",
                    mismatch_type_label(diff.mismatch_type),
                    diff.context_window
                ),
            });
        }
    }

    anchors
}

fn collect_timeout_anchor(
    trace_events: Option<&[NormalizedTraceEvent]>,
    diff_report: Option<&TraceDiffReport>,
) -> Option<DiagnosisAnchor> {
    #[derive(Debug)]
    struct TimeoutMarker {
        tick: u64,
        index: Option<usize>,
        source: &'static str,
    }

    let mut markers = Vec::new();

    if let Some(trace) = trace_events {
        if let Some((idx, event)) = trace
            .iter()
            .enumerate()
            .find(|(_, ev)| ev.reason == "timeout")
        {
            markers.push(TimeoutMarker {
                tick: event.tick,
                index: Some(idx),
                source: "trace",
            });
        }
    }

    if let Some(diff) = diff_report {
        if let Some(row) = diff.context.iter().find(|row| {
            row.sil.as_ref().is_some_and(|ev| ev.reason == "timeout")
                || row.board.as_ref().is_some_and(|ev| ev.reason == "timeout")
        }) {
            let tick = row
                .sil
                .as_ref()
                .filter(|ev| ev.reason == "timeout")
                .map(|ev| ev.tick)
                .or_else(|| {
                    row.board
                        .as_ref()
                        .filter(|ev| ev.reason == "timeout")
                        .map(|ev| ev.tick)
                })
                .unwrap_or_default();
            markers.push(TimeoutMarker {
                tick,
                index: Some(row.index),
                source: "diff_context",
            });
        }
    }

    markers.sort_by(|a, b| a.tick.cmp(&b.tick).then_with(|| a.index.cmp(&b.index)));

    markers.first().map(|m| DiagnosisAnchor {
        kind: AnchorKind::Timeout,
        tick: Some(m.tick),
        trace_index: m.index,
        detail: format!("first timeout seen in {}", m.source),
    })
}

fn score_expected_input_never_changed(
    timeout_anchor: Option<&DiagnosisAnchor>,
    plc: &PlcEvidence,
    scenario: &ScenarioEvidence,
    io_snapshot: Option<&IoSnapshotArtifact>,
) -> CandidateDraft {
    let mut confidence = 0.20;
    let mut evidence = Vec::new();

    if let Some(anchor) = timeout_anchor {
        confidence += 0.35;
        evidence.push(format!(
            "timeout anchor at tick {}",
            anchor.tick.unwrap_or_default()
        ));

        let timeout_tick = anchor.tick.unwrap_or_default();
        let changed_before = scenario.changed_inputs_until(timeout_tick);
        if changed_before.is_empty() {
            confidence += 0.15;
            evidence.push(format!(
                "no DI/AI changes scheduled before timeout tick {}",
                timeout_tick
            ));
        }

        if !plc.wait_input_ids.is_empty() {
            let wait_inputs_changed = plc
                .wait_input_ids
                .iter()
                .any(|id| changed_before.contains(id));
            if !wait_inputs_changed {
                confidence += 0.15;
                evidence
                    .push("wait-related channels were never toggled before timeout".to_string());
            }
        }
    }

    if scenario.first_change_tick_by_input.is_empty() {
        confidence += 0.10;
        evidence.push("scenario has no scripted input/fault/force DI/AI mutations".to_string());
    }

    if !plc.wait_input_names.is_empty() {
        evidence.push(format!(
            "wait predicates reference {}",
            preview_set(&plc.wait_input_names, 3)
        ));
    }
    if let Some(snapshot) = io_snapshot {
        evidence.push(format!(
            "io snapshot artifact schema_version={} ticks={}",
            snapshot.schema_version,
            snapshot.ticks.len()
        ));
        if let Some(timeout_tick) = timeout_anchor.and_then(|anchor| anchor.tick) {
            let unchanged =
                snapshot.unchanged_digital_inputs_until(&plc.wait_input_ids, timeout_tick);
            if !unchanged.is_empty() {
                confidence += 0.10;
                evidence.push(format!(
                    "io snapshot shows wait DI channels unchanged until timeout: {}",
                    preview_u16_set(&unchanged, 6)
                ));
            }
        }
    }

    CandidateDraft {
        category: DiagnosisCategory::ExpectedInputNeverChanged,
        confidence,
        evidence,
    }
}

fn score_actuator_command_missing(
    mismatch_anchor: Option<&DiagnosisAnchor>,
    diff_report: Option<&TraceDiffReport>,
    plc: &PlcEvidence,
) -> CandidateDraft {
    let mut confidence = 0.18;
    let mut evidence = Vec::new();

    if let Some(anchor) = mismatch_anchor {
        confidence += 0.35;
        evidence.push(format!(
            "mismatch anchor at tick {}",
            anchor.tick.unwrap_or_default()
        ));
    }

    if let Some(diff) = diff_report {
        if diff.board_events < diff.sil_events {
            confidence += 0.15;
            evidence.push(format!(
                "board trace shorter than SIL trace (board={}, sil={})",
                diff.board_events, diff.sil_events
            ));
        }

        if matches!(
            diff.mismatch_type,
            Some(TraceMismatchType::Step | TraceMismatchType::Edge)
        ) {
            confidence += 0.10;
            evidence.push("mismatch type indicates transition/action path divergence".to_string());
        }
    }

    if !plc.action_targets.is_empty() {
        confidence += 0.05;
        evidence.push(format!(
            "PLC actions target {}",
            preview_set(&plc.action_targets, 3)
        ));
    }

    CandidateDraft {
        category: DiagnosisCategory::ActuatorCommandMissing,
        confidence,
        evidence,
    }
}

fn score_interlock_or_requires_blocked(
    timeout_anchor: Option<&DiagnosisAnchor>,
    plc: &PlcEvidence,
) -> CandidateDraft {
    let mut confidence = 0.14;
    let mut evidence = Vec::new();

    if timeout_anchor.is_some() {
        confidence += 0.22;
        evidence.push("timeout anchor is compatible with blocked preconditions".to_string());
    }

    if plc.interlock_constraints > 0 {
        confidence += 0.32;
        evidence.push(format!(
            "PLC defines {} safety requires/conflicts constraints",
            plc.interlock_constraints
        ));
    }

    if !plc.wait_input_names.is_empty() {
        confidence += 0.05;
        evidence.push("wait predicates can be indirectly blocked by interlocks".to_string());
    }

    CandidateDraft {
        category: DiagnosisCategory::InterlockOrRequiresBlocked,
        confidence,
        evidence,
    }
}

fn score_mapping_or_alias_mismatch(
    mismatch_anchor: Option<&DiagnosisAnchor>,
    diff_report: Option<&TraceDiffReport>,
    plc: &PlcEvidence,
    scenario: &ScenarioEvidence,
) -> CandidateDraft {
    let mut confidence = 0.10;
    let mut evidence = Vec::new();

    if mismatch_anchor.is_some() {
        confidence += 0.34;
        evidence.push("trace mismatch anchor suggests signal mapping divergence".to_string());
    }

    if matches!(
        diff_report.and_then(|diff| diff.mismatch_type),
        Some(TraceMismatchType::Reason | TraceMismatchType::Step)
    ) {
        confidence += 0.10;
        evidence.push("mismatch type is reason/step (common in alias wiring issues)".to_string());
    }

    if !plc.known_input_ids.is_empty() {
        let unknown: BTreeSet<u16> = scenario
            .all_input_ids
            .difference(&plc.known_input_ids)
            .copied()
            .collect();
        if !unknown.is_empty() {
            confidence += 0.35;
            evidence.push(format!(
                "scenario references input ids missing in PLC topology: {}",
                preview_u16_set(&unknown, 6)
            ));
        }
    }

    CandidateDraft {
        category: DiagnosisCategory::MappingOrAliasMismatch,
        confidence,
        evidence,
    }
}

fn score_timeout_budget_too_short(
    timeout_anchor: Option<&DiagnosisAnchor>,
    timing_report: Option<&TimingReport>,
    plc: &PlcEvidence,
    scenario: &Scenario,
) -> CandidateDraft {
    let mut confidence = 0.08;
    let mut evidence = Vec::new();

    if timeout_anchor.is_some() {
        confidence += 0.25;
        evidence.push("timeout anchor is present".to_string());
    }

    if let Some(min_timeout_ticks) = plc.min_timeout_ticks {
        evidence.push(format!(
            "minimum configured timeout={} ticks",
            min_timeout_ticks
        ));

        if min_timeout_ticks <= 3 {
            confidence += 0.15;
            evidence.push("configured timeout window is short (<=3 ticks)".to_string());
        }

        if let Some(anchor_tick) = timeout_anchor.and_then(|anchor| anchor.tick) {
            if anchor_tick <= min_timeout_ticks.saturating_add(1) {
                confidence += 0.10;
                evidence.push("timeout occurs near configured budget boundary".to_string());
            }
        }
    }

    if let Some(timing) = timing_report {
        let tick_budget_us = scenario.tick_ms.saturating_mul(1_000);
        if tick_budget_us > 0 {
            evidence.push(format!(
                "timing p99={}us vs tick_budget={}us",
                timing.exec_us_p99, tick_budget_us
            ));
            if timing.exec_us_p99.saturating_mul(10) >= tick_budget_us.saturating_mul(8) {
                confidence += 0.20;
                evidence.push("p99 execution consumes >=80% of tick budget".to_string());
            }
        }

        if timing.overrun_count > 0 {
            confidence += 0.20;
            evidence.push(format!(
                "timing report records {} overruns",
                timing.overrun_count
            ));
        }
    }

    CandidateDraft {
        category: DiagnosisCategory::TimeoutBudgetTooShort,
        confidence,
        evidence,
    }
}

fn preview_set(set: &BTreeSet<String>, limit: usize) -> String {
    set.iter()
        .take(limit)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ")
}

fn preview_u16_set(set: &BTreeSet<u16>, limit: usize) -> String {
    set.iter()
        .take(limit)
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn mismatch_type_label(mismatch_type: Option<TraceMismatchType>) -> &'static str {
    match mismatch_type {
        Some(TraceMismatchType::Step) => "step",
        Some(TraceMismatchType::Reason) => "reason",
        Some(TraceMismatchType::Edge) => "edge",
        None => "unknown",
    }
}

fn parse_channel_id(raw: &str, digital: bool) -> Option<u16> {
    let upper = raw.trim().to_ascii_uppercase();
    let token = if digital {
        upper.strip_prefix('X').or_else(|| upper.strip_prefix("DI"))
    } else {
        upper.strip_prefix("AI")
    }?;
    if token.is_empty() || !token.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    token.parse::<u16>().ok()
}

fn duration_to_ticks(duration: &DurationValue, tick_ms: u64) -> u64 {
    if tick_ms == 0 {
        return 0;
    }
    let duration_ms = match duration.unit {
        TimeUnit::Ms => duration.value,
        TimeUnit::S => duration.value.saturating_mul(1_000),
    };
    duration_ms.saturating_add(tick_ms.saturating_sub(1)) / tick_ms
}

fn ms_to_tick(at_ms: u64, tick_ms: u64) -> u64 {
    if tick_ms == 0 {
        return 0;
    }
    at_ms / tick_ms
}

fn round_confidence(value: f64) -> f64 {
    let clamped = value.clamp(0.0, 0.99);
    (clamped * 1_000.0).round() / 1_000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timing_report::TimingReport;
    use crate::trace_diff::{TraceDiffContextRow, TraceMismatchType};

    fn fixture_plc() -> &'static str {
        r#"
[topology]

device X0: digital_input
device X1: digital_input
device Y0: digital_output

[constraints]

safety: Y0.on requires X1.on

[tasks]

task cycle:
    step wait_start:
        wait: X0 == true
        timeout: 30ms -> goto fault

    step run:
        action: set Y0 on

    on_complete: goto done

task fault:
    step safe_stop:
        action: set Y0 off

task done:
    step halt:
"#
    }

    fn fixture_scenario() -> Scenario {
        Scenario::from_yaml_str(
            r#"
tick_ms: 10
duration_ms: 100
inputs: []
"#,
        )
        .expect("scenario should parse")
    }

    fn fixture_trace() -> Vec<NormalizedTraceEvent> {
        vec![
            NormalizedTraceEvent {
                tick: 0,
                task: 0,
                from_step: 0,
                to_step: 1,
                reason: "action".to_string(),
            },
            NormalizedTraceEvent {
                tick: 3,
                task: 0,
                from_step: 1,
                to_step: 2,
                reason: "timeout".to_string(),
            },
        ]
    }

    fn fixture_diff(trace: &[NormalizedTraceEvent]) -> TraceDiffReport {
        TraceDiffReport {
            is_match: false,
            sil_events: 2,
            board_events: 1,
            first_mismatch_tick: Some(3),
            mismatch_type: Some(TraceMismatchType::Step),
            mismatch_index: Some(1),
            context_window: 2,
            context: vec![TraceDiffContextRow {
                index: 1,
                sil: trace.get(1).cloned(),
                board: None,
            }],
        }
    }

    fn fixture_timing() -> TimingReport {
        TimingReport {
            schema_version: 1,
            count: 8,
            overrun_count: 1,
            exec_us_min: 500,
            exec_us_max: 11_000,
            exec_us_p50: 2_000,
            exec_us_p95: 9_500,
            exec_us_p99: 9_200,
            exec_us_mean: 3_400.0,
        }
    }

    fn fixture_io_snapshot() -> IoSnapshotArtifact {
        IoSnapshotArtifact {
            schema_version: 1,
            tick_ms: 10,
            ticks: vec![
                IoTickSnapshot {
                    tick: 0,
                    digital_inputs: vec![false, false],
                    analog_inputs: vec![],
                    digital_outputs: vec![false],
                    analog_outputs: vec![],
                },
                IoTickSnapshot {
                    tick: 1,
                    digital_inputs: vec![false, false],
                    analog_inputs: vec![],
                    digital_outputs: vec![false],
                    analog_outputs: vec![],
                },
                IoTickSnapshot {
                    tick: 2,
                    digital_inputs: vec![false, false],
                    analog_inputs: vec![],
                    digital_outputs: vec![false],
                    analog_outputs: vec![],
                },
                IoTickSnapshot {
                    tick: 3,
                    digital_inputs: vec![false, false],
                    analog_inputs: vec![],
                    digital_outputs: vec![false],
                    analog_outputs: vec![],
                },
            ],
        }
    }

    #[test]
    fn diagnose_recognizes_timeout_and_mismatch_anchors() {
        let scenario = fixture_scenario();
        let trace = fixture_trace();
        let diff = fixture_diff(&trace);
        let timing = fixture_timing();

        let report = diagnose(DiagnosisInput {
            plc_source: fixture_plc(),
            scenario: &scenario,
            trace_events: Some(&trace),
            diff_report: Some(&diff),
            timing_report: Some(&timing),
            evidence_source: EvidenceSource::NoBoard,
            io_snapshot: None,
        })
        .expect("diagnosis should succeed");

        assert_eq!(report.schema_version, 1);
        assert!(
            report
                .anchors
                .iter()
                .any(|anchor| anchor.kind == AnchorKind::Timeout),
            "timeout anchor should exist"
        );
        assert!(
            report
                .anchors
                .iter()
                .any(|anchor| anchor.kind == AnchorKind::FirstTraceMismatch),
            "first mismatch anchor should exist"
        );

        let categories: BTreeSet<DiagnosisCategory> = report
            .candidates
            .iter()
            .map(|candidate| candidate.category)
            .collect();
        assert_eq!(
            categories,
            BTreeSet::from([
                DiagnosisCategory::ExpectedInputNeverChanged,
                DiagnosisCategory::ActuatorCommandMissing,
                DiagnosisCategory::InterlockOrRequiresBlocked,
                DiagnosisCategory::MappingOrAliasMismatch,
                DiagnosisCategory::TimeoutBudgetTooShort,
            ])
        );

        for (idx, candidate) in report.candidates.iter().enumerate() {
            assert_eq!(candidate.rank, u32::try_from(idx + 1).unwrap_or(u32::MAX));
            assert!(candidate.issue_code.starts_with("DIAG-"));
            assert!((0.0..=1.0).contains(&candidate.confidence));
            assert!(!candidate.evidence.is_empty());
            assert!(!candidate.suggested_fix.is_empty());
            assert_eq!(candidate.evidence_source, EvidenceSource::NoBoard);
        }
    }

    #[test]
    fn diagnose_is_deterministic_for_same_input() {
        let scenario = fixture_scenario();
        let trace = fixture_trace();
        let diff = fixture_diff(&trace);

        let report_a = diagnose(DiagnosisInput {
            plc_source: fixture_plc(),
            scenario: &scenario,
            trace_events: Some(&trace),
            diff_report: Some(&diff),
            timing_report: None,
            evidence_source: EvidenceSource::Mixed,
            io_snapshot: None,
        })
        .expect("first run should succeed");

        let report_b = diagnose(DiagnosisInput {
            plc_source: fixture_plc(),
            scenario: &scenario,
            trace_events: Some(&trace),
            diff_report: Some(&diff),
            timing_report: None,
            evidence_source: EvidenceSource::Mixed,
            io_snapshot: None,
        })
        .expect("second run should succeed");

        let json_a = serde_json::to_string(&report_a).expect("serialize report a");
        let json_b = serde_json::to_string(&report_b).expect("serialize report b");
        assert_eq!(json_a, json_b, "JSON output must be deterministic");
    }

    #[test]
    fn diagnose_accepts_trace_or_diff_only_inputs() {
        let scenario = fixture_scenario();
        let trace = fixture_trace();
        let diff = fixture_diff(&trace);

        let trace_only = diagnose(DiagnosisInput {
            plc_source: fixture_plc(),
            scenario: &scenario,
            trace_events: Some(&trace),
            diff_report: None,
            timing_report: None,
            evidence_source: EvidenceSource::RuntimeLive,
            io_snapshot: None,
        })
        .expect("trace-only diagnosis should succeed");
        assert!(
            trace_only
                .anchors
                .iter()
                .any(|anchor| anchor.kind == AnchorKind::Timeout)
        );

        let diff_only = diagnose(DiagnosisInput {
            plc_source: fixture_plc(),
            scenario: &scenario,
            trace_events: None,
            diff_report: Some(&diff),
            timing_report: None,
            evidence_source: EvidenceSource::HilBoard,
            io_snapshot: None,
        })
        .expect("diff-only diagnosis should succeed");
        assert!(
            diff_only
                .anchors
                .iter()
                .any(|anchor| anchor.kind == AnchorKind::FirstTraceMismatch)
        );
    }

    #[test]
    fn diagnose_records_evidence_inputs_when_snapshot_is_present() {
        let scenario = fixture_scenario();
        let trace = fixture_trace();
        let diff = fixture_diff(&trace);
        let timing = fixture_timing();
        let snapshot = fixture_io_snapshot();

        let report = diagnose(DiagnosisInput {
            plc_source: fixture_plc(),
            scenario: &scenario,
            trace_events: Some(&trace),
            diff_report: Some(&diff),
            timing_report: Some(&timing),
            evidence_source: EvidenceSource::NoBoard,
            io_snapshot: Some(&snapshot),
        })
        .expect("diagnosis should succeed");

        assert_eq!(
            report.evidence_inputs,
            vec![
                EvidenceInputKind::Trace,
                EvidenceInputKind::Diff,
                EvidenceInputKind::TimingReport,
                EvidenceInputKind::IoSnapshot,
            ]
        );
    }

    #[test]
    fn io_snapshot_boosts_expected_input_candidate_when_wait_channel_is_flat() {
        let scenario = fixture_scenario();
        let trace = fixture_trace();
        let diff = fixture_diff(&trace);
        let snapshot = fixture_io_snapshot();

        let without_snapshot = diagnose(DiagnosisInput {
            plc_source: fixture_plc(),
            scenario: &scenario,
            trace_events: Some(&trace),
            diff_report: Some(&diff),
            timing_report: None,
            evidence_source: EvidenceSource::NoBoard,
            io_snapshot: None,
        })
        .expect("baseline diagnosis should succeed");
        let with_snapshot = diagnose(DiagnosisInput {
            plc_source: fixture_plc(),
            scenario: &scenario,
            trace_events: Some(&trace),
            diff_report: Some(&diff),
            timing_report: None,
            evidence_source: EvidenceSource::NoBoard,
            io_snapshot: Some(&snapshot),
        })
        .expect("snapshot diagnosis should succeed");

        let base = without_snapshot
            .candidates
            .iter()
            .find(|candidate| candidate.category == DiagnosisCategory::ExpectedInputNeverChanged)
            .expect("baseline expected-input candidate");
        let boosted = with_snapshot
            .candidates
            .iter()
            .find(|candidate| candidate.category == DiagnosisCategory::ExpectedInputNeverChanged)
            .expect("snapshot expected-input candidate");

        assert!(
            boosted.confidence >= base.confidence,
            "snapshot should not reduce confidence"
        );
        assert!(
            boosted
                .evidence
                .iter()
                .any(|line| line.contains("io snapshot")),
            "snapshot evidence should be retained"
        );
    }

    #[test]
    fn diagnose_rejects_missing_trace_and_diff() {
        let scenario = fixture_scenario();
        let err = diagnose(DiagnosisInput {
            plc_source: fixture_plc(),
            scenario: &scenario,
            trace_events: None,
            diff_report: None,
            timing_report: None,
            evidence_source: EvidenceSource::NoBoard,
            io_snapshot: None,
        })
        .expect_err("missing artifacts should fail");

        assert_eq!(err, DiagnosisError::MissingTraceOrDiff);
    }

    #[test]
    fn evidence_source_serialization_is_stable() {
        assert_eq!(
            serde_json::to_string(&EvidenceSource::NoBoard).expect("serialize no_board"),
            "\"no_board\""
        );
        assert_eq!(
            serde_json::to_string(&EvidenceSource::HilBoard).expect("serialize hil_board"),
            "\"hil_board\""
        );
        assert_eq!(
            serde_json::to_string(&EvidenceSource::RuntimeLive).expect("serialize runtime_live"),
            "\"runtime_live\""
        );
        assert_eq!(
            serde_json::to_string(&EvidenceSource::Mixed).expect("serialize mixed"),
            "\"mixed\""
        );
    }
}
