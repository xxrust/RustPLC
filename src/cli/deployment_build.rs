#[derive(Debug, Serialize)]
struct BuildMeta<'a> {
    plc_sha256: &'a str,
    generated_at: &'a str,
    tool_version: &'a str,
    runtime_semver: &'a str,
    git_commit: &'a str,
    git_dirty: bool,
    runtime_budget: RuntimeBudgetSummary,
    realtime_profile: RealtimeProfile,
    #[serde(skip_serializing_if = "Option::is_none")]
    io_map: Option<IoMap>,
}

#[derive(Debug, Clone, Serialize)]
struct RealtimeProfile {
    tick_ms: u64,
    thresholds: RealtimeThresholdConfig,
    overrun_count: u64,
    p99_exec_us: u64,
}

#[derive(Debug, Clone, Serialize)]
struct RealtimeThresholdConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    max_p99_exec_us: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_overrun_count: Option<u64>,
}


#[derive(Debug, Clone)]
struct GitMetadata {
    commit: String,
    dirty: bool,
}

#[derive(Debug, Clone, Serialize)]
struct AnalogContract {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    analog_inputs: BTreeMap<String, AnalogInputContractEntry>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    analog_outputs: BTreeMap<String, AnalogOutputContractEntry>,
}

#[derive(Debug, Clone, Serialize)]
struct AnalogInputContractEntry {
    min: f32,
    max: f32,
    scale: f32,
    offset: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    unit: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct AnalogOutputContractEntry {
    min: f32,
    max: f32,
    ramp_ms: u64,
    scale: f32,
    offset: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    unit: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct AnalogCalibrationFile {
    #[serde(default)]
    analog_inputs: BTreeMap<String, AnalogCalibrationEntry>,
    #[serde(default)]
    analog_outputs: BTreeMap<String, AnalogCalibrationEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct AnalogCalibrationEntry {
    #[serde(default = "default_calibration_scale")]
    scale: f32,
    #[serde(default)]
    offset: f32,
}

fn default_calibration_scale() -> f32 {
    1.0
}

fn run_build_rp2040_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "build-rp2040");
    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut out_dir: Option<PathBuf> = None;
    let mut io_map_path: Option<PathBuf> = None;
    let mut analog_calibration_path: Option<PathBuf> = None;
    let mut emit_uf2: Option<PathBuf> = None;
    let mut output_mode = CliOutputMode::Human;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => {
                out_dir =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --out <dir>".to_string()
                    })?));
            }
            "--io-map" => {
                io_map_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --io-map <file>".to_string()
                    })?));
            }
            "--analog-calibration" => {
                analog_calibration_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --analog-calibration <file>".to_string()
                    })?));
            }
            "--emit-uf2" => {
                emit_uf2 =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --emit-uf2 <file.uf2>".to_string()
                    })?));
            }
            "--output" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --output <human|json>".to_string())?;
                output_mode = CliOutputMode::parse(&raw).ok_or_else(|| {
                    format!("Invalid --output value `{raw}` (expected `human` or `json`)")
                })?;
            }
            "-h" | "--help" => {
                return Err(usage.clone());
            }
            other => return Err(format!("Unknown argument for build-rp2040: {other}")),
        }
    }

    let out_dir = out_dir.ok_or_else(|| usage.clone())?;
    fs::create_dir_all(&out_dir)
        .map_err(|err| format!("Failed to create out dir {out_dir:?}: {err}"))?;

    if !is_supported_plc_source_path(Path::new(&plc_path)) {
        return Err(format!(
            "Expected a .plc or .bundle.toml path, got: {plc_path}"
        ));
    }

    let loaded = load_plc_source(Path::new(&plc_path))?;
    let plc_source = loaded.source.clone();
    let plc_bytes = plc_source.as_bytes().to_vec();

    let sha256 = {
        let mut h = Sha256::new();
        h.update(&plc_bytes);
        hex::encode(h.finalize())
    };

    let ir_bundle = compile_pipeline(&loaded).map_err(|errors| errors.join("\n\n"))?;

    // For build artifacts we use 1ms ticks so ms-based DSL durations are always aligned.
    let runtime_program = state_machine_to_runtime_program(
        &ir_bundle.topology,
        &ir_bundle.constraints,
        &ir_bundle.state_machine,
        1,
    )
    .map_err(|err| format!("Failed to bridge to runtime Program: {err}"))?;

    let usage = io_usage_for_program(&runtime_program);
    let io_map = match io_map_path.as_ref() {
        None => None,
        Some(path) => {
            let toml_str = fs::read_to_string(&path)
                .map_err(|err| format!("Failed to read io map {path:?}: {err}"))?;
            let m = IoMap::from_toml_str(&toml_str)
                .map_err(|err| format!("Failed to parse io map TOML: {err}"))?;
            match m.validate_for_usage(usage) {
                Ok(()) => {}
                Err(IoMapError::MissingRequired { kind, id }) => {
                    return Err(format!(
                        "Invalid io map for this program: missing required mapping for {kind}{id}\n\
\n\
hint: the io map must contain a GPIO assignment for every DI/DO/AI/AO used by the program.\n\
Start from the generated `io_map.template.toml` under `--out <dir>` and fill in GPIO numbers."
                    ));
                }
                Err(err) => {
                    return Err(format!("Invalid io map for this program: {err}"));
                }
            }
            Some(m)
        }
    };

    let generated_src = codegen::generate_program_module(&runtime_program, "generated")
        .map_err(|err| format!("Codegen failed: {err:?}"))?;

    let mut generated_src = generated_src;
    if !generated_src.ends_with('\n') {
        generated_src.push('\n');
    }

    let generated_path = out_dir.join("generated_program.rs");
    fs::write(&generated_path, generated_src)
        .map_err(|err| format!("Failed to write {generated_path:?}: {err}"))?;

    let iomap_path = out_dir.join("io_map.template.toml");
    let iomap = io_map_template_for_program(&runtime_program);
    fs::write(&iomap_path, iomap)
        .map_err(|err| format!("Failed to write {iomap_path:?}: {err}"))?;

    let mut analog_contract = build_analog_contract(&loaded)?;
    if let Some(path) = analog_calibration_path.as_ref() {
        let calibration_toml = fs::read_to_string(path)
            .map_err(|err| format!("Failed to read analog calibration file {path:?}: {err}"))?;
        apply_analog_calibration(&mut analog_contract, &calibration_toml)?;
    }
    let analog_contract_toml = toml::to_string_pretty(&analog_contract)
        .map_err(|err| format!("Failed to serialize analog contract TOML: {err}"))?;
    let analog_contract_path = out_dir.join("analog_contract.toml");
    fs::write(&analog_contract_path, analog_contract_toml)
        .map_err(|err| format!("Failed to write {analog_contract_path:?}: {err}"))?;

    let analog_cal_template_path = out_dir.join("analog_calibration.template.toml");
    let analog_cal_template = analog_calibration_template_for_contract(&analog_contract);
    fs::write(&analog_cal_template_path, analog_cal_template)
        .map_err(|err| format!("Failed to write {analog_cal_template_path:?}: {err}"))?;

    let generated_at = time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string());
    let git_metadata = detect_git_metadata();

    let meta = BuildMeta {
        plc_sha256: &sha256,
        generated_at: &generated_at,
        tool_version: env!("CARGO_PKG_VERSION"),
        runtime_semver: runtime_core::VERSION,
        git_commit: &git_metadata.commit,
        git_dirty: git_metadata.dirty,
        runtime_budget: ir_bundle.runtime_budget.summary(),
        realtime_profile: RealtimeProfile {
            tick_ms: 1,
            thresholds: RealtimeThresholdConfig {
                max_p99_exec_us: None,
                max_overrun_count: None,
            },
            overrun_count: 0,
            p99_exec_us: 0,
        },
        io_map,
    };
    let mut meta_json = serde_json::to_string_pretty(&meta)
        .map_err(|err| format!("Failed to serialize build_meta.json: {err}"))?;
    meta_json.push('\n');
    let meta_path = out_dir.join("build_meta.json");
    fs::write(&meta_path, meta_json)
        .map_err(|err| format!("Failed to write {meta_path:?}: {err}"))?;

    if let Some(uf2_path) = emit_uf2 {
        let io_map_path = io_map_path.as_ref().ok_or_else(|| {
            "--emit-uf2 requires --io-map <file> so board pin mapping is explicit".to_string()
        })?;
        emit_rp2040_uf2(
            &generated_path,
            io_map_path,
            &analog_contract_path,
            &uf2_path,
        )?;
    }

    if output_mode == CliOutputMode::Json {
        #[derive(Serialize)]
        struct BuildRp2040Json {
            schema_version: u32,
            command: &'static str,
            output: &'static str,
            status: &'static str,
            out_dir: String,
            artifacts: BTreeMap<&'static str, String>,
        }
        let mut artifacts = BTreeMap::<&'static str, String>::new();
        artifacts.insert(
            "generated_program",
            display_path_relative_to_cwd(&generated_path),
        );
        artifacts.insert("io_map_template", display_path_relative_to_cwd(&iomap_path));
        artifacts.insert(
            "analog_contract",
            display_path_relative_to_cwd(&analog_contract_path),
        );
        artifacts.insert(
            "analog_calibration_template",
            display_path_relative_to_cwd(&analog_cal_template_path),
        );
        artifacts.insert("build_meta", display_path_relative_to_cwd(&meta_path));
        let payload = BuildRp2040Json {
            schema_version: 1,
            command: "build-rp2040",
            output: output_mode.as_str(),
            status: "pass",
            out_dir: display_path_relative_to_cwd(&out_dir),
            artifacts,
        };
        let mut json = serde_json::to_string_pretty(&payload)
            .map_err(|err| format!("Failed to serialize build-rp2040 JSON output: {err}"))?;
        json.push('\n');
        print!("{json}");
    }

    Ok(())
}

