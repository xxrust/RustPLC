use crate::cli_support::common::{CliOutputMode, DispatchResult, display_path_relative_to_cwd};
use crate::cli_support::help::command_usage;
use crate::cli_support::plc_pipeline::{
    compile_loaded_codegen_semantics, parse_loaded_plc_with_required_purpose,
};
use rust_plc::ast::VariableType as AstVariableType;
use rust_plc::codegen::st::{StCodegenConfig, generate_st};
use rust_plc::ir::{
    BinaryValue, State, StateMachine, TopologyGraph, TransitionAction, TransitionGuard,
    VariableType,
};
use rust_plc::source_bundle::{is_supported_plc_source_path, load_plc_source};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn try_dispatch(
    program: &str,
    command: &str,
    remaining: &[String],
) -> Option<DispatchResult> {
    let result = match command {
        "gen-keyence" => DispatchResult {
            error_prefix: Some("[KVGEN-000]"),
            result: run_gen_keyence_subcommand(program, remaining.iter().cloned()),
        },
        _ => return None,
    };
    Some(result)
}

#[derive(Debug, Serialize)]
struct KeyenceExportReport {
    schema_version: u32,
    command: &'static str,
    output: &'static str,
    source_plc: String,
    out_dir: String,
    cpu_model: String,
    program_name: String,
    mnm_path: String,
    st_reference_path: String,
    variable_manifest_path: String,
    fb_manifest_path: String,
    validation_report_path: String,
    variable_count: usize,
    device_count: usize,
    status: &'static str,
    codegen_blockers: Vec<String>,
}

#[derive(Debug, Clone)]
struct KeyenceBoolDeviceMap {
    by_name: BTreeMap<String, String>,
}

impl KeyenceBoolDeviceMap {
    fn device_for(&self, name: &str) -> Option<&str> {
        self.by_name.get(name).map(String::as_str)
    }
}

#[derive(Debug, Clone)]
struct KeyenceExecutableMnm {
    text: String,
    variable_devices: KeyenceBoolDeviceMap,
}

#[derive(Debug, Clone, Copy)]
struct KeyenceRelayAddress {
    channel: usize,
    bit: usize,
}

impl KeyenceRelayAddress {
    fn new(channel: usize, bit: usize) -> Self {
        let total_bits = channel
            .checked_mul(16)
            .and_then(|base| base.checked_add(bit))
            .expect("KEYENCE relay allocation overflowed usize");
        Self::from_bit_index(total_bits)
    }

    fn from_bit_index(bit_index: usize) -> Self {
        Self {
            channel: bit_index / 16,
            bit: bit_index % 16,
        }
    }

    fn offset(self, bit_offset: usize) -> Self {
        let total_bits = self
            .channel
            .checked_mul(16)
            .and_then(|base| base.checked_add(self.bit))
            .and_then(|base| base.checked_add(bit_offset))
            .expect("KEYENCE relay allocation overflowed usize");
        Self::from_bit_index(total_bits)
    }
}

impl std::fmt::Display for KeyenceRelayAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "R{}{:02}", self.channel, self.bit)
    }
}

