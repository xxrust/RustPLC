use super::{DeviceActionContract, DeviceActionResultBucket};
use crate::ast::{
    ActionStatement, DeviceType, StepStatement, TasksSection, TopologyRelation, TopologySection,
};
use crate::error::PlcError;
use rustplc_device_semantics::SourceDeviceContract;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessDeviceSourceReport {
    pub device: String,
    pub family: &'static str,
    pub missing_feedback_ports: Vec<&'static str>,
    pub open_loop_policy: Option<String>,
}

pub fn all_process_source_contracts() -> &'static [SourceDeviceContract] {
    &[
        rustplc_device_semantics::proportional_valve::SOURCE_CONTRACT,
        rustplc_device_semantics::gripper::SOURCE_CONTRACT,
        rustplc_device_semantics::conveyor::SOURCE_CONTRACT,
        rustplc_device_semantics::pump::SOURCE_CONTRACT,
        rustplc_device_semantics::heater::SOURCE_CONTRACT,
        rustplc_device_semantics::vision::SOURCE_CONTRACT,
    ]
}

pub fn source_contract_for_device_type(
    device_type: &DeviceType,
) -> Option<&'static SourceDeviceContract> {
    match device_type {
        DeviceType::ProportionalValve => {
            Some(&rustplc_device_semantics::proportional_valve::SOURCE_CONTRACT)
        }
        DeviceType::Gripper => Some(&rustplc_device_semantics::gripper::SOURCE_CONTRACT),
        DeviceType::Conveyor => Some(&rustplc_device_semantics::conveyor::SOURCE_CONTRACT),
        DeviceType::Pump => Some(&rustplc_device_semantics::pump::SOURCE_CONTRACT),
        DeviceType::Heater => Some(&rustplc_device_semantics::heater::SOURCE_CONTRACT),
        DeviceType::VisionSensor => Some(&rustplc_device_semantics::vision::SOURCE_CONTRACT),
        _ => None,
    }
}

pub fn collect_process_device_source_reports(
    topology: &TopologySection,
) -> Vec<ProcessDeviceSourceReport> {
    topology
        .devices
        .iter()
        .filter_map(|device| {
            let contract = source_contract_for_device_type(&device.device_type)?;
            let missing_feedback_ports = contract
                .required_feedback_ports
                .iter()
                .copied()
                .filter(|port| !has_reported_feedback(topology, &device.name, port))
                .collect::<Vec<_>>();

            Some(ProcessDeviceSourceReport {
                device: device.name.clone(),
                family: contract.family,
                missing_feedback_ports,
                open_loop_policy: device.attributes.open_loop_policy.clone(),
            })
        })
        .collect()
}

pub fn validate_process_device_source_contracts(
    topology: &TopologySection,
    errors: &mut Vec<PlcError>,
) {
    for device in &topology.devices {
        let Some(contract) = source_contract_for_device_type(&device.device_type) else {
            continue;
        };
        if has_explicit_open_loop_policy(device.attributes.open_loop_policy.as_deref()) {
            continue;
        }

        let missing_feedback_ports = contract
            .required_feedback_ports
            .iter()
            .copied()
            .filter(|port| !has_reported_feedback(topology, &device.name, port))
            .collect::<Vec<_>>();
        if missing_feedback_ports.is_empty() {
            continue;
        }

        errors.push(process_feedback_contract_error(
            device.line.max(1),
            &device.name,
            contract,
            &missing_feedback_ports,
        ));
    }
}

pub fn validate_process_device_actions_in_tasks(
    tasks: &TasksSection,
    device_types: &HashMap<String, crate::ir::DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    for task in &tasks.tasks {
        for step in &task.steps {
            validate_process_device_actions_in_statements(
                &step.statements,
                step.line.max(task.line).max(1),
                device_types,
                errors,
            );
        }
    }
}

