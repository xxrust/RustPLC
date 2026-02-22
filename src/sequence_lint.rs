use crate::ast::{PlcProgram, StepStatement};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintLevel {
    Warn,
    Error,
}

impl LintLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }
}

impl FromStr for LintLevel {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "warn" => Ok(Self::Warn),
            "error" => Ok(Self::Error),
            _ => Err(format!(
                "invalid lint level `{value}`, expected `warn` or `error`"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CriticalWaitExemption {
    TaskStep { task: String, step: String },
    TaskAll { task: String },
}

impl CriticalWaitExemption {
    pub fn parse(spec: &str) -> Result<Self, String> {
        let (task, step) = spec
            .split_once('.')
            .ok_or_else(|| "expected <task>.<step> or <task>.*".to_string())?;
        let task = task.trim();
        let step = step.trim();
        if task.is_empty() || step.is_empty() {
            return Err("task/step cannot be empty".to_string());
        }
        if step == "*" {
            return Ok(Self::TaskAll {
                task: task.to_string(),
            });
        }
        Ok(Self::TaskStep {
            task: task.to_string(),
            step: step.to_string(),
        })
    }

    fn matches(&self, task: &str, step: &str) -> bool {
        match self {
            Self::TaskStep {
                task: exempt_task,
                step: exempt_step,
            } => exempt_task == task && exempt_step == step,
            Self::TaskAll { task: exempt_task } => exempt_task == task,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SequenceLintConfig {
    pub critical_wait_level: LintLevel,
    pub critical_wait_exemptions: Vec<CriticalWaitExemption>,
}

impl Default for SequenceLintConfig {
    fn default() -> Self {
        Self {
            critical_wait_level: LintLevel::Warn,
            critical_wait_exemptions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceLintFinding {
    pub level: LintLevel,
    pub line: usize,
    pub task: String,
    pub step: String,
    pub message: String,
    pub suggestion: String,
}

impl fmt::Display for SequenceLintFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{} [sequence-lint] <input>:{}:1",
            self.level.label(),
            self.line.max(1)
        )?;
        writeln!(f, "  规则: critical_wait_recovery")?;
        writeln!(f, "  位置: {}.{}", self.task, self.step)?;
        writeln!(f, "  原因: {}", self.message)?;
        write!(f, "  建议: {}", self.suggestion)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct WaitFacts {
    has_wait: bool,
    has_timeout: bool,
    has_allow_indefinite_wait: bool,
}

pub fn lint_critical_wait_recovery(
    program: &PlcProgram,
    config: &SequenceLintConfig,
) -> Vec<SequenceLintFinding> {
    let mut findings = Vec::new();

    for task in &program.tasks.tasks {
        for step in &task.steps {
            let mut facts = WaitFacts::default();
            collect_wait_facts(&step.statements, &mut facts);

            if !facts.has_wait {
                continue;
            }
            if facts.has_timeout || facts.has_allow_indefinite_wait {
                continue;
            }
            if config
                .critical_wait_exemptions
                .iter()
                .any(|item| item.matches(&task.name, &step.name))
            {
                continue;
            }

            findings.push(SequenceLintFinding {
                level: config.critical_wait_level,
                line: step.line.max(task.line).max(1),
                task: task.name.clone(),
                step: step.name.clone(),
                message: format!(
                    "关键路径 wait 缺少 timeout+goto，当前 step `{}` 仅声明了 wait",
                    step.name
                ),
                suggestion:
                    "请添加 `timeout: <时长> -> goto <恢复 task>`；若为人工等待点，可设置 `allow_indefinite_wait: true` 或通过 --critical-wait-exempt 豁免"
                        .to_string(),
            });
        }
    }

    findings
}

fn collect_wait_facts(statements: &[StepStatement], facts: &mut WaitFacts) {
    for statement in statements {
        match statement {
            StepStatement::Wait(_) => {
                facts.has_wait = true;
            }
            StepStatement::Timeout(_) => {
                facts.has_timeout = true;
            }
            StepStatement::AllowIndefiniteWait(value) => {
                if *value {
                    facts.has_allow_indefinite_wait = true;
                }
            }
            StepStatement::Repeat { body, .. } => collect_wait_facts(body, facts),
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    collect_wait_facts(&branch.statements, facts);
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    collect_wait_facts(&branch.statements, facts);
                }
            }
            StepStatement::Action(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Goto(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CriticalWaitExemption, LintLevel, SequenceLintConfig, lint_critical_wait_recovery,
    };
    use crate::parser::parse_plc;
    use crate::semantic::preprocess_program;

    fn lint(source: &str, config: SequenceLintConfig) -> Vec<String> {
        let parsed = parse_plc(source).expect("fixture should parse");
        let expanded = preprocess_program(&parsed).expect("fixture should preprocess");
        lint_critical_wait_recovery(&expanded, &config)
            .into_iter()
            .map(|f| f.to_string())
            .collect::<Vec<_>>()
    }

    #[test]
    fn reports_wait_without_timeout_on_critical_path() {
        let source = r#"
[topology]

[constraints]

[tasks]
task main:
    step wait_sensor:
        wait: X0 == true
"#;
        let findings = lint(source, SequenceLintConfig::default());
        assert_eq!(findings.len(), 1);
        assert!(findings[0].contains("critical_wait_recovery"));
        assert!(findings[0].contains("缺少 timeout+goto"));
    }

    #[test]
    fn allow_indefinite_wait_is_treated_as_exemption() {
        let source = r#"
[topology]

[constraints]

[tasks]
task ready:
    step wait_start:
        wait: X0 == true
        allow_indefinite_wait: true
"#;
        let findings = lint(source, SequenceLintConfig::default());
        assert!(
            findings.is_empty(),
            "manual wait with allow_indefinite_wait should be exempt"
        );
    }

    #[test]
    fn cli_exemption_suppresses_specific_step() {
        let source = r#"
[topology]

[constraints]

[tasks]
task main:
    step wait_sensor:
        wait: X0 == true
"#;
        let findings = lint(
            source,
            SequenceLintConfig {
                critical_wait_level: LintLevel::Error,
                critical_wait_exemptions: vec![
                    CriticalWaitExemption::parse("main.wait_sensor").expect("valid exemption"),
                ],
            },
        );
        assert!(
            findings.is_empty(),
            "exempt step should not produce finding"
        );
    }
}
