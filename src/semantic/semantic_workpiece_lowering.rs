#[derive(Debug, Clone, Default)]
struct WorkpieceCatalog {
    site_kinds: HashMap<String, AstWorkpieceSiteKind>,
    holders: HashSet<String>,
    carriers: HashMap<String, CarrierShape>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CarrierShape {
    dimensions: Vec<u32>,
}

#[allow(dead_code)]
fn validate_and_lower_workpiece_topology(
    topology: &TopologySection,
    constraint_set: &mut ConstraintSet,
    errors: &mut Vec<PlcError>,
) -> WorkpieceCatalog {
    let mut catalog = WorkpieceCatalog::default();
    let mut seen_workpiece_types = HashSet::<String>::new();
    let mut seen_places = HashSet::<String>::new();

    for site in &topology.workpiece_sites {
        if !seen_places.insert(site.name.clone()) {
            errors.push(PlcError::semantic_with_reason(
                site.line.max(1),
                format!("workpiece site '{}' 重复声明", site.name),
                "请删除重复的 site/location 声明，或改名以保持引用唯一".to_string(),
            ));
            continue;
        }
        catalog
            .site_kinds
            .insert(site.name.clone(), site.kind.clone());
        constraint_set.workpiece_sites.push(IrWorkpieceSiteDef {
            name: site.name.clone(),
            kind: map_workpiece_site_kind(&site.kind),
            capacity: site.capacity,
        });
    }

    for holder in &topology.workpiece_holders {
        if !seen_places.insert(holder.name.clone()) {
            errors.push(PlcError::semantic_with_reason(
                holder.line.max(1),
                format!("workpiece endpoint '{}' 重复声明", holder.name),
                "holder 与 site/location 不能重名，否则 effect 引用会失去唯一性".to_string(),
            ));
            continue;
        }
        catalog.holders.insert(holder.name.clone());
        constraint_set.workpiece_holders.push(IrWorkpieceHolderDef {
            name: holder.name.clone(),
            capacity: holder.capacity,
        });
    }

    for workpiece in &topology.workpiece_types {
        if !seen_workpiece_types.insert(workpiece.name.clone()) {
            errors.push(PlcError::semantic_with_reason(
                workpiece.line.max(1),
                format!("workpiece type '{}' 重复声明", workpiece.name),
                "请合并重复的 workpiece type 声明，或改名".to_string(),
            ));
            continue;
        }

        validate_workpiece_type_declaration(workpiece, &catalog, errors);
        constraint_set.workpiece_types.push(IrWorkpieceTypeDef {
            name: workpiece.name.clone(),
            properties: workpiece
                .properties
                .iter()
                .map(|property| IrWorkpiecePropertyDef {
                    name: property.name.clone(),
                    property_type: map_workpiece_property_type(&property.property_type),
                })
                .collect(),
            normal_terminal_states: workpiece.normal_terminal_states.clone(),
            abnormal_terminal_states: workpiece.abnormal_terminal_states.clone(),
            ingress_sites: workpiece.ingress_sites.clone(),
            normal_egress_sites: workpiece.normal_egress_sites.clone(),
            abnormal_egress_sites: workpiece.abnormal_egress_sites.clone(),
            allows: vec![],
            derived_from: vec![],
        });
    }

    catalog
}

fn map_workpiece_site_kind(kind: &AstWorkpieceSiteKind) -> IrWorkpieceSiteKind {
    match kind {
        AstWorkpieceSiteKind::WorkpieceLocation => IrWorkpieceSiteKind::WorkpieceLocation,
        AstWorkpieceSiteKind::CarrierLocation => IrWorkpieceSiteKind::CarrierLocation,
    }
}

fn map_workpiece_property_type(kind: &AstWorkpiecePropertyType) -> IrWorkpiecePropertyTypeDef {
    match kind {
        AstWorkpiecePropertyType::Bool => IrWorkpiecePropertyTypeDef::Bool,
        AstWorkpiecePropertyType::Enum { values } => IrWorkpiecePropertyTypeDef::Enum {
            values: values.clone(),
        },
    }
}

fn validate_workpiece_type_declaration(
    workpiece: &AstWorkpieceTypeDeclaration,
    catalog: &WorkpieceCatalog,
    errors: &mut Vec<PlcError>,
) {
    let mut seen_properties = HashSet::<String>::new();
    for property in &workpiece.properties {
        if !seen_properties.insert(property.name.clone()) {
            errors.push(PlcError::semantic_with_reason(
                workpiece.line.max(1),
                format!(
                    "workpiece type '{}' 的属性 '{}' 重复声明",
                    workpiece.name, property.name
                ),
                "同一个 workpiece type 内，属性名必须唯一".to_string(),
            ));
        }
        if let AstWorkpiecePropertyType::Enum { values } = &property.property_type {
            if values.is_empty() {
                errors.push(PlcError::semantic_with_reason(
                    workpiece.line.max(1),
                    format!(
                        "workpiece type '{}' 的枚举属性 '{}' 为空",
                        workpiece.name, property.name
                    ),
                    "enum 属性至少需要一个候选值".to_string(),
                ));
            }
        }
    }
    for property in &workpiece.properties {
        let AstWorkpiecePropertyType::Enum { values } = &property.property_type else {
            continue;
        };
        let mut seen_values = HashSet::<String>::new();
        for value in values {
            if seen_values.insert(value.clone()) {
                continue;
            }
            errors.push(PlcError::semantic_with_reason(
                workpiece.line.max(1),
                format!(
                    "workpiece type '{}' enum property '{}' repeats value '{}'",
                    workpiece.name, property.name, value
                ),
                "remove the duplicate enum value".to_string(),
            ));
        }
    }

    validate_terminal_egress_pair(
        workpiece.line.max(1),
        &workpiece.name,
        "normal",
        &workpiece.normal_terminal_states,
        &workpiece.normal_egress_sites,
        errors,
    );
    validate_terminal_egress_pair(
        workpiece.line.max(1),
        &workpiece.name,
        "abnormal",
        &workpiece.abnormal_terminal_states,
        &workpiece.abnormal_egress_sites,
        errors,
    );
    validate_unique_workpiece_entries(
        workpiece.line.max(1),
        &workpiece.name,
        "normal terminal state",
        &workpiece.normal_terminal_states,
        errors,
    );
    validate_unique_workpiece_entries(
        workpiece.line.max(1),
        &workpiece.name,
        "abnormal terminal state",
        &workpiece.abnormal_terminal_states,
        errors,
    );
    validate_reserved_terminal_state_entries(
        workpiece.line.max(1),
        &workpiece.name,
        "normal",
        &workpiece.normal_terminal_states,
        errors,
    );
    validate_reserved_terminal_state_entries(
        workpiece.line.max(1),
        &workpiece.name,
        "abnormal",
        &workpiece.abnormal_terminal_states,
        errors,
    );
    validate_unique_workpiece_entries(
        workpiece.line.max(1),
        &workpiece.name,
        "ingress site",
        &workpiece.ingress_sites,
        errors,
    );
    validate_unique_workpiece_entries(
        workpiece.line.max(1),
        &workpiece.name,
        "normal egress site",
        &workpiece.normal_egress_sites,
        errors,
    );
    validate_unique_workpiece_entries(
        workpiece.line.max(1),
        &workpiece.name,
        "abnormal egress site",
        &workpiece.abnormal_egress_sites,
        errors,
    );
    validate_disjoint_workpiece_entries(
        workpiece.line.max(1),
        &workpiece.name,
        "terminal state",
        "normal",
        &workpiece.normal_terminal_states,
        "abnormal",
        &workpiece.abnormal_terminal_states,
        errors,
    );
    validate_disjoint_workpiece_entries(
        workpiece.line.max(1),
        &workpiece.name,
        "egress site",
        "normal",
        &workpiece.normal_egress_sites,
        "abnormal",
        &workpiece.abnormal_egress_sites,
        errors,
    );

    for site in workpiece
        .ingress_sites
        .iter()
        .chain(workpiece.normal_egress_sites.iter())
        .chain(workpiece.abnormal_egress_sites.iter())
    {
        validate_workpiece_location_reference(workpiece.line.max(1), site, catalog, errors);
    }
}

fn validate_terminal_egress_pair(
    line: usize,
    workpiece_name: &str,
    category: &str,
    terminal_states: &[String],
    egress_sites: &[String],
    errors: &mut Vec<PlcError>,
) {
    if terminal_states.is_empty() != egress_sites.is_empty() {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "workpiece type '{}' 的 {} terminal states 与 egress sites 必须成对声明",
                workpiece_name, category
            ),
            "如果声明了一侧，另一侧也必须同时存在".to_string(),
        ));
    }
}

