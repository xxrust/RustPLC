use crate::component_library::{
    ComponentLibrary, ComponentLibraryIssue, ComponentLibraryValidationError, ComponentType,
    parse_component_library_value,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComponentInstance {
    pub id: String,
    pub component_id: String,
    pub params: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentConnection {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComponentTopology {
    pub schema_version: u32,
    pub component_library: ComponentLibrary,
    pub components: Vec<ComponentInstance>,
    pub connections: Vec<ComponentConnection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentTopologyIssue {
    pub code: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("component topology validation failed")]
pub struct ComponentTopologyValidationError {
    pub issues: Vec<ComponentTopologyIssue>,
}

pub fn parse_component_topology_json(
    input: &str,
) -> Result<ComponentTopology, ComponentTopologyValidationError> {
    let value =
        serde_json::from_str::<Value>(input).map_err(|err| ComponentTopologyValidationError {
            issues: vec![topo_issue(
                "CTOP-PARSE-001",
                "$",
                format!("invalid JSON: {err}"),
            )],
        })?;
    parse_component_topology_value(&value)
}

pub fn parse_component_topology_value(
    value: &Value,
) -> Result<ComponentTopology, ComponentTopologyValidationError> {
    let mut issues = Vec::new();
    let Some(root) = value.as_object() else {
        return Err(ComponentTopologyValidationError {
            issues: vec![topo_issue("CTOP-PARSE-002", "$", "root must be an object")],
        });
    };

    let schema_version = match root.get("schema_version") {
        None => {
            issues.push(topo_issue(
                "CTOP-SCHEMA-001",
                "$.schema_version",
                "missing required field `schema_version`",
            ));
            0
        }
        Some(v) => match v.as_u64() {
            Some(1) => 1,
            Some(other) => {
                issues.push(topo_issue(
                    "CTOP-SCHEMA-003",
                    "$.schema_version",
                    format!("unsupported schema_version `{other}` (expected `1`)"),
                ));
                0
            }
            None => {
                issues.push(topo_issue(
                    "CTOP-SCHEMA-002",
                    "$.schema_version",
                    "schema_version must be an integer",
                ));
                0
            }
        },
    };

    let component_library = match root.get("component_library") {
        None => {
            issues.push(topo_issue(
                "CTOP-LIB-001",
                "$.component_library",
                "missing required field `component_library`",
            ));
            None
        }
        Some(v) => match parse_component_library_value(v) {
            Ok(lib) => Some(lib),
            Err(err) => {
                issues.extend(component_library_issues_to_topology(err));
                None
            }
        },
    };

    let instances = parse_instances(root.get("components"), &mut issues);
    let connections = parse_connections(root.get("connections"), &mut issues);

    let Some(component_library) = component_library else {
        return Err(ComponentTopologyValidationError { issues });
    };

    validate_topology_relations(&component_library, &instances, &connections, &mut issues);

    if !issues.is_empty() {
        return Err(ComponentTopologyValidationError { issues });
    }

    Ok(ComponentTopology {
        schema_version,
        component_library,
        components: instances,
        connections,
    })
}

pub fn write_component_topology_json(
    path: &Path,
    topology: &ComponentTopology,
) -> Result<(), String> {
    let mut body = serde_json::to_string_pretty(topology)
        .map_err(|err| format!("Failed to serialize component topology JSON: {err}"))?;
    body.push('\n');
    std::fs::write(path, body).map_err(|err| {
        format!(
            "Failed to write component topology {}: {err}",
            path.display()
        )
    })
}

fn parse_instances(
    raw: Option<&Value>,
    issues: &mut Vec<ComponentTopologyIssue>,
) -> Vec<ComponentInstance> {
    let Some(raw) = raw else {
        issues.push(topo_issue(
            "CTOP-INS-001",
            "$.components",
            "missing required field `components`",
        ));
        return Vec::new();
    };
    let Some(entries) = raw.as_array() else {
        issues.push(topo_issue(
            "CTOP-INS-002",
            "$.components",
            "components must be an array",
        ));
        return Vec::new();
    };

    let mut out = Vec::new();
    for (idx, entry) in entries.iter().enumerate() {
        let path = format!("$.components[{idx}]");
        let Some(obj) = entry.as_object() else {
            issues.push(topo_issue(
                "CTOP-INS-003",
                path,
                "component instance must be an object",
            ));
            continue;
        };

        let Some(id) = read_required_string(obj, "id", idx, issues, "CTOP-INS-004", "CTOP-INS-005")
        else {
            continue;
        };
        let Some(component_id) = read_required_string(
            obj,
            "component_id",
            idx,
            issues,
            "CTOP-INS-006",
            "CTOP-INS-007",
        ) else {
            continue;
        };
        let params = match obj.get("params") {
            Some(v) => match v.as_object() {
                Some(map) => map
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<BTreeMap<_, _>>(),
                None => {
                    issues.push(topo_issue(
                        "CTOP-INS-008",
                        format!("$.components[{idx}].params"),
                        "params must be an object when provided",
                    ));
                    continue;
                }
            },
            None => BTreeMap::new(),
        };

        out.push(ComponentInstance {
            id,
            component_id,
            params,
        });
    }
    out
}

fn parse_connections(
    raw: Option<&Value>,
    issues: &mut Vec<ComponentTopologyIssue>,
) -> Vec<ComponentConnection> {
    let Some(raw) = raw else {
        issues.push(topo_issue(
            "CTOP-CONN-001",
            "$.connections",
            "missing required field `connections`",
        ));
        return Vec::new();
    };
    let Some(entries) = raw.as_array() else {
        issues.push(topo_issue(
            "CTOP-CONN-002",
            "$.connections",
            "connections must be an array",
        ));
        return Vec::new();
    };

    let mut out = Vec::new();
    for (idx, entry) in entries.iter().enumerate() {
        let path = format!("$.connections[{idx}]");
        let Some(obj) = entry.as_object() else {
            issues.push(topo_issue(
                "CTOP-CONN-003",
                path,
                "connection entry must be an object",
            ));
            continue;
        };
        let Some(from) = read_connection_endpoint(obj, "from", idx, issues) else {
            continue;
        };
        let Some(to) = read_connection_endpoint(obj, "to", idx, issues) else {
            continue;
        };
        out.push(ComponentConnection { from, to });
    }
    out
}

fn validate_topology_relations(
    component_library: &ComponentLibrary,
    instances: &[ComponentInstance],
    connections: &[ComponentConnection],
    issues: &mut Vec<ComponentTopologyIssue>,
) {
    let mut by_component = BTreeMap::<String, ComponentType>::new();
    for component in &component_library.components {
        by_component.insert(component.id.clone(), component.component_type);
    }

    let mut instance_component_type = BTreeMap::<String, ComponentType>::new();
    let mut instance_ids = BTreeSet::<String>::new();
    for (idx, instance) in instances.iter().enumerate() {
        if !instance_ids.insert(instance.id.clone()) {
            issues.push(topo_issue(
                "CTOP-INS-009",
                format!("$.components[{idx}].id"),
                format!("duplicate instance id `{}`", instance.id),
            ));
            continue;
        }
        let Some(component_type) = by_component.get(&instance.component_id).copied() else {
            issues.push(topo_issue(
                "CTOP-INS-010",
                format!("$.components[{idx}].component_id"),
                format!(
                    "unknown component_id `{}`; not found in component_library",
                    instance.component_id
                ),
            ));
            continue;
        };
        instance_component_type.insert(instance.id.clone(), component_type);
    }

    let mut connected_inputs = BTreeSet::<(String, &'static str)>::new();
    for (idx, connection) in connections.iter().enumerate() {
        let Some((from_instance, from_port)) = parse_endpoint(
            &connection.from,
            format!("$.connections[{idx}].from"),
            issues,
        ) else {
            continue;
        };
        let Some((to_instance, to_port)) =
            parse_endpoint(&connection.to, format!("$.connections[{idx}].to"), issues)
        else {
            continue;
        };

        let Some(from_type) = instance_component_type.get(from_instance).copied() else {
            issues.push(topo_issue(
                "CTOP-CONN-005",
                format!("$.connections[{idx}].from"),
                format!("unknown source instance `{from_instance}`"),
            ));
            continue;
        };
        let Some(to_type) = instance_component_type.get(to_instance).copied() else {
            issues.push(topo_issue(
                "CTOP-CONN-006",
                format!("$.connections[{idx}].to"),
                format!("unknown target instance `{to_instance}`"),
            ));
            continue;
        };

        let from_catalog = port_catalog(from_type);
        let to_catalog = port_catalog(to_type);
        if !from_catalog.outputs.contains(&from_port) {
            issues.push(topo_issue(
                "CTOP-CONN-007",
                format!("$.connections[{idx}].from"),
                format!("`{}` is not an output port on `{from_instance}`", from_port),
            ));
            continue;
        }
        if !to_catalog.inputs.contains(&to_port) {
            issues.push(topo_issue(
                "CTOP-CONN-008",
                format!("$.connections[{idx}].to"),
                format!("`{}` is not an input port on `{to_instance}`", to_port),
            ));
            continue;
        }
        connected_inputs.insert((to_instance.to_string(), to_port));
    }

    for (idx, instance) in instances.iter().enumerate() {
        let Some(component_type) = instance_component_type.get(&instance.id).copied() else {
            continue;
        };
        let catalog = port_catalog(component_type);
        for required in catalog.required_inputs {
            if !connected_inputs.contains(&(instance.id.clone(), required)) {
                issues.push(topo_issue(
                    "CTOP-CONN-009",
                    format!("$.components[{idx}].id"),
                    format!(
                        "required input `{required}` on instance `{}` is not connected",
                        instance.id
                    ),
                ));
            }
        }
    }
}

#[derive(Debug)]
struct PortCatalog {
    inputs: &'static [&'static str],
    outputs: &'static [&'static str],
    required_inputs: &'static [&'static str],
}

fn port_catalog(component_type: ComponentType) -> PortCatalog {
    match component_type {
        ComponentType::Cylinder => PortCatalog {
            inputs: &["cmd_extend", "cmd_retract"],
            outputs: &["sensor_extended", "sensor_retracted", "state"],
            required_inputs: &["cmd_extend", "cmd_retract"],
        },
        ComponentType::Sensor => PortCatalog {
            inputs: &[],
            outputs: &["state"],
            required_inputs: &[],
        },
        ComponentType::Switch => PortCatalog {
            inputs: &[],
            outputs: &["state"],
            required_inputs: &[],
        },
        ComponentType::StepperPd => PortCatalog {
            inputs: &["pulse", "direction", "enable"],
            outputs: &["position_steps", "alarm"],
            required_inputs: &["pulse", "direction"],
        },
    }
}

fn parse_endpoint<'a>(
    raw: &'a str,
    path: String,
    issues: &mut Vec<ComponentTopologyIssue>,
) -> Option<(&'a str, &'a str)> {
    let Some((instance, port)) = raw.split_once('.') else {
        issues.push(topo_issue(
            "CTOP-CONN-004",
            path,
            "endpoint must use `<instance_id>.<port>` format",
        ));
        return None;
    };
    if instance.trim().is_empty() || port.trim().is_empty() {
        issues.push(topo_issue(
            "CTOP-CONN-004",
            path,
            "endpoint must use `<instance_id>.<port>` format",
        ));
        return None;
    }
    Some((instance, port))
}

fn read_connection_endpoint(
    obj: &Map<String, Value>,
    field: &str,
    idx: usize,
    issues: &mut Vec<ComponentTopologyIssue>,
) -> Option<String> {
    let path = format!("$.connections[{idx}].{field}");
    let Some(value) = obj.get(field) else {
        issues.push(topo_issue(
            "CTOP-CONN-004",
            path,
            format!("missing required field `{field}`"),
        ));
        return None;
    };
    let Some(raw) = value.as_str() else {
        issues.push(topo_issue(
            "CTOP-CONN-004",
            path,
            format!("field `{field}` must be a non-empty string endpoint"),
        ));
        return None;
    };
    if raw.trim().is_empty() {
        issues.push(topo_issue(
            "CTOP-CONN-004",
            path,
            format!("field `{field}` must be a non-empty string endpoint"),
        ));
        return None;
    }
    Some(raw.to_string())
}

fn read_required_string(
    obj: &Map<String, Value>,
    field: &str,
    index: usize,
    issues: &mut Vec<ComponentTopologyIssue>,
    missing_code: &str,
    invalid_code: &str,
) -> Option<String> {
    let path = format!("$.components[{index}].{field}");
    let Some(value) = obj.get(field) else {
        issues.push(topo_issue(
            missing_code,
            path,
            format!("missing required field `{field}`"),
        ));
        return None;
    };
    let Some(raw) = value.as_str() else {
        issues.push(topo_issue(
            invalid_code,
            path,
            format!("field `{field}` must be a non-empty string"),
        ));
        return None;
    };
    if raw.trim().is_empty() {
        issues.push(topo_issue(
            invalid_code,
            path,
            format!("field `{field}` must be a non-empty string"),
        ));
        return None;
    }
    Some(raw.to_string())
}

fn component_library_issues_to_topology(
    err: ComponentLibraryValidationError,
) -> Vec<ComponentTopologyIssue> {
    err.issues
        .into_iter()
        .map(
            |ComponentLibraryIssue {
                 code,
                 path,
                 message,
             }| ComponentTopologyIssue {
                code: format!("CTOP-LIB::{code}"),
                path: format!("$.component_library{}", path.trim_start_matches('$')),
                message,
            },
        )
        .collect()
}

fn topo_issue(
    code: impl Into<String>,
    path: impl Into<String>,
    message: impl Into<String>,
) -> ComponentTopologyIssue {
    ComponentTopologyIssue {
        code: code.into(),
        path: path.into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_topology_json() -> &'static str {
        r#"{
  "schema_version": 1,
  "component_library": {
    "schema_version": 1,
    "components": [
      { "id": "sensor", "name": "Sensor", "type": "sensor", "params": {} },
      { "id": "switch", "name": "Switch", "type": "switch", "params": {} },
      { "id": "cylinder", "name": "Cylinder", "type": "cylinder", "params": { "stroke_ticks": 5 } },
      { "id": "stepper", "name": "Stepper", "type": "stepper_pd", "params": {} }
    ]
  },
  "components": [
    { "id": "s_start", "component_id": "switch", "params": {} },
    { "id": "x_front", "component_id": "sensor", "params": {} },
    { "id": "cyl_a", "component_id": "cylinder", "params": {} },
    { "id": "m1", "component_id": "stepper", "params": {} }
  ],
  "connections": [
    { "from": "s_start.state", "to": "cyl_a.cmd_extend" },
    { "from": "x_front.state", "to": "cyl_a.cmd_retract" },
    { "from": "s_start.state", "to": "m1.pulse" },
    { "from": "x_front.state", "to": "m1.direction" }
  ]
}"#
    }

    #[test]
    fn parses_valid_component_topology() {
        let topology =
            parse_component_topology_json(valid_topology_json()).expect("topology parse");
        assert_eq!(topology.schema_version, 1);
        assert_eq!(topology.components.len(), 4);
        assert_eq!(topology.connections.len(), 4);
    }

    #[test]
    fn reports_invalid_direction_or_port_with_stable_code() {
        let err = parse_component_topology_json(
            r#"{
  "schema_version": 1,
  "component_library": {
    "schema_version": 1,
    "components": [
      { "id": "sensor", "name": "Sensor", "type": "sensor", "params": {} },
      { "id": "cylinder", "name": "Cylinder", "type": "cylinder", "params": {} }
    ]
  },
  "components": [
    { "id": "x0", "component_id": "sensor", "params": {} },
    { "id": "c0", "component_id": "cylinder", "params": {} }
  ],
  "connections": [
    { "from": "x0.state", "to": "c0.sensor_extended" }
  ]
}"#,
        )
        .expect_err("invalid direction should fail");
        assert!(
            err.issues
                .iter()
                .any(|issue| issue.code == "CTOP-CONN-008" && issue.path == "$.connections[0].to")
        );
    }

    #[test]
    fn reports_missing_required_connection_with_stable_code() {
        let err = parse_component_topology_json(
            r#"{
  "schema_version": 1,
  "component_library": {
    "schema_version": 1,
    "components": [
      { "id": "switch", "name": "Switch", "type": "switch", "params": {} },
      { "id": "cylinder", "name": "Cylinder", "type": "cylinder", "params": {} }
    ]
  },
  "components": [
    { "id": "s0", "component_id": "switch", "params": {} },
    { "id": "c0", "component_id": "cylinder", "params": {} }
  ],
  "connections": [
    { "from": "s0.state", "to": "c0.cmd_extend" }
  ]
}"#,
        )
        .expect_err("missing required input should fail");
        assert!(
            err.issues
                .iter()
                .any(|issue| issue.code == "CTOP-CONN-009" && issue.path == "$.components[1].id")
        );
    }
}
