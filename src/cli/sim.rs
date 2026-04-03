use crate::cli_support::common::{DispatchResult, display_path_relative_to_cwd};
use crate::cli_support::help::command_usage;
use crate::cli_support::plc_pipeline::compile_plc_to_runtime_program;
use crate::cli_support::runtime_probe::io_sizes_for_program_and_scenario;
use crate::cli_support::scenario_yaml::{
    format_resolve_scenario_yaml_error, parse_scenario_yaml, read_scenario_yaml_file,
    scenario_mismatch_hint_for_example,
};
use io_traits::{AnalogInputId, AnalogOutputId, DigitalInputId, DigitalOutputId, Io};
use runtime_core::{Action, Instr, Program, Step, StepId, Task};
use rust_plc::alarm_runtime::{
    AlarmBuildInput, AlarmDispatchConfig, AlarmDispatcher, AlarmSeverity, build_alarm_event,
};
use rust_plc::diagnostics::{
    DiagnosisInput, EvidenceSource, IoSnapshotArtifact, IoTickSnapshot, diagnose,
};
use rust_plc::scenario_resolve::resolve_scenario_yaml_for_plc;
use rust_plc::sim_regress::{SimRegressOptions, SimRegressSummary, run_sim_regress_with_options};
use rust_plc::source_bundle::load_plc_source;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub(super) fn try_dispatch(
    program: &str,
    command: &str,
    remaining: &[String],
) -> Option<DispatchResult> {
    let result = match command {
        "sim" => run_sim_subcommand(program, remaining.iter().cloned()),
        "sim-plc" => run_sim_plc_subcommand(program, remaining.iter().cloned()),
        "sim-regress" => run_sim_regress_subcommand(program, remaining.iter().cloned()),
        "sim-pid-kpi" => run_sim_pid_kpi_subcommand(program, remaining.iter().cloned()),
        _ => return None,
    };
    Some(DispatchResult {
        error_prefix: None,
        result,
    })
}

static SIM_STEP1_ACTIONS: [Action; 1] = [Action::SetDigital {
    id: DigitalOutputId(0),
    value: true,
}];

// A deliberately tiny runtime-core program used by the `sim` subcommand.
//
// wait di0 == true -> set do0 true -> halt
static SIM_STEPS: [Step<'static>; 3] = [
    Step {
        name: "wait_di0_true",
        instr: Instr::WaitDigital {
            id: DigitalInputId(0),
            equals: true,
            next: StepId(1),
            timeout: None,
        },
    },
    Step {
        name: "set_do0_true",
        instr: Instr::Action {
            actions: &SIM_STEP1_ACTIONS,
            next: StepId(2),
        },
    },
    Step {
        name: "halt",
        instr: Instr::Halt,
    },
];

static SIM_TASKS: [Task<'static>; 1] = [Task {
    name: "main",
    steps: &SIM_STEPS,
    entry: StepId(0),
}];

static SIM_PROGRAM: Program<'static> = Program {
    tasks: &SIM_TASKS,
    pid_loops: &[],
    var_init: &[],
    cam_configs: &[],
    cam_tables: &[],
    axis_fault_policies: &[],
    semantic_resources: &[],
    resource_claims: &[],
    workpiece_types: &[],
    workpiece_sites: &[],
    workpiece_holders: &[],
};

#[derive(Debug, Clone, Copy)]
enum OnlineForceChannelKind {
    Di,
    Ai,
    Do,
    Ao,
}

impl OnlineForceChannelKind {
    fn label(self) -> &'static str {
        match self {
            Self::Di => "digital_input",
            Self::Ai => "analog_input",
            Self::Do => "digital_output",
            Self::Ao => "analog_output",
        }
    }

    fn short(self) -> &'static str {
        match self {
            Self::Di => "di",
            Self::Ai => "ai",
            Self::Do => "do",
            Self::Ao => "ao",
        }
    }
}

#[derive(Debug, Clone)]
enum OnlineForceValue {
    Digital(bool),
    Analog(f32),
}

#[derive(Debug, Clone)]
struct OnlineForceCommand {
    at_ms: u64,
    actor: String,
    source: String,
    channel_kind: OnlineForceChannelKind,
    channel_id: u16,
    value: Option<OnlineForceValue>,
}

#[derive(Debug, Deserialize)]
struct OnlineForceScriptEntryRaw {
    at_ms: u64,
    actor: String,
    source: String,
    channel: String,
    #[serde(default)]
    value: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum ForceAuditValue {
    Digital(bool),
    Analog(f32),
}

#[derive(Debug, Clone, Serialize)]
struct OnlineForceAuditEntry {
    at_ms: u64,
    tick: u64,
    actor: String,
    source: String,
    channel: String,
    channel_kind: &'static str,
    channel_id: u16,
    operation: &'static str,
    from: Option<ForceAuditValue>,
    to: Option<ForceAuditValue>,
}

fn parse_online_force_channel(raw: &str) -> Result<(OnlineForceChannelKind, u16), String> {
    let token = raw.trim().to_ascii_lowercase();
    let (kind, tail) = if let Some(v) = token.strip_prefix("di") {
        (OnlineForceChannelKind::Di, v)
    } else if let Some(v) = token.strip_prefix("ai") {
        (OnlineForceChannelKind::Ai, v)
    } else if let Some(v) = token.strip_prefix("do") {
        (OnlineForceChannelKind::Do, v)
    } else if let Some(v) = token.strip_prefix("ao") {
        (OnlineForceChannelKind::Ao, v)
    } else {
        return Err(format!(
            "invalid channel `{raw}` (expected DI<n>/AI<n>/DO<n>/AO<n>)"
        ));
    };

    if tail.is_empty() {
        return Err(format!(
            "invalid channel `{raw}` (missing numeric id after kind prefix)"
        ));
    }
    let id = tail
        .parse::<u16>()
        .map_err(|_| format!("invalid channel `{raw}` (id must be u16)"))?;
    Ok((kind, id))
}

fn parse_online_force_value(
    raw: Option<serde_json::Value>,
    kind: OnlineForceChannelKind,
) -> Result<Option<OnlineForceValue>, String> {
    let Some(v) = raw else {
        return Ok(None);
    };
    match kind {
        OnlineForceChannelKind::Di | OnlineForceChannelKind::Do => match v {
            serde_json::Value::Bool(b) => Ok(Some(OnlineForceValue::Digital(b))),
            serde_json::Value::Null => Ok(None),
            other => Err(format!(
                "{} channel expects bool/null value, got {other}",
                kind.short()
            )),
        },
        OnlineForceChannelKind::Ai | OnlineForceChannelKind::Ao => match v {
            serde_json::Value::Number(n) => {
                let f = n.as_f64().ok_or_else(|| {
                    format!(
                        "{} channel expects numeric/null value, got non-finite number",
                        kind.short()
                    )
                })?;
                if !f.is_finite() {
                    return Err(format!(
                        "{} channel expects finite numeric/null value",
                        kind.short()
                    ));
                }
                Ok(Some(OnlineForceValue::Analog(f as f32)))
            }
            serde_json::Value::Null => Ok(None),
            other => Err(format!(
                "{} channel expects numeric/null value, got {other}",
                kind.short()
            )),
        },
    }
}

fn load_online_force_script(path: &Path, tick_ms: u64) -> Result<Vec<OnlineForceCommand>, String> {
    let body = fs::read_to_string(path).map_err(|err| {
        format!(
            "Failed to read online-force script {}: {err}",
            path.display()
        )
    })?;
    let mut commands = Vec::<OnlineForceCommand>::new();
    for (lineno, line) in body.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let raw: OnlineForceScriptEntryRaw = serde_json::from_str(trimmed)
            .map_err(|err| format!("Invalid JSONL at {}:{}: {err}", path.display(), lineno + 1))?;
        if tick_ms != 0 && raw.at_ms % tick_ms != 0 {
            return Err(format!(
                "at_ms={} is not aligned to tick_ms={} at {}:{}",
                raw.at_ms,
                tick_ms,
                path.display(),
                lineno + 1
            ));
        }
        let (kind, id) = parse_online_force_channel(&raw.channel)
            .map_err(|err| format!("{err} at {}:{}", path.display(), lineno + 1))?;
        let value = parse_online_force_value(raw.value, kind)
            .map_err(|err| format!("{err} at {}:{}", path.display(), lineno + 1))?;
        commands.push(OnlineForceCommand {
            at_ms: raw.at_ms,
            actor: raw.actor,
            source: raw.source,
            channel_kind: kind,
            channel_id: id,
            value,
        });
    }
    commands.sort_by(|a, b| a.at_ms.cmp(&b.at_ms));
    Ok(commands)
}