fn validate_unique_workpiece_entries(
    line: usize,
    workpiece_name: &str,
    entry_kind: &str,
    entries: &[String],
    errors: &mut Vec<PlcError>,
) {
    let mut seen = HashSet::<String>::new();
    for entry in entries {
        if seen.insert(entry.clone()) {
            continue;
        }
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "workpiece type '{}' repeats {} '{}'",
                workpiece_name, entry_kind, entry
            ),
            format!("remove the duplicate {} entry", entry_kind),
        ));
    }
}

fn validate_disjoint_workpiece_entries(
    line: usize,
    workpiece_name: &str,
    entry_kind: &str,
    left_label: &str,
    left_entries: &[String],
    right_label: &str,
    right_entries: &[String],
    errors: &mut Vec<PlcError>,
) {
    let right = right_entries.iter().cloned().collect::<HashSet<_>>();
    for entry in left_entries {
        if !right.contains(entry) {
            continue;
        }
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "workpiece type '{}' declares {} '{}' in both {} and {} categories",
                workpiece_name, entry_kind, entry, left_label, right_label
            ),
            format!(
                "keep each {} in exactly one of {} or {}",
                entry_kind, left_label, right_label
            ),
        ));
    }
}

fn validate_reserved_terminal_state_entries(
    line: usize,
    workpiece_name: &str,
    category: &str,
    entries: &[String],
    errors: &mut Vec<PlcError>,
) {
    for entry in entries {
        if entry != "consumed" {
            continue;
        }
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "workpiece type '{}' cannot declare reserved terminal state 'consumed' in {} category",
                workpiece_name, category
            ),
            "model in-process consumption via split/merge effects instead of terminal egress"
                .to_string(),
        ));
    }
}

fn validate_unique_workpiece_rule_entries(
    line: usize,
    workpiece_name: &str,
    rule_kind: &str,
    entries: &[String],
    errors: &mut Vec<PlcError>,
) {
    let mut seen = HashSet::<String>::new();
    for entry in entries {
        if seen.insert(entry.clone()) {
            continue;
        }
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "workpiece type '{}' repeats {} rule '{}'",
                workpiece_name, rule_kind, entry
            ),
            format!("remove the duplicate {} rule", rule_kind),
        ));
    }
}

