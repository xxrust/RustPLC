use crate::ir::{DeviceKind, TopologyGraph};
use crate::parser::parse_plc;
use crate::semantic::{build_topology_graph, preprocess_program};
use petgraph::graph::NodeIndex;
use petgraph::Direction;
use serde_yaml::{Mapping, Number, Value};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputKind {
    Digital,
    Analog,
}

impl InputKind {
    fn label(self) -> &'static str {
        match self {
            Self::Digital => "digital_input",
            Self::Analog => "analog_input",
        }
    }
}

#[derive(Debug)]
struct ScenarioNameResolver {
    topology: TopologyGraph,
    by_name: HashMap<String, NodeIndex>,
}

impl ScenarioNameResolver {
    fn from_plc_source(plc_source: &str) -> Result<Self, String> {
        let parsed = parse_plc(plc_source).map_err(|e| e.to_string())?;
        let expanded = preprocess_program(&parsed).map_err(|errors| {
            errors
                .into_iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        })?;
        let topology = build_topology_graph(&expanded).map_err(|errors| {
            errors
                .into_iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        })?;

        let mut by_name = HashMap::new();
        for idx in topology.graph.node_indices() {
            by_name.insert(topology.graph[idx].name.clone(), idx);
        }

        Ok(Self { topology, by_name })
    }

    fn known_names_preview(&self, max: usize) -> String {
        let mut names = self
            .by_name
            .keys()
            .filter(|n| !is_physical_digital_name(n) && !is_physical_analog_name(n))
            .cloned()
            .collect::<Vec<_>>();
        names.sort();
        names.truncate(max);
        if names.is_empty() {
            return "<none>".to_string();
        }
        names.join(", ")
    }

