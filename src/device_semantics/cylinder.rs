use crate::ast::{
    ActionStatement, ActionTarget, GotoDirective, StepStatement, TasksSection, TimeoutDirective,
    TopologyRelation, TopologySection,
};
use crate::device_semantics::{DeviceActionContract, DeviceActionResultBucket};
use crate::error::PlcError;
use crate::ir::{DeviceKind, TransitionAction};
pub use rustplc_device_semantics::cylinder::{
    COMMAND_PORT, CylinderActionDefaults, CylinderActionOutcome, CylinderContractError,
    CylinderFeedbackFault, CylinderSafetyFault, CylinderStrokeFault, CylinderStrokeVerb,
    CylinderTopologyContractKind, DEFAULT_ACTION_DEFAULTS, DEFAULT_ALLOW_EXTEND,
    DEFAULT_ALLOW_RETRACT, DEFAULT_FEEDBACK_DEBOUNCE_MS, DEFAULT_SIMULATION_MODE,
    DEFAULT_STROKE_TIMEOUT_MS, EXTENDED_STATE_PORT, FAMILY, RETRACTED_STATE_PORT, STROKE_ACTION,
};
use std::collections::HashMap;

/// Capability envelope for cylinder closed-loop stroke actions.
/// `timeout` is optional; fault routing may be declared without it.
pub const CLOSED_LOOP_STROKE_CAPABILITY: DeviceActionContract<'static> = DeviceActionContract {
    family: FAMILY,
    action: STROKE_ACTION,
    result_buckets: &[
        DeviceActionResultBucket::Complete,
        DeviceActionResultBucket::Timeout,
        DeviceActionResultBucket::MotionFault,
        DeviceActionResultBucket::SafetyFault,
    ],
};

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

