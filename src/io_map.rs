use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct IoMap {
    pub digital_inputs: BTreeMap<u16, u8>,
    pub digital_outputs: BTreeMap<u16, u8>,
    #[serde(default)]
    pub analog_inputs: BTreeMap<u16, u8>,
    #[serde(default)]
    pub analog_outputs: BTreeMap<u16, u8>,
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

        Ok(Self {
            digital_inputs,
            digital_outputs,
            analog_inputs,
            analog_outputs,
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
}