fn run_gen_keyence_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "gen-keyence");
    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut out_dir: Option<PathBuf> = None;
    let mut program_name = "Main".to_string();
    let mut cpu_model = "KV-X550".to_string();
    let mut output_mode = CliOutputMode::Human;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out-dir" => {
                let value = args.next().ok_or_else(|| {
                    "Missing value for --out-dir <dir> in gen-keyence subcommand".to_string()
                })?;
                out_dir = Some(PathBuf::from(value));
            }
            "--program-name" => {
                program_name = args
                    .next()
                    .ok_or_else(|| "Missing value for --program-name <Main>".to_string())?;
                if program_name.trim().is_empty() {
                    return Err("--program-name cannot be empty".to_string());
                }
            }
            "--cpu" => {
                cpu_model = args
                    .next()
                    .ok_or_else(|| "Missing value for --cpu <KV-X550>".to_string())?;
                if cpu_model.trim().is_empty() {
                    return Err("--cpu cannot be empty".to_string());
                }
            }
            "--output" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --output <human|json>".to_string())?;
                output_mode = CliOutputMode::parse(&raw).ok_or_else(|| {
                    format!("Invalid value for --output `{raw}` (expected human or json)")
                })?;
            }
            "-h" | "--help" => return Err(usage.clone()),
            other => {
                return Err(format!(
                    "Unknown argument for gen-keyence: {other}\n{usage}"
                ));
            }
        }
    }

    if !is_supported_plc_source_path(Path::new(&plc_path)) {
        return Err(format!(
            "gen-keyence expects a supported PLC source path, got: {plc_path}"
        ));
    }

    let out_dir = out_dir.unwrap_or_else(|| Path::new("out").join("codegen").join("keyence"));
    fs::create_dir_all(out_dir.join("mnm"))
        .map_err(|err| format!("Failed to create KEYENCE MNM directory: {err}"))?;
    fs::create_dir_all(out_dir.join("variables"))
        .map_err(|err| format!("Failed to create KEYENCE variable directory: {err}"))?;
    fs::create_dir_all(out_dir.join("fb"))
        .map_err(|err| format!("Failed to create KEYENCE FB directory: {err}"))?;
    fs::create_dir_all(out_dir.join("st_reference"))
        .map_err(|err| format!("Failed to create KEYENCE ST reference directory: {err}"))?;

    let loaded = load_plc_source(Path::new(&plc_path))?;
    let parsed = parse_loaded_plc_with_required_purpose(&loaded).map_err(|err| err.to_string())?;
    let semantics =
        compile_loaded_codegen_semantics(&loaded).map_err(|errors| errors.join("\n"))?;
    let st_config = StCodegenConfig {
        program_name: program_name.clone(),
        source_file: plc_path.clone(),
        include_verification_summary: true,
        task_interval_ms: 10,
    };
    let st_result = generate_st(
        &semantics.topology,
        &semantics.constraints,
        &semantics.state_machine,
        &st_config,
    );
    let (st_text, st_blockers) = match st_result {
        Ok(st_text) => (st_text, Vec::new()),
        Err(errors) => {
            let blockers = errors
                .into_iter()
                .map(|error| error.to_string())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            (render_keyence_blocked_st_reference(&blockers), blockers)
        }
    };
    let executable_mnm = if st_blockers.is_empty() {
        match render_keyence_executable_mnm_subset(
            &program_name,
            &cpu_model,
            &semantics.topology,
            &semantics.state_machine,
        ) {
            Ok(mnm) => Ok(mnm),
            Err(blockers) => Err(blockers),
        }
    } else {
        Err(Vec::new())
    };
    let (status, codegen_blockers) = match &executable_mnm {
        Ok(_) => (
            "mnm_subset_unverified_requires_kv_studio_roundtrip_and_compile",
            Vec::new(),
        ),
        Err(keyence_blockers) if st_blockers.is_empty() => {
            let blockers = keyence_blockers
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            (
                "draft_unverified_requires_kv_studio_import_and_compile",
                blockers,
            )
        }
        Err(_) => (
            "blocked_by_rustplc_st_backend_requires_keyence_mapping",
            st_blockers,
        ),
    };

    let st_reference_path = out_dir
        .join("st_reference")
        .join(format!("{}.st", sanitize_filename(&program_name)));
    fs::write(&st_reference_path, &st_text).map_err(|err| {
        format!(
            "Failed to write ST reference {}: {err}",
            st_reference_path.display()
        )
    })?;

    let mnm_path = out_dir
        .join("mnm")
        .join(format!("{}.mnm", sanitize_filename(&program_name)));
    fs::write(
        &mnm_path,
        match &executable_mnm {
            Ok(mnm) => mnm.text.clone(),
            Err(_) => render_keyence_mnm_draft(
                &program_name,
                &cpu_model,
                &st_text,
                status,
                &codegen_blockers,
            ),
        },
    )
    .map_err(|err| format!("Failed to write MNM draft {}: {err}", mnm_path.display()))?;

    let variable_manifest_path = out_dir.join("variables").join("variables.csv");
    fs::write(
        &variable_manifest_path,
        render_keyence_variable_manifest_csv(
            &parsed,
            executable_mnm
                .as_ref()
                .ok()
                .map(|mnm| &mnm.variable_devices),
        ),
    )
    .map_err(|err| {
        format!(
            "Failed to write variable manifest {}: {err}",
            variable_manifest_path.display()
        )
    })?;

    let fb_manifest_path = out_dir.join("fb").join("fb_manifest.md");
    fs::write(&fb_manifest_path, render_keyence_fb_manifest(&parsed)).map_err(|err| {
        format!(
            "Failed to write FB manifest {}: {err}",
            fb_manifest_path.display()
        )
    })?;

    let validation_report_path = out_dir.join("validation_report.md");
    fs::write(
        &validation_report_path,
        render_keyence_validation_report(
            &plc_path,
            &cpu_model,
            &program_name,
            status,
            &codegen_blockers,
        ),
    )
    .map_err(|err| {
        format!(
            "Failed to write KEYENCE validation report {}: {err}",
            validation_report_path.display()
        )
    })?;

    let report = KeyenceExportReport {
        schema_version: 1,
        command: "gen-keyence",
        output: output_mode.as_str(),
        source_plc: plc_path,
        out_dir: display_path_relative_to_cwd(&out_dir),
        cpu_model,
        program_name,
        mnm_path: display_path_relative_to_cwd(&mnm_path),
        st_reference_path: display_path_relative_to_cwd(&st_reference_path),
        variable_manifest_path: display_path_relative_to_cwd(&variable_manifest_path),
        fb_manifest_path: display_path_relative_to_cwd(&fb_manifest_path),
        validation_report_path: display_path_relative_to_cwd(&validation_report_path),
        variable_count: parsed.topology.variables.len(),
        device_count: parsed.topology.devices.len(),
        status,
        codegen_blockers,
    };

    match output_mode {
        CliOutputMode::Human => {
            eprintln!("gen-keyence: PASS (draft artifacts only)");
            eprintln!("  source: {}", report.source_plc);
            eprintln!("  out_dir: {}", report.out_dir);
            eprintln!("  mnm: {}", report.mnm_path);
            eprintln!("  variables: {}", report.variable_manifest_path);
            eprintln!("  fb_manifest: {}", report.fb_manifest_path);
            eprintln!("  status: {}", report.status);
        }
        CliOutputMode::Json => {
            let mut body = serde_json::to_string_pretty(&report)
                .map_err(|err| format!("Failed to serialize KEYENCE report: {err}"))?;
            body.push('\n');
            print!("{body}");
        }
    }

    Ok(())
}

