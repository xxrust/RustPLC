use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    // Make `memory.x` visible to the linker when building for embedded targets.
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR must be set"));
    fs::write(out_dir.join("memory.x"), include_bytes!("memory.x")).expect("write memory.x");
    println!("cargo:rustc-link-search={}", out_dir.display());
    println!("cargo:rerun-if-changed=memory.x");

    // Allow injecting a generated Program module at build time.
    //
    // If `RUST_PLC_GENERATED_PROGRAM_RS` is set to a path, we copy it into OUT_DIR and include it.
    // Otherwise we emit a tiny default program that just halts.
    let generated_path = out_dir.join("generated_program.rs");
    println!("cargo:rerun-if-env-changed=RUST_PLC_GENERATED_PROGRAM_RS");
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| manifest_dir.clone());

    match env::var("RUST_PLC_GENERATED_PROGRAM_RS") {
        Ok(path) => {
            let path = resolve_input_path(&workspace_root, &path);
            let contents = fs::read_to_string(&path).unwrap_or_else(|e| {
                panic!(
                    "failed to read RUST_PLC_GENERATED_PROGRAM_RS={}: {e}",
                    path.display()
                )
            });
            fs::write(&generated_path, contents).expect("write generated_program.rs");
        }
        Err(_) => {
            fs::write(&generated_path, default_generated_program())
                .expect("write default generated_program.rs");
        }
    }

    // Compile-time embed of IO map TOML (optional).
    let io_map_rs_path = out_dir.join("io_map.rs");
    println!("cargo:rerun-if-env-changed=RUST_PLC_IO_MAP_TOML");
    println!("cargo:rerun-if-env-changed=RUST_PLC_ANALOG_CONTRACT_TOML");
    match env::var("RUST_PLC_IO_MAP_TOML") {
        Ok(path) => {
            let path = resolve_input_path(&workspace_root, &path);
            let toml_str = fs::read_to_string(&path).unwrap_or_else(|e| {
                panic!(
                    "failed to read RUST_PLC_IO_MAP_TOML={}: {e}",
                    path.display()
                )
            });
            let map =
                parse_io_map(&toml_str).unwrap_or_else(|e| panic!("invalid io map TOML: {e}"));
            let analog_contract = match env::var("RUST_PLC_ANALOG_CONTRACT_TOML") {
                Ok(analog_path) => {
                    let analog_path = resolve_input_path(&workspace_root, &analog_path);
                    let contract_toml = fs::read_to_string(&analog_path).unwrap_or_else(|e| {
                        panic!(
                            "failed to read RUST_PLC_ANALOG_CONTRACT_TOML={}: {e}",
                            analog_path.display()
                        )
                    });
                    parse_analog_contract(&contract_toml)
                        .unwrap_or_else(|e| panic!("invalid analog contract TOML: {e}"))
                }
                Err(_) => AnalogContract::default(),
            };
            fs::write(&io_map_rs_path, render_io_map_rs(&map, &analog_contract))
                .expect("write io_map.rs");
        }
        Err(_) => {
            fs::write(
                &io_map_rs_path,
                render_io_map_rs(&IoMap::default(), &AnalogContract::default()),
            )
            .expect("write io_map.rs");
        }
    }
}

fn resolve_input_path(workspace_root: &Path, raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        workspace_root.join(path)
    }
}

#[derive(Debug, Default)]
struct IoMap {
    digital_inputs: BTreeMap<u16, u8>,
    digital_outputs: BTreeMap<u16, u8>,
    analog_inputs: BTreeMap<u16, u8>,
    analog_outputs: BTreeMap<u16, u8>,
}

#[derive(Debug, Default)]
struct AnalogContract {
    analog_inputs: BTreeMap<u16, AnalogInputContractEntry>,
    analog_outputs: BTreeMap<u16, AnalogOutputContractEntry>,
}

#[derive(Debug, Clone, Copy)]
struct AnalogInputContractEntry {
    min: f32,
    max: f32,
    scale: f32,
    offset: f32,
}

#[derive(Debug, Clone, Copy)]
struct AnalogOutputContractEntry {
    min: f32,
    max: f32,
    ramp_ms: u32,
    scale: f32,
    offset: f32,
}