fn validate_process_device_actions_in_statements(
    statements: &[StepStatement],
    line: usize,
    device_types: &HashMap<String, crate::ir::DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    for statement in statements {
        match statement {
            StepStatement::Action(ActionStatement::DeviceAction {
                family,
                action_name,
                target,
                ..
            }) => validate_process_device_action(
                line,
                family,
                action_name,
                &target.device,
                device_types,
                errors,
            ),
            StepStatement::Repeat { body, .. } => {
                validate_process_device_actions_in_statements(body, line, device_types, errors)
            }
            StepStatement::Parallel(block) => {
                for branch in &block.branches {
                    validate_process_device_actions_in_statements(
                        &branch.statements,
                        line,
                        device_types,
                        errors,
                    );
                }
            }
            StepStatement::Race(block) => {
                for branch in &block.branches {
                    validate_process_device_actions_in_statements(
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

fn validate_process_device_action(
    line: usize,
    family: &str,
    action_name: &str,
    target: &str,
    device_types: &HashMap<String, crate::ir::DeviceKind>,
    errors: &mut Vec<PlcError>,
) {
    let Some(kind) = device_types.get(target) else {
        errors.push(PlcError::undefined_reference_with_reason(
            line,
            "设备",
            target,
            "device action 的第一个参数必须是 [topology] 中声明的设备".to_string(),
        ));
        return;
    };
    let Some(contract) = source_contract_for_device_kind(kind) else {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!("[PROC-002] device '{target}' does not expose process device actions."),
            format!("`{family}.{action_name}` 只能作用于声明了过程设备语义的设备"),
        ));
        return;
    };
    if contract.family != family {
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "[PROC-002] device action family mismatch for '{target}': expected {}, got {family}.",
                contract.family
            ),
            "请使用与设备族一致的高层动作，例如 heater.heat_to(...) 或 gripper.grip(...)".to_string(),
        ));
        return;
    }
    if !contract
        .actions
        .iter()
        .any(|action| action.name == action_name)
    {
        let allowed = contract
            .actions
            .iter()
            .map(|action| action.name)
            .collect::<Vec<_>>()
            .join(", ");
        errors.push(PlcError::semantic_with_reason(
            line,
            format!(
                "[PROC-003] unsupported process action '{family}.{action_name}' for '{target}'."
            ),
            format!("该设备族支持的动作为: {allowed}"),
        ));
    }
}

pub fn source_contract_for_device_kind(
    kind: &crate::ir::DeviceKind,
) -> Option<&'static SourceDeviceContract> {
    match kind {
        crate::ir::DeviceKind::ProportionalValve => {
            Some(&rustplc_device_semantics::proportional_valve::SOURCE_CONTRACT)
        }
        crate::ir::DeviceKind::Gripper => Some(&rustplc_device_semantics::gripper::SOURCE_CONTRACT),
        crate::ir::DeviceKind::Conveyor => {
            Some(&rustplc_device_semantics::conveyor::SOURCE_CONTRACT)
        }
        crate::ir::DeviceKind::Pump => Some(&rustplc_device_semantics::pump::SOURCE_CONTRACT),
        crate::ir::DeviceKind::Heater => Some(&rustplc_device_semantics::heater::SOURCE_CONTRACT),
        crate::ir::DeviceKind::VisionSensor => {
            Some(&rustplc_device_semantics::vision::SOURCE_CONTRACT)
        }
        _ => None,
    }
}

