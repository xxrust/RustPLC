pub fn build_topology_graph(program: &PlcProgram) -> Result<TopologyGraph, Vec<PlcError>> {
    build_topology_from_ast(&program.topology)
}

pub fn build_state_machine(program: &PlcProgram) -> Result<StateMachine, Vec<PlcError>> {
    let mut expanded = preprocess_program(program)?;
    let variable_types = collect_variable_types(&expanded.topology);
    let mut expr_errors = Vec::new();
    validate_expression_actions_in_tasks(&expanded.tasks, &variable_types, &mut expr_errors);
    let extern_signatures =
        collect_extern_function_signatures(&expanded.topology, &mut expr_errors);
    validate_extern_calls_in_tasks(
        &expanded.tasks,
        &extern_signatures,
        &variable_types,
        &mut expr_errors,
    );
    validate_non_pure_extern_concurrency_in_tasks(
        &expanded.tasks,
        &extern_signatures,
        &mut expr_errors,
    );
    let device_kinds = collect_device_kinds(&expanded.topology);
    let cam_table_names = expanded
        .topology
        .cam_tables
        .iter()
        .map(|table| table.name.clone())
        .collect::<HashSet<_>>();
    device_semantics::cam::validate_cam_actions_in_tasks(
        &expanded.tasks,
        &device_kinds,
        &cam_table_names,
        &mut expr_errors,
    );
    device_semantics::axis::validate_axis_motion_actions_in_tasks(
        &expanded.tasks,
        &device_kinds,
        &mut expr_errors,
    );
    device_semantics::validate_task_action_semantics(
        &expanded.tasks,
        &device_kinds,
        &mut expr_errors,
    );
    device_semantics::axis::resolve_axis_motion_parameters_in_tasks(
        &mut expanded.tasks,
        &expanded.topology,
        &mut expr_errors,
    );
    device_semantics::axis::validate_vertical_axis_brake_sequence_in_tasks(
        &expanded.tasks,
        &expanded.topology,
        &mut expr_errors,
    );
    let has_workpiece_context = !expanded.topology.workpiece_types.is_empty()
        || !expanded.topology.workpiece_sites.is_empty()
        || !expanded.topology.workpiece_holders.is_empty()
        || !expanded.topology.workpiece_carriers.is_empty()
        || tasks_use_workpiece_effects(&expanded.tasks);
    if has_workpiece_context {
        let mut workpiece_constraints = ConstraintSet::default();
        let catalog = validate_and_lower_workpiece_topology_v2(
            &expanded.topology,
            &mut workpiece_constraints,
            &mut expr_errors,
        );
        validate_workpiece_effects_in_tasks_v2(
            &expanded.tasks,
            &catalog,
            &workpiece_constraints.workpiece_types,
            &mut expr_errors,
        );
    }
    if !expr_errors.is_empty() {
        return Err(expr_errors);
    }
    let wait_ctx = WaitExpressionContext::for_program(&expanded);
    build_state_machine_from_ast_with_context(&expanded.tasks, &wait_ctx, Some(&device_kinds))
}

include!("semantic_externs.rs");
include!("semantic_wait_regions.rs");
include!("semantic_topology_lowering.rs");
include!("semantic_workpiece_lowering.rs");