fn build_online_force_audit(
    commands: &[OnlineForceCommand],
    tick_ms: u64,
) -> Vec<OnlineForceAuditEntry> {
    let mut out = Vec::<OnlineForceAuditEntry>::new();
    let mut di = BTreeMap::<u16, bool>::new();
    let mut ai = BTreeMap::<u16, f32>::new();
    let mut do_ = BTreeMap::<u16, bool>::new();
    let mut ao = BTreeMap::<u16, f32>::new();

    for cmd in commands {
        let (from, to) = match cmd.channel_kind {
            OnlineForceChannelKind::Di => {
                let before = di
                    .get(&cmd.channel_id)
                    .copied()
                    .map(ForceAuditValue::Digital);
                match cmd.value.as_ref() {
                    Some(OnlineForceValue::Digital(v)) => {
                        di.insert(cmd.channel_id, *v);
                        (before, Some(ForceAuditValue::Digital(*v)))
                    }
                    None => {
                        di.remove(&cmd.channel_id);
                        (before, None)
                    }
                    Some(OnlineForceValue::Analog(_)) => continue,
                }
            }
            OnlineForceChannelKind::Ai => {
                let before = ai
                    .get(&cmd.channel_id)
                    .copied()
                    .map(ForceAuditValue::Analog);
                match cmd.value.as_ref() {
                    Some(OnlineForceValue::Analog(v)) => {
                        ai.insert(cmd.channel_id, *v);
                        (before, Some(ForceAuditValue::Analog(*v)))
                    }
                    None => {
                        ai.remove(&cmd.channel_id);
                        (before, None)
                    }
                    Some(OnlineForceValue::Digital(_)) => continue,
                }
            }
            OnlineForceChannelKind::Do => {
                let before = do_
                    .get(&cmd.channel_id)
                    .copied()
                    .map(ForceAuditValue::Digital);
                match cmd.value.as_ref() {
                    Some(OnlineForceValue::Digital(v)) => {
                        do_.insert(cmd.channel_id, *v);
                        (before, Some(ForceAuditValue::Digital(*v)))
                    }
                    None => {
                        do_.remove(&cmd.channel_id);
                        (before, None)
                    }
                    Some(OnlineForceValue::Analog(_)) => continue,
                }
            }
            OnlineForceChannelKind::Ao => {
                let before = ao
                    .get(&cmd.channel_id)
                    .copied()
                    .map(ForceAuditValue::Analog);
                match cmd.value.as_ref() {
                    Some(OnlineForceValue::Analog(v)) => {
                        ao.insert(cmd.channel_id, *v);
                        (before, Some(ForceAuditValue::Analog(*v)))
                    }
                    None => {
                        ao.remove(&cmd.channel_id);
                        (before, None)
                    }
                    Some(OnlineForceValue::Digital(_)) => continue,
                }
            }
        };

        out.push(OnlineForceAuditEntry {
            at_ms: cmd.at_ms,
            tick: if tick_ms == 0 { 0 } else { cmd.at_ms / tick_ms },
            actor: cmd.actor.clone(),
            source: cmd.source.clone(),
            channel: format!("{}{}", cmd.channel_kind.short(), cmd.channel_id),
            channel_kind: cmd.channel_kind.label(),
            channel_id: cmd.channel_id,
            operation: if cmd.value.is_some() { "set" } else { "clear" },
            from,
            to,
        });
    }

    out
}

fn inject_online_force_commands(
    scenario: &mut sim::Scenario,
    commands: &[OnlineForceCommand],
) -> Result<(), String> {
    let mut by_at = BTreeMap::<u64, sim::ForceSet>::new();
    for cmd in commands {
        let set = by_at.entry(cmd.at_ms).or_default();
        match (cmd.channel_kind, cmd.value.as_ref()) {
            (OnlineForceChannelKind::Di, Some(OnlineForceValue::Digital(v))) => {
                set.digital_inputs.insert(cmd.channel_id, Some(*v));
            }
            (OnlineForceChannelKind::Di, None) => {
                set.digital_inputs.insert(cmd.channel_id, None);
            }
            (OnlineForceChannelKind::Ai, Some(OnlineForceValue::Analog(v))) => {
                set.analog_inputs.insert(cmd.channel_id, Some(*v));
            }
            (OnlineForceChannelKind::Ai, None) => {
                set.analog_inputs.insert(cmd.channel_id, None);
            }
            (OnlineForceChannelKind::Do, Some(OnlineForceValue::Digital(v))) => {
                set.digital_outputs.insert(cmd.channel_id, Some(*v));
            }
            (OnlineForceChannelKind::Do, None) => {
                set.digital_outputs.insert(cmd.channel_id, None);
            }
            (OnlineForceChannelKind::Ao, Some(OnlineForceValue::Analog(v))) => {
                set.analog_outputs.insert(cmd.channel_id, Some(*v));
            }
            (OnlineForceChannelKind::Ao, None) => {
                set.analog_outputs.insert(cmd.channel_id, None);
            }
            _ => {
                return Err(format!(
                    "online-force value type mismatch at {}{}",
                    cmd.channel_kind.short(),
                    cmd.channel_id
                ));
            }
        }
    }

    for (at_ms, set) in by_at {
        scenario.forces.push(sim::ForceEvent { at_ms, set });
    }
    scenario.forces.sort_by_key(|event| event.at_ms);
    Ok(())
}

fn default_online_force_audit_path(trace_out: &Path) -> PathBuf {
    trace_out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join("online_force_audit.jsonl")
}

