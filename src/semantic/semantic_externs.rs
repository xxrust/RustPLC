#[derive(Debug, Clone)]
struct ExternFunctionSignature {
    line: usize,
    param_types: Vec<AstVariableType>,
    return_types: Vec<AstVariableType>,
    pure: bool,
}

fn collect_variable_types(topology: &TopologySection) -> HashMap<String, AstVariableType> {
    topology
        .variables
        .iter()
        .map(|variable| (variable.name.clone(), variable.var_type.clone()))
        .collect()
}

fn collect_extern_function_signatures(
    topology: &TopologySection,
    errors: &mut Vec<PlcError>,
) -> HashMap<String, ExternFunctionSignature> {
    let mut signatures: HashMap<String, ExternFunctionSignature> = HashMap::new();

    for decl in &topology.extern_functions {
        let line = decl.line.max(1);
        validate_extern_function_contract(decl, errors);
        validate_extern_function_signature_types(decl, errors);
        if let Some(previous) = signatures.get(&decl.name) {
            errors.push(PlcError::duplicate_definition_with_reason(
                line,
                "extern 函数",
                &decl.name,
                format!(
                    "extern 函数 {} 已在第 {} 行声明，请保持函数签名唯一",
                    decl.name, previous.line
                ),
            ));
            continue;
        }

        signatures.insert(
            decl.name.clone(),
            ExternFunctionSignature {
                line,
                param_types: decl
                    .params
                    .iter()
                    .map(|param| param.var_type.clone())
                    .collect(),
                return_types: decl.return_types.clone(),
                pure: decl.contract.pure,
            },
        );
    }

    signatures
}

fn validate_extern_function_contract(
    decl: &AstExternFunctionDeclaration,
    errors: &mut Vec<PlcError>,
) {
    let line = decl.line.max(1);

    if decl.contract.rust_module.trim().is_empty() {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!("extern 函数 {} 的 rust_module 不能为空", decl.name),
            "请为 rust_module 设置非空字符串（例如 \"math::add\"）",
        ));
    }

    if decl.contract.time_bound_us == 0 {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "extern 函数 {} 的 time_bound_us 必须为正整数，当前为 0",
                decl.name
            ),
            "请将 time_bound_us 设置为大于 0 的整数值（单位：微秒）",
        ));
    }
}

fn validate_extern_function_signature_types(
    decl: &AstExternFunctionDeclaration,
    errors: &mut Vec<PlcError>,
) {
    let line = decl.line.max(1);

    for (index, param) in decl.params.iter().enumerate() {
        if !is_phase1_supported_extern_type(&param.var_type) {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!(
                    "extern 函数 {} 参数 #{} 使用了不支持的类型 {}",
                    decl.name,
                    index + 1,
                    ast_variable_type_name(&param.var_type)
                ),
                "Phase 1 仅支持标量类型：bool/int/float",
            ));
        }
    }

    for (index, return_type) in decl.return_types.iter().enumerate() {
        if !is_phase1_supported_extern_type(return_type) {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!(
                    "extern 函数 {} 返回值 #{} 使用了不支持的类型 {}",
                    decl.name,
                    index + 1,
                    ast_variable_type_name(return_type)
                ),
                "Phase 1 仅支持标量类型：bool/int/float",
            ));
        }
    }
}

fn is_phase1_supported_extern_type(var_type: &AstVariableType) -> bool {
    matches!(
        var_type,
        AstVariableType::Float | AstVariableType::Int | AstVariableType::Bool
    )
}

pub fn build_constraint_set(program: &PlcProgram) -> Result<ConstraintSet, Vec<PlcError>> {
    let expanded = preprocess_program(program)?;
    let mut errors = Vec::new();
    device_semantics::axis::validate_vertical_axis_brake_sequence_in_tasks(
        &expanded.tasks,
        &expanded.topology,
        &mut errors,
    );
    match build_constraint_set_from_ast(&expanded.topology, &expanded.constraints, &expanded.tasks)
    {
        Ok(constraints) if errors.is_empty() => Ok(constraints),
        Ok(_) => Err(errors),
        Err(mut constraint_errors) => {
            errors.append(&mut constraint_errors);
            Err(errors)
        }
    }
}

pub fn build_timing_model(program: &PlcProgram) -> Result<TimingModel, Vec<PlcError>> {
    let expanded = preprocess_program(program)?;
    build_timing_model_from_ast(&expanded.topology, &expanded.tasks)
}

