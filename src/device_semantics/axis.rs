use crate::ast::{
    ActionStatement, ActionTarget, AxisFaultRouteDirective as AstAxisFaultRouteDirective,
    AxisFaultRouteKind as AstAxisFaultRouteKind, GotoDirective, StepStatement, TasksSection,
    TimeoutDirective,
};
use crate::device_semantics::{DeviceActionContract, DeviceActionResultBucket};
use crate::error::PlcError;
use crate::ir::{
    AxisFaultBranch, AxisFaultKind, AxisFaultRouteBranch as IrAxisFaultRouteBranch,
    AxisFaultRouteKind as IrAxisFaultRouteKind, AxisTimeoutBranch, DeviceKind, TransitionAction,
};
use std::collections::HashMap;

pub const FAMILY: &str = "axis";

pub const CLOSED_LOOP_MOVE_CAPABILITY: DeviceActionContract<'static> = DeviceActionContract {
    family: FAMILY,
    action: "move",
    result_buckets: &[
        DeviceActionResultBucket::Complete,
        DeviceActionResultBucket::Timeout,
        DeviceActionResultBucket::MotionFault,
        DeviceActionResultBucket::SafetyFault,
    ],
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisMoveVerb {
    Relative,
    Absolute,
}

impl AxisMoveVerb {
    pub const fn action_keyword(self) -> &'static str {
        match self {
            Self::Relative => "axis.move_relative",
            Self::Absolute => "axis.move_absolute",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AxisMoveActionView<'a> {
    pub verb: AxisMoveVerb,
    pub target: &'a ActionTarget,
    pub timeout: Option<&'a TimeoutDirective>,
    pub on_reject: Option<&'a GotoDirective>,
    pub on_motion_fault: Option<&'a GotoDirective>,
    pub on_safety_fault: Option<&'a GotoDirective>,
    pub on_reject_routes: &'a [AstAxisFaultRouteDirective],
    pub on_motion_fault_routes: &'a [AstAxisFaultRouteDirective],
    pub on_safety_fault_routes: &'a [AstAxisFaultRouteDirective],
}

impl<'a> AxisMoveActionView<'a> {
    pub fn action_text(self) -> String {
        format!("{} {}", self.verb.action_keyword(), self.target.device)
    }