fn write_online_force_audit(path: &Path, entries: &[OnlineForceAuditEntry]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "Failed to create online-force audit directory {}: {err}",
                    parent.display()
                )
            })?;
        }
    }

    let file = fs::File::create(path).map_err(|err| {
        format!(
            "Failed to create online-force audit {}: {err}",
            path.display()
        )
    })?;
    let mut writer = std::io::BufWriter::new(file);
    for entry in entries {
        let line = serde_json::to_string(entry)
            .map_err(|err| format!("Failed to serialize online-force audit entry: {err}"))?;
        writer.write_all(line.as_bytes()).map_err(|err| {
            format!(
                "Failed to write online-force audit {}: {err}",
                path.display()
            )
        })?;
        writer.write_all(b"\n").map_err(|err| {
            format!(
                "Failed to write online-force audit {}: {err}",
                path.display()
            )
        })?;
    }
    writer.flush().map_err(|err| {
        format!(
            "Failed to flush online-force audit {}: {err}",
            path.display()
        )
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OnlineVariableKind {
    Bool,
    Real,
}

impl OnlineVariableKind {
    fn label(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Real => "real",
        }
    }
}

#[derive(Debug, Clone)]
enum OnlineVariableValue {
    Bool(bool),
    Real(f32),
}

#[derive(Debug, Clone)]
struct OnlineVariableCommand {
    at_ms: u64,
    actor: String,
    source: String,
    variable_kind: OnlineVariableKind,
    variable_name: String,
    variable_key: String,
    value: Option<OnlineVariableValue>,
}

#[derive(Debug, Deserialize)]
struct OnlineVariableScriptEntryRaw {
    at_ms: u64,
    actor: String,
    source: String,
    variable: String,
    #[serde(default)]
    value: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum OnlineVariableAuditValue {
    Bool(bool),
    Real(f32),
}

#[derive(Debug, Serialize)]
struct OnlineVariableAuditEntry {
    at_ms: u64,
    tick: u64,
    actor: String,
    source: String,
    variable: String,
    variable_kind: &'static str,
    bound_channel: Option<String>,
    operation: &'static str,
    from: Option<OnlineVariableAuditValue>,
    to: Option<OnlineVariableAuditValue>,
}

#[derive(Debug, Clone, Default)]
struct OnlineVariableBindings {
    bool_to_di: BTreeMap<String, u16>,
    real_to_ai: BTreeMap<String, u16>,
}

#[derive(Debug, Deserialize)]
struct OnlineVariableBindingsFileRaw {
    #[serde(default = "online_var_binding_schema_version")]
    schema_version: u32,
    #[serde(default)]
    bool: BTreeMap<String, toml::Value>,
    #[serde(default)]
    real: BTreeMap<String, toml::Value>,
}

fn online_var_binding_schema_version() -> u32 {
    1
}

fn normalize_online_variable_name(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

fn parse_online_variable_binding_channel(
    raw: &toml::Value,
    kind: OnlineVariableKind,
    var_name: &str,
) -> Result<u16, String> {
    let prefixes = match kind {
        OnlineVariableKind::Bool => ["di", "x"].as_slice(),
        OnlineVariableKind::Real => ["ai"].as_slice(),
    };
    match raw {
        toml::Value::Integer(v) => {
            if *v < 0 || *v > u16::MAX as i64 {
                return Err(format!(
                    "invalid {} binding for `{}`: integer id out of range for u16",
                    kind.label(),
                    var_name
                ));
            }
            Ok(*v as u16)
        }
        toml::Value::String(s) => parse_retain_channel_id(s, prefixes)
            .map_err(|err| format!("invalid {} binding for `{}`: {err}", kind.label(), var_name)),
        _ => Err(format!(
            "invalid {} binding for `{}`: expected integer id or channel string",
            kind.label(),
            var_name
        )),
    }
}

fn load_online_variable_bindings(path: &Path) -> Result<OnlineVariableBindings, String> {
    let body = fs::read_to_string(path).map_err(|err| {
        format!(
            "Failed to read online-variable bindings {}: {err}",
            path.display()
        )
    })?;
    let raw: OnlineVariableBindingsFileRaw = toml::from_str(&body).map_err(|err| {
        format!(
            "Failed to parse online-variable bindings {}: {err}",
            path.display()
        )
    })?;
    if raw.schema_version != online_var_binding_schema_version() {
        return Err(format!(
            "online-variable bindings schema_version={} is unsupported (expected {})",
            raw.schema_version,
            online_var_binding_schema_version()
        ));
    }

    let mut out = OnlineVariableBindings::default();
    for (name, channel) in &raw.bool {
        let key = normalize_online_variable_name(name);
        if key.is_empty() {
            return Err("online-variable bool binding name cannot be empty".to_string());
        }
        let id = parse_online_variable_binding_channel(channel, OnlineVariableKind::Bool, name)?;
        if out.bool_to_di.insert(key.clone(), id).is_some() {
            return Err(format!(
                "duplicate BOOL binding for `{name}` after normalization"
            ));
        }
    }
    for (name, channel) in &raw.real {
        let key = normalize_online_variable_name(name);
        if key.is_empty() {
            return Err("online-variable real binding name cannot be empty".to_string());
        }
        let id = parse_online_variable_binding_channel(channel, OnlineVariableKind::Real, name)?;
        if out.real_to_ai.insert(key.clone(), id).is_some() {
            return Err(format!(
                "duplicate REAL binding for `{name}` after normalization"
            ));
        }
    }

    Ok(out)
}

fn parse_online_variable_target(raw: &str) -> Result<(OnlineVariableKind, String), String> {
    let token = raw.trim();
    let Some((kind_raw, name_raw)) = token.split_once(':') else {
        return Err(format!(
            "invalid variable `{raw}` (expected BOOL:<name> or REAL:<name>)"
        ));
    };
    let kind = match kind_raw.trim().to_ascii_lowercase().as_str() {
        "bool" | "boolean" => OnlineVariableKind::Bool,
        "real" | "float" | "f32" => OnlineVariableKind::Real,
        _ => {
            return Err(format!(
                "invalid variable `{raw}` (unknown type prefix `{kind_raw}`; expected BOOL or REAL)"
            ));
        }
    };
    let name = name_raw.trim();
    if name.is_empty() {
        return Err(format!("invalid variable `{raw}` (name cannot be empty)"));
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        return Err(format!(
            "invalid variable `{raw}` (name must contain only [A-Za-z0-9_.-])"
        ));
    }
    Ok((kind, name.to_string()))
}

fn parse_online_variable_value(
    raw: Option<serde_json::Value>,
    kind: OnlineVariableKind,
) -> Result<Option<OnlineVariableValue>, String> {
    let Some(v) = raw else {
        return Ok(None);
    };
    match kind {
        OnlineVariableKind::Bool => match v {
            serde_json::Value::Bool(value) => Ok(Some(OnlineVariableValue::Bool(value))),
            serde_json::Value::Null => Ok(None),
            other => Err(format!(
                "BOOL variable expects bool/null value, got {other}"
            )),
        },
        OnlineVariableKind::Real => match v {
            serde_json::Value::Number(value) => {
                let parsed = value
                    .as_f64()
                    .ok_or_else(|| "REAL variable expects finite numeric/null value".to_string())?;
                if !parsed.is_finite() {
                    return Err("REAL variable expects finite numeric/null value".to_string());
                }
                Ok(Some(OnlineVariableValue::Real(parsed as f32)))
            }
            serde_json::Value::Null => Ok(None),
            other => Err(format!(
                "REAL variable expects numeric/null value, got {other}"
            )),
        },
    }
}

fn load_online_variable_script(
    path: &Path,
    tick_ms: u64,
) -> Result<Vec<OnlineVariableCommand>, String> {
    let body = fs::read_to_string(path).map_err(|err| {
        format!(
            "Failed to read online-variable script {}: {err}",
            path.display()
        )
    })?;
    let mut commands = Vec::<OnlineVariableCommand>::new();
    for (lineno, line) in body.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let raw: OnlineVariableScriptEntryRaw = serde_json::from_str(trimmed)
            .map_err(|err| format!("Invalid JSONL at {}:{}: {err}", path.display(), lineno + 1))?;
        if tick_ms != 0 && raw.at_ms % tick_ms != 0 {
            return Err(format!(
                "at_ms={} is not aligned to tick_ms={} at {}:{}",
                raw.at_ms,
                tick_ms,
                path.display(),
                lineno + 1
            ));
        }
        let (kind, name) = parse_online_variable_target(&raw.variable)
            .map_err(|err| format!("{err} at {}:{}", path.display(), lineno + 1))?;
        let value = parse_online_variable_value(raw.value, kind)
            .map_err(|err| format!("{err} at {}:{}", path.display(), lineno + 1))?;
        commands.push(OnlineVariableCommand {
            at_ms: raw.at_ms,
            actor: raw.actor,
            source: raw.source,
            variable_kind: kind,
            variable_key: normalize_online_variable_name(&name),
            variable_name: name,
            value,
        });
    }
    commands.sort_by(|a, b| a.at_ms.cmp(&b.at_ms));
    Ok(commands)
}

fn parse_auto_online_variable_channel_id(
    kind: OnlineVariableKind,
    variable_key: &str,
) -> Option<u16> {
    let prefixes = match kind {
        OnlineVariableKind::Bool => ["di", "x"].as_slice(),
        OnlineVariableKind::Real => ["ai"].as_slice(),
    };
    parse_retain_channel_id(variable_key, prefixes).ok()
}

fn resolve_online_variable_channel(
    cmd: &OnlineVariableCommand,
    bindings: Option<&OnlineVariableBindings>,
) -> Result<u16, String> {
    let from_bindings = bindings.and_then(|defs| match cmd.variable_kind {
        OnlineVariableKind::Bool => defs.bool_to_di.get(&cmd.variable_key).copied(),
        OnlineVariableKind::Real => defs.real_to_ai.get(&cmd.variable_key).copied(),
    });
    if let Some(id) = from_bindings {
        return Ok(id);
    }
    if let Some(id) = parse_auto_online_variable_channel_id(cmd.variable_kind, &cmd.variable_key) {
        return Ok(id);
    }
    Err(format!(
        "missing {} binding for variable `{}`; add --online-var-bindings <bindings.toml> or use auto-mappable names (BOOL:DI<n>, REAL:AI<n>)",
        cmd.variable_kind.label().to_ascii_uppercase(),
        cmd.variable_name
    ))
}

fn inject_online_variable_commands(
    scenario: &mut sim::Scenario,
    commands: &[OnlineVariableCommand],
    bindings: Option<&OnlineVariableBindings>,
) -> Result<(), String> {
    let mut by_at = BTreeMap::<u64, sim::ForceSet>::new();
    for cmd in commands {
        let id = resolve_online_variable_channel(cmd, bindings)?;
        let set = by_at.entry(cmd.at_ms).or_default();
        match (cmd.variable_kind, cmd.value.as_ref()) {
            (OnlineVariableKind::Bool, Some(OnlineVariableValue::Bool(v))) => {
                set.digital_inputs.insert(id, Some(*v));
            }
            (OnlineVariableKind::Bool, None) => {
                set.digital_inputs.insert(id, None);
            }
            (OnlineVariableKind::Real, Some(OnlineVariableValue::Real(v))) => {
                set.analog_inputs.insert(id, Some(*v));
            }
            (OnlineVariableKind::Real, None) => {
                set.analog_inputs.insert(id, None);
            }
            _ => {
                return Err(format!(
                    "online-variable value type mismatch at {}:{}",
                    cmd.variable_kind.label(),
                    cmd.variable_name
                ));
            }
        }
    }
    for (at_ms, set) in by_at {
        scenario.forces.push(sim::ForceEvent { at_ms, set });
    }
    scenario.forces.sort_by_key(|event| event.at_ms);
    Ok(())
}

fn build_online_variable_audit(
    commands: &[OnlineVariableCommand],
    tick_ms: u64,
    bindings: Option<&OnlineVariableBindings>,
) -> Result<Vec<OnlineVariableAuditEntry>, String> {
    let mut out = Vec::<OnlineVariableAuditEntry>::new();
    let mut bool_values = BTreeMap::<String, bool>::new();
    let mut real_values = BTreeMap::<String, f32>::new();

    for cmd in commands {
        let bound_channel =
            resolve_online_variable_channel(cmd, bindings).map(|id| match cmd.variable_kind {
                OnlineVariableKind::Bool => format!("di{id}"),
                OnlineVariableKind::Real => format!("ai{id}"),
            })?;
        let (from, to) = match cmd.variable_kind {
            OnlineVariableKind::Bool => {
                let before = bool_values
                    .get(&cmd.variable_name)
                    .copied()
                    .map(OnlineVariableAuditValue::Bool);
                match cmd.value.as_ref() {
                    Some(OnlineVariableValue::Bool(v)) => {
                        bool_values.insert(cmd.variable_name.clone(), *v);
                        (before, Some(OnlineVariableAuditValue::Bool(*v)))
                    }
                    None => {
                        bool_values.remove(&cmd.variable_name);
                        (before, None)
                    }
                    Some(OnlineVariableValue::Real(_)) => continue,
                }
            }
            OnlineVariableKind::Real => {
                let before = real_values
                    .get(&cmd.variable_name)
                    .copied()
                    .map(OnlineVariableAuditValue::Real);
                match cmd.value.as_ref() {
                    Some(OnlineVariableValue::Real(v)) => {
                        real_values.insert(cmd.variable_name.clone(), *v);
                        (before, Some(OnlineVariableAuditValue::Real(*v)))
                    }
                    None => {
                        real_values.remove(&cmd.variable_name);
                        (before, None)
                    }
                    Some(OnlineVariableValue::Bool(_)) => continue,
                }
            }
        };

        out.push(OnlineVariableAuditEntry {
            at_ms: cmd.at_ms,
            tick: if tick_ms == 0 { 0 } else { cmd.at_ms / tick_ms },
            actor: cmd.actor.clone(),
            source: cmd.source.clone(),
            variable: format!("{}:{}", cmd.variable_kind.label(), cmd.variable_name),
            variable_kind: cmd.variable_kind.label(),
            bound_channel: Some(bound_channel),
            operation: if cmd.value.is_some() { "set" } else { "clear" },
            from,
            to,
        });
    }

    Ok(out)
}

fn default_online_variable_audit_path(trace_out: &Path) -> PathBuf {
    trace_out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join("online_var_audit.jsonl")
}

fn default_alarm_event_audit_path(trace_out: &Path) -> PathBuf {
    trace_out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join("alarm_events.ndjson")
}

fn capture_io_tick_snapshot(io: &sim::SimIo) -> IoTickSnapshot {
    let mut digital_inputs = Vec::with_capacity(io.num_digital_inputs());
    for idx in 0..io.num_digital_inputs() {
        let Ok(id) = u16::try_from(idx) else {
            break;
        };
        digital_inputs.push(io.read_digital_input(DigitalInputId(id)));
    }

    let mut analog_inputs = Vec::with_capacity(io.num_analog_inputs());
    for idx in 0..io.num_analog_inputs() {
        let Ok(id) = u16::try_from(idx) else {
            break;
        };
        analog_inputs.push(io.read_analog_input(AnalogInputId(id)));
    }

    let mut digital_outputs = Vec::with_capacity(io.num_digital_outputs());
    for idx in 0..io.num_digital_outputs() {
        let Ok(id) = u16::try_from(idx) else {
            break;
        };
        digital_outputs.push(io.read_digital_output_value(DigitalOutputId(id)));
    }

    let mut analog_outputs = Vec::with_capacity(io.num_analog_outputs());
    for idx in 0..io.num_analog_outputs() {
        let Ok(id) = u16::try_from(idx) else {
            break;
        };
        analog_outputs.push(io.read_analog_output_value(AnalogOutputId(id)));
    }

    IoTickSnapshot {
        tick: io.tick().0,
        digital_inputs,
        analog_inputs,
        digital_outputs,
        analog_outputs,
    }
}

fn write_io_snapshot_artifact(
    path: &Path,
    tick_ms: u64,
    ticks: Vec<IoTickSnapshot>,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "Failed to create io-snapshot artifact directory {}: {err}",
                    parent.display()
                )
            })?;
        }
    }

    let mut json = serde_json::to_string_pretty(&IoSnapshotArtifact {
        schema_version: 1,
        tick_ms,
        ticks,
    })
    .map_err(|err| format!("Failed to serialize io-snapshot artifact JSON: {err}"))?;
    json.push('\n');
    fs::write(path, json).map_err(|err| {
        format!(
            "Failed to write io-snapshot artifact {}: {err}",
            path.display()
        )
    })
}

