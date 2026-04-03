use crate::ast::{
    ActionStatement, ActionTarget, GotoDirective, StepStatement, TasksSection, TimeoutDirective,
};
use crate::device_semantics::{DeviceActionContract, DeviceActionResultBucket};
use crate::error::PlcError;
use crate::ir::{DeviceKind, TransitionAction};
use std::collections::HashMap;

pub const FAMILY: &str = "cylinder";
pub const COMMAND_PORT: &str = "cmd";
pub const EXTENDED_STATE_PORT: &str = "extended";
pub const RETRACTED_STATE_PORT: &str = "retracted";

/// Capability envelope for cylinder closed-loop stroke actions.
/// `timeout` is optional; fault routing may be declared without it.
pub const CLOSED_LOOP_STROKE_CAPABILITY: DeviceActionContract<'static> = DeviceActionContract {
    family: FAMILY,
    action: "stroke",
    result_buckets: &[
        DeviceActionResultBucket::Complete,
        DeviceActionResultBucket::Timeout,
        DeviceActionResultBucket::MotionFault,
        DeviceActionResultBucket::SafetyFault,
    ],
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CylinderStrokeVerb {
    Extend,
    Retract,
}

impl CylinderStrokeVerb {
    pub const fn expected_state_port(self) -> &'static str {
        match self {
            Self::Extend => EXTENDED_STATE_PORT,
            Self::Retract => RETRACTED_STATE_PORT,
        }
    }

    pub const fn action_keyword(self) -> &'static str {
        match self {
            Self::Extend => "extend",
            Self::Retract => "retract",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CylinderStrokeActionView<'a> {
    pub verb: CylinderStrokeVerb,
    pub target: &'a ActionTarget,
    pub timeout: Option<&'a TimeoutDirective>,
    pub on_motion_fault: Option<&'a GotoDirective>,
    pub on_safety_fault: Option<&'a GotoDirective>,
}

impl<'a> CylinderStrokeActionView<'a> {
    pub fn action_text(self) -> String {
        format!("{} {}", self.verb.action_keyword(), self.target.device)
    }

    pub fn branch_targets(self) -> Vec<(&'static str, &'a GotoDirective)> {
        let mut branches = Vec::new();
        if let Some(branch) = self.timeout {
            branches.push(("timeout", &branch.target));
        }
        if let Some(branch) = self.on_motion_fault {
            branches.push(("on_motion_fault", branch));
        }
        if let Some(branch) = self.on_safety_fault {
            branches.push(("on_safety_fault", branch));
        }
        branches
    }

    pub const fn uses_closed_loop_semantics(self) -> bool {
        self.timeout.is_some() || self.on_motion_fault.is_some() || self.on_safety_fault.is_some()
    }
}

pub fn stroke_action_view(action: &ActionStatement) -> Option<CylinderStrokeActionView<'_>> {
    match action {
        ActionStatement::Extend {
            target,
            timeout,
            on_motion_fault,
            on_safety_fault,
        } => Some(CylinderStrokeActionView {
            verb: CylinderStrokeVerb::Extend,
            target,
            timeout: timeout.as_ref(),
            on_motion_fault: on_motion_fault.as_ref(),
            on_safety_fault: on_safety_fault.as_ref(),
        }),
        ActionStatement::Retract {
            target,
            timeout,
            on_motion_fault,
            on_safety_fault,
        } => Some(CylinderStrokeActionView {
            verb: CylinderStrokeVerb::Retract,
            target,
            timeout: timeout.as_ref(),
            on_motion_fault: on_motion_fault.as_ref(),
            on_safety_fault: on_safety_fault.as_ref(),
        }),
        _ => None,
    }
}