fn validate_workpiece_location_reference(
    line: usize,
    site: &str,
    catalog: &WorkpieceCatalog,
    errors: &mut Vec<PlcError>,
) {
    if site.contains(".slot[") {
        validate_workpiece_contract_reference_v2(line, site, catalog, errors);
        return;
    }
    match catalog.site_kinds.get(site) {
        Some(AstWorkpieceSiteKind::WorkpieceLocation) => {}
        Some(AstWorkpieceSiteKind::CarrierLocation) => errors.push(PlcError::semantic_with_reason(
            line,
            format!("工件契约引用了 carrier_location '{}'", site),
            "Phase 1 的 ingress/egress 只能引用 workpiece_location".to_string(),
        )),
        None => errors.push(PlcError::undefined_reference_with_reason(
            line,
            "workpiece_location",
            site,
            "请先在 [topology] 中声明对应的 location".to_string(),
        )),
    }
}

fn tasks_use_workpiece_effects(tasks: &TasksSection) -> bool {
    tasks
        .tasks
        .iter()
        .flat_map(|task| task.steps.iter())
        .any(|step| statements_use_workpiece_effects(&step.statements))
}

fn statements_use_workpiece_effects(statements: &[StepStatement]) -> bool {
    statements.iter().any(|statement| match statement {
        StepStatement::Effect(_) => true,
        StepStatement::Repeat { body, .. } => statements_use_workpiece_effects(body),
        StepStatement::Parallel(block) => block
            .branches
            .iter()
            .any(|branch| statements_use_workpiece_effects(&branch.statements)),
        StepStatement::Race(block) => block
            .branches
            .iter()
            .any(|branch| statements_use_workpiece_effects(&branch.statements)),
        StepStatement::Action(_)
        | StepStatement::Wait(_)
        | StepStatement::IfElse { .. }
        | StepStatement::Delay { .. }
        | StepStatement::Timeout(_)
        | StepStatement::Goto(_)
        | StepStatement::AllowIndefiniteWait(_) => false,
    })
}

#[allow(dead_code)]
fn validate_workpiece_effects_in_tasks(
    tasks: &TasksSection,
    catalog: &WorkpieceCatalog,
    workpiece_types: &[IrWorkpieceTypeDef],
    errors: &mut Vec<PlcError>,
) {
    if workpiece_types.is_empty() {
        errors.push(PlcError::semantic_with_reason(
            1,
            "检测到 effect 语句，但 [topology] 未声明任何 workpiece type".to_string(),
            "Phase 1 的工件 effect 需要至少一个 workpiece type 契约".to_string(),
        ));
        return;
    }

    if workpiece_types.len() != 1 {
        errors.push(PlcError::semantic_with_reason(
            1,
            format!(
                "当前声明了 {} 个 workpiece type，但 Phase 1 effect 只支持单工件类型",
                workpiece_types.len()
            ),
            "请先收敛到一个 workpiece type，再使用 transfer/acquire/finish effect".to_string(),
        ));
        return;
    }

    let workpiece = &workpiece_types[0];
    for task in &tasks.tasks {
        for step in &task.steps {
            validate_workpiece_effects_in_statements(&step.statements, catalog, workpiece, errors);
        }
    }
}