fn run_build_renode_stm32_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = "Usage: ".to_string()
        + program
        + " build-renode-stm32 <source.plc|source.bundle.toml> --scenario <scenario.yaml> --out <dir> [--output <human|json>]";
    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut scenario_path: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut output_mode = CliOutputMode::Human;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenario" => {
                scenario_path = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| "Missing value for --scenario <scenario.yaml>".to_string())?,
                ));
            }
            "--out" => {
                out_dir =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --out <dir>".to_string()
                    })?));
            }
            "--output" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --output <human|json>".to_string())?;
                output_mode = CliOutputMode::parse(&raw).ok_or_else(|| {
                    format!("Invalid --output value `{raw}` (expected `human` or `json`)")
                })?;
            }
            "-h" | "--help" => return Err(usage.clone()),
            other => return Err(format!("Unknown argument for build-renode-stm32: {other}")),
        }
    }

    let scenario_path = scenario_path.ok_or_else(|| usage.clone())?;
    let out_dir = out_dir.ok_or_else(|| usage.clone())?;
    fs::create_dir_all(&out_dir)
        .map_err(|err| format!("Failed to create out dir {out_dir:?}: {err}"))?;

    let loaded = load_plc_source(Path::new(&plc_path))?;
    let scenario_yaml = read_scenario_yaml_file(&scenario_path)?;
    let scenario_yaml =
        resolve_scenario_yaml_for_plc(&loaded.source, &scenario_yaml).map_err(|e| {
            format_resolve_scenario_yaml_error(&plc_path, &scenario_path, "build-renode-stm32", &e)
        })?;
    let scenario = parse_scenario_yaml(&scenario_yaml)?;

    let plc_source = loaded.source.clone();
    let plc_bytes = plc_source.as_bytes().to_vec();
    let sha256 = {
        let mut h = Sha256::new();
        h.update(&plc_bytes);
        hex::encode(h.finalize())
    };

    let ir_bundle = compile_pipeline(&loaded).map_err(|errors| errors.join("\n\n"))?;
    let runtime_program = state_machine_to_runtime_program(
        &ir_bundle.topology,
        &ir_bundle.constraints,
        &ir_bundle.state_machine,
        scenario.tick_ms,
    )
    .map_err(|err| format!("Failed to bridge to runtime Program: {err}"))?;
    let generated_src = codegen::generate_program_module(&runtime_program, "generated")
        .map_err(|err| format!("Codegen failed: {err:?}"))?;
    let generated_path = out_dir.join("generated_program.rs");
    fs::write(&generated_path, ensure_trailing_newline(generated_src))
        .map_err(|err| format!("Failed to write {generated_path:?}: {err}"))?;

    let scenario_out_path = out_dir.join("scenario.resolved.yaml");
    fs::write(&scenario_out_path, &scenario_yaml)
        .map_err(|err| format!("Failed to write {scenario_out_path:?}: {err}"))?;

    let generated_at = time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string());
    let git_metadata = detect_git_metadata();
    let meta = BuildMeta {
        plc_sha256: &sha256,
        generated_at: &generated_at,
        tool_version: env!("CARGO_PKG_VERSION"),
        runtime_semver: runtime_core::VERSION,
        git_commit: &git_metadata.commit,
        git_dirty: git_metadata.dirty,
        runtime_budget: ir_bundle.runtime_budget.summary(),
        realtime_profile: RealtimeProfile {
            tick_ms: scenario.tick_ms,
            thresholds: RealtimeThresholdConfig {
                max_p99_exec_us: None,
                max_overrun_count: None,
            },
            overrun_count: 0,
            p99_exec_us: 0,
        },
        io_map: None,
    };
    let mut meta_json = serde_json::to_string_pretty(&meta)
        .map_err(|err| format!("Failed to serialize build_meta.json: {err}"))?;
    meta_json.push('\n');
    let meta_path = out_dir.join("build_meta.json");
    fs::write(&meta_path, meta_json)
        .map_err(|err| format!("Failed to write {meta_path:?}: {err}"))?;

    let elf_path = emit_renode_stm32_elf(&generated_path, &scenario_out_path, &out_dir)?;

    if output_mode == CliOutputMode::Json {
        #[derive(Serialize)]
        struct BuildRenodeJson {
            schema_version: u32,
            command: &'static str,
            output: &'static str,
            status: &'static str,
            out_dir: String,
            artifacts: BTreeMap<&'static str, String>,
        }
        let mut artifacts = BTreeMap::<&'static str, String>::new();
        artifacts.insert(
            "generated_program",
            display_path_relative_to_cwd(&generated_path),
        );
        artifacts.insert("scenario", display_path_relative_to_cwd(&scenario_out_path));
        artifacts.insert("elf", display_path_relative_to_cwd(&elf_path));
        artifacts.insert("build_meta", display_path_relative_to_cwd(&meta_path));
        let payload = BuildRenodeJson {
            schema_version: 1,
            command: "build-renode-stm32",
            output: output_mode.as_str(),
            status: "pass",
            out_dir: display_path_relative_to_cwd(&out_dir),
            artifacts,
        };
        let mut json = serde_json::to_string_pretty(&payload)
            .map_err(|err| format!("Failed to serialize build-renode-stm32 JSON output: {err}"))?;
        json.push('\n');
        print!("{json}");
    }

    Ok(())
}