fn default_alarm_scenario_or_recipe_id(scenario_path: &Path) -> String {
    scenario_path
        .file_stem()
        .and_then(OsStr::to_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("scenario")
        .to_string()
}

fn write_online_variable_audit(
    path: &Path,
    entries: &[OnlineVariableAuditEntry],
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "Failed to create online-variable audit directory {}: {err}",
                    parent.display()
                )
            })?;
        }
    }

    let file = fs::File::create(path).map_err(|err| {
        format!(
            "Failed to create online-variable audit {}: {err}",
            path.display()
        )
    })?;
    let mut writer = std::io::BufWriter::new(file);
    for entry in entries {
        let line = serde_json::to_string(entry)
            .map_err(|err| format!("Failed to serialize online-variable audit entry: {err}"))?;
        writer.write_all(line.as_bytes()).map_err(|err| {
            format!(
                "Failed to write online-variable audit {}: {err}",
                path.display()
            )
        })?;
        writer.write_all(b"\n").map_err(|err| {
            format!(
                "Failed to write online-variable audit {}: {err}",
                path.display()
            )
        })?;
    }
    writer.flush().map_err(|err| {
        format!(
            "Failed to flush online-variable audit {}: {err}",
            path.display()
        )
    })
}

#[derive(Debug, Clone)]
struct RetainConfig {
    digital_inputs: BTreeMap<u16, bool>,
    analog_inputs: BTreeMap<u16, f32>,
    digital_outputs: BTreeMap<u16, bool>,
    analog_outputs: BTreeMap<u16, f32>,
}

impl RetainConfig {
    fn is_empty(&self) -> bool {
        self.digital_inputs.is_empty()
            && self.analog_inputs.is_empty()
            && self.digital_outputs.is_empty()
            && self.analog_outputs.is_empty()
    }
}

#[derive(Debug, Deserialize)]
struct RetainConfigFileRaw {
    #[serde(default = "retain_schema_version")]
    schema_version: u32,
    #[serde(default)]
    digital_inputs: BTreeMap<String, bool>,
    #[serde(default)]
    analog_inputs: BTreeMap<String, f32>,
    #[serde(default)]
    digital_outputs: BTreeMap<String, bool>,
    #[serde(default)]
    analog_outputs: BTreeMap<String, f32>,
}

fn retain_schema_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RetainStatePayload {
    schema_version: u32,
    #[serde(default)]
    digital_inputs: BTreeMap<u16, bool>,
    #[serde(default)]
    analog_inputs: BTreeMap<u16, f32>,
    #[serde(default)]
    digital_outputs: BTreeMap<u16, bool>,
    #[serde(default)]
    analog_outputs: BTreeMap<u16, f32>,
}