pub fn closed_loop_stroke_target(action: &TransitionAction) -> Option<&str> {
    match action {
        TransitionAction::Extend {
            target,
            timeout,
            on_motion_fault,
            on_safety_fault,
            ..
        }
        | TransitionAction::Retract {
            target,
            timeout,
            on_motion_fault,
            on_safety_fault,
            ..
        } if timeout.is_some() || on_motion_fault.is_some() || on_safety_fault.is_some() => {
            Some(target.as_str())
        }
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CylinderStrokeFault {
    OppositeFeedbackReasserted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CylinderSafetyFault {
    ContradictoryFeedback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CylinderActionOutcome {
    Done,
    Timeout,
    StrokeFault(CylinderStrokeFault),
    SafetyFault(CylinderSafetyFault),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CylinderSemanticErrorKind {
    TargetMustBeCylinder,
    MissingMotionFault,
    MissingSafetyFault,
}

impl CylinderSemanticErrorKind {
    pub const fn code(self) -> &'static str {
        match self {
            Self::TargetMustBeCylinder => "CYL-001",
            Self::MissingMotionFault => "CYL-002",
            Self::MissingSafetyFault => "CYL-003",
        }
    }
}

pub fn classify_stroke_routing(
    has_on_motion_fault: bool,
    has_on_safety_fault: bool,
) -> Vec<CylinderSemanticErrorKind> {
    let uses_fault_routing = has_on_motion_fault || has_on_safety_fault;
    let mut errors = Vec::new();
    if uses_fault_routing && !has_on_motion_fault {
        errors.push(CylinderSemanticErrorKind::MissingMotionFault);
    }
    if uses_fault_routing && !has_on_safety_fault {
        errors.push(CylinderSemanticErrorKind::MissingSafetyFault);
    }
    errors
}

pub fn validate_stroke_target_kind(
    line: usize,
    step_name: &str,
    target: &str,
    device_kinds: &HashMap<String, DeviceKind>,
) -> Option<PlcError> {
    match device_kinds.get(target) {
        Some(DeviceKind::Cylinder) => None,
        Some(kind) => Some(PlcError::semantic_with_reason(
            line,
            format!(
                "[{}] cylinder stroke target '{target}' must be cylinder.",
                CylinderSemanticErrorKind::TargetMustBeCylinder.code()
            ),
            format!(
                "step '{step_name}' 当前目标类型是 {kind:?}。extend/retract 这类气缸动作只能作用于 cylinder 设备；timeout/on_motion_fault/on_safety_fault 只是附加的闭环分流语义。"
            ),
        )),
        None => Some(PlcError::semantic_with_reason(
            line,
            format!(
                "[{}] cylinder stroke target '{target}' must be cylinder.",
                CylinderSemanticErrorKind::TargetMustBeCylinder.code()
            ),
            format!(
                "step '{step_name}' 引用了未定义设备。请先在 [topology] 中声明该 cylinder 设备；extend/retract 不能作用于未声明设备。"
            ),
        )),
    }
}

pub fn validate_cylinder_actions_in_tasks(
    tasks: &TasksSection,
    device_kinds: &HashMap<String, DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    for task in &tasks.tasks {
        for step in &task.steps {
            validate_cylinder_actions_in_statements(
                &step.statements,
                &step.name,
                step.line.max(1),
                device_kinds,
                errors,
            );
        }
    }
}

fn validate_cylinder_actions_in_statements(
    statements: &[StepStatement],
    step_name: &str,
    line: usize,
    device_kinds: &HashMap<String, DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::Extend {
                target,
                timeout: _,
                on_motion_fault,
                on_safety_fault,
            })
            | StepStatement::Action(ActionStatement::Retract {
                target,
                timeout: _,
                on_motion_fault,
                on_safety_fault,
            }) => {
                if let Some(error) =
                    validate_stroke_target_kind(line, step_name, &target.device, device_kinds)
                {
                    errors.push(error);
                }

                let has_on_motion_fault = on_motion_fault.is_some();
                let has_on_safety_fault = on_safety_fault.is_some();
                let uses_fault_routing = has_on_motion_fault || has_on_safety_fault;

                if !uses_fault_routing {
                    continue;
                }

                for kind in classify_stroke_routing(has_on_motion_fault, has_on_safety_fault) {
                    errors.push(cylinder_routing_error(
                        line,
                        step_name,
                        &target.device,
                        kind,
                    ));
                }
            }
            StepStatement::Repeat { body, .. } => {
                validate_cylinder_actions_in_statements(
                    body,
                    step_name,
                    line,
                    device_kinds,
                    errors,
                );
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_cylinder_actions_in_statements(
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
                    validate_cylinder_actions_in_statements(
                        &branch.statements,
                        step_name,
                        line,
                        device_kinds,
                        errors,
                    );
                }
            }
            StepStatement::Action(_)
            | StepStatement::Effect(_)
            | StepStatement::Wait(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
}

fn cylinder_routing_error(
    line: usize,
    step_name: &str,
    target: &str,
    kind: CylinderSemanticErrorKind,
) -> PlcError {
    match kind {
        CylinderSemanticErrorKind::MissingMotionFault => PlcError::semantic_with_reason(
            line,
            format!(
                "[{}] step '{step_name}' declares incomplete cylinder fault routing for '{target}'.",
                kind.code()
            ),
            "气缸动作若声明故障分流，必须同时给出 on_motion_fault -> <task.step> 与 on_safety_fault -> <task.step>；timeout 可单独声明。",
        ),
        CylinderSemanticErrorKind::MissingSafetyFault => PlcError::semantic_with_reason(
            line,
            format!(
                "[{}] step '{step_name}' declares incomplete cylinder fault routing for '{target}'.",
                kind.code()
            ),
            "气缸动作若声明故障分流，必须同时给出 on_motion_fault -> <task.step> 与 on_safety_fault -> <task.step>；timeout 可单独声明。",
        ),
        CylinderSemanticErrorKind::TargetMustBeCylinder => PlcError::semantic_with_reason(
            line,
            format!(
                "[{}] cylinder stroke target '{target}' must be cylinder.",
                kind.code()
            ),
            format!(
                "step '{step_name}' 的 extend/retract 目标必须是 cylinder 设备；timeout/on_motion_fault/on_safety_fault 只是叠加在该基础规则上的闭环语义。"
            ),
        ),
    }
}

pub fn complementary_end_state_port(requested: &str) -> Option<String> {
    requested
        .strip_suffix(".extended")
        .map(|prefix| state_port_key(prefix, RETRACTED_STATE_PORT))
        .or_else(|| {
            requested
                .strip_suffix(".retracted")
                .map(|prefix| state_port_key(prefix, EXTENDED_STATE_PORT))
        })
        .or_else(|| match requested {
            EXTENDED_STATE_PORT => Some(RETRACTED_STATE_PORT.to_string()),
            RETRACTED_STATE_PORT => Some(EXTENDED_STATE_PORT.to_string()),
            _ => None,
        })
}

pub fn is_end_state_port(port: &str) -> bool {
    matches!(port, EXTENDED_STATE_PORT | RETRACTED_STATE_PORT)
        || port.ends_with(".extended")
        || port.ends_with(".retracted")
}

pub fn state_port_key(port: &str, state: &str) -> String {
    if port == "self" {
        state.to_string()
    } else {
        format!("{port}.{state}")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CylinderSemanticErrorKind, CylinderStrokeVerb, EXTENDED_STATE_PORT, RETRACTED_STATE_PORT,
        classify_stroke_routing, closed_loop_stroke_target, complementary_end_state_port,
        is_end_state_port, stroke_action_view, validate_stroke_target_kind,
    };
    use crate::ast::{
        ActionStatement, ActionTarget, DurationValue, GotoDirective, TimeUnit, TimeoutDirective,
    };
    use crate::ir::{DeviceKind, MotionFaultBranch, MotionTimeoutBranch, TransitionAction};
    use std::collections::HashMap;

    #[test]
    fn maps_default_end_state_ports() {
        assert_eq!(
            complementary_end_state_port(EXTENDED_STATE_PORT).as_deref(),
            Some(RETRACTED_STATE_PORT)
        );
        assert_eq!(
            complementary_end_state_port(RETRACTED_STATE_PORT).as_deref(),
            Some(EXTENDED_STATE_PORT)
        );
    }

    #[test]
    fn maps_scoped_end_state_ports() {
        assert_eq!(
            complementary_end_state_port("rod_a.extended").as_deref(),
            Some("rod_a.retracted")
        );
        assert_eq!(
            complementary_end_state_port("rod_a.retracted").as_deref(),
            Some("rod_a.extended")
        );
        assert_eq!(complementary_end_state_port("rod_a.mid"), None);
    }

    #[test]
    fn detects_only_terminal_feedback_ports() {
        assert!(is_end_state_port("extended"));
        assert!(is_end_state_port("retracted"));
        assert!(is_end_state_port("rod_a.extended"));
        assert!(is_end_state_port("rod_a.retracted"));
        assert!(!is_end_state_port("sense"));
        assert!(!is_end_state_port("mid"));
    }

    #[test]
    fn stroke_verb_maps_to_expected_terminal_port() {
        assert_eq!(CylinderStrokeVerb::Extend.expected_state_port(), "extended");
        assert_eq!(
            CylinderStrokeVerb::Retract.expected_state_port(),
            "retracted"
        );
        assert_eq!(CylinderStrokeVerb::Extend.action_keyword(), "extend");
        assert_eq!(CylinderStrokeVerb::Retract.action_keyword(), "retract");
    }

    #[test]
    fn classifies_stroke_routing_requirements() {
        assert_eq!(classify_stroke_routing(false, false), vec![]);
        assert_eq!(classify_stroke_routing(true, true), vec![]);
        assert_eq!(
            classify_stroke_routing(true, false),
            vec![CylinderSemanticErrorKind::MissingSafetyFault]
        );
        assert_eq!(
            classify_stroke_routing(false, true),
            vec![CylinderSemanticErrorKind::MissingMotionFault]
        );
    }

    #[test]
    fn only_cylinder_targets_can_use_cylinder_stroke_semantics() {
        let mut device_kinds = HashMap::new();
        device_kinds.insert("cyl_a".to_string(), DeviceKind::Cylinder);
        device_kinds.insert("motor_a".to_string(), DeviceKind::Motor);

        assert!(validate_stroke_target_kind(1, "extend", "cyl_a", &device_kinds).is_none());
        let err = validate_stroke_target_kind(1, "extend", "motor_a", &device_kinds)
            .expect("motor target should be rejected");
        assert!(err.to_string().contains("[CYL-001]"));
    }

    #[test]
    fn extracts_stroke_action_view_and_branch_targets() {
        let action = ActionStatement::Extend {
            target: ActionTarget::simple("cyl_a"),
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
            on_motion_fault: Some(GotoDirective {
                line: 0,
                task: "fault".to_string(),
                step: Some("motion_fault".to_string()),
            }),
            on_safety_fault: Some(GotoDirective {
                line: 0,
                task: "fault".to_string(),
                step: Some("safety_fault".to_string()),
            }),
        };

        let view = stroke_action_view(&action).expect("extend should map to cylinder view");
        assert_eq!(view.action_text(), "extend cyl_a");
        assert!(view.uses_closed_loop_semantics());
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
                "on_motion_fault:fault.motion_fault".to_string(),
                "on_safety_fault:fault.safety_fault".to_string(),
            ]
        );
    }

    #[test]
    fn detects_closed_loop_cylinder_stroke_transition_action() {
        let action = TransitionAction::Extend {
            target: "cyl_a".to_string(),
            port: "cmd".to_string(),
            timeout: Some(MotionTimeoutBranch {
                duration_ms: 10,
                target_task: "fault".to_string(),
                target_step: Some("timeout".to_string()),
            }),
            on_motion_fault: Some(MotionFaultBranch {
                target_task: "fault".to_string(),
                target_step: Some("motion_fault".to_string()),
            }),
            on_safety_fault: None,
        };

        assert_eq!(closed_loop_stroke_target(&action), Some("cyl_a"));
        assert_eq!(
            closed_loop_stroke_target(&TransitionAction::Extend {
                target: "cyl_a".to_string(),
                port: "cmd".to_string(),
                timeout: None,
                on_motion_fault: None,
                on_safety_fault: None,
            }),
            None
        );
    }
}