fn tool_command(bin: &str) -> std::process::Command {
    if let Some((program, args)) = split_command_prefix(bin) {
        let mut cmd = std::process::Command::new(program);
        cmd.args(args);
        return cmd;
    }

    #[cfg(windows)]
    {
        let lower = bin.to_ascii_lowercase();
        if lower.ends_with(".bat") {
            let mut cmd = std::process::Command::new("cmd");
            cmd.arg("/C").arg(bin);
            return cmd;
        }
        if lower.ends_with(".ps1") {
            let mut cmd = std::process::Command::new("powershell");
            cmd.arg("-NonInteractive").arg("-File").arg(bin);
            return cmd;
        }
    }
    std::process::Command::new(bin)
}

fn split_command_prefix(raw: &str) -> Option<(&str, Vec<&str>)> {
    let parts: Vec<&str> = raw.split_whitespace().collect();
    if parts.len() <= 1 {
        return None;
    }
    Some((parts[0], parts[1..].to_vec()))
}

fn uses_wsl_command_prefix(raw: &str) -> bool {
    split_command_prefix(raw)
        .map(|(program, _)| program.eq_ignore_ascii_case("wsl"))
        .unwrap_or(false)
}

#[cfg(windows)]
fn windows_path_to_wsl(path: &Path) -> Option<PathBuf> {
    use std::path::{Component, Prefix};

    let mut components = path.components();
    let drive = match components.next()? {
        Component::Prefix(prefix) => match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                (letter as char).to_ascii_lowercase()
            }
            _ => return None,
        },
        _ => return None,
    };

    let mut out = PathBuf::from(format!("/mnt/{drive}"));
    for component in components {
        match component {
            Component::RootDir => {}
            Component::Normal(part) => out.push(part),
            _ => return None,
        }
    }
    Some(out)
}