impl RetainStatePayload {
    fn from_config_defaults(config: &RetainConfig) -> Self {
        Self {
            schema_version: retain_schema_version(),
            digital_inputs: config.digital_inputs.clone(),
            analog_inputs: config.analog_inputs.clone(),
            digital_outputs: config.digital_outputs.clone(),
            analog_outputs: config.analog_outputs.clone(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct RetainStateEnvelope {
    schema_version: u32,
    checksum_sha256: String,
    payload: RetainStatePayload,
}

fn parse_retain_channel_id(raw: &str, prefixes: &[&str]) -> Result<u16, String> {
    let token = raw.trim();
    if token.is_empty() {
        return Err("channel id key cannot be empty".to_string());
    }
    if let Ok(id) = token.parse::<u16>() {
        return Ok(id);
    }
    let lower = token.to_ascii_lowercase();
    for prefix in prefixes {
        if let Some(rest) = lower.strip_prefix(prefix) {
            return rest.parse::<u16>().map_err(|_| {
                format!(
                    "invalid retain channel key `{raw}` (expected <id> or {}<id>)",
                    prefix
                )
            });
        }
    }
    Err(format!(
        "invalid retain channel key `{raw}` (expected prefixes {:?} + integer id)",
        prefixes
    ))
}

fn normalize_retain_bool_map(
    raw: &BTreeMap<String, bool>,
    prefixes: &[&str],
    label: &str,
) -> Result<BTreeMap<u16, bool>, String> {
    let mut out = BTreeMap::<u16, bool>::new();
    for (k, v) in raw {
        let id = parse_retain_channel_id(k, prefixes)
            .map_err(|err| format!("invalid {label} key `{k}`: {err}"))?;
        if out.insert(id, *v).is_some() {
            return Err(format!(
                "duplicate retain {label} id {id} after key normalization"
            ));
        }
    }
    Ok(out)
}

fn normalize_retain_f32_map(
    raw: &BTreeMap<String, f32>,
    prefixes: &[&str],
    label: &str,
) -> Result<BTreeMap<u16, f32>, String> {
    let mut out = BTreeMap::<u16, f32>::new();
    for (k, v) in raw {
        if !v.is_finite() {
            return Err(format!("retain {label}.{k} must be finite"));
        }
        let id = parse_retain_channel_id(k, prefixes)
            .map_err(|err| format!("invalid {label} key `{k}`: {err}"))?;
        if out.insert(id, *v).is_some() {
            return Err(format!(
                "duplicate retain {label} id {id} after key normalization"
            ));
        }
    }
    Ok(out)
}

fn load_retain_config(path: &Path) -> Result<RetainConfig, String> {
    let body = fs::read_to_string(path)
        .map_err(|err| format!("Failed to read retain config {}: {err}", path.display()))?;
    let raw: RetainConfigFileRaw = toml::from_str(&body)
        .map_err(|err| format!("Failed to parse retain config {}: {err}", path.display()))?;
    if raw.schema_version != retain_schema_version() {
        return Err(format!(
            "retain config schema_version={} is unsupported (expected {})",
            raw.schema_version,
            retain_schema_version()
        ));
    }

    Ok(RetainConfig {
        digital_inputs: normalize_retain_bool_map(
            &raw.digital_inputs,
            &["di", "x"],
            "digital_inputs",
        )?,
        analog_inputs: normalize_retain_f32_map(&raw.analog_inputs, &["ai"], "analog_inputs")?,
        digital_outputs: normalize_retain_bool_map(
            &raw.digital_outputs,
            &["do", "y"],
            "digital_outputs",
        )?,
        analog_outputs: normalize_retain_f32_map(&raw.analog_outputs, &["ao"], "analog_outputs")?,
    })
}

fn default_retain_state_path(trace_out: &Path) -> PathBuf {
    trace_out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join("retain_state.json")
}

fn compute_retain_checksum(payload: &RetainStatePayload) -> Result<String, String> {
    let bytes = serde_json::to_vec(payload)
        .map_err(|err| format!("Failed to serialize retain payload for checksum: {err}"))?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

fn load_retain_state(path: &Path, config: &RetainConfig) -> (RetainStatePayload, Option<String>) {
    if !path.exists() {
        return (
            RetainStatePayload::from_config_defaults(config),
            Some(format!(
                "retain state {} does not exist; using config defaults",
                path.display()
            )),
        );
    }
    let body = match fs::read_to_string(path) {
        Ok(v) => v,
        Err(err) => {
            return (
                RetainStatePayload::from_config_defaults(config),
                Some(format!(
                    "failed to read retain state {} ({err}); using config defaults",
                    path.display()
                )),
            );
        }
    };

    let envelope: RetainStateEnvelope = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(err) => {
            return (
                RetainStatePayload::from_config_defaults(config),
                Some(format!(
                    "retain state {} is invalid JSON ({err}); using config defaults",
                    path.display()
                )),
            );
        }
    };
    if envelope.schema_version != retain_schema_version() {
        return (
            RetainStatePayload::from_config_defaults(config),
            Some(format!(
                "retain state {} schema_version={} is unsupported; using config defaults",
                path.display(),
                envelope.schema_version
            )),
        );
    }
    if envelope.payload.schema_version != retain_schema_version() {
        return (
            RetainStatePayload::from_config_defaults(config),
            Some(format!(
                "retain payload schema_version={} is unsupported in {}; using config defaults",
                envelope.payload.schema_version,
                path.display()
            )),
        );
    }
    let checksum = match compute_retain_checksum(&envelope.payload) {
        Ok(v) => v,
        Err(err) => {
            return (
                RetainStatePayload::from_config_defaults(config),
                Some(format!(
                    "failed to verify retain checksum for {} ({err}); using config defaults",
                    path.display()
                )),
            );
        }
    };
    if checksum != envelope.checksum_sha256 {
        return (
            RetainStatePayload::from_config_defaults(config),
            Some(format!(
                "retain checksum mismatch for {}; using config defaults",
                path.display()
            )),
        );
    }

    let mut payload = RetainStatePayload::from_config_defaults(config);
    for id in config.digital_inputs.keys() {
        if let Some(v) = envelope.payload.digital_inputs.get(id) {
            payload.digital_inputs.insert(*id, *v);
        }
    }
    for id in config.analog_inputs.keys() {
        if let Some(v) = envelope.payload.analog_inputs.get(id) {
            payload.analog_inputs.insert(*id, *v);
        }
    }
    for id in config.digital_outputs.keys() {
        if let Some(v) = envelope.payload.digital_outputs.get(id) {
            payload.digital_outputs.insert(*id, *v);
        }
    }
    for id in config.analog_outputs.keys() {
        if let Some(v) = envelope.payload.analog_outputs.get(id) {
            payload.analog_outputs.insert(*id, *v);
        }
    }
    (payload, None)
}

fn write_retain_state(path: &Path, payload: &RetainStatePayload) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "Failed to create retain state directory {}: {err}",
                    parent.display()
                )
            })?;
        }
    }
    let envelope = RetainStateEnvelope {
        schema_version: retain_schema_version(),
        checksum_sha256: compute_retain_checksum(payload)?,
        payload: payload.clone(),
    };
    let mut json = serde_json::to_string_pretty(&envelope)
        .map_err(|err| format!("Failed to serialize retain state JSON: {err}"))?;
    json.push('\n');
    fs::write(path, json)
        .map_err(|err| format!("Failed to write retain state {}: {err}", path.display()))
}

fn apply_retain_payload_to_scenario(scenario: &mut sim::Scenario, payload: &RetainStatePayload) {
    if !payload.digital_inputs.is_empty() || !payload.analog_inputs.is_empty() {
        let mut set = sim::InputSet::default();
        for (id, value) in &payload.digital_inputs {
            set.digital_inputs.insert(*id, *value);
        }
        for (id, value) in &payload.analog_inputs {
            set.analog_inputs.insert(*id, *value);
        }
        // Place retain bootstrap first so explicit scenario scripting at the same tick can override it.
        scenario.inputs.insert(0, sim::InputEvent { at_ms: 0, set });
        scenario.inputs.sort_by_key(|event| event.at_ms);
    }

    if !payload.digital_outputs.is_empty() || !payload.analog_outputs.is_empty() {
        let mut set = sim::ForceSet::default();
        for (id, value) in &payload.digital_outputs {
            set.digital_outputs.insert(*id, Some(*value));
        }
        for (id, value) in &payload.analog_outputs {
            set.analog_outputs.insert(*id, Some(*value));
        }
        scenario.forces.insert(0, sim::ForceEvent { at_ms: 0, set });

        // Outputs use a one-tick bootstrap force so runtime writes can take over afterwards.
        if scenario.tick_ms > 0
            && (scenario.duration_ms == 0 || scenario.tick_ms < scenario.duration_ms)
        {
            let mut clear = sim::ForceSet::default();
            for id in payload.digital_outputs.keys() {
                clear.digital_outputs.insert(*id, None);
            }
            for id in payload.analog_outputs.keys() {
                clear.analog_outputs.insert(*id, None);
            }
            scenario.forces.push(sim::ForceEvent {
                at_ms: scenario.tick_ms,
                set: clear,
            });
        }

        scenario.forces.sort_by_key(|event| event.at_ms);
    }
}