pub fn transition_action_uses_closed_loop_stroke(action: &TransitionAction) -> bool {
    closed_loop_stroke_target(action).is_some()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CylinderStrokeContract {
    pub family: &'static str,
    pub action: &'static str,
    pub verb: CylinderStrokeVerb,
    pub command_port: String,
    pub requested_state_port: String,
    pub opposing_state_port: Option<String>,
    pub topology_contract: CylinderTopologyContractKind,
    pub defaults: CylinderActionDefaults,
}

impl CylinderStrokeContract {
    pub fn new(
        verb: CylinderStrokeVerb,
        command_port: &str,
        topology_contract: CylinderTopologyContractKind,
    ) -> Result<Self, CylinderContractError> {
        let requested_state_port = state_port_key(command_port, verb.expected_state_port());
        let opposing_state_port = match topology_contract {
            CylinderTopologyContractKind::OpenLoopCommand => None,
            CylinderTopologyContractKind::ClosedLoopDualEndFeedback => Some(
                complementary_end_state_port(&requested_state_port)
                    .ok_or(CylinderContractError::MissingComplementaryEndState)?,
            ),
        };

        Ok(Self {
            family: FAMILY,
            action: STROKE_ACTION,
            verb,
            command_port: command_port.to_string(),
            requested_state_port,
            opposing_state_port,
            topology_contract,
            defaults: DEFAULT_ACTION_DEFAULTS,
        })
    }

    pub fn closed_loop(
        verb: CylinderStrokeVerb,
        command_port: &str,
    ) -> Result<Self, CylinderContractError> {
        Self::new(
            verb,
            command_port,
            CylinderTopologyContractKind::ClosedLoopDualEndFeedback,
        )
    }

    pub fn open_loop(
        verb: CylinderStrokeVerb,
        command_port: &str,
    ) -> Result<Self, CylinderContractError> {
        Self::new(
            verb,
            command_port,
            CylinderTopologyContractKind::OpenLoopCommand,
        )
    }

    pub fn required_opposing_state_port(&self) -> Result<&str, CylinderContractError> {
        self.opposing_state_port
            .as_deref()
            .ok_or(CylinderContractError::MissingComplementaryEndState)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CylinderSemanticErrorKind {
    TargetMustBeCylinder,
    MissingMotionFault,
    MissingSafetyFault,
    MissingClosedLoopFeedback,
}

impl CylinderSemanticErrorKind {
    pub const fn code(self) -> &'static str {
        match self {
            Self::TargetMustBeCylinder => "CYL-001",
            Self::MissingMotionFault => "CYL-002",
            Self::MissingSafetyFault => "CYL-003",
            Self::MissingClosedLoopFeedback => "CYL-004",
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

pub fn validate_closed_loop_feedback_contracts_in_tasks(
    tasks: &TasksSection,
    topology: &TopologySection,
    device_kinds: &HashMap<String, DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    for task in &tasks.tasks {
        for step in &task.steps {
            validate_closed_loop_feedback_contracts_in_statements(
                &step.statements,
                &step.name,
                step.line.max(1),
                topology,
                device_kinds,
                errors,
            );
        }
    }
}

fn validate_closed_loop_feedback_contracts_in_statements(
    statements: &[StepStatement],
    step_name: &str,
    line: usize,
    topology: &TopologySection,
    device_kinds: &HashMap<String, DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(action) => {
                let Some(view) = stroke_action_view(action) else {
                    continue;
                };
                if !view.uses_closed_loop_semantics() {
                    continue;
                }
                if !matches!(
                    device_kinds.get(&view.target.device),
                    Some(DeviceKind::Cylinder)
                ) {
                    continue;
                }
                let Ok(contract) =
                    CylinderStrokeContract::closed_loop(view.verb, view.target.port.as_str())
                else {
                    continue;
                };
                let Ok(opposing_state_port) = contract.required_opposing_state_port() else {
                    continue;
                };
                if !has_reported_cylinder_feedback(
                    topology,
                    &view.target.device,
                    &contract.requested_state_port,
                ) || !has_reported_cylinder_feedback(
                    topology,
                    &view.target.device,
                    opposing_state_port,
                ) {
                    errors.push(cylinder_missing_closed_loop_feedback_error(
                        line,
                        step_name,
                        &view.target.device,
                        &contract.requested_state_port,
                    ));
                }
            }
            StepStatement::Repeat { body, .. } => {
                validate_closed_loop_feedback_contracts_in_statements(
                    body,
                    step_name,
                    line,
                    topology,
                    device_kinds,
                    errors,
                )
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_closed_loop_feedback_contracts_in_statements(
                        &branch.statements,
                        step_name,
                        line,
                        topology,
                        device_kinds,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_closed_loop_feedback_contracts_in_statements(
                        &branch.statements,
                        step_name,
                        line,
                        topology,
                        device_kinds,
                        errors,
                    );
                }
            }
            StepStatement::Effect(_)
            | StepStatement::Wait(_)
            | StepStatement::IfElse { .. }
            | StepStatement::Delay { .. }
            | StepStatement::Timeout(_)
            | StepStatement::Goto(_)
            | StepStatement::AllowIndefiniteWait(_) => {}
        }
    }
}

fn has_reported_cylinder_feedback(
    topology: &TopologySection,
    cylinder: &str,
    state_port: &str,
) -> bool {
    topology
        .connections
        .iter()
        .filter(|connection| {
            connection.from == cylinder
                && connection.relation == TopologyRelation::Detects
                && connection.from_port.as_deref() == Some(state_port)
        })
        .any(|detects| {
            topology.connections.iter().any(|reports| {
                reports.from == detects.to && reports.relation == TopologyRelation::ReportsTo
            })
        })
}

fn cylinder_missing_closed_loop_feedback_error(
    line: usize,
    step_name: &str,
    target: &str,
    requested_state: &str,
) -> PlcError {
    PlcError::semantic_with_reason(
        line,
        format!(
            "[{}] step '{step_name}' declares closed-loop cylinder action for '{target}' without complete dual-end feedback.",
            CylinderSemanticErrorKind::MissingClosedLoopFeedback.code()
        ),
        format!(
            "closed-loop cylinder action for '{target}.{requested_state}' requires both requested and complementary end-state feedback paths before runtime lowering"
        ),
    )
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
        CylinderSemanticErrorKind::MissingClosedLoopFeedback => {
            cylinder_missing_closed_loop_feedback_error(line, step_name, target, "")
        }
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
        CylinderSemanticErrorKind, CylinderStrokeContract, CylinderStrokeVerb,
        CylinderTopologyContractKind, DEFAULT_ACTION_DEFAULTS, EXTENDED_STATE_PORT,
        RETRACTED_STATE_PORT, classify_stroke_routing, closed_loop_stroke_target,
        complementary_end_state_port, is_end_state_port, stroke_action_view,
        validate_stroke_target_kind,
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
    fn closed_loop_stroke_contract_carries_defaults_and_end_feedback_contract() {
        let contract = CylinderStrokeContract::closed_loop(CylinderStrokeVerb::Extend, "cmd")
            .expect("default cylinder ports should have a complementary end state");

        assert_eq!(contract.family, "cylinder");
        assert_eq!(contract.action, "stroke");
        assert_eq!(contract.verb, CylinderStrokeVerb::Extend);
        assert_eq!(contract.command_port, "cmd");
        assert_eq!(contract.requested_state_port, "cmd.extended");
        assert_eq!(contract.required_opposing_state_port(), Ok("cmd.retracted"));
        assert_eq!(
            contract.topology_contract,
            CylinderTopologyContractKind::ClosedLoopDualEndFeedback
        );
        assert_eq!(contract.defaults, DEFAULT_ACTION_DEFAULTS);
        assert_eq!(contract.defaults.feedback_debounce_ms, 20);
        assert_eq!(contract.defaults.stroke_timeout_ms, 3000);
        assert!(contract.defaults.allow_extend);
        assert!(contract.defaults.allow_retract);
        assert!(!contract.defaults.simulation_mode);
    }

    #[test]
    fn open_loop_stroke_contract_does_not_require_feedback() {
        let contract = CylinderStrokeContract::open_loop(CylinderStrokeVerb::Retract, "cmd")
            .expect("open-loop contract should not need complementary feedback");

        assert_eq!(contract.requested_state_port, "cmd.retracted");
        assert_eq!(contract.opposing_state_port, None);
        assert_eq!(
            contract.topology_contract,
            CylinderTopologyContractKind::OpenLoopCommand
        );
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