#[cfg(not(windows))]
fn windows_path_to_wsl(_path: &Path) -> Option<PathBuf> {
    None
}

fn ensure_trailing_newline(mut text: String) -> String {
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

fn emit_renode_stm32_elf(
    generated_program_rs: &Path,
    scenario_yaml: &Path,
    out_dir: &Path,
) -> Result<PathBuf, String> {
    let generated_program_rs = absolutize_path(generated_program_rs)?;
    let scenario_yaml = absolutize_path(scenario_yaml)?;
    let out_dir = absolutize_path(out_dir)?;
    fs::create_dir_all(&out_dir)
        .map_err(|err| format!("Failed to create Renode build output dir {out_dir:?}: {err}"))?;

    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cargo_bin = env::var("RUST_PLC_CARGO_BIN").unwrap_or_else(|_| "cargo".to_string());
    let target_dir = out_dir.join("cargo-target");
    let target_dir_for_process = if uses_wsl_command_prefix(&cargo_bin) {
        windows_path_to_wsl(&target_dir).unwrap_or_else(|| target_dir.clone())
    } else {
        target_dir.clone()
    };
    let generated_program_for_process = if uses_wsl_command_prefix(&cargo_bin) {
        windows_path_to_wsl(&generated_program_rs).unwrap_or_else(|| generated_program_rs.clone())
    } else {
        generated_program_rs.clone()
    };
    let scenario_yaml_for_process = if uses_wsl_command_prefix(&cargo_bin) {
        windows_path_to_wsl(&scenario_yaml).unwrap_or_else(|| scenario_yaml.clone())
    } else {
        scenario_yaml.clone()
    };
    let cargo = tool_command(&cargo_bin)
        .current_dir(&repo_root)
        .env("CARGO_TARGET_DIR", &target_dir_for_process)
        .env("RUSTFLAGS", "-C link-arg=-Tlink.x")
        .env("RUST_PLC_GENERATED_PROGRAM_RS", &generated_program_for_process)
        .env("RUST_PLC_SCENARIO_YAML", &scenario_yaml_for_process)
        .arg("build")
        .arg("-p")
        .arg("board-renode-stm32")
        .arg("--target")
        .arg("thumbv7em-none-eabi")
        .arg("--release")
        .output()
        .map_err(|err| {
            format!("Failed to run cargo for Renode STM32 firmware build (bin={cargo_bin}): {err}")
        })?;
    if !cargo.status.success() {
        return Err(format!(
            "Renode STM32 firmware build failed.\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&cargo.stdout),
            String::from_utf8_lossy(&cargo.stderr)
        ));
    }

    let built_elf = target_dir.join("thumbv7em-none-eabi/release/board-renode-stm32");
    if !built_elf.exists() {
        return Err(format!(
            "Expected Renode STM32 firmware ELF does not exist after build: {built_elf:?}"
        ));
    }
    let elf_out = out_dir.join("board-renode-stm32.elf");
    fs::copy(&built_elf, &elf_out)
        .map_err(|err| format!("Failed to copy Renode STM32 ELF to {elf_out:?}: {err}"))?;
    Ok(elf_out)
}

fn emit_rp2040_uf2(
    generated_program_rs: &Path,
    io_map_toml: &Path,
    analog_contract_toml: &Path,
    uf2_out: &Path,
) -> Result<(), String> {
    let generated_program_rs = absolutize_path(generated_program_rs)?;
    let io_map_toml = absolutize_path(io_map_toml)?;
    let analog_contract_toml = absolutize_path(analog_contract_toml)?;
    let uf2_out = absolutize_path(uf2_out)?;

    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(parent) = uf2_out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to create UF2 output dir {parent:?}: {err}"))?;
        }
    }

    let cargo_bin = env::var("RUST_PLC_CARGO_BIN").unwrap_or_else(|_| "cargo".to_string());
    let elf2uf2_bin = env::var("RUST_PLC_ELF2UF2_BIN").unwrap_or_else(|_| "elf2uf2-rs".to_string());

    let cargo = tool_command(&cargo_bin)
        .current_dir(&repo_root)
        .env("RUST_PLC_GENERATED_PROGRAM_RS", &generated_program_rs)
        .env("RUST_PLC_IO_MAP_TOML", &io_map_toml)
        .env("RUST_PLC_ANALOG_CONTRACT_TOML", &analog_contract_toml)
        .arg("build")
        .arg("-p")
        .arg("board-rp2040")
        .arg("--target")
        .arg("thumbv6m-none-eabi")
        .arg("--release")
        .output()
        .map_err(|err| {
            format!("Failed to run cargo for RP2040 firmware build (bin={cargo_bin}): {err}")
        })?;
    if !cargo.status.success() {
        return Err(format!(
            "RP2040 firmware build failed.\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&cargo.stdout),
            String::from_utf8_lossy(&cargo.stderr)
        ));
    }

    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                repo_root.join(path)
            }
        })
        .unwrap_or_else(|| repo_root.join("target"));
    let elf = target_dir.join("thumbv6m-none-eabi/release/board-rp2040");
    if !elf.exists() {
        return Err(format!(
            "Expected firmware ELF does not exist after build: {elf:?}"
        ));
    }

    let uf2 = tool_command(&elf2uf2_bin)
        .arg(&elf)
        .arg(&uf2_out)
        .output()
        .map_err(|err| {
            format!("Failed to run {elf2uf2_bin} (install with `cargo install elf2uf2-rs`): {err}")
        })?;
    if !uf2.status.success() {
        return Err(format!(
            "UF2 conversion failed.\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&uf2.stdout),
            String::from_utf8_lossy(&uf2.stderr)
        ));
    }

    Ok(())
}