fn render_keyence_executable_mnm_subset(
    program_name: &str,
    cpu_model: &str,
    topology: &TopologyGraph,
    state_machine: &StateMachine,
) -> Result<KeyenceExecutableMnm, Vec<String>> {
    let mut blockers = Vec::new();
    if state_machine.states.is_empty() {
        blockers.push("KEYENCE subset requires at least one state".to_string());
    }
    if state_machine.states.len() > 80 {
        blockers.push(format!(
            "KEYENCE subset supports at most 80 state relays in phase 1, got {}",
            state_machine.states.len()
        ));
    }

    for variable in &topology.variables {
        if variable.var_type != VariableType::Bool {
            blockers.push(format!(
                "KEYENCE subset supports BOOL variables only, `{}` is {:?}",
                variable.name, variable.var_type
            ));
        }
    }

    let assigned_variables = collect_keyence_assigned_bool_variables(state_machine);
    let variable_devices = allocate_keyence_bool_devices(topology, &assigned_variables);
    let state_devices = allocate_keyence_state_devices(state_machine);
    let next_state_devices = allocate_keyence_next_state_devices(state_machine);
    let mut outgoing_counts: BTreeMap<(String, String), usize> = BTreeMap::new();

    for transition in &state_machine.transitions {
        *outgoing_counts
            .entry((
                transition.from.task_name.clone(),
                transition.from.step_name.clone(),
            ))
            .or_default() += 1;
        if !transition.effects.is_empty() {
            blockers.push(format!(
                "workpiece effects are not supported by the KEYENCE executable MNM subset at {}.{}",
                transition.from.task_name, transition.from.step_name
            ));
        }
        if !transition.timers.is_empty() {
            blockers.push(format!(
                "timer operations are not supported by the KEYENCE executable MNM subset at {}.{}",
                transition.from.task_name, transition.from.step_name
            ));
        }
        if keyence_guard_contacts(&transition.guard, &variable_devices).is_err() {
            blockers.push(format!(
                "unsupported guard for KEYENCE executable MNM subset at {}.{}: {:?}",
                transition.from.task_name, transition.from.step_name, transition.guard
            ));
        }
        for action in &transition.actions {
            if keyence_action_coil(action, &variable_devices).is_err() {
                blockers.push(format!(
                    "unsupported action for KEYENCE executable MNM subset at {}.{}: {:?}",
                    transition.from.task_name, transition.from.step_name, action
                ));
            }
        }
    }
    for ((task_name, step_name), count) in outgoing_counts {
        if count > 1 {
            blockers.push(format!(
                "KEYENCE executable MNM subset supports one outgoing transition per state, got {count} at {task_name}.{step_name}"
            ));
        }
    }

    if !blockers.is_empty() {
        return Err(blockers);
    }

    let mut out = String::new();
    out.push_str("DEVICE:60\n");
    let _ = std::fmt::Write::write_fmt(
        &mut out,
        format_args!(";MODULE:{}\n", sanitize_filename(program_name)),
    );
    out.push_str(";MODULE_TYPE:0\n");
    out.push_str("; @generated by rust_plc gen-keyence\n");
    out.push_str("; Artifact type: KEYENCE MNM subset candidate\n");
    out.push_str("; Subset: BOOL contacts/coils, state relays, SET/RES transitions, END/ENDH\n");
    out.push_str("; Status: mnm_subset_unverified_requires_kv_studio_roundtrip_and_compile\n");
    let _ = std::fmt::Write::write_fmt(&mut out, format_args!("; CPU: {cpu_model}\n"));
    let _ = std::fmt::Write::write_fmt(&mut out, format_args!("; Program: {program_name}\n"));
    out.push_str("; Validation required: import MNM, export it back, compare body/devices, then run Ctrl+F2/Ctrl+F9 in KV STUDIO\n");

    emit_keyence_initial_state_rung(&mut out, state_machine, &state_devices);
    for transition in &state_machine.transitions {
        let from_device = state_device_for(&transition.from, &state_devices)
            .expect("state device exists after preflight");
        let next_device = state_device_for(&transition.to, &next_state_devices)
            .expect("target state device exists after semantic compile");
        let contacts = keyence_guard_contacts(&transition.guard, &variable_devices)
            .expect("guard preflight should have rejected unsupported guards");
        for action in &transition.actions {
            let coil = keyence_action_coil(action, &variable_devices)
                .expect("action preflight should have rejected unsupported actions");
            emit_keyence_condition_prefix(&mut out, from_device, &contacts);
            let _ = std::fmt::Write::write_fmt(&mut out, format_args!("{} {}\n", coil.0, coil.1));
        }
        if transition.from != transition.to {
            emit_keyence_condition_prefix(&mut out, from_device, &contacts);
            let _ = std::fmt::Write::write_fmt(&mut out, format_args!("SET {next_device}\n"));
        }
    }
    emit_keyence_next_state_commit(&mut out, state_machine, &state_devices, &next_state_devices);
    out.push_str("END\n");
    out.push_str("ENDH\n");

    Ok(KeyenceExecutableMnm {
        text: out,
        variable_devices,
    })
}

