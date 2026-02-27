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

/// Resolve the vendored iec2c binary and its lib working directory.
///
/// Resolution order:
///   1. vendor/matiec/bin/<platform>/iec2c[.exe]  — always present in repo
///   2. PATH (iec2c or matiec)                    — CI / developer install
///
/// Returns `None` when no usable binary is found on the current platform.
/// On platforms where no binary is available (e.g. Linux without a build),
/// the matiec tests are skipped rather than failed.
fn find_iec2c() -> Option<(PathBuf, PathBuf)> {
    let vendor_lib = repo_root().join("vendor").join("matiec").join("lib");

    // 1. Vendored binary
    #[cfg(target_os = "windows")]
    let vendor_bin = repo_root()
        .join("vendor")
        .join("matiec")
        .join("bin")
        .join("windows")
        .join("iec2c.exe");

    #[cfg(not(target_os = "windows"))]
    let vendor_bin = repo_root()
        .join("vendor")
        .join("matiec")
        .join("bin")
        .join("linux")
        .join("iec2c");

    if vendor_bin.exists() && vendor_lib.join("ieclib.txt").exists() {
        return Some((vendor_bin, vendor_lib));
    }

    // 2. PATH fallback — locate binary then find lib/ relative to it
    for candidate in ["iec2c", "matiec"] {
        let which_cmd = if cfg!(windows) { "where" } else { "which" };
        let Ok(out) = Command::new(which_cmd).arg(candidate).output() else {
            continue;
        };
        if !out.status.success() {
            continue;
        }
        let raw = String::from_utf8_lossy(&out.stdout);
        let Some(first_line) = raw.lines().next() else {
            continue;
        };
        let bin_path = PathBuf::from(first_line.trim());
        // Walk up from the binary looking for lib/ieclib.txt
        let mut dir = bin_path.parent().map(|p| p.to_path_buf());
        while let Some(d) = dir {
            if d.join("lib").join("ieclib.txt").exists() {
                return Some((bin_path, d.join("lib")));
            }
            dir = d.parent().map(|p| p.to_path_buf());
        }
    }

    None
}

/// Run iec2c on `st_file`, writing generated C artifacts to `out_dir`.
/// `lib_dir` must contain ieclib.txt; iec2c is invoked with that as cwd.
fn run_iec2c(iec2c: &Path, lib_dir: &Path, st_file: &Path, out_dir: &Path) -> std::process::Output {
    // iec2c resolves lib/ieclib.txt relative to cwd, so we run from lib_dir's parent
    // (which has lib/ as a subdirectory).
    let cwd = lib_dir.parent().unwrap_or(lib_dir);
    Command::new(iec2c)
        .arg("-T")
        .arg(out_dir)
        .arg(st_file)
        .current_dir(cwd)
        .output()
        .expect("should run iec2c")
}

// ---------------------------------------------------------------------------
// Tests: ST generation (no external tool required)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Tests: matiec round-trip (requires vendored or PATH iec2c)
// ---------------------------------------------------------------------------

/// Verify that the vendored iec2c binary and lib/ are present in the repo.
/// This test always runs and fails loudly if the vendor directory is incomplete,
/// so that a broken vendor state is caught before the round-trip tests are skipped.
#[test]
fn matiec_vendor_directory_is_complete() {
    let lib = repo_root().join("vendor").join("matiec").join("lib");
    assert!(
        lib.join("ieclib.txt").exists(),
        "vendor/matiec/lib/ieclib.txt is missing — run the vendor copy step"
    );

    // On Windows we always expect the pre-built binary.
    #[cfg(target_os = "windows")]
    {
        let bin = repo_root()
            .join("vendor")
            .join("matiec")
            .join("bin")
            .join("windows")
            .join("iec2c.exe");
        assert!(
            bin.exists(),
            "vendor/matiec/bin/windows/iec2c.exe is missing"
        );
    }
}

#[test]
fn st_codegen_two_cylinder_compiles_with_matiec() {
    let Some((iec2c, lib_dir)) = find_iec2c() else {
        eprintln!("[SKIP] iec2c not available on this platform — skipping matiec round-trip");
        return;
    };

    let dir = unique_temp_dir("st_codegen_matiec_two");
    let out_st = dir.join("two_cylinder.st");
    run_gen_st(&example_path("two_cylinder.plc"), &out_st);

    let output = run_iec2c(&iec2c, &lib_dir, &out_st, &dir);
    assert!(
        output.status.success(),
        "iec2c compile failed for two_cylinder.st:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify iec2c produced the expected C artifacts
    assert!(
        dir.join("POUS.c").exists(),
        "iec2c should produce POUS.c for two_cylinder"
    );
    assert!(
        dir.join("POUS.h").exists(),
        "iec2c should produce POUS.h for two_cylinder"
    );
}

#[test]
fn st_codegen_assembly_station_compiles_with_matiec() {
    let Some((iec2c, lib_dir)) = find_iec2c() else {
        eprintln!("[SKIP] iec2c not available on this platform — skipping matiec round-trip");
        return;
    };

    let dir = unique_temp_dir("st_codegen_matiec_assembly");
    let out_st = dir.join("assembly_station.st");
    run_gen_st(&example_path("assembly_station.plc"), &out_st);

    let output = run_iec2c(&iec2c, &lib_dir, &out_st, &dir);
    assert!(
        output.status.success(),
        "iec2c compile failed for assembly_station.st:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        dir.join("POUS.c").exists(),
        "iec2c should produce POUS.c for assembly_station"
    );
    assert!(
        dir.join("POUS.h").exists(),
        "iec2c should produce POUS.h for assembly_station"
    );
}
