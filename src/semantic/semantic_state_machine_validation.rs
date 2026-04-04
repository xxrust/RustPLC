#[derive(Debug, Clone, Default)]
struct AnalyzedStatements {
    actions: Vec<TransitionAction>,
    effects: Vec<IrWorkpieceEffect>,
    waits: Vec<String>,
    delays_ms: Vec<u64>,
    gotos: Vec<GotoDirective>,
    timeouts: Vec<TimeoutDirective>,
    if_elses: Vec<IfElseSpec>,
    parallel_blocks: Vec<ParallelBlock>,
    race_blocks: Vec<RaceBlock>,
}

#[derive(Debug, Clone)]
struct IfElseSpec {
    condition: ConditionExpression,
    then_goto: GotoDirective,
    else_goto: GotoDirective,
}

#[derive(Debug, Clone, Default)]
struct DeviceTimingProfile {
    response_ms: Option<u64>,
    stroke_ms: Option<u64>,
    retract_ms: Option<u64>,
    ramp_ms: Option<u64>,
}

fn collect_device_kinds(topology: &TopologySection) -> HashMap<String, DeviceKind> {
    topology
        .devices
        .iter()
        .map(|device| {
            (
                device.name.clone(),
                ast_type_to_ir_kind(&device.device_type),
            )
        })
        .collect()
}

fn collect_known_states(
    topology: &TopologySection,
    device_kinds: &HashMap<String, DeviceKind>,
) -> HashMap<String, HashSet<String>> {
    let mut known_states = HashMap::new();

    for device in &topology.devices {
        let Some(kind) = device_kinds.get(&device.name) else {
            continue;
        };

        let mut states = HashSet::new();
        for port in &device.attributes.ports {
            for state in &port.states {
                states.insert(state.clone());
            }
        }

        if let Some(custom_states) = &device.attributes.custom_states {
            if custom_states.len() > 8 {
                eprintln!(
                    "WARNING [semantic] 设备 {} 声明了 {} 个 states（> 8），请确认状态空间规模合理",
                    device.name,
                    custom_states.len()
                );
            }

            for state in custom_states {
                states.insert(state.clone());
            }
        } else {
            for state in default_states_for_kind(kind) {
                states.insert(state.to_string());
            }
        }

        known_states.insert(device.name.clone(), states);
    }

    for device in &topology.devices {
        if let Some(detects) = &device.attributes.detects {
            known_states
                .entry(detects.device.clone())
                .or_default()
                .insert(detects.state.clone());
        }
    }

    known_states
}

fn collect_task_steps(tasks: &TasksSection) -> HashMap<String, HashSet<String>> {
    let mut task_steps = HashMap::new();

    for task in &tasks.tasks {
        let steps = task
            .steps
            .iter()
            .map(|step| step.name.clone())
            .collect::<HashSet<_>>();
        task_steps.insert(task.name.clone(), steps);
    }

    task_steps
}

fn validate_state_reference(
    state: &crate::ast::StateReference,
    line: usize,
    source: &str,
    device_kinds: &HashMap<String, DeviceKind>,
    known_states: &HashMap<String, HashSet<String>>,
    errors: &mut Vec<PlcError>,
) {
    let Some(kind) = device_kinds.get(&state.device) else {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            "设备",
            &state.device,
            format!("{source} 使用前需要先在 [topology] 段定义设备"),
        ));
        return;
    };

    if *kind == DeviceKind::Motor && state.port == "self" {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "{source} 使用了已废弃的电机状态写法 {}.{}",
                state.device, state.state
            ),
            format!(
                "请改用显式端口状态，例如 {}.run.on/off 或 {}.direction.forward/reverse",
                state.device, state.device
            ),
        ));
        return;
    }

    if state.state.is_empty() {
        errors.push(PlcError::semantic(
            line,
            format!("{source} 设备 {} 缺少状态名", state.device),
        ));
        return;
    }

    let Some(allowed_states) = known_states.get(&state.device) else {
        return;
    };

    if !allowed_states.is_empty() && !allowed_states.contains(&state.state) {
        errors.push(PlcError::semantic(
            line,
            format!(
                "{source} 引用了设备 {} 的未定义状态 {}",
                state.device, state.state
            ),
        ));
    }
}

fn validate_device_reference(
    device_name: &str,
    line: usize,
    source: &str,
    device_kinds: &HashMap<String, DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    if !device_kinds.contains_key(device_name) {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            "设备",
            device_name,
            format!("{source} 约束引用前需要定义该设备"),
        ));
    }
}

fn validate_causality_node_reference(
    node_name: &str,
    line: usize,
    device_kinds: &HashMap<String, DeviceKind>,
    variable_names: &HashSet<String>,
    extern_function_names: &HashSet<String>,
    errors: &mut Vec<PlcError>,
) {
    if device_kinds.contains_key(node_name)
        || variable_names.contains(node_name)
        || extern_function_names.contains(node_name)
    {
        return;
    }

    errors.push(PlcError::undefined_reference_with_reason(
        line,
        "因果节点",
        node_name,
        "causality 链路节点需要先定义为设备、[topology] variable 或 extern function".to_string(),
    ));
}

fn validate_timing_target(
    target: &TimingTarget,
    line: usize,
    task_steps: &HashMap<String, HashSet<String>>,
    errors: &mut Vec<PlcError>,
) {
    match target {
        TimingTarget::Task { task } => {
            if !task_steps.contains_key(task) {
                errors.push(PlcError::undefined_reference_with_reason(
                    line,
                    " task",
                    task,
                    "请先在 [tasks] 段定义该 task".to_string(),
                ));
            }
        }
        TimingTarget::Step { task, step } => {
            let Some(steps) = task_steps.get(task) else {
                errors.push(PlcError::undefined_reference_with_reason(
                    line,
                    " task",
                    task,
                    "请先在 [tasks] 段定义该 task".to_string(),
                ));
                return;
            };

            if !steps.contains(step) {
                errors.push(PlcError::semantic(
                    line,
                    format!("timing 约束引用了未定义 step {task}.{step}"),
                ));
            }
        }
    }
}

fn collect_device_ranges(topology: &TopologySection) -> HashMap<String, (f64, f64)> {
    topology
        .devices
        .iter()
        .filter_map(|device| {
            device.attributes.range.as_ref().map(|r| {
                let (min, max) = if r.min <= r.max {
                    (r.min, r.max)
                } else {
                    (r.max, r.min)
                };
                (device.name.clone(), (min, max))
            })
        })
        .collect()
}

fn collect_device_port_types(
    topology: &TopologySection,
    device_kinds: &HashMap<String, DeviceKind>,
) -> HashMap<String, PortType> {
    let mut out = HashMap::new();

    for device in &topology.devices {
        for port in &device.attributes.ports {
            out.insert(
                format!("{}.{}", device.name, port.id),
                port.port_type.clone(),
            );
        }

        if let Some(kind) = device_kinds.get(&device.name) {
            for port in default_analog_ports_for_kind(kind) {
                out.entry(format!("{}.{}", device.name, port))
                    .or_insert(PortType::Analog);
            }
        }
    }

    out
}

fn collect_device_units(topology: &TopologySection) -> HashMap<String, String> {
    topology
        .devices
        .iter()
        .filter_map(|device| {
            device
                .attributes
                .unit
                .as_ref()
                .map(|unit| (device.name.clone(), unit.clone()))
        })
        .collect()
}

fn default_analog_ports_for_kind(kind: &DeviceKind) -> &'static [&'static str] {
    match kind {
        DeviceKind::CamCoupling => &["following_error", "master_pos", "slave_cmd"],
        DeviceKind::AnalogInput => &["in"],
        DeviceKind::AnalogOutput => &["out"],
        DeviceKind::Pid => &["in", "out"],
        _ => &[],
    }
}