fn collect_keyence_assigned_bool_variables(state_machine: &StateMachine) -> BTreeSet<String> {
    let mut assigned = BTreeSet::new();
    for transition in &state_machine.transitions {
        for action in &transition.actions {
            match action {
                TransitionAction::Compute { target, expr_raw }
                    if parse_keyence_bool_literal(expr_raw).is_some() =>
                {
                    assigned.insert(target.clone());
                }
                TransitionAction::Set { target, .. } => {
                    assigned.insert(target.clone());
                }
                _ => {}
            }
        }
    }
    assigned
}

fn allocate_keyence_bool_devices(
    topology: &TopologyGraph,
    assigned_variables: &BTreeSet<String>,
) -> KeyenceBoolDeviceMap {
    let mut by_name = BTreeMap::new();
    let mut input_index = 0usize;
    let output_base = KeyenceRelayAddress::new(5, 0);
    let mut output_index = 0usize;
    for variable in &topology.variables {
        if variable.var_type != VariableType::Bool {
            continue;
        }
        let device = if assigned_variables.contains(&variable.name) {
            let device = output_base.offset(output_index).to_string();
            output_index += 1;
            device
        } else {
            let device = format!("R{:03}", input_index);
            input_index += 1;
            device
        };
        by_name.insert(variable.name.clone(), device);
    }
    KeyenceBoolDeviceMap { by_name }
}