fn build_analog_contract(input: &LoadedPlcSource) -> Result<AnalogContract, String> {
    let parsed = parse_loaded_plc_with_required_purpose(input)
        .map_err(|err| format!("Failed to parse PLC source: {err}"))?;
    let expanded = preprocess_program(&parsed)
        .map_err(|errors| format_loaded_plc_errors(errors, input).join("\n"))?;

    let mut analog_inputs = BTreeMap::<String, AnalogInputContractEntry>::new();
    let mut analog_outputs = BTreeMap::<String, AnalogOutputContractEntry>::new();
    for d in expanded.topology.devices {
        match d.device_type {
            rust_plc::ast::DeviceType::AnalogInput => {
                let Some(id) = parse_prefixed_numeric_id(&d.name, "AI") else {
                    continue;
                };
                let (min, max) = d
                    .attributes
                    .range
                    .map(|r| (r.min as f32, r.max as f32))
                    // Fallback keeps old projects buildable even when range is omitted.
                    .unwrap_or((0.0, 3.3));
                analog_inputs.insert(
                    format!("ai{id}"),
                    AnalogInputContractEntry {
                        min,
                        max,
                        scale: 1.0,
                        offset: 0.0,
                        unit: d.attributes.unit.clone(),
                    },
                );
            }
            rust_plc::ast::DeviceType::AnalogOutput => {
                let Some(id) = parse_prefixed_numeric_id(&d.name, "AO") else {
                    continue;
                };
                let (min, max) = d
                    .attributes
                    .range
                    .map(|r| (r.min as f32, r.max as f32))
                    .unwrap_or((0.0, 10.0));
                let ramp_ms = d
                    .attributes
                    .ramp_time
                    .as_ref()
                    .map(duration_to_ms)
                    .unwrap_or(0);
                analog_outputs.insert(
                    format!("ao{id}"),
                    AnalogOutputContractEntry {
                        min,
                        max,
                        ramp_ms,
                        scale: 1.0,
                        offset: 0.0,
                        unit: d.attributes.unit.clone(),
                    },
                );
            }
            _ => {}
        }
    }

    Ok(AnalogContract {
        analog_inputs,
        analog_outputs,
    })
}

fn parse_prefixed_numeric_id(name: &str, prefix: &str) -> Option<u16> {
    name.strip_prefix(prefix)?.parse::<u16>().ok()
}

