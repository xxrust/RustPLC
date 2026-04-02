use runtime_core::Program;
use rust_plc::error::PlcError;
use rust_plc::ir::{ConstraintSet, StateMachine, TopologyGraph};
use rust_plc::parser::parse_plc;
use rust_plc::runtime_bridge::state_machine_to_runtime_program;
use rust_plc::semantic::{
    build_constraint_set, build_state_machine, build_topology_graph, preprocess_program,
    preprocess_program_with_library,
};
use rust_plc::topology_semantic_gate::{
    collect_topology_deprecation_warnings, validate_device_purpose_required,
    validate_removed_legacy_io_model, validate_topology_semantics,
};
use rust_plc::verification::{VerificationSummary, verify_all};
use std::path::Path;

pub(crate) struct CompiledPlcSemantics {
    pub(crate) source_program: rust_plc::ast::PlcProgram,
    pub(crate) expanded_program: rust_plc::ast::PlcProgram,
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

pub(crate) fn compile_plc_semantics(
    plc_source: &str,
) -> Result<CompiledPlcSemantics, Vec<String>> {
    let program = parse_plc_with_required_purpose(plc_source).map_err(|err| vec![err])?;
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
    .map_err(flatten_semantic_errors_vec)?;
    validate_topology_semantics(&expanded.topology)
        .map_err(|gate_error| vec![gate_error.to_string()])?;

    let mut errors = Vec::<PlcError>::new();
    let topology = collect_stage(build_topology_graph(&expanded), &mut errors);
    let state_machine = collect_stage(build_state_machine(&expanded), &mut errors);
    let constraints = collect_stage(build_constraint_set(&expanded), &mut errors);

    if !errors.is_empty() {
        return Err(flatten_semantic_errors_vec(errors));
    }

    Ok(CompiledPlcSemantics {
        source_program: program,
        expanded_program: expanded,
        topology: topology.expect("topology exists when semantic errors are empty"),
        state_machine: state_machine.expect("state machine exists when semantic errors are empty"),
        constraints: constraints.expect("constraints exist when semantic errors are empty"),
    })
}

pub(crate) fn preprocess_plc_source(
    plc_source: &str,
) -> Result<rust_plc::ast::PlcProgram, String> {
    let program = parse_plc_with_required_purpose(plc_source)?;
    preprocess_program(&program).map_err(flatten_semantic_errors)
}

pub(crate) fn build_runtime_semantics(plc_source: &str) -> Result<CompiledPlcSemantics, String> {
    compile_plc_semantics(plc_source).map_err(|errors| errors.join("\n"))
}

pub(crate) fn verify_compiled_plc_semantics(
    semantics: &CompiledPlcSemantics,
) -> Result<VerificationSummary, Vec<String>> {
    verify_all(
        &semantics.expanded_program,
        &semantics.topology,
        &semantics.constraints,
        &semantics.state_machine,
    )
    .map_err(|issues| issues.into_iter().map(|issue| issue.to_string()).collect())
}

pub(crate) fn collect_compiled_plc_warnings(semantics: &CompiledPlcSemantics) -> Vec<String> {
    collect_topology_deprecation_warnings(&semantics.source_program.topology)
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

fn flatten_semantic_errors(errors: Vec<rust_plc::error::PlcError>) -> String {
    flatten_semantic_errors_vec(errors).join("\n")
}