fn validate_analog_actions_in_statements(
    statements: &[StepStatement],
    line: usize,
    device_kinds: &HashMap<String, DeviceKind>,
    device_ranges: &HashMap<String, (f64, f64)>,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::SetAnalog { target, value }) => {
                if let Some(kind) = device_kinds.get(&target.device) {
                    if *kind != DeviceKind::AnalogOutput
                        && *kind != DeviceKind::Motor
                        && *kind != DeviceKind::Vfd
                    {
                        errors.push(PlcError::type_mismatch_with_reason(
                            line,
                            "analog_output / motor / vfd",
                            device_kind_name(kind),
                            format!("set_analog {target}"),
                            "set_analog 只能用于 analog_output、motor 或 vfd 类型设备",
                        ));
                    }
                }
                if let Some((min, max)) = device_ranges.get(&target.device) {
                    if *value < *min || *value > *max {
                        errors.push(PlcError::semantic_with_reason(
                            line,
                            format!("set_analog {target} {value} 超出声明范围 {min}..{max}",),
                            "请确保 set_analog 值在设备声明的 range 范围内",
                        ));
                    }
                }
            }
            StepStatement::Action(ActionStatement::SetAnalogExpr { target, .. }) => {
                if let Some(kind) = device_kinds.get(&target.device) {
                    if *kind != DeviceKind::AnalogOutput
                        && *kind != DeviceKind::Motor
                        && *kind != DeviceKind::Vfd
                    {
                        errors.push(PlcError::type_mismatch_with_reason(
                            line,
                            "analog_output / motor / vfd",
                            device_kind_name(kind),
                            format!("set_analog {target}"),
                            "set_analog 只能用于 analog_output、motor 或 vfd 类型设备",
                        ));
                    }
                }
            }
            StepStatement::Action(ActionStatement::Set { target, .. }) => {
                if let Some(kind) = device_kinds.get(&target.device) {
                    if *kind == DeviceKind::AnalogOutput || *kind == DeviceKind::AnalogInput {
                        errors.push(PlcError::type_mismatch_with_reason(
                            line,
                            "digital_output 或 solenoid_valve 等离散设备",
                            device_kind_name(kind),
                            format!("set {target} on/off"),
                            "模拟量设备请使用 set_analog 指令",
                        ));
                    }
                }
            }
            StepStatement::Repeat { body, .. } => {
                validate_analog_actions_in_statements(
                    body,
                    line,
                    device_kinds,
                    device_ranges,
                    errors,
                );
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_analog_actions_in_statements(
                        &branch.statements,
                        line,
                        device_kinds,
                        device_ranges,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_analog_actions_in_statements(
                        &branch.statements,
                        line,
                        device_kinds,
                        device_ranges,
                        errors,
                    );
                }
            }
            _ => {}
        }
    }
}

fn validate_set_enum_values(statements: &[StepStatement], line: usize, errors: &mut Vec<PlcError>) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::Set { target, value }) => {
                if set_enum_to_binary(value).is_none() {
                    errors.push(PlcError::semantic_with_reason(
                        line,
                        format!("set {target} {value} 使用了不支持的状态值"),
                        "set 状态值仅支持 on/off/forward/reverse/active/idle".to_string(),
                    ));
                }
            }
            StepStatement::Repeat { body, .. } => validate_set_enum_values(body, line, errors),
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_set_enum_values(&branch.statements, line, errors);
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_set_enum_values(&branch.statements, line, errors);
                }
            }
            StepStatement::Action(_)
            | StepStatement::Wait(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_)
            | StepStatement::Effect(_) => {}
        }
    }
}

fn validate_expression_actions_in_tasks(
    tasks: &TasksSection,
    variable_types: &HashMap<String, AstVariableType>,
    errors: &mut Vec<PlcError>,
) {
    for task in &tasks.tasks {
        for step in &task.steps {
            validate_expression_actions_in_statements(
                &step.statements,
                step.line.max(1),
                variable_types,
                errors,
            );
        }
    }
}

fn validate_extern_calls_in_tasks(
    tasks: &TasksSection,
    extern_signatures: &HashMap<String, ExternFunctionSignature>,
    variable_types: &HashMap<String, AstVariableType>,
    errors: &mut Vec<PlcError>,
) {
    for task in &tasks.tasks {
        for step in &task.steps {
            validate_extern_calls_in_statements(
                &step.statements,
                step.line.max(1),
                extern_signatures,
                variable_types,
                errors,
            );
        }
    }
}

fn validate_non_pure_extern_concurrency_in_tasks(
    tasks: &TasksSection,
    extern_signatures: &HashMap<String, ExternFunctionSignature>,
    errors: &mut Vec<PlcError>,
) {
    for task in &tasks.tasks {
        for step in &task.steps {
            validate_non_pure_extern_concurrency_in_statements(
                &step.statements,
                step.line.max(1),
                extern_signatures,
                errors,
            );
        }
    }
}

fn validate_non_pure_extern_concurrency_in_statements(
    statements: &[StepStatement],
    line: usize,
    extern_signatures: &HashMap<String, ExternFunctionSignature>,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Parallel(block) => {
                let branch_statements = block
                    .branches
                    .iter()
                    .map(|branch| branch.statements.as_slice())
                    .collect::<Vec<_>>();
                validate_non_pure_extern_concurrency_in_branches(
                    &branch_statements,
                    "parallel",
                    line,
                    extern_signatures,
                    errors,
                );

                for branch in &block.branches {
                    validate_non_pure_extern_concurrency_in_statements(
                        &branch.statements,
                        line,
                        extern_signatures,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                let branch_statements = block
                    .branches
                    .iter()
                    .map(|branch| branch.statements.as_slice())
                    .collect::<Vec<_>>();
                validate_non_pure_extern_concurrency_in_branches(
                    &branch_statements,
                    "race",
                    line,
                    extern_signatures,
                    errors,
                );

                for branch in &block.branches {
                    validate_non_pure_extern_concurrency_in_statements(
                        &branch.statements,
                        line,
                        extern_signatures,
                        errors,
                    );
                }
            }
            StepStatement::Repeat { body, .. } => {
                validate_non_pure_extern_concurrency_in_statements(
                    body,
                    line,
                    extern_signatures,
                    errors,
                );
            }
            StepStatement::Action(_)
            | StepStatement::Wait(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_)
            | StepStatement::Effect(_) => {}
        }
    }
}

fn validate_non_pure_extern_concurrency_in_branches(
    branches: &[&[StepStatement]],
    block_kind: &str,
    line: usize,
    extern_signatures: &HashMap<String, ExternFunctionSignature>,
    errors: &mut Vec<PlcError>,
) {
    let mut first_seen_by_function: HashMap<String, usize> = HashMap::new();

    for (branch_index, statements) in branches.iter().enumerate() {
        let mut calls = HashSet::new();
        collect_non_pure_extern_calls(statements, extern_signatures, &mut calls);
        for function in calls {
            if let Some(first_branch) = first_seen_by_function.get(&function).copied() {
                errors.push(PlcError::semantic_with_reason(
                    line,
                    format!(
                        "non-pure extern 函数 {function} 在 {block_kind} 分支 #{} 与 #{} 中并发调用",
                        first_branch + 1,
                        branch_index + 1
                    ),
                    "请将 pure: false 的 extern 调用改为串行执行，避免在 parallel/race 多分支中重复调用同一函数",
                ));
            } else {
                first_seen_by_function.insert(function, branch_index);
            }
        }
    }
}

fn collect_non_pure_extern_calls(
    statements: &[StepStatement],
    extern_signatures: &HashMap<String, ExternFunctionSignature>,
    out: &mut HashSet<String>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::Call { function, .. }) => {
                if extern_signatures
                    .get(function)
                    .map(|signature| !signature.pure)
                    .unwrap_or(false)
                {
                    out.insert(function.clone());
                }
            }
            StepStatement::Repeat { body, .. } => {
                collect_non_pure_extern_calls(body, extern_signatures, out);
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    collect_non_pure_extern_calls(&branch.statements, extern_signatures, out);
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    collect_non_pure_extern_calls(&branch.statements, extern_signatures, out);
                }
            }
            StepStatement::Action(_)
            | StepStatement::Wait(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_)
            | StepStatement::Effect(_) => {}
        }
    }
}

fn validate_extern_calls_in_statements(
    statements: &[StepStatement],
    line: usize,
    extern_signatures: &HashMap<String, ExternFunctionSignature>,
    variable_types: &HashMap<String, AstVariableType>,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::Call {
                function,
                args,
                binding,
            }) => {
                validate_extern_call_signature(
                    function,
                    args,
                    binding,
                    line,
                    extern_signatures,
                    variable_types,
                    errors,
                );
            }
            StepStatement::Repeat { body, .. } => validate_extern_calls_in_statements(
                body,
                line,
                extern_signatures,
                variable_types,
                errors,
            ),
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_extern_calls_in_statements(
                        &branch.statements,
                        line,
                        extern_signatures,
                        variable_types,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_extern_calls_in_statements(
                        &branch.statements,
                        line,
                        extern_signatures,
                        variable_types,
                        errors,
                    );
                }
            }
            StepStatement::Action(_)
            | StepStatement::Wait(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_)
            | StepStatement::Effect(_) => {}
        }
    }
}

