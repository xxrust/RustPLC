use crate::ir::{DeviceKind, TopologyGraph};
use crate::parser::parse_plc;
use crate::semantic::{build_topology_graph, preprocess_program};
use petgraph::graph::NodeIndex;
use petgraph::Direction;
use serde_yaml::{Mapping, Number, Value};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

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

        let start = self
            .by_name
            .get(raw)
            .copied()
            .ok_or_else(|| format!("unknown {} name `{raw}`", kind.label()))?;

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
                format!("invalid numeric key at {path_prefix} (expected 0..=65535 integer): {k:?}")
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

fn resolve_inputs_in_document(
    resolver: &ScenarioNameResolver,
    doc: &mut Value,
) -> Result<(), String> {
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

        if let Some(Value::Mapping(di_map)) =
            set_map.get(Value::String("digital_inputs".to_string()))
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

        if let Some(Value::Mapping(ai_map)) =
            set_map.get(Value::String("analog_inputs".to_string()))
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

fn resolve_faults_in_document(
    resolver: &ScenarioNameResolver,
    doc: &mut Value,
) -> Result<(), String> {
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

#[derive(Debug, Default)]
struct ExpandedAtMs {
    di: HashMap<u16, (Value, String)>,
    ai: HashMap<u16, (Value, String)>,
}

fn yaml_key(name: &str) -> Value {
    Value::String(name.to_string())
}

fn get_u64_field(m: &Mapping, key: &str, path: &str) -> Result<u64, String> {
    let Some(v) = m.get(&yaml_key(key)) else {
        return Err(format!("missing required field `{key}` at {path}"));
    };
    let Some(n) = v.as_u64() else {
        return Err(format!("invalid `{key}` at {path} (expected u64): {v:?}"));
    };
    Ok(n)
}

fn get_bool_field_opt(m: &Mapping, key: &str, path: &str) -> Result<Option<bool>, String> {
    let Some(v) = m.get(&yaml_key(key)) else {
        return Ok(None);
    };
    let Some(b) = v.as_bool() else {
        return Err(format!("invalid `{key}` at {path} (expected bool): {v:?}"));
    };
    Ok(Some(b))
}

fn get_number_field_opt<'a>(m: &'a Mapping, key: &str) -> Option<&'a Value> {
    m.get(&yaml_key(key))
}

fn resolve_id_value(
    resolver: &ScenarioNameResolver,
    kind: InputKind,
    v: &Value,
    path: &str,
) -> Result<u16, String> {
    if let Some(id) = val_to_u16_number(v) {
        return Ok(id);
    }
    if let Value::String(s) = v {
        return resolver.resolve_input_id(s, kind).map_err(|e| {
            format!(
                "{e} at {path}\n\nhint: known device names include: {}",
                resolver.known_names_preview(12)
            )
        });
    }
    Err(format!(
        "invalid target at {path} (expected string or integer): {v:?}"
    ))
}

fn ensure_aligned(name: &str, value_ms: u64, tick_ms: u64, path: &str) -> Result<(), String> {
    if tick_ms == 0 {
        return Err("tick_ms must be > 0".to_string());
    }
    if value_ms % tick_ms != 0 {
        return Err(format!(
            "{name} must be aligned to tick_ms ({tick_ms}) at {path}; got {value_ms}"
        ));
    }
    Ok(())
}

fn ensure_lt_duration(at_ms: u64, duration_ms: u64, path: &str) -> Result<(), String> {
    if duration_ms != 0 && at_ms >= duration_ms {
        return Err(format!(
            "at_ms must be < duration_ms ({duration_ms}) at {path}; got {at_ms}"
        ));
    }
    Ok(())
}

fn insert_di(
    by_at_ms: &mut BTreeMap<u64, ExpandedAtMs>,
    at_ms: u64,
    id: u16,
    value: bool,
    source: String,
) -> Result<(), String> {
    let slot = by_at_ms.entry(at_ms).or_default();
    let v = Value::Bool(value);
    if let Some((prev, prev_src)) = slot.di.get(&id) {
        if prev != &v {
            return Err(format!(
                "conflicting assignments for digital_input id {id} at inputs(at_ms={at_ms}): {prev_src} vs {source}"
            ));
        }
        return Ok(());
    }
    slot.di.insert(id, (v, source));
    Ok(())
}