#[allow(dead_code)]
fn validate_workpiece_effects_in_statements(
    statements: &[StepStatement],
    catalog: &WorkpieceCatalog,
    workpiece: &IrWorkpieceTypeDef,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Effect(effect) => match &effect.kind {
                AstEffectKind::Acquire { holder, from } => {
                    validate_holder_reference(effect.line.max(1), holder, catalog, errors);
                    validate_workpiece_location_reference(
                        effect.line.max(1),
                        from,
                        catalog,
                        errors,
                    );
                }
                AstEffectKind::Transfer { from, to } => {
                    validate_workpiece_endpoint_reference(
                        effect.line.max(1),
                        from,
                        catalog,
                        errors,
                    );
                    validate_workpiece_endpoint_reference(effect.line.max(1), to, catalog, errors);
                }
                AstEffectKind::Finish { at, terminal_state } => {
                    validate_workpiece_location_reference(effect.line.max(1), at, catalog, errors);
                    let normal = workpiece
                        .normal_terminal_states
                        .iter()
                        .any(|state| state == terminal_state);
                    let abnormal = workpiece
                        .abnormal_terminal_states
                        .iter()
                        .any(|state| state == terminal_state);
                    if !normal && !abnormal {
                        errors.push(PlcError::semantic_with_reason(
                            effect.line.max(1),
                            format!(
                                "terminal state '{}' 未在 workpiece type '{}' 中声明",
                                terminal_state, workpiece.name
                            ),
                            "finish effect 只能使用已声明的 normal/abnormal terminal state"
                                .to_string(),
                        ));
                    }
                    if normal && !workpiece.normal_egress_sites.iter().any(|site| site == at) {
                        errors.push(PlcError::semantic_with_reason(
                            effect.line.max(1),
                            format!(
                                "finish at '{}' as '{}' 不满足 normal egress 契约",
                                at, terminal_state
                            ),
                            "normal terminal state 只能落在 normal_egress_sites".to_string(),
                        ));
                    }
                    if abnormal
                        && !workpiece
                            .abnormal_egress_sites
                            .iter()
                            .any(|site| site == at)
                    {
                        errors.push(PlcError::semantic_with_reason(
                            effect.line.max(1),
                            format!(
                                "finish at '{}' as '{}' 不满足 abnormal egress 契约",
                                at, terminal_state
                            ),
                            "abnormal terminal state 只能落在 abnormal_egress_sites".to_string(),
                        ));
                    }
                }
                _ => {}
            },
            StepStatement::Repeat { body, .. } => {
                validate_workpiece_effects_in_statements(body, catalog, workpiece, errors)
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_workpiece_effects_in_statements(
                        &branch.statements,
                        catalog,
                        workpiece,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_workpiece_effects_in_statements(
                        &branch.statements,
                        catalog,
                        workpiece,
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
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
}

fn validate_holder_reference(
    line: usize,
    holder: &str,
    catalog: &WorkpieceCatalog,
    errors: &mut Vec<PlcError>,
) {
    if !catalog.holders.contains(holder) {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            "workpiece_holder",
            holder,
            "请先在 [topology] 中声明对应的 holder".to_string(),
        ));
    }
}

#[allow(dead_code)]
fn validate_workpiece_endpoint_reference(
    line: usize,
    endpoint: &str,
    catalog: &WorkpieceCatalog,
    errors: &mut Vec<PlcError>,
) {
    if catalog.holders.contains(endpoint) {
        return;
    }
    validate_workpiece_location_reference(line, endpoint, catalog, errors);
}

fn validate_and_lower_workpiece_topology_v2(
    topology: &TopologySection,
    constraint_set: &mut ConstraintSet,
    errors: &mut Vec<PlcError>,
) -> WorkpieceCatalog {
    let mut catalog = WorkpieceCatalog::default();
    let mut seen_workpiece_types = HashSet::<String>::new();
    let mut seen_endpoints = HashSet::<String>::new();
    let declared_type_names = topology
        .workpiece_types
        .iter()
        .map(|workpiece| workpiece.name.clone())
        .collect::<HashSet<_>>();

    for site in &topology.workpiece_sites {
        if !seen_endpoints.insert(site.name.clone()) {
            errors.push(PlcError::semantic_with_reason(
                site.line.max(1),
                format!(
                    "workpiece endpoint '{}' is declared more than once",
                    site.name
                ),
                "rename the duplicate workpiece site".to_string(),
            ));
            continue;
        }
        catalog
            .site_kinds
            .insert(site.name.clone(), site.kind.clone());
        constraint_set.workpiece_sites.push(IrWorkpieceSiteDef {
            name: site.name.clone(),
            kind: map_workpiece_site_kind(&site.kind),
            capacity: site.capacity,
        });
    }

    for holder in &topology.workpiece_holders {
        if !seen_endpoints.insert(holder.name.clone()) {
            errors.push(PlcError::semantic_with_reason(
                holder.line.max(1),
                format!(
                    "workpiece endpoint '{}' is declared more than once",
                    holder.name
                ),
                "rename the duplicate workpiece holder".to_string(),
            ));
            continue;
        }
        catalog.holders.insert(holder.name.clone());
        constraint_set.workpiece_holders.push(IrWorkpieceHolderDef {
            name: holder.name.clone(),
            capacity: holder.capacity,
        });
    }

    for carrier in &topology.workpiece_carriers {
        if !seen_endpoints.insert(carrier.name.clone()) {
            errors.push(PlcError::semantic_with_reason(
                carrier.line.max(1),
                format!(
                    "workpiece endpoint '{}' is declared more than once",
                    carrier.name
                ),
                "rename the duplicate workpiece carrier".to_string(),
            ));
            continue;
        }
        let shape = carrier_shape_from_ast(&carrier.layout);
        catalog.carriers.insert(carrier.name.clone(), shape.clone());
        constraint_set
            .workpiece_carriers
            .push(IrWorkpieceCarrierDef {
                name: carrier.name.clone(),
                layout: map_workpiece_carrier_layout(&carrier.layout),
            });
    }

    for workpiece in &topology.workpiece_types {
        if !seen_workpiece_types.insert(workpiece.name.clone()) {
            errors.push(PlcError::semantic_with_reason(
                workpiece.line.max(1),
                format!(
                    "workpiece type '{}' is declared more than once",
                    workpiece.name
                ),
                "merge or rename the duplicate workpiece type".to_string(),
            ));
            continue;
        }

        validate_workpiece_type_declaration_v2(workpiece, &catalog, &declared_type_names, errors);
        constraint_set.workpiece_types.push(IrWorkpieceTypeDef {
            name: workpiece.name.clone(),
            properties: workpiece
                .properties
                .iter()
                .map(|property| IrWorkpiecePropertyDef {
                    name: property.name.clone(),
                    property_type: map_workpiece_property_type(&property.property_type),
                })
                .collect(),
            normal_terminal_states: workpiece.normal_terminal_states.clone(),
            abnormal_terminal_states: workpiece.abnormal_terminal_states.clone(),
            ingress_sites: workpiece.ingress_sites.clone(),
            normal_egress_sites: workpiece.normal_egress_sites.clone(),
            abnormal_egress_sites: workpiece.abnormal_egress_sites.clone(),
            allows: workpiece.allows.iter().map(map_workpiece_allow).collect(),
            derived_from: workpiece
                .derived_from
                .iter()
                .map(map_workpiece_derivation)
                .collect(),
        });
    }

    validate_workpiece_type_contract_alignment(&topology.workpiece_types, errors);

    catalog
}

fn validate_workpiece_type_contract_alignment(
    workpieces: &[AstWorkpieceTypeDeclaration],
    errors: &mut Vec<PlcError>,
) {
    let index = workpieces
        .iter()
        .map(|workpiece| (workpiece.name.clone(), workpiece))
        .collect::<HashMap<_, _>>();

    for workpiece in workpieces {
        for allow in &workpiece.allows {
            let AstWorkpieceAllowDeclaration::SplitInto { target } = allow;
            let Some(target_def) = index.get(target) else {
                continue;
            };
            let has_counterpart = target_def.derived_from.iter().any(|rule| {
                matches!(
                    rule,
                    AstWorkpieceDerivationDeclaration::WorkpieceType { workpiece_type }
                        if workpiece_type == &workpiece.name
                )
            });
            if !has_counterpart {
                errors.push(PlcError::semantic_with_reason(
                    workpiece.line.max(1),
                    format!(
                        "workpiece type '{}' declares split_into({}), but target type '{}' is missing derived_from({})",
                        workpiece.name, target, target, workpiece.name
                    ),
                    "declare the matching derived_from(...) on the split target workpiece type"
                        .to_string(),
                ));
            }
        }

        for derivation in &workpiece.derived_from {
            let AstWorkpieceDerivationDeclaration::WorkpieceType { workpiece_type } = derivation
            else {
                continue;
            };
            let Some(source_def) = index.get(workpiece_type) else {
                continue;
            };
            let has_counterpart = source_def.allows.iter().any(|allow| {
                matches!(
                    allow,
                    AstWorkpieceAllowDeclaration::SplitInto { target }
                        if target == &workpiece.name
                )
            });
            if !has_counterpart {
                errors.push(PlcError::semantic_with_reason(
                    workpiece.line.max(1),
                    format!(
                        "workpiece type '{}' declares derived_from({}), but source type '{}' is missing split_into({})",
                        workpiece.name, workpiece_type, workpiece_type, workpiece.name
                    ),
                    "declare the matching split_into(...) on the source workpiece type"
                        .to_string(),
                ));
            }
        }
    }
}

fn carrier_shape_from_ast(layout: &AstWorkpieceCarrierLayout) -> CarrierShape {
    match layout {
        AstWorkpieceCarrierLayout::Slots { count } => CarrierShape {
            dimensions: vec![*count],
        },
        AstWorkpieceCarrierLayout::Grid { rows, cols } => CarrierShape {
            dimensions: vec![*rows, *cols],
        },
    }
}

fn map_workpiece_carrier_layout(layout: &AstWorkpieceCarrierLayout) -> IrWorkpieceCarrierLayoutDef {
    match layout {
        AstWorkpieceCarrierLayout::Slots { count } => {
            IrWorkpieceCarrierLayoutDef::Slots { count: *count }
        }
        AstWorkpieceCarrierLayout::Grid { rows, cols } => IrWorkpieceCarrierLayoutDef::Grid {
            rows: *rows,
            cols: *cols,
        },
    }
}

fn map_workpiece_allow(allow: &AstWorkpieceAllowDeclaration) -> IrWorkpieceAllowDef {
    match allow {
        AstWorkpieceAllowDeclaration::SplitInto { target } => IrWorkpieceAllowDef::SplitInto {
            target: target.clone(),
        },
    }
}

fn map_workpiece_derivation(
    derivation: &AstWorkpieceDerivationDeclaration,
) -> IrWorkpieceDerivationDef {
    match derivation {
        AstWorkpieceDerivationDeclaration::WorkpieceType { workpiece_type } => {
            IrWorkpieceDerivationDef::WorkpieceType {
                workpiece_type: workpiece_type.clone(),
            }
        }
        AstWorkpieceDerivationDeclaration::Merge { inputs } => IrWorkpieceDerivationDef::Merge {
            inputs: inputs.clone(),
        },
    }
}

fn validate_workpiece_type_declaration_v2(
    workpiece: &AstWorkpieceTypeDeclaration,
    catalog: &WorkpieceCatalog,
    declared_type_names: &HashSet<String>,
    errors: &mut Vec<PlcError>,
) {
    validate_workpiece_type_declaration(workpiece, catalog, errors);

    for allow in &workpiece.allows {
        match allow {
            AstWorkpieceAllowDeclaration::SplitInto { target } => {
                if !declared_type_names.contains(target) {
                    errors.push(PlcError::undefined_reference_with_reason(
                        workpiece.line.max(1),
                        "workpiece_type",
                        target,
                        "declare the target workpiece type before using split_into".to_string(),
                    ));
                }
            }
        }
    }
    let allow_rules = workpiece
        .allows
        .iter()
        .map(|allow| match allow {
            AstWorkpieceAllowDeclaration::SplitInto { target } => {
                format!("split_into({target})")
            }
        })
        .collect::<Vec<_>>();
    validate_unique_workpiece_rule_entries(
        workpiece.line.max(1),
        &workpiece.name,
        "split_into",
        &allow_rules,
        errors,
    );

    for derivation in &workpiece.derived_from {
        match derivation {
            AstWorkpieceDerivationDeclaration::WorkpieceType { workpiece_type } => {
                if !declared_type_names.contains(workpiece_type) {
                    errors.push(PlcError::undefined_reference_with_reason(
                        workpiece.line.max(1),
                        "workpiece_type",
                        workpiece_type,
                        "declare the source workpiece type before using derived_from".to_string(),
                    ));
                }
            }
            AstWorkpieceDerivationDeclaration::Merge { inputs } => {
                if inputs.len() < 2 {
                    errors.push(PlcError::semantic_with_reason(
                        workpiece.line.max(1),
                        format!(
                            "workpiece type '{}' merge derivation needs at least two inputs",
                            workpiece.name
                        ),
                        "declare two or more source workpiece types in merge(...)".to_string(),
                    ));
                }
                for input in inputs {
                    if !declared_type_names.contains(input) {
                        errors.push(PlcError::undefined_reference_with_reason(
                            workpiece.line.max(1),
                            "workpiece_type",
                            input,
                            "declare each merge input workpiece type first".to_string(),
                        ));
                    }
                }
            }
        }
    }
    let derivation_rules = workpiece
        .derived_from
        .iter()
        .map(|rule| match rule {
            AstWorkpieceDerivationDeclaration::WorkpieceType { workpiece_type } => {
                format!("derived_from({workpiece_type})")
            }
            AstWorkpieceDerivationDeclaration::Merge { inputs } => {
                let mut normalized = inputs.clone();
                normalized.sort();
                format!("merge({})", normalized.join(", "))
            }
        })
        .collect::<Vec<_>>();
    validate_unique_workpiece_rule_entries(
        workpiece.line.max(1),
        &workpiece.name,
        "derived_from",
        &derivation_rules,
        errors,
    );
    validate_unambiguous_workpiece_merge_derivations(workpiece, errors);

    for site in workpiece
        .ingress_sites
        .iter()
        .chain(workpiece.normal_egress_sites.iter())
        .chain(workpiece.abnormal_egress_sites.iter())
    {
        validate_workpiece_contract_reference_v2(workpiece.line.max(1), site, catalog, errors);
    }
}

fn validate_unambiguous_workpiece_merge_derivations(
    workpiece: &AstWorkpieceTypeDeclaration,
    errors: &mut Vec<PlcError>,
) {
    let mut seen_by_arity = HashMap::<usize, String>::new();
    for derivation in &workpiece.derived_from {
        let AstWorkpieceDerivationDeclaration::Merge { inputs } = derivation else {
            continue;
        };
        let mut normalized = inputs.clone();
        normalized.sort();
        let rule = format!("merge({})", normalized.join(", "));
        match seen_by_arity.entry(inputs.len()) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(rule);
            }
            std::collections::hash_map::Entry::Occupied(entry) => {
                if entry.get() == &rule {
                    continue;
                }
                errors.push(PlcError::semantic_with_reason(
                    workpiece.line.max(1),
                    format!(
                        "workpiece type '{}' declares multiple merge(...) derivations with {} inputs",
                        workpiece.name,
                        inputs.len()
                    ),
                    "keep at most one merge(...) derivation per input arity in WPM v1"
                        .to_string(),
                ));
            }
        }
    }
}