fn validate_extern_call_signature(
    function: &str,
    args: &[AstExpression],
    binding: &AstExternCallBinding,
    line: usize,
    extern_signatures: &HashMap<String, ExternFunctionSignature>,
    variable_types: &HashMap<String, AstVariableType>,
    errors: &mut Vec<PlcError>,
) {
    let Some(signature) = extern_signatures.get(function) else {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            "extern 函数",
            function,
            format!("action: call {function}(...) 调用前需要先在 [topology] 中声明"),
        ));
        return;
    };

    if args.len() != signature.param_types.len() {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "extern 函数 {function} 参数个数错误：期望 {} 个，实际 {} 个",
                signature.param_types.len(),
                args.len()
            ),
            "请检查 action: call 参数列表与 extern function 声明是否一致".to_string(),
        ));
    }

    for (index, (arg, expected_type)) in args.iter().zip(&signature.param_types).enumerate() {
        let Some(actual_type) = infer_expression_type(arg, variable_types) else {
            continue;
        };
        if actual_type != *expected_type {
            errors.push(PlcError::type_mismatch_with_reason(
                line,
                ast_variable_type_name(expected_type),
                ast_variable_type_name(&actual_type),
                format!("extern 调用 {function} 参数 #{}", index + 1),
                "请将实参与 extern function 声明的参数类型保持一致",
            ));
        }
    }

    let binding_targets: &[String] = match binding {
        AstExternCallBinding::Single(name) => std::slice::from_ref(name),
        AstExternCallBinding::Tuple(names) => names.as_slice(),
    };

    let expected_return_count = signature.return_types.len();
    if binding_targets.len() != expected_return_count {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "extern 函数 {function} 返回值绑定数量错误：期望 {expected_return_count} 个，实际 {} 个",
                binding_targets.len()
            ),
            "请让 -> 绑定变量数量与 extern function 返回类型数量保持一致".to_string(),
        ));
        return;
    }

    for (index, (target, expected_type)) in binding_targets
        .iter()
        .zip(&signature.return_types)
        .enumerate()
    {
        let Some(actual_type) = variable_types.get(target) else {
            errors.push(PlcError::undefined_reference_with_reason(
                line,
                "变量",
                target,
                format!("extern 函数 {function} 返回值绑定目标必须先在 [topology] 中声明"),
            ));
            continue;
        };

        if actual_type != expected_type {
            errors.push(PlcError::type_mismatch_with_reason(
                line,
                ast_variable_type_name(expected_type),
                ast_variable_type_name(actual_type),
                format!("extern 调用 {function} 返回绑定 #{} ({target})", index + 1),
                "请将绑定变量类型与 extern function 返回类型保持一致",
            ));
        }
    }
}

fn infer_expression_type(
    expr: &AstExpression,
    variable_types: &HashMap<String, AstVariableType>,
) -> Option<AstVariableType> {
    match expr {
        AstExpression::Literal(_) => Some(AstVariableType::Float),
        AstExpression::Boolean(_) => Some(AstVariableType::Bool),
        AstExpression::Variable(name) => variable_types.get(name).cloned(),
        AstExpression::UnaryNeg(inner) => match infer_expression_type(inner, variable_types)? {
            AstVariableType::Bool => None,
            AstVariableType::Int => Some(AstVariableType::Int),
            AstVariableType::Float => Some(AstVariableType::Float),
        },
        AstExpression::UnaryNot(inner) => match infer_expression_type(inner, variable_types)? {
            AstVariableType::Bool => Some(AstVariableType::Bool),
            _ => None,
        },
        AstExpression::BinaryOp { op, left, right } => {
            let left_type = infer_expression_type(left, variable_types)?;
            let right_type = infer_expression_type(right, variable_types)?;
            match op {
                AstBinaryOperator::Add
                | AstBinaryOperator::Sub
                | AstBinaryOperator::Mul
                | AstBinaryOperator::Div
                | AstBinaryOperator::Mod => match (left_type, right_type) {
                    (AstVariableType::Bool, _) | (_, AstVariableType::Bool) => None,
                    (AstVariableType::Float, _) | (_, AstVariableType::Float) => {
                        Some(AstVariableType::Float)
                    }
                    (AstVariableType::Int, AstVariableType::Int) => Some(AstVariableType::Int),
                },
                AstBinaryOperator::Eq | AstBinaryOperator::Neq => match (left_type, right_type) {
                    (AstVariableType::Bool, AstVariableType::Bool) => Some(AstVariableType::Bool),
                    (AstVariableType::Bool, _) | (_, AstVariableType::Bool) => None,
                    _ => Some(AstVariableType::Bool),
                },
                AstBinaryOperator::Gt
                | AstBinaryOperator::Lt
                | AstBinaryOperator::Gte
                | AstBinaryOperator::Lte => match (left_type, right_type) {
                    (AstVariableType::Bool, _) | (_, AstVariableType::Bool) => None,
                    _ => Some(AstVariableType::Bool),
                },
                AstBinaryOperator::And | AstBinaryOperator::Or => {
                    if left_type == AstVariableType::Bool && right_type == AstVariableType::Bool {
                        Some(AstVariableType::Bool)
                    } else {
                        None
                    }
                }
            }
        }
        AstExpression::FunctionCall { .. } => Some(AstVariableType::Float),
    }
}

fn validate_expression_actions_in_statements(
    statements: &[StepStatement],
    line: usize,
    variable_types: &HashMap<String, AstVariableType>,
    errors: &mut Vec<PlcError>,
) {
    let variables = variable_types.keys().cloned().collect::<HashSet<_>>();
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::Compute { target, expr }) => {
                if !variables.contains(target) {
                    errors.push(PlcError::undefined_reference_with_reason(
                        line,
                        "变量",
                        target,
                        "compute 目标变量必须先在 [topology] 中使用 variable 声明".to_string(),
                    ));
                }
                validate_expression_variables(expr, line, &variables, errors);
                if let Some(target_type) = variable_types.get(target) {
                    match infer_expression_type(expr, variable_types) {
                        Some(actual_type)
                            if !expression_type_assignable_to(&actual_type, target_type) =>
                        {
                            errors.push(PlcError::type_mismatch_with_reason(
                                line,
                                ast_variable_type_name(target_type),
                                ast_variable_type_name(&actual_type),
                                format!("compute {target}"),
                                "compute 表达式类型必须与目标变量类型一致".to_string(),
                            ));
                        }
                        None => errors.push(PlcError::semantic_with_reason(
                            line,
                            format!("compute {target} 表达式类型不合法"),
                            "请检查布尔/比较/算术表达式是否符合类型规则".to_string(),
                        )),
                        _ => {}
                    }
                }
            }
            StepStatement::Action(ActionStatement::SetAnalogExpr { expr, .. }) => {
                validate_expression_variables(expr, line, &variables, errors);
                match infer_expression_type(expr, variable_types) {
                    Some(AstVariableType::Bool) => {
                        errors.push(PlcError::type_mismatch_with_reason(
                            line,
                            "float/int",
                            "bool",
                            "set_analog expression".to_string(),
                            "set_analog 表达式必须是数值类型".to_string(),
                        ))
                    }
                    None => errors.push(PlcError::semantic_with_reason(
                        line,
                        "set_analog 表达式类型不合法".to_string(),
                        "请检查布尔/比较/算术表达式是否符合类型规则".to_string(),
                    )),
                    _ => {}
                }
            }
            StepStatement::Action(ActionStatement::Call { args, .. }) => {
                for arg in args {
                    validate_expression_variables(arg, line, &variables, errors);
                }
            }
            StepStatement::Action(ActionStatement::CamPhase { offset, .. }) => {
                validate_expression_variables(offset, line, &variables, errors);
                match infer_expression_type(offset, variable_types) {
                    Some(AstVariableType::Bool) => {
                        errors.push(PlcError::type_mismatch_with_reason(
                            line,
                            "float/int",
                            "bool",
                            "cam_phase offset".to_string(),
                            "cam_phase 偏移表达式必须是数值类型".to_string(),
                        ))
                    }
                    None => errors.push(PlcError::semantic_with_reason(
                        line,
                        "cam_phase 偏移表达式类型不合法".to_string(),
                        "请检查布尔/比较/算术表达式是否符合类型规则".to_string(),
                    )),
                    _ => {}
                }
            }
            StepStatement::Wait(wait) => {
                for condition in wait_condition_terms(&wait.condition) {
                    if let Some((left, right)) = condition.expression_pair() {
                        validate_expression_variables(left, line, &variables, errors);
                        validate_expression_variables(right, line, &variables, errors);
                    }
                }
            }
            StepStatement::IfElse { condition, .. } => {
                if let Some((left, right)) = condition.expression_pair() {
                    validate_expression_variables(left, line, &variables, errors);
                    validate_expression_variables(right, line, &variables, errors);
                }
            }
            StepStatement::Repeat { body, .. } => {
                validate_expression_actions_in_statements(body, line, variable_types, errors);
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_expression_actions_in_statements(
                        &branch.statements,
                        line,
                        variable_types,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_expression_actions_in_statements(
                        &branch.statements,
                        line,
                        variable_types,
                        errors,
                    );
                }
            }
            StepStatement::Action(_)
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_)
            | StepStatement::Effect(_) => {}
        }
    }
}