fn parse_io_map(input: &str) -> Result<IoMap, String> {
    let v: toml::Value = toml::from_str(input).map_err(|e| e.to_string())?;
    let di = v
        .get("digital_inputs")
        .and_then(|v| v.as_table())
        .ok_or_else(|| "missing [digital_inputs]".to_string())?;
    let do_ = v
        .get("digital_outputs")
        .and_then(|v| v.as_table())
        .ok_or_else(|| "missing [digital_outputs]".to_string())?;
    let ai = v.get("analog_inputs").and_then(|v| v.as_table());
    let ao = v.get("analog_outputs").and_then(|v| v.as_table());

    Ok(IoMap {
        digital_inputs: parse_section(di, "di", 0, 29, "0..=29")?,
        digital_outputs: parse_section(do_, "do", 0, 29, "0..=29")?,
        analog_inputs: match ai {
            Some(t) => parse_section(t, "ai", 26, 29, "26..=29 (RP2040 ADC-capable GPIO)")?,
            None => BTreeMap::new(),
        },
        analog_outputs: match ao {
            Some(t) => parse_section(t, "ao", 0, 29, "0..=29")?,
            None => BTreeMap::new(),
        },
    })
}

fn parse_section(
    t: &toml::value::Table,
    prefix: &str,
    min_gpio: i64,
    max_gpio: i64,
    allowed: &str,
) -> Result<BTreeMap<u16, u8>, String> {
    let mut out = BTreeMap::<u16, u8>::new();
    for (k, v) in t {
        let id_str = k
            .strip_prefix(prefix)
            .ok_or_else(|| format!("invalid key {k:?} (expected {prefix}<id>)"))?;
        let id: u16 = id_str
            .parse()
            .map_err(|_| format!("invalid key {k:?} (expected {prefix}<id>)"))?;
        let gpio = v
            .as_integer()
            .ok_or_else(|| format!("invalid value for {k:?} (expected integer gpio)"))?;
        if !(min_gpio..=max_gpio).contains(&gpio) {
            return Err(format!(
                "invalid gpio {gpio} for {prefix}{id} (allowed: {allowed})"
            ));
        }
        out.insert(id, gpio as u8);
    }
    Ok(out)
}

fn parse_analog_contract(input: &str) -> Result<AnalogContract, String> {
    let v: toml::Value = toml::from_str(input).map_err(|e| e.to_string())?;
    let ai = v.get("analog_inputs").and_then(|v| v.as_table());
    let ao = v.get("analog_outputs").and_then(|v| v.as_table());
    Ok(AnalogContract {
        analog_inputs: match ai {
            Some(t) => parse_analog_inputs_table(t)?,
            None => BTreeMap::new(),
        },
        analog_outputs: match ao {
            Some(t) => parse_analog_outputs_table(t)?,
            None => BTreeMap::new(),
        },
    })
}

fn parse_analog_inputs_table(
    table: &toml::value::Table,
) -> Result<BTreeMap<u16, AnalogInputContractEntry>, String> {
    let mut out = BTreeMap::new();
    for (k, v) in table {
        let id = parse_analog_key(k, "ai")?;
        let t = v
            .as_table()
            .ok_or_else(|| format!("analog_inputs.{k} must be a table"))?;
        let min = parse_required_float(t, "min", &format!("analog_inputs.{k}"))?;
        let max = parse_required_float(t, "max", &format!("analog_inputs.{k}"))?;
        if !(min < max) {
            return Err(format!(
                "analog_inputs.{k} has invalid range: min ({min}) must be < max ({max})"
            ));
        }
        let scale = parse_optional_float(t, "scale", &format!("analog_inputs.{k}"), 1.0)?;
        let offset = parse_optional_float(t, "offset", &format!("analog_inputs.{k}"), 0.0)?;
        out.insert(
            id,
            AnalogInputContractEntry {
                min,
                max,
                scale,
                offset,
            },
        );
    }
    Ok(out)
}

