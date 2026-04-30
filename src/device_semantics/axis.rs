use super::{DeviceActionContract, DeviceActionResultBucket, device_kind_name};
use crate::ast::{
    ActionStatement, AxisFaultRouteDirective as AstAxisFaultRouteDirective,
    AxisFaultRouteKind as AstAxisFaultRouteKind, ComparisonOperator, DeviceType, LiteralValue,
    StepStatement, TasksSection, TopologySection, WaitCondition, WaitStatement,
};
use crate::axis_profile::resolve_axis_profiles;
use crate::error::PlcError;
use crate::ir::{AxisBrakeConfig, AxisOrientation, BinaryValue as IrBinaryValue, DeviceKind};
use rustplc_device_semantics::axis::AxisFaultRouteKind;
pub use rustplc_device_semantics::axis::{
    DEFAULT_PORT, DEFAULT_REQUIRE_HOMED, FAMILY, MOVE_ABSOLUTE_ACTION, MOVE_RELATIVE_ACTION,
};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;

const AXIS_MOTION_PARAM_SETS_DIR: &str = "axis_motion_param_sets";

pub const AXIS_MOTION_CAPABILITY: DeviceActionContract<'static> = DeviceActionContract {
    family: FAMILY,
    action: "move_*",
    result_buckets: &[
        DeviceActionResultBucket::Complete,
        DeviceActionResultBucket::Timeout,
        DeviceActionResultBucket::Reject,
        DeviceActionResultBucket::MotionFault,
        DeviceActionResultBucket::SafetyFault,
    ],
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisMotionBranchKind {
    Timeout,
    Reject,
    MotionFault,
    SafetyFault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxisMotionRequiredBranch {
    pub kind: AxisMotionBranchKind,
    pub name: &'static str,
    pub diagnostic_code: &'static str,
    pub fix: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxisFaultRouteBucket {
    pub branch_name: &'static str,
    pub allowed_kinds: &'static [AxisFaultRouteKind],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxisMotionActionContract {
    pub family: &'static str,
    pub relative_action: &'static str,
    pub absolute_action: &'static str,
    pub default_port: &'static str,
    pub default_require_homed: bool,
    pub required_branches: &'static [AxisMotionRequiredBranch],
    pub route_buckets: &'static [AxisFaultRouteBucket],
}

const AXIS_REJECT_ROUTE_KINDS: &[AxisFaultRouteKind] =
    &[AxisFaultRouteKind::Reject, AxisFaultRouteKind::Vendor];
const AXIS_MOTION_FAULT_ROUTE_KINDS: &[AxisFaultRouteKind] =
    &[AxisFaultRouteKind::Motion, AxisFaultRouteKind::Vendor];
const AXIS_SAFETY_FAULT_ROUTE_KINDS: &[AxisFaultRouteKind] =
    &[AxisFaultRouteKind::Safety, AxisFaultRouteKind::Vendor];

pub const AXIS_MOTION_REQUIRED_BRANCHES: &[AxisMotionRequiredBranch] = &[
    AxisMotionRequiredBranch {
        kind: AxisMotionBranchKind::Timeout,
        name: "timeout",
        diagnostic_code: "AXIS-001",
        fix: "Add timeout: <duration> -> <task.step> branch.",
    },
    AxisMotionRequiredBranch {
        kind: AxisMotionBranchKind::Reject,
        name: "on_reject",
        diagnostic_code: "AXIS-002",
        fix: "Add on_reject -> <task.step> branch.",
    },
    AxisMotionRequiredBranch {
        kind: AxisMotionBranchKind::MotionFault,
        name: "on_motion_fault",
        diagnostic_code: "AXIS-003",
        fix: "Add on_motion_fault -> <task.step> branch.",
    },
    AxisMotionRequiredBranch {
        kind: AxisMotionBranchKind::SafetyFault,
        name: "on_safety_fault",
        diagnostic_code: "AXIS-004",
        fix: "Add on_safety_fault -> <task.step> branch.",
    },
];

pub const AXIS_FAULT_ROUTE_BUCKETS: &[AxisFaultRouteBucket] = &[
    AxisFaultRouteBucket {
        branch_name: "on_reject",
        allowed_kinds: AXIS_REJECT_ROUTE_KINDS,
    },
    AxisFaultRouteBucket {
        branch_name: "on_motion_fault",
        allowed_kinds: AXIS_MOTION_FAULT_ROUTE_KINDS,
    },
    AxisFaultRouteBucket {
        branch_name: "on_safety_fault",
        allowed_kinds: AXIS_SAFETY_FAULT_ROUTE_KINDS,
    },
];

pub const AXIS_MOTION_ACTION_CONTRACT: AxisMotionActionContract = AxisMotionActionContract {
    family: FAMILY,
    relative_action: MOVE_RELATIVE_ACTION,
    absolute_action: MOVE_ABSOLUTE_ACTION,
    default_port: DEFAULT_PORT,
    default_require_homed: DEFAULT_REQUIRE_HOMED,
    required_branches: AXIS_MOTION_REQUIRED_BRANCHES,
    route_buckets: AXIS_FAULT_ROUTE_BUCKETS,
};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AxisMotionParamSetDef {
    name: String,
    config_id: String,
    speed: f64,
    acceleration: f64,
    deceleration: f64,
}

#[derive(Debug, Clone, Copy, Default)]
struct BrakeSequenceProgress {
    engage_seen: bool,
    confirm_seen: bool,
}

pub fn validate_axis_motion_actions_in_tasks(
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

pub fn resolve_axis_motion_parameters_in_tasks(
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

pub fn validate_vertical_axis_brake_sequence_in_tasks(
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
            if matches!(profile.orientation, AxisOrientation::Vertical) {
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
                for branch in AXIS_MOTION_ACTION_CONTRACT.required_branches {
                    let present = match branch.kind {
                        AxisMotionBranchKind::Timeout => timeout.is_some(),
                        AxisMotionBranchKind::Reject => on_reject.is_some(),
                        AxisMotionBranchKind::MotionFault => on_motion_fault.is_some(),
                        AxisMotionBranchKind::SafetyFault => on_safety_fault.is_some(),
                    };
                    if !present {
                        errors.push(axis_motion_branch_error(
                            line,
                            branch.diagnostic_code,
                            step_name,
                            branch.name,
                            branch.fix,
                        ));
                    }
                }

                for bucket in AXIS_MOTION_ACTION_CONTRACT.route_buckets {
                    let routes: &[AstAxisFaultRouteDirective] = match bucket.branch_name {
                        "on_reject" => on_reject_routes.as_slice(),
                        "on_motion_fault" => on_motion_fault_routes.as_slice(),
                        "on_safety_fault" => on_safety_fault_routes.as_slice(),
                        _ => &[],
                    };
                    validate_axis_fault_routes(
                        line,
                        step_name,
                        bucket.branch_name,
                        routes,
                        bucket.allowed_kinds,
                        errors,
                    );
                }
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
    allowed_kinds: &[AxisFaultRouteKind],
    errors: &mut Vec<PlcError>,
) {
    for route in routes {
        if let Some(kind) = route.kind {
            let kind = axis_fault_route_kind_from_ast(kind);
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

fn axis_fault_route_kind_from_ast(kind: AstAxisFaultRouteKind) -> AxisFaultRouteKind {
    match kind {
        AstAxisFaultRouteKind::Reject => AxisFaultRouteKind::Reject,
        AstAxisFaultRouteKind::Motion => AxisFaultRouteKind::Motion,
        AstAxisFaultRouteKind::Safety => AxisFaultRouteKind::Safety,
        AstAxisFaultRouteKind::Vendor => AxisFaultRouteKind::Vendor,
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
                    "请调整 position 或更新轴配置 soft_limit_min/soft_limit_max。".to_string(),
                ));
                return;
            }
        }
    }

    *speed = Some(resolved_speed);
    *acceleration = Some(resolved_acc);
    *deceleration = Some(resolved_dec);
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
                    && set_action_value_to_binary(value.as_str()) == Some(IrBinaryValue::Off)
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
    brake_requirements: &HashMap<String, AxisBrakeConfig>,
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
                    && set_action_value_to_binary(value.as_str())
                        == Some(brake.engage_value.clone())
                {
                    if let Some(state) = progress.get_mut(&target.device) {
                        state.engage_seen = true;
                        state.confirm_seen = false;
                    }
                    continue;
                }

                if target.port == "enable"
                    && set_action_value_to_binary(value.as_str()) == Some(IrBinaryValue::Off)
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

fn wait_asserts_brake_confirmed(wait: &WaitStatement, axis: &str, brake: &AxisBrakeConfig) -> bool {
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

fn set_action_value_to_binary(value: &str) -> Option<IrBinaryValue> {
    match value {
        "on" | "forward" | "active" => Some(IrBinaryValue::On),
        "off" | "reverse" | "idle" => Some(IrBinaryValue::Off),
        _ => None,
    }
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

#[cfg(test)]
mod tests {
    use super::{AXIS_MOTION_ACTION_CONTRACT, AXIS_MOTION_CAPABILITY, AxisMotionBranchKind};
    use crate::device_semantics::DeviceActionResultBucket;
    use rustplc_device_semantics::axis::{
        DEFAULT_PORT, DEFAULT_REQUIRE_HOMED, FAMILY, MOVE_ABSOLUTE_ACTION, MOVE_RELATIVE_ACTION,
    };

    #[test]
    fn axis_motion_contract_carries_shared_defaults_and_actions() {
        assert_eq!(AXIS_MOTION_ACTION_CONTRACT.family, FAMILY);
        assert_eq!(
            AXIS_MOTION_ACTION_CONTRACT.relative_action,
            MOVE_RELATIVE_ACTION
        );
        assert_eq!(
            AXIS_MOTION_ACTION_CONTRACT.absolute_action,
            MOVE_ABSOLUTE_ACTION
        );
        assert_eq!(AXIS_MOTION_ACTION_CONTRACT.default_port, DEFAULT_PORT);
        assert_eq!(
            AXIS_MOTION_ACTION_CONTRACT.default_require_homed,
            DEFAULT_REQUIRE_HOMED
        );
    }

    #[test]
    fn axis_motion_contract_requires_all_blocking_outcome_branches() {
        let kinds = AXIS_MOTION_ACTION_CONTRACT
            .required_branches
            .iter()
            .map(|branch| branch.kind)
            .collect::<Vec<_>>();

        assert_eq!(
            kinds,
            vec![
                AxisMotionBranchKind::Timeout,
                AxisMotionBranchKind::Reject,
                AxisMotionBranchKind::MotionFault,
                AxisMotionBranchKind::SafetyFault,
            ]
        );
        assert_eq!(
            AXIS_MOTION_CAPABILITY.result_buckets,
            &[
                DeviceActionResultBucket::Complete,
                DeviceActionResultBucket::Timeout,
                DeviceActionResultBucket::Reject,
                DeviceActionResultBucket::MotionFault,
                DeviceActionResultBucket::SafetyFault,
            ]
        );
    }
}