fn validate_cam_actions_in_tasks(
    tasks: &TasksSection,
    device_kinds: &HashMap<String, DeviceKind>,
    cam_table_names: &HashSet<String>,
    errors: &mut Vec<PlcError>,
) {
    for task in &tasks.tasks {
        for step in &task.steps {
            validate_cam_actions_in_statements(
                &step.statements,
                step.line.max(1),
                device_kinds,
                cam_table_names,
                errors,
            );
        }
    }
}

fn validate_cam_actions_in_statements(
    statements: &[StepStatement],
    line: usize,
    device_kinds: &HashMap<String, DeviceKind>,
    cam_table_names: &HashSet<String>,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::CamEngage { target })
            | StepStatement::Action(ActionStatement::CamDisengage { target })
            | StepStatement::Action(ActionStatement::CamPhase { target, .. }) => {
                match device_kinds.get(target) {
                    Some(DeviceKind::CamCoupling) => {}
                    Some(kind) => errors.push(PlcError::type_mismatch_with_reason(
                        line,
                        "cam_coupling",
                        device_kind_name(kind),
                        format!("cam action {target}"),
                        "cam 动作仅支持作用于 cam_coupling 设备",
                    )),
                    None => errors.push(PlcError::undefined_reference_with_reason(
                        line,
                        "设备",
                        target,
                        "cam 动作引用前需要在 [topology] 中定义 cam_coupling 设备".to_string(),
                    )),
                }
            }
            StepStatement::Action(ActionStatement::CamSwitch { target, new_table }) => {
                match device_kinds.get(target) {
                    Some(DeviceKind::CamCoupling) => {}
                    Some(kind) => errors.push(PlcError::type_mismatch_with_reason(
                        line,
                        "cam_coupling",
                        device_kind_name(kind),
                        format!("cam_switch {target}"),
                        "cam_switch 仅支持作用于 cam_coupling 设备",
                    )),
                    None => errors.push(PlcError::undefined_reference_with_reason(
                        line,
                        "设备",
                        target,
                        "cam_switch 引用前需要定义 cam_coupling 设备".to_string(),
                    )),
                }
                if !cam_table_names.contains(new_table) {
                    errors.push(PlcError::undefined_reference_with_reason(
                        line,
                        "cam_table",
                        new_table,
                        "cam_switch 的目标表需要先在 [topology] 中声明".to_string(),
                    ));
                }
            }
            StepStatement::Repeat { body, .. } => validate_cam_actions_in_statements(
                body,
                line,
                device_kinds,
                cam_table_names,
                errors,
            ),
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_cam_actions_in_statements(
                        &branch.statements,
                        line,
                        device_kinds,
                        cam_table_names,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_cam_actions_in_statements(
                        &branch.statements,
                        line,
                        device_kinds,
                        cam_table_names,
                        errors,
                    );
                }
            }
            StepStatement::Action(_)
            | StepStatement::Wait(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_)
            | StepStatement::Effect(_) => {}
        }
    }
}

fn validate_axis_motion_actions_in_tasks(
    tasks: &TasksSection,
    device_kinds: &HashMap<String, DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    for task in &tasks.tasks {
        for step in &task.steps {
            validate_axis_motion_actions_in_statements(
                &step.statements,
                &step.name,
                step.line.max(1),
                device_kinds,
                errors,
            );
        }
    }
}

fn validate_axis_motion_actions_in_statements(
    statements: &[StepStatement],
    step_name: &str,
    line: usize,
    device_kinds: &HashMap<String, DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::AxisMoveRelative {
                target,
                timeout,
                on_reject,
                on_motion_fault,
                on_safety_fault,
                on_reject_routes,
                on_motion_fault_routes,
                on_safety_fault_routes,
                ..
            })
            | StepStatement::Action(ActionStatement::AxisMoveAbsolute {
                target,
                timeout,
                on_reject,
                on_motion_fault,
                on_safety_fault,
                on_reject_routes,
                on_motion_fault_routes,
                on_safety_fault_routes,
                ..
            }) => {
                if timeout.is_none() {
                    errors.push(axis_motion_branch_error(
                        line,
                        "AXIS-001",
                        step_name,
                        "timeout",
                        "添加 timeout: <duration> -> <task.step> 分支。",
                    ));
                }
                if on_reject.is_none() {
                    errors.push(axis_motion_branch_error(
                        line,
                        "AXIS-002",
                        step_name,
                        "on_reject",
                        "添加 on_reject -> <task.step> 分支。",
                    ));
                }
                if on_motion_fault.is_none() {
                    errors.push(axis_motion_branch_error(
                        line,
                        "AXIS-003",
                        step_name,
                        "on_motion_fault",
                        "添加 on_motion_fault -> <task.step> 分支。",
                    ));
                }
                if on_safety_fault.is_none() {
                    errors.push(axis_motion_branch_error(
                        line,
                        "AXIS-004",
                        step_name,
                        "on_safety_fault",
                        "添加 on_safety_fault -> <task.step> 分支。",
                    ));
                }
                validate_axis_fault_routes(
                    line,
                    step_name,
                    "on_reject",
                    on_reject_routes,
                    &[AstAxisFaultRouteKind::Reject, AstAxisFaultRouteKind::Vendor],
                    errors,
                );
                validate_axis_fault_routes(
                    line,
                    step_name,
                    "on_motion_fault",
                    on_motion_fault_routes,
                    &[AstAxisFaultRouteKind::Motion, AstAxisFaultRouteKind::Vendor],
                    errors,
                );
                validate_axis_fault_routes(
                    line,
                    step_name,
                    "on_safety_fault",
                    on_safety_fault_routes,
                    &[AstAxisFaultRouteKind::Safety, AstAxisFaultRouteKind::Vendor],
                    errors,
                );
                validate_axis_motion_target_kind(
                    line,
                    step_name,
                    &target.device,
                    device_kinds,
                    errors,
                );
            }
            StepStatement::Repeat { body, .. } => validate_axis_motion_actions_in_statements(
                body,
                step_name,
                line,
                device_kinds,
                errors,
            ),
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_axis_motion_actions_in_statements(
                        &branch.statements,
                        step_name,
                        line,
                        device_kinds,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_axis_motion_actions_in_statements(
                        &branch.statements,
                        step_name,
                        line,
                        device_kinds,
                        errors,
                    );
                }
            }
            StepStatement::Action(_)
            | StepStatement::Wait(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_)
            | StepStatement::Effect(_) => {}
        }
    }
}

fn axis_motion_branch_error(
    line: usize,
    rule_id: &str,
    step_name: &str,
    branch_name: &str,
    fix: &str,
) -> PlcError {
    PlcError::semantic_with_reason(
        line,
        format!("[{rule_id}] step '{step_name}' is missing {branch_name} branch."),
        fix,
    )
}

fn validate_axis_fault_routes(
    line: usize,
    step_name: &str,
    branch_name: &str,
    routes: &[AstAxisFaultRouteDirective],
    allowed_kinds: &[AstAxisFaultRouteKind],
    errors: &mut Vec<PlcError>,
) {
    for route in routes {
        if let Some(kind) = route.kind {
            if !allowed_kinds.contains(&kind) {
                errors.push(PlcError::semantic_with_reason(
                    line,
                    format!(
                        "[AXIS-010] step '{step_name}' has incompatible {branch_name} route kind '{kind:?}'."
                    ),
                    format!(
                        "{branch_name} 仅允许 kind 为 {}，请调整 matcher。",
                        allowed_kinds
                            .iter()
                            .map(|v| format!("{:?}", v).to_lowercase())
                            .collect::<Vec<_>>()
                            .join("/")
                    ),
                ));
            }
        }
    }
}

