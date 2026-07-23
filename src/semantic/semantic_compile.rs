#[derive(Debug)]
pub struct SemanticCompileArtifacts {
    pub expanded_program: PlcProgram,
    pub topology: TopologyGraph,
    pub state_machine: StateMachine,
    pub constraints: ConstraintSet,
    pub timing_model: TimingModel,
}

/// Compile source AST into the complete semantic IR set through one preprocessing pass.
pub fn compile_semantic_program_with_library(
    program: &PlcProgram,
    device_library: Option<&crate::device_library::DeviceLibrary>,
) -> Result<SemanticCompileArtifacts, Vec<PlcError>> {
    validate_source_topology_semantics(program)?;
    validate_state_machine_source(program, true)?;

    let expanded_program = preprocess_program_with_library(program, device_library)?;
    if let Err(error) = validate_topology_semantics(&expanded_program.topology) {
        return Err(topology_gate_error_to_plc_errors(error));
    }

    let mut errors = Vec::new();
    let topology = collect_semantic_compile_stage(
        build_topology_graph_from_preprocessed(&expanded_program),
        &mut errors,
    );
    let state_machine = collect_semantic_compile_stage(
        build_state_machine_from_preprocessed(expanded_program.clone()),
        &mut errors,
    );
    let constraints = collect_semantic_compile_stage(
        build_constraint_set_from_preprocessed(&expanded_program),
        &mut errors,
    );
    let timing_model = collect_semantic_compile_stage(
        build_timing_model_from_preprocessed(&expanded_program),
        &mut errors,
    );

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(SemanticCompileArtifacts {
        expanded_program,
        topology: topology.expect("topology exists when semantic errors are empty"),
        state_machine: state_machine.expect("state machine exists when semantic errors are empty"),
        constraints: constraints.expect("constraints exist when semantic errors are empty"),
        timing_model: timing_model.expect("timing model exists when semantic errors are empty"),
    })
}

fn collect_semantic_compile_stage<T>(
    result: Result<T, Vec<PlcError>>,
    errors: &mut Vec<PlcError>,
) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(mut stage_errors) => {
            errors.append(&mut stage_errors);
            None
        }
    }
}