fn duration_to_ms(duration: &rust_plc::ast::DurationValue) -> u64 {
    match duration.unit {
        rust_plc::ast::TimeUnit::Ms => duration.value,
        rust_plc::ast::TimeUnit::S => duration.value.saturating_mul(1000),
    }
}

fn apply_analog_calibration(
    contract: &mut AnalogContract,
    calibration_toml: &str,
) -> Result<(), String> {
    let cal: AnalogCalibrationFile =
        toml::from_str(calibration_toml).map_err(|e| format!("Invalid calibration TOML: {e}"))?;

    for (k, v) in &cal.analog_inputs {
        validate_calibration_entry(v, &format!("analog_inputs.{k}"))?;
        let entry = contract.analog_inputs.get_mut(k).ok_or_else(|| {
            format!("analog calibration key not found in contract: analog_inputs.{k}")
        })?;
        entry.scale = v.scale;
        entry.offset = v.offset;
    }
    for (k, v) in &cal.analog_outputs {
        validate_calibration_entry(v, &format!("analog_outputs.{k}"))?;
        let entry = contract.analog_outputs.get_mut(k).ok_or_else(|| {
            format!("analog calibration key not found in contract: analog_outputs.{k}")
        })?;
        entry.scale = v.scale;
        entry.offset = v.offset;
    }
    Ok(())
}

fn validate_calibration_entry(v: &AnalogCalibrationEntry, scope: &str) -> Result<(), String> {
    if !v.scale.is_finite() || v.scale.abs() < 1e-9 {
        return Err(format!("{scope}.scale must be finite and non-zero"));
    }
    if !v.offset.is_finite() {
        return Err(format!("{scope}.offset must be finite"));
    }
    Ok(())
}

fn analog_calibration_template_for_contract(contract: &AnalogContract) -> String {
    let mut out = String::new();
    out.push_str("# Analog calibration template (optional)\n");
    out.push_str("#\n");
    out.push_str("# The firmware applies calibration as:\n");
    out.push_str("#   eng_calibrated = eng_raw * scale + offset\n");
    out.push_str("#\n");
    out.push_str("# Notes:\n");
    out.push_str("# - Keys match analog_contract.toml sections: ai0/ao0/...\n");
    out.push_str("# - Only entries present here override defaults.\n\n");

    if !contract.analog_inputs.is_empty() {
        out.push_str("[analog_inputs]\n");
        for k in contract.analog_inputs.keys() {
            out.push_str(&format!("# {k} = {{ scale = 1.0, offset = 0.0 }}\n"));
        }
        out.push('\n');
    }

    if !contract.analog_outputs.is_empty() {
        out.push_str("[analog_outputs]\n");
        for k in contract.analog_outputs.keys() {
            out.push_str(&format!("# {k} = {{ scale = 1.0, offset = 0.0 }}\n"));
        }
        out.push('\n');
    }

    out
}

fn absolutize_path(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let cwd = env::current_dir().map_err(|err| format!("Failed to read current dir: {err}"))?;
    Ok(cwd.join(path))
}

fn detect_git_metadata() -> GitMetadata {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let commit = std::process::Command::new("git")
        .current_dir(&repo_root)
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
            } else {
                None
            }
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

    let dirty = std::process::Command::new("git")
        .current_dir(&repo_root)
        .arg("status")
        .arg("--porcelain")
        .output()
        .ok()
        .map(|out| out.status.success() && !out.stdout.is_empty())
        .unwrap_or(false);

    GitMetadata { commit, dirty }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

fn sha256_file(path: &Path) -> Result<(String, u64), String> {
    let bytes = fs::read(path).map_err(|err| format!("Failed to read artifact {path:?}: {err}"))?;
    let size = bytes.len() as u64;
    Ok((sha256_hex(&bytes), size))
}

