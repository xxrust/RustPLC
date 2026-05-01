use crate::ast::{ActionTarget, DeviceType};
use std::collections::HashMap;

pub fn raw_drive_provider_bypass_message(
    target: &ActionTarget,
    device_types: &HashMap<&str, &DeviceType>,
) -> Option<String> {
    let has_process_drive_consumer = device_types.values().any(|kind| {
        matches!(
            kind,
            DeviceType::Conveyor | DeviceType::Pump | DeviceType::Heater
        )
    });
    if !has_process_drive_consumer {
        return None;
    }

    let device_type = device_types.get(target.device.as_str())?;
    let is_drive_provider = matches!(device_type, DeviceType::Motor | DeviceType::Vfd);
    let writes_drive_port = matches!(
        target.port.as_str(),
        "run" | "direction" | "cmd" | "speed_cmd" | "frequency_cmd" | "self"
    );
    if is_drive_provider && writes_drive_port {
        return Some(format!(
            "normal task action writes drive capability provider port `{target}` directly while a process device is declared"
        ));
    }

    None
}
