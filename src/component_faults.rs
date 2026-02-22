use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComponentFaultKind {
    Jammed,
    MotionTimeout,
    StuckOn,
    StuckOff,
    Chatter,
    LostStep,
    Stall,
    DirectionReversed,
}

impl ComponentFaultKind {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "jammed" => Some(Self::Jammed),
            "motion_timeout" => Some(Self::MotionTimeout),
            "stuck_on" => Some(Self::StuckOn),
            "stuck_off" => Some(Self::StuckOff),
            "chatter" => Some(Self::Chatter),
            "lost_step" => Some(Self::LostStep),
            "stall" => Some(Self::Stall),
            "direction_reversed" => Some(Self::DirectionReversed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComponentFaultEvent {
    pub at_ms: u64,
    pub duration_ms: Option<u64>,
    pub target_component_id: String,
    pub fault_kind: ComponentFaultKind,
    pub params: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentFaultIssue {
    pub code: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("component fault validation failed")]
pub struct ComponentFaultValidationError {
    pub issues: Vec<ComponentFaultIssue>,
}

pub fn parse_component_fault_events_json(
    input: &str,
) -> Result<Vec<ComponentFaultEvent>, ComponentFaultValidationError> {
    let value =
        serde_json::from_str::<Value>(input).map_err(|err| ComponentFaultValidationError {
            issues: vec![fault_issue(
                "CFLT-PARSE-001",
                "$",
                format!("invalid JSON: {err}"),
            )],
        })?;
    parse_component_fault_events_value(&value)
}

pub fn parse_component_fault_events_value(
    value: &Value,
) -> Result<Vec<ComponentFaultEvent>, ComponentFaultValidationError> {
    let Some(items) = value.as_array() else {
        return Err(ComponentFaultValidationError {
            issues: vec![fault_issue(
                "CFLT-PARSE-002",
                "$",
                "fault list root must be an array",
            )],
        });
    };

    let mut out = Vec::new();
    let mut issues = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        let path = format!("$[{idx}]");
        let Some(obj) = item.as_object() else {
            issues.push(fault_issue(
                "CFLT-001",
                path,
                "fault entry must be an object",
            ));
            continue;
        };

        let Some(at_ms) = obj.get("at_ms").and_then(Value::as_u64).or_else(|| {
            issues.push(fault_issue(
                "CFLT-002",
                format!("$[{idx}].at_ms"),
                "at_ms must be an unsigned integer",
            ));
            None
        }) else {
            continue;
        };

        let duration_ms = match obj.get("duration_ms") {
            None => None,
            Some(v) => match v.as_u64() {
                Some(n) if n > 0 => Some(n),
                Some(_) => {
                    issues.push(fault_issue(
                        "CFLT-003",
                        format!("$[{idx}].duration_ms"),
                        "duration_ms must be >= 1 when provided",
                    ));
                    continue;
                }
                None => {
                    issues.push(fault_issue(
                        "CFLT-003",
                        format!("$[{idx}].duration_ms"),
                        "duration_ms must be an unsigned integer when provided",
                    ));
                    continue;
                }
            },
        };

        let Some(target_component_id) = obj
            .get("target_component_id")
            .and_then(Value::as_str)
            .filter(|v| !v.trim().is_empty())
            .map(|v| v.to_string())
            .or_else(|| {
                issues.push(fault_issue(
                    "CFLT-004",
                    format!("$[{idx}].target_component_id"),
                    "target_component_id must be a non-empty string",
                ));
                None
            })
        else {
            continue;
        };

        let Some(raw_kind) = obj
            .get("fault_kind")
            .and_then(Value::as_str)
            .filter(|v| !v.trim().is_empty())
            .map(|v| v.to_string())
            .or_else(|| {
                issues.push(fault_issue(
                    "CFLT-005",
                    format!("$[{idx}].fault_kind"),
                    "fault_kind must be a non-empty string",
                ));
                None
            })
        else {
            continue;
        };
        let Some(fault_kind) = ComponentFaultKind::parse(&raw_kind) else {
            issues.push(fault_issue(
                "CFLT-006",
                format!("$[{idx}].fault_kind"),
                format!(
                    "unsupported fault_kind `{raw_kind}` (expected one of jammed|motion_timeout|stuck_on|stuck_off|chatter|lost_step|stall|direction_reversed)"
                ),
            ));
            continue;
        };

        let params = match obj.get("params") {
            None => BTreeMap::new(),
            Some(v) => match v.as_object() {
                Some(map) => map
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<BTreeMap<_, _>>(),
                None => {
                    issues.push(fault_issue(
                        "CFLT-007",
                        format!("$[{idx}].params"),
                        "params must be an object when provided",
                    ));
                    continue;
                }
            },
        };

        validate_fault_params(idx, fault_kind, &params, &mut issues);
        out.push(ComponentFaultEvent {
            at_ms,
            duration_ms,
            target_component_id,
            fault_kind,
            params,
        });
    }

    if !issues.is_empty() {
        return Err(ComponentFaultValidationError { issues });
    }
    Ok(out)
}

fn validate_fault_params(
    index: usize,
    fault_kind: ComponentFaultKind,
    params: &BTreeMap<String, Value>,
    issues: &mut Vec<ComponentFaultIssue>,
) {
    match fault_kind {
        ComponentFaultKind::MotionTimeout => {
            let Some(timeout_ms) = params.get("timeout_ms").and_then(Value::as_u64) else {
                issues.push(fault_issue(
                    "CFLT-010",
                    format!("$[{index}].params.timeout_ms"),
                    "motion_timeout requires params.timeout_ms (u64 >= 1)",
                ));
                return;
            };
            if timeout_ms == 0 {
                issues.push(fault_issue(
                    "CFLT-010",
                    format!("$[{index}].params.timeout_ms"),
                    "motion_timeout requires params.timeout_ms (u64 >= 1)",
                ));
            }
        }
        ComponentFaultKind::Chatter => {
            let period_ms = params.get("period_ms").and_then(Value::as_u64);
            let duty_percent = params.get("duty_percent").and_then(Value::as_u64);
            match (period_ms, duty_percent) {
                (Some(period), Some(duty)) if period > 0 && (1..=99).contains(&duty) => {}
                _ => issues.push(fault_issue(
                    "CFLT-011",
                    format!("$[{index}].params"),
                    "chatter requires period_ms>=1 and duty_percent in 1..=99",
                )),
            }
        }
        ComponentFaultKind::LostStep => {
            let Some(ratio) = params.get("ratio").and_then(Value::as_f64) else {
                issues.push(fault_issue(
                    "CFLT-012",
                    format!("$[{index}].params.ratio"),
                    "lost_step requires ratio in (0.0, 1.0]",
                ));
                return;
            };
            if !(ratio > 0.0 && ratio <= 1.0) {
                issues.push(fault_issue(
                    "CFLT-012",
                    format!("$[{index}].params.ratio"),
                    "lost_step requires ratio in (0.0, 1.0]",
                ));
            }
        }
        ComponentFaultKind::Jammed
        | ComponentFaultKind::StuckOn
        | ComponentFaultKind::StuckOff
        | ComponentFaultKind::Stall
        | ComponentFaultKind::DirectionReversed => {}
    }
}

fn fault_issue(
    code: impl Into<String>,
    path: impl Into<String>,
    message: impl Into<String>,
) -> ComponentFaultIssue {
    ComponentFaultIssue {
        code: code.into(),
        path: path.into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_component_specific_fault_kinds() {
        let faults = parse_component_fault_events_json(
            r#"[
  { "at_ms": 100, "target_component_id": "cyl_a", "fault_kind": "jammed" },
  { "at_ms": 120, "target_component_id": "x0", "fault_kind": "stuck_on" },
  { "at_ms": 140, "target_component_id": "sw0", "fault_kind": "stuck_off" },
  { "at_ms": 160, "target_component_id": "m1", "fault_kind": "stall" },
  { "at_ms": 180, "target_component_id": "m1", "fault_kind": "direction_reversed" },
  { "at_ms": 200, "target_component_id": "m1", "fault_kind": "lost_step", "params": { "ratio": 0.25 } },
  { "at_ms": 220, "target_component_id": "x0", "fault_kind": "chatter", "params": { "period_ms": 4, "duty_percent": 50 } },
  { "at_ms": 240, "target_component_id": "cyl_a", "fault_kind": "motion_timeout", "params": { "timeout_ms": 30 } }
]"#,
        )
        .expect("faults should parse");
        assert_eq!(faults.len(), 8);
    }

    #[test]
    fn rejects_invalid_lost_step_ratio_with_stable_code() {
        let err = parse_component_fault_events_json(
            r#"[
  { "at_ms": 100, "target_component_id": "m1", "fault_kind": "lost_step", "params": { "ratio": 2.0 } }
]"#,
        )
        .expect_err("invalid lost_step ratio should fail");
        assert!(
            err.issues
                .iter()
                .any(|issue| issue.code == "CFLT-012" && issue.path == "$[0].params.ratio")
        );
    }

    #[test]
    fn rejects_invalid_chatter_params_with_stable_code() {
        let err = parse_component_fault_events_json(
            r#"[
  { "at_ms": 100, "target_component_id": "x0", "fault_kind": "chatter", "params": { "period_ms": 0, "duty_percent": 120 } }
]"#,
        )
        .expect_err("invalid chatter params should fail");
        assert!(
            err.issues
                .iter()
                .any(|issue| issue.code == "CFLT-011" && issue.path == "$[0].params")
        );
    }
}