fn allocate_keyence_state_devices(
    state_machine: &StateMachine,
) -> BTreeMap<(String, String), String> {
    let state_base = KeyenceRelayAddress::new(9, 0);
    state_machine
        .states
        .iter()
        .enumerate()
        .map(|(index, state)| {
            (
                (state.task_name.clone(), state.step_name.clone()),
                state_base.offset(index).to_string(),
            )
        })
        .collect()
}

fn allocate_keyence_next_state_devices(
    state_machine: &StateMachine,
) -> BTreeMap<(String, String), String> {
    let next_state_base = KeyenceRelayAddress::new(20, 0);
    state_machine
        .states
        .iter()
        .enumerate()
        .map(|(index, state)| {
            (
                (state.task_name.clone(), state.step_name.clone()),
                next_state_base.offset(index).to_string(),
            )
        })
        .collect()
}

fn state_device_for<'a>(
    state: &State,
    state_devices: &'a BTreeMap<(String, String), String>,
) -> Option<&'a str> {
    state_devices
        .get(&(state.task_name.clone(), state.step_name.clone()))
        .map(String::as_str)
}

fn emit_keyence_initial_state_rung(
    out: &mut String,
    state_machine: &StateMachine,
    state_devices: &BTreeMap<(String, String), String>,
) {
    let Some(initial_device) = state_device_for(&state_machine.initial, state_devices) else {
        return;
    };
    let mut state_iter = state_machine
        .states
        .iter()
        .filter_map(|state| state_device_for(state, state_devices));
    let Some(first_state_device) = state_iter.next() else {
        return;
    };
    let _ = std::fmt::Write::write_fmt(out, format_args!("LDB {first_state_device}\n"));
    for state_device in state_iter {
        let _ = std::fmt::Write::write_fmt(out, format_args!("ANB {state_device}\n"));
    }
    let _ = std::fmt::Write::write_fmt(out, format_args!("SET {initial_device}\n"));
}

fn emit_keyence_next_state_commit(
    out: &mut String,
    state_machine: &StateMachine,
    state_devices: &BTreeMap<(String, String), String>,
    next_state_devices: &BTreeMap<(String, String), String>,
) {
    for target_state in &state_machine.states {
        let Some(next_device) = state_device_for(target_state, next_state_devices) else {
            continue;
        };
        let Some(target_device) = state_device_for(target_state, state_devices) else {
            continue;
        };
        let _ = std::fmt::Write::write_fmt(out, format_args!("LD {next_device}\n"));
        let _ = std::fmt::Write::write_fmt(out, format_args!("SET {target_device}\n"));
        for other_state in &state_machine.states {
            if other_state == target_state {
                continue;
            }
            if let Some(other_device) = state_device_for(other_state, state_devices) {
                let _ = std::fmt::Write::write_fmt(out, format_args!("LD {next_device}\n"));
                let _ = std::fmt::Write::write_fmt(out, format_args!("RES {other_device}\n"));
            }
        }
        let _ = std::fmt::Write::write_fmt(out, format_args!("LD {next_device}\n"));
        let _ = std::fmt::Write::write_fmt(out, format_args!("RES {next_device}\n"));
    }
}

