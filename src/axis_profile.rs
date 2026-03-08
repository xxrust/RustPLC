use crate::ast::{DeviceDeclaration, DeviceType};
use crate::error::PlcError;
use crate::ir::{AxisDeviceType, AxisProfile};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

const AXIS_MODELS_DIR: &str = "axis_models";
const AXIS_CONFIGS_DIR: &str = "axis_configs";

#[derive(Debug, Clone, Deserialize)]
struct AxisModelDef {
    name: String,
    device_type: String,
    position_unit: String,
    max_speed: f64,
    max_acceleration: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct AxisConfigDef {
    name: String,
    position_unit: String,
    speed_limit: f64,
    acceleration_limit: f64,
}

pub fn resolve_axis_profiles(
    devices: &[DeviceDeclaration],
) -> Result<BTreeMap<String, AxisProfile>, Vec<PlcError>> {
    resolve_axis_profiles_with_dirs(
        devices,
        Path::new(AXIS_MODELS_DIR),
        Path::new(AXIS_CONFIGS_DIR),
    )
}

fn resolve_axis_profiles_with_dirs(
    devices: &[DeviceDeclaration],
    models_dir: &Path,
    configs_dir: &Path,
) -> Result<BTreeMap<String, AxisProfile>, Vec<PlcError>> {
    let axis_devices = devices
        .iter()
        .filter(|device| {
            matches!(
                device.device_type,
                DeviceType::StepperMotor | DeviceType::ServoDrive
            )
        })
        .collect::<Vec<_>>();

    if axis_devices.is_empty() {
        return Ok(BTreeMap::new());
    }

    let models = load_axis_models(models_dir)?;
    let configs = load_axis_configs(configs_dir)?;

    let mut errors = Vec::new();
    let mut profiles = BTreeMap::new();

    for device in axis_devices {
        let line = device.line.max(1);
        if !device.attributes.extra_params.is_empty() {
            let mut names = device
                .attributes
                .extra_params
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            names.sort();
            errors.push(PlcError::semantic_with_reason(
                line,
                format!(
                    "[AXP-006] axis device '{}' no longer accepts inline axis params: {}",
                    device.name,
                    names.join(", ")
                ),
                "请删除这些旧字段，并改为在设备上声明 model_ref/config_ref。".to_string(),
            ));
            continue;
        }

        let Some(model_ref) = device
            .attributes
            .model_ref
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        else {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!(
                    "[AXP-001] axis device '{}' is missing model_ref.",
                    device.name
                ),
                "请在轴设备上声明 model_ref: \"<axis_model_name>\"。".to_string(),
            ));
            continue;
        };