fn validate_axis_motion_target_kind(
    line: usize,
    step_name: &str,
    target: &str,
    device_kinds: &HashMap<String, DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    match device_kinds.get(target) {
        Some(DeviceKind::StepperMotor) | Some(DeviceKind::ServoDrive) => {}
        Some(kind) => errors.push(PlcError::semantic_with_reason(
            line,
            format!("[AXIS-005] axis target '{target}' must be stepper_motor or servo_drive."),
            format!(
                "step '{step_name}' 当前目标类型为 {}。请改用 stepper_motor/servo_drive 设备。",
                device_kind_name(kind)
            ),
        )),
        None => errors.push(PlcError::semantic_with_reason(
            line,
            format!("[AXIS-005] axis target '{target}' must be stepper_motor or servo_drive."),
            format!(
                "step '{step_name}' 引用了未定义设备。请先在 [topology] 声明该轴设备，且类型为 stepper_motor 或 servo_drive。"
            ),
        )),
    }
}

const AXIS_MOTION_PARAM_SETS_DIR: &str = "axis_motion_param_sets";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AxisMotionParamSetDef {
    name: String,
    config_id: String,
    speed: f64,
    acceleration: f64,
    deceleration: f64,
}

fn resolve_axis_motion_parameters_in_tasks(
    tasks: &mut TasksSection,
    topology: &TopologySection,
    errors: &mut Vec<PlcError>,
) {
    if !tasks_contain_axis_motion_actions(tasks) {
        return;
    }

    let axis_profiles = match resolve_axis_profiles(&topology.devices) {
        Ok(profiles) => profiles,
        Err(mut profile_errors) => {
            errors.append(&mut profile_errors);
            return;
        }
    };

    let motion_param_sets = match load_axis_motion_param_sets() {
        Ok(sets) => sets,
        Err(mut load_errors) => {
            errors.append(&mut load_errors);
            return;
        }
    };

    let mut device_default_param_sets = HashMap::<String, String>::new();
    for device in &topology.devices {
        if !matches!(
            device.device_type,
            DeviceType::StepperMotor | DeviceType::ServoDrive
        ) {
            continue;
        }
        if let Some(default_set) = device
            .attributes
            .motion_param_set
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            device_default_param_sets.insert(device.name.clone(), default_set.to_string());
        }
    }

    for task in &mut tasks.tasks {
        for step in &mut task.steps {
            resolve_axis_motion_parameters_in_statements(
                &mut step.statements,
                step.line.max(1),
                &axis_profiles,
                &motion_param_sets,
                &device_default_param_sets,
                errors,
            );
        }
    }
}

fn tasks_contain_axis_motion_actions(tasks: &TasksSection) -> bool {
    tasks
        .tasks
        .iter()
        .flat_map(|task| task.steps.iter())
        .any(|step| statements_contain_axis_motion_actions(&step.statements))
}

fn statements_contain_axis_motion_actions(statements: &[StepStatement]) -> bool {
    statements.iter().any(|statement| match statement {
        StepStatement::Action(ActionStatement::AxisMoveRelative { .. })
        | StepStatement::Action(ActionStatement::AxisMoveAbsolute { .. }) => true,
        StepStatement::Repeat { body, .. } => statements_contain_axis_motion_actions(body),
        StepStatement::Parallel(block) => block
            .branches
            .iter()
            .any(|branch| statements_contain_axis_motion_actions(&branch.statements)),
        StepStatement::Race(block) => block
            .branches
            .iter()
            .any(|branch| statements_contain_axis_motion_actions(&branch.statements)),
        StepStatement::Action(_)
        | StepStatement::Wait(_)
        | StepStatement::IfElse { .. }
        | StepStatement::Delay { .. }
        | StepStatement::Timeout(_)
        | StepStatement::Goto(_)
        | StepStatement::AllowIndefiniteWait(_)
        | StepStatement::Effect(_) => false,
    })
}

fn resolve_axis_motion_parameters_in_statements(
    statements: &mut [StepStatement],
    line: usize,
    axis_profiles: &BTreeMap<String, crate::ir::AxisProfile>,
    motion_param_sets: &HashMap<String, AxisMotionParamSetDef>,
    device_default_param_sets: &HashMap<String, String>,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::AxisMoveRelative {
                target,
                params,
                speed,
                acceleration,
                deceleration,
                ..
            }) => resolve_axis_motion_parameters_on_action(
                line,
                None,
                &target.device,
                params,
                speed,
                acceleration,
                deceleration,
                axis_profiles,
                motion_param_sets,
                device_default_param_sets,
                errors,
            ),
            StepStatement::Action(ActionStatement::AxisMoveAbsolute {
                target,
                params,
                position,
                speed,
                acceleration,
                deceleration,
                ..
            }) => resolve_axis_motion_parameters_on_action(
                line,
                Some(*position),
                &target.device,
                params,
                speed,
                acceleration,
                deceleration,
                axis_profiles,
                motion_param_sets,
                device_default_param_sets,
                errors,
            ),
            StepStatement::Repeat { body, .. } => resolve_axis_motion_parameters_in_statements(
                body,
                line,
                axis_profiles,
                motion_param_sets,
                device_default_param_sets,
                errors,
            ),
            StepStatement::Parallel(block) => {
                for branch in &mut block.branches {
                    resolve_axis_motion_parameters_in_statements(
                        &mut branch.statements,
                        line,
                        axis_profiles,
                        motion_param_sets,
                        device_default_param_sets,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &mut block.branches {
                    resolve_axis_motion_parameters_in_statements(
                        &mut branch.statements,
                        line,
                        axis_profiles,
                        motion_param_sets,
                        device_default_param_sets,
                        errors,
                    );
                }
            }
            StepStatement::Action(_)
            | StepStatement::Wait(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_)
            | StepStatement::Effect(_) => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_axis_motion_parameters_on_action(
    line: usize,
    absolute_position: Option<f64>,
    target_device: &str,
    params: &Option<String>,
    speed: &mut Option<f64>,
    acceleration: &mut Option<f64>,
    deceleration: &mut Option<f64>,
    axis_profiles: &BTreeMap<String, crate::ir::AxisProfile>,
    motion_param_sets: &HashMap<String, AxisMotionParamSetDef>,
    device_default_param_sets: &HashMap<String, String>,
    errors: &mut Vec<PlcError>,
) {
    let Some(profile) = axis_profiles.get(target_device) else {
        return;
    };

    let explicit_params = params
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string);
    let selected_params_name = explicit_params
        .clone()
        .or_else(|| device_default_param_sets.get(target_device).cloned());

    let selected_param_set = match selected_params_name.as_deref() {
        Some(name) => {
            let Some(def) = motion_param_sets.get(name) else {
                errors.push(PlcError::semantic_with_reason(
                    line,
                    format!(
                        "[AXIS-006] axis target '{}' references unknown motion params '{}'.",
                        target_device, name
                    ),
                    format!(
                        "请在 {AXIS_MOTION_PARAM_SETS_DIR}/{}.toml 中定义该参数集，或修正 params。",
                        name
                    ),
                ));
                return;
            };
            if def.config_id.trim() != profile.config_ref {
                errors.push(PlcError::semantic_with_reason(
                    line,
                    format!(
                        "[AXIS-006] motion params '{}' is bound to config '{}' but '{}' uses config '{}'.",
                        name, def.config_id, target_device, profile.config_ref
                    ),
                    "请确保参数集 config_id 与目标轴设备 config_ref 一致。".to_string(),
                ));
                return;
            }
            Some(def)
        }
        None => None,
    };

    let resolved_speed = speed
        .as_ref()
        .copied()
        .or_else(|| selected_param_set.map(|def| def.speed));
    let resolved_acc = acceleration
        .as_ref()
        .copied()
        .or_else(|| selected_param_set.map(|def| def.acceleration));
    let resolved_dec = deceleration
        .as_ref()
        .copied()
        .or_else(|| selected_param_set.map(|def| def.deceleration));

    let Some(resolved_speed) = resolved_speed else {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "[AXIS-007] axis.move on '{}' is missing speed/acc/dec parameters.",
                target_device
            ),
            "请提供 params 引用，或显式填写 speed/acc/dec。".to_string(),
        ));
        return;
    };
    let Some(resolved_acc) = resolved_acc else {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "[AXIS-007] axis.move on '{}' is missing speed/acc/dec parameters.",
                target_device
            ),
            "请提供 params 引用，或显式填写 speed/acc/dec。".to_string(),
        ));
        return;
    };
    let Some(resolved_dec) = resolved_dec else {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "[AXIS-007] axis.move on '{}' is missing speed/acc/dec parameters.",
                target_device
            ),
            "请提供 params 引用，或显式填写 speed/acc/dec。".to_string(),
        ));
        return;
    };

    if !resolved_speed.is_finite()
        || !resolved_acc.is_finite()
        || !resolved_dec.is_finite()
        || resolved_speed <= 0.0
        || resolved_acc <= 0.0
        || resolved_dec <= 0.0
    {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "[AXIS-008] axis.move parameters on '{}' must be positive finite values.",
                target_device
            ),
            "请确保 speed/acc/dec 均为正数。".to_string(),
        ));
        return;
    }

    let max_acc = profile.max_acceleration as f64;
    if resolved_acc > max_acc || resolved_dec > max_acc {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "[AXIS-009] axis.move parameters on '{}' exceed profile limits.",
                target_device
            ),
            format!(
                "请满足 acc/dec <= {}（由 model/config 限制推导）。",
                max_acc
            ),
        ));
        return;
    }

    if let Some(position) = absolute_position {
        if let (Some(min), Some(max)) = (profile.soft_limit_min, profile.soft_limit_max) {
            let min = min as f64;
            let max = max as f64;
            if position < min || position > max {
                errors.push(PlcError::semantic_with_reason(
                    line,
                    format!(
                        "[AXIS-011] axis.move_absolute position on '{}' exceeds soft limits {}..{}.",
                        target_device, min, max
                    ),
                    "请调整 position 或更新轴配置 soft_limit_min/soft_limit_max。"
                        .to_string(),
                ));
                return;
            }
        }
    }

    *speed = Some(resolved_speed);
    *acceleration = Some(resolved_acc);
    *deceleration = Some(resolved_dec);
}