    fn resolve_input_id(&self, raw: &str, kind: InputKind) -> Result<u16, String> {
        // Allow explicit numeric id strings.
        if let Some(id) = parse_decimal_u16(raw) {
            return Ok(id);
        }

        // Allow explicit physical names (X<N>, AI<N>) and explicit logical id names (DI<N>, AI<N>).
        match kind {
            InputKind::Digital => {
                if let Some(id) =
                    parse_prefixed_u16(raw, 'X').or_else(|| parse_prefixed_token_u16(raw, "DI"))
                {
                    return Ok(id);
                }
            }
            InputKind::Analog => {
                if let Some(id) = parse_prefixed_token_u16(raw, "AI") {
                    return Ok(id);
                }
            }
        }

        let start = self.by_name.get(raw).copied().ok_or_else(|| {
            format!("unknown {} name `{raw}`", kind.label())
        })?;

        let want_kind = match kind {
            InputKind::Digital => DeviceKind::DigitalInput,
            InputKind::Analog => DeviceKind::AnalogInput,
        };
        let parse_physical = match kind {
            InputKind::Digital => |name: &str| parse_prefixed_u16(name, 'X'),
            InputKind::Analog => |name: &str| parse_prefixed_token_u16(name, "AI"),
        };

        let ids = self.collect_physical_ids(start, want_kind, parse_physical);
        unique_physical_id(ids).map_err(|candidates| {
            if candidates.is_empty() {
                format!(
                    "unresolvable {} name `{raw}` (no upstream physical inputs found in topology)",
                    kind.label()
                )
            } else {
                format!(
                    "ambiguous {} name `{raw}` (maps to multiple physical ids: {})",
                    kind.label(),
                    candidates
                        .into_iter()
                        .map(|id| id.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        })
    }

    fn collect_physical_ids(
        &self,
        start: NodeIndex,
        kind: DeviceKind,
        parse: fn(&str) -> Option<u16>,
    ) -> Vec<u16> {
        let mut queue = VecDeque::new();
        let mut visited = HashSet::new();
        let mut out = Vec::new();

        queue.push_back(start);
        visited.insert(start);

        while let Some(n) = queue.pop_front() {
            let device = &self.topology.graph[n];
            if device.kind == kind {
                if let Some(id) = parse(&device.name) {
                    out.push(id);
                }
            }
            for pred in self
                .topology
                .graph
                .neighbors_directed(n, Direction::Incoming)
            {
                if visited.insert(pred) {
                    queue.push_back(pred);
                }
            }
        }

        out
    }
}

fn unique_physical_id(mut ids: Vec<u16>) -> Result<u16, Vec<u16>> {
    ids.sort_unstable();
    ids.dedup();
    if ids.len() == 1 {
        Ok(ids[0])
    } else {
        Err(ids)
    }
}

fn parse_decimal_u16(s: &str) -> Option<u16> {
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    s.parse::<u16>().ok()
}

fn parse_prefixed_u16(name: &str, prefix: char) -> Option<u16> {
    let mut chars = name.chars();
    let first = chars.next()?;
    if first.to_ascii_uppercase() != prefix {
        return None;
    }
    let rest: String = chars.collect();
    parse_decimal_u16(&rest)
}

fn parse_prefixed_token_u16(name: &str, prefix: &str) -> Option<u16> {
    let (head, rest) = name.split_at(prefix.len().min(name.len()));
    if head.eq_ignore_ascii_case(prefix) {
        parse_decimal_u16(rest)
    } else {
        None
    }
}

fn is_physical_digital_name(name: &str) -> bool {
    parse_prefixed_u16(name, 'X').is_some()
}

fn is_physical_analog_name(name: &str) -> bool {
    parse_prefixed_token_u16(name, "AI").is_some()
}

fn val_to_u16_number(v: &Value) -> Option<u16> {
    match v {
        Value::Number(n) => n.as_u64().and_then(|u| u16::try_from(u).ok()),
        Value::String(s) => parse_decimal_u16(s),
        _ => None,
    }
}

fn u16_to_yaml_number(id: u16) -> Value {
    Value::Number(Number::from(id))
}

fn resolve_map_keys_to_u16(
    resolver: &ScenarioNameResolver,
    map: &Mapping,
    kind: InputKind,
    path_prefix: &str,
) -> Result<Mapping, String> {
    let mut out = Mapping::new();
    let mut seen = HashMap::<u16, Value>::new();
    let mut seen_from = HashMap::<u16, String>::new();

    for (k, v) in map {
        let raw_key = match k {
            Value::Number(_) => None,
            Value::String(s) => Some(s.as_str()),
            other => {
                return Err(format!(
                    "invalid key type at {path_prefix} (expected string or integer): {other:?}"
                ));
            }
        };

        let id = if let Some(name) = raw_key {
            resolver.resolve_input_id(name, kind).map_err(|e| {
                format!(
                    "{e} at {path_prefix}.{name}\n\nhint: known device names include: {}",
                    resolver.known_names_preview(12)
                )
            })?
        } else {
            val_to_u16_number(k).ok_or_else(|| {
                format!(
                    "invalid numeric key at {path_prefix} (expected 0..=65535 integer): {k:?}"
                )
            })?
        };

        if let Some(prev) = seen.get(&id) {
            if prev != v {
                let prev_from = seen_from
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| "<unknown>".to_string());
                return Err(format!(
                    "conflicting assignments for {} id {} at {path_prefix}: first from {prev_from}, then from {k:?}",
                    kind.label(),
                    id
                ));
            }
            continue;
        }

        seen.insert(id, v.clone());
        seen_from.insert(id, format!("{k:?}"));
        out.insert(u16_to_yaml_number(id), v.clone());
    }

    Ok(out)
}

fn resolve_inputs_in_document(resolver: &ScenarioNameResolver, doc: &mut Value) -> Result<(), String> {
    let Value::Mapping(root) = doc else {
        // Let the downstream deserializer produce a better error later.
        return Ok(());
    };

    let Some(Value::Sequence(inputs)) = root.get_mut(Value::String("inputs".to_string())) else {
        return Ok(());
    };

    for (i, ev) in inputs.iter_mut().enumerate() {
        let Value::Mapping(ev_map) = ev else {
            continue;
        };
        let Some(Value::Mapping(set_map)) = ev_map.get_mut(Value::String("set".to_string())) else {
            continue;
        };

        if let Some(Value::Mapping(di_map)) = set_map.get(Value::String("digital_inputs".to_string()))
        {
            let resolved = resolve_map_keys_to_u16(
                resolver,
                di_map,
                InputKind::Digital,
                &format!("inputs[{i}].set.digital_inputs"),
            )?;
            set_map.insert(
                Value::String("digital_inputs".to_string()),
                Value::Mapping(resolved),
            );
        }

        if let Some(Value::Mapping(ai_map)) = set_map.get(Value::String("analog_inputs".to_string()))
        {
            let resolved = resolve_map_keys_to_u16(
                resolver,
                ai_map,
                InputKind::Analog,
                &format!("inputs[{i}].set.analog_inputs"),
            )?;
            set_map.insert(
                Value::String("analog_inputs".to_string()),
                Value::Mapping(resolved),
            );
        }
    }

    Ok(())
}

fn resolve_target_field(
    resolver: &ScenarioNameResolver,
    item: &mut Value,
    field_path: &str,
    kind: InputKind,
) -> Result<(), String> {
    let Value::Mapping(m) = item else {
        return Ok(());
    };
    let key = Value::String("target".to_string());
    let Some(v) = m.get_mut(&key) else {
        return Ok(());
    };
    match v {
        Value::Number(_) => Ok(()),
        Value::String(s) => {
            let id = resolver.resolve_input_id(s, kind).map_err(|e| {
                format!(
                    "{e} at {field_path}\n\nhint: known device names include: {}",
                    resolver.known_names_preview(12)
                )
            })?;
            *v = u16_to_yaml_number(id);
            Ok(())
        }
        other => Err(format!(
            "invalid target type at {field_path} (expected string or integer): {other:?}"
        )),
    }
}

fn resolve_digital_bursts_in_document(
    resolver: &ScenarioNameResolver,
    doc: &mut Value,
) -> Result<(), String> {
    let Value::Mapping(root) = doc else {
        return Ok(());
    };
    let Some(Value::Sequence(bursts)) = root.get_mut(Value::String("digital_bursts".to_string()))
    else {
        return Ok(());
    };

    for (i, b) in bursts.iter_mut().enumerate() {
        resolve_target_field(
            resolver,
            b,
            &format!("digital_bursts[{i}].target"),
            InputKind::Digital,
        )?;
    }
    Ok(())
}

fn resolve_faults_in_document(resolver: &ScenarioNameResolver, doc: &mut Value) -> Result<(), String> {
    let Value::Mapping(root) = doc else {
        return Ok(());
    };
    let Some(Value::Sequence(faults)) = root.get_mut(Value::String("faults".to_string())) else {
        return Ok(());
    };

    for (i, f) in faults.iter_mut().enumerate() {
        let Value::Mapping(fm) = f else {
            continue;
        };
        let Some(Value::Mapping(stuck)) = fm.get_mut(Value::String("sensor_stuck".to_string()))
        else {
            continue;
        };
        let mut wrapped = Value::Mapping(stuck.clone());
        resolve_target_field(
            resolver,
            &mut wrapped,
            &format!("faults[{i}].sensor_stuck.target"),
            InputKind::Digital,
        )?;
        if let Value::Mapping(new_stuck) = wrapped {
            *stuck = new_stuck;
        }
    }
    Ok(())
}

/// Resolve device-name keys in a scenario YAML into numeric DI/AI ids using the given PLC's topology.
///
/// This is a preprocessor for authoring convenience: it keeps the stable backend format
/// (`sim::Scenario` expects numeric IDs) while allowing YAML like:
///
/// ```yaml
/// inputs:
///   - at_ms: 0
///     set:
///       digital_inputs:
///         start_button: true
/// ```
pub fn resolve_scenario_yaml_for_plc(
    plc_source: &str,
    scenario_yaml: &str,
) -> Result<String, String> {
    let resolver = ScenarioNameResolver::from_plc_source(plc_source)
        .map_err(|e| format!("Failed to build PLC topology for scenario name resolution: {e}"))?;

    let mut v: Value = serde_yaml::from_str(scenario_yaml)
        .map_err(|e| format!("Failed to parse scenario YAML: {e}"))?;

    resolve_inputs_in_document(&resolver, &mut v)?;
    resolve_digital_bursts_in_document(&resolver, &mut v)?;
    resolve_faults_in_document(&resolver, &mut v)?;

    let mut out = serde_yaml::to_string(&v)
        .map_err(|e| format!("Failed to serialize resolved scenario YAML: {e}"))?;
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLC_FIXTURE: &str = r#"
[topology]
device X0: digital_input
device AI0: analog_input { range: 0..100 }

device start_button: digital_input { connected_to: X0 }
device pressure_sensor: sensor { connected_to: AI0 }

[constraints]
[tasks]
task main:
    step halt:
"#;

    #[test]
    fn resolves_named_digital_and_analog_inputs() {
        let yaml = r#"
tick_ms: 10
duration_ms: 100
inputs:
  - at_ms: 0
    set:
      digital_inputs:
        start_button: true
      analog_inputs:
        pressure_sensor: 12.5
"#;
        let resolved = resolve_scenario_yaml_for_plc(PLC_FIXTURE, yaml).expect("resolve");
        let scenario = sim::Scenario::from_yaml_str(&resolved).expect("parse");
        assert_eq!(scenario.inputs[0].set.digital_inputs.get(&0), Some(&true));
        assert_eq!(scenario.inputs[0].set.analog_inputs.get(&0), Some(&12.5));
    }

    #[test]
    fn allows_mixed_numeric_and_name_when_values_match() {
        let yaml = r#"
tick_ms: 10
duration_ms: 100
inputs:
  - at_ms: 0
    set:
      digital_inputs:
        start_button: true
        0: true
"#;
        let resolved = resolve_scenario_yaml_for_plc(PLC_FIXTURE, yaml).expect("resolve");
        let scenario = sim::Scenario::from_yaml_str(&resolved).expect("parse");
        assert_eq!(scenario.inputs[0].set.digital_inputs.get(&0), Some(&true));
    }

    #[test]
    fn rejects_conflicting_assignments_to_same_id() {
        let yaml = r#"
tick_ms: 10
duration_ms: 100
inputs:
  - at_ms: 0
    set:
      digital_inputs:
        start_button: true
        0: false
"#;
        let err = resolve_scenario_yaml_for_plc(PLC_FIXTURE, yaml).unwrap_err();
        assert!(err.contains("conflicting assignments"), "err was: {err}");
    }

    #[test]
    fn unknown_name_errors_include_path() {
        let yaml = r#"
tick_ms: 10
duration_ms: 100
inputs:
  - at_ms: 0
    set:
      digital_inputs:
        does_not_exist: true
"#;
        let err = resolve_scenario_yaml_for_plc(PLC_FIXTURE, yaml).unwrap_err();
        assert!(
            err.contains("inputs[0].set.digital_inputs.does_not_exist"),
            "err was: {err}"
        );
    }
}