    pub fn branch_targets(self) -> Vec<(&'static str, &'a GotoDirective)> {
        let mut branches = Vec::new();
        if let Some(timeout) = self.timeout {
            branches.push(("timeout", &timeout.target));
        }
        if let Some(branch) = self.on_reject {
            branches.push(("on_reject", branch));
        }
        if let Some(branch) = self.on_motion_fault {
            branches.push(("on_motion_fault", branch));
        }
        if let Some(branch) = self.on_safety_fault {
            branches.push(("on_safety_fault", branch));
        }
        branches
    }
}

pub fn move_action_view(action: &ActionStatement) -> Option<AxisMoveActionView<'_>> {
    match action {
        ActionStatement::AxisMoveRelative {
            target,
            timeout,
            on_reject,
            on_motion_fault,
            on_safety_fault,
            on_reject_routes,
            on_motion_fault_routes,
            on_safety_fault_routes,
            ..
        } => Some(AxisMoveActionView {
            verb: AxisMoveVerb::Relative,
            target,
            timeout: timeout.as_ref(),
            on_reject: on_reject.as_ref(),
            on_motion_fault: on_motion_fault.as_ref(),
            on_safety_fault: on_safety_fault.as_ref(),
            on_reject_routes,
            on_motion_fault_routes,
            on_safety_fault_routes,
        }),
        ActionStatement::AxisMoveAbsolute {
            target,
            timeout,
            on_reject,
            on_motion_fault,
            on_safety_fault,
            on_reject_routes,
            on_motion_fault_routes,
            on_safety_fault_routes,
            ..
        } => Some(AxisMoveActionView {
            verb: AxisMoveVerb::Absolute,
            target,
            timeout: timeout.as_ref(),
            on_reject: on_reject.as_ref(),
            on_motion_fault: on_motion_fault.as_ref(),
            on_safety_fault: on_safety_fault.as_ref(),
            on_reject_routes,
            on_motion_fault_routes,
            on_safety_fault_routes,
        }),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AxisMoveTransitionView<'a> {
    pub verb: AxisMoveVerb,
    pub target: &'a str,
    pub value_raw: &'a str,
    pub speed_raw: &'a str,
    pub require_homed: bool,
    pub timeout: &'a AxisTimeoutBranch,
    pub on_reject: &'a AxisFaultBranch,
    pub on_motion_fault: &'a AxisFaultBranch,
    pub on_safety_fault: &'a AxisFaultBranch,
    pub on_reject_routes: &'a [IrAxisFaultRouteBranch],
    pub on_motion_fault_routes: &'a [IrAxisFaultRouteBranch],
    pub on_safety_fault_routes: &'a [IrAxisFaultRouteBranch],
}

impl<'a> AxisMoveTransitionView<'a> {
    pub fn for_each_target(self, mut visit: impl FnMut(&'a str, Option<&'a str>)) {
        visit(&self.timeout.target_task, self.timeout.target_step.as_deref());
        visit(&self.on_reject.target_task, self.on_reject.target_step.as_deref());
        visit(
            &self.on_motion_fault.target_task,
            self.on_motion_fault.target_step.as_deref(),
        );
        visit(
            &self.on_safety_fault.target_task,
            self.on_safety_fault.target_step.as_deref(),
        );
        for route in self.on_reject_routes {
            visit(&route.target_task, route.target_step.as_deref());
        }
        for route in self.on_motion_fault_routes {
            visit(&route.target_task, route.target_step.as_deref());
        }
        for route in self.on_safety_fault_routes {
            visit(&route.target_task, route.target_step.as_deref());
        }
    }
}

pub fn move_transition_view(action: &TransitionAction) -> Option<AxisMoveTransitionView<'_>> {
    match action {
        TransitionAction::AxisMoveRelative {
            target,
            distance_raw,
            speed_raw,
            timeout,
            on_reject,
            on_motion_fault,
            on_safety_fault,
            on_reject_routes,
            on_motion_fault_routes,
            on_safety_fault_routes,
            ..
        } => Some(AxisMoveTransitionView {
            verb: AxisMoveVerb::Relative,
            target,
            value_raw: distance_raw,
            speed_raw,
            require_homed: false,
            timeout,
            on_reject,
            on_motion_fault,
            on_safety_fault,
            on_reject_routes,
            on_motion_fault_routes,
            on_safety_fault_routes,
        }),
        TransitionAction::AxisMoveAbsolute {
            target,
            position_raw,
            speed_raw,
            require_homed,
            timeout,
            on_reject,
            on_motion_fault,
            on_safety_fault,
            on_reject_routes,
            on_motion_fault_routes,
            on_safety_fault_routes,
            ..
        } => Some(AxisMoveTransitionView {
            verb: AxisMoveVerb::Absolute,
            target,
            value_raw: position_raw,
            speed_raw,
            require_homed: *require_homed,
            timeout,
            on_reject,
            on_motion_fault,
            on_safety_fault,
            on_reject_routes,
            on_motion_fault_routes,
            on_safety_fault_routes,
        }),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisSemanticErrorKind {
    TargetMustBeAxisDrive,
    MissingTimeout,
    MissingReject,
    MissingMotionFault,
    MissingSafetyFault,
}

impl AxisSemanticErrorKind {
    pub const fn code(self) -> &'static str {
        match self {
            Self::TargetMustBeAxisDrive => "AXIS-005",
            Self::MissingTimeout => "AXIS-001",
            Self::MissingReject => "AXIS-002",
            Self::MissingMotionFault => "AXIS-003",
            Self::MissingSafetyFault => "AXIS-004",
        }
    }
}

pub fn classify_move_routing(view: AxisMoveActionView<'_>) -> Vec<AxisSemanticErrorKind> {
    let mut errors = Vec::new();
    if view.timeout.is_none() {
        errors.push(AxisSemanticErrorKind::MissingTimeout);
    }
    if view.on_reject.is_none() {
        errors.push(AxisSemanticErrorKind::MissingReject);
    }
    if view.on_motion_fault.is_none() {
        errors.push(AxisSemanticErrorKind::MissingMotionFault);
    }
    if view.on_safety_fault.is_none() {
        errors.push(AxisSemanticErrorKind::MissingSafetyFault);
    }
    errors
}

pub fn validate_move_target_kind(
    line: usize,
    step_name: &str,
    target: &str,
    device_kinds: &HashMap<String, DeviceKind>,
) -> Option<PlcError> {
    match device_kinds.get(target) {
        Some(DeviceKind::StepperMotor) | Some(DeviceKind::ServoDrive) => None,
        Some(kind) => Some(PlcError::semantic_with_reason(
            line,
            format!(
                "[{}] axis target '{target}' must be stepper_motor or servo_drive.",
                AxisSemanticErrorKind::TargetMustBeAxisDrive.code()
            ),
            format!(
                "step '{step_name}' 当前目标类型是 {kind:?}。axis.move_relative / axis.move_absolute 只能作用于 stepper_motor 或 servo_drive 设备。"
            ),
        )),
        None => Some(PlcError::semantic_with_reason(
            line,
            format!(
                "[{}] axis target '{target}' must be stepper_motor or servo_drive.",
                AxisSemanticErrorKind::TargetMustBeAxisDrive.code()
            ),
            format!(
                "step '{step_name}' 引用了未声明设备。请先在 [topology] 中声明该轴设备，且类型必须是 stepper_motor 或 servo_drive。"
            ),
        )),
    }
}

pub fn validate_axis_actions_in_tasks(
    tasks: &TasksSection,
    device_kinds: &HashMap<String, DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    for task in &tasks.tasks {
        for step in &task.steps {
            validate_axis_actions_in_statements(
                &step.statements,
                &step.name,
                step.line.max(1),
                device_kinds,
                errors,
            );
        }
    }
}

fn validate_axis_actions_in_statements(
    statements: &[StepStatement],
    step_name: &str,
    line: usize,
    device_kinds: &HashMap<String, DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(action) => {
                let Some(view) = move_action_view(action) else {
                    continue;
                };

                if let Some(error) =
                    validate_move_target_kind(line, step_name, &view.target.device, device_kinds)
                {
                    errors.push(error);
                }

                for kind in classify_move_routing(view) {
                    errors.push(axis_routing_error(
                        line,
                        step_name,
                        kind,
                    ));
                }

                validate_fault_routes(
                    line,
                    step_name,
                    "on_reject",
                    view.on_reject_routes,
                    &[AstAxisFaultRouteKind::Reject, AstAxisFaultRouteKind::Vendor],
                    errors,
                );
                validate_fault_routes(
                    line,
                    step_name,
                    "on_motion_fault",
                    view.on_motion_fault_routes,
                    &[AstAxisFaultRouteKind::Motion, AstAxisFaultRouteKind::Vendor],
                    errors,
                );
                validate_fault_routes(
                    line,
                    step_name,
                    "on_safety_fault",
                    view.on_safety_fault_routes,
                    &[AstAxisFaultRouteKind::Safety, AstAxisFaultRouteKind::Vendor],
                    errors,
                );
            }
            StepStatement::Repeat { body, .. } => {
                validate_axis_actions_in_statements(body, step_name, line, device_kinds, errors);
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_axis_actions_in_statements(
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
                    validate_axis_actions_in_statements(
                        &branch.statements,
                        step_name,
                        line,
                        device_kinds,
                        errors,
                    );
                }
            }
            StepStatement::Wait(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_)
            | StepStatement::Effect(_) => {}
        }
    }
}