fn capture_retain_payload(config: &RetainConfig, io: &sim::SimIo) -> RetainStatePayload {
    let mut payload = RetainStatePayload::from_config_defaults(config);
    for id in config.digital_inputs.keys() {
        payload
            .digital_inputs
            .insert(*id, io.read_digital_input(io_traits::DigitalInputId(*id)));
    }
    for id in config.analog_inputs.keys() {
        payload
            .analog_inputs
            .insert(*id, io.read_analog_input(io_traits::AnalogInputId(*id)));
    }
    for id in config.digital_outputs.keys() {
        let value = io
            .digital_output_edges()
            .iter()
            .rev()
            .find(|edge| edge.id.0 == *id)
            .map(|edge| edge.value)
            .unwrap_or(false);
        payload.digital_outputs.insert(*id, value);
    }
    for id in config.analog_outputs.keys() {
        let value = io
            .analog_output_edges()
            .iter()
            .rev()
            .find(|edge| edge.id.0 == *id)
            .map(|edge| edge.value)
            .unwrap_or(0.0);
        payload.analog_outputs.insert(*id, value);
    }
    payload
}

fn run_sim_subcommand(program: &str, mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let usage = command_usage(program, "sim");
    let Some(scenario_path) = args.next() else {
        return Err(usage);
    };

    let mut out_path: Option<String> = None;
    let mut vcd_out_path: Option<String> = None;
    let mut analog_out_path: Option<String> = None;
    let mut report_out_path: Option<String> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => {
                out_path = Some(
                    args.next()
                        .ok_or_else(|| "Missing value for --out <trace.jsonl>".to_string())?,
                );
            }
            "--vcd-out" => {
                vcd_out_path = Some(
                    args.next()
                        .ok_or_else(|| "Missing value for --vcd-out <wave.vcd>".to_string())?,
                );
            }
            "--analog-out" => {
                analog_out_path =
                    Some(args.next().ok_or_else(|| {
                        "Missing value for --analog-out <analog.csv>".to_string()
                    })?);
            }
            "--report-out" => {
                report_out_path =
                    Some(args.next().ok_or_else(|| {
                        "Missing value for --report-out <report.json>".to_string()
                    })?);
            }
            "-h" | "--help" => {
                return Err(usage.clone());
            }
            other => {
                return Err(format!("Unknown argument for sim: {other}"));
            }
        }
    }

    let scenario_path = PathBuf::from(&scenario_path);
    let scenario_yaml = read_scenario_yaml_file(&scenario_path)?;
    let scenario = parse_scenario_yaml(&scenario_yaml)?;

    let out_path = out_path.map(PathBuf::from);
    let base_dir = out_path
        .as_deref()
        .and_then(|p| p.parent())
        .filter(|p| !p.as_os_str().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if out_path.is_some() {
                PathBuf::from(".")
            } else {
                PathBuf::from("out")
            }
        });

    let out_path = out_path.unwrap_or_else(|| base_dir.join("trace.jsonl"));
    let vcd_out_path = vcd_out_path
        .map(PathBuf::from)
        .unwrap_or_else(|| base_dir.join("wave.vcd"));
    let analog_out_path = analog_out_path
        .map(PathBuf::from)
        .unwrap_or_else(|| base_dir.join("analog.csv"));
    let report_out_path = report_out_path
        .map(PathBuf::from)
        .unwrap_or_else(|| base_dir.join("report.json"));

    for p in [&out_path, &vcd_out_path, &analog_out_path, &report_out_path] {
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|err| {
                    format!("Failed to create output directory {parent:?}: {err}")
                })?;
            }
        }
    }

    let mut io = sim::SimIo::new(1, 1, 0, 0);
    let run = sim::run_program_for_scenario(&SIM_PROGRAM, &scenario, &mut io)
        .map_err(|err| format!("Simulation failed: {err}"))?;

    fs::write(&out_path, run.trace.into_string())
        .map_err(|err| format!("Failed to write trace file {out_path:?}: {err}"))?;

    let vcd = sim::export_vcd_digital(&io, scenario.tick_ms);
    fs::write(&vcd_out_path, vcd)
        .map_err(|err| format!("Failed to write VCD file {vcd_out_path:?}: {err}"))?;

    let analog_csv = sim::export_analog_outputs_csv(&io, scenario.tick_ms);
    fs::write(&analog_out_path, analog_csv)
        .map_err(|err| format!("Failed to write analog CSV file {analog_out_path:?}: {err}"))?;

    let mut report_json = serde_json::to_string_pretty(&run.report)
        .map_err(|err| format!("Failed to serialize report JSON: {err}"))?;
    report_json.push('\n');
    fs::write(&report_out_path, report_json)
        .map_err(|err| format!("Failed to write report file {report_out_path:?}: {err}"))?;

    Ok(())
}