fn validate_workpiece_contract_reference_v2(
    line: usize,
    endpoint: &str,
    catalog: &WorkpieceCatalog,
    errors: &mut Vec<PlcError>,
) {
    if let Some((carrier, selectors)) = parse_workpiece_slot_reference(endpoint) {
        validate_carrier_slot_reference(line, &carrier, &selectors, true, catalog, errors);
        return;
    }

    match catalog.site_kinds.get(endpoint) {
        Some(AstWorkpieceSiteKind::WorkpieceLocation) => {}
        Some(AstWorkpieceSiteKind::CarrierLocation) => errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "carrier_location '{}' cannot be used as a workpiece ingress/egress",
                endpoint
            ),
            "use a concrete carrier slot or a workpiece_location".to_string(),
        )),
        None => errors.push(PlcError::undefined_reference_with_reason(
            line,
            "workpiece_endpoint",
            endpoint,
            "declare the location or carrier slot in [topology]".to_string(),
        )),
    }
}

fn validate_workpiece_place_reference_v2(
    line: usize,
    endpoint: &str,
    catalog: &WorkpieceCatalog,
    errors: &mut Vec<PlcError>,
) {
    if let Some((carrier, selectors)) = parse_workpiece_slot_reference(endpoint) {
        validate_carrier_slot_reference(line, &carrier, &selectors, false, catalog, errors);
        return;
    }
    validate_workpiece_location_reference(line, endpoint, catalog, errors);
}

