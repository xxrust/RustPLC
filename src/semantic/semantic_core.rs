pub fn build_topology_graph(program: &PlcProgram) -> Result<TopologyGraph, Vec<PlcError>> {
    build_topology_from_ast(&program.topology)
}

pub fn validate_source_topology_semantics(program: &PlcProgram) -> Result<(), Vec<PlcError>> {
    let mut errors = Vec::new();

    if let Err(gate_error) = validate_removed_legacy_io_model(&program.topology) {
        errors.extend(topology_gate_error_to_plc_errors(gate_error));
    }
    if let Err(gate_error) = validate_topology_semantics(&program.topology) {
        errors.extend(topology_gate_error_to_plc_errors(gate_error));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

pub fn build_state_machine(program: &PlcProgram) -> Result<StateMachine, Vec<PlcError>> {
    if source_topology_gates_required(program) {
        validate_source_topology_semantics(program)?;
    }
    let mut source_errors = Vec::new();
    validate_raw_io_bypass_in_tasks(program, &mut source_errors);
    if !source_errors.is_empty() {
        return Err(source_errors);
    }
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
    device_semantics::cylinder::validate_closed_loop_feedback_contracts_in_tasks(
        &expanded.tasks,
        &expanded.topology,
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

fn source_topology_gates_required(program: &PlcProgram) -> bool {
    program.topology.devices.iter().any(|device| {
        matches!(device.device_type, DeviceType::Plc) && device.attributes.model_ref.is_some()
    })
}

fn topology_gate_error_to_plc_errors(error: TopologySemanticGateError) -> Vec<PlcError> {
    error
        .issues
        .into_iter()
        .map(|issue| {
            PlcError::semantic_with_reason(
                issue.line.max(1),
                format!("[{}] {}", issue.code.as_str(), issue.message),
                issue.suggestion,
            )
        })
        .collect()
}

fn validate_raw_io_bypass_in_tasks(program: &PlcProgram, errors: &mut Vec<PlcError>) {
    if !source_topology_gates_required(program) {
        return;
    }

    let device_types = program
        .topology
        .devices
        .iter()
        .map(|device| (device.name.as_str(), &device.device_type))
        .collect::<HashMap<_, _>>();

    for task in &program.tasks.tasks {
        for step in &task.steps {
            validate_raw_io_bypass_in_statements(
                &step.statements,
                step.line.max(task.line).max(1),
                &device_types,
                errors,
            );
        }
    }
}

fn validate_raw_io_bypass_in_statements(
    statements: &[StepStatement],
    line: usize,
    device_types: &HashMap<&str, &DeviceType>,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::Set { target, .. })
            | StepStatement::Action(ActionStatement::SetAnalog { target, .. })
            | StepStatement::Action(ActionStatement::SetAnalogExpr { target, .. }) => {
                if let Some(message) = raw_io_bypass_message(target, device_types) {
                    errors.push(PlcError::semantic_with_reason(
                        line,
                        format!("[SEM-110] {message}"),
                        "use the high-level device action, or move this source into an explicit low-level fixture/mode instead of normal task logic",
                    ));
                }
            }
            StepStatement::Repeat { body, .. } => {
                validate_raw_io_bypass_in_statements(body, line, device_types, errors);
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_raw_io_bypass_in_statements(
                        &branch.statements,
                        line,
                        device_types,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_raw_io_bypass_in_statements(
                        &branch.statements,
                        line,
                        device_types,
                        errors,
                    );
                }
            }
            _ => {}
        }
    }
}

fn raw_io_bypass_message(
    target: &ActionTarget,
    device_types: &HashMap<&str, &DeviceType>,
) -> Option<String> {
    if target.port != "self" && parse_physical_plc_port_ref(&target.port).is_some() {
        return Some(format!(
            "normal task action writes controller port `{target}` directly"
        ));
    }

    let device_type = device_types.get(target.device.as_str())?;
    match device_type {
        DeviceType::StepperMotor | DeviceType::ServoDrive
            if matches!(target.port.as_str(), "enable" | "pulse" | "direction") =>
        {
            Some(format!(
                "normal task action writes low-level axis port `{target}` directly"
            ))
        }
        DeviceType::SolenoidValve if target.port.contains("coil") || target.port == "out" => {
            Some(format!(
                "normal task action writes valve port `{target}` directly"
            ))
        }
        DeviceType::Conveyor if matches!(target.port.as_str(), "drive" | "run" | "cmd") => {
            Some(format!(
                "normal task action writes conveyor drive port `{target}` directly"
            ))
        }
        DeviceType::Pump if matches!(target.port.as_str(), "drive" | "run" | "cmd") => {
            Some(format!(
                "normal task action writes pump drive port `{target}` directly"
            ))
        }
        DeviceType::Heater if matches!(target.port.as_str(), "power" | "cmd") => {
            Some(format!(
                "normal task action writes heater power port `{target}` directly"
            ))
        }
        DeviceType::VisionSensor if target.port == "trigger" => Some(format!(
            "normal task action writes vision trigger port `{target}` directly"
        )),
        DeviceType::ProportionalValve if matches!(target.port.as_str(), "cmd" | "opening") => {
            Some(format!(
                "normal task action writes proportional valve command port `{target}` directly"
            ))
        }
        DeviceType::Gripper if matches!(target.port.as_str(), "cmd" | "grip" | "release") => {
            Some(format!(
                "normal task action writes gripper command port `{target}` directly"
            ))
        }
        _ => None,
    }
}

include!("semantic_externs.rs");
include!("semantic_wait_regions.rs");
include!("semantic_topology_lowering.rs");
include!("semantic_workpiece_lowering.rs");
