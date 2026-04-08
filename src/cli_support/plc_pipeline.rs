use runtime_core::Program;
use rust_plc::ast::{PlcProgram, StepStatement, TasksSection};
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
use serde::Deserialize;
use std::fs;
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

const PROJECT_MANIFEST_FILE: &str = "rustplc.project.toml";
const PROJECT_WORKPIECE_POLICY_FILE: &str = "config/workpiece.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProjectWorkpiecePolicy {
    required: bool,
}

#[derive(Debug, Deserialize)]
struct WorkpiecePolicyFile {
    #[serde(default = "default_workpiece_policy_schema_version")]
    schema_version: u32,
    #[serde(default)]
    workpiece: WorkpiecePolicySection,
}

#[derive(Debug, Deserialize)]
struct WorkpiecePolicySection {
    #[serde(default = "default_workpiece_required")]
    required: bool,
}

impl Default for WorkpiecePolicySection {
    fn default() -> Self {
        Self {
            required: default_workpiece_required(),
        }
    }
}

fn default_workpiece_required() -> bool {
    true
}

fn default_workpiece_policy_schema_version() -> u32 {
    1
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
    enforce_project_workpiece_policy(&program, input.requested_path.as_path())?;
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

pub(crate) fn build_loaded_runtime_semantics(
    input: &LoadedPlcSource,
) -> Result<RuntimeSemantics, String> {
    let program = parse_loaded_plc_with_required_purpose(input)?;
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
    .map_err(|errors| format_loaded_plc_errors(errors, input).join("\n"))?;
    validate_topology_semantics(&expanded.topology)
        .map_err(|gate_error| format_topology_gate_error(gate_error, input))?;

    let mut errors = Vec::<PlcError>::new();
    let topology = collect_stage(build_topology_graph(&expanded), &mut errors);
    let state_machine = collect_stage(build_state_machine(&expanded), &mut errors);
    let constraints = collect_stage(build_constraint_set(&expanded), &mut errors);

    if !errors.is_empty() {
        return Err(format_loaded_plc_errors(errors, input).join("\n"));
    }

    Ok(RuntimeSemantics {
        topology: topology.expect("topology exists when semantic errors are empty"),
        state_machine: state_machine.expect("state machine exists when semantic errors are empty"),
        constraints: constraints.expect("constraints exist when semantic errors are empty"),
    })
}

pub(crate) fn compile_loaded_plc_to_runtime_program(
    input: &LoadedPlcSource,
    tick_ms: u64,
) -> Result<Program<'static>, String> {
    let semantics = build_loaded_runtime_semantics(input)?;
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

fn enforce_project_workpiece_policy(program: &PlcProgram, source_path: &Path) -> Result<(), String> {
    let Some(project_root) = find_project_root(source_path) else {
        return Ok(());
    };

    let policy = load_project_workpiece_policy(&project_root)?;
    if !policy.required {
        return Ok(());
    }

    if program.topology.workpiece_types.is_empty() {
        return Err(format!(
            "Project workpiece policy requires first-class workpiece semantics, but no `workpiece ...: workpiece_type` declaration was found.\n\
Project: {}\n\
Policy: {} (required=true)\n\
Fix: declare at least one workpiece type and add matching workpiece flow effects in tasks, or set `required = false` only for a deliberate no-workpiece exception.",
            project_root.join(PROJECT_MANIFEST_FILE).display(),
            project_root.join(PROJECT_WORKPIECE_POLICY_FILE).display(),
        ));
    }

    if !tasks_use_workpiece_effects(&program.tasks) {
        return Err(format!(
            "Project workpiece policy requires first-class workpiece semantics, but no task uses any `effect:` statement.\n\
Project: {}\n\
Policy: {} (required=true)\n\
Fix: add `effect: acquire`, `effect: transfer`, `effect: finish`, or other workpiece effects to the real task steps that move or terminate the part.",
            project_root.join(PROJECT_MANIFEST_FILE).display(),
            project_root.join(PROJECT_WORKPIECE_POLICY_FILE).display(),
        ));
    }

    Ok(())
}

fn find_project_root(source_path: &Path) -> Option<std::path::PathBuf> {
    let start = if source_path.is_dir() {
        source_path
    } else {
        source_path.parent()?
    };
    for dir in start.ancestors() {
        if dir.join(PROJECT_MANIFEST_FILE).exists() {
            return Some(dir.to_path_buf());
        }
    }
    None
}

fn load_project_workpiece_policy(project_root: &Path) -> Result<ProjectWorkpiecePolicy, String> {
    let policy_path = project_root.join(PROJECT_WORKPIECE_POLICY_FILE);
    if !policy_path.exists() {
        return Ok(ProjectWorkpiecePolicy { required: true });
    }

    let text = fs::read_to_string(&policy_path)
        .map_err(|err| format!("Failed to read {}: {err}", policy_path.display()))?;
    let parsed: WorkpiecePolicyFile = toml::from_str(&text)
        .map_err(|err| format!("Failed to parse {}: {err}", policy_path.display()))?;
    if parsed.schema_version != 1 {
        return Err(format!(
            "Unsupported workpiece policy schema_version {} in {} (expected 1)",
            parsed.schema_version,
            policy_path.display()
        ));
    }
    Ok(ProjectWorkpiecePolicy {
        required: parsed.workpiece.required,
    })
}

fn tasks_use_workpiece_effects(tasks: &TasksSection) -> bool {
    tasks.tasks.iter().any(|task| {
        task.steps
            .iter()
            .any(|step| statements_use_workpiece_effects(&step.statements))
    })
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
        _ => false,
    })
}