fn insert_ai(
    by_at_ms: &mut BTreeMap<u64, ExpandedAtMs>,
    at_ms: u64,
    id: u16,
    value: Value,
    source: String,
) -> Result<(), String> {
    let slot = by_at_ms.entry(at_ms).or_default();
    if let Some((prev, prev_src)) = slot.ai.get(&id) {
        if prev != &value {
            return Err(format!(
                "conflicting assignments for analog_input id {id} at inputs(at_ms={at_ms}): {prev_src} vs {source}"
            ));
        }
        return Ok(());
    }
    slot.ai.insert(id, (value, source));
    Ok(())
}

fn collect_existing_inputs(doc: &Value) -> Result<BTreeMap<u64, ExpandedAtMs>, String> {
    let mut out = BTreeMap::<u64, ExpandedAtMs>::new();

    let Value::Mapping(root) = doc else {
        return Ok(out);
    };
    let Some(Value::Sequence(inputs)) = root.get(&yaml_key("inputs")) else {
        return Ok(out);
    };

    for (i, ev) in inputs.iter().enumerate() {
        let Value::Mapping(ev_map) = ev else { continue };
        let Some(at_ms) = ev_map.get(&yaml_key("at_ms")).and_then(|v| v.as_u64()) else {
            continue;
        };
        let Some(Value::Mapping(set_map)) = ev_map.get(&yaml_key("set")) else {
            continue;
        };

        if let Some(Value::Mapping(di_map)) = set_map.get(&yaml_key("digital_inputs")) {
            for (k, v) in di_map {
                let Some(id) = val_to_u16_number(k) else {
                    continue;
                };
                let Some(b) = v.as_bool() else { continue };
                insert_di(
                    &mut out,
                    at_ms,
                    id,
                    b,
                    format!("inputs[{i}].set.digital_inputs.{k:?}"),
                )?;
            }
        }
        if let Some(Value::Mapping(ai_map)) = set_map.get(&yaml_key("analog_inputs")) {
            for (k, v) in ai_map {
                let Some(id) = val_to_u16_number(k) else {
                    continue;
                };
                insert_ai(
                    &mut out,
                    at_ms,
                    id,
                    v.clone(),
                    format!("inputs[{i}].set.analog_inputs.{k:?}"),
                )?;
            }
        }
    }

    Ok(out)
}

fn expand_pulse_sugar(
    resolver: &ScenarioNameResolver,
    root: &Mapping,
    tick_ms: u64,
    duration_ms: u64,
    by_at_ms: &mut BTreeMap<u64, ExpandedAtMs>,
) -> Result<(), String> {
    let pulse_val = root
        .get(&yaml_key("pulse"))
        .or_else(|| root.get(&yaml_key("pulses")));
    let Some(pulse_val) = pulse_val else {
        return Ok(());
    };
    let Value::Sequence(pulses) = pulse_val else {
        return Err("`pulse` must be a YAML list".to_string());
    };

    for (i, p) in pulses.iter().enumerate() {
        let path = format!("pulse[{i}]");
        let Value::Mapping(pm) = p else {
            return Err(format!("invalid pulse entry at {path} (expected map)"));
        };

        let at_ms = get_u64_field(pm, "at_ms", &path)?;
        let width_ms = get_u64_field(pm, "width_ms", &path)?;
        if width_ms == 0 {
            return Err(format!("width_ms must be > 0 at {path}.width_ms"));
        }
        ensure_aligned("at_ms", at_ms, tick_ms, &format!("{path}.at_ms"))?;
        ensure_aligned("width_ms", width_ms, tick_ms, &format!("{path}.width_ms"))?;
        ensure_lt_duration(at_ms, duration_ms, &format!("{path}.at_ms"))?;

        let Some(target_v) = pm
            .get(&yaml_key("digital"))
            .or_else(|| pm.get(&yaml_key("target")))
        else {
            return Err(format!("missing required field `digital` at {path}"));
        };
        let id = resolve_id_value(
            resolver,
            InputKind::Digital,
            target_v,
            &format!("{path}.digital"),
        )?;

        let value = get_bool_field_opt(pm, "value", &format!("{path}.value"))?.unwrap_or(true);
        let inactive_value =
            get_bool_field_opt(pm, "inactive_value", &format!("{path}.inactive_value"))?
                .unwrap_or(false);

        let release_at = at_ms.saturating_add(width_ms);
        ensure_aligned(
            "release_at",
            release_at,
            tick_ms,
            &format!("{path}.width_ms"),
        )?;
        ensure_lt_duration(release_at, duration_ms, &format!("{path}.width_ms"))?;

        insert_di(by_at_ms, at_ms, id, value, format!("{path} (press)"))?;
        insert_di(
            by_at_ms,
            release_at,
            id,
            inactive_value,
            format!("{path} (release)"),
        )?;
    }
    Ok(())
}