fn io_map_template_for_program(program: &Program<'_>) -> String {
    use std::collections::BTreeSet;

    let mut dis = BTreeSet::<u16>::new();
    let mut dos = BTreeSet::<u16>::new();
    let mut ais = BTreeSet::<u16>::new();
    let mut aos = BTreeSet::<u16>::new();
    for task in program.tasks {
        for step in task.steps {
            match step.instr {
                Instr::WaitAllDigital { conditions, .. } => {
                    for condition in conditions {
                        dis.insert(condition.id.0);
                    }
                }
                Instr::WaitDigital { id, .. } => {
                    dis.insert(id.0);
                }
                Instr::WaitDigitalEdge { id, .. } => {
                    dis.insert(id.0);
                }
                Instr::WaitAnalog { id, .. } => {
                    ais.insert(id.0);
                }
                Instr::Action { actions, .. } => {
                    for a in actions {
                        match *a {
                            Action::SetDigital { id, .. } => {
                                dos.insert(id.0);
                            }
                            Action::Extend { output }
                            | Action::Retract { output }
                            | Action::CylinderMotion { output, .. } => {
                                dos.insert(output.0);
                            }
                            Action::SetAnalog { id, .. } => {
                                aos.insert(id.0);
                            }
                            Action::SetAnalogExpr { id, .. } => {
                                aos.insert(id.0);
                            }
                            Action::Compute { .. }
                            | Action::CallExtern { .. }
                            | Action::AxisMove { .. }
                            | Action::ProcessDeviceAction { .. }
                            | Action::CamEngage { .. }
                            | Action::CamDisengage { .. }
                            | Action::CamSwitch { .. }
                            | Action::CamPhase { .. }
                            | Action::WorkpieceAcquire { .. }
                            | Action::WorkpieceTransfer { .. }
                            | Action::WorkpieceFinish { .. }
                            | Action::WorkpieceMount { .. }
                            | Action::WorkpieceUnmount { .. }
                            | Action::WorkpieceTransformCarrier { .. }
                            | Action::WorkpieceSplit { .. }
                            | Action::WorkpieceMerge { .. } => {}
                            Action::Log { .. } => {}
                        }
                    }
                }
                Instr::WaitExpr { .. }
                | Instr::WaitVariableEdge { .. }
                | Instr::WaitCamDigital { .. }
                | Instr::WaitCamAnalog { .. }
                | Instr::Delay { .. }
                | Instr::Goto { .. }
                | Instr::Halt => {}
            }
        }
    }
    for cam in program.cam_configs {
        ais.insert(cam.master_input.0);
        ais.insert(cam.slave_feedback.0);
        aos.insert(cam.slave_output.0);
    }
    for pid in program.pid_loops {
        ais.insert(pid.pv.0);
        aos.insert(pid.out.0);
    }

    let mut out = String::new();
    out.push_str("# RP2040 I/O map template (fill in GPIO numbers for your wiring)\n");
    out.push_str("# This file is a template; it may be incomplete by design.\n\n");
    out.push_str("# GPIO mapping notes:\n");
    out.push_str("# - DI/DO/AO: 0..=29 or \"virtual\" (no physical GPIO binding)\n");
    out.push_str("# - AI: 26..=29 (ADC-capable) or \"virtual\" (board-provided synthetic)\n\n");

    out.push_str("[digital_inputs]\n");
    if dis.is_empty() {
        out.push_str("# di0 = 2\n");
    } else {
        for id in dis {
            out.push_str(&format!("# di{id} = 2\n"));
        }
    }
    out.push('\n');

    out.push_str("[digital_outputs]\n");
    if dos.is_empty() {
        out.push_str("# do0 = 16\n");
    } else {
        for id in dos {
            out.push_str(&format!("# do{id} = 16\n"));
        }
    }
    out.push('\n');

    out.push_str("[analog_inputs]\n");
    out.push_str("# RP2040 ADC-capable GPIO: 26, 27, 28, 29\n");
    if ais.is_empty() {
        out.push_str("# ai0 = 26\n");
    } else {
        for id in ais {
            out.push_str(&format!("# ai{id} = 26\n"));
        }
    }
    out.push('\n');

    out.push_str("[analog_outputs]\n");
    if aos.is_empty() {
        out.push_str("# ao0 = 26\n");
    } else {
        for id in aos {
            out.push_str(&format!("# ao{id} = 26\n"));
        }
    }

    out.push('\n');
    out.push_str("# Motion (optional): Pulse/Dir stepper + AB encoder (PIO-first).\n");
    out.push_str(
        "# These channels are NOT inferred from the PLC program. Fill in GPIO wiring and\n",
    );
    out.push_str("# axis parameters if you plan to use board-level motion feedback/commands.\n");
    out.push_str("#\n");
    out.push_str("# Note: if you include a [motion] section, it must not be empty.\n");
    out.push_str("#\n");
    out.push_str("# [motion.stepper.axis0]\n");
    out.push_str("# step_gpio = 2\n");
    out.push_str("# dir_gpio = 3\n");
    out.push_str("# en_gpio = 4\n");
    out.push_str("# dir_inverted = false\n");
    out.push_str("# v_max_sps = 20000  # steps per second\n");
    out.push_str("# acc_sps2 = 40000   # steps per second^2\n");
    out.push_str("# dec_sps2 = 40000   # steps per second^2\n");
    out.push_str("#\n");
    out.push_str("# [motion.stepper.axis1]\n");
    out.push_str("# step_gpio = 5\n");
    out.push_str("# dir_gpio = 6\n");
    out.push_str("# en_gpio = 7\n");
    out.push_str("# dir_inverted = false\n");
    out.push_str("# v_max_sps = 20000\n");
    out.push_str("# acc_sps2 = 40000\n");
    out.push_str("# dec_sps2 = 40000\n");
    out.push_str("#\n");
    out.push_str("# [motion.encoder.axis0]\n");
    out.push_str("# a_gpio = 8\n");
    out.push_str("# b_gpio = 9\n");
    out.push_str("# ppr = 1024\n");
    out.push_str("# quad = 4\n");
    out.push_str("# count_sign = \"normal\"  # normal|inverted\n");
    out.push_str("# scale = 1.0\n");
    out.push_str("#\n");
    out.push_str("# [motion.encoder.axis1]\n");
    out.push_str("# a_gpio = 10\n");
    out.push_str("# b_gpio = 11\n");
    out.push_str("# ppr = 1024\n");
    out.push_str("# quad = 4\n");
    out.push_str("# count_sign = \"normal\"\n");
    out.push_str("# scale = 1.0\n");

    out.push('\n');
    out.push_str("[safe_state]\n");
    out.push_str("# Default: all outputs -> 0 on exit (de-energize)\n");
    out.push_str("# mode = \"all_zero\"  # all_zero | profile\n");
    out.push_str("# on_exit_timeout_ms = 300\n");
    out.push_str("#\n");
    out.push_str("# If mode = \"profile\", define per-output safe values and ordering groups.\n");
    out.push_str("# Example (NC brake coil, 0=brake):\n");
    out.push_str("# [safe_state.do.Y2]\n");
    out.push_str("# safe_value = 0\n");
    out.push_str("# group = 10\n");
    out.push_str("#\n");
    out.push_str("# Example (disable stepper enable after brake):\n");
    out.push_str("# [safe_state.do.Y1]\n");
    out.push_str("# safe_value = 0\n");
    out.push_str("# group = 20\n");
    out.push_str("#\n");
    out.push_str("# Example (analog output safe value):\n");
    out.push_str("# [safe_state.ao.AO0]\n");
    out.push_str("# safe_value = 0.0\n");
    out.push_str("# group = 30\n");
    out
}

