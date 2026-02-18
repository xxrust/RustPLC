use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SafeStateMode {
    /// Default policy: write 0/false for all outputs on exit.
    AllZero,
    /// Use per-output safe values/groups from the io_map's `[safe_state.*]` sections.
    Profile,
}

impl Default for SafeStateMode {
    fn default() -> Self {
        Self::AllZero
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SafeStateBoolEntry {
    pub safe_value: bool,
    pub group: u16,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
pub struct SafeStateF32Entry {
    pub safe_value: f32,
    pub group: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SafeStateConfig {
    #[serde(default)]
    pub mode: SafeStateMode,
    #[serde(default)]
    pub on_exit_timeout_ms: u64,
    /// Digital output safe values (by logical DO id).
    #[serde(default)]
    pub digital_outputs: BTreeMap<u16, SafeStateBoolEntry>,
    /// Analog output safe values (by logical AO id).
    #[serde(default)]
    pub analog_outputs: BTreeMap<u16, SafeStateF32Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct IoMap {
    pub digital_inputs: BTreeMap<u16, u8>,
    pub digital_outputs: BTreeMap<u16, u8>,
    #[serde(default)]
    pub analog_inputs: BTreeMap<u16, u8>,
    #[serde(default)]
    pub analog_outputs: BTreeMap<u16, u8>,
    #[serde(default)]
    pub safe_state: SafeStateConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IoUsage {
    pub digital_inputs: &'static [u16],
    pub digital_outputs: &'static [u16],
    pub analog_inputs: &'static [u16],
    pub analog_outputs: &'static [u16],
}

#[derive(Debug, thiserror::Error)]
pub enum IoMapError {
    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("expected a table [{section}] in io map")]
    MissingSection { section: &'static str },

    #[error(
        "invalid key {key:?} in [{section}] (expected prefix {prefix:?} + integer id, e.g. {example:?})"
    )]
    InvalidKey {
        section: &'static str,
        key: String,
        prefix: &'static str,
        example: &'static str,
    },

    #[error("invalid gpio number {gpio} for {kind}{id} (allowed: {allowed})")]
    InvalidGpio {
        kind: &'static str,
        id: u16,
        gpio: i64,
        allowed: &'static str,
    },

    #[error("gpio {gpio} is assigned multiple times ({a} and {b})")]
    DuplicateGpio { gpio: u8, a: String, b: String },

    #[error("missing required mapping for {kind}{id}")]
    MissingRequired { kind: &'static str, id: u16 },

    #[error("invalid [safe_state] section: {details}")]
    InvalidSafeState { details: String },
}

impl IoMap {
    pub fn from_toml_str(input: &str) -> Result<Self, IoMapError> {
        let v: toml::Value = toml::from_str(input)?;

        let di_table = v.get("digital_inputs").and_then(|v| v.as_table()).ok_or(
            IoMapError::MissingSection {
                section: "digital_inputs",
            },
        )?;
        let do_table = v.get("digital_outputs").and_then(|v| v.as_table()).ok_or(
            IoMapError::MissingSection {
                section: "digital_outputs",
            },
        )?;
        let ai_table = v.get("analog_inputs").and_then(|v| v.as_table());
        let ao_table = v.get("analog_outputs").and_then(|v| v.as_table());

        let digital_inputs =
            parse_map_section(di_table, "digital_inputs", "di", "di0", 0, 29, "0..=29")?;
        let digital_outputs =
            parse_map_section(do_table, "digital_outputs", "do", "do0", 0, 29, "0..=29")?;
        let analog_inputs = match ai_table {
            Some(t) => parse_map_section(
                t,
                "analog_inputs",
                "ai",
                "ai0",
                26,
                29,
                "26..=29 (RP2040 ADC-capable GPIO)",
            )?,
            None => BTreeMap::new(),
        };
        let analog_outputs = match ao_table {
            Some(t) => parse_map_section(t, "analog_outputs", "ao", "ao0", 0, 29, "0..=29")?,
            None => BTreeMap::new(),
        };

        let safe_state = parse_safe_state(&v)?;

        Ok(Self {
            digital_inputs,
            digital_outputs,
            analog_inputs,
            analog_outputs,
            safe_state,
        })
    }

    pub fn validate_for_usage(&self, usage: IoUsage) -> Result<(), IoMapError> {
        // Ensure required ids exist.
        for &id in usage.digital_inputs {
            if !self.digital_inputs.contains_key(&id) {
                return Err(IoMapError::MissingRequired { kind: "di", id });
            }
        }
        for &id in usage.digital_outputs {
            if !self.digital_outputs.contains_key(&id) {
                return Err(IoMapError::MissingRequired { kind: "do", id });
            }
        }
        for &id in usage.analog_outputs {
            if !self.analog_outputs.contains_key(&id) {
                return Err(IoMapError::MissingRequired { kind: "ao", id });
            }
        }
        for &id in usage.analog_inputs {
            if !self.analog_inputs.contains_key(&id) {
                return Err(IoMapError::MissingRequired { kind: "ai", id });
            }
        }

        // Ensure no gpio is assigned twice across DI/DO sets.
        let mut seen = BTreeMap::<u8, String>::new();
        for (&id, &gpio) in &self.digital_inputs {
            let key = format!("di{id}");
            if let Some(prev) = seen.insert(gpio, key.clone()) {
                return Err(IoMapError::DuplicateGpio {
                    gpio,
                    a: prev,
                    b: key,
                });
            }
        }
        for (&id, &gpio) in &self.digital_outputs {
            let key = format!("do{id}");
            if let Some(prev) = seen.insert(gpio, key.clone()) {
                return Err(IoMapError::DuplicateGpio {
                    gpio,
                    a: prev,
                    b: key,
                });
            }
        }
        for (&id, &gpio) in &self.analog_outputs {
            let key = format!("ao{id}");
            if let Some(prev) = seen.insert(gpio, key.clone()) {
                return Err(IoMapError::DuplicateGpio {
                    gpio,
                    a: prev,
                    b: key,
                });
            }
        }
        for (&id, &gpio) in &self.analog_inputs {
            let key = format!("ai{id}");
            if let Some(prev) = seen.insert(gpio, key.clone()) {
                return Err(IoMapError::DuplicateGpio {
                    gpio,
                    a: prev,
                    b: key,
                });
            }
        }

        Ok(())
    }

    pub fn referenced_digital_inputs(&self) -> BTreeSet<u16> {
        self.digital_inputs.keys().copied().collect()
    }

    pub fn referenced_digital_outputs(&self) -> BTreeSet<u16> {
        self.digital_outputs.keys().copied().collect()
    }
}

fn parse_safe_state(v: &toml::Value) -> Result<SafeStateConfig, IoMapError> {
    let Some(ss) = v.get("safe_state") else {
        return Ok(SafeStateConfig::default());
    };
    let Some(ss) = ss.as_table() else {
        return Err(IoMapError::InvalidSafeState {
            details: "safe_state must be a table".to_string(),
        });
    };

    let mode = match ss.get("mode").and_then(|v| v.as_str()) {
        None => SafeStateMode::AllZero,
        Some("all_zero") => SafeStateMode::AllZero,
        Some("profile") => SafeStateMode::Profile,
        Some(other) => {
            return Err(IoMapError::InvalidSafeState {
                details: format!("safe_state.mode must be all_zero|profile, got {other:?}"),
            });
        }
    };

    let on_exit_timeout_ms = ss
        .get("on_exit_timeout_ms")
        .and_then(|v| v.as_integer())
        .unwrap_or(0);
    if on_exit_timeout_ms < 0 {
        return Err(IoMapError::InvalidSafeState {
            details: format!("safe_state.on_exit_timeout_ms must be >= 0, got {on_exit_timeout_ms}"),
        });
    }

    let digital_outputs = parse_safe_do_section(ss.get("do"))?;
    let analog_outputs = parse_safe_ao_section(ss.get("ao"))?;

    Ok(SafeStateConfig {
        mode,
        on_exit_timeout_ms: on_exit_timeout_ms as u64,
        digital_outputs,
        analog_outputs,
    })
}

fn parse_safe_do_section(
    v: Option<&toml::Value>,
) -> Result<BTreeMap<u16, SafeStateBoolEntry>, IoMapError> {
    let Some(v) = v else {
        return Ok(BTreeMap::new());
    };
    let Some(t) = v.as_table() else {
        return Err(IoMapError::InvalidSafeState {
            details: "safe_state.do must be a table".to_string(),
        });
    };

    let mut out = BTreeMap::new();
    for (k, v) in t {
        let Some(entry) = v.as_table() else {
            return Err(IoMapError::InvalidSafeState {
                details: format!("safe_state.do.{k} must be a table"),
            });
        };
        let id = parse_safe_state_id(k, &["Y", "do"])?;

        let safe_value = match entry.get("safe_value") {
            Some(toml::Value::Boolean(b)) => *b,
            Some(toml::Value::Integer(n)) => match *n {
                0 => false,
                1 => true,
                other => {
                    return Err(IoMapError::InvalidSafeState {
                        details: format!(
                            "safe_state.do.{k}.safe_value must be 0|1|bool, got {other}"
                        ),
                    });
                }
            },
            Some(other) => {
                return Err(IoMapError::InvalidSafeState {
                    details: format!(
                        "safe_state.do.{k}.safe_value must be 0|1|bool, got {other:?}"
                    ),
                });
            }
            None => {
                return Err(IoMapError::InvalidSafeState {
                    details: format!("safe_state.do.{k}.safe_value is required"),
                });
            }
        };

        let group = entry.get("group").and_then(|v| v.as_integer()).unwrap_or(0);
        if group < 0 || group > (u16::MAX as i64) {
            return Err(IoMapError::InvalidSafeState {
                details: format!("safe_state.do.{k}.group must be 0..={}", u16::MAX),
            });
        }

        out.insert(
            id,
            SafeStateBoolEntry {
                safe_value,
                group: group as u16,
            },
        );
    }
    Ok(out)
}

fn parse_safe_ao_section(
    v: Option<&toml::Value>,
) -> Result<BTreeMap<u16, SafeStateF32Entry>, IoMapError> {
    let Some(v) = v else {
        return Ok(BTreeMap::new());
    };
    let Some(t) = v.as_table() else {
        return Err(IoMapError::InvalidSafeState {
            details: "safe_state.ao must be a table".to_string(),
        });
    };

    let mut out = BTreeMap::new();
    for (k, v) in t {
        let Some(entry) = v.as_table() else {
            return Err(IoMapError::InvalidSafeState {
                details: format!("safe_state.ao.{k} must be a table"),
            });
        };
        let id = parse_safe_state_id(k, &["AO", "ao"])?;

        let safe_value = match entry.get("safe_value") {
            Some(toml::Value::Float(n)) => *n as f32,
            Some(toml::Value::Integer(n)) => *n as f32,
            Some(other) => {
                return Err(IoMapError::InvalidSafeState {
                    details: format!(
                        "safe_state.ao.{k}.safe_value must be a number, got {other:?}"
                    ),
                });
            }
            None => {
                return Err(IoMapError::InvalidSafeState {
                    details: format!("safe_state.ao.{k}.safe_value is required"),
                });
            }
        };

        let group = entry.get("group").and_then(|v| v.as_integer()).unwrap_or(0);
        if group < 0 || group > (u16::MAX as i64) {
            return Err(IoMapError::InvalidSafeState {
                details: format!("safe_state.ao.{k}.group must be 0..={}", u16::MAX),
            });
        }

        out.insert(
            id,
            SafeStateF32Entry {
                safe_value,
                group: group as u16,
            },
        );
    }
    Ok(out)
}

fn parse_safe_state_id(key: &str, prefixes: &[&str]) -> Result<u16, IoMapError> {
    for p in prefixes {
        if let Some(rest) = key.strip_prefix(p) {
            if let Ok(id) = rest.parse::<u16>() {
                return Ok(id);
            }
        }
    }
    if let Ok(id) = key.parse::<u16>() {
        return Ok(id);
    }
    Err(IoMapError::InvalidSafeState {
        details: format!(
            "invalid safe_state key {key:?} (expected prefixes {:?} + integer id)",
            prefixes
        ),
    })
}

fn parse_map_section(
    t: &toml::value::Table,
    section: &'static str,
    prefix: &'static str,
    example: &'static str,
    min_gpio: i64,
    max_gpio: i64,
    allowed: &'static str,
) -> Result<BTreeMap<u16, u8>, IoMapError> {
    let mut out = BTreeMap::<u16, u8>::new();
    for (k, v) in t {
        let id_str = k
            .strip_prefix(prefix)
            .ok_or_else(|| IoMapError::InvalidKey {
                section,
                key: k.clone(),
                prefix,
                example,
            })?;
        let id: u16 = id_str.parse().map_err(|_| IoMapError::InvalidKey {
            section,
            key: k.clone(),
            prefix,
            example,
        })?;
        let gpio = v.as_integer().ok_or_else(|| IoMapError::InvalidGpio {
            kind: prefix,
            id,
            gpio: i64::MIN,
            allowed,
        })?;
        if !(min_gpio..=max_gpio).contains(&gpio) {
            return Err(IoMapError::InvalidGpio {
                kind: prefix,
                id,
                gpio,
                allowed,
            });
        }
        out.insert(id, gpio as u8);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage_one_di_do() -> IoUsage {
        static DIS: [u16; 1] = [0];
        static DOS: [u16; 1] = [0];
        static AIS: [u16; 0] = [];
        static AOS: [u16; 0] = [];
        IoUsage {
            digital_inputs: &DIS,
            digital_outputs: &DOS,
            analog_inputs: &AIS,
            analog_outputs: &AOS,
        }
    }

    fn usage_with_ai0_ao0() -> IoUsage {
        static DIS: [u16; 1] = [0];
        static DOS: [u16; 1] = [0];
        static AIS: [u16; 1] = [0];
        static AOS: [u16; 1] = [0];
        IoUsage {
            digital_inputs: &DIS,
            digital_outputs: &DOS,
            analog_inputs: &AIS,
            analog_outputs: &AOS,
        }
    }

    #[test]
    fn parses_and_validates_ok() {
        let input = r#"
[digital_inputs]
di0 = 2

[digital_outputs]
do0 = 16

[analog_inputs]
ai0 = 26

[analog_outputs]
ao0 = 20
"#;
        let m = IoMap::from_toml_str(input).expect("parse");
        m.validate_for_usage(usage_with_ai0_ao0())
            .expect("validate");
    }

    #[test]
    fn rejects_duplicate_gpio() {
        let input = r#"
[digital_inputs]
di0 = 2

[digital_outputs]
do0 = 2
"#;
        let m = IoMap::from_toml_str(input).expect("parse");
        let err = m.validate_for_usage(usage_one_di_do()).unwrap_err();
        assert!(err.to_string().contains("assigned multiple times"));
    }

    #[test]
    fn rejects_missing_required() {
        let input = r#"
[digital_inputs]
di0 = 2

[digital_outputs]
"#;
        let m = IoMap::from_toml_str(input).expect("parse");
        let err = m.validate_for_usage(usage_one_di_do()).unwrap_err();
        assert!(err.to_string().contains("missing required mapping"));
    }

    #[test]
    fn rejects_invalid_gpio_range() {
        let input = r#"
[digital_inputs]
di0 = 99

[digital_outputs]
do0 = 16
"#;
        let err = IoMap::from_toml_str(input).unwrap_err();
        assert!(err.to_string().contains("allowed"));
    }

    #[test]
    fn rejects_missing_required_analog_output() {
        let input = r#"
[digital_inputs]
di0 = 2

[digital_outputs]
do0 = 16
"#;
        let m = IoMap::from_toml_str(input).expect("parse");
        let err = m.validate_for_usage(usage_with_ai0_ao0()).unwrap_err();
        assert!(err.to_string().contains("missing required mapping for ao0"));
    }

    #[test]
    fn rejects_missing_required_analog_input() {
        let input = r#"
[digital_inputs]
di0 = 2

[digital_outputs]
do0 = 16

[analog_outputs]
ao0 = 20
"#;
        let m = IoMap::from_toml_str(input).expect("parse");
        let err = m.validate_for_usage(usage_with_ai0_ao0()).unwrap_err();
        assert!(err.to_string().contains("missing required mapping for ai0"));
    }

    #[test]
    fn rejects_analog_input_on_non_adc_gpio() {
        let input = r#"
[digital_inputs]
di0 = 2

[digital_outputs]
do0 = 16

[analog_inputs]
ai0 = 20

[analog_outputs]
ao0 = 21
"#;
        let err = IoMap::from_toml_str(input).unwrap_err();
        assert!(
            err.to_string()
                .contains("26..=29 (RP2040 ADC-capable GPIO)")
        );
    }

    #[test]
    fn parse_safe_state_all_zero_default_when_missing() {
        let toml = r#"
[digital_inputs]
di0 = 0

[digital_outputs]
do0 = 1
"#;
        let map = IoMap::from_toml_str(toml).unwrap();
        assert_eq!(map.safe_state.mode, SafeStateMode::AllZero);
        assert!(map.safe_state.digital_outputs.is_empty());
        assert!(map.safe_state.analog_outputs.is_empty());
    }

    #[test]
    fn parse_safe_state_profile_with_nested_tables() {
        let toml = r#"
[digital_inputs]
di0 = 0

[digital_outputs]
do0 = 1
do2 = 3

[safe_state]
mode = "profile"

[safe_state.do.Y2]
safe_value = 0
group = 10

[safe_state.do.do0]
safe_value = true
group = 20

[safe_state.ao.AO0]
safe_value = 0.0
group = 30
"#;
        let map = IoMap::from_toml_str(toml).unwrap();
        assert_eq!(map.safe_state.mode, SafeStateMode::Profile);
        assert_eq!(map.safe_state.digital_outputs.get(&2).unwrap().safe_value, false);
        assert_eq!(map.safe_state.digital_outputs.get(&2).unwrap().group, 10);
        assert_eq!(map.safe_state.digital_outputs.get(&0).unwrap().safe_value, true);
        assert_eq!(map.safe_state.analog_outputs.get(&0).unwrap().group, 30);
    }
}
