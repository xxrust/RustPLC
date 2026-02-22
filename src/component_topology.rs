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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag_rules: Option<ComponentTagRules>,
    pub component_library: ComponentLibrary,
    pub components: Vec<ComponentInstance>,
    pub connections: Vec<ComponentConnection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ComponentTagRules {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub danger_level: Option<DangerLevelRuleConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub functional_group: Option<GroupConnectionRuleConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location_group: Option<LocationGroupRuleConfig>,
}

impl ComponentTagRules {
    fn is_empty(&self) -> bool {
        self.danger_level.is_none()
            && self.functional_group.is_none()
            && self.location_group.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DangerLevelRuleConfig {
    pub dual_channel_levels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GroupConnectionRuleConfig {
    #[serde(default)]
    pub mode: GroupConnectionMode,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum GroupConnectionMode {
    #[default]
    AllowAny,
    WithinOnly,
    CrossOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct LocationGroupRuleConfig {
    #[serde(default)]
    pub mode: GroupConnectionMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_cross_zone_pairs: Vec<[String; 2]>,
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

    let tag_rules = parse_tag_rules(root.get("tag_rules"), &mut issues);
    let instances = parse_instances(root.get("components"), &mut issues);
    let connections = parse_connections(root.get("connections"), &mut issues);

    let Some(component_library) = component_library else {
        return Err(ComponentTopologyValidationError { issues });
    };

    validate_topology_relations(
        &component_library,
        &instances,
        &connections,
        &tag_rules,
        &mut issues,
    );

    if !issues.is_empty() {
        return Err(ComponentTopologyValidationError { issues });
    }

    Ok(ComponentTopology {
        schema_version,
        tag_rules: (!tag_rules.is_empty()).then_some(tag_rules),
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

fn parse_tag_rules(
    raw: Option<&Value>,
    issues: &mut Vec<ComponentTopologyIssue>,
) -> ComponentTagRules {
    let Some(raw) = raw else {
        return ComponentTagRules::default();
    };
    let Some(obj) = raw.as_object() else {
        issues.push(topo_issue(
            "CTOP-TAGRULE-001",
            "$.tag_rules",
            "tag_rules must be an object",
        ));
        return ComponentTagRules::default();
    };

    let mut rules = ComponentTagRules::default();

    if let Some(raw_danger) = obj.get("danger_level") {
        rules.danger_level = parse_danger_level_rule(raw_danger, issues);
    }
    if let Some(raw_group) = obj.get("functional_group") {
        rules.functional_group = parse_group_rule(
            raw_group,
            "$.tag_rules.functional_group",
            "CTOP-TAGRULE-004",
            "CTOP-TAGRULE-005",
            issues,
        );
    }
    if let Some(raw_location) = obj.get("location_group") {
        rules.location_group = parse_location_group_rule(raw_location, issues);
    }

    rules
}

fn parse_danger_level_rule(
    raw: &Value,
    issues: &mut Vec<ComponentTopologyIssue>,
) -> Option<DangerLevelRuleConfig> {
    let path = "$.tag_rules.danger_level";
    let Some(obj) = raw.as_object() else {
        issues.push(topo_issue(
            "CTOP-TAGRULE-002",
            path,
            "danger_level rule must be an object",
        ));
        return None;
    };

    let Some(raw_levels) = obj.get("dual_channel_levels") else {
        issues.push(topo_issue(
            "CTOP-TAGRULE-003",
            format!("{path}.dual_channel_levels"),
            "danger_level rule requires `dual_channel_levels`",
        ));
        return None;
    };

    let Some(levels) = raw_levels.as_array() else {
        issues.push(topo_issue(
            "CTOP-TAGRULE-003",
            format!("{path}.dual_channel_levels"),
            "`dual_channel_levels` must be an array of non-empty strings",
        ));
        return None;
    };

    let mut normalized = Vec::new();
    for (idx, entry) in levels.iter().enumerate() {
        let Some(value) = entry.as_str() else {
            issues.push(topo_issue(
                "CTOP-TAGRULE-003",
                format!("{path}.dual_channel_levels[{idx}]"),
                "each danger level must be a non-empty string",
            ));
            continue;
        };
        let trimmed = value.trim();
        if trimmed.is_empty() {
            issues.push(topo_issue(
                "CTOP-TAGRULE-003",
                format!("{path}.dual_channel_levels[{idx}]"),
                "each danger level must be a non-empty string",
            ));
            continue;
        }
        normalized.push(trimmed.to_string());
    }

    normalized.sort();
    normalized.dedup();
    if normalized.is_empty() {
        issues.push(topo_issue(
            "CTOP-TAGRULE-003",
            format!("{path}.dual_channel_levels"),
            "`dual_channel_levels` must include at least one level",
        ));
        return None;
    }

    Some(DangerLevelRuleConfig {
        dual_channel_levels: normalized,
    })
}

fn parse_group_rule(
    raw: &Value,
    path: &str,
    object_error_code: &str,
    mode_error_code: &str,
    issues: &mut Vec<ComponentTopologyIssue>,
) -> Option<GroupConnectionRuleConfig> {
    let Some(obj) = raw.as_object() else {
        issues.push(topo_issue(
            object_error_code,
            path,
            "group rule must be an object",
        ));
        return None;
    };

    let Some(raw_mode) = obj.get("mode") else {
        return Some(GroupConnectionRuleConfig::default());
    };
    let Some(mode_str) = raw_mode.as_str() else {
        issues.push(topo_issue(
            mode_error_code,
            format!("{path}.mode"),
            "mode must be one of: allow_any, within_only, cross_only",
        ));
        return None;
    };

    let mode = match mode_str {
        "allow_any" => GroupConnectionMode::AllowAny,
        "within_only" => GroupConnectionMode::WithinOnly,
        "cross_only" => GroupConnectionMode::CrossOnly,
        _ => {
            issues.push(topo_issue(
                mode_error_code,
                format!("{path}.mode"),
                "mode must be one of: allow_any, within_only, cross_only",
            ));
            return None;
        }
    };
    Some(GroupConnectionRuleConfig { mode })
}

fn parse_location_group_rule(
    raw: &Value,
    issues: &mut Vec<ComponentTopologyIssue>,
) -> Option<LocationGroupRuleConfig> {
    let Some(mut rule) = parse_group_rule(
        raw,
        "$.tag_rules.location_group",
        "CTOP-TAGRULE-006",
        "CTOP-TAGRULE-007",
        issues,
    )
    .map(|base| LocationGroupRuleConfig {
        mode: base.mode,
        allowed_cross_zone_pairs: Vec::new(),
    }) else {
        return None;
    };

    let Some(obj) = raw.as_object() else {
        return Some(rule);
    };

    let Some(raw_pairs) = obj.get("allowed_cross_zone_pairs") else {
        return Some(rule);
    };
    let Some(entries) = raw_pairs.as_array() else {
        issues.push(topo_issue(
            "CTOP-TAGRULE-008",
            "$.tag_rules.location_group.allowed_cross_zone_pairs",
            "allowed_cross_zone_pairs must be an array of [source_zone, target_zone]",
        ));
        return Some(rule);
    };

    for (idx, entry) in entries.iter().enumerate() {
        let Some(pair) = entry.as_array() else {
            issues.push(topo_issue(
                "CTOP-TAGRULE-008",
                format!("$.tag_rules.location_group.allowed_cross_zone_pairs[{idx}]"),
                "each pair must be [source_zone, target_zone]",
            ));
            continue;
        };
        if pair.len() != 2 {
            issues.push(topo_issue(
                "CTOP-TAGRULE-008",
                format!("$.tag_rules.location_group.allowed_cross_zone_pairs[{idx}]"),
                "each pair must include exactly 2 strings",
            ));
            continue;
        }
        let Some(source) = pair[0].as_str().map(str::trim).filter(|s| !s.is_empty()) else {
            issues.push(topo_issue(
                "CTOP-TAGRULE-008",
                format!("$.tag_rules.location_group.allowed_cross_zone_pairs[{idx}][0]"),
                "source zone must be a non-empty string",
            ));
            continue;
        };
        let Some(target) = pair[1].as_str().map(str::trim).filter(|s| !s.is_empty()) else {
            issues.push(topo_issue(
                "CTOP-TAGRULE-008",
                format!("$.tag_rules.location_group.allowed_cross_zone_pairs[{idx}][1]"),
                "target zone must be a non-empty string",
            ));
            continue;
        };
        rule.allowed_cross_zone_pairs
            .push([source.to_string(), target.to_string()]);
    }

    Some(rule)
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
    tag_rules: &ComponentTagRules,
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
    let mut resolved_connections = Vec::<ResolvedConnection>::new();
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
        resolved_connections.push(ResolvedConnection {
            index: idx,
            from_instance: from_instance.to_string(),
            to_instance: to_instance.to_string(),
            from_type,
        });
    }

    let instance_indexes = instances
        .iter()
        .enumerate()
        .map(|(idx, instance)| (instance.id.clone(), idx))
        .collect::<BTreeMap<_, _>>();

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

    validate_tag_rules(
        tag_rules,
        instances,
        &instance_indexes,
        &resolved_connections,
        issues,
    );
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct InstanceTags {
    functional_group: BTreeSet<String>,
    danger_level: BTreeSet<String>,
    location_group: BTreeSet<String>,
}

impl InstanceTags {
    fn is_empty(&self) -> bool {
        self.functional_group.is_empty()
            && self.danger_level.is_empty()
            && self.location_group.is_empty()
    }
}

#[derive(Debug, Clone)]
struct ResolvedConnection {
    index: usize,
    from_instance: String,
    to_instance: String,
    from_type: ComponentType,
}

fn validate_tag_rules(
    tag_rules: &ComponentTagRules,
    instances: &[ComponentInstance],
    instance_indexes: &BTreeMap<String, usize>,
    resolved_connections: &[ResolvedConnection],
    issues: &mut Vec<ComponentTopologyIssue>,
) {
    if tag_rules.is_empty() {
        return;
    }

    let tags_by_instance = instances
        .iter()
        .map(|instance| (instance.id.clone(), extract_instance_tags(instance)))
        .collect::<BTreeMap<_, _>>();

    if let Some(danger_rule) = &tag_rules.danger_level {
        validate_danger_level_rule(
            danger_rule,
            instance_indexes,
            &tags_by_instance,
            resolved_connections,
            issues,
        );
    }
    if let Some(group_rule) = &tag_rules.functional_group {
        validate_functional_group_rule(group_rule, &tags_by_instance, resolved_connections, issues);
    }
    if let Some(location_rule) = &tag_rules.location_group {
        validate_location_group_rule(
            location_rule,
            &tags_by_instance,
            resolved_connections,
            issues,
        );
    }
}

fn extract_instance_tags(instance: &ComponentInstance) -> InstanceTags {
    let Some(raw_tags) = instance.params.get("tags") else {
        return InstanceTags::default();
    };
    let Some(obj) = raw_tags.as_object() else {
        return InstanceTags::default();
    };

    InstanceTags {
        functional_group: read_tag_dimension(obj.get("functional_group")),
        danger_level: read_tag_dimension(obj.get("danger_level")),
        location_group: read_tag_dimension(obj.get("location_group")),
    }
}

fn read_tag_dimension(raw: Option<&Value>) -> BTreeSet<String> {
    let Some(raw) = raw else {
        return BTreeSet::new();
    };
    let Some(entries) = raw.as_array() else {
        return BTreeSet::new();
    };

    let mut out = BTreeSet::new();
    for entry in entries {
        let Some(value) = entry.as_str() else {
            continue;
        };
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            out.insert(trimmed.to_string());
        }
    }
    out
}

fn validate_danger_level_rule(
    danger_rule: &DangerLevelRuleConfig,
    instance_indexes: &BTreeMap<String, usize>,
    tags_by_instance: &BTreeMap<String, InstanceTags>,
    resolved_connections: &[ResolvedConnection],
    issues: &mut Vec<ComponentTopologyIssue>,
) {
    let required_levels = danger_rule
        .dual_channel_levels
        .iter()
        .map(|level| level.as_str())
        .collect::<BTreeSet<_>>();

    for (instance_id, tags) in tags_by_instance {
        if tags
            .danger_level
            .iter()
            .all(|level| !required_levels.contains(level.as_str()))
        {
            continue;
        }

        let incoming_detection_channels = resolved_connections
            .iter()
            .filter(|conn| {
                conn.to_instance == *instance_id
                    && matches!(
                        conn.from_type,
                        ComponentType::Sensor | ComponentType::Switch
                    )
            })
            .map(|conn| conn.from_instance.as_str())
            .collect::<BTreeSet<_>>();

        if incoming_detection_channels.len() >= 2 {
            continue;
        }

        let Some(component_idx) = instance_indexes.get(instance_id) else {
            continue;
        };
        issues.push(topo_issue(
            "CTOP-TAGRULE-101",
            format!("$.components[{component_idx}].params.tags.danger_level"),
            format!(
                "component `{instance_id}` matches high-risk danger_level but has only {} independent detection channel(s); at least 2 sensor/switch channels are required",
                incoming_detection_channels.len()
            ),
        ));
    }
}

fn validate_functional_group_rule(
    group_rule: &GroupConnectionRuleConfig,
    tags_by_instance: &BTreeMap<String, InstanceTags>,
    resolved_connections: &[ResolvedConnection],
    issues: &mut Vec<ComponentTopologyIssue>,
) {
    if group_rule.mode == GroupConnectionMode::AllowAny {
        return;
    }

    for connection in resolved_connections {
        let Some(from_tags) = tags_by_instance.get(&connection.from_instance) else {
            continue;
        };
        let Some(to_tags) = tags_by_instance.get(&connection.to_instance) else {
            continue;
        };
        if from_tags.functional_group.is_empty() || to_tags.functional_group.is_empty() {
            continue;
        }

        let has_shared_group = from_tags
            .functional_group
            .iter()
            .any(|tag| to_tags.functional_group.contains(tag));

        match group_rule.mode {
            GroupConnectionMode::WithinOnly if !has_shared_group => {
                issues.push(topo_issue(
                    "CTOP-TAGRULE-102",
                    format!("$.connections[{}]", connection.index),
                    format!(
                        "connection `{}` -> `{}` crosses functional_group boundaries but rule mode is `within_only`",
                        connection.from_instance, connection.to_instance
                    ),
                ));
            }
            GroupConnectionMode::CrossOnly if has_shared_group => {
                issues.push(topo_issue(
                    "CTOP-TAGRULE-103",
                    format!("$.connections[{}]", connection.index),
                    format!(
                        "connection `{}` -> `{}` stays within the same functional_group but rule mode is `cross_only`",
                        connection.from_instance, connection.to_instance
                    ),
                ));
            }
            _ => {}
        }
    }
}

fn validate_location_group_rule(
    location_rule: &LocationGroupRuleConfig,
    tags_by_instance: &BTreeMap<String, InstanceTags>,
    resolved_connections: &[ResolvedConnection],
    issues: &mut Vec<ComponentTopologyIssue>,
) {
    if location_rule.mode == GroupConnectionMode::AllowAny {
        return;
    }

    for connection in resolved_connections {
        let Some(from_tags) = tags_by_instance.get(&connection.from_instance) else {
            continue;
        };
        let Some(to_tags) = tags_by_instance.get(&connection.to_instance) else {
            continue;
        };
        if from_tags.location_group.is_empty() || to_tags.location_group.is_empty() {
            continue;
        }

        let same_zone =
            has_related_location_tag(&from_tags.location_group, &to_tags.location_group);

        match location_rule.mode {
            GroupConnectionMode::WithinOnly if !same_zone => {
                if is_allowed_cross_zone_connection(
                    &from_tags.location_group,
                    &to_tags.location_group,
                    &location_rule.allowed_cross_zone_pairs,
                ) {
                    continue;
                }
                issues.push(topo_issue(
                    "CTOP-TAGRULE-104",
                    format!("$.connections[{}]", connection.index),
                    format!(
                        "connection `{}` -> `{}` crosses location_group isolation boundaries",
                        connection.from_instance, connection.to_instance
                    ),
                ));
            }
            GroupConnectionMode::CrossOnly if same_zone => {
                issues.push(topo_issue(
                    "CTOP-TAGRULE-105",
                    format!("$.connections[{}]", connection.index),
                    format!(
                        "connection `{}` -> `{}` remains in the same location_group but rule mode is `cross_only`",
                        connection.from_instance, connection.to_instance
                    ),
                ));
            }
            _ => {}
        }
    }
}

fn has_related_location_tag(left: &BTreeSet<String>, right: &BTreeSet<String>) -> bool {
    left.iter()
        .any(|a| right.iter().any(|b| is_related_location_path(a, b)))
}

fn is_related_location_path(left: &str, right: &str) -> bool {
    is_same_or_path_prefix(left, right) || is_same_or_path_prefix(right, left)
}

fn is_same_or_path_prefix(prefix: &str, value: &str) -> bool {
    if prefix == value {
        return true;
    }
    value.len() > prefix.len()
        && value.starts_with(prefix)
        && value.as_bytes().get(prefix.len()) == Some(&b'/')
}

fn is_allowed_cross_zone_connection(
    source_locations: &BTreeSet<String>,
    target_locations: &BTreeSet<String>,
    allowed_pairs: &[[String; 2]],
) -> bool {
    allowed_pairs.iter().any(|pair| {
        let forward_match = source_locations
            .iter()
            .any(|source| is_related_location_path(source, &pair[0]))
            && target_locations
                .iter()
                .any(|target| is_related_location_path(target, &pair[1]));
        let reverse_match = source_locations
            .iter()
            .any(|source| is_related_location_path(source, &pair[1]))
            && target_locations
                .iter()
                .any(|target| is_related_location_path(target, &pair[0]));
        forward_match || reverse_match
    })
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentTopologySemanticDiffReport {
    pub schema_version: u32,
    pub is_match: bool,
    pub summary: ComponentTopologySemanticDiffSummary,
    pub nodes: ComponentNodeDiff,
    pub ports: ComponentPortDiff,
    pub relations: ComponentRelationDiff,
    pub tags: ComponentTagDiff,
    pub impact: ComponentImpactAnalysis,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentTopologySemanticDiffSummary {
    pub node_changes: usize,
    pub port_changes: usize,
    pub relation_changes: usize,
    pub tag_changes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentNodeSnapshot {
    pub node_id: String,
    pub component_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentNodeMutation {
    pub node_id: String,
    pub from_component_id: String,
    pub to_component_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentNodeDiff {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added: Vec<ComponentNodeSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<ComponentNodeSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modified: Vec<ComponentNodeMutation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentPortContractSnapshot {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub required_inputs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentPortSnapshot {
    pub node_id: String,
    pub ports: ComponentPortContractSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentPortMutation {
    pub node_id: String,
    pub from: ComponentPortContractSnapshot,
    pub to: ComponentPortContractSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentPortDiff {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added: Vec<ComponentPortSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<ComponentPortSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modified: Vec<ComponentPortMutation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentRelationDiff {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added: Vec<ComponentConnection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<ComponentConnection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentTagSnapshot {
    pub node_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub functional_group: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub danger_level: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub location_group: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentTagMutation {
    pub node_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added_functional_group: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed_functional_group: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added_danger_level: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed_danger_level: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added_location_group: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed_location_group: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentTagDiff {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added: Vec<ComponentTagSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<ComponentTagSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modified: Vec<ComponentTagMutation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentImpactAnalysis {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relation_change_nodes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tag_change_nodes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blast_radius_nodes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blast_radius_relations: Vec<ComponentConnection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub high_risk_nodes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review_reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComponentPortProfile {
    inputs: BTreeSet<String>,
    outputs: BTreeSet<String>,
    required_inputs: BTreeSet<String>,
}

pub fn diff_component_topology_semantics(
    before: &ComponentTopology,
    after: &ComponentTopology,
) -> ComponentTopologySemanticDiffReport {
    let before_nodes = before
        .components
        .iter()
        .map(|instance| (instance.id.clone(), instance.component_id.clone()))
        .collect::<BTreeMap<_, _>>();
    let after_nodes = after
        .components
        .iter()
        .map(|instance| (instance.id.clone(), instance.component_id.clone()))
        .collect::<BTreeMap<_, _>>();

    let mut node_added = Vec::new();
    let mut node_removed = Vec::new();
    let mut node_modified = Vec::new();
    for (node_id, component_id) in &after_nodes {
        match before_nodes.get(node_id) {
            None => node_added.push(ComponentNodeSnapshot {
                node_id: node_id.clone(),
                component_id: component_id.clone(),
            }),
            Some(before_component_id) if before_component_id != component_id => {
                node_modified.push(ComponentNodeMutation {
                    node_id: node_id.clone(),
                    from_component_id: before_component_id.clone(),
                    to_component_id: component_id.clone(),
                });
            }
            _ => {}
        }
    }
    for (node_id, component_id) in &before_nodes {
        if !after_nodes.contains_key(node_id) {
            node_removed.push(ComponentNodeSnapshot {
                node_id: node_id.clone(),
                component_id: component_id.clone(),
            });
        }
    }

    let before_ports = collect_component_port_profiles(before);
    let after_ports = collect_component_port_profiles(after);
    let mut port_added = Vec::new();
    let mut port_removed = Vec::new();
    let mut port_modified = Vec::new();
    for (node_id, profile) in &after_ports {
        match before_ports.get(node_id) {
            None => port_added.push(ComponentPortSnapshot {
                node_id: node_id.clone(),
                ports: component_port_snapshot(profile),
            }),
            Some(before_profile) if before_profile != profile => {
                port_modified.push(ComponentPortMutation {
                    node_id: node_id.clone(),
                    from: component_port_snapshot(before_profile),
                    to: component_port_snapshot(profile),
                });
            }
            _ => {}
        }
    }
    for (node_id, profile) in &before_ports {
        if !after_ports.contains_key(node_id) {
            port_removed.push(ComponentPortSnapshot {
                node_id: node_id.clone(),
                ports: component_port_snapshot(profile),
            });
        }
    }

    let before_relations = relation_key_set(&before.connections);
    let after_relations = relation_key_set(&after.connections);
    let relation_added = after_relations
        .difference(&before_relations)
        .map(relation_from_key)
        .collect::<Vec<_>>();
    let relation_removed = before_relations
        .difference(&after_relations)
        .map(relation_from_key)
        .collect::<Vec<_>>();

    let before_tags = collect_instance_tags(before);
    let after_tags = collect_instance_tags(after);
    let mut tag_added = Vec::new();
    let mut tag_removed = Vec::new();
    let mut tag_modified = Vec::new();
    for (node_id, tags) in &after_tags {
        match before_tags.get(node_id) {
            None => {
                let snapshot = component_tag_snapshot(node_id, tags);
                if !tags.is_empty() {
                    tag_added.push(snapshot);
                }
            }
            Some(before_tag) if before_tag != tags => {
                tag_modified.push(ComponentTagMutation {
                    node_id: node_id.clone(),
                    added_functional_group: sorted_diff(
                        &tags.functional_group,
                        &before_tag.functional_group,
                    ),
                    removed_functional_group: sorted_diff(
                        &before_tag.functional_group,
                        &tags.functional_group,
                    ),
                    added_danger_level: sorted_diff(&tags.danger_level, &before_tag.danger_level),
                    removed_danger_level: sorted_diff(&before_tag.danger_level, &tags.danger_level),
                    added_location_group: sorted_diff(
                        &tags.location_group,
                        &before_tag.location_group,
                    ),
                    removed_location_group: sorted_diff(
                        &before_tag.location_group,
                        &tags.location_group,
                    ),
                });
            }
            _ => {}
        }
    }
    for (node_id, tags) in &before_tags {
        if !after_tags.contains_key(node_id) {
            let snapshot = component_tag_snapshot(node_id, tags);
            if !tags.is_empty() {
                tag_removed.push(snapshot);
            }
        }
    }

    let mut relation_change_nodes = BTreeSet::new();
    for relation in relation_added.iter().chain(relation_removed.iter()) {
        let (from_node, to_node) = relation_node_ids(relation);
        relation_change_nodes.insert(from_node);
        relation_change_nodes.insert(to_node);
    }

    let mut tag_change_nodes = BTreeSet::new();
    for snapshot in &tag_added {
        tag_change_nodes.insert(snapshot.node_id.clone());
    }
    for snapshot in &tag_removed {
        tag_change_nodes.insert(snapshot.node_id.clone());
    }
    for mutation in &tag_modified {
        tag_change_nodes.insert(mutation.node_id.clone());
    }

    let mut high_risk_nodes = BTreeSet::new();
    for snapshot in &tag_added {
        if !snapshot.danger_level.is_empty() {
            high_risk_nodes.insert(snapshot.node_id.clone());
        }
    }
    for snapshot in &tag_removed {
        if !snapshot.danger_level.is_empty() {
            high_risk_nodes.insert(snapshot.node_id.clone());
        }
    }
    for mutation in &tag_modified {
        if !mutation.added_danger_level.is_empty() || !mutation.removed_danger_level.is_empty() {
            high_risk_nodes.insert(mutation.node_id.clone());
        }
    }

    let mut blast_radius_nodes = relation_change_nodes
        .union(&tag_change_nodes)
        .cloned()
        .collect::<BTreeSet<_>>();
    let adjacency = build_relation_adjacency(before, after);
    for seed in blast_radius_nodes.clone() {
        if let Some(neighbors) = adjacency.get(&seed) {
            blast_radius_nodes.extend(neighbors.iter().cloned());
        }
    }

    let union_relations = before_relations
        .union(&after_relations)
        .map(relation_from_key)
        .collect::<Vec<_>>();
    let blast_radius_relations = union_relations
        .into_iter()
        .filter(|relation| {
            let (from_node, to_node) = relation_node_ids(relation);
            blast_radius_nodes.contains(&from_node) || blast_radius_nodes.contains(&to_node)
        })
        .collect::<Vec<_>>();

    let mut review_reasons = Vec::new();
    if !relation_change_nodes.is_empty() {
        review_reasons.push(
            "relations changed; rerun component-topology-validate and scenario simulation"
                .to_string(),
        );
    }
    if !tag_change_nodes.is_empty() {
        review_reasons.push(
            "tags changed; rerun tag-rule validation and update safety review notes".to_string(),
        );
    }
    if !high_risk_nodes.is_empty() {
        review_reasons.push(
            "danger_level tags changed; confirm dual-channel detection coverage remains valid"
                .to_string(),
        );
    }
    if !port_added.is_empty() || !port_removed.is_empty() || !port_modified.is_empty() {
        review_reasons.push(
            "port contracts changed; verify endpoint bindings and connection direction assumptions"
                .to_string(),
        );
    }

    let node_changes = node_added.len() + node_removed.len() + node_modified.len();
    let port_changes = port_added.len() + port_removed.len() + port_modified.len();
    let relation_changes = relation_added.len() + relation_removed.len();
    let tag_changes = tag_added.len() + tag_removed.len() + tag_modified.len();
    let is_match =
        node_changes == 0 && port_changes == 0 && relation_changes == 0 && tag_changes == 0;

    ComponentTopologySemanticDiffReport {
        schema_version: 1,
        is_match,
        summary: ComponentTopologySemanticDiffSummary {
            node_changes,
            port_changes,
            relation_changes,
            tag_changes,
        },
        nodes: ComponentNodeDiff {
            added: node_added,
            removed: node_removed,
            modified: node_modified,
        },
        ports: ComponentPortDiff {
            added: port_added,
            removed: port_removed,
            modified: port_modified,
        },
        relations: ComponentRelationDiff {
            added: relation_added,
            removed: relation_removed,
        },
        tags: ComponentTagDiff {
            added: tag_added,
            removed: tag_removed,
            modified: tag_modified,
        },
        impact: ComponentImpactAnalysis {
            relation_change_nodes: relation_change_nodes.into_iter().collect(),
            tag_change_nodes: tag_change_nodes.into_iter().collect(),
            blast_radius_nodes: blast_radius_nodes.into_iter().collect(),
            blast_radius_relations,
            high_risk_nodes: high_risk_nodes.into_iter().collect(),
            review_reasons,
        },
    }
}

fn collect_component_port_profiles(
    topology: &ComponentTopology,
) -> BTreeMap<String, ComponentPortProfile> {
    let mut component_types = BTreeMap::new();
    for component in &topology.component_library.components {
        component_types.insert(component.id.clone(), component.component_type);
    }

    let mut out = BTreeMap::new();
    for instance in &topology.components {
        let Some(component_type) = component_types.get(&instance.component_id).copied() else {
            continue;
        };
        let catalog = port_catalog(component_type);
        out.insert(
            instance.id.clone(),
            ComponentPortProfile {
                inputs: catalog
                    .inputs
                    .iter()
                    .map(|port| (*port).to_string())
                    .collect(),
                outputs: catalog
                    .outputs
                    .iter()
                    .map(|port| (*port).to_string())
                    .collect(),
                required_inputs: catalog
                    .required_inputs
                    .iter()
                    .map(|port| (*port).to_string())
                    .collect(),
            },
        );
    }
    out
}

fn component_port_snapshot(profile: &ComponentPortProfile) -> ComponentPortContractSnapshot {
    ComponentPortContractSnapshot {
        inputs: profile.inputs.iter().cloned().collect(),
        outputs: profile.outputs.iter().cloned().collect(),
        required_inputs: profile.required_inputs.iter().cloned().collect(),
    }
}

fn relation_key_set(connections: &[ComponentConnection]) -> BTreeSet<(String, String)> {
    connections
        .iter()
        .map(|connection| (connection.from.clone(), connection.to.clone()))
        .collect()
}

fn relation_from_key(key: &(String, String)) -> ComponentConnection {
    ComponentConnection {
        from: key.0.clone(),
        to: key.1.clone(),
    }
}

fn collect_instance_tags(topology: &ComponentTopology) -> BTreeMap<String, InstanceTags> {
    topology
        .components
        .iter()
        .map(|instance| (instance.id.clone(), extract_instance_tags(instance)))
        .collect()
}

fn component_tag_snapshot(node_id: &str, tags: &InstanceTags) -> ComponentTagSnapshot {
    ComponentTagSnapshot {
        node_id: node_id.to_string(),
        functional_group: tags.functional_group.iter().cloned().collect(),
        danger_level: tags.danger_level.iter().cloned().collect(),
        location_group: tags.location_group.iter().cloned().collect(),
    }
}

fn sorted_diff(left: &BTreeSet<String>, right: &BTreeSet<String>) -> Vec<String> {
    left.difference(right).cloned().collect()
}

fn relation_node_ids(relation: &ComponentConnection) -> (String, String) {
    (
        endpoint_instance_id(&relation.from),
        endpoint_instance_id(&relation.to),
    )
}

fn endpoint_instance_id(endpoint: &str) -> String {
    endpoint
        .split_once('.')
        .map(|(instance_id, _)| instance_id)
        .unwrap_or(endpoint)
        .to_string()
}

fn build_relation_adjacency(
    before: &ComponentTopology,
    after: &ComponentTopology,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut adjacency = BTreeMap::<String, BTreeSet<String>>::new();
    let relation_keys = relation_key_set(&before.connections)
        .union(&relation_key_set(&after.connections))
        .cloned()
        .collect::<Vec<_>>();
    for relation in relation_keys {
        let connection = relation_from_key(&relation);
        let (from_node, to_node) = relation_node_ids(&connection);
        adjacency
            .entry(from_node.clone())
            .or_default()
            .insert(to_node.clone());
        adjacency.entry(to_node).or_default().insert(from_node);
    }
    adjacency
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

    #[test]
    fn reports_danger_level_rule_violation_with_structured_issue() {
        let err = parse_component_topology_json(
            r#"{
  "schema_version": 1,
  "tag_rules": {
    "danger_level": {
      "dual_channel_levels": ["high"]
    }
  },
  "component_library": {
    "schema_version": 1,
    "components": [
      { "id": "switch", "name": "Switch", "type": "switch", "params": {} },
      { "id": "sensor", "name": "Sensor", "type": "sensor", "params": {} },
      { "id": "cylinder", "name": "Cylinder", "type": "cylinder", "params": {} }
    ]
  },
  "components": [
    { "id": "s0", "component_id": "switch", "params": {} },
    { "id": "x0", "component_id": "sensor", "params": {} },
    { "id": "c0", "component_id": "cylinder", "params": { "tags": { "danger_level": ["high"] } } }
  ],
  "connections": [
    { "from": "s0.state", "to": "c0.cmd_extend" },
    { "from": "s0.state", "to": "c0.cmd_retract" }
  ]
}"#,
        )
        .expect_err("danger_level dual-channel rule should fail");

        let issue = err
            .issues
            .iter()
            .find(|issue| issue.code == "CTOP-TAGRULE-101")
            .expect("expected CTOP-TAGRULE-101 issue");
        assert_eq!(issue.path, "$.components[2].params.tags.danger_level");
        assert!(
            issue.message.contains("at least 2"),
            "message should explain dual-channel requirement, got: {}",
            issue.message
        );
    }

    #[test]
    fn reports_functional_group_mode_violation_with_stable_code() {
        let err = parse_component_topology_json(
            r#"{
  "schema_version": 1,
  "tag_rules": {
    "functional_group": {
      "mode": "within_only"
    }
  },
  "component_library": {
    "schema_version": 1,
    "components": [
      { "id": "switch", "name": "Switch", "type": "switch", "params": {} },
      { "id": "sensor", "name": "Sensor", "type": "sensor", "params": {} },
      { "id": "cylinder", "name": "Cylinder", "type": "cylinder", "params": {} }
    ]
  },
  "components": [
    { "id": "s0", "component_id": "switch", "params": { "tags": { "functional_group": ["control"] } } },
    { "id": "x0", "component_id": "sensor", "params": { "tags": { "functional_group": ["sensing"] } } },
    { "id": "c0", "component_id": "cylinder", "params": { "tags": { "functional_group": ["actuation"] } } }
  ],
  "connections": [
    { "from": "s0.state", "to": "c0.cmd_extend" },
    { "from": "x0.state", "to": "c0.cmd_retract" }
  ]
}"#,
        )
        .expect_err("within_only functional_group rule should fail");

        assert!(
            err.issues
                .iter()
                .any(|issue| issue.code == "CTOP-TAGRULE-102" && issue.path == "$.connections[0]"),
            "expected CTOP-TAGRULE-102 on first connection"
        );
    }

    #[test]
    fn reports_location_group_isolation_violation_with_hierarchical_match() {
        let err = parse_component_topology_json(
            r#"{
  "schema_version": 1,
  "tag_rules": {
    "location_group": {
      "mode": "within_only"
    }
  },
  "component_library": {
    "schema_version": 1,
    "components": [
      { "id": "switch", "name": "Switch", "type": "switch", "params": {} },
      { "id": "sensor", "name": "Sensor", "type": "sensor", "params": {} },
      { "id": "cylinder", "name": "Cylinder", "type": "cylinder", "params": {} }
    ]
  },
  "components": [
    { "id": "s0", "component_id": "switch", "params": { "tags": { "location_group": ["line_a"] } } },
    { "id": "x0", "component_id": "sensor", "params": { "tags": { "location_group": ["line_b/cell_1"] } } },
    { "id": "c0", "component_id": "cylinder", "params": { "tags": { "location_group": ["line_a/cell_1/station_7"] } } }
  ],
  "connections": [
    { "from": "s0.state", "to": "c0.cmd_extend" },
    { "from": "x0.state", "to": "c0.cmd_retract" }
  ]
}"#,
        )
        .expect_err("location isolation rule should fail for cross-zone connection");

        assert!(
            err.issues
                .iter()
                .any(|issue| issue.code == "CTOP-TAGRULE-104" && issue.path == "$.connections[1]"),
            "expected CTOP-TAGRULE-104 for the cross-zone connection"
        );
    }

    #[test]
    fn reports_invalid_tag_rule_config_with_path_and_code() {
        let err = parse_component_topology_json(
            r#"{
  "schema_version": 1,
  "tag_rules": {
    "functional_group": {
      "mode": "invalid_mode"
    }
  },
  "component_library": {
    "schema_version": 1,
    "components": [
      { "id": "switch", "name": "Switch", "type": "switch", "params": {} },
      { "id": "sensor", "name": "Sensor", "type": "sensor", "params": {} },
      { "id": "cylinder", "name": "Cylinder", "type": "cylinder", "params": {} }
    ]
  },
  "components": [
    { "id": "s0", "component_id": "switch", "params": {} },
    { "id": "x0", "component_id": "sensor", "params": {} },
    { "id": "c0", "component_id": "cylinder", "params": {} }
  ],
  "connections": [
    { "from": "s0.state", "to": "c0.cmd_extend" },
    { "from": "x0.state", "to": "c0.cmd_retract" }
  ]
}"#,
        )
        .expect_err("invalid tag rule config should fail");

        assert!(
            err.issues.iter().any(|issue| {
                issue.code == "CTOP-TAGRULE-005"
                    && issue.path == "$.tag_rules.functional_group.mode"
                    && !issue.message.is_empty()
            }),
            "expected structured config issue with code/path/message"
        );
    }

    #[test]
    fn semantic_diff_reports_node_port_relation_tag_and_impact_changes() {
        let before = parse_component_topology_json(
            r#"{
  "schema_version": 1,
  "component_library": {
    "schema_version": 1,
    "components": [
      { "id": "switch", "name": "Switch", "type": "switch", "params": {} },
      { "id": "sensor", "name": "Sensor", "type": "sensor", "params": {} },
      { "id": "cylinder", "name": "Cylinder", "type": "cylinder", "params": {} },
      { "id": "stepper", "name": "Stepper", "type": "stepper_pd", "params": {} }
    ]
  },
  "components": [
    { "id": "s0", "component_id": "switch", "params": {} },
    { "id": "x0", "component_id": "sensor", "params": {} },
    {
      "id": "c0",
      "component_id": "cylinder",
      "params": {
        "tags": {
          "functional_group": ["actuation"],
          "danger_level": ["low"],
          "location_group": ["line_a/cell_1"]
        }
      }
    }
  ],
  "connections": [
    { "from": "s0.state", "to": "c0.cmd_extend" },
    { "from": "x0.state", "to": "c0.cmd_retract" }
  ]
}"#,
        )
        .expect("parse before topology");
        let after = parse_component_topology_json(
            r#"{
  "schema_version": 1,
  "component_library": {
    "schema_version": 1,
    "components": [
      { "id": "switch", "name": "Switch", "type": "switch", "params": {} },
      { "id": "sensor", "name": "Sensor", "type": "sensor", "params": {} },
      { "id": "cylinder", "name": "Cylinder", "type": "cylinder", "params": {} },
      { "id": "stepper", "name": "Stepper", "type": "stepper_pd", "params": {} }
    ]
  },
  "components": [
    { "id": "s0", "component_id": "switch", "params": {} },
    { "id": "x0", "component_id": "sensor", "params": {} },
    {
      "id": "c0",
      "component_id": "stepper",
      "params": {
        "tags": {
          "functional_group": ["motion"],
          "danger_level": ["high"],
          "location_group": ["line_b/cell_1"]
        }
      }
    },
    { "id": "m1", "component_id": "cylinder", "params": {} }
  ],
  "connections": [
    { "from": "s0.state", "to": "c0.pulse" },
    { "from": "x0.state", "to": "c0.direction" },
    { "from": "c0.position_steps", "to": "m1.cmd_extend" },
    { "from": "s0.state", "to": "m1.cmd_retract" }
  ]
}"#,
        )
        .expect("parse after topology");

        let report = diff_component_topology_semantics(&before, &after);
        assert!(!report.is_match, "semantic diff should detect changes");
        assert_eq!(report.summary.node_changes, 2);
        assert_eq!(report.summary.port_changes, 2);
        assert_eq!(report.summary.relation_changes, 6);
        assert_eq!(report.summary.tag_changes, 1);
        assert!(
            report.nodes.added.iter().any(|entry| entry.node_id == "m1"),
            "expected added node m1"
        );
        assert!(
            report
                .nodes
                .modified
                .iter()
                .any(|entry| entry.node_id == "c0"
                    && entry.from_component_id == "cylinder"
                    && entry.to_component_id == "stepper"),
            "expected component change on c0"
        );
        assert!(
            report
                .ports
                .modified
                .iter()
                .any(|entry| entry.node_id == "c0"),
            "expected c0 port profile change"
        );
        assert!(
            report
                .tags
                .modified
                .iter()
                .any(|entry| entry.node_id == "c0"
                    && entry.added_danger_level == vec!["high".to_string()]
                    && entry.removed_danger_level == vec!["low".to_string()]),
            "expected danger_level mutation on c0"
        );
        assert!(
            report.impact.tag_change_nodes.contains(&"c0".to_string()),
            "impact analysis should include c0 as tag-changed node"
        );
        assert!(
            report.impact.high_risk_nodes.contains(&"c0".to_string()),
            "danger_level change should mark c0 as high-risk node"
        );
        assert!(
            !report.impact.review_reasons.is_empty(),
            "impact analysis should emit review hints"
        );
    }

    #[test]
    fn semantic_diff_reports_match_for_identical_topology() {
        let topology =
            parse_component_topology_json(valid_topology_json()).expect("parse valid topology");
        let report = diff_component_topology_semantics(&topology, &topology);
        assert!(report.is_match);
        assert_eq!(report.summary.node_changes, 0);
        assert_eq!(report.summary.port_changes, 0);
        assert_eq!(report.summary.relation_changes, 0);
        assert_eq!(report.summary.tag_changes, 0);
        assert!(report.impact.review_reasons.is_empty());
    }
}