fn validate_workpiece_endpoint_reference_v2(
    line: usize,
    endpoint: &str,
    catalog: &WorkpieceCatalog,
    errors: &mut Vec<PlcError>,
) {
    if catalog.holders.contains(endpoint) {
        return;
    }
    validate_workpiece_place_reference_v2(line, endpoint, catalog, errors);
}

fn validate_carrier_slot_reference(
    line: usize,
    carrier: &str,
    selectors: &[String],
    allow_wildcards: bool,
    catalog: &WorkpieceCatalog,
    errors: &mut Vec<PlcError>,
) {
    let Some(shape) = catalog.carriers.get(carrier) else {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            "workpiece_carrier",
            carrier,
            "declare the carrier before using carrier.slot[...]".to_string(),
        ));
        return;
    };

    if selectors.len() != shape.dimensions.len() {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "carrier '{}' expects {} slot dimensions, but '{}' provides {}",
                carrier,
                shape.dimensions.len(),
                format_slot_reference(carrier, selectors),
                selectors.len()
            ),
            "match the slot index arity to the carrier declaration".to_string(),
        ));
        return;
    }

    for (idx, selector) in selectors.iter().enumerate() {
        if selector == "*" {
            if !allow_wildcards {
                errors.push(PlcError::semantic_with_reason(
                    line,
                    format!(
                        "wildcard slot reference '{}' is not allowed in runtime effects",
                        format_slot_reference(carrier, selectors)
                    ),
                    "use a concrete slot index in effect statements".to_string(),
                ));
            }
            continue;
        }
        if let Ok(value) = selector.parse::<u32>() {
            if value >= shape.dimensions[idx] {
                errors.push(PlcError::semantic_with_reason(
                    line,
                    format!(
                        "slot index {} is out of range for carrier '{}' dimension {}",
                        value, carrier, idx
                    ),
                    "keep slot indices within the declared carrier bounds".to_string(),
                ));
            }
        }
    }
}

