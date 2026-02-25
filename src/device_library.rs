use crate::error::PlcError;
use serde::Deserialize;
use std::collections::HashMap;
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
    }
}
