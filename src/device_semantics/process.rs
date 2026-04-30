use super::{DeviceActionContract, DeviceActionResultBucket};

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
