use std::fmt::Write as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlcPortKind {
    DigitalInput,
    DigitalOutput,
    AnalogInput,
    AnalogOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlcPortRef {
    pub kind: PlcPortKind,
    pub id: u16,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ChannelRegistry;

impl ChannelRegistry {
    pub const fn new() -> Self {
        Self
    }

    pub fn resolve_endpoint(&self, name: &str) -> Option<PlcPortRef> {
        parse_plc_port_ref(name)
    }

    pub fn resolve_physical(&self, name: &str) -> Option<PlcPortRef> {
        parse_physical_plc_port_ref(name)
    }

    pub fn canonical_device_name(&self, port: PlcPortRef) -> String {
        canonical_physical_device_name(port.kind, port.id)
    }

    pub fn io_map_key(&self, port: PlcPortRef) -> String {
        io_map_key_for_port(port.kind, port.id)
    }
}

pub fn parse_physical_digital_input_id(name: &str) -> Option<u16> {
    parse_prefixed_u16_exact(name, 'X')
}

pub fn parse_physical_digital_output_id(name: &str) -> Option<u16> {
    parse_prefixed_u16_exact(name, 'Y')
}

pub fn parse_physical_analog_input_id(name: &str) -> Option<u16> {
    parse_prefixed_token_u16_exact(name, "AI")
}

pub fn parse_physical_analog_output_id(name: &str) -> Option<u16> {
    parse_prefixed_token_u16_exact(name, "AO")
}

/// Resolve scenario/topology-facing digital input endpoints:
/// - physical `X<n>`
/// - logical `DI<n>`
pub fn parse_digital_input_endpoint_id(name: &str) -> Option<u16> {
    parse_prefixed_u16_case_insensitive(name, 'X')
        .or_else(|| parse_prefixed_token_u16_case_insensitive(name, "DI"))
}

/// Resolve scenario/topology-facing analog input endpoints:
/// - physical/logical `AI<n>`
pub fn parse_analog_input_endpoint_id(name: &str) -> Option<u16> {
    parse_prefixed_token_u16_case_insensitive(name, "AI")
}

pub fn parse_plc_port_ref(name: &str) -> Option<PlcPortRef> {
    if let Some(id) = parse_prefixed_u16_case_insensitive(name, 'X') {
        return Some(PlcPortRef {
            kind: PlcPortKind::DigitalInput,
            id,
        });
    }
    if let Some(id) = parse_prefixed_u16_case_insensitive(name, 'Y') {
        return Some(PlcPortRef {
            kind: PlcPortKind::DigitalOutput,
            id,
        });
    }
    if let Some(id) = parse_prefixed_token_u16_case_insensitive(name, "DI") {
        return Some(PlcPortRef {
            kind: PlcPortKind::DigitalInput,
            id,
        });
    }
    if let Some(id) = parse_prefixed_token_u16_case_insensitive(name, "DO") {
        return Some(PlcPortRef {
            kind: PlcPortKind::DigitalOutput,
            id,
        });
    }
    if let Some(id) = parse_prefixed_token_u16_case_insensitive(name, "AI") {
        return Some(PlcPortRef {
            kind: PlcPortKind::AnalogInput,
            id,
        });
    }
    if let Some(id) = parse_prefixed_token_u16_case_insensitive(name, "AO") {
        return Some(PlcPortRef {
            kind: PlcPortKind::AnalogOutput,
            id,
        });
    }
    None
}

pub fn parse_physical_plc_port_ref(name: &str) -> Option<PlcPortRef> {
    if let Some(id) = parse_physical_digital_input_id(name) {
        return Some(PlcPortRef {
            kind: PlcPortKind::DigitalInput,
            id,
        });
    }
    if let Some(id) = parse_physical_digital_output_id(name) {
        return Some(PlcPortRef {
            kind: PlcPortKind::DigitalOutput,
            id,
        });
    }
    if let Some(id) = parse_physical_analog_input_id(name) {
        return Some(PlcPortRef {
            kind: PlcPortKind::AnalogInput,
            id,
        });
    }
    if let Some(id) = parse_physical_analog_output_id(name) {
        return Some(PlcPortRef {
            kind: PlcPortKind::AnalogOutput,
            id,
        });
    }
    None
}

pub fn canonical_physical_device_name(kind: PlcPortKind, id: u16) -> String {
    let mut out = String::new();
    match kind {
        PlcPortKind::DigitalInput => {
            out.push('X');
            let _ = write!(&mut out, "{id}");
        }
        PlcPortKind::DigitalOutput => {
            out.push('Y');
            let _ = write!(&mut out, "{id}");
        }
        PlcPortKind::AnalogInput => {
            out.push_str("AI");
            let _ = write!(&mut out, "{id}");
        }
        PlcPortKind::AnalogOutput => {
            out.push_str("AO");
            let _ = write!(&mut out, "{id}");
        }
    }
    out
}

pub fn io_map_key_for_port(kind: PlcPortKind, id: u16) -> String {
    let mut out = String::new();
    match kind {
        PlcPortKind::DigitalInput => out.push_str("di"),
        PlcPortKind::DigitalOutput => out.push_str("do"),
        PlcPortKind::AnalogInput => out.push_str("ai"),
        PlcPortKind::AnalogOutput => out.push_str("ao"),
    }
    let _ = write!(&mut out, "{id}");
    out
}

fn parse_decimal_u16(s: &str) -> Option<u16> {
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    s.parse::<u16>().ok()
}

fn parse_prefixed_u16_exact(name: &str, prefix: char) -> Option<u16> {
    let rest = name.strip_prefix(prefix)?;
    parse_decimal_u16(rest)
}

fn parse_prefixed_token_u16_exact(name: &str, prefix: &str) -> Option<u16> {
    let rest = name.strip_prefix(prefix)?;
    parse_decimal_u16(rest)
}

fn parse_prefixed_u16_case_insensitive(name: &str, prefix: char) -> Option<u16> {
    let mut chars = name.chars();
    let first = chars.next()?;
    if first.to_ascii_uppercase() != prefix {
        return None;
    }
    let rest: String = chars.collect();
    parse_decimal_u16(&rest)
}

fn parse_prefixed_token_u16_case_insensitive(name: &str, prefix: &str) -> Option<u16> {
    let (head, rest) = name.split_at(prefix.len().min(name.len()));
    if head.eq_ignore_ascii_case(prefix) {
        parse_decimal_u16(rest)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_physical_port_ids_with_strict_casing() {
        assert_eq!(parse_physical_digital_input_id("X0"), Some(0));
        assert_eq!(parse_physical_digital_output_id("Y7"), Some(7));
        assert_eq!(parse_physical_analog_input_id("AI2"), Some(2));
        assert_eq!(parse_physical_analog_output_id("AO5"), Some(5));

        assert_eq!(parse_physical_digital_input_id("x0"), None);
        assert_eq!(parse_physical_analog_input_id("ai2"), None);
    }

    #[test]
    fn parses_endpoint_ids_case_insensitively_with_aliases() {
        assert_eq!(parse_digital_input_endpoint_id("X3"), Some(3));
        assert_eq!(parse_digital_input_endpoint_id("x3"), Some(3));
        assert_eq!(parse_digital_input_endpoint_id("DI3"), Some(3));
        assert_eq!(parse_digital_input_endpoint_id("di3"), Some(3));
        assert_eq!(parse_analog_input_endpoint_id("AI9"), Some(9));
        assert_eq!(parse_analog_input_endpoint_id("ai9"), Some(9));
    }

    #[test]
    fn parses_unified_plc_port_reference() {
        assert_eq!(
            parse_plc_port_ref("Y1"),
            Some(PlcPortRef {
                kind: PlcPortKind::DigitalOutput,
                id: 1
            })
        );
        assert_eq!(
            parse_plc_port_ref("do12"),
            Some(PlcPortRef {
                kind: PlcPortKind::DigitalOutput,
                id: 12
            })
        );
        assert_eq!(
            parse_plc_port_ref("AI4"),
            Some(PlcPortRef {
                kind: PlcPortKind::AnalogInput,
                id: 4
            })
        );
        assert_eq!(parse_plc_port_ref("sensor_A"), None);
    }

    #[test]
    fn channel_registry_exposes_canonical_names_and_io_map_keys() {
        let registry = ChannelRegistry::new();
        let r = registry.resolve_endpoint("do2").expect("do2");
        assert_eq!(registry.canonical_device_name(r), "Y2");
        assert_eq!(registry.io_map_key(r), "do2");

        let x = registry.resolve_physical("X7").expect("X7");
        assert_eq!(registry.io_map_key(x), "di7");
    }
}
