use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MotionConfig {
    /// Stepper (Pulse/Dir/EN) axis configurations keyed by axis index (0, 1).
    #[serde(default)]
    pub stepper: BTreeMap<u8, StepperAxisConfig>,
    /// Incremental AB encoder configurations keyed by axis index (0, 1).
    #[serde(default)]
    pub encoder: BTreeMap<u8, AbEncoderAxisConfig>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CountSignConvention {
    /// Positive count matches "forward" according to driver/firmware wiring.
    Normal,
    /// Invert the computed sign/count before publishing to the PLC.
    Inverted,
}

impl Default for CountSignConvention {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StepperAxisConfig {
    pub step_gpio: u8,
    pub dir_gpio: u8,
    pub en_gpio: u8,
    #[serde(default)]
    pub dir_inverted: bool,
    /// Optional trapezoid profile defaults (steps/s and steps/s^2).
    #[serde(default)]
    pub v_max_sps: Option<u32>,
    #[serde(default)]
    pub acc_sps2: Option<u32>,
    #[serde(default)]
    pub dec_sps2: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AbEncoderAxisConfig {
    pub a_gpio: u8,
    pub b_gpio: u8,
    pub ppr: u32,
    /// Quadrature multiplier (typically 1, 2, or 4). Defaults to 4.
    #[serde(default = "default_quad")]
    pub quad: u8,
    #[serde(default)]
    pub count_sign: CountSignConvention,
    /// Optional scaling factor to publish into PLC engineering units.
    #[serde(default = "default_scale")]
    pub scale: f32,
}

fn default_quad() -> u8 {
    4
}

fn default_scale() -> f32 {
    1.0
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion: Option<MotionConfig>,
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

    #[error("invalid gpio value {value:?} for {kind}{id} (allowed: {allowed})")]
    InvalidGpioValue {
        kind: &'static str,
        id: u16,
        value: String,
        allowed: &'static str,
    },

    #[error("gpio {gpio} is assigned multiple times ({a} and {b})")]
    DuplicateGpio { gpio: u8, a: String, b: String },

    #[error("missing required mapping for {kind}{id}")]
    MissingRequired { kind: &'static str, id: u16 },

    #[error("invalid [safe_state] section: {details}")]
    InvalidSafeState { details: String },

    #[error("invalid [motion] section at {path}: {message}")]
    InvalidMotion { path: String, message: String },
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
            parse_map_section(di_table, "digital_inputs", "di", "di0", 0, 29, "0..=29 or \"virtual\"")?;
        let digital_outputs =
            parse_map_section(do_table, "digital_outputs", "do", "do0", 0, 29, "0..=29 or \"virtual\"")?;
        let analog_inputs = match ai_table {
            Some(t) => parse_map_section(
                t,
                "analog_inputs",
                "ai",
                "ai0",
                26,
                29,
                "26..=29 (RP2040 ADC-capable GPIO) or \"virtual\"",
            )?,
            None => BTreeMap::new(),
        };
        let analog_outputs = match ao_table {
            Some(t) => parse_map_section(t, "analog_outputs", "ao", "ao0", 0, 29, "0..=29 or \"virtual\"")?,
            None => BTreeMap::new(),
        };

        let motion = parse_motion_config(&v)?;
        let safe_state = parse_safe_state(&v)?;

        Ok(Self {
            digital_inputs,
            digital_outputs,
            analog_inputs,
            analog_outputs,
            motion,
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
        let mut insert_seen = |gpio: u8, key: String| -> Result<(), IoMapError> {
            // `255` is reserved for "virtual" channels (no physical GPIO binding).
            if gpio == u8::MAX {
                return Ok(());
            }
            if let Some(prev) = seen.insert(gpio, key.clone()) {
                return Err(IoMapError::DuplicateGpio {
                    gpio,
                    a: prev,
                    b: key,
                });
            }
            Ok(())
        };
        for (&id, &gpio) in &self.digital_inputs {
            insert_seen(gpio, format!("di{id}"))?;
        }
        for (&id, &gpio) in &self.digital_outputs {
            insert_seen(gpio, format!("do{id}"))?;
        }
        for (&id, &gpio) in &self.analog_outputs {
            insert_seen(gpio, format!("ao{id}"))?;
        }
        for (&id, &gpio) in &self.analog_inputs {
            insert_seen(gpio, format!("ai{id}"))?;
        }

        if let Some(motion) = &self.motion {
            for (&axis, cfg) in &motion.stepper {
                insert_seen(cfg.step_gpio, format!("motion.stepper.axis{axis}.step_gpio"))?;
                insert_seen(cfg.dir_gpio, format!("motion.stepper.axis{axis}.dir_gpio"))?;
                insert_seen(cfg.en_gpio, format!("motion.stepper.axis{axis}.en_gpio"))?;
            }
            for (&axis, cfg) in &motion.encoder {
                insert_seen(cfg.a_gpio, format!("motion.encoder.axis{axis}.a_gpio"))?;
                insert_seen(cfg.b_gpio, format!("motion.encoder.axis{axis}.b_gpio"))?;
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

fn parse_motion_config(v: &toml::Value) -> Result<Option<MotionConfig>, IoMapError> {
    let Some(motion) = v.get("motion") else {
        return Ok(None);
    };
    let Some(motion) = motion.as_table() else {
        return Err(IoMapError::InvalidMotion {
            path: "motion".to_string(),
            message: "must be a table".to_string(),
        });
    };

    let mut cfg = MotionConfig::default();

    if let Some(stepper) = motion.get("stepper") {
        let Some(stepper) = stepper.as_table() else {
            return Err(IoMapError::InvalidMotion {
                path: "motion.stepper".to_string(),
                message: "must be a table".to_string(),
            });
        };
        for (axis_key, axis_value) in stepper {
            let axis = parse_axis_key(axis_key, "motion.stepper")?;
            let Some(t) = axis_value.as_table() else {
                return Err(IoMapError::InvalidMotion {
                    path: format!("motion.stepper.{axis_key}"),
                    message: "must be a table".to_string(),
                });
            };
            let step_gpio = parse_required_gpio(t, "step_gpio", &format!("motion.stepper.{axis_key}"))?;
            let dir_gpio = parse_required_gpio(t, "dir_gpio", &format!("motion.stepper.{axis_key}"))?;
            let en_gpio = parse_required_gpio(t, "en_gpio", &format!("motion.stepper.{axis_key}"))?;
            let dir_inverted = t
                .get("dir_inverted")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let v_max_sps = parse_optional_u32(t, "v_max_sps", &format!("motion.stepper.{axis_key}"))?;
            let acc_sps2 = parse_optional_u32(t, "acc_sps2", &format!("motion.stepper.{axis_key}"))?;
            let dec_sps2 = parse_optional_u32(t, "dec_sps2", &format!("motion.stepper.{axis_key}"))?;

            // Semantic checks (keep errors path-addressable and actionable).
            if step_gpio == dir_gpio || step_gpio == en_gpio || dir_gpio == en_gpio {
                return Err(IoMapError::InvalidMotion {
                    path: format!("motion.stepper.{axis_key}"),
                    message: "step_gpio/dir_gpio/en_gpio must be distinct (hint: assign three different GPIO numbers)"
                        .to_string(),
                });
            }
            let profile_any = v_max_sps.is_some() || acc_sps2.is_some() || dec_sps2.is_some();
            let profile_all = v_max_sps.is_some() && acc_sps2.is_some() && dec_sps2.is_some();
            if profile_any && !profile_all {
                return Err(IoMapError::InvalidMotion {
                    path: format!("motion.stepper.{axis_key}"),
                    message: "if any of v_max_sps/acc_sps2/dec_sps2 is set, all three must be set (hint: set the missing fields or remove the partial profile)"
                        .to_string(),
                });
            }
            for (field, value) in [
                ("v_max_sps", v_max_sps),
                ("acc_sps2", acc_sps2),
                ("dec_sps2", dec_sps2),
            ] {
                if let Some(v) = value {
                    if v == 0 {
                        return Err(IoMapError::InvalidMotion {
                            path: format!("motion.stepper.{axis_key}.{field}"),
                            message: "must be > 0".to_string(),
                        });
                    }
                }
            }

            cfg.stepper.insert(
                axis,
                StepperAxisConfig {
                    step_gpio,
                    dir_gpio,
                    en_gpio,
                    dir_inverted,
                    v_max_sps,
                    acc_sps2,
                    dec_sps2,
                },
            );
        }
    }

    if let Some(encoder) = motion.get("encoder") {
        let Some(encoder) = encoder.as_table() else {
            return Err(IoMapError::InvalidMotion {
                path: "motion.encoder".to_string(),
                message: "must be a table".to_string(),
            });
        };
        for (axis_key, axis_value) in encoder {
            let axis = parse_axis_key(axis_key, "motion.encoder")?;
            let Some(t) = axis_value.as_table() else {
                return Err(IoMapError::InvalidMotion {
                    path: format!("motion.encoder.{axis_key}"),
                    message: "must be a table".to_string(),
                });
            };
            let a_gpio = parse_required_gpio(t, "a_gpio", &format!("motion.encoder.{axis_key}"))?;
            let b_gpio = parse_required_gpio(t, "b_gpio", &format!("motion.encoder.{axis_key}"))?;
            if a_gpio == b_gpio {
                return Err(IoMapError::InvalidMotion {
                    path: format!("motion.encoder.{axis_key}"),
                    message: "a_gpio and b_gpio must be distinct (hint: wire A and B to different GPIOs)"
                        .to_string(),
                });
            }
            let ppr = parse_required_u32(t, "ppr", &format!("motion.encoder.{axis_key}"))?;
            if ppr == 0 {
                return Err(IoMapError::InvalidMotion {
                    path: format!("motion.encoder.{axis_key}.ppr"),
                    message: "must be > 0".to_string(),
                });
            }
            let quad = t
                .get("quad")
                .and_then(|v| v.as_integer())
                .map(|v| v as i64)
                .unwrap_or(4);
            if quad != 1 && quad != 2 && quad != 4 {
                return Err(IoMapError::InvalidMotion {
                    path: format!("motion.encoder.{axis_key}.quad"),
                    message: "must be one of: 1, 2, 4".to_string(),
                });
            }

            let count_sign = match t.get("count_sign").and_then(|v| v.as_str()) {
                None => CountSignConvention::Normal,
                Some("normal") => CountSignConvention::Normal,
                Some("inverted") => CountSignConvention::Inverted,
                Some(other) => {
                    return Err(IoMapError::InvalidMotion {
                        path: format!("motion.encoder.{axis_key}.count_sign"),
                        message: format!("must be normal|inverted, got {other:?}"),
                    });
                }
            };
            let scale = t
                .get("scale")
                .and_then(|v| v.as_float())
                .unwrap_or(1.0);
            if !scale.is_finite() || scale <= 0.0 {
                return Err(IoMapError::InvalidMotion {
                    path: format!("motion.encoder.{axis_key}.scale"),
                    message: "must be a finite number > 0".to_string(),
                });
            }

            cfg.encoder.insert(
                axis,
                AbEncoderAxisConfig {
                    a_gpio,
                    b_gpio,
                    ppr,
                    quad: quad as u8,
                    count_sign,
                    scale: scale as f32,
                },
            );
        }
    }

    // If [motion] exists but is empty, treat it as a config error (avoids silent no-op).
    if cfg.stepper.is_empty() && cfg.encoder.is_empty() {
        return Err(IoMapError::InvalidMotion {
            path: "motion".to_string(),
            message: "must contain at least one of: [motion.stepper], [motion.encoder]".to_string(),
        });
    }

    Ok(Some(cfg))
}

fn parse_axis_key(key: &str, parent: &str) -> Result<u8, IoMapError> {
    let path = format!("{parent}.{key}");
    let axis = key
        .strip_prefix("axis")
        .ok_or_else(|| IoMapError::InvalidMotion {
            path: path.clone(),
            message: "axis key must be `axis0` or `axis1`".to_string(),
        })?;
    let axis: u8 = axis.parse().map_err(|_| IoMapError::InvalidMotion {
        path: path.clone(),
        message: "axis key must be `axis0` or `axis1`".to_string(),
    })?;
    if axis > 1 {
        return Err(IoMapError::InvalidMotion {
            path,
            message: "only axis0 and axis1 are supported in this PRD stage".to_string(),
        });
    }
    Ok(axis)
}

fn parse_required_u32(t: &toml::value::Table, field: &str, base: &str) -> Result<u32, IoMapError> {
    let path = format!("{base}.{field}");
    let v = t.get(field).ok_or_else(|| IoMapError::InvalidMotion {
        path: path.clone(),
        message: "missing required field".to_string(),
    })?;
    let Some(i) = v.as_integer() else {
        return Err(IoMapError::InvalidMotion {
            path,
            message: "must be an integer".to_string(),
        });
    };
    if i < 0 || i > u32::MAX as i64 {
        return Err(IoMapError::InvalidMotion {
            path,
            message: "out of range".to_string(),
        });
    }
    Ok(i as u32)
}

fn parse_optional_u32(
    t: &toml::value::Table,
    field: &str,
    base: &str,
) -> Result<Option<u32>, IoMapError> {
    let Some(v) = t.get(field) else {
        return Ok(None);
    };
    let path = format!("{base}.{field}");
    let Some(i) = v.as_integer() else {
        return Err(IoMapError::InvalidMotion {
            path,
            message: "must be an integer".to_string(),
        });
    };
    if i < 0 || i > u32::MAX as i64 {
        return Err(IoMapError::InvalidMotion {
            path,
            message: "out of range".to_string(),
        });
    }
    Ok(Some(i as u32))
}

fn parse_required_gpio(t: &toml::value::Table, field: &str, base: &str) -> Result<u8, IoMapError> {
    let path = format!("{base}.{field}");
    let v = t.get(field).ok_or_else(|| IoMapError::InvalidMotion {
        path: path.clone(),
        message: "missing required field".to_string(),
    })?;
    let Some(i) = v.as_integer() else {
        return Err(IoMapError::InvalidMotion {
            path,
            message: "must be an integer GPIO number".to_string(),
        });
    };
    if i < 0 || i > 29 {
        return Err(IoMapError::InvalidMotion {
            path,
            message: "gpio must be in 0..=29".to_string(),
        });
    }
    Ok(i as u8)
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
        let gpio = if let Some(i) = v.as_integer() {
            if !(min_gpio..=max_gpio).contains(&i) {
                return Err(IoMapError::InvalidGpio {
                    kind: prefix,
                    id,
                    gpio: i,
                    allowed,
                });
            }
            i as u8
        } else if let Some(s) = v.as_str() {
            if s.eq_ignore_ascii_case("virtual") {
                u8::MAX
            } else {
                return Err(IoMapError::InvalidGpioValue {
                    kind: prefix,
                    id,
                    value: s.to_string(),
                    allowed,
                });
            }
        } else {
            return Err(IoMapError::InvalidGpioValue {
                kind: prefix,
                id,
                value: format!("{v:?}"),
                allowed,
            });
        };
        out.insert(id, gpio);
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

    #[test]
    fn parses_motion_config_for_axis0_and_reports_path_errors() {
        let ok = r#"
[digital_inputs]
di0 = 0

[digital_outputs]
do0 = 1

[motion.stepper.axis0]
step_gpio = 2
dir_gpio = 3
en_gpio = 4
v_max_sps = 20000
acc_sps2 = 40000
dec_sps2 = 40000

[motion.encoder.axis0]
a_gpio = 8
b_gpio = 9
ppr = 1024
quad = 4
count_sign = "normal"
scale = 1.0
"#;
        let map = IoMap::from_toml_str(ok).expect("parse ok");
        let motion = map.motion.expect("motion parsed");
        assert!(motion.stepper.contains_key(&0));
        assert!(motion.encoder.contains_key(&0));

        let bad = r#"
[digital_inputs]
di0 = 0

[digital_outputs]
do0 = 1

[motion.stepper.axis0]
dir_gpio = 3
en_gpio = 4
"#;
        let err = IoMap::from_toml_str(bad).unwrap_err().to_string();
        assert!(
            err.contains("motion.stepper.axis0.step_gpio"),
            "expected path in error, got: {err}"
        );
    }

    #[test]
    fn motion_semantic_validation_reports_actionable_errors() {
        let dup_pins = r#"
[digital_inputs]
di0 = 0

[digital_outputs]
do0 = 1

[motion.stepper.axis0]
step_gpio = 2
dir_gpio = 2
en_gpio = 4
"#;
        let err = IoMap::from_toml_str(dup_pins).unwrap_err().to_string();
        assert!(
            err.contains("motion.stepper.axis0") && err.contains("distinct"),
            "expected distinct-pin error, got: {err}"
        );

        let partial_profile = r#"
[digital_inputs]
di0 = 0

[digital_outputs]
do0 = 1

[motion.stepper.axis0]
step_gpio = 2
dir_gpio = 3
en_gpio = 4
v_max_sps = 20000
"#;
        let err = IoMap::from_toml_str(partial_profile).unwrap_err().to_string();
        assert!(
            err.contains("motion.stepper.axis0") && err.contains("all three must be set"),
            "expected partial-profile error, got: {err}"
        );

        let ab_same_pin = r#"
[digital_inputs]
di0 = 0

[digital_outputs]
do0 = 1

[motion.encoder.axis0]
a_gpio = 8
b_gpio = 8
ppr = 1024
"#;
        let err = IoMap::from_toml_str(ab_same_pin).unwrap_err().to_string();
        assert!(
            err.contains("motion.encoder.axis0") && err.contains("distinct"),
            "expected AB distinct-pin error, got: {err}"
        );
    }
}
