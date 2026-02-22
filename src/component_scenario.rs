use crate::component_faults::{
    ComponentFaultEvent, ComponentFaultIssue, ComponentFaultValidationError,
    parse_component_fault_events_value,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentSwitchEvent {
    pub at_ms: u64,
    pub target: String,
    pub value: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentSensorEvent {
    pub at_ms: u64,
    pub target: String,
    pub value: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComponentScenario {
    pub schema_version: u32,
    pub tick_ms: u64,
    pub duration_ms: u64,
    #[serde(default)]
    pub switch_events: Vec<ComponentSwitchEvent>,
    #[serde(default)]
    pub sensor_events: Vec<ComponentSensorEvent>,
    #[serde(default)]
    pub component_faults: Vec<ComponentFaultEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentScenarioIssue {
    pub code: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("component scenario validation failed")]
pub struct ComponentScenarioValidationError {
    pub issues: Vec<ComponentScenarioIssue>,
}

pub fn parse_component_scenario_json(
    input: &str,
) -> Result<ComponentScenario, ComponentScenarioValidationError> {
    let value =
        serde_json::from_str::<Value>(input).map_err(|err| ComponentScenarioValidationError {
            issues: vec![issue("CSCN-PARSE-001", "$", format!("invalid JSON: {err}"))],
        })?;
    parse_component_scenario_value(&value)
}

pub fn parse_component_scenario_value(
    value: &Value,
) -> Result<ComponentScenario, ComponentScenarioValidationError> {
    let Some(root) = value.as_object() else {
        return Err(ComponentScenarioValidationError {
            issues: vec![issue("CSCN-PARSE-002", "$", "root must be an object")],
        });
    };

    let mut issues = Vec::new();
    if root.contains_key("faults") {
        issues.push(issue(
            "CSCN-MIG-001",
            "$.faults",
            "legacy `faults.sensor_stuck` is no longer accepted; migrate to `component_faults` with `fault_kind: stuck_on|stuck_off|...`",
        ));
    }
    if root.contains_key("forces") {
        issues.push(issue(
            "CSCN-MIG-002",
            "$.forces",
            "legacy `forces` is no longer accepted in component scenario; model actuator/signal anomalies via `component_faults`",
        ));
    }

    let schema_version = match root.get("schema_version").and_then(Value::as_u64) {
        Some(1) => 1,
        Some(other) => {
            issues.push(issue(
                "CSCN-SCHEMA-003",
                "$.schema_version",
                format!("unsupported schema_version `{other}` (expected `1`)"),
            ));
            0
        }
        None => {
            issues.push(issue(
                "CSCN-SCHEMA-001",
                "$.schema_version",
                "schema_version must be integer `1`",
            ));
            0
        }
    };

    let tick_ms = match root.get("tick_ms").and_then(Value::as_u64) {
        Some(v) if v > 0 => v,
        _ => {
            issues.push(issue(
                "CSCN-TIME-001",
                "$.tick_ms",
                "tick_ms must be an unsigned integer >= 1",
            ));
            0
        }
    };

    let duration_ms = match root.get("duration_ms").and_then(Value::as_u64) {
        Some(v) if v >= tick_ms && tick_ms > 0 => v,
        _ => {
            issues.push(issue(
                "CSCN-TIME-002",
                "$.duration_ms",
                "duration_ms must be >= tick_ms",
            ));
            0
        }
    };
    if tick_ms > 0 && duration_ms > 0 && duration_ms % tick_ms != 0 {
        issues.push(issue(
            "CSCN-TIME-003",
            "$.duration_ms",
            format!("duration_ms ({duration_ms}) must align to tick_ms ({tick_ms})"),
        ));
    }

    let mut switch_events =
        parse_bool_events(root.get("switch_events"), "switch_events", &mut issues)
            .into_iter()
            .map(|event| ComponentSwitchEvent {
                at_ms: event.at_ms,
                target: event.target,
                value: event.value,
            })
            .collect::<Vec<_>>();
    let mut sensor_events =
        parse_bool_events(root.get("sensor_events"), "sensor_events", &mut issues)
            .into_iter()
            .map(|event| ComponentSensorEvent {
                at_ms: event.at_ms,
                target: event.target,
                value: event.value,
            })
            .collect::<Vec<_>>();
    let mut component_faults = match root.get("component_faults") {
        None => Vec::new(),
        Some(v) => match parse_component_fault_events_value(v) {
            Ok(items) => items,
            Err(err) => {
                issues.extend(fault_issues_to_scenario(err));
                Vec::new()
            }
        },
    };

    if duration_ms > 0 {
        for (idx, event) in switch_events.iter().enumerate() {
            if event.at_ms >= duration_ms {
                issues.push(issue(
                    "CSCN-EVT-003",
                    format!("$.switch_events[{idx}].at_ms"),
                    format!(
                        "switch event at_ms ({}) must be < duration_ms ({duration_ms})",
                        event.at_ms
                    ),
                ));
            }
            if tick_ms > 0 && event.at_ms % tick_ms != 0 {
                issues.push(issue(
                    "CSCN-EVT-004",
                    format!("$.switch_events[{idx}].at_ms"),
                    format!(
                        "switch event at_ms ({}) must align to tick_ms ({tick_ms})",
                        event.at_ms
                    ),
                ));
            }
        }
        for (idx, event) in sensor_events.iter().enumerate() {
            if event.at_ms >= duration_ms {
                issues.push(issue(
                    "CSCN-EVT-005",
                    format!("$.sensor_events[{idx}].at_ms"),
                    format!(
                        "sensor event at_ms ({}) must be < duration_ms ({duration_ms})",
                        event.at_ms
                    ),
                ));
            }
            if tick_ms > 0 && event.at_ms % tick_ms != 0 {
                issues.push(issue(
                    "CSCN-EVT-006",
                    format!("$.sensor_events[{idx}].at_ms"),
                    format!(
                        "sensor event at_ms ({}) must align to tick_ms ({tick_ms})",
                        event.at_ms
                    ),
                ));
            }
        }
        for (idx, event) in component_faults.iter().enumerate() {
            if event.at_ms >= duration_ms {
                issues.push(issue(
                    "CSCN-EVT-007",
                    format!("$.component_faults[{idx}].at_ms"),
                    format!(
                        "fault event at_ms ({}) must be < duration_ms ({duration_ms})",
                        event.at_ms
                    ),
                ));
            }
            if tick_ms > 0 && event.at_ms % tick_ms != 0 {
                issues.push(issue(
                    "CSCN-EVT-008",
                    format!("$.component_faults[{idx}].at_ms"),
                    format!(
                        "fault event at_ms ({}) must align to tick_ms ({tick_ms})",
                        event.at_ms
                    ),
                ));
            }
        }
    }

    if !issues.is_empty() {
        return Err(ComponentScenarioValidationError { issues });
    }

    switch_events.sort_by_key(|event| event.at_ms);
    sensor_events.sort_by_key(|event| event.at_ms);
    component_faults.sort_by_key(|event| event.at_ms);

    Ok(ComponentScenario {
        schema_version,
        tick_ms,
        duration_ms,
        switch_events,
        sensor_events,
        component_faults,
    })
}

pub fn write_component_scenario_json(
    path: &Path,
    scenario: &ComponentScenario,
) -> Result<(), String> {
    let mut body = serde_json::to_string_pretty(scenario)
        .map_err(|err| format!("Failed to serialize component scenario JSON: {err}"))?;
    body.push('\n');
    std::fs::write(path, body).map_err(|err| {
        format!(
            "Failed to write component scenario {}: {err}",
            path.display()
        )
    })
}

#[derive(Debug)]
struct BoolEvent {
    at_ms: u64,
    target: String,
    value: bool,
}

fn parse_bool_events(
    raw: Option<&Value>,
    field: &str,
    issues: &mut Vec<ComponentScenarioIssue>,
) -> Vec<BoolEvent> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    let Some(items) = raw.as_array() else {
        issues.push(issue(
            "CSCN-EVT-001",
            format!("$.{field}"),
            format!("{field} must be an array"),
        ));
        return Vec::new();
    };

    let mut out = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        let Some(obj) = item.as_object() else {
            issues.push(issue(
                "CSCN-EVT-002",
                format!("$.{field}[{idx}]"),
                "event entry must be an object",
            ));
            continue;
        };
        let Some(at_ms) = obj.get("at_ms").and_then(Value::as_u64) else {
            issues.push(issue(
                "CSCN-EVT-002",
                format!("$.{field}[{idx}].at_ms"),
                "at_ms must be u64",
            ));
            continue;
        };
        let Some(target) = obj
            .get("target")
            .and_then(Value::as_str)
            .filter(|v| !v.trim().is_empty())
            .map(|v| v.to_string())
        else {
            issues.push(issue(
                "CSCN-EVT-002",
                format!("$.{field}[{idx}].target"),
                "target must be non-empty string",
            ));
            continue;
        };
        let Some(value) = obj.get("value").and_then(Value::as_bool) else {
            issues.push(issue(
                "CSCN-EVT-002",
                format!("$.{field}[{idx}].value"),
                "value must be bool",
            ));
            continue;
        };
        out.push(BoolEvent {
            at_ms,
            target,
            value,
        });
    }
    out
}

fn fault_issues_to_scenario(err: ComponentFaultValidationError) -> Vec<ComponentScenarioIssue> {
    err.issues
        .into_iter()
        .map(
            |ComponentFaultIssue {
                 code,
                 path,
                 message,
             }| ComponentScenarioIssue {
                code: format!("CSCN-FLT::{code}"),
                path: format!("$.component_faults{}", path.trim_start_matches('$')),
                message,
            },
        )
        .collect()
}

fn issue(
    code: impl Into<String>,
    path: impl Into<String>,
    message: impl Into<String>,
) -> ComponentScenarioIssue {
    ComponentScenarioIssue {
        code: code.into(),
        path: path.into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_legacy_faults_and_forces_with_migration_hints() {
        let err = parse_component_scenario_json(
            r#"{
  "schema_version": 1,
  "tick_ms": 10,
  "duration_ms": 100,
  "faults": [ { "sensor_stuck": { "at_ms": 10, "target": 0, "value": true } } ],
  "forces": []
}"#,
        )
        .expect_err("legacy fields should fail");
        assert!(err.issues.iter().any(|issue| issue.code == "CSCN-MIG-001"));
        assert!(err.issues.iter().any(|issue| issue.code == "CSCN-MIG-002"));
    }

    #[test]
    fn parses_component_scenario_with_faults() {
        let scenario = parse_component_scenario_json(
            r#"{
  "schema_version": 1,
  "tick_ms": 10,
  "duration_ms": 100,
  "switch_events": [{ "at_ms": 0, "target": "s0", "value": true }],
  "sensor_events": [{ "at_ms": 20, "target": "x0", "value": true }],
  "component_faults": [
    { "at_ms": 40, "target_component_id": "m1", "fault_kind": "stall" }
  ]
}"#,
        )
        .expect("scenario should parse");
        assert_eq!(scenario.switch_events.len(), 1);
        assert_eq!(scenario.sensor_events.len(), 1);
        assert_eq!(scenario.component_faults.len(), 1);
    }
}