fn io_usage_for_program(program: &Program<'_>) -> IoUsage {
    use std::collections::BTreeSet;

    let mut dis = BTreeSet::<u16>::new();
    let mut dos = BTreeSet::<u16>::new();
    let mut ais = BTreeSet::<u16>::new();
    let mut aos = BTreeSet::<u16>::new();
    for task in program.tasks {
        for step in task.steps {
            match step.instr {
                Instr::WaitAllDigital { conditions, .. } => {
                    for condition in conditions {
                        dis.insert(condition.id.0);
                    }
                }
                Instr::WaitDigital { id, .. } => {
                    dis.insert(id.0);
                }
                Instr::WaitDigitalEdge { id, .. } => {
                    dis.insert(id.0);
                }
                Instr::WaitAnalog { id, .. } => {
                    ais.insert(id.0);
                }
                Instr::Action { actions, .. } => {
                    for a in actions {
                        match *a {
                            Action::SetDigital { id, .. } => {
                                dos.insert(id.0);
                            }
                            Action::Extend { output }
                            | Action::Retract { output }
                            | Action::CylinderMotion { output, .. } => {
                                dos.insert(output.0);
                            }
                            Action::SetAnalog { id, .. } => {
                                aos.insert(id.0);
                            }
                            Action::SetAnalogExpr { id, .. } => {
                                aos.insert(id.0);
                            }
                            Action::Compute { .. }
                            | Action::CallExtern { .. }
                            | Action::AxisMove { .. }
                            | Action::ProcessDeviceAction { .. }
                            | Action::CamEngage { .. }
                            | Action::CamDisengage { .. }
                            | Action::CamSwitch { .. }
                            | Action::CamPhase { .. }
                            | Action::WorkpieceAcquire { .. }
                            | Action::WorkpieceTransfer { .. }
                            | Action::WorkpieceFinish { .. }
                            | Action::WorkpieceMount { .. }
                            | Action::WorkpieceUnmount { .. }
                            | Action::WorkpieceTransformCarrier { .. }
                            | Action::WorkpieceSplit { .. }
                            | Action::WorkpieceMerge { .. } => {}
                            Action::Log { .. } => {}
                        }
                    }
                }
                Instr::WaitExpr { .. }
                | Instr::WaitVariableEdge { .. }
                | Instr::WaitCamDigital { .. }
                | Instr::WaitCamAnalog { .. }
                | Instr::Delay { .. }
                | Instr::Goto { .. }
                | Instr::Halt => {}
            }
        }
    }
    for cam in program.cam_configs {
        ais.insert(cam.master_input.0);
        ais.insert(cam.slave_feedback.0);
        aos.insert(cam.slave_output.0);
    }
    for pid in program.pid_loops {
        ais.insert(pid.pv.0);
        aos.insert(pid.out.0);
    }

    // `IoUsage` is a tiny borrowed wrapper; we leak the sets to keep build-rp2040 code simple.
    let dis: &'static [u16] = Box::leak(dis.into_iter().collect::<Vec<_>>().into_boxed_slice());
    let dos: &'static [u16] = Box::leak(dos.into_iter().collect::<Vec<_>>().into_boxed_slice());
    let ais: &'static [u16] = Box::leak(ais.into_iter().collect::<Vec<_>>().into_boxed_slice());
    let aos: &'static [u16] = Box::leak(aos.into_iter().collect::<Vec<_>>().into_boxed_slice());
    IoUsage {
        digital_inputs: dis,
        digital_outputs: dos,
        analog_inputs: ais,
        analog_outputs: aos,
    }
}