fn axis_routing_error(
    line: usize,
    step_name: &str,
    kind: AxisSemanticErrorKind,
) -> PlcError {
    let (branch_name, fix) = match kind {
        AxisSemanticErrorKind::MissingTimeout => {
            ("timeout", "添加 timeout: <duration> -> <task.step> 分支。")
        }
        AxisSemanticErrorKind::MissingReject => {
            ("on_reject", "添加 on_reject -> <task.step> 分支。")
        }
        AxisSemanticErrorKind::MissingMotionFault => (
            "on_motion_fault",
            "添加 on_motion_fault -> <task.step> 分支。",
        ),
        AxisSemanticErrorKind::MissingSafetyFault => (
            "on_safety_fault",
            "添加 on_safety_fault -> <task.step> 分支。",
        ),
        AxisSemanticErrorKind::TargetMustBeAxisDrive => {
            ("target", "将目标改为 stepper_motor 或 servo_drive 设备。")
        }
    };
    PlcError::semantic_with_reason(
        line,
        format!("[{}] step '{step_name}' is missing {branch_name} branch.", kind.code()),
        fix.to_string(),
    )
}

pub fn validate_fault_routes(
    line: usize,
    step_name: &str,
    branch_name: &str,
    routes: &[AstAxisFaultRouteDirective],
    allowed_kinds: &[AstAxisFaultRouteKind],
    errors: &mut Vec<PlcError>,
) {
    for route in routes {
        if let Some(kind) = route.kind
            && !allowed_kinds.contains(&kind)
        {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!(
                    "[AXIS-010] step '{step_name}' has incompatible {branch_name} route kind '{kind:?}'."
                ),
                format!(
                    "{branch_name} 仅允许 kind 为 {}。",
                    allowed_kinds
                        .iter()
                        .map(|value| format!("{:?}", value).to_lowercase())
                        .collect::<Vec<_>>()
                        .join("/")
                ),
            ));
        }
    }
}