        let Some(config_ref) = device
            .attributes
            .config_ref
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        else {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!(
                    "[AXP-002] axis device '{}' is missing config_ref.",
                    device.name
                ),
                "请在轴设备上声明 config_ref: \"<axis_config_name>\"。".to_string(),
            ));
            continue;
        };

        let Some(model) = models.get(model_ref) else {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!(
                    "[AXP-003] axis model '{}' referenced by '{}' is not found.",
                    model_ref, device.name
                ),
                format!(
                    "请在 {AXIS_MODELS_DIR}/{}.toml 中定义该型号，或修正 model_ref。",
                    model_ref
                ),
            ));
            continue;
        };

        let Some(config) = configs.get(config_ref) else {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!(
                    "[AXP-004] axis config '{}' referenced by '{}' is not found.",
                    config_ref, device.name
                ),
                format!(
                    "请在 {AXIS_CONFIGS_DIR}/{}.toml 中定义该配置，或修正 config_ref。",
                    config_ref
                ),
            ));
            continue;
        };

        let axis_type = match device.device_type {
            DeviceType::StepperMotor => AxisDeviceType::StepperMotor,
            DeviceType::ServoDrive => AxisDeviceType::ServoDrive,
            _ => unreachable!("axis filter should only keep stepper/servo"),
        };

        let expected_type = axis_type_name(&axis_type);
        if model.device_type.trim() != expected_type {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!(
                    "[AXP-003] model '{}' type mismatch for '{}': expected {}, got {}.",
                    model_ref, device.name, expected_type, model.device_type
                ),
                "请为该设备选择同类型轴型号。".to_string(),
            ));
            continue;
        }

        if model.position_unit.trim() != config.position_unit.trim() {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!(
                    "[AXP-005] axis unit mismatch on '{}': model '{}' uses {}, config '{}' uses {}.",
                    device.name,
                    model_ref,
                    model.position_unit,
                    config_ref,
                    config.position_unit
                ),
                "请统一 model_ref/config_ref 的 position_unit。".to_string(),
            ));
            continue;
        }

        if !model.max_speed.is_finite()
            || !model.max_acceleration.is_finite()
            || model.max_speed <= 0.0
            || model.max_acceleration <= 0.0
        {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!(
                    "[AXP-003] axis model '{}' has invalid bounds for '{}'.",
                    model_ref, device.name
                ),
                "模型中的 max_speed/max_acceleration 必须为正数。".to_string(),
            ));
            continue;
        }

        if !config.speed_limit.is_finite()
            || !config.acceleration_limit.is_finite()
            || config.speed_limit <= 0.0
            || config.acceleration_limit <= 0.0
        {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!(
                    "[AXP-005] axis config '{}' has invalid limits for '{}'.",
                    config_ref, device.name
                ),
                "配置中的 speed_limit/acceleration_limit 必须为正数。".to_string(),
            ));
            continue;
        }

        if config.speed_limit > model.max_speed
            || config.acceleration_limit > model.max_acceleration
        {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!(
                    "[AXP-005] axis config '{}' exceeds model '{}' limits on '{}'.",
                    config_ref, model_ref, device.name
                ),
                format!(
                    "配置上限必须满足 speed_limit <= {} 且 acceleration_limit <= {}。",
                    model.max_speed, model.max_acceleration
                ),
            ));
            continue;
        }

        profiles.insert(
            device.name.clone(),
            AxisProfile {
                device_type: axis_type,
                position_unit: model.position_unit.clone(),
                max_speed: config.speed_limit as f32,
                max_acceleration: config.acceleration_limit as f32,
                model_ref: model_ref.to_string(),
                config_ref: config_ref.to_string(),
            },
        );
    }

    if errors.is_empty() {
        Ok(profiles)
    } else {
        Err(errors)
    }
}

fn axis_type_name(axis_type: &AxisDeviceType) -> &'static str {
    match axis_type {
        AxisDeviceType::StepperMotor => "stepper_motor",
        AxisDeviceType::ServoDrive => "servo_drive",
    }
}

fn load_axis_models(dir: &Path) -> Result<HashMap<String, AxisModelDef>, Vec<PlcError>> {
    load_axis_defs(dir, "AXP-100", |content, path| {
        toml::from_str::<AxisModelDef>(&content).map_err(|err| {
            PlcError::semantic_with_reason(
                1,
                format!(
                    "[AXP-100] failed to parse axis model file '{}'.",
                    path.display()
                ),
                err.to_string(),
            )
        })
    })
}

fn load_axis_configs(dir: &Path) -> Result<HashMap<String, AxisConfigDef>, Vec<PlcError>> {
    load_axis_defs(dir, "AXP-101", |content, path| {
        toml::from_str::<AxisConfigDef>(&content).map_err(|err| {
            PlcError::semantic_with_reason(
                1,
                format!(
                    "[AXP-101] failed to parse axis config file '{}'.",
                    path.display()
                ),
                err.to_string(),
            )
        })
    })
}