fn keyence_guard_contacts(
    guard: &TransitionGuard,
    variable_devices: &KeyenceBoolDeviceMap,
) -> Result<Vec<(&'static str, String)>, ()> {
    match guard {
        TransitionGuard::Always => Ok(Vec::new()),
        TransitionGuard::Condition { expression } => {
            let (name, expected) = parse_keyence_bool_condition(expression).ok_or(())?;
            let device = variable_devices.device_for(&name).ok_or(())?.to_string();
            Ok(vec![(if expected { "AND" } else { "ANB" }, device)])
        }
        TransitionGuard::Edge { .. }
        | TransitionGuard::Timeout { .. }
        | TransitionGuard::Delay { .. } => Err(()),
    }
}

fn keyence_action_coil(
    action: &TransitionAction,
    variable_devices: &KeyenceBoolDeviceMap,
) -> Result<(&'static str, String), ()> {
    match action {
        TransitionAction::Compute { target, expr_raw } => {
            let value = parse_keyence_bool_literal(expr_raw).ok_or(())?;
            let device = variable_devices.device_for(target).ok_or(())?.to_string();
            Ok((if value { "SET" } else { "RES" }, device))
        }
        TransitionAction::Set { target, value, .. } => {
            let device = variable_devices.device_for(target).ok_or(())?.to_string();
            Ok((
                match value {
                    BinaryValue::On => "SET",
                    BinaryValue::Off => "RES",
                },
                device,
            ))
        }
        TransitionAction::Log { .. } => Err(()),
        _ => Err(()),
    }
}