pub fn result_bucket_names_for_device_action(family: &str, action_name: &str) -> Vec<String> {
    all_process_source_contracts()
        .iter()
        .find(|contract| contract.family == family)
        .and_then(|contract| {
            contract
                .actions
                .iter()
                .find(|action| action.name == action_name)
        })
        .map(|action| {
            action
                .result_buckets
                .iter()
                .map(|bucket| match bucket {
                    rustplc_device_semantics::ActionResultBucket::Complete => "complete",
                    rustplc_device_semantics::ActionResultBucket::Timeout => "timeout",
                    rustplc_device_semantics::ActionResultBucket::Reject => "reject",
                    rustplc_device_semantics::ActionResultBucket::MotionFault => "motion_fault",
                    rustplc_device_semantics::ActionResultBucket::SafetyFault => "safety_fault",
                })
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn has_explicit_open_loop_policy(policy: Option<&str>) -> bool {
    policy.is_some_and(|value| !value.trim().is_empty())
}

fn has_reported_feedback(topology: &TopologySection, device: &str, port: &str) -> bool {
    topology.connections.iter().any(|connection| {
        connection.from == device
            && connection.from_port.as_deref() == Some(port)
            && connection.relation == TopologyRelation::ReportsTo
    }) || topology
        .connections
        .iter()
        .filter(|connection| {
            connection.from == device
                && connection.from_port.as_deref() == Some(port)
                && connection.relation == TopologyRelation::Detects
        })
        .any(|detects| {
            topology.connections.iter().any(|reports| {
                reports.from == detects.to && reports.relation == TopologyRelation::ReportsTo
            })
        })
}

fn process_feedback_contract_error(
    line: usize,
    device: &str,
    contract: &SourceDeviceContract,
    missing_feedback_ports: &[&str],
) -> PlcError {
    let actions = contract
        .actions
        .iter()
        .map(|action| action.name)
        .collect::<Vec<_>>()
        .join(", ");
    PlcError::semantic_with_reason(
        line,
        format!(
            "[PROC-001] process device '{device}' ({}) is missing required feedback ports: {}.",
            contract.family,
            missing_feedback_ports.join(", ")
        ),
        format!(
            "process actions [{actions}] require closed feedback before runtime/codegen lowering; add reports_to/detects feedback paths or declare open_loop_policy on the device"
        ),
    )
}

pub const PROPORTIONAL_VALVE_ACTION_CONTRACTS: &[DeviceActionContract<'static>] = &[
    DeviceActionContract {
        family: rustplc_device_semantics::proportional_valve::FAMILY,
        action: "set_opening",
        result_buckets: &[
            DeviceActionResultBucket::Complete,
            DeviceActionResultBucket::Timeout,
            DeviceActionResultBucket::Reject,
            DeviceActionResultBucket::MotionFault,
            DeviceActionResultBucket::SafetyFault,
        ],
    },
    DeviceActionContract {
        family: rustplc_device_semantics::proportional_valve::FAMILY,
        action: "reset_fault",
        result_buckets: &[
            DeviceActionResultBucket::Complete,
            DeviceActionResultBucket::Reject,
            DeviceActionResultBucket::SafetyFault,
        ],
    },
];

pub const GRIPPER_ACTION_CONTRACTS: &[DeviceActionContract<'static>] = &[
    DeviceActionContract {
        family: rustplc_device_semantics::gripper::FAMILY,
        action: "grip",
        result_buckets: &[
            DeviceActionResultBucket::Complete,
            DeviceActionResultBucket::Timeout,
            DeviceActionResultBucket::Reject,
            DeviceActionResultBucket::MotionFault,
            DeviceActionResultBucket::SafetyFault,
        ],
    },
    DeviceActionContract {
        family: rustplc_device_semantics::gripper::FAMILY,
        action: "release",
        result_buckets: &[
            DeviceActionResultBucket::Complete,
            DeviceActionResultBucket::Timeout,
            DeviceActionResultBucket::MotionFault,
            DeviceActionResultBucket::SafetyFault,
        ],
    },
];

pub const CONVEYOR_ACTION_CONTRACTS: &[DeviceActionContract<'static>] = &[
    DeviceActionContract {
        family: rustplc_device_semantics::conveyor::FAMILY,
        action: "start",
        result_buckets: &[
            DeviceActionResultBucket::Complete,
            DeviceActionResultBucket::Timeout,
            DeviceActionResultBucket::Reject,
            DeviceActionResultBucket::MotionFault,
            DeviceActionResultBucket::SafetyFault,
        ],
    },
    DeviceActionContract {
        family: rustplc_device_semantics::conveyor::FAMILY,
        action: "stop",
        result_buckets: &[
            DeviceActionResultBucket::Complete,
            DeviceActionResultBucket::Timeout,
        ],
    },
];

pub const PUMP_ACTION_CONTRACTS: &[DeviceActionContract<'static>] = &[
    DeviceActionContract {
        family: rustplc_device_semantics::pump::FAMILY,
        action: "start",
        result_buckets: &[
            DeviceActionResultBucket::Complete,
            DeviceActionResultBucket::Timeout,
            DeviceActionResultBucket::Reject,
            DeviceActionResultBucket::MotionFault,
            DeviceActionResultBucket::SafetyFault,
        ],
    },
    DeviceActionContract {
        family: rustplc_device_semantics::pump::FAMILY,
        action: "hold_pressure",
        result_buckets: &[
            DeviceActionResultBucket::Complete,
            DeviceActionResultBucket::Timeout,
            DeviceActionResultBucket::MotionFault,
            DeviceActionResultBucket::SafetyFault,
        ],
    },
];

pub const HEATER_ACTION_CONTRACTS: &[DeviceActionContract<'static>] = &[
    DeviceActionContract {
        family: rustplc_device_semantics::heater::FAMILY,
        action: "heat_to",
        result_buckets: &[
            DeviceActionResultBucket::Complete,
            DeviceActionResultBucket::Timeout,
            DeviceActionResultBucket::Reject,
            DeviceActionResultBucket::MotionFault,
            DeviceActionResultBucket::SafetyFault,
        ],
    },
    DeviceActionContract {
        family: rustplc_device_semantics::heater::FAMILY,
        action: "stop_heat",
        result_buckets: &[
            DeviceActionResultBucket::Complete,
            DeviceActionResultBucket::Timeout,
        ],
    },
];

pub const VISION_ACTION_CONTRACTS: &[DeviceActionContract<'static>] = &[
    DeviceActionContract {
        family: rustplc_device_semantics::vision::FAMILY,
        action: "inspect",
        result_buckets: &[
            DeviceActionResultBucket::Complete,
            DeviceActionResultBucket::Timeout,
            DeviceActionResultBucket::Reject,
            DeviceActionResultBucket::MotionFault,
            DeviceActionResultBucket::SafetyFault,
        ],
    },
    DeviceActionContract {
        family: rustplc_device_semantics::vision::FAMILY,
        action: "reset_fault",
        result_buckets: &[
            DeviceActionResultBucket::Complete,
            DeviceActionResultBucket::Reject,
            DeviceActionResultBucket::SafetyFault,
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::{
        CONVEYOR_ACTION_CONTRACTS, GRIPPER_ACTION_CONTRACTS, HEATER_ACTION_CONTRACTS,
        PROPORTIONAL_VALVE_ACTION_CONTRACTS, PUMP_ACTION_CONTRACTS, VISION_ACTION_CONTRACTS,
    };
    use crate::device_semantics::DeviceActionResultBucket;

    #[test]
    fn process_device_contracts_expose_result_buckets() {
        let families = [
            PROPORTIONAL_VALVE_ACTION_CONTRACTS,
            GRIPPER_ACTION_CONTRACTS,
            CONVEYOR_ACTION_CONTRACTS,
            PUMP_ACTION_CONTRACTS,
            HEATER_ACTION_CONTRACTS,
            VISION_ACTION_CONTRACTS,
        ];

        for contracts in families {
            assert!(!contracts.is_empty());
            for contract in contracts {
                assert!(!contract.family.is_empty());
                assert!(!contract.action.is_empty());
                assert!(
                    contract
                        .result_buckets
                        .contains(&DeviceActionResultBucket::Complete),
                    "{:?} should expose a completion bucket",
                    contract
                );
            }
        }
    }
}