#[derive(Debug, Clone, Copy, Default)]
struct BrakeSequenceProgress {
    engage_seen: bool,
    confirm_seen: bool,
}

fn validate_vertical_axis_brake_sequence_in_tasks(
    tasks: &TasksSection,
    topology: &TopologySection,
    errors: &mut Vec<PlcError>,
) {
    let disable_targets = collect_axis_disable_targets_from_tasks(tasks);
    if disable_targets.is_empty() {
        return;
    }

    let profile_devices = topology
        .devices
        .iter()
        .filter(|device| {
            disable_targets.contains(&device.name)
                && matches!(
                    device.device_type,
                    DeviceType::StepperMotor | DeviceType::ServoDrive
                )
                && device
                    .attributes
                    .model_ref
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty())
                && device
                    .attributes
                    .config_ref
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty())
        })
        .cloned()
        .collect::<Vec<_>>();

    if profile_devices.is_empty() {
        return;
    }

    let axis_profiles = match resolve_axis_profiles(&profile_devices) {
        Ok(profiles) => profiles,
        Err(mut profile_errors) => {
            errors.append(&mut profile_errors);
            return;
        }
    };

    let brake_requirements = axis_profiles
        .iter()
        .filter_map(|(axis, profile)| {
            if matches!(profile.orientation, crate::ir::AxisOrientation::Vertical) {
                profile.brake.clone().map(|brake| (axis.clone(), brake))
            } else {
                None
            }
        })
        .collect::<HashMap<_, _>>();

    if brake_requirements.is_empty() {
        return;
    }

    for task in &tasks.tasks {
        for step in &task.steps {
            let mut progress = brake_requirements
                .keys()
                .map(|axis| (axis.clone(), BrakeSequenceProgress::default()))
                .collect::<HashMap<_, _>>();
            validate_vertical_axis_brake_sequence_in_statements(
                &step.statements,
                step.line.max(1),
                &task.name,
                &step.name,
                &brake_requirements,
                &mut progress,
                errors,
            );
        }
    }
}

fn collect_axis_disable_targets_from_tasks(tasks: &TasksSection) -> HashSet<String> {
    let mut targets = HashSet::new();
    for task in &tasks.tasks {
        for step in &task.steps {
            collect_axis_disable_targets_from_statements(&step.statements, &mut targets);
        }
    }
    targets
}

fn collect_axis_disable_targets_from_statements(
    statements: &[StepStatement],
    targets: &mut HashSet<String>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::Set { target, value }) => {
                if target.port == "enable"
                    && set_enum_to_binary(value.as_str()) == Some(IrBinaryValue::Off)
                {
                    targets.insert(target.device.clone());
                }
            }
            StepStatement::Repeat { body, .. } => {
                collect_axis_disable_targets_from_statements(body, targets)
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    collect_axis_disable_targets_from_statements(&branch.statements, targets);
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    collect_axis_disable_targets_from_statements(&branch.statements, targets);
                }
            }
            StepStatement::Action(_)
            | StepStatement::Wait(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_)
            | StepStatement::Effect(_) => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_vertical_axis_brake_sequence_in_statements(
    statements: &[StepStatement],
    line: usize,
    task_name: &str,
    step_name: &str,
    brake_requirements: &HashMap<String, crate::ir::AxisBrakeConfig>,
    progress: &mut HashMap<String, BrakeSequenceProgress>,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::Set { target, value }) => {
                let Some(brake) = brake_requirements.get(&target.device) else {
                    continue;
                };

                if target.port == brake.engage_port
                    && set_enum_to_binary(value.as_str()) == Some(brake.engage_value.clone())
                {
                    if let Some(state) = progress.get_mut(&target.device) {
                        state.engage_seen = true;
                        state.confirm_seen = false;
                    }
                    continue;
                }

                if target.port == "enable"
                    && set_enum_to_binary(value.as_str()) == Some(IrBinaryValue::Off)
                {
                    let state = progress.get(&target.device).copied().unwrap_or_default();
                    if !(state.engage_seen && state.confirm_seen) {
                        errors.push(PlcError::semantic_with_reason(
                            line,
                            format!(
                                "[AXIS-012] vertical axis '{}' disables enable before brake_engage_confirmed.",
                                target.device
                            ),
                            format!(
                                "task '{}', step '{}' 中请先执行 `set {}.{} {}`，再 `wait: {}.{} == {}`，最后再 `set {}.enable off`。",
                                task_name,
                                step_name,
                                target.device,
                                brake.engage_port,
                                binary_value_text(&brake.engage_value),
                                target.device,
                                brake.engage_confirm_port,
                                bool_text(brake.engage_confirm_value),
                                target.device
                            ),
                        ));
                    }
                }
            }
            StepStatement::Wait(wait) => {
                for (axis, brake) in brake_requirements {
                    let Some(state) = progress.get(axis).copied() else {
                        continue;
                    };
                    if !state.engage_seen {
                        continue;
                    }
                    if wait_asserts_brake_confirmed(wait, axis, brake) {
                        if let Some(state_mut) = progress.get_mut(axis) {
                            state_mut.confirm_seen = true;
                        }
                    }
                }
            }
            StepStatement::Repeat { body, .. } => {
                validate_vertical_axis_brake_sequence_in_statements(
                    body,
                    line,
                    task_name,
                    step_name,
                    brake_requirements,
                    progress,
                    errors,
                );
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    let mut branch_progress = progress.clone();
                    validate_vertical_axis_brake_sequence_in_statements(
                        &branch.statements,
                        line,
                        task_name,
                        step_name,
                        brake_requirements,
                        &mut branch_progress,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    let mut branch_progress = progress.clone();
                    validate_vertical_axis_brake_sequence_in_statements(
                        &branch.statements,
                        line,
                        task_name,
                        step_name,
                        brake_requirements,
                        &mut branch_progress,
                        errors,
                    );
                }
            }
            StepStatement::Action(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_)
            | StepStatement::Effect(_) => {}
        }
    }
}

fn wait_asserts_brake_confirmed(
    wait: &WaitStatement,
    axis: &str,
    brake: &crate::ir::AxisBrakeConfig,
) -> bool {
    let expected_left = format!("{axis}.{}", brake.engage_confirm_port);
    let expected_right = brake.engage_confirm_value;

    let terms = match &wait.condition {
        WaitCondition::Single(term) => vec![term],
        WaitCondition::And(terms) => terms.iter().collect(),
        WaitCondition::Or(_) => return false,
    };

    terms.into_iter().any(|term| {
        !term.is_expression_compare()
            && matches!(term.operator, ComparisonOperator::Eq)
            && term.left == expected_left
            && literal_matches_bool(&term.right, expected_right)
    })
}