fn run_sim_plc_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "sim-plc");
    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut scenario_path: Option<PathBuf> = None;
    let mut out_path: Option<PathBuf> = None;
    let mut retain_config_path: Option<PathBuf> = None;
    let mut retain_state_path: Option<PathBuf> = None;
    let mut enable_online_force_dev = false;
    let mut online_force_script: Option<PathBuf> = None;
    let mut online_force_audit_out: Option<PathBuf> = None;
    let mut online_var_script: Option<PathBuf> = None;
    let mut online_var_bindings_path: Option<PathBuf> = None;
    let mut online_var_audit_out: Option<PathBuf> = None;
    let mut alarm_options_seen = false;
    let mut alarm_audit_out: Option<PathBuf> = None;
    let mut alarm_hmi_ws: Option<String> = None;
    let mut alarm_scenario_id: Option<String> = None;
    let mut alarm_top_n: usize = 3;
    let mut alarm_dedup_window_ms: u64 = 1_000;
    let mut alarm_min_interval_ms: u64 = 200;
    let mut io_snapshot_out: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenario" => {
                scenario_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --scenario <scenario.yaml>".to_string()
                    })?));
            }
            "--out" => {
                out_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --out <trace.jsonl>".to_string()
                    })?));
            }
            "--retain-config" => {
                retain_config_path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --retain-config <retain.toml>".to_string()
                })?));
            }
            "--retain-state" => {
                retain_state_path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --retain-state <retain_state.json>".to_string()
                })?));
            }
            "--enable-online-force-dev" => {
                enable_online_force_dev = true;
            }
            "--online-force-script" => {
                online_force_script = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --online-force-script <script.jsonl>".to_string()
                })?));
            }
            "--online-force-audit-out" => {
                online_force_audit_out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --online-force-audit-out <audit.jsonl>".to_string()
                })?));
            }
            "--online-var-script" => {
                online_var_script = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --online-var-script <script.jsonl>".to_string()
                })?));
            }
            "--online-var-bindings" => {
                online_var_bindings_path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --online-var-bindings <bindings.toml>".to_string()
                })?));
            }
            "--online-var-audit-out" => {
                online_var_audit_out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --online-var-audit-out <audit.jsonl>".to_string()
                })?));
            }
            "--alarm-audit-out" => {
                alarm_options_seen = true;
                alarm_audit_out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --alarm-audit-out <alarm_events.ndjson>".to_string()
                })?));
            }
            "--alarm-hmi-ws" => {
                alarm_options_seen = true;
                alarm_hmi_ws = Some(args.next().ok_or_else(|| {
                    "Missing value for --alarm-hmi-ws <ws://host:port/path>".to_string()
                })?);
            }
            "--alarm-scenario-id" => {
                alarm_options_seen = true;
                let value = args
                    .next()
                    .ok_or_else(|| "Missing value for --alarm-scenario-id <id>".to_string())?;
                if value.trim().is_empty() {
                    return Err("--alarm-scenario-id cannot be empty".to_string());
                }
                alarm_scenario_id = Some(value);
            }
            "--alarm-top" => {
                alarm_options_seen = true;
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --alarm-top <n>".to_string())?;
                alarm_top_n = raw
                    .parse::<usize>()
                    .map_err(|_| format!("Invalid --alarm-top value (expected usize): {raw}"))?;
                if alarm_top_n == 0 {
                    return Err("Invalid --alarm-top value (expected >= 1)".to_string());
                }
            }
            "--alarm-dedup-window-ms" => {
                alarm_options_seen = true;
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --alarm-dedup-window-ms <ms>".to_string())?;
                alarm_dedup_window_ms = raw.parse::<u64>().map_err(|_| {
                    format!("Invalid --alarm-dedup-window-ms value (expected u64): {raw}")
                })?;
            }
            "--alarm-min-interval-ms" => {
                alarm_options_seen = true;
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --alarm-min-interval-ms <ms>".to_string())?;
                alarm_min_interval_ms = raw.parse::<u64>().map_err(|_| {
                    format!("Invalid --alarm-min-interval-ms value (expected u64): {raw}")
                })?;
            }
            "--io-snapshot-out" => {
                io_snapshot_out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --io-snapshot-out <io_snapshot.json>".to_string()
                })?));
            }
            "-h" | "--help" => {
                return Err(usage.clone());
            }
            other => return Err(format!("Unknown argument for sim-plc: {other}")),
        }
    }

    let scenario_path = scenario_path.ok_or_else(|| usage.clone())?;
    let out_path = out_path.ok_or_else(|| usage.clone())?;

    if retain_state_path.is_some() && retain_config_path.is_none() {
        return Err("--retain-state requires --retain-config".to_string());
    }
    if (online_force_script.is_some()
        || online_force_audit_out.is_some()
        || online_var_script.is_some()
        || online_var_bindings_path.is_some()
        || online_var_audit_out.is_some())
        && !enable_online_force_dev
    {
        return Err(
            "online-force/variable dev control plane is disabled by default; add --enable-online-force-dev to use --online-force-script/--online-force-audit-out/--online-var-script/--online-var-bindings/--online-var-audit-out"
                .to_string(),
        );
    }

    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to create output directory {parent:?}: {err}"))?;
        }
    }
    if let Some(path) = &io_snapshot_out {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|err| {
                    format!("Failed to create io-snapshot output directory {parent:?}: {err}")
                })?;
            }
        }
    }

    let loaded = load_plc_source(Path::new(&plc_path))?;
    let scenario_yaml = read_scenario_yaml_file(&scenario_path)?;
    let scenario_yaml =
        resolve_scenario_yaml_for_plc(&loaded.source, &scenario_yaml).map_err(|e| {
            format_resolve_scenario_yaml_error(&plc_path, &scenario_path, "sim-plc", &e)
        })?;
    let mut scenario = parse_scenario_yaml(&scenario_yaml)?;

    let mut retain_session: Option<(RetainConfig, PathBuf)> = None;
    if let Some(config_path) = retain_config_path {
        let config = load_retain_config(&config_path)?;
        if config.is_empty() {
            return Err(format!(
                "retain config {} has no retained channels configured",
                config_path.display()
            ));
        }
        let state_path = retain_state_path
            .clone()
            .unwrap_or_else(|| default_retain_state_path(&out_path));
        let (payload, warning) = load_retain_state(&state_path, &config);
        if let Some(msg) = warning {
            eprintln!("[RET-201] {msg}");
        }
        apply_retain_payload_to_scenario(&mut scenario, &payload);
        retain_session = Some((config, state_path));
    }

    let audit_path = if enable_online_force_dev {
        Some(
            online_force_audit_out
                .clone()
                .unwrap_or_else(|| default_online_force_audit_path(&out_path)),
        )
    } else {
        None
    };
    let variable_audit_path = if enable_online_force_dev
        && (online_var_script.is_some() || online_var_audit_out.is_some())
    {
        Some(
            online_var_audit_out
                .clone()
                .unwrap_or_else(|| default_online_variable_audit_path(&out_path)),
        )
    } else {
        None
    };
    let alarm_audit_path = if alarm_options_seen {
        Some(
            alarm_audit_out
                .clone()
                .unwrap_or_else(|| default_alarm_event_audit_path(&out_path)),
        )
    } else {
        None
    };
    let alarm_scenario_or_recipe_id = if alarm_options_seen {
        alarm_scenario_id
            .clone()
            .unwrap_or_else(|| default_alarm_scenario_or_recipe_id(&scenario_path))
    } else {
        String::new()
    };
    let alarm_hmi_ws_display = alarm_hmi_ws.clone();
    let alarm_dispatcher = if let Some(path) = &alarm_audit_path {
        Some(
            AlarmDispatcher::new(AlarmDispatchConfig {
                audit_path: path.clone(),
                websocket_url: alarm_hmi_ws.clone(),
                dedup_window_ms: alarm_dedup_window_ms,
                min_emit_interval_ms: alarm_min_interval_ms,
                queue_capacity: 64,
            })
            .map_err(|err| format!("Failed to initialize alarm dispatcher: {err}"))?,
        )
    } else {
        None
    };

    let mut online_commands = Vec::new();
    if let Some(script_path) = &online_force_script {
        online_commands = load_online_force_script(script_path, scenario.tick_ms)?;
        inject_online_force_commands(&mut scenario, &online_commands)?;
    }

    if let Some(path) = &audit_path {
        let audit_entries = build_online_force_audit(&online_commands, scenario.tick_ms);
        write_online_force_audit(path, &audit_entries)?;
    }
    let mut online_variable_commands = Vec::new();
    let online_variable_bindings = if let Some(path) = &online_var_bindings_path {
        Some(load_online_variable_bindings(path)?)
    } else {
        None
    };
    if let Some(script_path) = &online_var_script {
        online_variable_commands = load_online_variable_script(script_path, scenario.tick_ms)?;
        inject_online_variable_commands(
            &mut scenario,
            &online_variable_commands,
            online_variable_bindings.as_ref(),
        )?;
    }
    if let Some(path) = &variable_audit_path {
        let variable_audit = build_online_variable_audit(
            &online_variable_commands,
            scenario.tick_ms,
            online_variable_bindings.as_ref(),
        )?;
        write_online_variable_audit(path, &variable_audit)?;
    }

    let program = compile_plc_to_runtime_program(&loaded.source, scenario.tick_ms)?;

    let (num_di, num_do, num_ai, num_ao) = io_sizes_for_program_and_scenario(&program, &scenario);
    let mut io = sim::SimIo::new(num_di, num_do, num_ai, num_ao);
    let mut io_snapshots = Vec::new();
    let run = if io_snapshot_out.is_some() {
        sim::run_program_for_scenario_with_tick_observer(&program, &scenario, &mut io, |io| {
            io_snapshots.push(capture_io_tick_snapshot(io));
        })
    } else {
        sim::run_program_for_scenario(&program, &scenario, &mut io)
    }
    .map_err(|e| {
        let mut msg = format!("{e}");
        if let Some(hint) =
            scenario_mismatch_hint_for_example(&plc_path, &scenario_path, &e, "sim-plc")
        {
            msg.push_str("\n\n");
            msg.push_str(&hint);
        }
        msg
    })?;
    if let Some(path) = &io_snapshot_out {
        write_io_snapshot_artifact(path, scenario.tick_ms, io_snapshots)?;
    }
    let trace_text = run.trace.into_string();
    fs::write(&out_path, &trace_text)
        .map_err(|err| format!("Failed to write trace file {out_path:?}: {err}"))?;
    if let Some(dispatcher) = alarm_dispatcher {
        let trace_events = rust_plc::trace_diff::parse_trace_jsonl(&trace_text)
            .map_err(|err| format!("Failed to parse generated trace for alarm events: {err}"))?;
        let timeout_events = trace_events
            .iter()
            .filter(|event| event.reason == "timeout")
            .collect::<Vec<_>>();
        if !timeout_events.is_empty() {
            let diagnosis = diagnose(DiagnosisInput {
                plc_source: &loaded.source,
                scenario: &scenario,
                trace_events: Some(trace_events.as_slice()),
                diff_report: None,
                timing_report: None,
                evidence_source: EvidenceSource::RuntimeLive,
                io_snapshot: None,
            })
            .map_err(|err| format!("Failed to build runtime alarm diagnosis: {err}"))?;
            let evidence_ref = display_path_relative_to_cwd(&out_path);
            for timeout in timeout_events {
                let alarm_event = build_alarm_event(AlarmBuildInput {
                    diagnosis: &diagnosis,
                    severity: AlarmSeverity::Critical,
                    first_seen_ms: timeout.tick.saturating_mul(scenario.tick_ms),
                    top_n: alarm_top_n,
                    evidence_ref: &evidence_ref,
                    evidence_source: EvidenceSource::RuntimeLive,
                    scenario_or_recipe_id: &alarm_scenario_or_recipe_id,
                });
                let _ = dispatcher.publish(alarm_event).map_err(|err| {
                    format!("Failed to enqueue runtime alarm event for publishing: {err}")
                })?;
            }
        }
        dispatcher
            .close()
            .map_err(|err| format!("Failed to finalize runtime alarm dispatcher: {err}"))?;
    }
    if let Some((config, state_path)) = retain_session {
        let payload = capture_retain_payload(&config, &io);
        write_retain_state(&state_path, &payload)?;
        eprintln!("sim-plc: retain state {}", state_path.display());
    }
    if let Some(path) = audit_path {
        eprintln!("sim-plc: online-force audit {}", path.display());
    }
    if let Some(path) = variable_audit_path {
        eprintln!("sim-plc: online-variable audit {}", path.display());
    }
    if let Some(path) = alarm_audit_path {
        eprintln!("sim-plc: alarm-event audit {}", path.display());
    }
    if let Some(ws_url) = alarm_hmi_ws_display {
        eprintln!("sim-plc: alarm-event realtime {}", ws_url);
    }
    if let Some(path) = io_snapshot_out {
        eprintln!("sim-plc: io-snapshot {}", path.display());
    }
    Ok(())
}

