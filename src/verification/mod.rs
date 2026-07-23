pub mod causality;
pub mod liveness;
pub mod safety;
pub mod timing;

use crate::ast::PlcProgram;
use crate::ir::{ConstraintSet, StateMachine, TopologyGraph};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::panic::{self, AssertUnwindSafe};
use std::thread::ScopedJoinHandle;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WarningLevel {
    Error,
    Warn,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WarningEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub level: WarningLevel,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CheckerSummary {
    pub level: String,
    pub warnings: Vec<WarningEntry>,
    pub checked_rules: usize,
    pub skipped_rules: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SafetySummary {
    pub level: String,
    pub explored_depth: usize,
    pub warnings: Vec<WarningEntry>,
    pub checked_rules: usize,
    pub skipped_rules: usize,
    pub coverage: safety::SafetyCoverage,
    pub rule_statuses: Vec<safety::SafetyRuleStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerificationSummary {
    pub safety: SafetySummary,
    pub liveness: CheckerSummary,
    pub timing: CheckerSummary,
    pub causality: CheckerSummary,
    #[serde(default = "default_station_protocol_summary")]
    pub station_protocol: CheckerSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationInputFingerprints {
    pub safety: String,
    pub liveness: String,
    pub timing: String,
    pub causality: String,
    #[serde(default)]
    pub station_protocol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReusableVerificationSummary {
    pub fingerprints: VerificationInputFingerprints,
    pub summary: VerificationSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationReuseReport {
    pub reused_checkers: Vec<String>,
    pub checked_checkers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationIssue {
    pub checker: String,
    pub line: usize,
    pub reason: String,
    pub suggestion: String,
    pub details: Vec<String>,
}

impl fmt::Display for VerificationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "ERROR [{}] 验证失败", self.checker)?;
        writeln!(f, "  位置: <input>:{}:1", self.line)?;
        writeln!(f, "  原因: {}", self.reason)?;

        for detail in &self.details {
            writeln!(f, "  分析: {detail}")?;
        }

        write!(f, "  建议: {}", self.suggestion)
    }
}

pub fn verification_input_fingerprints(
    topology: &TopologyGraph,
    constraints: &ConstraintSet,
    state_machine: &StateMachine,
) -> VerificationInputFingerprints {
    VerificationInputFingerprints {
        safety: fingerprint_serializable(
            "verification.safety.v1",
            &(
                &constraints.safety,
                &constraints.workpiece_types,
                &constraints.workpiece_sites,
                &constraints.workpiece_holders,
                &constraints.workpiece_carriers,
                &constraints.semantic_resources,
                &constraints.resource_claims,
                state_machine,
            ),
        ),
        liveness: fingerprint_serializable("verification.liveness.v1", state_machine),
        timing: fingerprint_serializable(
            "verification.timing.v1",
            &(&constraints.timing, topology, state_machine),
        ),
        causality: fingerprint_serializable(
            "verification.causality.v1",
            &(&constraints.causality, topology, state_machine),
        ),
        station_protocol: fingerprint_serializable(
            "verification.station_protocol.v1",
            &topology.station_protocol,
        ),
    }
}

pub fn reusable_verification_summary(
    topology: &TopologyGraph,
    constraints: &ConstraintSet,
    state_machine: &StateMachine,
    summary: VerificationSummary,
) -> ReusableVerificationSummary {
    ReusableVerificationSummary {
        fingerprints: verification_input_fingerprints(topology, constraints, state_machine),
        summary,
    }
}

fn fingerprint_serializable<T: Serialize>(domain: &str, value: &T) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain.as_bytes());
    hasher.update(b"\0");
    let bytes = serde_json::to_vec(value)
        .expect("verification fingerprint inputs must serialize deterministically");
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub fn verify_all(
    program: &PlcProgram,
    topology: &TopologyGraph,
    constraints: &ConstraintSet,
    state_machine: &StateMachine,
) -> Result<VerificationSummary, Vec<VerificationIssue>> {
    let mut issues = Vec::new();
    let VerificationEngineResults {
        safety: safety_result,
        liveness: liveness_result,
        timing: timing_result,
        causality: causality_result,
    } = run_verification_engines_parallel(program, topology, constraints, state_machine);

    let safety_summary = match safety_result {
        Ok(Ok(report)) => {
            let level = match report.level {
                safety::SafetyProofLevel::Complete => "完备证明",
                safety::SafetyProofLevel::Bounded => "有界验证",
            }
            .to_string();

            SafetySummary {
                level,
                explored_depth: report.explored_depth,
                warnings: report
                    .warnings
                    .into_iter()
                    .map(|warning| warning_entry(&warning))
                    .collect(),
                checked_rules: report.checked_rules,
                skipped_rules: report.skipped_rules,
                coverage: report.coverage,
                rule_statuses: report.rule_statuses,
            }
        }
        Ok(Err(diagnostics)) => {
            issues.extend(diagnostics.into_iter().map(|diag| VerificationIssue {
                checker: "safety".to_string(),
                line: diag.line.max(1),
                reason: format!("约束 {}：{}", diag.constraint, diag.reason),
                suggestion: diag.suggestion,
                details: vec![format!("违反路径: {}", diag.violation_path.join(" -> "))],
            }));

            SafetySummary {
                level: "失败".to_string(),
                explored_depth: 0,
                warnings: Vec::new(),
                checked_rules: 0,
                skipped_rules: constraints.safety.len(),
                coverage: safety::SafetyCoverage {
                    bound_rules: 0,
                    degraded_rules: 0,
                    skipped_rules: constraints.safety.len(),
                    total_rules: constraints.safety.len(),
                },
                rule_statuses: Vec::new(),
            }
        }
        Err(issue) => {
            issues.push(issue);
            failed_safety_summary(constraints)
        }
    };

    match liveness_result {
        Ok(Ok(())) => {}
        Ok(Err(diagnostics)) => {
            issues.extend(diagnostics.into_iter().map(|diag| VerificationIssue {
                checker: "liveness".to_string(),
                line: diag.line.max(1),
                reason: diag.reason,
                suggestion: diag.suggestion,
                details: vec![diag.physical_analysis],
            }));
        }
        Err(issue) => issues.push(issue),
    }

    match timing_result {
        Ok(Ok(())) => {}
        Ok(Err(diagnostics)) => {
            issues.extend(diagnostics.into_iter().map(|diag| VerificationIssue {
                checker: "timing".to_string(),
                line: diag.line.max(1),
                reason: format!("{}；{}", diag.constraint, diag.conclusion),
                suggestion: timing_suggestion(&diag.constraint),
                details: vec![diag.analysis],
            }));
        }
        Err(issue) => issues.push(issue),
    }

    match causality_result {
        Ok(Ok(())) => {}
        Ok(Err(diagnostics)) => {
            issues.extend(diagnostics.into_iter().map(|diag| {
                let mut details = Vec::new();
                if let Some(action) = diag.action {
                    details.push(format!("动作: {action}"));
                }
                if let Some(wait) = diag.wait {
                    details.push(format!("等待: {wait}"));
                }
                details.push(format!("期望链路: {}", diag.expected_chain));
                details.push(format!("实际链路: {}", diag.actual_chain));

                VerificationIssue {
                    checker: "causality".to_string(),
                    line: diag.line.max(1),
                    reason: format!("检测到因果链断裂：{}", diag.broken_link),
                    suggestion: diag.suggestion,
                    details,
                }
            }));
        }
        Err(issue) => issues.push(issue),
    }

    if !issues.is_empty() {
        return Err(issues);
    }

    Ok(VerificationSummary {
        safety: safety_summary,
        liveness: CheckerSummary {
            level: "通过".to_string(),
            warnings: Vec::new(),
            checked_rules: state_machine.states.len().max(1),
            skipped_rules: 0,
        },
        timing: CheckerSummary {
            level: "通过".to_string(),
            warnings: Vec::new(),
            checked_rules: constraints.timing.len(),
            skipped_rules: 0,
        },
        causality: CheckerSummary {
            level: "通过".to_string(),
            warnings: Vec::new(),
            checked_rules: constraints.causality.len(),
            skipped_rules: 0,
        },
        station_protocol: station_protocol_summary(topology),
    })
}

pub fn verify_all_incremental(
    program: &PlcProgram,
    topology: &TopologyGraph,
    constraints: &ConstraintSet,
    state_machine: &StateMachine,
    reusable: Option<&ReusableVerificationSummary>,
) -> Result<(VerificationSummary, VerificationReuseReport), Vec<VerificationIssue>> {
    let current_fingerprints =
        verification_input_fingerprints(topology, constraints, state_machine);
    let Some(reusable) = reusable else {
        let summary = verify_all(program, topology, constraints, state_machine)?;
        return Ok((summary, checked_all_reuse_report()));
    };

    let reuse_safety = reusable.fingerprints.safety == current_fingerprints.safety;
    let reuse_liveness = reusable.fingerprints.liveness == current_fingerprints.liveness;
    let reuse_timing = reusable.fingerprints.timing == current_fingerprints.timing;
    let reuse_causality = reusable.fingerprints.causality == current_fingerprints.causality;
    let reuse_station_protocol =
        reusable.fingerprints.station_protocol == current_fingerprints.station_protocol;

    if !(reuse_safety
        || reuse_liveness
        || reuse_timing
        || reuse_causality
        || reuse_station_protocol)
    {
        let summary = verify_all(program, topology, constraints, state_machine)?;
        return Ok((summary, checked_all_reuse_report()));
    }

    let mut report = VerificationReuseReport {
        reused_checkers: Vec::new(),
        checked_checkers: Vec::new(),
    };
    let mut issues = Vec::new();

    let safety_summary = if reuse_safety {
        report.reused_checkers.push("safety".to_string());
        reusable.summary.safety.clone()
    } else {
        report.checked_checkers.push("safety".to_string());
        let result = run_verification_stage_now("safety", || {
            safety::verify_safety(program, constraints, state_machine)
        });
        safety_summary_from_result(result, constraints, &mut issues)
    };

    let liveness_summary = if reuse_liveness {
        report.reused_checkers.push("liveness".to_string());
        reusable.summary.liveness.clone()
    } else {
        report.checked_checkers.push("liveness".to_string());
        let result = run_verification_stage_now("liveness", || {
            liveness::verify_liveness(program, state_machine)
        });
        collect_liveness_result(result, &mut issues);
        passed_liveness_summary(state_machine)
    };

    let timing_summary = if reuse_timing {
        report.reused_checkers.push("timing".to_string());
        reusable.summary.timing.clone()
    } else {
        report.checked_checkers.push("timing".to_string());
        let result = run_verification_stage_now("timing", || {
            timing::verify_timing(program, topology, constraints, state_machine)
        });
        collect_timing_result(result, &mut issues);
        passed_timing_summary(constraints)
    };

    let causality_summary = if reuse_causality {
        report.reused_checkers.push("causality".to_string());
        reusable.summary.causality.clone()
    } else {
        report.checked_checkers.push("causality".to_string());
        let result = run_verification_stage_now("causality", || {
            causality::verify_causality(program, topology, constraints)
        });
        collect_causality_result(result, &mut issues);
        passed_causality_summary(constraints)
    };
    let station_protocol_summary = if reuse_station_protocol {
        report.reused_checkers.push("station_protocol".to_string());
        reusable.summary.station_protocol.clone()
    } else {
        report.checked_checkers.push("station_protocol".to_string());
        station_protocol_summary(topology)
    };

    if !issues.is_empty() {
        return Err(issues);
    }

    Ok((
        VerificationSummary {
            safety: safety_summary,
            liveness: liveness_summary,
            timing: timing_summary,
            causality: causality_summary,
            station_protocol: station_protocol_summary,
        },
        report,
    ))
}

type SafetyVerificationResult = Result<safety::SafetyReport, Vec<safety::SafetyDiagnostic>>;
type LivenessVerificationResult = Result<(), Vec<liveness::LivenessDiagnostic>>;
type TimingVerificationResult = Result<(), Vec<timing::TimingDiagnostic>>;
type CausalityVerificationResult = Result<(), Vec<causality::CausalityDiagnostic>>;

struct VerificationEngineResults {
    safety: Result<SafetyVerificationResult, VerificationIssue>,
    liveness: Result<LivenessVerificationResult, VerificationIssue>,
    timing: Result<TimingVerificationResult, VerificationIssue>,
    causality: Result<CausalityVerificationResult, VerificationIssue>,
}

fn run_verification_engines_parallel(
    program: &PlcProgram,
    topology: &TopologyGraph,
    constraints: &ConstraintSet,
    state_machine: &StateMachine,
) -> VerificationEngineResults {
    std::thread::scope(|scope| {
        let safety_handle =
            scope.spawn(|| safety::verify_safety(program, constraints, state_machine));
        let liveness_handle = scope.spawn(|| liveness::verify_liveness(program, state_machine));
        let timing_handle =
            scope.spawn(|| timing::verify_timing(program, topology, constraints, state_machine));
        let causality_handle =
            scope.spawn(|| causality::verify_causality(program, topology, constraints));

        VerificationEngineResults {
            safety: join_verification_stage("safety", safety_handle),
            liveness: join_verification_stage("liveness", liveness_handle),
            timing: join_verification_stage("timing", timing_handle),
            causality: join_verification_stage("causality", causality_handle),
        }
    })
}

fn join_verification_stage<'scope, T, E>(
    checker: &str,
    handle: ScopedJoinHandle<'scope, Result<T, Vec<E>>>,
) -> Result<Result<T, Vec<E>>, VerificationIssue> {
    handle.join().map_err(|_| VerificationIssue {
        checker: checker.to_string(),
        line: 1,
        reason: format!("internal compiler error: verification checker `{checker}` panicked"),
        suggestion: "rerun with RUST_BACKTRACE=1 and report this compiler bug".to_string(),
        details: Vec::new(),
    })
}

fn run_verification_stage_now<T, E, F>(
    checker: &str,
    f: F,
) -> Result<Result<T, Vec<E>>, VerificationIssue>
where
    F: FnOnce() -> Result<T, Vec<E>>,
{
    panic::catch_unwind(AssertUnwindSafe(f)).map_err(|_| VerificationIssue {
        checker: checker.to_string(),
        line: 1,
        reason: format!("internal compiler error: verification checker `{checker}` panicked"),
        suggestion: "rerun with RUST_BACKTRACE=1 and report this compiler bug".to_string(),
        details: Vec::new(),
    })
}

fn checked_all_reuse_report() -> VerificationReuseReport {
    VerificationReuseReport {
        reused_checkers: Vec::new(),
        checked_checkers: vec![
            "safety".to_string(),
            "liveness".to_string(),
            "timing".to_string(),
            "causality".to_string(),
            "station_protocol".to_string(),
        ],
    }
}

fn safety_summary_from_result(
    safety_result: Result<SafetyVerificationResult, VerificationIssue>,
    constraints: &ConstraintSet,
    issues: &mut Vec<VerificationIssue>,
) -> SafetySummary {
    match safety_result {
        Ok(Ok(report)) => {
            let level = match report.level {
                safety::SafetyProofLevel::Complete => "瀹屽璇佹槑",
                safety::SafetyProofLevel::Bounded => "鏈夌晫楠岃瘉",
            }
            .to_string();

            SafetySummary {
                level,
                explored_depth: report.explored_depth,
                warnings: report
                    .warnings
                    .into_iter()
                    .map(|warning| warning_entry(&warning))
                    .collect(),
                checked_rules: report.checked_rules,
                skipped_rules: report.skipped_rules,
                coverage: report.coverage,
                rule_statuses: report.rule_statuses,
            }
        }
        Ok(Err(diagnostics)) => {
            issues.extend(diagnostics.into_iter().map(|diag| VerificationIssue {
                checker: "safety".to_string(),
                line: diag.line.max(1),
                reason: format!("constraint {}: {}", diag.constraint, diag.reason),
                suggestion: diag.suggestion,
                details: vec![format!("杩濆弽璺緞: {}", diag.violation_path.join(" -> "))],
            }));

            SafetySummary {
                level: "澶辫触".to_string(),
                explored_depth: 0,
                warnings: Vec::new(),
                checked_rules: 0,
                skipped_rules: constraints.safety.len(),
                coverage: safety::SafetyCoverage {
                    bound_rules: 0,
                    degraded_rules: 0,
                    skipped_rules: constraints.safety.len(),
                    total_rules: constraints.safety.len(),
                },
                rule_statuses: Vec::new(),
            }
        }
        Err(issue) => {
            issues.push(issue);
            failed_safety_summary(constraints)
        }
    }
}

fn collect_liveness_result(
    liveness_result: Result<LivenessVerificationResult, VerificationIssue>,
    issues: &mut Vec<VerificationIssue>,
) {
    match liveness_result {
        Ok(Ok(())) => {}
        Ok(Err(diagnostics)) => {
            issues.extend(diagnostics.into_iter().map(|diag| VerificationIssue {
                checker: "liveness".to_string(),
                line: diag.line.max(1),
                reason: diag.reason,
                suggestion: diag.suggestion,
                details: vec![diag.physical_analysis],
            }));
        }
        Err(issue) => issues.push(issue),
    }
}

fn collect_timing_result(
    timing_result: Result<TimingVerificationResult, VerificationIssue>,
    issues: &mut Vec<VerificationIssue>,
) {
    match timing_result {
        Ok(Ok(())) => {}
        Ok(Err(diagnostics)) => {
            issues.extend(diagnostics.into_iter().map(|diag| VerificationIssue {
                checker: "timing".to_string(),
                line: diag.line.max(1),
                reason: format!("{}; {}", diag.constraint, diag.conclusion),
                suggestion: timing_suggestion(&diag.constraint),
                details: vec![diag.analysis],
            }));
        }
        Err(issue) => issues.push(issue),
    }
}

fn collect_causality_result(
    causality_result: Result<CausalityVerificationResult, VerificationIssue>,
    issues: &mut Vec<VerificationIssue>,
) {
    match causality_result {
        Ok(Ok(())) => {}
        Ok(Err(diagnostics)) => {
            issues.extend(diagnostics.into_iter().map(|diag| {
                let mut details = Vec::new();
                if let Some(action) = diag.action {
                    details.push(format!("鍔ㄤ綔: {action}"));
                }
                if let Some(wait) = diag.wait {
                    details.push(format!("绛夊緟: {wait}"));
                }
                details.push(format!("鏈熸湜閾捐矾: {}", diag.expected_chain));
                details.push(format!("瀹為檯閾捐矾: {}", diag.actual_chain));

                VerificationIssue {
                    checker: "causality".to_string(),
                    line: diag.line.max(1),
                    reason: format!("妫€娴嬪埌鍥犳灉閾炬柇瑁傦細{}", diag.broken_link),
                    suggestion: diag.suggestion,
                    details,
                }
            }));
        }
        Err(issue) => issues.push(issue),
    }
}

fn passed_liveness_summary(state_machine: &StateMachine) -> CheckerSummary {
    CheckerSummary {
        level: "閫氳繃".to_string(),
        warnings: Vec::new(),
        checked_rules: state_machine.states.len().max(1),
        skipped_rules: 0,
    }
}

fn passed_timing_summary(constraints: &ConstraintSet) -> CheckerSummary {
    CheckerSummary {
        level: "閫氳繃".to_string(),
        warnings: Vec::new(),
        checked_rules: constraints.timing.len(),
        skipped_rules: 0,
    }
}

fn passed_causality_summary(constraints: &ConstraintSet) -> CheckerSummary {
    CheckerSummary {
        level: "閫氳繃".to_string(),
        warnings: Vec::new(),
        checked_rules: constraints.causality.len(),
        skipped_rules: 0,
    }
}

fn station_protocol_summary(topology: &TopologyGraph) -> CheckerSummary {
    let protocol = &topology.station_protocol;
    let checked_rules = protocol.stations.len()
        + protocol.handshakes.len()
        + protocol.transfer_points.len()
        + protocol.controller_syncs.len();
    let controller_count = protocol.controllers.len();
    let mut warnings = Vec::new();

    if checked_rules == 0 && controller_count > 1 {
        warnings.push(WarningEntry {
            code: Some("STP-001".to_string()),
            level: WarningLevel::Warn,
            message: format!(
                "{controller_count} PLC controllers are declared without station protocol ownership, handshake, or transfer-point contracts"
            ),
        });
    }
    if controller_count > 1 && protocol.controller_syncs.is_empty() {
        warnings.push(WarningEntry {
            code: Some("STP-002".to_string()),
            level: WarningLevel::Warn,
            message: format!(
                "{controller_count} PLC controllers are declared without controller_sync timing contracts"
            ),
        });
    }
    if !protocol.controller_syncs.is_empty() {
        let covered = protocol
            .controller_syncs
            .iter()
            .flat_map(|sync| sync.controllers.iter().cloned())
            .collect::<std::collections::HashSet<_>>();
        let uncovered = protocol
            .controllers
            .iter()
            .filter(|controller| !covered.contains(*controller))
            .cloned()
            .collect::<Vec<_>>();
        if !uncovered.is_empty() {
            warnings.push(WarningEntry {
                code: Some("STP-003".to_string()),
                level: WarningLevel::Warn,
                message: format!(
                    "PLC controllers are not covered by any controller_sync timing contract: {}",
                    uncovered.join(", ")
                ),
            });
        }
    }

    CheckerSummary {
        level: "閫氳繃".to_string(),
        warnings,
        checked_rules,
        skipped_rules: usize::from(checked_rules == 0 && controller_count <= 1),
    }
}

fn default_station_protocol_summary() -> CheckerSummary {
    CheckerSummary {
        level: "legacy".to_string(),
        warnings: Vec::new(),
        checked_rules: 0,
        skipped_rules: 0,
    }
}

fn failed_safety_summary(constraints: &ConstraintSet) -> SafetySummary {
    SafetySummary {
        level: "澶辫触".to_string(),
        explored_depth: 0,
        warnings: Vec::new(),
        checked_rules: 0,
        skipped_rules: constraints.safety.len(),
        coverage: safety::SafetyCoverage {
            bound_rules: 0,
            degraded_rules: 0,
            skipped_rules: constraints.safety.len(),
            total_rules: constraints.safety.len(),
        },
        rule_statuses: Vec::new(),
    }
}

fn warning_entry(raw: &str) -> WarningEntry {
    let trimmed = raw.trim();
    if let Some(message) = trimmed.strip_prefix("ERROR:").map(str::trim) {
        return WarningEntry {
            code: None,
            level: WarningLevel::Error,
            message: message.to_string(),
        };
    }
    if let Some(message) = trimmed.strip_prefix("WARNING:").map(str::trim) {
        return WarningEntry {
            code: None,
            level: WarningLevel::Warn,
            message: message.to_string(),
        };
    }
    if let Some(message) = trimmed.strip_prefix("INFO:").map(str::trim) {
        return WarningEntry {
            code: None,
            level: WarningLevel::Info,
            message: message.to_string(),
        };
    }
    WarningEntry {
        code: None,
        level: WarningLevel::Info,
        message: trimmed.to_string(),
    }
}

fn timing_suggestion(constraint: &str) -> String {
    if constraint.contains("must_complete_within_worst_case") {
        "请放宽 must_complete_within_worst_case 阈值，或下调相关 step 的 timeout 上界".to_string()
    } else if constraint.contains("must_complete_within") {
        "请放宽 must_complete_within 阈值，或缩短动作响应/行程时间".to_string()
    } else {
        "请调整流程顺序、增加必要延时，或放宽 must_start_after 约束".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{WarningEntry, WarningLevel};

    #[test]
    fn warning_entry_deserializes_without_code_for_backward_compatibility() {
        let payload = r#"{"level":"warn","message":"legacy warning"}"#;
        let warning: WarningEntry =
            serde_json::from_str(payload).expect("legacy warning payload should deserialize");
        assert_eq!(warning.code, None);
        assert_eq!(warning.level, WarningLevel::Warn);
        assert_eq!(warning.message, "legacy warning");
    }

    #[test]
    fn warning_entry_deserializes_with_code() {
        let payload =
            r#"{"level":"warn","message":"migration warning","code":"MIG-AXIS-BLOCK-001"}"#;
        let warning: WarningEntry =
            serde_json::from_str(payload).expect("code-aware warning payload should deserialize");
        assert_eq!(warning.code.as_deref(), Some("MIG-AXIS-BLOCK-001"));
        assert_eq!(warning.level, WarningLevel::Warn);
    }
}