fn parse_workpiece_slot_reference(raw: &str) -> Option<(String, Vec<String>)> {
    let (carrier, rest) = raw.split_once(".slot[")?;
    let selectors = rest.strip_suffix(']')?;
    let parts = selectors
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if carrier.is_empty() || parts.is_empty() {
        return None;
    }
    Some((carrier.to_string(), parts))
}

fn format_slot_reference(carrier: &str, selectors: &[String]) -> String {
    format!("{}.slot[{}]", carrier, selectors.join(", "))
}

fn validate_workpiece_effects_in_tasks_v2(
    tasks: &TasksSection,
    catalog: &WorkpieceCatalog,
    workpiece_types: &[IrWorkpieceTypeDef],
    errors: &mut Vec<PlcError>,
) {
    if workpiece_types.is_empty() {
        errors.push(PlcError::semantic_with_reason(
            1,
            "workpiece effects require at least one workpiece type".to_string(),
            "declare a workpiece type in [topology] before using effect statements".to_string(),
        ));
        return;
    }

    for task in &tasks.tasks {
        for step in &task.steps {
            validate_workpiece_effects_in_statements_v2(
                &step.statements,
                catalog,
                workpiece_types,
                errors,
            );
        }
    }
}

fn validate_workpiece_effects_in_statements_v2(
    statements: &[StepStatement],
    catalog: &WorkpieceCatalog,
    workpiece_types: &[IrWorkpieceTypeDef],
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Effect(effect) => match &effect.kind {
                AstEffectKind::Acquire { holder, from } => {
                    validate_holder_reference(effect.line.max(1), holder, catalog, errors);
                    validate_workpiece_place_reference_v2(
                        effect.line.max(1),
                        from,
                        catalog,
                        errors,
                    );
                    if workpiece_types.len() != 1 {
                        errors.push(PlcError::semantic_with_reason(
                            effect.line.max(1),
                            "acquire/transfer/finish effects remain single-type in this phase"
                                .to_string(),
                            "use one workpiece type per flow when relying on untyped transfer effects"
                                .to_string(),
                        ));
                    }
                }
                AstEffectKind::Transfer { from, to } => {
                    validate_workpiece_endpoint_reference_v2(
                        effect.line.max(1),
                        from,
                        catalog,
                        errors,
                    );
                    validate_workpiece_endpoint_reference_v2(
                        effect.line.max(1),
                        to,
                        catalog,
                        errors,
                    );
                    if workpiece_types.len() != 1 {
                        errors.push(PlcError::semantic_with_reason(
                            effect.line.max(1),
                            "acquire/transfer/finish effects remain single-type in this phase"
                                .to_string(),
                            "use one workpiece type per flow when relying on untyped transfer effects"
                                .to_string(),
                        ));
                    }
                }
                AstEffectKind::Finish { at, terminal_state } => {
                    validate_workpiece_place_reference_v2(effect.line.max(1), at, catalog, errors);
                    if workpiece_types.len() != 1 {
                        errors.push(PlcError::semantic_with_reason(
                            effect.line.max(1),
                            "finish remains single-type in this phase".to_string(),
                            "use one workpiece type per flow when relying on untyped finish"
                                .to_string(),
                        ));
                        continue;
                    }
                    let workpiece = &workpiece_types[0];
                    let normal = workpiece
                        .normal_terminal_states
                        .iter()
                        .any(|state| state == terminal_state);
                    let abnormal = workpiece
                        .abnormal_terminal_states
                        .iter()
                        .any(|state| state == terminal_state);
                    if !normal && !abnormal {
                        errors.push(PlcError::semantic_with_reason(
                            effect.line.max(1),
                            format!(
                                "terminal state '{}' is not declared on workpiece type '{}'",
                                terminal_state, workpiece.name
                            ),
                            "declare the terminal state before using finish".to_string(),
                        ));
                    }
                    let candidates = if normal {
                        &workpiece.normal_egress_sites
                    } else {
                        &workpiece.abnormal_egress_sites
                    };
                    if (normal || abnormal)
                        && !candidates
                            .iter()
                            .any(|candidate| workpiece_endpoint_matches_pattern(at, candidate))
                    {
                        errors.push(PlcError::semantic_with_reason(
                            effect.line.max(1),
                            format!("finish endpoint '{}' does not satisfy the declared egress contract", at),
                            "finish at a declared egress location or carrier slot".to_string(),
                        ));
                    }
                }
                AstEffectKind::Mount {
                    workpiece_type,
                    slot,
                } => {
                    validate_declared_workpiece_type(
                        effect.line.max(1),
                        workpiece_type,
                        workpiece_types,
                        errors,
                    );
                    validate_workpiece_place_reference_v2(
                        effect.line.max(1),
                        slot,
                        catalog,
                        errors,
                    );
                }
                AstEffectKind::Unmount {
                    workpiece_type,
                    slot,
                    to,
                } => {
                    validate_declared_workpiece_type(
                        effect.line.max(1),
                        workpiece_type,
                        workpiece_types,
                        errors,
                    );
                    validate_workpiece_place_reference_v2(
                        effect.line.max(1),
                        slot,
                        catalog,
                        errors,
                    );
                    validate_workpiece_place_reference_v2(effect.line.max(1), to, catalog, errors);
                }
                AstEffectKind::Split {
                    source_type,
                    target_type,
                    count,
                    consumed: _,
                } => {
                    validate_declared_workpiece_type(
                        effect.line.max(1),
                        source_type,
                        workpiece_types,
                        errors,
                    );
                    validate_declared_workpiece_type(
                        effect.line.max(1),
                        target_type,
                        workpiece_types,
                        errors,
                    );
                    if *count == 0 {
                        errors.push(PlcError::semantic_with_reason(
                            effect.line.max(1),
                            "split count must be greater than zero".to_string(),
                            "use a finite positive count for split".to_string(),
                        ));
                    }
                    if let Some(source_def) = find_workpiece_type(workpiece_types, source_type) {
                        let allowed = source_def.allows.iter().any(|allow| {
                            matches!(allow, IrWorkpieceAllowDef::SplitInto { target } if target == target_type)
                        });
                        if !allowed {
                            errors.push(PlcError::semantic_with_reason(
                                effect.line.max(1),
                                format!(
                                    "workpiece type '{}' does not allow split_into({})",
                                    source_type, target_type
                                ),
                                "declare split_into(...) on the source workpiece type".to_string(),
                            ));
                        }
                    }
                    if let Some(target_def) = find_workpiece_type(workpiece_types, target_type) {
                        let derived = target_def.derived_from.iter().any(|rule| {
                            matches!(
                                rule,
                                IrWorkpieceDerivationDef::WorkpieceType { workpiece_type } if workpiece_type == source_type
                            )
                        });
                        if !derived {
                            errors.push(PlcError::semantic_with_reason(
                                effect.line.max(1),
                                format!(
                                    "workpiece type '{}' is not derived_from '{}'",
                                    target_type, source_type
                                ),
                                "declare derived_from on the split output workpiece type"
                                    .to_string(),
                            ));
                        }
                    }
                }
                AstEffectKind::Merge {
                    inputs,
                    target_type,
                    consumed_inputs: _,
                } => {
                    validate_declared_workpiece_type(
                        effect.line.max(1),
                        target_type,
                        workpiece_types,
                        errors,
                    );
                    if inputs.len() < 2 {
                        errors.push(PlcError::semantic_with_reason(
                            effect.line.max(1),
                            "merge requires at least two inputs".to_string(),
                            "list two or more explicit merge inputs".to_string(),
                        ));
                    }
                    if let Some(target_def) = find_workpiece_type(workpiece_types, target_type) {
                        let matches_merge = target_def.derived_from.iter().any(|rule| {
                            matches!(rule, IrWorkpieceDerivationDef::Merge { inputs: expected } if expected.len() == inputs.len())
                        });
                        if !matches_merge {
                            errors.push(PlcError::semantic_with_reason(
                                effect.line.max(1),
                                format!(
                                    "workpiece type '{}' has no merge(...) derivation matching {} inputs",
                                    target_type,
                                    inputs.len()
                                ),
                                "declare a merge(...) derivation on the target workpiece type".to_string(),
                            ));
                        }
                    }
                }
                AstEffectKind::TransformCarrier { carrier, .. } => {
                    if !catalog.carriers.contains_key(carrier) {
                        errors.push(PlcError::undefined_reference_with_reason(
                            effect.line.max(1),
                            "workpiece_carrier",
                            carrier,
                            "declare the carrier before transforming it".to_string(),
                        ));
                    }
                }
            },
            StepStatement::Repeat { body, .. } => {
                validate_workpiece_effects_in_statements_v2(body, catalog, workpiece_types, errors)
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_workpiece_effects_in_statements_v2(
                        &branch.statements,
                        catalog,
                        workpiece_types,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_workpiece_effects_in_statements_v2(
                        &branch.statements,
                        catalog,
                        workpiece_types,
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
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
}

fn validate_declared_workpiece_type(
    line: usize,
    name: &str,
    workpiece_types: &[IrWorkpieceTypeDef],
    errors: &mut Vec<PlcError>,
) {
    if find_workpiece_type(workpiece_types, name).is_none() {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            "workpiece_type",
            name,
            "declare the workpiece type in [topology] first".to_string(),
        ));
    }
}

fn find_workpiece_type<'a>(
    workpiece_types: &'a [IrWorkpieceTypeDef],
    name: &str,
) -> Option<&'a IrWorkpieceTypeDef> {
    workpiece_types
        .iter()
        .find(|workpiece| workpiece.name == name)
}

