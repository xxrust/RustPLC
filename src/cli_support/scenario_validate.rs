use crate::cli_support::common::{CliOutputMode, display_path_relative_to_cwd};
use crate::cli_support::scenario_init::{ScenarioInitInputHints, aliases_contain_keyword};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScenarioValidateSeverity {
    Error,
    Warn,
}

impl ScenarioValidateSeverity {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN",
        }
    }

    pub(crate) fn json_label(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ScenarioValidateFinding {
    pub(crate) severity: ScenarioValidateSeverity,
    pub(crate) tag: String,
    pub(crate) message: String,
    pub(crate) suggestion: Option<String>,
}

impl ScenarioValidateFinding {
    pub(crate) fn error(
        tag: impl Into<String>,
        message: impl Into<String>,
        suggestion: Option<String>,
    ) -> Self {
        Self {
            severity: ScenarioValidateSeverity::Error,
            tag: tag.into(),
            message: message.into(),
            suggestion,
        }
    }

    pub(crate) fn warn(
        tag: impl Into<String>,
        message: impl Into<String>,
        suggestion: Option<String>,
    ) -> Self {
        Self {
            severity: ScenarioValidateSeverity::Warn,
            tag: tag.into(),
            message: message.into(),
            suggestion,
        }
    }

    pub(crate) fn code(&self) -> &'static str {
        match self.tag.as_str() {
            "duration_ms" => "SCN-VAL-001",
            "runtime.probe" => "SCN-VAL-002",
            "risk.start_button_held" => "SCN-RISK-001",
            "risk.sensors_all_true_at_start" => "SCN-RISK-002",
            "risk.scenario_plc_mismatch" => "SCN-MAP-001",
            tag if tag.ends_with(".at_ms") => "SCN-TICK-001",
            tag if tag.contains("digital_inputs") => "SCN-MAP-002",
            tag if tag.contains("analog_inputs") => "SCN-MAP-003",
            tag if tag.contains("digital_outputs") => "SCN-MAP-004",
            tag if tag.contains("analog_outputs") => "SCN-MAP-005",
            _ => match self.severity {
                ScenarioValidateSeverity::Error => "SCN-VAL-999",
                ScenarioValidateSeverity::Warn => "SCN-RISK-999",
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct ScenarioValidateIssueJson<'a> {
    code: &'static str,
    severity: &'static str,
    tag: &'a str,
    message: &'a str,
    suggestion: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct ScenarioValidateJsonReport<'a> {
    schema_version: u32,
    command: &'static str,
    output: &'static str,
    status: &'static str,
    error_count: usize,
    warn_count: usize,
    issues: Vec<ScenarioValidateIssueJson<'a>>,
}

pub(crate) fn print_scenario_validate_findings(
    findings: &[ScenarioValidateFinding],
    output: CliOutputMode,
) {
    let errors = findings
        .iter()
        .filter(|finding| finding.severity == ScenarioValidateSeverity::Error)
        .count();
    let warnings = findings
        .iter()
        .filter(|finding| finding.severity == ScenarioValidateSeverity::Warn)
        .count();

    if output == CliOutputMode::Json {
        let report = ScenarioValidateJsonReport {
            schema_version: 1,
            command: "scenario-validate",
            output: output.as_str(),
            status: if errors == 0 { "pass" } else { "fail" },
            error_count: errors,
            warn_count: warnings,
            issues: findings
                .iter()
                .map(|finding| ScenarioValidateIssueJson {
                    code: finding.code(),
                    severity: finding.severity.json_label(),
                    tag: &finding.tag,
                    message: &finding.message,
                    suggestion: finding.suggestion.as_deref(),
                })
                .collect(),
        };
        match serde_json::to_string_pretty(&report) {
            Ok(mut body) => {
                body.push('\n');
                print!("{body}");
            }
            Err(err) => eprintln!("Failed to serialize scenario-validate JSON output: {err}"),
        }
        return;
    }

    if errors == 0 && warnings == 0 {
        eprintln!("scenario-validate: PASS (no issues)");
        return;
    }
    if errors == 0 {
        eprintln!("scenario-validate: PASS ({warnings} warning(s))");
    } else {
        eprintln!("scenario-validate: FAIL ({errors} error(s), {warnings} warning(s))");
    }

    for finding in findings {
        eprintln!(
            "{} [{}:{}] {}",
            finding.severity.label(),
            finding.code(),
            finding.tag,
            finding.message
        );
        if let Some(suggestion) = &finding.suggestion {
            eprintln!("  Fix:\n{suggestion}");
        }
    }
}

fn collect_scenario_referenced_inputs(
    scenario: &sim::Scenario,
) -> (Vec<(String, u16)>, Vec<(String, u16)>) {
    let mut digital = Vec::<(String, u16)>::new();
    let mut analog = Vec::<(String, u16)>::new();

    for (event_idx, event) in scenario.inputs.iter().enumerate() {
        for (&id, _) in &event.set.digital_inputs {
            digital.push((format!("inputs[{event_idx}].set.digital_inputs.{id}"), id));
        }
        for (&id, _) in &event.set.analog_inputs {
            analog.push((format!("inputs[{event_idx}].set.analog_inputs.{id}"), id));
        }
    }
    for (idx, burst) in scenario.digital_bursts.iter().enumerate() {
        digital.push((format!("digital_bursts[{idx}].target"), burst.target));
    }
    for (idx, fault) in scenario.faults.iter().enumerate() {
        digital.push((
            format!("faults[{idx}].sensor_stuck.target"),
            fault.sensor_stuck.target,
        ));
    }

    for (event_idx, force) in scenario.forces.iter().enumerate() {
        for (&id, _) in &force.set.digital_inputs {
            digital.push((format!("forces[{event_idx}].set.digital_inputs.{id}"), id));
        }
        for (&id, _) in &force.set.analog_inputs {
            analog.push((format!("forces[{event_idx}].set.analog_inputs.{id}"), id));
        }
    }

    (digital, analog)
}

pub(crate) fn collect_scenario_referenced_forced_outputs(
    scenario: &sim::Scenario,
) -> (Vec<(String, u16)>, Vec<(String, u16)>) {
    let mut digital = Vec::<(String, u16)>::new();
    let mut analog = Vec::<(String, u16)>::new();

    for (event_idx, force) in scenario.forces.iter().enumerate() {
        for (&id, _) in &force.set.digital_outputs {
            digital.push((format!("forces[{event_idx}].set.digital_outputs.{id}"), id));
        }
        for (&id, _) in &force.set.analog_outputs {
            analog.push((format!("forces[{event_idx}].set.analog_outputs.{id}"), id));
        }
    }

    (digital, analog)
}

fn collect_initial_digital_values(scenario: &sim::Scenario) -> BTreeMap<u16, bool> {
    let mut values = BTreeMap::<u16, bool>::new();
    for event in &scenario.inputs {
        if event.at_ms != 0 {
            continue;
        }
        for (&id, &value) in &event.set.digital_inputs {
            values.insert(id, value);
        }
    }

    for fault in &scenario.faults {
        if fault.sensor_stuck.at_ms != 0 {
            continue;
        }
        values.insert(fault.sensor_stuck.target, fault.sensor_stuck.value);
    }
    values
}

fn first_alias(aliases: &BTreeMap<u16, Vec<String>>, id: u16) -> Option<String> {
    aliases
        .get(&id)
        .and_then(|names| names.first())
        .map(|name| name.to_string())
}

fn has_later_digital_false(scenario: &sim::Scenario, id: u16) -> bool {
    scenario.inputs.iter().any(|event| {
        event.at_ms > 0
            && event
                .set
                .digital_inputs
                .get(&id)
                .copied()
                .map(|value| !value)
                .unwrap_or(false)
    }) || scenario.faults.iter().any(|fault| {
        fault.sensor_stuck.target == id && fault.sensor_stuck.at_ms > 0 && !fault.sensor_stuck.value
    })
}

pub(crate) fn validate_scenario_against_plc(
    plc_path: &Path,
    scenario_path: &Path,
    scenario: &sim::Scenario,
    hints: &ScenarioInitInputHints,
) -> Vec<ScenarioValidateFinding> {
    let mut findings = Vec::<ScenarioValidateFinding>::new();

    if scenario.duration_ms == 0 {
        findings.push(ScenarioValidateFinding::error(
            "duration_ms",
            "must be > 0",
            Some("duration_ms: 1000".to_string()),
        ));
    }

    let mut io = sim::SimIo::new(1, 1, 0, 0);
    if let Err(err) = scenario.apply_to_simio(&mut io) {
        if let sim::ScenarioError::Validation { path, message } = err {
            let tick_suggestion = if path.ends_with(".at_ms") {
                format!(
                    "Use multiples of tick_ms ({}), e.g. 0, {}, {}",
                    scenario.tick_ms,
                    scenario.tick_ms,
                    scenario.tick_ms.saturating_mul(2)
                )
            } else {
                "Check the scenario field value and retry".to_string()
            };
            findings.push(ScenarioValidateFinding::error(
                path,
                message,
                Some(tick_suggestion),
            ));
        }
    }

    let valid_di = if !hints.physical_digital_ids.is_empty() {
        hints
            .physical_digital_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
    } else {
        hints.digital_ids.iter().copied().collect::<BTreeSet<_>>()
    };
    let valid_ai = if !hints.physical_analog_ids.is_empty() {
        hints
            .physical_analog_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
    } else {
        hints.analog_ids.iter().copied().collect::<BTreeSet<_>>()
    };

    let (digital_refs, analog_refs) = collect_scenario_referenced_inputs(scenario);
    let plc_display = display_path_relative_to_cwd(plc_path);
    let scenario_display = display_path_relative_to_cwd(scenario_path);
    let skeleton_cmd = format!(
        "  rust_plc scenario-init {} --out {} --preset normal",
        plc_display, scenario_display
    );

    for (path, id) in digital_refs {
        if !valid_di.is_empty() && !valid_di.contains(&id) {
            let known = valid_di.iter().copied().collect::<Vec<_>>();
            let known_text = if known.is_empty() {
                "none".to_string()
            } else {
                known
                    .iter()
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            findings.push(ScenarioValidateFinding::error(
                path,
                format!("DI{id} does not exist in `{plc_display}` (known DI ids: {known_text})"),
                Some(format!(
                    "Regenerate a PLC-matched scenario skeleton:\n{skeleton_cmd}"
                )),
            ));
        }
    }
    for (path, id) in analog_refs {
        if !valid_ai.is_empty() && !valid_ai.contains(&id) {
            let known = valid_ai.iter().copied().collect::<Vec<_>>();
            let known_text = if known.is_empty() {
                "none".to_string()
            } else {
                known
                    .iter()
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            findings.push(ScenarioValidateFinding::error(
                path,
                format!("AI{id} does not exist in `{plc_display}` (known AI ids: {known_text})"),
                Some(format!(
                    "Regenerate a PLC-matched scenario skeleton:\n{skeleton_cmd}"
                )),
            ));
        }
    }

    let initial = collect_initial_digital_values(scenario);
    let mut start_ids = hints
        .digital_aliases
        .iter()
        .filter_map(|(&id, aliases)| {
            if aliases_contain_keyword(aliases, "start") {
                Some(id)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    start_ids.sort_unstable();
    start_ids.dedup();

    for id in start_ids {
        if initial.get(&id).copied().unwrap_or(false) && !has_later_digital_false(scenario, id) {
            let label = first_alias(&hints.digital_aliases, id)
                .map(|name| format!("{name} (DI{id})"))
                .unwrap_or_else(|| format!("DI{id}"));
            findings.push(ScenarioValidateFinding::warn(
                "risk.start_button_held",
                format!("{label} starts true and is never released; this can cause same-tick loops"),
                Some(format!(
                    "inputs:\n  - at_ms: 0\n    set:\n      digital_inputs:\n        {id}: true\n  - at_ms: {}\n    set:\n      digital_inputs:\n        {id}: false",
                    scenario.tick_ms
                )),
            ));
        }
    }

    let mut sensor_ids = hints
        .digital_aliases
        .iter()
        .filter_map(|(&id, aliases)| {
            if aliases_contain_keyword(aliases, "sensor") {
                Some(id)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    sensor_ids.sort_unstable();
    sensor_ids.dedup();

    if !sensor_ids.is_empty()
        && sensor_ids
            .iter()
            .all(|id| initial.get(id).copied().unwrap_or(false))
    {
        let preview = sensor_ids.iter().take(3).copied().collect::<Vec<_>>();
        let mut snippet = String::from("inputs:\n  - at_ms: 0\n    set:\n      digital_inputs:\n");
        for id in preview {
            snippet.push_str(&format!("        {id}: false\n"));
        }
        snippet.push_str("  # add later `at_ms` edges to set each sensor true when reached");
        findings.push(ScenarioValidateFinding::warn(
            "risk.sensors_all_true_at_start",
            "all known sensor inputs start true; waits/guards may be satisfied immediately"
                .to_string(),
            Some(snippet),
        ));
    }

    findings
}