pub fn lower_fault_branch(
    goto: &GotoDirective,
    kind: AxisFaultKind,
    error_code: Option<&str>,
) -> AxisFaultBranch {
    AxisFaultBranch {
        target_task: goto.task.clone(),
        target_step: goto.step.clone(),
        category: kind.category(),
        vendor_code: kind.vendor_code(),
        kind,
        error_code: error_code.map(ToString::to_string),
    }
}

pub fn lower_fault_routes(routes: &[AstAxisFaultRouteDirective]) -> Vec<IrAxisFaultRouteBranch> {
    routes
        .iter()
        .map(|route| IrAxisFaultRouteBranch {
            target_task: route.target.task.clone(),
            target_step: route.target.step.clone(),
            kind: route.kind.map(lower_fault_route_kind),
            code: route.code,
        })
        .collect()
}

pub fn lower_fault_route_kind(kind: AstAxisFaultRouteKind) -> IrAxisFaultRouteKind {
    match kind {
        AstAxisFaultRouteKind::Reject => IrAxisFaultRouteKind::Reject,
        AstAxisFaultRouteKind::Motion => IrAxisFaultRouteKind::Motion,
        AstAxisFaultRouteKind::Safety => IrAxisFaultRouteKind::Safety,
        AstAxisFaultRouteKind::Vendor => IrAxisFaultRouteKind::Vendor,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        classify_move_routing, lower_fault_branch, lower_fault_routes, move_action_view,
        move_transition_view, validate_move_target_kind, AxisMoveVerb, AxisSemanticErrorKind,
    };
    use crate::ast::{
        ActionStatement, ActionTarget, AxisFaultRouteDirective, AxisFaultRouteKind, DurationValue,
        GotoDirective, TimeUnit, TimeoutDirective,
    };
    use crate::ir::{
        AxisFaultBranch, AxisFaultCategory, AxisFaultKind, AxisFaultRouteBranch, AxisTimeoutBranch,
        DeviceKind, TransitionAction,
    };
    use std::collections::HashMap;

    #[test]
    fn extracts_axis_action_view_and_branch_targets() {
        let action = ActionStatement::AxisMoveRelative {
            target: ActionTarget::simple("axis_x"),
            params: None,
            distance: 12.5,
            speed: Some(10.0),
            acceleration: Some(20.0),
            deceleration: Some(20.0),
            timeout: Some(TimeoutDirective {
                duration: DurationValue {
                    value: 100,
                    unit: TimeUnit::Ms,
                },
                target: GotoDirective {
                    line: 0,
                    task: "fault".to_string(),
                    step: Some("timeout".to_string()),
                },
            }),
            on_reject: Some(GotoDirective {
                line: 0,
                task: "fault".to_string(),
                step: Some("reject".to_string()),
            }),
            on_motion_fault: Some(GotoDirective {
                line: 0,
                task: "fault".to_string(),
                step: Some("motion".to_string()),
            }),
            on_safety_fault: Some(GotoDirective {
                line: 0,
                task: "fault".to_string(),
                step: Some("safety".to_string()),
            }),
            on_reject_routes: vec![],
            on_motion_fault_routes: vec![],
            on_safety_fault_routes: vec![],
            semantic_tag: None,
        };

        let view = move_action_view(&action).expect("axis move should map to view");
        assert_eq!(view.verb, AxisMoveVerb::Relative);
        assert_eq!(view.action_text(), "axis.move_relative axis_x");
        assert_eq!(
            view.branch_targets()
                .into_iter()
                .map(|(kind, target)| format!(
                    "{kind}:{}.{}",
                    target.task,
                    target.step.as_deref().unwrap_or_default()
                ))
                .collect::<Vec<_>>(),
            vec![
                "timeout:fault.timeout".to_string(),
                "on_reject:fault.reject".to_string(),
                "on_motion_fault:fault.motion".to_string(),
                "on_safety_fault:fault.safety".to_string(),
            ]
        );
    }

    #[test]
    fn extracts_axis_transition_targets() {
        let action = TransitionAction::AxisMoveAbsolute {
            target: "axis_z".to_string(),
            port: "cmd".to_string(),
            position_raw: "100.0".to_string(),
            speed_raw: "50.0".to_string(),
            require_homed: true,
            timeout: AxisTimeoutBranch {
                duration_ms: 200,
                target_task: "fault".to_string(),
                target_step: Some("timeout".to_string()),
            },
            on_reject: AxisFaultBranch {
                target_task: "fault".to_string(),
                target_step: Some("reject".to_string()),
                kind: AxisFaultKind::Reject,
                category: AxisFaultCategory::Recoverable,
                vendor_code: None,
                error_code: None,
            },
            on_motion_fault: AxisFaultBranch {
                target_task: "fault".to_string(),
                target_step: Some("motion".to_string()),
                kind: AxisFaultKind::Motion,
                category: AxisFaultCategory::NonRecoverable,
                vendor_code: None,
                error_code: None,
            },
            on_safety_fault: AxisFaultBranch {
                target_task: "fault".to_string(),
                target_step: Some("safety".to_string()),
                kind: AxisFaultKind::Safety,
                category: AxisFaultCategory::Safety,
                vendor_code: None,
                error_code: None,
            },
            on_reject_routes: vec![AxisFaultRouteBranch {
                target_task: "fault".to_string(),
                target_step: Some("reject_vendor".to_string()),
                kind: Some(crate::ir::AxisFaultRouteKind::Vendor),
                code: Some(42),
            }],
            on_motion_fault_routes: vec![],
            on_safety_fault_routes: vec![],
            semantic_tag: None,
        };

        let view = move_transition_view(&action).expect("transition should map to axis view");
        assert_eq!(view.verb, AxisMoveVerb::Absolute);
        assert!(view.require_homed);

        let mut targets = Vec::new();
        view.for_each_target(|task, step| {
            targets.push(format!("{task}.{}", step.unwrap_or_default()));
        });
        assert_eq!(
            targets,
            vec![
                "fault.timeout".to_string(),
                "fault.reject".to_string(),
                "fault.motion".to_string(),
                "fault.safety".to_string(),
                "fault.reject_vendor".to_string(),
            ]
        );
    }

    #[test]
    fn classifies_missing_axis_routes() {
        let action = ActionStatement::AxisMoveRelative {
            target: ActionTarget::simple("axis_x"),
            params: None,
            distance: 5.0,
            speed: Some(5.0),
            acceleration: Some(10.0),
            deceleration: Some(10.0),
            timeout: None,
            on_reject: None,
            on_motion_fault: None,
            on_safety_fault: None,
            on_reject_routes: vec![],
            on_motion_fault_routes: vec![],
            on_safety_fault_routes: vec![],
            semantic_tag: None,
        };

        let view = move_action_view(&action).expect("axis move should map to view");
        assert_eq!(
            classify_move_routing(view),
            vec![
                AxisSemanticErrorKind::MissingTimeout,
                AxisSemanticErrorKind::MissingReject,
                AxisSemanticErrorKind::MissingMotionFault,
                AxisSemanticErrorKind::MissingSafetyFault,
            ]
        );
    }

    #[test]
    fn only_axis_drives_can_use_axis_move_semantics() {
        let mut device_kinds = HashMap::new();
        device_kinds.insert("servo_x".to_string(), DeviceKind::ServoDrive);
        device_kinds.insert("motor_x".to_string(), DeviceKind::Motor);

        assert!(validate_move_target_kind(1, "move", "servo_x", &device_kinds).is_none());
        let err = validate_move_target_kind(1, "move", "motor_x", &device_kinds)
            .expect("motor target should be rejected");
        assert!(err.to_string().contains("[AXIS-005]"));
    }

    #[test]
    fn lowers_axis_fault_branch_and_routes() {
        let goto = GotoDirective {
            line: 0,
            task: "fault".to_string(),
            step: Some("reject".to_string()),
        };
        let branch = lower_fault_branch(&goto, AxisFaultKind::Reject, Some("AXIS_REJECT"));
        assert_eq!(branch.target_task, "fault");
        assert_eq!(branch.target_step.as_deref(), Some("reject"));
        assert_eq!(branch.error_code.as_deref(), Some("AXIS_REJECT"));

        let routes = lower_fault_routes(&[AxisFaultRouteDirective {
            line: 0,
            kind: Some(AxisFaultRouteKind::Vendor),
            code: Some(42),
            target: GotoDirective {
                line: 0,
                task: "fault".to_string(),
                step: Some("vendor".to_string()),
            },
        }]);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].target_task, "fault");
        assert_eq!(routes[0].target_step.as_deref(), Some("vendor"));
        assert_eq!(routes[0].code, Some(42));
    }
}