fn literal_matches_bool(literal: &LiteralValue, expected: bool) -> bool {
    match literal {
        LiteralValue::Boolean(value) => *value == expected,
        LiteralValue::String(value) => {
            let normalized = value.trim();
            (normalized == "true" && expected) || (normalized == "false" && !expected)
        }
        _ => false,
    }
}

fn binary_value_text(value: &IrBinaryValue) -> &'static str {
    match value {
        IrBinaryValue::On => "on",
        IrBinaryValue::Off => "off",
    }
}

fn bool_text(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn load_axis_motion_param_sets() -> Result<HashMap<String, AxisMotionParamSetDef>, Vec<PlcError>> {
    let root = Path::new(AXIS_MOTION_PARAM_SETS_DIR);
    let mut defs = HashMap::new();
    let mut errors = Vec::new();

    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(err) => {
            errors.push(PlcError::semantic_with_reason(
                1,
                format!("[AXIS-006] failed to read {AXIS_MOTION_PARAM_SETS_DIR} directory: {err}"),
                "请确认 axis_motion_param_sets 目录存在且可读。".to_string(),
            ));
            return Err(errors);
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }

        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(err) => {
                errors.push(PlcError::semantic_with_reason(
                    1,
                    format!(
                        "[AXIS-006] failed to read motion params file '{}': {err}",
                        path.display()
                    ),
                    "请确认参数集文件可读。".to_string(),
                ));
                continue;
            }
        };

        let def = match toml::from_str::<AxisMotionParamSetDef>(&content) {
            Ok(def) => def,
            Err(err) => {
                errors.push(PlcError::semantic_with_reason(
                    1,
                    format!(
                        "[AXIS-006] failed to parse motion params file '{}': {err}",
                        path.display()
                    ),
                    "请检查 TOML 字段并确保仅使用 name/config_id/speed/acceleration/deceleration。"
                        .to_string(),
                ));
                continue;
            }
        };

        defs.insert(def.name.clone(), def);
    }

    if errors.is_empty() {
        Ok(defs)
    } else {
        Err(errors)
    }
}

fn validate_expression_variables(
    expr: &AstExpression,
    line: usize,
    variables: &HashSet<String>,
    errors: &mut Vec<PlcError>,
) {
    match expr {
        AstExpression::Literal(_) => {}
        AstExpression::Boolean(_) => {}
        AstExpression::Variable(name) => {
            if !variables.contains(name) {
                errors.push(PlcError::undefined_reference_with_reason(
                    line,
                    "变量",
                    name,
                    "表达式变量必须先在 [topology] 中使用 variable 声明".to_string(),
                ));
            }
        }
        AstExpression::UnaryNeg(inner) => {
            validate_expression_variables(inner, line, variables, errors)
        }
        AstExpression::UnaryNot(inner) => {
            validate_expression_variables(inner, line, variables, errors)
        }
        AstExpression::BinaryOp { left, right, .. } => {
            validate_expression_variables(left, line, variables, errors);
            validate_expression_variables(right, line, variables, errors);
        }
        AstExpression::FunctionCall { args, .. } => {
            for arg in args {
                validate_expression_variables(arg, line, variables, errors);
            }
            validate_builtin_function_call(expr, line, errors);
        }
    }
}

fn validate_builtin_function_call(expr: &AstExpression, line: usize, errors: &mut Vec<PlcError>) {
    let AstExpression::FunctionCall { name, args } = expr else {
        return;
    };

    let expected_arity = match name.as_str() {
        "abs" | "sin" | "cos" | "sqrt" => 1,
        "min" | "max" | "pow" | "fmod" => 2,
        "clamp" => 3,
        _ => {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!("不支持的内置函数: {name}"),
                "支持函数: abs/min/max/sin/cos/sqrt/pow/fmod/clamp".to_string(),
            ));
            return;
        }
    };

    if args.len() != expected_arity {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "函数 {name} 参数个数错误：期望 {expected_arity} 个，实际 {} 个",
                args.len()
            ),
            "请检查函数调用参数数量".to_string(),
        ));
    }
}

fn expression_type_assignable_to(
    expression_type: &AstVariableType,
    target_type: &AstVariableType,
) -> bool {
    matches!(
        (expression_type, target_type),
        (AstVariableType::Bool, AstVariableType::Bool)
            | (AstVariableType::Int, AstVariableType::Int)
            | (AstVariableType::Int, AstVariableType::Float)
            | (AstVariableType::Float, AstVariableType::Float)
    )
}

fn validate_motor_legacy_set_actions(
    statements: &[StepStatement],
    line: usize,
    device_kinds: &HashMap<String, DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::Set { target, value })
                if target.port == "self"
                    && matches!(device_kinds.get(&target.device), Some(DeviceKind::Motor)) =>
            {
                errors.push(PlcError::semantic_with_reason(
                    line,
                    format!("set {} {value} 旧写法已废弃", target.device),
                    format!(
                        "请改用显式端口写法：set {}.run on/off 或 set {}.direction forward/reverse",
                        target.device, target.device
                    ),
                ));
            }
            StepStatement::Repeat { body, .. } => {
                validate_motor_legacy_set_actions(body, line, device_kinds, errors)
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_motor_legacy_set_actions(
                        &branch.statements,
                        line,
                        device_kinds,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_motor_legacy_set_actions(
                        &branch.statements,
                        line,
                        device_kinds,
                        errors,
                    );
                }
            }
            StepStatement::Action(_)
            | StepStatement::Wait(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_)
            | StepStatement::Effect(_) => {}
        }
    }
}

fn validate_wait_device_references_in_statements(
    statements: &[StepStatement],
    line: usize,
    device_kinds: &HashMap<String, DeviceKind>,
    device_port_types: &HashMap<String, PortType>,
    device_ranges: &HashMap<String, (f64, f64)>,
    device_units: &HashMap<String, String>,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Wait(wait) => {
                let should_validate_references =
                    matches!(wait.condition, WaitCondition::And(_) | WaitCondition::Or(_));
                for condition in wait_condition_terms(&wait.condition) {
                    if condition.is_expression_compare() {
                        continue;
                    }
                    validate_motor_legacy_wait_operand(&condition.left, line, device_kinds, errors);
                    if should_validate_references {
                        validate_wait_operand_device(
                            &condition.left,
                            line,
                            "wait 条件左值",
                            device_kinds,
                            errors,
                        );
                        if let LiteralValue::State(state) = &condition.right {
                            validate_device_reference(
                                &state.device,
                                line,
                                "wait 条件右值",
                                device_kinds,
                                errors,
                            );
                        }
                    }
                    if let Some((value, unit)) = threshold_literal_value_and_unit(&condition.right)
                    {
                        if wait_operand_device_name(&condition.left).is_some() {
                            validate_analog_threshold_comparison(
                                &condition.left,
                                value,
                                unit,
                                line,
                                "wait 条件阈值比较",
                                device_kinds,
                                device_port_types,
                                device_ranges,
                                device_units,
                                errors,
                            );
                        }
                    }
                }
            }
            StepStatement::IfElse { .. } => {}
            StepStatement::Repeat { body, .. } => {
                validate_wait_device_references_in_statements(
                    body,
                    line,
                    device_kinds,
                    device_port_types,
                    device_ranges,
                    device_units,
                    errors,
                );
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_wait_device_references_in_statements(
                        &branch.statements,
                        line,
                        device_kinds,
                        device_port_types,
                        device_ranges,
                        device_units,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_wait_device_references_in_statements(
                        &branch.statements,
                        line,
                        device_kinds,
                        device_port_types,
                        device_ranges,
                        device_units,
                        errors,
                    );
                }
            }
            StepStatement::Action(_)
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_)
            | StepStatement::Effect(_) => {}
        }
    }
}

fn wait_condition_terms(condition: &WaitCondition) -> Vec<&ConditionExpression> {
    match condition {
        WaitCondition::Single(term) => vec![term],
        WaitCondition::And(terms) | WaitCondition::Or(terms) => terms.iter().collect(),
    }
}

fn validate_wait_operand_device(
    operand: &str,
    line: usize,
    source: &str,
    device_kinds: &HashMap<String, DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    if let Some(candidate) = wait_operand_device_name(operand) {
        validate_device_reference(candidate, line, source, device_kinds, errors);
    }
}