fn emit_keyence_condition_prefix(
    out: &mut String,
    state_device: &str,
    contacts: &[(&'static str, String)],
) {
    let _ = std::fmt::Write::write_fmt(out, format_args!("LD {state_device}\n"));
    for (instruction, device) in contacts {
        let _ = std::fmt::Write::write_fmt(out, format_args!("{instruction} {device}\n"));
    }
}

fn parse_keyence_bool_condition(expression: &str) -> Option<(String, bool)> {
    let normalized = expression.trim();
    if let Some((left, right)) = normalized.split_once("==") {
        let expected = parse_keyence_bool_literal(right)?;
        return Some((left.trim().to_string(), expected));
    }
    if let Some((left, right)) = normalized.split_once("!=") {
        let expected = !parse_keyence_bool_literal(right)?;
        return Some((left.trim().to_string(), expected));
    }
    if is_simple_identifier(normalized) {
        return Some((normalized.to_string(), true));
    }
    None
}

fn parse_keyence_bool_literal(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "on" | "1" => Some(true),
        "false" | "off" | "0" => Some(false),
        _ => None,
    }
}

fn is_simple_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn render_keyence_mnm_draft(
    program_name: &str,
    cpu_model: &str,
    st_text: &str,
    status: &str,
    blockers: &[String],
) -> String {
    let mut out = String::new();
    out.push_str("; @generated by rust_plc gen-keyence\n");
    out.push_str("; Artifact type: KEYENCE MNM/ST draft for human import review\n");
    let _ = std::fmt::Write::write_fmt(&mut out, format_args!("; Status: {status}\n"));
    let _ = std::fmt::Write::write_fmt(&mut out, format_args!("; CPU: {cpu_model}\n"));
    let _ = std::fmt::Write::write_fmt(&mut out, format_args!("; Program: {program_name}\n"));
    out.push_str("; Variables are externalized in variables/variables.csv; do not treat this as a KV STUDIO compile pass.\n");
    if !blockers.is_empty() {
        out.push_str("; RustPLC backend blockers:\n");
        for blocker in blockers {
            let _ = std::fmt::Write::write_fmt(&mut out, format_args!("; - {blocker}\n"));
        }
    }
    out.push_str("; --- ST reference body follows ---\n");
    for line in st_text.lines() {
        let _ = std::fmt::Write::write_fmt(&mut out, format_args!("; {line}\n"));
    }
    out
}

fn render_keyence_blocked_st_reference(blockers: &[String]) -> String {
    let mut out = String::from("(* RustPLC ST backend blocked KEYENCE reference generation. *)\n");
    out.push_str("(* The KEYENCE package still contains variable and FB manifests, but executable MNM requires a vendor-specific lowering pass. *)\n");
    for blocker in blockers {
        let _ = std::fmt::Write::write_fmt(&mut out, format_args!("(* - {blocker} *)\n"));
    }
    out
}

fn render_keyence_variable_manifest_csv(
    program: &rust_plc::ast::PlcProgram,
    device_map: Option<&KeyenceBoolDeviceMap>,
) -> String {
    let mut out = String::from("scope,name,keyence_type,device,initial_value,comment\n");
    for var in &program.topology.variables {
        let device = device_map
            .and_then(|map| map.device_for(&var.name))
            .unwrap_or("");
        let _ = std::fmt::Write::write_fmt(
            &mut out,
            format_args!(
                "global,{},{},{},{},{}\n",
                csv_escape(&var.name),
                keyence_ast_type_name(&var.var_type),
                csv_escape(device),
                csv_escape(&var.initial_value),
                csv_escape("RustPLC variable; reconstruct in KV STUDIO variable editor")
            ),
        );
    }
    for alias_group in &program.topology.controller_io {
        for alias in &alias_group.aliases {
            let direction = match alias.direction {
                rust_plc::ast::ControllerIoDirection::Input => "BOOL_INPUT",
                rust_plc::ast::ControllerIoDirection::Output => "BOOL_OUTPUT",
            };
            let _ = std::fmt::Write::write_fmt(
                &mut out,
                format_args!(
                    "controller_io,{},{},{},{},{}\n",
                    csv_escape(&alias.alias),
                    direction,
                    csv_escape(&alias.port),
                    csv_escape(alias.safe_state.as_deref().unwrap_or("")),
                    csv_escape(
                        alias
                            .purpose
                            .as_deref()
                            .unwrap_or("RustPLC controller_io alias")
                    )
                ),
            );
        }
    }
    out
}

fn render_keyence_fb_manifest(program: &rust_plc::ast::PlcProgram) -> String {
    let mut out = String::from("# KEYENCE FB Manifest\n\n");
    out.push_str("Status: generated review manifest only. Official FB import must be done in KV STUDIO and validated with Ctrl+F2/Ctrl+F9.\n\n");
    out.push_str("## Official FBs Imported Directly\n\nNone declared by RustPLC generator.\n\n");
    out.push_str("## Device Families Requiring Mapping Review\n\n");
    if program.topology.devices.is_empty() {
        out.push_str("- none\n");
    } else {
        for device in &program.topology.devices {
            let _ = std::fmt::Write::write_fmt(
                &mut out,
                format_args!("- `{}`: `{:?}`\n", device.name, device.device_type),
            );
        }
    }
    out
}

fn render_keyence_validation_report(
    source_plc: &str,
    cpu_model: &str,
    program_name: &str,
    status: &str,
    blockers: &[String],
) -> String {
    let mut out = format!(
        "# KEYENCE Validation Report\n\n- Source PLC: `{source_plc}`\n- CPU model: `{cpu_model}`\n- Program: `{program_name}`\n- Status: `{status}`\n\nThis package was generated by RustPLC, but it has not been imported into KV STUDIO and has not passed Ctrl+F2/Ctrl+F9. Per the KEYENCE workflow, final acceptance requires MNM import, variable reconstruction through the Variable Editor, and collected KV STUDIO compile evidence.\n"
    );
    if !blockers.is_empty() {
        out.push_str("\n## RustPLC Backend Blockers\n\n");
        for blocker in blockers {
            let _ = std::fmt::Write::write_fmt(&mut out, format_args!("- {blocker}\n"));
        }
        out.push_str("\nThese blockers mean the current package is a KEYENCE review scaffold, not an executable vendor program.\n");
    }
    out
}

fn keyence_ast_type_name(var_type: &AstVariableType) -> &'static str {
    match var_type {
        AstVariableType::Bool => "BOOL",
        AstVariableType::Int => "DINT",
        AstVariableType::Float => "REAL",
    }
}

fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn sanitize_filename(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "Main".to_string()
    } else {
        sanitized
    }
}
