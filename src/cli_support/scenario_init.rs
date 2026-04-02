use crate::cli_support::plc_pipeline::build_runtime_semantics;
use petgraph::Direction;
use runtime_core::Instr;
use rust_plc::ir::{DeviceKind, TopologyGraph};
use rust_plc::plc_port::{PlcPortKind, parse_physical_plc_port_ref};
use rust_plc::runtime_bridge::state_machine_to_runtime_program;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScenarioInitPreset {
    Minimal,
    Normal,
    Timeout,
    SensorStuck,
    Bounce,
}

impl ScenarioInitPreset {
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        match raw {
            "minimal" => Some(Self::Minimal),
            "normal" => Some(Self::Normal),
            "timeout" => Some(Self::Timeout),
            "sensor_stuck" => Some(Self::SensorStuck),
            "bounce" => Some(Self::Bounce),
            _ => None,
        }
    }

    pub(crate) fn expected_values() -> &'static str {
        "minimal|normal|timeout|sensor_stuck|bounce"
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Normal => "normal",
            Self::Timeout => "timeout",
            Self::SensorStuck => "sensor_stuck",
            Self::Bounce => "bounce",
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct ScenarioInitInputHints {
    pub(crate) digital_ids: Vec<u16>,
    pub(crate) analog_ids: Vec<u16>,
    pub(crate) physical_digital_ids: Vec<u16>,
    pub(crate) physical_analog_ids: Vec<u16>,
    pub(crate) digital_aliases: BTreeMap<u16, Vec<String>>,
    pub(crate) analog_aliases: BTreeMap<u16, Vec<String>>,
}

pub(crate) fn default_scenario_init_out_path(plc_path: &Path) -> PathBuf {
    let parent = plc_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = plc_path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("scenario");
    parent.join(format!("{stem}.scenario.yaml"))
}

pub(crate) fn collect_scenario_init_hints(
    plc_source: &str,
) -> Result<ScenarioInitInputHints, String> {
    let semantics = build_runtime_semantics(plc_source)?;
    let runtime = state_machine_to_runtime_program(
        &semantics.topology,
        &semantics.constraints,
        &semantics.state_machine,
        10,
    )
    .map_err(|e| e.to_string())?;

    let mut used_di = BTreeSet::<u16>::new();
    let mut used_ai = BTreeSet::<u16>::new();
    for task in runtime.tasks {
        for step in task.steps {
            match step.instr {
                Instr::WaitDigital { id, .. } => {
                    used_di.insert(id.0);
                }
                Instr::WaitAnalog { id, .. } => {
                    used_ai.insert(id.0);
                }
                Instr::WaitAllDigital { conditions, .. } => {
                    for condition in conditions {
                        used_di.insert(condition.id.0);
                    }
                }
                Instr::WaitExpr { .. }
                | Instr::WaitCamDigital { .. }
                | Instr::WaitCamAnalog { .. }
                | Instr::Action { .. }
                | Instr::Delay { .. }
                | Instr::Goto { .. }
                | Instr::Halt => {}
            }
        }
    }
    for cam in runtime.cam_configs {
        used_ai.insert(cam.master_input.0);
        used_ai.insert(cam.slave_feedback.0);
    }
    for pid in runtime.pid_loops {
        used_ai.insert(pid.pv.0);
    }

    let mut digital_aliases = BTreeMap::<u16, Vec<String>>::new();
    let mut analog_aliases = BTreeMap::<u16, Vec<String>>::new();
    for node in semantics.topology.graph.node_indices() {
        let device = &semantics.topology.graph[node];
        match device.kind {
            DeviceKind::DigitalInput => {
                if let Some(port) = parse_physical_plc_port_ref(&device.name) {
                    if !matches!(port.kind, PlcPortKind::DigitalInput) {
                        continue;
                    }
                    let aliases = collect_downstream_aliases(
                        &semantics.topology,
                        node,
                        is_physical_digital_input_name,
                    );
                    digital_aliases.insert(port.id, aliases);
                }
            }
            DeviceKind::AnalogInput => {
                if let Some(port) = parse_physical_plc_port_ref(&device.name) {
                    if !matches!(port.kind, PlcPortKind::AnalogInput) {
                        continue;
                    }
                    let aliases = collect_downstream_aliases(
                        &semantics.topology,
                        node,
                        is_physical_analog_input_name,
                    );
                    analog_aliases.insert(port.id, aliases);
                }
            }
            _ => {}
        }
    }

    if used_di.is_empty() {
        used_di.extend(digital_aliases.keys().copied());
    }
    if used_ai.is_empty() {
        used_ai.extend(analog_aliases.keys().copied());
    }

    let physical_digital_ids = digital_aliases.keys().copied().collect::<Vec<_>>();
    let physical_analog_ids = analog_aliases.keys().copied().collect::<Vec<_>>();

    Ok(ScenarioInitInputHints {
        digital_ids: used_di.into_iter().collect(),
        analog_ids: used_ai.into_iter().collect(),
        physical_digital_ids,
        physical_analog_ids,
        digital_aliases,
        analog_aliases,
    })
}

