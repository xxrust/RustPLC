use runtime_core::Program;
use rust_plc::error::PlcError;
use rust_plc::ir::{ConstraintSet, StateMachine, TopologyGraph};
use rust_plc::parser::parse_plc;
use rust_plc::runtime_bridge::state_machine_to_runtime_program;
use rust_plc::semantic::{
    build_constraint_set, build_state_machine, build_topology_graph,
    preprocess_program_with_library,
};
use rust_plc::source_bundle::{LoadedPlcSource, remap_plc_error};
use rust_plc::topology_semantic_gate::{
    collect_topology_deprecation_warnings, validate_device_purpose_required,
    validate_removed_legacy_io_model, validate_topology_semantics,
};
use rust_plc::verification::verify_all;
use std::fmt::Write as _;
use std::path::Path;

pub(crate) struct RuntimeSemantics {
    pub(crate) topology: TopologyGraph,
    pub(crate) state_machine: StateMachine,
    pub(crate) constraints: ConstraintSet,
}

pub(crate) struct CodegenSemantics {
    pub(crate) topology: TopologyGraph,
    pub(crate) state_machine: StateMachine,
    pub(crate) constraints: ConstraintSet,
}

pub(crate) fn parse_plc_with_required_purpose(
    source: &str,
) -> Result<rust_plc::ast::PlcProgram, String> {
    let program = parse_plc(source).map_err(|e| e.to_string())?;
    validate_removed_legacy_io_model(&program.topology)
        .map_err(|gate_error| gate_error.to_string())?;
    validate_device_purpose_required(&program.topology)
        .map_err(|gate_error| gate_error.to_string())?;
    Ok(program)
}

pub(crate) fn parse_loaded_plc_with_required_purpose(
    input: &LoadedPlcSource,
) -> Result<rust_plc::ast::PlcProgram, String> {
    let program =
        parse_plc(&input.source).map_err(|e| remap_plc_error(e, &input.source_map).to_string())?;
    validate_removed_legacy_io_model(&program.topology)
        .map_err(|gate_error| format_topology_gate_error(gate_error, input))?;
    validate_device_purpose_required(&program.topology)
        .map_err(|gate_error| format_topology_gate_error(gate_error, input))?;
    Ok(program)
}

pub(crate) fn format_loaded_plc_errors(
    errors: Vec<PlcError>,
    input: &LoadedPlcSource,
) -> Vec<String> {
    errors
        .into_iter()
        .map(|error| remap_plc_error(error, &input.source_map).to_string())
        .collect()
}

pub(crate) fn compile_loaded_codegen_semantics(
    input: &LoadedPlcSource,
) -> Result<CodegenSemantics, Vec<String>> {
    let program = parse_loaded_plc_with_required_purpose(input).map_err(|err| vec![err])?;
    for warning in collect_topology_deprecation_warnings(&program.topology) {
        eprintln!("WARNING [deprecation] {warning}");
    }

    let devices_dir = Path::new("devices");
    let device_library = rust_plc::device_library::DeviceLibrary::load(devices_dir)
        .map_err(flatten_semantic_errors_vec)?;

    let expanded = preprocess_program_with_library(
        &program,
        if device_library.is_empty() {
            None
        } else {
            Some(&device_library)
        },
    )
    .map_err(|errors| format_loaded_plc_errors(errors, input))?;
    validate_topology_semantics(&expanded.topology)
        .map_err(|gate_error| vec![format_topology_gate_error(gate_error, input)])?;

    let mut errors = Vec::<PlcError>::new();
    let topology = collect_stage(build_topology_graph(&expanded), &mut errors);
    let state_machine = collect_stage(build_state_machine(&expanded), &mut errors);
    let constraints = collect_stage(build_constraint_set(&expanded), &mut errors);

    if !errors.is_empty() {
        return Err(format_loaded_plc_errors(errors, input));
    }

    let topology = topology.expect("topology exists when semantic errors are empty");
    let state_machine = state_machine.expect("state machine exists when semantic errors are empty");
    let constraints = constraints.expect("constraints exist when semantic errors are empty");

    verify_all(&expanded, &topology, &constraints, &state_machine).map_err(|issues| {
        issues
            .into_iter()
            .map(|issue| issue.to_string())
            .collect::<Vec<_>>()
    })?;

    Ok(CodegenSemantics {
        topology,
        state_machine,
        constraints,
    })
}

pub(crate) fn build_runtime_semantics(plc_source: &str) -> Result<RuntimeSemantics, String> {
    let program = parse_plc_with_required_purpose(plc_source)?;
    let devices_dir = Path::new("devices");
    let device_library = rust_plc::device_library::DeviceLibrary::load(devices_dir)
        .map_err(|errors| flatten_semantic_errors_vec(errors).join("\n"))?;

    let expanded = preprocess_program_with_library(
        &program,
        if device_library.is_empty() {
            None
        } else {
            Some(&device_library)
        },
    )
    .map_err(|errors| flatten_semantic_errors_vec(errors).join("\n"))?;
    validate_topology_semantics(&expanded.topology).map_err(|gate_error| gate_error.to_string())?;

    let mut errors = Vec::<PlcError>::new();
    let topology = collect_stage(build_topology_graph(&expanded), &mut errors);
    let state_machine = collect_stage(build_state_machine(&expanded), &mut errors);
    let constraints = collect_stage(build_constraint_set(&expanded), &mut errors);

    if !errors.is_empty() {
        return Err(flatten_semantic_errors_vec(errors).join("\n"));
    }

    Ok(RuntimeSemantics {
        topology: topology.expect("topology exists when semantic errors are empty"),
        state_machine: state_machine.expect("state machine exists when semantic errors are empty"),
        constraints: constraints.expect("constraints exist when semantic errors are empty"),
    })
}

pub(crate) fn compile_plc_to_runtime_program(
    plc_source: &str,
    tick_ms: u64,
) -> Result<Program<'static>, String> {
    let semantics = build_runtime_semantics(plc_source)?;
    state_machine_to_runtime_program(
        &semantics.topology,
        &semantics.constraints,
        &semantics.state_machine,
        tick_ms,
    )
    .map_err(|e| e.to_string())
}

fn collect_stage<T>(result: Result<T, Vec<PlcError>>, errors: &mut Vec<PlcError>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(mut stage_errors) => {
            errors.append(&mut stage_errors);
            None
        }
    }
}

fn flatten_semantic_errors_vec(errors: Vec<rust_plc::error::PlcError>) -> Vec<String> {
    errors
        .into_iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
}

fn format_topology_gate_error(
    gate_error: rust_plc::topology_semantic_gate::TopologySemanticGateError,
    input: &LoadedPlcSource,
) -> String {
    let mut rendered = format!(
        "ERROR [{}] Topology semantic gate rejected the program\n",
        gate_error.code
    );
    for issue in gate_error.issues {
        if let Some(location) = input.source_map.remap_location(issue.line.max(1), 1) {
            let _ = writeln!(
                rendered,
                "  - [{}] {}:{}:{}",
                issue.code.as_str(),
                location.file,
                location.line.max(1),
                location.column.max(1)
            );
        } else {
            let _ = writeln!(
                rendered,
                "  - [{}] line {}: {}",
                issue.code.as_str(),
                issue.line.max(1),
                issue.message
            );
        }
    }
    rendered.trim_end().to_string()
}