fn run_sim_regress_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "sim-regress");
    let mut plc_dir: Option<PathBuf> = None;
    let mut scenario_dir: Option<PathBuf> = None;
    let mut artifacts_dir: Option<PathBuf> = None;
    let mut summary_out: Option<PathBuf> = None;
    let mut minimize_failure = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--plc-dir" => {
                plc_dir =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --plc-dir <dir>".to_string()
                    })?));
            }
            "--scenario-dir" => {
                scenario_dir =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --scenario-dir <dir>".to_string()
                    })?));
            }
            "--artifacts-dir" => {
                artifacts_dir =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --artifacts-dir <dir>".to_string()
                    })?));
            }
            "--summary-out" => {
                summary_out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --summary-out <summary.json>".to_string()
                })?));
            }
            "--minimize-failure" => {
                minimize_failure = true;
            }
            "-h" | "--help" => {
                return Err(usage.clone());
            }
            other => {
                return Err(format!("Unknown argument for sim-regress: {other}"));
            }
        }
    }

    let plc_dir = plc_dir.ok_or_else(|| usage.clone())?;
    let scenario_dir = scenario_dir.ok_or_else(|| usage.clone())?;

    let artifacts_dir = artifacts_dir.unwrap_or_else(|| PathBuf::from("out/sim-regress"));
    let summary_out = summary_out.unwrap_or_else(|| artifacts_dir.join("summary.json"));

    if let Some(parent) = summary_out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to create output directory {parent:?}: {err}"))?;
        }
    }

    let summary = run_sim_regress_with_options(
        &plc_dir,
        &scenario_dir,
        &artifacts_dir,
        SimRegressOptions {
            minimize: minimize_failure,
        },
    )
    .map_err(|e| format!("sim-regress failed: {e}"))?;
    write_sim_regress_summary(&summary_out, &summary)?;
    if minimize_failure {
        let feedback_path = artifacts_dir.join("feedback.json");
        write_sim_regress_feedback(&feedback_path, &summary)?;
    }
    Ok(())
}

fn run_sim_pid_kpi_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "sim-pid-kpi");
    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut scenario_path: Option<PathBuf> = None;
    let mut out_path: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenario" => {
                scenario_path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --scenario <pid_scenario.yaml>".to_string()
                })?));
            }
            "--out" => {
                out_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --out <kpi.json>".to_string()
                    })?));
            }
            "-h" | "--help" => {
                return Err(usage.clone());
            }
            other => return Err(format!("Unknown argument for sim-pid-kpi: {other}")),
        }
    }

    let scenario_path = scenario_path.ok_or_else(|| usage.clone())?;
    let out_path = out_path.unwrap_or_else(|| PathBuf::from("out/pid_kpi.json"));

    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to create output directory {parent:?}: {err}"))?;
        }
    }

    let loaded = load_plc_source(Path::new(&plc_path))?;
    let pid_example = "Example:\n\
tick_ms: 100\n\
duration_ms: 10000\n\
loop_index: 0\n\
initial_pv: 0.0\n\
model:\n\
  kind: first_order\n\
  gain: 1.0\n\
  tau_ms: 500\n";
    let scenario_yaml = fs::read_to_string(&scenario_path).map_err(|err| {
        format!(
            "Failed to read PID scenario YAML {}: {err}\n\n{pid_example}",
            scenario_path.display()
        )
    })?;
    let scenario = sim::PidControlScenario::from_yaml_str(&scenario_yaml)
        .map_err(|err| format!("Failed to parse PID scenario YAML: {err}\n\n{pid_example}"))?;
    let runtime_program = compile_plc_to_runtime_program(&loaded.source, scenario.tick_ms)?;
    let report = sim::run_pid_kpi(&runtime_program, &scenario)
        .map_err(|err| format!("Failed to run PID KPI simulation: {err}"))?;

    let mut json = serde_json::to_string_pretty(&report)
        .map_err(|err| format!("Failed to serialize KPI JSON: {err}"))?;
    json.push('\n');
    fs::write(&out_path, json)
        .map_err(|err| format!("Failed to write KPI file {out_path:?}: {err}"))?;

    Ok(())
}

fn write_sim_regress_summary(path: &Path, summary: &SimRegressSummary) -> Result<(), String> {
    let mut json = serde_json::to_string_pretty(summary)
        .map_err(|err| format!("Failed to serialize summary JSON: {err}"))?;
    json.push('\n');
    fs::write(path, json).map_err(|err| format!("Failed to write summary file {path:?}: {err}"))?;
    Ok(())
}

#[derive(Debug, Serialize)]
struct SimRegressFeedbackFile {
    schema_version: u32,
    total_failures: usize,
    feedback: Vec<SimRegressFeedbackEntry>,
}

#[derive(Debug, Serialize)]
struct SimRegressFeedbackEntry {
    plc: String,
    scenario: String,
    failure_kind: String,
    template_hint: String,
    parameter_hints: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    minimized_scenario_path: Option<String>,
}

fn feedback_template_hint_for_failure_kind(kind: &str) -> &'static str {
    match kind {
        "timeout" => "fault_sensor_stuck",
        "compile_error" | "scenario_error" => "nominal_cycle",
        _ => "risk_gate_probe",
    }
}

fn feedback_parameter_hints_for_failure(
    failure: &rust_plc::sim_regress::SimRegressFailure,
) -> Vec<String> {
    let mut hints = Vec::<String>::new();
    match failure.failure.kind.as_str() {
        "timeout" => {
            hints.push("increase duration_ms to keep timeout windows observable".to_string());
            hints.push(
                "tune start_pulse_ms to align start signal release with task waits".to_string(),
            );
            hints.push("adjust sensor_window_ms to control sensor-edge spacing".to_string());
        }
        "scenario_error" => {
            hints.push(
                "run scenario-validate and fix mapping/tick alignment issues first".to_string(),
            );
        }
        "compile_error" => {
            hints
                .push("fix PLC semantic/verification errors before scenario expansion".to_string());
        }
        _ => {
            hints.push(
                "re-run with --minimize-failure and inspect minimized_scenario.yaml".to_string(),
            );
        }
    }
    if let Some(mini) = &failure.minimization {
        hints.push(format!(
            "duration_ms near {} reproduces this failure signature with lower noise",
            mini.minimized_duration_ms
        ));
    }
    hints
}

fn write_sim_regress_feedback(path: &Path, summary: &SimRegressSummary) -> Result<(), String> {
    let feedback = summary
        .failures
        .iter()
        .map(|failure| SimRegressFeedbackEntry {
            plc: failure.plc.clone(),
            scenario: failure.scenario.clone(),
            failure_kind: failure.failure.kind.clone(),
            template_hint: feedback_template_hint_for_failure_kind(&failure.failure.kind)
                .to_string(),
            parameter_hints: feedback_parameter_hints_for_failure(failure),
            minimized_scenario_path: failure.minimized_scenario_path.clone(),
        })
        .collect::<Vec<_>>();
    let file = SimRegressFeedbackFile {
        schema_version: 1,
        total_failures: summary.failures.len(),
        feedback,
    };
    let mut json = serde_json::to_string_pretty(&file)
        .map_err(|err| format!("Failed to serialize feedback JSON: {err}"))?;
    json.push('\n');
    fs::write(path, json).map_err(|err| format!("Failed to write feedback file {path:?}: {err}"))
}