fn collect_downstream_aliases(
    topology: &TopologyGraph,
    node: petgraph::graph::NodeIndex,
    is_physical_input_name: fn(&str) -> bool,
) -> Vec<String> {
    let mut aliases = topology
        .graph
        .neighbors_directed(node, Direction::Outgoing)
        .chain(topology.graph.neighbors_directed(node, Direction::Incoming))
        .filter_map(|neighbor| {
            let name = topology.graph[neighbor].name.as_str();
            if is_physical_input_name(name) {
                return None;
            }
            Some(name.to_string())
        })
        .collect::<Vec<_>>();
    aliases.sort();
    aliases.dedup();
    aliases
}

fn is_physical_digital_input_name(name: &str) -> bool {
    matches!(
        parse_physical_plc_port_ref(name),
        Some(port) if matches!(port.kind, PlcPortKind::DigitalInput)
    )
}

fn is_physical_analog_input_name(name: &str) -> bool {
    matches!(
        parse_physical_plc_port_ref(name),
        Some(port) if matches!(port.kind, PlcPortKind::AnalogInput)
    )
}

fn render_input_alias_comment(aliases: &BTreeMap<u16, Vec<String>>, id: u16) -> String {
    let Some(names) = aliases.get(&id) else {
        return String::new();
    };
    if names.is_empty() {
        return String::new();
    }
    let shown = names.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
    if names.len() > 3 {
        format!(" # {shown}, ...")
    } else {
        format!(" # {shown}")
    }
}

pub(crate) fn aliases_contain_keyword(aliases: &[String], keyword: &str) -> bool {
    aliases
        .iter()
        .any(|name| name.to_ascii_lowercase().contains(keyword))
}

