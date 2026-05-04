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
    validate_station_protocol_semantics(program, &mut source_errors);
    if !source_errors.is_empty() {
        return Err(source_errors);
    }
    validate_raw_io_bypass_in_tasks(program, &mut source_errors);
    if !source_errors.is_empty() {
        return Err(source_errors);
    }
    if source_topology_gates_required(program) {
        device_semantics::process::validate_process_device_source_contracts(
            &program.topology,
            &mut source_errors,
        );
    }
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
        &expanded.topology,
        &device_kinds,
        &mut expr_errors,
    );
    device_semantics::process::validate_process_device_actions_in_tasks(
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

fn validate_station_protocol_semantics(program: &PlcProgram, errors: &mut Vec<PlcError>) {
    if program.topology.stations.is_empty()
        && program.topology.handshakes.is_empty()
        && program.topology.transfer_points.is_empty()
    {
        return;
    }

    let devices = program
        .topology
        .devices
        .iter()
        .map(|device| device.name.as_str())
        .collect::<HashSet<_>>();
    let tasks = collect_task_steps(&program.tasks);
    let task_names = tasks.keys().cloned().collect::<HashSet<_>>();
    let sites = program
        .topology
        .workpiece_sites
        .iter()
        .map(|site| (site.name.as_str(), site.capacity))
        .collect::<HashMap<_, _>>();

    let mut station_names = HashSet::<String>::new();
    let mut device_owner = HashMap::<String, String>::new();
    let mut task_owner = HashMap::<String, String>::new();

    for station in &program.topology.stations {
        if !station_names.insert(station.name.clone()) {
            errors.push(PlcError::duplicate_definition_with_reason(
                station.line.max(1),
                "station",
                &station.name,
                "station 名称必须唯一",
            ));
        }

        let mut local_devices = HashSet::<&str>::new();
        for device in &station.owns {
            if !local_devices.insert(device.as_str()) {
                errors.push(PlcError::semantic_with_reason(
                    station.line.max(1),
                    format!(
                        "[SEM-203] station '{}' owns duplicate device '{}'.",
                        station.name, device
                    ),
                    "请在 station.owns 中只列出一次同一设备".to_string(),
                ));
            }
            if !devices.contains(device.as_str()) {
                errors.push(PlcError::undefined_reference_with_reason(
                    station.line.max(1),
                    "设备",
                    device,
                    format!("[SEM-201] station '{}' owns 未定义设备", station.name),
                ));
                continue;
            }
            if let Some(previous) = device_owner.insert(device.clone(), station.name.clone()) {
                errors.push(PlcError::semantic_with_reason(
                    station.line.max(1),
                    format!(
                        "[SEM-202] device '{device}' is owned by both '{previous}' and '{}'.",
                        station.name
                    ),
                    "一个设备只能归属一个 station；跨站协作请通过 handshake/transfer_point 建模"
                        .to_string(),
                ));
            }
        }

        let mut local_tasks = HashSet::<&str>::new();
        for task in &station.tasks {
            if !local_tasks.insert(task.as_str()) {
                errors.push(PlcError::semantic_with_reason(
                    station.line.max(1),
                    format!(
                        "[SEM-205] station '{}' declares duplicate task '{}'.",
                        station.name, task
                    ),
                    "请在 station.tasks 中只列出一次同一 task".to_string(),
                ));
            }
            if !task_names.contains(task) {
                errors.push(PlcError::undefined_reference_with_reason(
                    station.line.max(1),
                    "task",
                    task,
                    format!(
                        "[SEM-206] station '{}' references 未定义 task",
                        station.name
                    ),
                ));
                continue;
            }
            if let Some(previous) = task_owner.insert(task.clone(), station.name.clone()) {
                errors.push(PlcError::semantic_with_reason(
                    station.line.max(1),
                    format!(
                        "[SEM-207] task '{task}' is assigned to both '{previous}' and '{}'.",
                        station.name
                    ),
                    "一个 task 只能归属一个 station；共享流程应拆成显式 handshake".to_string(),
                ));
            }
        }
    }

    validate_station_task_ownership(program, &device_owner, &task_owner, errors);
    validate_handshakes(program, &station_names, &tasks, errors);
    validate_transfer_points(program, &station_names, &sites, errors);
}

fn validate_station_task_ownership(
    program: &PlcProgram,
    device_owner: &HashMap<String, String>,
    task_owner: &HashMap<String, String>,
    errors: &mut Vec<PlcError>,
) {
    for task in &program.tasks.tasks {
        let Some(owner_station) = task_owner.get(&task.name) else {
            continue;
        };
        for step in &task.steps {
            let mut targets = Vec::new();
            collect_action_target_devices(&step.statements, &mut targets);
            for target in targets {
                let Some(target_owner) = device_owner.get(&target) else {
                    continue;
                };
                if target_owner != owner_station {
                    errors.push(PlcError::semantic_with_reason(
                        step.line.max(task.line).max(1),
                        format!(
                            "[SEM-204] task '{}' belongs to station '{}' but writes device '{}' owned by station '{}'.",
                            task.name, owner_station, target, target_owner
                        ),
                        "请把动作移到设备所属 station 的 task，或通过 handshake/transfer_point 表达跨站交互"
                            .to_string(),
                    ));
                }
            }
        }
    }
}

fn collect_action_target_devices(statements: &[StepStatement], out: &mut Vec<String>) {
    for statement in statements {
        match statement {
            StepStatement::Action(action) => match action {
                ActionStatement::Set { target, .. }
                | ActionStatement::SetAnalog { target, .. }
                | ActionStatement::SetAnalogExpr { target, .. }
                | ActionStatement::DeviceAction { target, .. }
                | ActionStatement::AxisMoveRelative { target, .. }
                | ActionStatement::AxisMoveAbsolute { target, .. } => {
                    out.push(target.device.clone());
                }
                ActionStatement::Extend { target, .. }
                | ActionStatement::Retract { target, .. } => {
                    out.push(target.device.clone());
                }
                ActionStatement::CamEngage { target }
                | ActionStatement::CamDisengage { target }
                | ActionStatement::CamSwitch { target, .. }
                | ActionStatement::CamPhase { target, .. } => out.push(target.clone()),
                ActionStatement::Call { .. }
                | ActionStatement::Compute { .. }
                | ActionStatement::Log { .. } => {}
            },
            StepStatement::Repeat { body, .. } => collect_action_target_devices(body, out),
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    collect_action_target_devices(&branch.statements, out);
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    collect_action_target_devices(&branch.statements, out);
                }
            }
            StepStatement::Effect(_)
            | StepStatement::Wait(_)
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::IfElse { .. }
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
}

fn validate_handshakes(
    program: &PlcProgram,
    station_names: &HashSet<String>,
    task_steps: &HashMap<String, HashSet<String>>,
    errors: &mut Vec<PlcError>,
) {
    let mut handshake_names = HashSet::<String>::new();
    let mut signals = HashMap::<String, String>::new();
    let mut wait_edges = HashMap::<String, Vec<String>>::new();

    for handshake in &program.topology.handshakes {
        if !handshake_names.insert(handshake.name.clone()) {
            errors.push(PlcError::duplicate_definition_with_reason(
                handshake.line.max(1),
                "handshake",
                &handshake.name,
                "handshake 名称必须唯一",
            ));
        }
        for (field, station) in [("from", &handshake.from), ("to", &handshake.to)] {
            if !station_names.contains(station) {
                errors.push(PlcError::undefined_reference_with_reason(
                    handshake.line.max(1),
                    "station",
                    station,
                    format!(
                        "[SEM-211] handshake '{}' 的 {field} station 未定义",
                        handshake.name
                    ),
                ));
            }
        }
        for signal in [&handshake.request, &handshake.allow, &handshake.complete] {
            if let Some(previous) = signals.insert(signal.clone(), handshake.name.clone()) {
                errors.push(PlcError::semantic_with_reason(
                    handshake.line.max(1),
                    format!(
                        "[SEM-212] handshake signal '{signal}' is reused by '{previous}' and '{}'.",
                        handshake.name
                    ),
                    "站间握手信号必须唯一，避免两个协议共享同一 request/allow/complete 位"
                        .to_string(),
                ));
            }
        }
        validate_goto_target(
            handshake.line.max(1),
            &handshake.timeout.target,
            task_steps,
            "[SEM-214] handshake timeout goto target is invalid",
            errors,
        );
        wait_edges
            .entry(handshake.from.clone())
            .or_default()
            .push(handshake.to.clone());
    }

    for station in station_names {
        let mut visiting = HashSet::new();
        let mut visited = HashSet::new();
        if station_wait_graph_has_cycle(station, &wait_edges, &mut visiting, &mut visited) {
            errors.push(PlcError::semantic_with_reason(
                1,
                format!("[SEM-213] handshake wait graph contains a cycle at station '{station}'."),
                "请打破 A 等 B、B 等 A 这类循环等待，或增加上层仲裁 station".to_string(),
            ));
            break;
        }
    }
}

fn station_wait_graph_has_cycle(
    station: &str,
    edges: &HashMap<String, Vec<String>>,
    visiting: &mut HashSet<String>,
    visited: &mut HashSet<String>,
) -> bool {
    if visited.contains(station) {
        return false;
    }
    if !visiting.insert(station.to_string()) {
        return true;
    }
    for next in edges.get(station).into_iter().flatten() {
        if station_wait_graph_has_cycle(next, edges, visiting, visited) {
            return true;
        }
    }
    visiting.remove(station);
    visited.insert(station.to_string());
    false
}

fn validate_transfer_points(
    program: &PlcProgram,
    station_names: &HashSet<String>,
    sites: &HashMap<&str, u32>,
    errors: &mut Vec<PlcError>,
) {
    let handshakes = program
        .topology
        .handshakes
        .iter()
        .map(|handshake| (handshake.name.as_str(), handshake))
        .collect::<HashMap<_, _>>();
    let mut names = HashSet::<String>::new();

    for transfer in &program.topology.transfer_points {
        if !names.insert(transfer.name.clone()) {
            errors.push(PlcError::duplicate_definition_with_reason(
                transfer.line.max(1),
                "transfer_point",
                &transfer.name,
                "transfer_point 名称必须唯一",
            ));
        }
        for (field, station) in [
            ("from_station", &transfer.from_station),
            ("to_station", &transfer.to_station),
        ] {
            if !station_names.contains(station) {
                errors.push(PlcError::undefined_reference_with_reason(
                    transfer.line.max(1),
                    "station",
                    station,
                    format!(
                        "[SEM-221] transfer_point '{}' 的 {field} 未定义",
                        transfer.name
                    ),
                ));
            }
        }
        match sites.get(transfer.site.as_str()) {
            Some(1) => {}
            Some(capacity) => errors.push(PlcError::semantic_with_reason(
                transfer.line.max(1),
                format!(
                    "[SEM-223] transfer_point '{}' site '{}' has capacity {}, expected 1.",
                    transfer.name, transfer.site, capacity
                ),
                "交接点应是单工件缓冲；请将 site capacity 设置为 1，或拆成多个 transfer_point"
                    .to_string(),
            )),
            None => errors.push(PlcError::undefined_reference_with_reason(
                transfer.line.max(1),
                "workpiece site",
                &transfer.site,
                format!(
                    "[SEM-222] transfer_point '{}' 引用了未定义 site",
                    transfer.name
                ),
            )),
        }
        let Some(handshake) = handshakes.get(transfer.handshake.as_str()) else {
            errors.push(PlcError::undefined_reference_with_reason(
                transfer.line.max(1),
                "handshake",
                &transfer.handshake,
                format!(
                    "[SEM-224] transfer_point '{}' 引用了未定义 handshake",
                    transfer.name
                ),
            ));
            continue;
        };
        if handshake.from != transfer.from_station || handshake.to != transfer.to_station {
            errors.push(PlcError::semantic_with_reason(
                transfer.line.max(1),
                format!(
                    "[SEM-225] transfer_point '{}' station pair does not match handshake '{}'.",
                    transfer.name, transfer.handshake
                ),
                format!(
                    "transfer_point 是 {} -> {}，但 handshake 是 {} -> {}",
                    transfer.from_station, transfer.to_station, handshake.from, handshake.to
                ),
            ));
        }
    }
}

fn validate_goto_target(
    line: usize,
    target: &GotoDirective,
    task_steps: &HashMap<String, HashSet<String>>,
    code: &str,
    errors: &mut Vec<PlcError>,
) {
    let Some(steps) = task_steps.get(&target.task) else {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            "task",
            &target.task,
            code.to_string(),
        ));
        return;
    };
    if let Some(step) = &target.step {
        if !steps.contains(step) {
            errors.push(PlcError::undefined_reference_with_reason(
                line,
                "step",
                &format!("{}.{}", target.task, step),
                code.to_string(),
            ));
        }
    }
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

