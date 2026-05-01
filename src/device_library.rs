use crate::error::PlcError;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceDef {
    pub identity: DeviceIdentity,
    #[serde(default)]
    pub interfaces: DeviceInterfaces,
    #[serde(default)]
    pub parameters: Vec<DeviceParameterDef>,
    #[serde(default)]
    pub device_constraints: DeviceConstraints,
    #[serde(default)]
    pub interface_contract: Option<DeviceInterfaceContract>,
    #[serde(default)]
    pub capabilities: Vec<DeviceCapability>,
    #[serde(default)]
    pub defaults: Option<DeviceDefaults>,
    #[serde(default)]
    pub alarm_map: Option<DeviceAlarmMap>,
    #[serde(default)]
    pub verification_contract: Option<DeviceVerificationContract>,
    #[serde(default)]
    pub codegen_support: Option<DeviceCodegenSupport>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceIdentity {
    pub name: String,
    #[serde(rename = "type")]
    pub device_type: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DeviceInterfaces {
    #[serde(default)]
    pub ports: Vec<PortDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PortDef {
    pub name: String,
    #[serde(default)]
    pub states: Vec<String>,
    #[serde(default = "default_state_default")]
    pub default_state: String,
    #[serde(default)]
    pub direction: String,
    #[serde(default)]
    pub port_type: String,
    #[serde(default)]
    pub range_min: Option<f64>,
    #[serde(default)]
    pub range_max: Option<f64>,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub external: bool,
}

fn default_state_default() -> String {
    String::new()
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DeviceConstraints {
    #[serde(default)]
    pub safety: Vec<DeviceSafetyConstraint>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceSafetyConstraint {
    pub left: String,
    pub right: String,
    pub relation: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceParameterDef {
    pub name: String,
    #[serde(rename = "type")]
    pub parameter_type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: String,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DeviceInterfaceContract {
    #[serde(default)]
    pub ports: Vec<DeviceInterfacePortContract>,
    #[serde(default)]
    pub actions: Vec<DeviceInterfaceActionContract>,
    #[serde(flatten)]
    pub metadata: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceInterfacePortContract {
    pub name: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub allowed_states: Vec<String>,
    #[serde(default)]
    pub direction: String,
    #[serde(default)]
    pub description: String,
    #[serde(flatten)]
    pub metadata: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceInterfaceActionContract {
    pub name: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub consumes: Vec<String>,
    #[serde(default)]
    pub produces: Vec<String>,
    #[serde(default)]
    pub faults: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(flatten)]
    pub metadata: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceCapability {
    pub name: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub ports: Vec<String>,
    #[serde(default)]
    pub parameters: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(flatten)]
    pub metadata: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DeviceDefaults {
    #[serde(default)]
    pub parameters: BTreeMap<String, toml::Value>,
    #[serde(default)]
    pub ports: BTreeMap<String, String>,
    #[serde(flatten)]
    pub metadata: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DeviceAlarmMap {
    #[serde(default)]
    pub entries: Vec<DeviceAlarmMapping>,
    #[serde(flatten)]
    pub metadata: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceAlarmMapping {
    pub code: String,
    #[serde(default)]
    pub condition: String,
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub recoverable: bool,
    #[serde(default)]
    pub description: String,
    #[serde(flatten)]
    pub metadata: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DeviceVerificationContract {
    #[serde(default)]
    pub assumptions: Vec<String>,
    #[serde(default)]
    pub safety: Vec<String>,
    #[serde(default)]
    pub liveness: Vec<String>,
    #[serde(default)]
    pub timing: Vec<String>,
    #[serde(default)]
    pub causality: Vec<String>,
    #[serde(flatten)]
    pub metadata: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DeviceCodegenSupport {
    #[serde(default)]
    pub targets: Vec<String>,
    #[serde(default)]
    pub unsupported_targets: Vec<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(flatten)]
    pub metadata: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Default)]
pub struct DeviceLibrary {
    defs: HashMap<String, DeviceDef>,
}

impl DeviceLibrary {
    pub fn load(dir: &Path) -> Result<Self, Vec<PlcError>> {
        if !dir.exists() || !dir.is_dir() {
            return Ok(Self::empty());
        }

        let entries = std::fs::read_dir(dir).map_err(|e| {
            vec![PlcError::device_library_io_error(
                dir.display().to_string(),
                e.to_string(),
            )]
        })?;

        let mut defs = HashMap::new();
        let mut errors = Vec::new();

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    errors.push(PlcError::device_library_io_error(
                        dir.display().to_string(),
                        e.to_string(),
                    ));
                    continue;
                }
            };

            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }

            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) => {
                    errors.push(PlcError::device_library_io_error(
                        path.display().to_string(),
                        e.to_string(),
                    ));
                    continue;
                }
            };

            let def: DeviceDef = match toml::from_str(&content) {
                Ok(d) => d,
                Err(e) => {
                    errors.push(PlcError::device_library_parse_error(
                        path.display().to_string(),
                        e.to_string(),
                    ));
                    continue;
                }
            };

            let key = def.identity.device_type.clone();
            defs.insert(key, def);
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(Self { defs })
    }

    pub fn empty() -> Self {
        Self {
            defs: HashMap::new(),
        }
    }

    pub fn get(&self, type_key: &str) -> Option<&DeviceDef> {
        self.defs.get(type_key)
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    pub fn alarm_coverage_gaps(&self, type_key: &str) -> Option<Vec<String>> {
        let def = self.defs.get(type_key)?;
        let declared = declared_fault_conditions(def);
        if declared.is_empty() {
            return Some(Vec::new());
        }
        let mapped = def
            .alarm_map
            .as_ref()
            .map(mapped_alarm_conditions)
            .unwrap_or_default();
        Some(
            declared
                .into_iter()
                .filter(|condition| !mapped.contains(condition))
                .collect(),
        )
    }
}

fn declared_fault_conditions(def: &DeviceDef) -> BTreeSet<String> {
    let mut conditions = BTreeSet::new();
    if let Some(contract) = &def.interface_contract {
        for action in &contract.actions {
            for fault in &action.faults {
                if !fault.trim().is_empty() {
                    conditions.insert(fault.clone());
                }
            }
        }
    }
    conditions
}

fn mapped_alarm_conditions(alarm_map: &DeviceAlarmMap) -> BTreeSet<String> {
    alarm_map
        .entries
        .iter()
        .filter_map(|entry| {
            let condition = entry.condition.trim();
            if condition.is_empty() {
                None
            } else {
                Some(condition.to_string())
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_library_returns_none() {
        let lib = DeviceLibrary::empty();
        assert!(lib.is_empty());
        assert!(lib.get("cylinder").is_none());
    }

    #[test]
    fn load_nonexistent_dir_returns_empty() {
        let lib = DeviceLibrary::load(Path::new("nonexistent_dir_12345")).unwrap();
        assert!(lib.is_empty());
    }

    #[test]
    fn parse_toml_inline() {
        let toml_str = r#"
[identity]
name = "双线圈电磁阀"
type = "solenoid_valve"

[[interfaces.ports]]
name = "coil_A"
states = ["on", "off"]
default_state = "off"
direction = "output"
port_type = "digital"

[[interfaces.ports]]
name = "coil_B"
states = ["on", "off"]
default_state = "off"
direction = "output"
port_type = "digital"

[[device_constraints.safety]]
left = "coil_A.on"
right = "coil_B.on"
relation = "conflicts_with"
reason = "双线圈不能同时通电"
"#;

        let def: DeviceDef = toml::from_str(toml_str).unwrap();
        assert_eq!(def.identity.device_type, "solenoid_valve");
        assert_eq!(def.interfaces.ports.len(), 2);
        assert_eq!(def.interfaces.ports[0].name, "coil_A");
        assert_eq!(def.interfaces.ports[0].states, vec!["on", "off"]);
        assert_eq!(def.device_constraints.safety.len(), 1);
        assert_eq!(def.device_constraints.safety[0].relation, "conflicts_with");
        assert!(def.interface_contract.is_none());
        assert!(def.capabilities.is_empty());
        assert!(def.defaults.is_none());
        assert!(def.alarm_map.is_none());
        assert!(def.verification_contract.is_none());
        assert!(def.codegen_support.is_none());
    }

    #[test]
    fn parse_toml_with_extended_contract_fields() {
        let toml_str = r#"
[identity]
name = "Servo Drive"
type = "servo_drive"

[[interfaces.ports]]
name = "enable"
states = ["on", "off"]
default_state = "off"
direction = "output"
port_type = "digital"

[interface_contract]
version = "1"

[[interface_contract.ports]]
name = "enable"
required = true
allowed_states = ["on", "off"]
direction = "output"

[[interface_contract.actions]]
name = "axis.move_absolute"
required = true
consumes = ["enable"]
produces = ["in_position"]
faults = ["timeout", "motion_fault"]

[[capabilities]]
name = "positioning"
kind = "motion"
ports = ["enable", "in_position"]
parameters = ["max_speed"]
description = "Absolute positioning support"

[defaults.parameters]
max_speed = 1200
home_required = true

[defaults.ports]
enable = "off"

[alarm_map]
source = "drive_status"

[[alarm_map.entries]]
code = "ALM_TIMEOUT"
condition = "timeout"
severity = "fault"
recoverable = true
description = "Motion command timed out"

[verification_contract]
assumptions = ["axis is homed before motion"]
safety = ["enable must be off on fault"]
liveness = ["move eventually reaches done or fault"]
timing = ["move_absolute completes within configured timeout"]
causality = ["command edge drives motion request"]

[codegen_support]
targets = ["st", "openplc"]
unsupported_targets = ["ladder"]
notes = "Requires motion FB mapping"
"#;

        let def: DeviceDef = toml::from_str(toml_str).unwrap();

        let interface_contract = def.interface_contract.as_ref().unwrap();
        assert_eq!(
            interface_contract
                .metadata
                .get("version")
                .and_then(toml::Value::as_str),
            Some("1")
        );
        assert_eq!(interface_contract.ports.len(), 1);
        assert_eq!(interface_contract.ports[0].name, "enable");
        assert!(interface_contract.ports[0].required);
        assert_eq!(interface_contract.actions.len(), 1);
        assert_eq!(
            interface_contract.actions[0].faults,
            vec!["timeout", "motion_fault"]
        );

        assert_eq!(def.capabilities.len(), 1);
        assert_eq!(def.capabilities[0].name, "positioning");
        assert_eq!(def.capabilities[0].kind, "motion");

        let defaults = def.defaults.as_ref().unwrap();
        assert_eq!(
            defaults
                .parameters
                .get("max_speed")
                .and_then(toml::Value::as_integer),
            Some(1200)
        );
        assert_eq!(
            defaults
                .parameters
                .get("home_required")
                .and_then(toml::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            defaults.ports.get("enable").map(String::as_str),
            Some("off")
        );

        let alarm_map = def.alarm_map.as_ref().unwrap();
        assert_eq!(
            alarm_map
                .metadata
                .get("source")
                .and_then(toml::Value::as_str),
            Some("drive_status")
        );
        assert_eq!(alarm_map.entries.len(), 1);
        assert_eq!(alarm_map.entries[0].code, "ALM_TIMEOUT");
        assert!(alarm_map.entries[0].recoverable);

        let verification_contract = def.verification_contract.as_ref().unwrap();
        assert_eq!(
            verification_contract.safety,
            vec!["enable must be off on fault"]
        );
        assert_eq!(
            verification_contract.causality,
            vec!["command edge drives motion request"]
        );

        let codegen_support = def.codegen_support.as_ref().unwrap();
        assert_eq!(codegen_support.targets, vec!["st", "openplc"]);
        assert_eq!(codegen_support.unsupported_targets, vec!["ladder"]);
        assert_eq!(codegen_support.notes, "Requires motion FB mapping");
    }

    #[test]
    fn process_device_alarm_maps_cover_declared_fault_conditions() {
        let lib = DeviceLibrary::load(Path::new("devices")).expect("load device library");
        for type_key in [
            "proportional_valve",
            "gripper",
            "conveyor",
            "pump",
            "heater",
            "vision_sensor",
        ] {
            let def = lib.get(type_key).expect("process device definition");
            let alarm_map = def
                .alarm_map
                .as_ref()
                .expect("process device should declare alarm_map");
            assert!(
                alarm_map
                    .entries
                    .iter()
                    .all(|entry| !entry.code.is_empty() && !entry.severity.is_empty()),
                "{type_key} alarm_map entries should expose code and severity"
            );
            let gaps = lib
                .alarm_coverage_gaps(type_key)
                .expect("alarm coverage result");
            assert!(
                gaps.is_empty(),
                "{type_key} alarm_map missing declared faults: {gaps:?}"
            );
        }
    }
}