fn workpiece_endpoint_matches_pattern(endpoint: &str, pattern: &str) -> bool {
    if endpoint == pattern {
        return true;
    }
    let Some((endpoint_carrier, endpoint_selectors)) = parse_workpiece_slot_reference(endpoint)
    else {
        return false;
    };
    let Some((pattern_carrier, pattern_selectors)) = parse_workpiece_slot_reference(pattern) else {
        return false;
    };
    if endpoint_carrier != pattern_carrier || endpoint_selectors.len() != pattern_selectors.len() {
        return false;
    }
    endpoint_selectors
        .iter()
        .zip(pattern_selectors.iter())
        .all(|(value, pattern)| pattern == "*" || value == pattern)
}

pub fn build_timing_model_from_ast(
    topology: &TopologySection,
    tasks: &TasksSection,
) -> Result<TimingModel, Vec<PlcError>> {
    let device_profiles = collect_device_timing_profiles(topology);
    let mut intervals = BTreeMap::new();
    let mut errors = Vec::new();

    for task in &tasks.tasks {
        for step in &task.steps {
            let mut actions = Vec::new();
            collect_actions(&step.statements, &mut actions);

            for action in actions {
                if let Some(action_timing) = action_to_timing(
                    &task.name,
                    &step.name,
                    step.line,
                    &action,
                    &device_profiles,
                    &mut errors,
                ) {
                    insert_action_timing(&mut intervals, action_timing);
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(TimingModel { intervals })
    } else {
        Err(errors)
    }
}

