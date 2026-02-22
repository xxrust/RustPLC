use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComponentType {
    Cylinder,
    Sensor,
    Switch,
    StepperPd,
}

impl ComponentType {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "cylinder" => Some(Self::Cylinder),
            "sensor" => Some(Self::Sensor),
            "switch" => Some(Self::Switch),
            // UI-side shorthand alias; normalized to `stepper_pd` in serialized output.
            "stepper" => Some(Self::StepperPd),
            "stepper_pd" => Some(Self::StepperPd),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComponentDefinition {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub component_type: ComponentType,
    pub params: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComponentLibrary {
    pub schema_version: u32,
    pub components: Vec<ComponentDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentLibraryIssue {
    pub code: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("component library validation failed")]
pub struct ComponentLibraryValidationError {
    pub issues: Vec<ComponentLibraryIssue>,
}

pub fn parse_component_library_json(
    input: &str,
) -> Result<ComponentLibrary, ComponentLibraryValidationError> {
    let value =
        serde_json::from_str::<Value>(input).map_err(|err| ComponentLibraryValidationError {
            issues: vec![issue("CLIB-PARSE-001", "$", format!("invalid JSON: {err}"))],
        })?;
    parse_component_library_value(&value)
}

pub fn parse_component_library_value(
    value: &Value,
) -> Result<ComponentLibrary, ComponentLibraryValidationError> {
    let mut issues = Vec::new();
    let Some(root) = value.as_object() else {
        return Err(ComponentLibraryValidationError {
            issues: vec![issue("CLIB-PARSE-002", "$", "root must be an object")],
        });
    };

    let schema_version = match root.get("schema_version") {
        None => {
            issues.push(issue(
                "CLIB-SCHEMA-001",
                "$.schema_version",
                "missing required field `schema_version`",
            ));
            0
        }
        Some(v) => match v.as_u64() {
            Some(1) => 1,
            Some(other) => {
                issues.push(issue(
                    "CLIB-SCHEMA-003",
                    "$.schema_version",
                    format!("unsupported schema_version `{other}` (expected `1`)"),
                ));
                0
            }
            None => {
                issues.push(issue(
                    "CLIB-SCHEMA-002",
                    "$.schema_version",
                    "schema_version must be an integer",
                ));
                0
            }
        },
    };

    let Some(raw_components) = root.get("components") else {
        issues.push(issue(
            "CLIB-COMP-001",
            "$.components",
            "missing required field `components`",
        ));
        return Err(ComponentLibraryValidationError { issues });
    };

    let Some(component_array) = raw_components.as_array() else {
        issues.push(issue(
            "CLIB-COMP-002",
            "$.components",
            "components must be an array",
        ));
        return Err(ComponentLibraryValidationError { issues });
    };

    let mut components = Vec::new();
    let mut seen_ids = BTreeSet::<String>::new();
    for (idx, entry) in component_array.iter().enumerate() {
        let path = format!("$.components[{idx}]");
        let Some(obj) = entry.as_object() else {
            issues.push(issue(
                "CLIB-COMP-003",
                path,
                "component entry must be an object",
            ));
            continue;
        };

        let id = match read_non_empty_string(
            obj,
            "id",
            idx,
            &mut issues,
            "CLIB-COMP-004",
            "CLIB-COMP-005",
        ) {
            Some(v) => v,
            None => continue,
        };
        let name = match read_non_empty_string(
            obj,
            "name",
            idx,
            &mut issues,
            "CLIB-COMP-007",
            "CLIB-COMP-008",
        ) {
            Some(v) => v,
            None => continue,
        };
        if !seen_ids.insert(id.clone()) {
            issues.push(issue(
                "CLIB-COMP-006",
                format!("$.components[{idx}].id"),
                format!("duplicate component id `{id}`"),
            ));
            continue;
        }

        let raw_type = match read_non_empty_string(
            obj,
            "type",
            idx,
            &mut issues,
            "CLIB-COMP-009",
            "CLIB-COMP-010",
        ) {
            Some(v) => v,
            None => continue,
        };
        let Some(component_type) = ComponentType::parse(&raw_type) else {
            issues.push(issue(
                "CLIB-COMP-010",
                format!("$.components[{idx}].type"),
                format!(
                    "unsupported component type `{raw_type}` (expected one of `cylinder|sensor|switch|stepper|stepper_pd`)"
                ),
            ));
            continue;
        };

        let params = match obj.get("params") {
            None => {
                issues.push(issue(
                    "CLIB-COMP-011",
                    format!("$.components[{idx}].params"),
                    "missing required field `params`",
                ));
                continue;
            }
            Some(v) => match v.as_object() {
                Some(map) => map
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<BTreeMap<_, _>>(),
                None => {
                    issues.push(issue(
                        "CLIB-COMP-012",
                        format!("$.components[{idx}].params"),
                        "params must be an object",
                    ));
                    continue;
                }
            },
        };

        components.push(ComponentDefinition {
            id,
            name,
            component_type,
            params,
        });
    }

    if !issues.is_empty() {
        return Err(ComponentLibraryValidationError { issues });
    }

    Ok(ComponentLibrary {
        schema_version,
        components,
    })
}

fn read_non_empty_string(
    obj: &Map<String, Value>,
    field: &str,
    index: usize,
    issues: &mut Vec<ComponentLibraryIssue>,
    missing_code: &str,
    invalid_code: &str,
) -> Option<String> {
    let path = format!("$.components[{index}].{field}");
    let Some(value) = obj.get(field) else {
        issues.push(issue(
            missing_code,
            path,
            format!("missing required field `{field}`"),
        ));
        return None;
    };
    let Some(raw) = value.as_str() else {
        issues.push(issue(
            invalid_code,
            path,
            format!("field `{field}` must be a non-empty string"),
        ));
        return None;
    };
    if raw.trim().is_empty() {
        issues.push(issue(
            invalid_code,
            path,
            format!("field `{field}` must be a non-empty string"),
        ));
        return None;
    }
    Some(raw.to_string())
}

fn issue(
    code: impl Into<String>,
    path: impl Into<String>,
    message: impl Into<String>,
) -> ComponentLibraryIssue {
    ComponentLibraryIssue {
        code: code.into(),
        path: path.into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_component_library_schema_v1() {
        let parsed = parse_component_library_json(
            r#"{
  "schema_version": 1,
  "components": [
    { "id": "cyl_a", "name": "Lift Cylinder", "type": "cylinder", "params": { "stroke_mm": 120 } },
    { "id": "x_home", "name": "Home Sensor", "type": "sensor", "params": { "channel": "DI0" } },
    { "id": "s_start", "name": "Start Switch", "type": "switch", "params": { "channel": "DI1" } },
    { "id": "m1", "name": "Axis M1", "type": "stepper_pd", "params": { "pulse_do": "DO2", "dir_do": "DO3" } }
  ]
}"#,
        )
        .expect("schema should parse");
        assert_eq!(parsed.schema_version, 1);
        assert_eq!(parsed.components.len(), 4);
        assert_eq!(parsed.components[0].id, "cyl_a");
        assert_eq!(
            parsed.components[3].component_type,
            ComponentType::StepperPd
        );
    }

    #[test]
    fn accepts_stepper_alias_and_normalizes_to_stepper_pd_variant() {
        let parsed = parse_component_library_json(
            r#"{
  "schema_version": 1,
  "components": [
    { "id": "m1", "name": "Axis", "type": "stepper", "params": {} }
  ]
}"#,
        )
        .expect("stepper alias should parse");
        assert_eq!(parsed.components.len(), 1);
        assert_eq!(
            parsed.components[0].component_type,
            ComponentType::StepperPd
        );
    }

    #[test]
    fn reports_missing_schema_version_with_stable_code() {
        let err = parse_component_library_json(
            r#"{
  "components": []
}"#,
        )
        .expect_err("missing schema_version should fail");
        assert!(
            err.issues
                .iter()
                .any(|issue| issue.code == "CLIB-SCHEMA-001" && issue.path == "$.schema_version")
        );
    }

    #[test]
    fn reports_components_type_error_with_stable_code() {
        let err = parse_component_library_json(
            r#"{
  "schema_version": 1,
  "components": {}
}"#,
        )
        .expect_err("components type mismatch should fail");
        assert!(
            err.issues
                .iter()
                .any(|issue| issue.code == "CLIB-COMP-002" && issue.path == "$.components")
        );
    }

    #[test]
    fn reports_duplicate_component_id_with_stable_code() {
        let err = parse_component_library_json(
            r#"{
  "schema_version": 1,
  "components": [
    { "id": "dup", "name": "A", "type": "sensor", "params": {} },
    { "id": "dup", "name": "B", "type": "switch", "params": {} }
  ]
}"#,
        )
        .expect_err("duplicate IDs should fail");
        assert!(
            err.issues
                .iter()
                .any(|issue| issue.code == "CLIB-COMP-006" && issue.path == "$.components[1].id")
        );
    }

    #[test]
    fn reports_unsupported_component_type_with_stable_code() {
        let err = parse_component_library_json(
            r#"{
  "schema_version": 1,
  "components": [
    { "id": "m1", "name": "Axis", "type": "servo", "params": {} }
  ]
}"#,
        )
        .expect_err("unsupported type should fail");
        assert!(
            err.issues
                .iter()
                .any(|issue| issue.code == "CLIB-COMP-010" && issue.path == "$.components[0].type")
        );
    }
}