fn source_topology_gates_required(program: &PlcProgram) -> bool {
    program.topology.devices.iter().any(|device| {
        matches!(device.device_type, DeviceType::Plc) && device.attributes.model_ref.is_some()
    })
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
    let controller_io_aliases = program
        .topology
        .controller_io
        .iter()
        .flat_map(|decl| {
            decl.aliases
                .iter()
                .map(move |alias| (decl.controller.as_str(), alias.alias.as_str()))
        })
        .collect::<HashSet<_>>();

    for task in &program.tasks.tasks {
        for step in &task.steps {
            validate_raw_io_bypass_in_statements(
                &step.statements,
                step.line.max(task.line).max(1),
                &device_types,
                &controller_io_aliases,
                errors,
            );
        }
    }
}

fn validate_raw_io_bypass_in_statements(
    statements: &[StepStatement],
    line: usize,
    device_types: &HashMap<&str, &DeviceType>,
    controller_io_aliases: &HashSet<(&str, &str)>,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::Set { target, .. })
            | StepStatement::Action(ActionStatement::SetAnalog { target, .. })
            | StepStatement::Action(ActionStatement::SetAnalogExpr { target, .. }) => {
                if let Some(message) =
                    raw_io_bypass_message(target, device_types, controller_io_aliases)
                {
                    errors.push(PlcError::semantic_with_reason(
                        line,
                        format!("[SEM-110] {message}"),
                        "use the high-level device action, or move this source into an explicit low-level fixture/mode instead of normal task logic",
                    ));
                }
            }
            StepStatement::Repeat { body, .. } => {
                validate_raw_io_bypass_in_statements(
                    body,
                    line,
                    device_types,
                    controller_io_aliases,
                    errors,
                );
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_raw_io_bypass_in_statements(
                        &branch.statements,
                        line,
                        device_types,
                        controller_io_aliases,
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
                        controller_io_aliases,
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
    controller_io_aliases: &HashSet<(&str, &str)>,
) -> Option<String> {
    if target.port != "self" && parse_physical_plc_port_ref(&target.port).is_some() {
        return Some(format!(
            "normal task action writes controller port `{target}` directly"
        ));
    }
    if controller_io_aliases.contains(&(target.device.as_str(), target.port.as_str())) {
        return Some(format!(
            "normal task action writes controller IO alias `{target}` directly"
        ));
    }

    let device_type = device_types.get(target.device.as_str())?;
    if let Some(message) =
        device_semantics::drive::raw_drive_provider_bypass_message(target, device_types)
    {
        return Some(message);
    }

    match device_type {
        DeviceType::StepperMotor | DeviceType::ServoDrive
            if matches!(target.port.as_str(), "enable" | "pulse" | "direction") =>
        {
            Some(format!(
                "normal task action writes low-level axis port `{target}` directly"
            ))
        }
        DeviceType::SolenoidValve if target.port.contains("coil") || target.port == "out" => Some(
            format!("normal task action writes valve port `{target}` directly"),
        ),
        DeviceType::Conveyor if matches!(target.port.as_str(), "drive" | "run" | "cmd") => Some(
            format!("normal task action writes conveyor drive port `{target}` directly"),
        ),
        DeviceType::Pump if matches!(target.port.as_str(), "drive" | "run" | "cmd") => Some(
            format!("normal task action writes pump drive port `{target}` directly"),
        ),
        DeviceType::Heater if matches!(target.port.as_str(), "power" | "cmd") => Some(format!(
            "normal task action writes heater power port `{target}` directly"
        )),
        DeviceType::VisionSensor if target.port == "trigger" => Some(format!(
            "normal task action writes vision trigger port `{target}` directly"
        )),
        DeviceType::ProportionalValve if matches!(target.port.as_str(), "cmd" | "opening") => {
            Some(format!(
                "normal task action writes proportional valve command port `{target}` directly"
            ))
        }
        DeviceType::Gripper if matches!(target.port.as_str(), "cmd" | "grip" | "release") => Some(
            format!("normal task action writes gripper command port `{target}` directly"),
        ),
        _ => None,
    }
}

include!("semantic_externs.rs");
include!("semantic_wait_regions.rs");
include!("semantic_topology_lowering.rs");
include!("semantic_workpiece_lowering.rs");
