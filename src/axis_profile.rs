use crate::ast::{DeviceDeclaration, DeviceType};
use crate::error::PlcError;
use crate::ir::{AxisDeviceType, AxisProfile};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

const AXIS_MODELS_DIR: &str = "axis_models";
const AXIS_CONFIGS_DIR: &str = "axis_configs";
const AXIS_FAMILIES_DIR: &str = "axis_families";
const AXIS_MOTOR_CLASSES_DIR: &str = "axis_motor_classes";
const AXIS_MOTION_PARAM_SETS_DIR: &str = "axis_motion_param_sets";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AxisMotorClassDef {
    name: String,
    max_speed: f64,
    max_acceleration: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AxisFamilyDef {
    name: String,
    motor_class_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AxisModelDef {
    name: String,
    device_type: String,
    family_id: String,
    position_unit: String,
    max_speed: f64,
    max_acceleration: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AxisConfigDef {
    name: String,
    model_id: String,
    position_unit: String,
    speed_limit: f64,
    acceleration_limit: f64,
    #[serde(default)]
    soft_limit_min: Option<f64>,
    #[serde(default)]
    soft_limit_max: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AxisMotionParamSetDef {
    name: String,
    config_id: String,
    speed: f64,
    acceleration: f64,
    deceleration: f64,
}

pub fn resolve_axis_profiles(
    devices: &[DeviceDeclaration],
) -> Result<BTreeMap<String, AxisProfile>, Vec<PlcError>> {
    resolve_axis_profiles_with_dirs(
        devices,
        Path::new(AXIS_MOTOR_CLASSES_DIR),
        Path::new(AXIS_FAMILIES_DIR),
        Path::new(AXIS_MODELS_DIR),
        Path::new(AXIS_CONFIGS_DIR),
        Path::new(AXIS_MOTION_PARAM_SETS_DIR),
    )
}

fn resolve_axis_profiles_with_dirs(
    devices: &[DeviceDeclaration],
    motor_classes_dir: &Path,
    families_dir: &Path,
    models_dir: &Path,
    configs_dir: &Path,
    motion_param_sets_dir: &Path,
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

    let motor_classes = load_axis_motor_classes(motor_classes_dir)?;
    let families = load_axis_families(families_dir)?;
    let models = load_axis_models(models_dir)?;
    let configs = load_axis_configs(configs_dir)?;
    let motion_param_sets = load_axis_motion_param_sets(motion_param_sets_dir)?;

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

        let Some(family) = families.get(model.family_id.trim()) else {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!(
                    "[AXP-007] axis model '{}' references unknown family_id '{}' for '{}'.",
                    model_ref, model.family_id, device.name
                ),
                format!(
                    "请在 {AXIS_FAMILIES_DIR}/{}.toml 中定义该 family，或修正 model.family_id。",
                    model.family_id
                ),
            ));
            continue;
        };

        let Some(motor_class) = motor_classes.get(family.motor_class_id.trim()) else {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!(
                    "[AXP-008] axis family '{}' references unknown motor_class_id '{}' for '{}'.",
                    family.name, family.motor_class_id, device.name
                ),
                format!(
                    "请在 {AXIS_MOTOR_CLASSES_DIR}/{}.toml 中定义该 motor_class，或修正 family.motor_class_id。",
                    family.motor_class_id
                ),
            ));
            continue;
        };

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

        if config.model_id.trim() != model_ref {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!(
                    "[AXP-009] axis config '{}' is bound to model '{}' but '{}' uses model_ref '{}'.",
                    config_ref, config.model_id, device.name, model_ref
                ),
                "请确保 config.model_id 与设备上的 model_ref 完全一致。".to_string(),
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

        if !motor_class.max_speed.is_finite()
            || !motor_class.max_acceleration.is_finite()
            || motor_class.max_speed <= 0.0
            || motor_class.max_acceleration <= 0.0
        {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!(
                    "[AXP-008] axis motor_class '{}' has invalid bounds for '{}'.",
                    motor_class.name, device.name
                ),
                "motor_class 的 max_speed/max_acceleration 必须为正数。".to_string(),
            ));
            continue;
        }

        if model.max_speed > motor_class.max_speed
            || model.max_acceleration > motor_class.max_acceleration
        {
            errors.push(PlcError::semantic_with_reason(
                line,
                format!(
                    "[AXP-008] axis model '{}' exceeds motor_class '{}' bounds on '{}'.",
                    model_ref, motor_class.name, device.name
                ),
                format!(
                    "请满足 model.max_speed <= {} 且 model.max_acceleration <= {}。",
                    motor_class.max_speed, motor_class.max_acceleration
                ),
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

        let soft_limits = match (config.soft_limit_min, config.soft_limit_max) {
            (Some(min), Some(max)) => {
                if !min.is_finite() || !max.is_finite() {
                    errors.push(PlcError::semantic_with_reason(
                        line,
                        format!(
                            "[AXP-011] axis config '{}' has non-finite soft limits for '{}'.",
                            config_ref, device.name
                        ),
                        "请确保 soft_limit_min/soft_limit_max 均为有限数值。".to_string(),
                    ));
                    continue;
                }
                if min > max {
                    errors.push(PlcError::semantic_with_reason(
                        line,
                        format!(
                            "[AXP-011] axis config '{}' has invalid soft limit range {}..{} for '{}'.",
                            config_ref, min, max, device.name
                        ),
                        "请确保 soft_limit_min <= soft_limit_max。".to_string(),
                    ));
                    continue;
                }
                Some((min as f32, max as f32))
            }
            (None, None) => None,
            _ => {
                errors.push(PlcError::semantic_with_reason(
                    line,
                    format!(
                        "[AXP-011] axis config '{}' must declare both soft_limit_min and soft_limit_max for '{}'.",
                        config_ref, device.name
                    ),
                    "请同时声明 soft_limit_min 和 soft_limit_max，或同时省略。".to_string(),
                ));
                continue;
            }
        };

        let motion_param_set = device
            .attributes
            .motion_param_set
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());

        if let Some(set_name) = motion_param_set {
            let Some(param_set) = motion_param_sets.get(set_name) else {
                errors.push(PlcError::semantic_with_reason(
                    line,
                    format!(
                        "[AXP-010] axis device '{}' references unknown motion_param_set '{}'.",
                        device.name, set_name
                    ),
                    format!(
                        "请在 {AXIS_MOTION_PARAM_SETS_DIR}/{}.toml 中定义该参数集，或修正 motion_param_set。",
                        set_name
                    ),
                ));
                continue;
            };

            if param_set.config_id.trim() != config_ref {
                errors.push(PlcError::semantic_with_reason(
                    line,
                    format!(
                        "[AXP-010] motion_param_set '{}' is bound to config '{}' but '{}' uses config_ref '{}'.",
                        set_name, param_set.config_id, device.name, config_ref
                    ),
                    "请确保 motion_param_set.config_id 与设备上的 config_ref 一致。".to_string(),
                ));
                continue;
            }

            if !param_set.speed.is_finite()
                || !param_set.acceleration.is_finite()
                || !param_set.deceleration.is_finite()
                || param_set.speed <= 0.0
                || param_set.acceleration <= 0.0
                || param_set.deceleration <= 0.0
            {
                errors.push(PlcError::semantic_with_reason(
                    line,
                    format!(
                        "[AXP-010] motion_param_set '{}' has invalid values for '{}'.",
                        set_name, device.name
                    ),
                    "speed/acceleration/deceleration 必须为正数。".to_string(),
                ));
                continue;
            }

            if param_set.speed > config.speed_limit
                || param_set.acceleration > config.acceleration_limit
                || param_set.deceleration > config.acceleration_limit
            {
                errors.push(PlcError::semantic_with_reason(
                    line,
                    format!(
                        "[AXP-010] motion_param_set '{}' exceeds config '{}' limits for '{}'.",
                        set_name, config_ref, device.name
                    ),
                    format!(
                        "请满足 speed <= {} 且 acceleration/deceleration <= {}。",
                        config.speed_limit, config.acceleration_limit
                    ),
                ));
                continue;
            }
        }

        profiles.insert(
            device.name.clone(),
            AxisProfile {
                device_type: axis_type,
                motor_class_id: motor_class.name.clone(),
                family_id: family.name.clone(),
                position_unit: model.position_unit.clone(),
                max_speed: config.speed_limit as f32,
                max_acceleration: config.acceleration_limit as f32,
                soft_limit_min: soft_limits.map(|(min, _)| min),
                soft_limit_max: soft_limits.map(|(_, max)| max),
                model_ref: model_ref.to_string(),
                config_ref: config_ref.to_string(),
                motion_param_set: motion_param_set.map(str::to_string),
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

fn load_axis_motor_classes(
    dir: &Path,
) -> Result<HashMap<String, AxisMotorClassDef>, Vec<PlcError>> {
    load_axis_defs(dir, "AXP-102", |content, path| {
        toml::from_str::<AxisMotorClassDef>(&content).map_err(|err| {
            PlcError::semantic_with_reason(
                1,
                format!(
                    "[AXP-102] failed to parse axis motor_class file '{}'.",
                    path.display()
                ),
                err.to_string(),
            )
        })
    })
}

fn load_axis_families(dir: &Path) -> Result<HashMap<String, AxisFamilyDef>, Vec<PlcError>> {
    load_axis_defs(dir, "AXP-103", |content, path| {
        toml::from_str::<AxisFamilyDef>(&content).map_err(|err| {
            PlcError::semantic_with_reason(
                1,
                format!(
                    "[AXP-103] failed to parse axis family file '{}'.",
                    path.display()
                ),
                err.to_string(),
            )
        })
    })
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

fn load_axis_motion_param_sets(
    dir: &Path,
) -> Result<HashMap<String, AxisMotionParamSetDef>, Vec<PlcError>> {
    load_axis_defs(dir, "AXP-104", |content, path| {
        toml::from_str::<AxisMotionParamSetDef>(&content).map_err(|err| {
            PlcError::semantic_with_reason(
                1,
                format!(
                    "[AXP-104] failed to parse motion_param_set file '{}'.",
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

impl NamedDef for AxisMotorClassDef {
    fn name(&self) -> &str {
        &self.name
    }
}

impl NamedDef for AxisFamilyDef {
    fn name(&self) -> &str {
        &self.name
    }
}

impl NamedDef for AxisConfigDef {
    fn name(&self) -> &str {
        &self.name
    }
}

impl NamedDef for AxisMotionParamSetDef {
    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_plc;
    use std::collections::BTreeMap;
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

    struct AxisFixtureDirs {
        root: PathBuf,
        motor_classes: PathBuf,
        families: PathBuf,
        models: PathBuf,
        configs: PathBuf,
        motion_param_sets: PathBuf,
    }

    fn mk_temp_axis_dirs(case: &str) -> AxisFixtureDirs {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "rust_plc_axis_profile_{case}_{}_{}",
            std::process::id(),
            nonce
        ));
        let motor_classes = root.join("axis_motor_classes");
        let families = root.join("axis_families");
        let models = root.join("axis_models");
        let configs = root.join("axis_configs");
        let motion_param_sets = root.join("axis_motion_param_sets");
        fs::create_dir_all(&motor_classes).expect("create motor classes dir");
        fs::create_dir_all(&families).expect("create families dir");
        fs::create_dir_all(&models).expect("create models dir");
        fs::create_dir_all(&configs).expect("create configs dir");
        fs::create_dir_all(&motion_param_sets).expect("create motion_param_sets dir");

        AxisFixtureDirs {
            root,
            motor_classes,
            families,
            models,
            configs,
            motion_param_sets,
        }
    }

    fn seed_default_axis_defs(dirs: &AxisFixtureDirs) {
        fs::write(
            dirs.motor_classes.join("stepper_basic.toml"),
            r#"
name = "stepper_basic"
max_speed = 7000.0
max_acceleration = 22000.0
"#,
        )
        .expect("write motor class");
        fs::write(
            dirs.families.join("nema_family.toml"),
            r#"
name = "nema_family"
motor_class_id = "stepper_basic"
"#,
        )
        .expect("write family");
        fs::write(
            dirs.models.join("stepper_generic.toml"),
            r#"
name = "stepper_generic"
device_type = "stepper_motor"
family_id = "nema_family"
position_unit = "pulse"
max_speed = 6000.0
max_acceleration = 20000.0
"#,
        )
        .expect("write model");
        fs::write(
            dirs.configs.join("stepper_default.toml"),
            r#"
name = "stepper_default"
model_id = "stepper_generic"
position_unit = "pulse"
speed_limit = 3000.0
acceleration_limit = 10000.0
"#,
        )
        .expect("write config");
        fs::write(
            dirs.models.join("servo_generic.toml"),
            r#"
name = "servo_generic"
device_type = "servo_drive"
family_id = "nema_family"
position_unit = "deg"
max_speed = 200.0
max_acceleration = 500.0
"#,
        )
        .expect("write servo model");
        fs::write(
            dirs.motion_param_sets.join("stepper_pick.toml"),
            r#"
name = "stepper_pick"
config_id = "stepper_default"
speed = 1200.0
acceleration = 2000.0
deceleration = 1800.0
"#,
        )
        .expect("write motion param set");
    }

    fn resolve_with_fixture(
        devices: &[DeviceDeclaration],
        dirs: &AxisFixtureDirs,
    ) -> Result<BTreeMap<String, AxisProfile>, Vec<PlcError>> {
        resolve_axis_profiles_with_dirs(
            devices,
            &dirs.motor_classes,
            &dirs.families,
            &dirs.models,
            &dirs.configs,
            &dirs.motion_param_sets,
        )
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
        let dirs = mk_temp_axis_dirs("ok");
        seed_default_axis_defs(&dirs);

        let devices = parse_devices(
            "device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default }",
        );
        let profiles = resolve_with_fixture(&devices, &dirs).expect("profile should resolve");
        let profile = profiles.get("axis_x").expect("axis_x profile should exist");
        assert_eq!(profile.model_ref, "stepper_generic");
        assert_eq!(profile.config_ref, "stepper_default");
        assert_eq!(profile.family_id, "nema_family");
        assert_eq!(profile.motor_class_id, "stepper_basic");
        assert_eq!(profile.max_speed, 3000.0);
        assert_eq!(profile.position_unit, "pulse");

        fs::remove_dir_all(dirs.root).expect("cleanup temp dir");
    }

    #[test]
    fn rejects_axis_device_missing_model_ref() {
        let dirs = mk_temp_axis_dirs("missing_model");
        seed_default_axis_defs(&dirs);

        let devices = parse_devices("device axis_x: stepper_motor { config_ref: stepper_default }");
        let errors =
            resolve_with_fixture(&devices, &dirs).expect_err("missing model_ref should fail");
        assert_error_contains(&errors, "[AXP-001]");

        fs::remove_dir_all(dirs.root).expect("cleanup temp dir");
    }

    #[test]
    fn rejects_axis_device_missing_config_ref() {
        let dirs = mk_temp_axis_dirs("missing_config");
        seed_default_axis_defs(&dirs);

        let devices = parse_devices("device axis_x: stepper_motor { model_ref: stepper_generic }");
        let errors =
            resolve_with_fixture(&devices, &dirs).expect_err("missing config_ref should fail");
        assert_error_contains(&errors, "[AXP-002]");

        fs::remove_dir_all(dirs.root).expect("cleanup temp dir");
    }

    #[test]
    fn rejects_unknown_model_or_config_refs() {
        let dirs = mk_temp_axis_dirs("missing_files");
        seed_default_axis_defs(&dirs);

        let missing_model = parse_devices(
            "device axis_x: stepper_motor { model_ref: not_found_model, config_ref: stepper_default }",
        );
        let model_errors =
            resolve_with_fixture(&missing_model, &dirs).expect_err("unknown model should fail");
        assert_error_contains(&model_errors, "[AXP-003]");

        let missing_config = parse_devices(
            "device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: not_found_config }",
        );
        let config_errors =
            resolve_with_fixture(&missing_config, &dirs).expect_err("unknown config should fail");
        assert_error_contains(&config_errors, "[AXP-004]");

        fs::remove_dir_all(dirs.root).expect("cleanup temp dir");
    }

    #[test]
    fn rejects_model_type_mismatch_for_axis_device() {
        let dirs = mk_temp_axis_dirs("type_mismatch");
        seed_default_axis_defs(&dirs);

        let devices = parse_devices(
            "device axis_x: stepper_motor { model_ref: servo_generic, config_ref: stepper_default }",
        );
        let errors = resolve_with_fixture(&devices, &dirs).expect_err("type mismatch should fail");
        assert_error_contains(&errors, "[AXP-003]");
        assert_error_contains(&errors, "type mismatch");

        fs::remove_dir_all(dirs.root).expect("cleanup temp dir");
    }

    #[test]
    fn rejects_legacy_inline_axis_params() {
        let dirs = mk_temp_axis_dirs("legacy_inline");
        seed_default_axis_defs(&dirs);

        let devices = parse_devices(
            "device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default, max_speed: 1000 }",
        );
        let errors = resolve_with_fixture(&devices, &dirs)
            .expect_err("legacy inline axis params should fail");
        assert_error_contains(&errors, "[AXP-006]");

        fs::remove_dir_all(dirs.root).expect("cleanup temp dir");
    }

    #[test]
    fn rejects_config_limits_exceeding_model_limits() {
        let dirs = mk_temp_axis_dirs("limit_exceeds");
        seed_default_axis_defs(&dirs);
        fs::write(
            dirs.models.join("tiny_stepper.toml"),
            r#"
name = "tiny_stepper"
device_type = "stepper_motor"
family_id = "nema_family"
position_unit = "pulse"
max_speed = 100.0
max_acceleration = 200.0
"#,
        )
        .expect("write tiny model");
        fs::write(
            dirs.configs.join("too_fast.toml"),
            r#"
name = "too_fast"
model_id = "tiny_stepper"
position_unit = "pulse"
speed_limit = 150.0
acceleration_limit = 250.0
"#,
        )
        .expect("write too fast config");

        let devices = parse_devices(
            "device axis_x: stepper_motor { model_ref: tiny_stepper, config_ref: too_fast }",
        );
        let errors = resolve_with_fixture(&devices, &dirs)
            .expect_err("config exceeding model limits should fail");
        assert_error_contains(&errors, "[AXP-005]");
        assert_error_contains(&errors, "exceeds model");

        fs::remove_dir_all(dirs.root).expect("cleanup temp dir");
    }

    #[test]
    fn rejects_config_bound_to_different_model() {
        let dirs = mk_temp_axis_dirs("model_binding_mismatch");
        seed_default_axis_defs(&dirs);
        fs::write(
            dirs.configs.join("wrong_bind.toml"),
            r#"
name = "wrong_bind"
model_id = "servo_generic"
position_unit = "pulse"
speed_limit = 500.0
acceleration_limit = 1000.0
"#,
        )
        .expect("write mismatched config");

        let devices = parse_devices(
            "device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: wrong_bind }",
        );
        let errors =
            resolve_with_fixture(&devices, &dirs).expect_err("config->model mismatch should fail");
        assert_error_contains(&errors, "[AXP-009]");

        fs::remove_dir_all(dirs.root).expect("cleanup temp dir");
    }

    #[test]
    fn rejects_model_with_unknown_family_id() {
        let dirs = mk_temp_axis_dirs("unknown_family");
        seed_default_axis_defs(&dirs);
        fs::write(
            dirs.models.join("bad_family.toml"),
            r#"
name = "bad_family"
device_type = "stepper_motor"
family_id = "missing_family"
position_unit = "pulse"
max_speed = 1000.0
max_acceleration = 1000.0
"#,
        )
        .expect("write unknown family model");
        fs::write(
            dirs.configs.join("bad_family_config.toml"),
            r#"
name = "bad_family_config"
model_id = "bad_family"
position_unit = "pulse"
speed_limit = 500.0
acceleration_limit = 800.0
"#,
        )
        .expect("write bad family config");

        let devices = parse_devices(
            "device axis_x: stepper_motor { model_ref: bad_family, config_ref: bad_family_config }",
        );
        let errors = resolve_with_fixture(&devices, &dirs).expect_err("missing family should fail");
        assert_error_contains(&errors, "[AXP-007]");

        fs::remove_dir_all(dirs.root).expect("cleanup temp dir");
    }

    #[test]
    fn rejects_model_exceeding_motor_class_bounds() {
        let dirs = mk_temp_axis_dirs("class_bounds");
        seed_default_axis_defs(&dirs);
        fs::write(
            dirs.models.join("too_big.toml"),
            r#"
name = "too_big"
device_type = "stepper_motor"
family_id = "nema_family"
position_unit = "pulse"
max_speed = 9000.0
max_acceleration = 30000.0
"#,
        )
        .expect("write too big model");
        fs::write(
            dirs.configs.join("too_big_config.toml"),
            r#"
name = "too_big_config"
model_id = "too_big"
position_unit = "pulse"
speed_limit = 8000.0
acceleration_limit = 20000.0
"#,
        )
        .expect("write too big config");

        let devices = parse_devices(
            "device axis_x: stepper_motor { model_ref: too_big, config_ref: too_big_config }",
        );
        let errors = resolve_with_fixture(&devices, &dirs)
            .expect_err("model exceeding class bounds should fail");
        assert_error_contains(&errors, "[AXP-008]");
        assert_error_contains(&errors, "exceeds motor_class");

        fs::remove_dir_all(dirs.root).expect("cleanup temp dir");
    }

    #[test]
    fn rejects_motion_param_set_exceeding_config_limits() {
        let dirs = mk_temp_axis_dirs("param_set_limits");
        seed_default_axis_defs(&dirs);
        fs::write(
            dirs.motion_param_sets.join("too_fast_set.toml"),
            r#"
name = "too_fast_set"
config_id = "stepper_default"
speed = 3200.0
acceleration = 12000.0
deceleration = 12000.0
"#,
        )
        .expect("write too fast motion_param_set");

        let devices = parse_devices(
            "device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: stepper_default, motion_param_set: too_fast_set }",
        );
        let errors = resolve_with_fixture(&devices, &dirs)
            .expect_err("out-of-range motion_param_set should fail");
        assert_error_contains(&errors, "[AXP-010]");
        assert_error_contains(&errors, "exceeds config");

        fs::remove_dir_all(dirs.root).expect("cleanup temp dir");
    }

    #[test]
    fn resolves_soft_limits_from_axis_config() {
        let dirs = mk_temp_axis_dirs("soft_limits_ok");
        seed_default_axis_defs(&dirs);
        fs::write(
            dirs.configs.join("soft_limited.toml"),
            r#"
name = "soft_limited"
model_id = "stepper_generic"
position_unit = "pulse"
speed_limit = 3000.0
acceleration_limit = 10000.0
soft_limit_min = -1000.0
soft_limit_max = 2000.0
"#,
        )
        .expect("write soft-limited config");

        let devices = parse_devices(
            "device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: soft_limited }",
        );
        let profiles = resolve_with_fixture(&devices, &dirs).expect("soft limits should resolve");
        let profile = profiles.get("axis_x").expect("axis profile should exist");
        assert_eq!(profile.soft_limit_min, Some(-1000.0));
        assert_eq!(profile.soft_limit_max, Some(2000.0));

        fs::remove_dir_all(dirs.root).expect("cleanup temp dir");
    }

    #[test]
    fn rejects_axis_config_with_partial_soft_limits() {
        let dirs = mk_temp_axis_dirs("soft_limits_partial");
        seed_default_axis_defs(&dirs);
        fs::write(
            dirs.configs.join("soft_limited_partial.toml"),
            r#"
name = "soft_limited_partial"
model_id = "stepper_generic"
position_unit = "pulse"
speed_limit = 3000.0
acceleration_limit = 10000.0
soft_limit_min = 0.0
"#,
        )
        .expect("write partial soft-limited config");

        let devices = parse_devices(
            "device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: soft_limited_partial }",
        );
        let errors =
            resolve_with_fixture(&devices, &dirs).expect_err("partial soft limits should fail");
        assert_error_contains(&errors, "[AXP-011]");

        fs::remove_dir_all(dirs.root).expect("cleanup temp dir");
    }

    #[test]
    fn rejects_axis_config_with_inverted_soft_limit_range() {
        let dirs = mk_temp_axis_dirs("soft_limits_inverted");
        seed_default_axis_defs(&dirs);
        fs::write(
            dirs.configs.join("soft_limited_inverted.toml"),
            r#"
name = "soft_limited_inverted"
model_id = "stepper_generic"
position_unit = "pulse"
speed_limit = 3000.0
acceleration_limit = 10000.0
soft_limit_min = 100.0
soft_limit_max = -100.0
"#,
        )
        .expect("write inverted soft-limited config");

        let devices = parse_devices(
            "device axis_x: stepper_motor { model_ref: stepper_generic, config_ref: soft_limited_inverted }",
        );
        let errors =
            resolve_with_fixture(&devices, &dirs).expect_err("inverted soft limits should fail");
        assert_error_contains(&errors, "[AXP-011]");

        fs::remove_dir_all(dirs.root).expect("cleanup temp dir");
    }
}
