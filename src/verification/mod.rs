pub mod causality;
pub mod liveness;
pub mod safety;
pub mod timing;

use crate::ast::PlcProgram;
use crate::ir::{ConstraintSet, StateMachine, TopologyGraph};
use serde::{Deserialize, Serialize};
use std::fmt;

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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CheckerSummary {
    pub level: String,
    pub warnings: Vec<WarningEntry>,
    pub checked_rules: usize,
    pub skipped_rules: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SafetySummary {
    pub level: String,
    pub explored_depth: usize,
    pub warnings: Vec<WarningEntry>,
    pub checked_rules: usize,
    pub skipped_rules: usize,
    pub coverage: safety::SafetyCoverage,
    pub rule_statuses: Vec<safety::SafetyRuleStatus>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct VerificationSummary {
    pub safety: SafetySummary,
    pub liveness: CheckerSummary,
    pub timing: CheckerSummary,
    pub causality: CheckerSummary,
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

pub fn verify_all(
    program: &PlcProgram,
    topology: &TopologyGraph,
    constraints: &ConstraintSet,
    state_machine: &StateMachine,
) -> Result<VerificationSummary, Vec<VerificationIssue>> {
    let mut issues = Vec::new();

    let safety_summary = match safety::verify_safety(program, constraints, state_machine) {
        Ok(report) => {
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
        Err(diagnostics) => {
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
    };

    if let Err(diagnostics) = liveness::verify_liveness(program, state_machine) {
        issues.extend(diagnostics.into_iter().map(|diag| VerificationIssue {
            checker: "liveness".to_string(),
            line: diag.line.max(1),
            reason: diag.reason,
            suggestion: diag.suggestion,
            details: vec![diag.physical_analysis],
        }));
    }

    if let Err(diagnostics) = timing::verify_timing(program, topology, constraints, state_machine) {
        issues.extend(diagnostics.into_iter().map(|diag| VerificationIssue {
            checker: "timing".to_string(),
            line: diag.line.max(1),
            reason: format!("{}；{}", diag.constraint, diag.conclusion),
            suggestion: timing_suggestion(&diag.constraint),
            details: vec![diag.analysis],
        }));
    }

    if let Err(diagnostics) = causality::verify_causality(program, topology, constraints) {
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
    })
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
        let payload = r#"{"level":"warn","message":"migration warning","code":"MIG-AXIS-BLOCK-001"}"#;
        let warning: WarningEntry =
            serde_json::from_str(payload).expect("code-aware warning payload should deserialize");
        assert_eq!(warning.code.as_deref(), Some("MIG-AXIS-BLOCK-001"));
        assert_eq!(warning.level, WarningLevel::Warn);
    }
}
