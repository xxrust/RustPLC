use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn example_path(name: &str) -> PathBuf {
    repo_root().join("examples").join(name)
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be monotonic")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "rust_plc_{prefix}_{}_{}",
        std::process::id(),
        nanos
    ));
    fs::create_dir_all(&dir).expect("should create temp dir");
    dir
}

fn run_gen_st(plc: &Path, out_st: &Path) {
    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("gen-st")
        .arg(plc)
        .arg("--out")
        .arg(out_st)
        .output()
        .expect("should run rust_plc gen-st");

    assert!(
        output.status.success(),
        "gen-st failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn maybe_iec2c() -> Option<String> {
    for candidate in ["iec2c", "matiec"] {
        let found = Command::new("bash")
            .arg("-lc")
            .arg(format!("command -v {candidate} >/dev/null 2>&1"))
            .status()
            .ok()
            .is_some_and(|status| status.success());
        if found {
            return Some(candidate.to_string());
        }
    }
    None
}

#[test]
fn st_codegen_two_cylinder_and_assembly_station_generate() {
    let dir = unique_temp_dir("st_codegen_generate");
    let two_st = dir.join("two_cylinder.st");
    let assembly_st = dir.join("assembly_station.st");

    run_gen_st(&example_path("two_cylinder.plc"), &two_st);
    run_gen_st(&example_path("assembly_station.plc"), &assembly_st);

    let two = fs::read_to_string(&two_st).expect("should read generated st");
    let assembly = fs::read_to_string(&assembly_st).expect("should read generated st");

    assert!(two.contains("PROGRAM Main"));
    assert!(two.contains("CASE _state OF"));
    assert!(assembly.contains("PROGRAM Main"));
    assert!(assembly.contains("CASE _state OF"));
}

#[test]
fn st_codegen_timer_calls_appear_before_case() {
    let dir = unique_temp_dir("st_codegen_timer");
    let out_st = dir.join("two_cylinder.st");

    run_gen_st(&example_path("two_cylinder.plc"), &out_st);
    let rendered = fs::read_to_string(&out_st).expect("should read generated st");

    let timer_pos = rendered
        .find("_timer_0(IN := _state = 0, PT := T#500ms);")
        .expect("timer call should exist");
    let case_pos = rendered.find("CASE _state OF").expect("CASE should exist");

    assert!(timer_pos < case_pos);
}

#[test]
fn st_codegen_two_cylinder_compiles_with_matiec_when_available() {
    let Some(iec2c) = maybe_iec2c() else {
        eprintln!("skip: iec2c/matiec not found in PATH");
        return;
    };

    let dir = unique_temp_dir("st_codegen_matiec_two");
    let out_st = dir.join("two_cylinder.st");
    run_gen_st(&example_path("two_cylinder.plc"), &out_st);

    let output = Command::new(&iec2c)
        .arg(&out_st)
        .current_dir(&dir)
        .output()
        .expect("should run iec2c");

    assert!(
        output.status.success(),
        "iec2c compile failed for two_cylinder.st:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn st_codegen_assembly_station_compiles_with_matiec_when_available() {
    let Some(iec2c) = maybe_iec2c() else {
        eprintln!("skip: iec2c/matiec not found in PATH");
        return;
    };

    let dir = unique_temp_dir("st_codegen_matiec_assembly");
    let out_st = dir.join("assembly_station.st");
    run_gen_st(&example_path("assembly_station.plc"), &out_st);

    let output = Command::new(&iec2c)
        .arg(&out_st)
        .current_dir(&dir)
        .output()
        .expect("should run iec2c");

    assert!(
        output.status.success(),
        "iec2c compile failed for assembly_station.st:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