fn validate_motor_legacy_wait_operand(
    operand: &str,
    line: usize,
    device_kinds: &HashMap<String, DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    let mut parts = operand.split('.');
    let Some(device) = parts.next() else {
        return;
    };
    let Some(state) = parts.next() else {
        return;
    };
    if parts.next().is_some() {
        return;
    }

    if matches!(device_kinds.get(device), Some(DeviceKind::Motor)) {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!("wait 条件使用了已废弃的电机状态写法 {device}.{state}"),
            format!(
                "请改用显式端口状态，例如 {device}.run.on/off 或 {device}.direction.forward/reverse"
            ),
        ));
    }
}

fn wait_operand_device_name(operand: &str) -> Option<&str> {
    let candidate = operand.split('.').next().unwrap_or(operand).trim();

    if candidate.is_empty() {
        None
    } else {
        Some(candidate)
    }
}

fn parse_threshold_target(target: &str) -> Option<(&str, Option<&str>)> {
    let mut parts = target.split('.');
    let device = parts.next()?.trim();
    if device.is_empty() {
        return None;
    }
    let Some(port) = parts.next() else {
        return Some((device, None));
    };
    if parts.next().is_some() {
        return None;
    }
    let port = port.trim();
    if port.is_empty() {
        return None;
    }
    Some((device, Some(port)))
}

fn map_safety_relation(relation: &AstSafetyRelation) -> IrSafetyRelation {
    match relation {
        AstSafetyRelation::ConflictsWith => IrSafetyRelation::ConflictsWith,
        AstSafetyRelation::Requires => IrSafetyRelation::Requires,
    }
}

fn map_semantic_resource_mode(mode: &AstSemanticResourceMode) -> IrSemanticResourceMode {
    match mode {
        AstSemanticResourceMode::Exclusive => IrSemanticResourceMode::Exclusive,
    }
}

fn map_resource_claim_source(source: &AstResourceClaimSource) -> IrResourceClaimSource {
    match source {
        AstResourceClaimSource::State(state_ref) => IrResourceClaimSource::State(StateExpr {
            device: state_ref.device.clone(),
            port: state_ref.port.clone(),
            state: state_ref.state.clone(),
        }),
        AstResourceClaimSource::ActionTag { tag } => {
            IrResourceClaimSource::ActionTag { tag: tag.clone() }
        }
    }
}

fn collect_declared_action_tags(tasks: &TasksSection) -> HashSet<String> {
    let mut tags = HashSet::new();
    for task in &tasks.tasks {
        for step in &task.steps {
            collect_action_tags_from_statements(&step.statements, &mut tags);
        }
    }
    tags
}

fn collect_action_tags_from_statements(statements: &[StepStatement], tags: &mut HashSet<String>) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::AxisMoveRelative {
                semantic_tag: Some(tag),
                ..
            })
            | StepStatement::Action(ActionStatement::AxisMoveAbsolute {
                semantic_tag: Some(tag),
                ..
            }) => {
                tags.insert(tag.clone());
            }
            StepStatement::Repeat { body, .. } => collect_action_tags_from_statements(body, tags),
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    collect_action_tags_from_statements(&branch.statements, tags);
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    collect_action_tags_from_statements(&branch.statements, tags);
                }
            }
            StepStatement::Action(_)
            | StepStatement::Wait(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_)
            | StepStatement::Effect(_) => {}
        }
    }
}

fn validate_safety_operand(
    operand: &SafetyOperand,
    line: usize,
    source: &str,
    device_kinds: &HashMap<String, DeviceKind>,
    known_states: &HashMap<String, HashSet<String>>,
    device_port_types: &HashMap<String, PortType>,
    device_ranges: &HashMap<String, (f64, f64)>,
    device_units: &HashMap<String, String>,
    errors: &mut Vec<PlcError>,
) {
    match operand {
        SafetyOperand::State(state_ref) => {
            validate_state_reference(state_ref, line, source, device_kinds, known_states, errors);
        }
        SafetyOperand::Threshold {
            device,
            value,
            unit,
            ..
        } => {
            if let Some(device_name) = wait_operand_device_name(device) {
                validate_device_reference(device_name, line, source, device_kinds, errors);
            }
            validate_analog_threshold_comparison(
                device,
                *value,
                unit.as_deref(),
                line,
                "safety 阈值比较",
                device_kinds,
                device_port_types,
                device_ranges,
                device_units,
                errors,
            );
        }
    }
}

fn validate_analog_threshold_comparison(
    target: &str,
    value: f64,
    value_unit: Option<&str>,
    line: usize,
    source: &str,
    device_kinds: &HashMap<String, DeviceKind>,
    device_port_types: &HashMap<String, PortType>,
    device_ranges: &HashMap<String, (f64, f64)>,
    device_units: &HashMap<String, String>,
    errors: &mut Vec<PlcError>,
) {
    let Some((device, port)) = parse_threshold_target(target) else {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!("{source} 目标 {target} 格式非法"),
            "阈值比较目标仅支持 device 或 device.port".to_string(),
        ));
        return;
    };

    let Some(kind) = device_kinds.get(device) else {
        return;
    };

    let range_key = if let Some(port_name) = port {
        let key = format!("{device}.{port_name}");
        let is_analog = device_port_types
            .get(&key)
            .is_some_and(|port_type| matches!(port_type, PortType::Analog));
        if !is_analog {
            errors.push(PlcError::type_mismatch_with_reason(
                line,
                "analog 端口",
                format!("{}.{}", device_kind_name(kind), port_name),
                format!("{source} {target}"),
                "阈值比较仅支持模拟量端口（如 cam_xy.following_error）",
            ));
            return;
        }
        Some(key)
    } else {
        if *kind != DeviceKind::AnalogInput {
            errors.push(PlcError::type_mismatch_with_reason(
                line,
                "analog_input",
                device_kind_name(kind),
                format!("{source} {target}"),
                "阈值比较仅支持 analog_input 设备，或 device.port 形式的模拟量端口",
            ));
            return;
        }
        Some(device.to_string())
    };

    let range = range_key
        .as_ref()
        .and_then(|key| device_ranges.get(key))
        .copied();

    if port.is_none() {
        let Some((min, max)) = range else {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!("模拟量输入 {device} 缺少 range，无法进行阈值比较"),
                "请在 [topology] 段为该设备声明 range: min..max",
            ));
            return;
        };

        if value < min || value > max {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!("阈值 {value} 超出 {device} 的 range {min}..{max}"),
                "请调整阈值或更新 range 范围",
            ));
        }
    }

    if let Some(expected_unit) = device_units.get(device) {
        if let Some(got_unit) = value_unit
            && got_unit != expected_unit
        {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!(
                    "阈值单位不一致：{device} 声明 unit=\"{expected_unit}\"，但比较值使用单位 \"{got_unit}\"",
                ),
                "请统一单位（修改 unit 或比较值单位），或移除比较值的单位后缀",
            ));
        }
    }
}

fn map_safety_operand(operand: &SafetyOperand) -> SafetyExpr {
    match operand {
        SafetyOperand::State(state_ref) => SafetyExpr::State(StateExpr {
            device: state_ref.device.clone(),
            port: state_ref.port.clone(),
            state: state_ref.state.clone(),
        }),
        SafetyOperand::Threshold {
            device,
            operator,
            value,
            unit: _,
        } => SafetyExpr::Threshold {
            device: device.clone(),
            operator: comparison_operator_to_string(operator).to_string(),
            value: value.to_string(),
        },
    }
}

fn comparison_operator_to_string(op: &ComparisonOperator) -> &'static str {
    match op {
        ComparisonOperator::Eq => "==",
        ComparisonOperator::Neq => "!=",
        ComparisonOperator::Gt => ">",
        ComparisonOperator::Lt => "<",
        ComparisonOperator::Gte => ">=",
        ComparisonOperator::Lte => "<=",
    }
}

fn map_timing_scope(target: &TimingTarget) -> TimingScope {
    match target {
        TimingTarget::Task { task } => TimingScope::Task { task: task.clone() },
        TimingTarget::Step { task, step } => TimingScope::Step {
            task: task.clone(),
            step: step.clone(),
        },
    }
}

fn map_timing_relation(relation: &AstTimingRelation) -> IrTimingRelation {
    match relation {
        AstTimingRelation::MustCompleteWithin => IrTimingRelation::MustCompleteWithin,
        AstTimingRelation::MustCompleteWithinWorstCase => {
            IrTimingRelation::MustCompleteWithinWorstCase
        }
        AstTimingRelation::MustStartAfter => IrTimingRelation::MustStartAfter,
    }
}

