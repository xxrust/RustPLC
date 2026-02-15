use std::{env, fs, path::PathBuf};

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
    match env::var("RUST_PLC_GENERATED_PROGRAM_RS") {
        Ok(path) => {
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("failed to read RUST_PLC_GENERATED_PROGRAM_RS={path}: {e}"));
            fs::write(&generated_path, contents).expect("write generated_program.rs");
        }
        Err(_) => {
            fs::write(&generated_path, default_generated_program()).expect("write default generated_program.rs");
        }
    }
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