pub(crate) fn render_scenario_init_yaml(
    plc_path: &Path,
    preset: ScenarioInitPreset,
    hints: &ScenarioInitInputHints,
) -> String {
    let source_name = plc_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("<unknown>");

    let mut out = String::new();
    out.push_str("# Generated by `rust_plc scenario-init`.\n");
    out.push_str(&format!("# Source PLC: {source_name}\n"));
    out.push_str(&format!("# Preset: {}\n", preset.as_str()));
    out.push_str("# Keep `at_ms` aligned to `tick_ms`, and keep `at_ms` < `duration_ms`.\n");
    out.push_str("tick_ms: 10\n");
    match preset {
        ScenarioInitPreset::Minimal => out.push_str("duration_ms: 1000\n\n"),
        ScenarioInitPreset::Normal => out.push_str("duration_ms: 6000\n\n"),
        ScenarioInitPreset::Timeout => out.push_str("duration_ms: 2000\n\n"),
        ScenarioInitPreset::SensorStuck => out.push_str("duration_ms: 3000\n\n"),
        ScenarioInitPreset::Bounce => out.push_str("duration_ms: 1000\n\n"),
    }

    if hints.digital_ids.is_empty() && hints.analog_ids.is_empty() {
        out.push_str("# No physical X*/AI* inputs were discovered from this PLC topology.\n");
    }

    let start_id = hints.digital_aliases.iter().find_map(|(&id, aliases)| {
        if aliases_contain_keyword(aliases, "start") {
            Some(id)
        } else {
            None
        }
    });
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

    match preset {
        ScenarioInitPreset::Minimal => {
            out.push_str("# Add input events under `inputs`, for example:\n");
            out.push_str("# - at_ms: 0\n");
            out.push_str("#   set:\n");
            out.push_str("#     digital_inputs:\n");
            out.push_str("#       0: true\n");
            out.push_str("#     analog_inputs:\n");
            out.push_str("#       0: 1.0\n");
            out.push_str("inputs: []\n");
        }
        ScenarioInitPreset::Normal
        | ScenarioInitPreset::Timeout
        | ScenarioInitPreset::SensorStuck
        | ScenarioInitPreset::Bounce => {
            out.push_str("inputs:\n");
            if let Some(start_id) = start_id {
                match preset {
                    ScenarioInitPreset::Bounce => {
                        let toggles = [
                            (0, true),
                            (10, false),
                            (20, true),
                            (30, false),
                            (40, true),
                            (50, false),
                        ];
                        for (at_ms, value) in toggles {
                            out.push_str(&format!("  - at_ms: {at_ms}\n"));
                            out.push_str("    set:\n");
                            out.push_str("      digital_inputs:\n");
                            let suffix =
                                render_input_alias_comment(&hints.digital_aliases, start_id);
                            out.push_str(&format!("        {start_id}: {value}{suffix}\n"));
                            if at_ms == 0 && !hints.analog_ids.is_empty() {
                                out.push_str("      analog_inputs:\n");
                                for id in &hints.analog_ids {
                                    let suffix =
                                        render_input_alias_comment(&hints.analog_aliases, *id);
                                    out.push_str(&format!("        {id}: 0.0{suffix}\n"));
                                }
                            }
                        }
                    }
                    _ => {
                        out.push_str("  - at_ms: 0\n");
                        out.push_str("    set:\n");
                        out.push_str("      digital_inputs:\n");
                        let suffix = render_input_alias_comment(&hints.digital_aliases, start_id);
                        out.push_str(&format!("        {start_id}: true{suffix}\n"));
                        if !hints.analog_ids.is_empty() {
                            out.push_str("      analog_inputs:\n");
                            for id in &hints.analog_ids {
                                let suffix = render_input_alias_comment(&hints.analog_aliases, *id);
                                out.push_str(&format!("        {id}: 0.0{suffix}\n"));
                            }
                        }
                        out.push_str("  - at_ms: 50\n");
                        out.push_str("    set:\n");
                        out.push_str("      digital_inputs:\n");
                        out.push_str(&format!("        {start_id}: false{suffix}\n"));
                    }
                }
            } else {
                out.push_str("  - at_ms: 0\n");
                out.push_str("    set:\n");
                out.push_str("      digital_inputs:\n");
                out.push_str("        0: true  # (example) press start button\n");
                if !hints.analog_ids.is_empty() {
                    out.push_str("      analog_inputs:\n");
                    for id in &hints.analog_ids {
                        let suffix = render_input_alias_comment(&hints.analog_aliases, *id);
                        out.push_str(&format!("        {id}: 0.0{suffix}\n"));
                    }
                }
                out.push_str("  - at_ms: 50\n");
                out.push_str("    set:\n");
                out.push_str("      digital_inputs:\n");
                out.push_str("        0: false # (example) release\n");
            }

            if preset == ScenarioInitPreset::Normal {
                let mut at_ms = 100u64;
                for id in &sensor_ids {
                    out.push_str(&format!("  - at_ms: {at_ms}\n"));
                    out.push_str("    set:\n");
                    out.push_str("      digital_inputs:\n");
                    let suffix = render_input_alias_comment(&hints.digital_aliases, *id);
                    out.push_str(&format!("        {id}: true{suffix}\n"));
                    at_ms = at_ms.saturating_add(20);
                    if at_ms >= 1000 {
                        break;
                    }
                }
            }

            if preset == ScenarioInitPreset::SensorStuck {
                let target = sensor_ids.first().copied().or(start_id).unwrap_or(0);
                out.push_str("\n# Fault injection example:\n");
                out.push_str("faults:\n");
                out.push_str("  - sensor_stuck:\n");
                out.push_str("      at_ms: 200\n");
                let suffix = render_input_alias_comment(&hints.digital_aliases, target);
                out.push_str(&format!("      target: {target}{suffix}\n"));
                out.push_str("      value: true\n");
            }
        }
    }

    out.push_str("\n# Force/override (optional). Use YAML `null` to clear a forced value.\n");
    out.push_str("# Example:\n");
    out.push_str("# forces:\n");
    out.push_str("#   - at_ms: 0\n");
    out.push_str("#     set:\n");
    out.push_str("#       digital_inputs:\n");
    out.push_str("#         0: true\n");
    out.push_str("#   - at_ms: 100\n");
    out.push_str("#     set:\n");
    out.push_str("#       digital_inputs:\n");
    out.push_str("#         0: null\n");
    out.push_str("forces: []\n");
    out
}