fn load_axis_defs<T: Clone>(
    dir: &Path,
    rule_id: &str,
    parse: impl Fn(String, &Path) -> Result<T, PlcError>,
) -> Result<HashMap<String, T>, Vec<PlcError>>
where
    T: NamedDef,
{
    let mut errors = Vec::new();
    let mut defs = HashMap::new();

    if !dir.exists() || !dir.is_dir() {
        return Err(vec![PlcError::semantic_with_reason(
            1,
            format!(
                "[{rule_id}] required directory '{}' is missing.",
                dir.display()
            ),
            "请在项目根目录创建该目录并提供对应 TOML 文件。".to_string(),
        )]);
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            return Err(vec![PlcError::semantic_with_reason(
                1,
                format!("[{rule_id}] failed to read '{}'.", dir.display()),
                err.to_string(),
            )]);
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(value) => value,
            Err(err) => {
                errors.push(PlcError::semantic_with_reason(
                    1,
                    format!(
                        "[{rule_id}] failed to read directory entry in '{}'.",
                        dir.display()
                    ),
                    err.to_string(),
                ));
                continue;
            }
        };

        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(err) => {
                errors.push(PlcError::semantic_with_reason(
                    1,
                    format!("[{rule_id}] failed to read '{}'.", path.display()),
                    err.to_string(),
                ));
                continue;
            }
        };

        let parsed = match parse(content, &path) {
            Ok(parsed) => parsed,
            Err(err) => {
                errors.push(err);
                continue;
            }
        };

        let def_name = parsed.name().to_string();
        if defs.insert(def_name.clone(), parsed).is_some() {
            errors.push(PlcError::semantic_with_reason(
                1,
                format!(
                    "[{rule_id}] duplicated definition name '{}' in '{}'.",
                    def_name,
                    dir.display()
                ),
                "请确保每个 name 在目录内唯一。".to_string(),
            ));
        }
    }

    if !errors.is_empty() {
        Err(errors)
    } else {
        Ok(defs)
    }
}

trait NamedDef {
    fn name(&self) -> &str;
}

impl NamedDef for AxisModelDef {
    fn name(&self) -> &str {
        &self.name
    }
}

impl NamedDef for AxisConfigDef {
    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_plc;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn parse_devices(topology_body: &str) -> Vec<DeviceDeclaration> {
        let source = format!(
            r#"
[topology]
{topology_body}

[constraints]

[tasks]
task main:
    step idle:
"#
        );

        let program = parse_plc(&source).expect("fixture should parse");
        program.topology.devices
    }