fn expand_hold_sugar(
    resolver: &ScenarioNameResolver,
    root: &Mapping,
    tick_ms: u64,
    duration_ms: u64,
    by_at_ms: &mut BTreeMap<u64, ExpandedAtMs>,
) -> Result<(), String> {
    let hold_val = root
        .get(&yaml_key("hold"))
        .or_else(|| root.get(&yaml_key("holds")));
    let Some(hold_val) = hold_val else {
        return Ok(());
    };
    let Value::Sequence(holds) = hold_val else {
        return Err("`hold` must be a YAML list".to_string());
    };

    for (i, h) in holds.iter().enumerate() {
        let path = format!("hold[{i}]");
        let Value::Mapping(hm) = h else {
            return Err(format!("invalid hold entry at {path} (expected map)"));
        };

        let from_ms = get_u64_field(hm, "from_ms", &path)?;
        let to_ms = get_u64_field(hm, "to_ms", &path)?;
        if to_ms <= from_ms {
            return Err(format!(
                "to_ms must be > from_ms at {path} (from_ms={from_ms}, to_ms={to_ms})"
            ));
        }
        ensure_aligned("from_ms", from_ms, tick_ms, &format!("{path}.from_ms"))?;
        ensure_aligned("to_ms", to_ms, tick_ms, &format!("{path}.to_ms"))?;
        ensure_lt_duration(from_ms, duration_ms, &format!("{path}.from_ms"))?;
        ensure_lt_duration(to_ms, duration_ms, &format!("{path}.to_ms"))?;

        let digital = hm.get(&yaml_key("digital"));
        let analog = hm.get(&yaml_key("analog"));
        let (kind, target_v) = match (digital, analog) {
            (Some(d), None) => (InputKind::Digital, d),
            (None, Some(a)) => (InputKind::Analog, a),
            _ => {
                return Err(format!(
                    "hold entry at {path} must contain exactly one of `digital` or `analog`"
                ));
            }
        };

        let id = resolve_id_value(resolver, kind, target_v, &format!("{path}.target"))?;

        let Some(value_v) =
            get_number_field_opt(hm, "value").or_else(|| hm.get(&yaml_key("value")))
        else {
            return Err(format!("missing required field `value` at {path}.value"));
        };

        match kind {
            InputKind::Digital => {
                let Some(b) = value_v.as_bool() else {
                    return Err(format!(
                        "invalid value at {path}.value (expected bool): {value_v:?}"
                    ));
                };
                let release =
                    get_bool_field_opt(hm, "release_value", &format!("{path}.release_value"))?
                        .unwrap_or(false);
                insert_di(by_at_ms, from_ms, id, b, format!("{path} (hold start)"))?;
                insert_di(by_at_ms, to_ms, id, release, format!("{path} (hold end)"))?;
            }
            InputKind::Analog => {
                if value_v.as_f64().is_none()
                    && value_v.as_i64().is_none()
                    && value_v.as_u64().is_none()
                {
                    return Err(format!(
                        "invalid value at {path}.value (expected number): {value_v:?}"
                    ));
                }
                let release_v = hm
                    .get(&yaml_key("release_value"))
                    .cloned()
                    .unwrap_or_else(|| Value::Number(Number::from(0)));
                insert_ai(
                    by_at_ms,
                    from_ms,
                    id,
                    value_v.clone(),
                    format!("{path} (hold start)"),
                )?;
                insert_ai(by_at_ms, to_ms, id, release_v, format!("{path} (hold end)"))?;
            }
        }
    }
    Ok(())
}

