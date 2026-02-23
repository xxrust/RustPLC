use crate::ast::DeviceType;

const KNOWN_SUBTYPES: &[&str] = &[
    "push_button",
    "e_stop_button",
    "limit_switch",
    "proximity_sensor",
    "selector_switch",
    "indicator_light",
];

const INPUT_FAMILY: &[&str] = &["digital_input", "sensor"];
const OUTPUT_FAMILY: &[&str] = &["digital_output"];

pub fn normalize_subtype(raw: &str) -> String {
    let mut normalized = String::new();
    let mut pending_separator = false;

    for ch in raw.trim().chars() {
        if ch.is_ascii_whitespace() || ch == '-' || ch == '_' {
            pending_separator = !normalized.is_empty();
            continue;
        }

        if pending_separator {
            normalized.push('_');
            pending_separator = false;
        }

        normalized.push(ch.to_ascii_lowercase());
    }

    normalized
}

pub fn known_subtypes() -> &'static [&'static str] {
    KNOWN_SUBTYPES
}

pub fn subtype_compatible_base_types(subtype: &str) -> Option<&'static [&'static str]> {
    let normalized = normalize_subtype(subtype);
    match normalized.as_str() {
        "push_button" | "e_stop_button" | "limit_switch" | "proximity_sensor"
        | "selector_switch" => Some(INPUT_FAMILY),
        "indicator_light" => Some(OUTPUT_FAMILY),
        _ => None,
    }
}

pub fn is_known_subtype(subtype: &str) -> bool {
    subtype_compatible_base_types(subtype).is_some()
}

pub fn subtype_matches_device_type(subtype: &str, device_type: &DeviceType) -> bool {
    let Some(base_types) = subtype_compatible_base_types(subtype) else {
        return false;
    };
    let base = device_type_label(device_type);
    base_types.contains(&base)
}

fn device_type_label(device_type: &DeviceType) -> &'static str {
    match device_type {
        DeviceType::DigitalOutput => "digital_output",
        DeviceType::DigitalInput => "digital_input",
        DeviceType::SolenoidValve => "solenoid_valve",
        DeviceType::Cylinder => "cylinder",
        DeviceType::Sensor => "sensor",
        DeviceType::Motor => "motor",
        DeviceType::AnalogInput => "analog_input",
        DeviceType::AnalogOutput => "analog_output",
        DeviceType::Pid => "pid",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        is_known_subtype, known_subtypes, normalize_subtype, subtype_compatible_base_types,
        subtype_matches_device_type,
    };
    use crate::ast::DeviceType;

    #[test]
    fn normalizes_case_spaces_and_hyphen() {
        assert_eq!(normalize_subtype("  E-Stop Button  "), "e_stop_button");
        assert_eq!(normalize_subtype("Limit   Switch"), "limit_switch");
        assert_eq!(normalize_subtype("selector_switch"), "selector_switch");
    }

    #[test]
    fn exposes_supported_subtype_list() {
        assert_eq!(
            known_subtypes(),
            &[
                "push_button",
                "e_stop_button",
                "limit_switch",
                "proximity_sensor",
                "selector_switch",
                "indicator_light",
            ]
        );
    }

    #[test]
    fn resolves_subtype_compatibility_matrix() {
        assert_eq!(
            subtype_compatible_base_types("push button"),
            Some(&["digital_input", "sensor"][..])
        );
        assert_eq!(
            subtype_compatible_base_types("Indicator-Light"),
            Some(&["digital_output"][..])
        );
        assert_eq!(subtype_compatible_base_types("unknown"), None);
        assert!(is_known_subtype("limit-switch"));
        assert!(!is_known_subtype("foo"));
    }

    #[test]
    fn checks_device_type_compatibility_from_matrix() {
        assert!(subtype_matches_device_type(
            "push_button",
            &DeviceType::DigitalInput
        ));
        assert!(subtype_matches_device_type(
            "push_button",
            &DeviceType::Sensor
        ));
        assert!(!subtype_matches_device_type(
            "push_button",
            &DeviceType::DigitalOutput
        ));
        assert!(subtype_matches_device_type(
            "indicator_light",
            &DeviceType::DigitalOutput
        ));
    }
}