    fn mk_temp_axis_dirs(case: &str) -> (PathBuf, PathBuf, PathBuf) {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "rust_plc_axis_profile_{case}_{}_{}",
            std::process::id(),
            nonce
        ));
        let models = root.join("axis_models");
        let configs = root.join("axis_configs");
        fs::create_dir_all(&models).expect("create models dir");
        fs::create_dir_all(&configs).expect("create configs dir");
        (root, models, configs)
    }

    fn seed_default_axis_defs(models: &Path, configs: &Path) {
        fs::write(
            models.join("stepper_generic.toml"),
            r#"
name = "stepper_generic"
device_type = "stepper_motor"
position_unit = "pulse"
max_speed = 6000.0
max_acceleration = 20000.0
"#,
        )
        .expect("write model");
        fs::write(
            configs.join("stepper_default.toml"),
            r#"
name = "stepper_default"
position_unit = "pulse"
speed_limit = 3000.0
acceleration_limit = 10000.0
"#,
        )
        .expect("write config");
        fs::write(
            models.join("servo_generic.toml"),
            r#"
name = "servo_generic"
device_type = "servo_drive"
position_unit = "deg"
max_speed = 200.0
max_acceleration = 500.0
"#,
        )
        .expect("write servo model");
    }

    fn assert_error_contains(errors: &[PlcError], expected: &str) {
        let rendered = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains(expected),
            "expected `{expected}` in:\n{rendered}"
        );
    }

    #[test]
    fn resolves_axis_profile_from_model_and_config_refs() {
        let (root, models, configs) = mk_temp_axis_dirs("ok");
        seed_default_axis_defs(&models, &configs);

        let devices = parse_devices(
            "device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default }",
        );
        let profiles = resolve_axis_profiles_with_dirs(&devices, &models, &configs)
            .expect("profile should resolve");
        let profile = profiles.get("axis_x").expect("axis_x profile should exist");
        assert_eq!(profile.model_ref, "stepper_generic");
        assert_eq!(profile.config_ref, "stepper_default");
        assert_eq!(profile.max_speed, 3000.0);
        assert_eq!(profile.position_unit, "pulse");

        fs::remove_dir_all(root).expect("cleanup temp dir");
    }

    #[test]
    fn rejects_axis_device_missing_model_ref() {
        let (root, models, configs) = mk_temp_axis_dirs("missing_model");
        seed_default_axis_defs(&models, &configs);

        let devices = parse_devices("device axis_x: stepper_motor { config_ref: stepper_default }");
        let errors = resolve_axis_profiles_with_dirs(&devices, &models, &configs)
            .expect_err("missing model_ref should fail");
        assert_error_contains(&errors, "[AXP-001]");

        fs::remove_dir_all(root).expect("cleanup temp dir");
    }

    #[test]
    fn rejects_axis_device_missing_config_ref() {
        let (root, models, configs) = mk_temp_axis_dirs("missing_config");
        seed_default_axis_defs(&models, &configs);

        let devices = parse_devices("device axis_x: stepper_motor { model_ref: stepper_generic }");
        let errors = resolve_axis_profiles_with_dirs(&devices, &models, &configs)
            .expect_err("missing config_ref should fail");
        assert_error_contains(&errors, "[AXP-002]");

        fs::remove_dir_all(root).expect("cleanup temp dir");
    }

    #[test]
    fn rejects_unknown_model_or_config_refs() {
        let (root, models, configs) = mk_temp_axis_dirs("missing_files");
        seed_default_axis_defs(&models, &configs);

        let missing_model = parse_devices(
            "device axis_x: stepper_motor { model_ref: not_found_model, config_ref: stepper_default }",
        );
        let model_errors = resolve_axis_profiles_with_dirs(&missing_model, &models, &configs)
            .expect_err("unknown model should fail");
        assert_error_contains(&model_errors, "[AXP-003]");

        let missing_config = parse_devices(
            "device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: not_found_config }",
        );
        let config_errors = resolve_axis_profiles_with_dirs(&missing_config, &models, &configs)
            .expect_err("unknown config should fail");
        assert_error_contains(&config_errors, "[AXP-004]");

        fs::remove_dir_all(root).expect("cleanup temp dir");
    }

    #[test]
    fn rejects_model_type_mismatch_for_axis_device() {
        let (root, models, configs) = mk_temp_axis_dirs("type_mismatch");
        seed_default_axis_defs(&models, &configs);

        let devices = parse_devices(
            "device axis_x: stepper_motor { model_ref: servo_generic, config_ref: stepper_default }",
        );
        let errors = resolve_axis_profiles_with_dirs(&devices, &models, &configs)
            .expect_err("type mismatch should fail");
        assert_error_contains(&errors, "[AXP-003]");
        assert_error_contains(&errors, "type mismatch");

        fs::remove_dir_all(root).expect("cleanup temp dir");
    }

    #[test]
    fn rejects_legacy_inline_axis_params() {
        let (root, models, configs) = mk_temp_axis_dirs("legacy_inline");
        seed_default_axis_defs(&models, &configs);

        let devices = parse_devices(
            "device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default, max_speed: 1000 }",
        );
        let errors = resolve_axis_profiles_with_dirs(&devices, &models, &configs)
            .expect_err("legacy inline axis params should fail");
        assert_error_contains(&errors, "[AXP-006]");

        fs::remove_dir_all(root).expect("cleanup temp dir");
    }

    #[test]
    fn rejects_config_limits_exceeding_model_limits() {
        let (root, models, configs) = mk_temp_axis_dirs("limit_exceeds");
        fs::write(
            models.join("tiny_stepper.toml"),
            r#"
name = "tiny_stepper"
device_type = "stepper_motor"
position_unit = "pulse"
max_speed = 100.0
max_acceleration = 200.0
"#,
        )
        .expect("write tiny model");
        fs::write(
            configs.join("too_fast.toml"),
            r#"
name = "too_fast"
position_unit = "pulse"
speed_limit = 150.0
acceleration_limit = 250.0
"#,
        )
        .expect("write too fast config");

        let devices = parse_devices(
            "device axis_x: stepper_motor { model_ref: tiny_stepper, config_ref: too_fast }",
        );
        let errors = resolve_axis_profiles_with_dirs(&devices, &models, &configs)
            .expect_err("config exceeding model limits should fail");
        assert_error_contains(&errors, "[AXP-005]");
        assert_error_contains(&errors, "exceeds model");

        fs::remove_dir_all(root).expect("cleanup temp dir");
    }
}