fn apply_sugar_expansion_in_document(
    resolver: &ScenarioNameResolver,
    doc: &mut Value,
) -> Result<(), String> {
    // Collect existing resolved `inputs` first to avoid borrow conflicts with the root mapping.
    let mut by_at_ms = collect_existing_inputs(doc)?;

    let Value::Mapping(root) = doc else {
        return Ok(());
    };

    let tick_ms = root
        .get(&yaml_key("tick_ms"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let duration_ms = root
        .get(&yaml_key("duration_ms"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    expand_pulse_sugar(resolver, root, tick_ms, duration_ms, &mut by_at_ms)?;
    expand_hold_sugar(resolver, root, tick_ms, duration_ms, &mut by_at_ms)?;

    // Remove sugar keys so downstream `sim::Scenario` (numeric backend) can deserialize.
    root.remove(&yaml_key("pulse"));
    root.remove(&yaml_key("pulses"));
    root.remove(&yaml_key("hold"));
    root.remove(&yaml_key("holds"));

    if by_at_ms.is_empty() {
        return Ok(());
    }

    let mut inputs_seq = Vec::<Value>::new();
    for (at_ms, sets) in by_at_ms {
        let mut ev = Mapping::new();
        ev.insert(yaml_key("at_ms"), Value::Number(Number::from(at_ms)));

        let mut set_map = Mapping::new();
        if !sets.di.is_empty() {
            let mut di_map = Mapping::new();
            let mut di_ids = sets.di.into_iter().collect::<Vec<_>>();
            di_ids.sort_by_key(|(id, _)| *id);
            for (id, (v, _src)) in di_ids {
                di_map.insert(u16_to_yaml_number(id), v);
            }
            set_map.insert(yaml_key("digital_inputs"), Value::Mapping(di_map));
        }
        if !sets.ai.is_empty() {
            let mut ai_map = Mapping::new();
            let mut ai_ids = sets.ai.into_iter().collect::<Vec<_>>();
            ai_ids.sort_by_key(|(id, _)| *id);
            for (id, (v, _src)) in ai_ids {
                ai_map.insert(u16_to_yaml_number(id), v);
            }
            set_map.insert(yaml_key("analog_inputs"), Value::Mapping(ai_map));
        }
        ev.insert(yaml_key("set"), Value::Mapping(set_map));
        inputs_seq.push(Value::Mapping(ev));
    }

    root.insert(yaml_key("inputs"), Value::Sequence(inputs_seq));
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
    apply_sugar_expansion_in_document(&resolver, &mut v)?;

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

device start_button: digital_input { driven_by: X0 }
device pressure_sensor: sensor { driven_by: AI0 }

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

    #[test]
    fn pulse_sugar_expands_to_press_and_release() {
        let yaml = r#"
tick_ms: 10
duration_ms: 100
pulse:
  - at_ms: 0
    width_ms: 20
    digital: start_button
"#;
        let resolved = resolve_scenario_yaml_for_plc(PLC_FIXTURE, yaml).expect("resolve");
        let scenario = sim::Scenario::from_yaml_str(&resolved).expect("parse");
        assert_eq!(scenario.inputs.len(), 2);
        assert_eq!(scenario.inputs[0].at_ms, 0);
        assert_eq!(scenario.inputs[0].set.digital_inputs.get(&0), Some(&true));
        assert_eq!(scenario.inputs[1].at_ms, 20);
        assert_eq!(scenario.inputs[1].set.digital_inputs.get(&0), Some(&false));
    }

    #[test]
    fn hold_sugar_expands_to_set_and_release() {
        let yaml = r#"
tick_ms: 10
duration_ms: 100
hold:
  - from_ms: 10
    to_ms: 40
    digital: start_button
    value: true
"#;
        let resolved = resolve_scenario_yaml_for_plc(PLC_FIXTURE, yaml).expect("resolve");
        let scenario = sim::Scenario::from_yaml_str(&resolved).expect("parse");
        assert_eq!(scenario.inputs.len(), 2);
        assert_eq!(scenario.inputs[0].at_ms, 10);
        assert_eq!(scenario.inputs[0].set.digital_inputs.get(&0), Some(&true));
        assert_eq!(scenario.inputs[1].at_ms, 40);
        assert_eq!(scenario.inputs[1].set.digital_inputs.get(&0), Some(&false));
    }

    #[test]
    fn pulse_width_must_align_to_tick() {
        let yaml = r#"
tick_ms: 10
duration_ms: 100
pulse:
  - at_ms: 0
    width_ms: 15
    digital: start_button
"#;
        let err = resolve_scenario_yaml_for_plc(PLC_FIXTURE, yaml).unwrap_err();
        assert!(err.contains("pulse[0].width_ms"), "err was: {err}");
    }
}