fn parse_analog_outputs_table(
    table: &toml::value::Table,
) -> Result<BTreeMap<u16, AnalogOutputContractEntry>, String> {
    let mut out = BTreeMap::new();
    for (k, v) in table {
        let id = parse_analog_key(k, "ao")?;
        let t = v
            .as_table()
            .ok_or_else(|| format!("analog_outputs.{k} must be a table"))?;
        let min = parse_required_float(t, "min", &format!("analog_outputs.{k}"))?;
        let max = parse_required_float(t, "max", &format!("analog_outputs.{k}"))?;
        if !(min < max) {
            return Err(format!(
                "analog_outputs.{k} has invalid range: min ({min}) must be < max ({max})"
            ));
        }
        let ramp_ms = t.get("ramp_ms").and_then(|v| v.as_integer()).unwrap_or(0);
        if ramp_ms < 0 {
            return Err(format!(
                "analog_outputs.{k}.ramp_ms must be >= 0, got {ramp_ms}"
            ));
        }
        let scale = parse_optional_float(t, "scale", &format!("analog_outputs.{k}"), 1.0)?;
        let offset = parse_optional_float(t, "offset", &format!("analog_outputs.{k}"), 0.0)?;
        out.insert(
            id,
            AnalogOutputContractEntry {
                min,
                max,
                ramp_ms: ramp_ms as u32,
                scale,
                offset,
            },
        );
    }
    Ok(out)
}

fn parse_analog_key(key: &str, prefix: &str) -> Result<u16, String> {
    let id_str = key
        .strip_prefix(prefix)
        .ok_or_else(|| format!("invalid key {key:?} (expected {prefix}<id>)"))?;
    id_str
        .parse::<u16>()
        .map_err(|_| format!("invalid key {key:?} (expected {prefix}<id>)"))
}

fn parse_required_float(
    table: &toml::value::Table,
    field: &str,
    scope: &str,
) -> Result<f32, String> {
    let v = table
        .get(field)
        .ok_or_else(|| format!("{scope}.{field} is required"))?;
    match v {
        toml::Value::Float(n) => Ok(*n as f32),
        toml::Value::Integer(n) => Ok(*n as f32),
        _ => Err(format!("{scope}.{field} must be a number")),
    }
}

fn parse_optional_float(
    table: &toml::value::Table,
    field: &str,
    scope: &str,
    default: f32,
) -> Result<f32, String> {
    let Some(v) = table.get(field) else {
        return Ok(default);
    };
    match v {
        toml::Value::Float(n) => Ok(*n as f32),
        toml::Value::Integer(n) => Ok(*n as f32),
        _ => Err(format!("{scope}.{field} must be a number")),
    }
}

fn render_io_map_rs(map: &IoMap, analog_contract: &AnalogContract) -> String {
    const MAX_DI: usize = 32;
    const MAX_DO: usize = 32;
    const MAX_AI: usize = 32;
    const MAX_AO: usize = 32;
    const UNUSED: u8 = 255;

    let mut di = [UNUSED; MAX_DI];
    let mut do_ = [UNUSED; MAX_DO];
    let mut ai = [UNUSED; MAX_AI];
    let mut ao = [UNUSED; MAX_AO];
    let mut ai_min = [0.0f32; MAX_AI];
    let mut ai_max = [3.3f32; MAX_AI];
    let mut ai_scale = [1.0f32; MAX_AI];
    let mut ai_offset = [0.0f32; MAX_AI];
    let mut ao_min = [0.0f32; MAX_AO];
    let mut ao_max = [10.0f32; MAX_AO];
    let mut ao_ramp = [0u32; MAX_AO];
    let mut ao_scale = [1.0f32; MAX_AO];
    let mut ao_offset = [0.0f32; MAX_AO];

    for (&id, &gpio) in &map.digital_inputs {
        let idx = id as usize;
        if idx >= MAX_DI {
            panic!("di{id} exceeds MAX_DI={MAX_DI}");
        }
        di[idx] = gpio;
    }
    for (&id, &gpio) in &map.digital_outputs {
        let idx = id as usize;
        if idx >= MAX_DO {
            panic!("do{id} exceeds MAX_DO={MAX_DO}");
        }
        do_[idx] = gpio;
    }
    for (&id, &gpio) in &map.analog_inputs {
        let idx = id as usize;
        if idx >= MAX_AI {
            panic!("ai{id} exceeds MAX_AI={MAX_AI}");
        }
        ai[idx] = gpio;
    }
    for (&id, &gpio) in &map.analog_outputs {
        let idx = id as usize;
        if idx >= MAX_AO {
            panic!("ao{id} exceeds MAX_AO={MAX_AO}");
        }
        ao[idx] = gpio;
    }
    for (&id, cfg) in &analog_contract.analog_inputs {
        let idx = id as usize;
        if idx >= MAX_AI {
            panic!("analog contract ai{id} exceeds MAX_AI={MAX_AI}");
        }
        ai_min[idx] = cfg.min;
        ai_max[idx] = cfg.max;
        ai_scale[idx] = cfg.scale;
        ai_offset[idx] = cfg.offset;
    }
    for (&id, cfg) in &analog_contract.analog_outputs {
        let idx = id as usize;
        if idx >= MAX_AO {
            panic!("analog contract ao{id} exceeds MAX_AO={MAX_AO}");
        }
        ao_min[idx] = cfg.min;
        ao_max[idx] = cfg.max;
        ao_ramp[idx] = cfg.ramp_ms;
        ao_scale[idx] = cfg.scale;
        ao_offset[idx] = cfg.offset;
    }

    let mut out = String::new();
    out.push_str("// @generated by board-rp2040 build.rs\n");
    out.push_str("pub const UNUSED_GPIO: u8 = 255;\n");
    out.push_str(&format!("pub const MAX_DI: usize = {MAX_DI};\n"));
    out.push_str(&format!("pub const MAX_DO: usize = {MAX_DO};\n"));
    out.push_str(&format!("pub const MAX_AI: usize = {MAX_AI};\n"));
    out.push_str(&format!("pub const MAX_AO: usize = {MAX_AO};\n"));
    out.push_str("pub const DI_GPIO: [u8; MAX_DI] = [\n");
    for v in di {
        out.push_str(&format!("  {v},\n"));
    }
    out.push_str("];\n");
    out.push_str("pub const DO_GPIO: [u8; MAX_DO] = [\n");
    for v in do_ {
        out.push_str(&format!("  {v},\n"));
    }
    out.push_str("];\n");
    out.push_str("pub const AI_GPIO: [u8; MAX_AI] = [\n");
    for v in ai {
        out.push_str(&format!("  {v},\n"));
    }
    out.push_str("];\n");
    out.push_str("pub const AO_GPIO: [u8; MAX_AO] = [\n");
    for v in ao {
        out.push_str(&format!("  {v},\n"));
    }
    out.push_str("];\n");
    out.push_str("pub const AI_ENG_MIN: [f32; MAX_AI] = [\n");
    for v in ai_min {
        out.push_str(&format!("  {v:.6},\n"));
    }
    out.push_str("];\n");
    out.push_str("pub const AI_ENG_MAX: [f32; MAX_AI] = [\n");
    for v in ai_max {
        out.push_str(&format!("  {v:.6},\n"));
    }
    out.push_str("];\n");
    out.push_str("pub const AI_CAL_SCALE: [f32; MAX_AI] = [\n");
    for v in ai_scale {
        out.push_str(&format!("  {v:.6},\n"));
    }
    out.push_str("];\n");
    out.push_str("pub const AI_CAL_OFFSET: [f32; MAX_AI] = [\n");
    for v in ai_offset {
        out.push_str(&format!("  {v:.6},\n"));
    }
    out.push_str("];\n");
    out.push_str("pub const AO_ENG_MIN: [f32; MAX_AO] = [\n");
    for v in ao_min {
        out.push_str(&format!("  {v:.6},\n"));
    }
    out.push_str("];\n");
    out.push_str("pub const AO_ENG_MAX: [f32; MAX_AO] = [\n");
    for v in ao_max {
        out.push_str(&format!("  {v:.6},\n"));
    }
    out.push_str("];\n");
    out.push_str("pub const AO_RAMP_MS: [u32; MAX_AO] = [\n");
    for v in ao_ramp {
        out.push_str(&format!("  {v},\n"));
    }
    out.push_str("];\n");
    out.push_str("pub const AO_CAL_SCALE: [f32; MAX_AO] = [\n");
    for v in ao_scale {
        out.push_str(&format!("  {v:.6},\n"));
    }
    out.push_str("];\n");
    out.push_str("pub const AO_CAL_OFFSET: [f32; MAX_AO] = [\n");
    for v in ao_offset {
        out.push_str(&format!("  {v:.6},\n"));
    }
    out.push_str("];\n");
    out
}

fn default_generated_program() -> &'static str {
    r#"// @generated default board-rp2040 program (no external generated Program provided)
#[allow(clippy::all)]

pub mod generated {
  use runtime_core::{Instr, Program, Step, StepId, Task};

  static STEPS: [Step<'static>; 1] = [
    Step { name: "halt", instr: Instr::Halt },
  ];

  static TASKS: [Task<'static>; 1] = [
    Task { name: "main", steps: &STEPS, entry: StepId(0) },
  ];

  pub static PROGRAM: Program<'static> = Program { tasks: &TASKS };
}
"#
}
