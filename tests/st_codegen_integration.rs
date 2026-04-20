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
    run_gen_st_with_extra_args(plc, out_st, &[]);
}

fn run_gen_st_with_extra_args(plc: &Path, out_st: &Path, extra_args: &[&str]) {
    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("gen-st")
        .arg(plc)
        .arg("--out")
        .arg(out_st)
        .args(extra_args)
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
fn st_codegen_project_scaffold_and_dual_axis_generate() {
    let dir = unique_temp_dir("st_codegen_generate");
    let scaffold_st = dir.join("project_scaffold_demo.st");
    let dual_axis_st = dir.join("dual_axis_platform.st");

    run_gen_st(&example_path("project_scaffold_demo/plc/main.plc"), &scaffold_st);
    run_gen_st(&example_path("dual_axis_platform.plc"), &dual_axis_st);

    let scaffold = fs::read_to_string(&scaffold_st).expect("should read generated st");
    let dual_axis = fs::read_to_string(&dual_axis_st).expect("should read generated st");

    assert!(scaffold.contains("PROGRAM Main"));
    assert!(scaffold.contains("CASE _state OF"));
    assert!(scaffold.contains("_timer_0(IN := _state = 0, PT := T#100ms);"));
    assert!(scaffold.contains("_state_trace_b13 AT %QX1.5 : BOOL;"));
    assert!(scaffold.contains("CONFIGURATION Config0"));
    assert!(scaffold.contains("TASK MainTask(INTERVAL := T#10ms, PRIORITY := 0);"));
    assert!(dual_axis.contains("PROGRAM Main"));
    assert!(dual_axis.contains("CASE _state OF"));
    assert!(dual_axis.contains("cycle.move_to_target__parallel_1_fork"));
    assert!(dual_axis.contains("CONFIGURATION Config0"));
}

#[test]
fn st_codegen_cli_allows_custom_task_interval() {
    let dir = unique_temp_dir("st_codegen_task_interval");
    let out_st = dir.join("dual_axis_platform_25ms.st");

    run_gen_st_with_extra_args(
        &example_path("dual_axis_platform.plc"),
        &out_st,
        &["--task-interval-ms", "25"],
    );

    let rendered = fs::read_to_string(&out_st).expect("should read generated st");
    assert!(rendered.contains("TASK MainTask(INTERVAL := T#25ms, PRIORITY := 0);"));
}

#[test]
fn st_codegen_timer_calls_appear_before_case() {
    let dir = unique_temp_dir("st_codegen_timer");
    let out_st = dir.join("project_scaffold_demo.st");

    run_gen_st(&example_path("project_scaffold_demo/plc/main.plc"), &out_st);
    let rendered = fs::read_to_string(&out_st).expect("should read generated st");

    let timer_pos = rendered
        .find("_timer_0(IN := _state = 0, PT := T#100ms);")
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
fn st_codegen_project_scaffold_compiles_with_matiec() {
    let Some((iec2c, lib_dir)) = find_iec2c() else {
        eprintln!("[SKIP] iec2c not available on this platform — skipping matiec round-trip");
        return;
    };

    let dir = unique_temp_dir("st_codegen_matiec_scaffold");
    let out_st = dir.join("project_scaffold_demo.st");
    run_gen_st(&example_path("project_scaffold_demo/plc/main.plc"), &out_st);

    let output = run_iec2c(&iec2c, &lib_dir, &out_st, &dir);
    assert!(
        output.status.success(),
        "iec2c compile failed for project_scaffold_demo.st:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify iec2c produced the expected C artifacts
    assert!(
        dir.join("POUS.c").exists(),
        "iec2c should produce POUS.c for project_scaffold_demo"
    );
    assert!(
        dir.join("POUS.h").exists(),
        "iec2c should produce POUS.h for project_scaffold_demo"
    );
    assert!(
        dir.join("Config0.c").exists(),
        "iec2c should produce Config0.c for project_scaffold_demo"
    );
    assert!(
        dir.join("Res0.c").exists(),
        "iec2c should produce Res0.c for project_scaffold_demo"
    );
}

#[test]
fn st_codegen_dual_axis_platform_compiles_with_matiec() {
    let Some((iec2c, lib_dir)) = find_iec2c() else {
        eprintln!("[SKIP] iec2c not available on this platform — skipping matiec round-trip");
        return;
    };

    let dir = unique_temp_dir("st_codegen_matiec_dual_axis");
    let out_st = dir.join("dual_axis_platform.st");
    run_gen_st(&example_path("dual_axis_platform.plc"), &out_st);

    let output = run_iec2c(&iec2c, &lib_dir, &out_st, &dir);
    assert!(
        output.status.success(),
        "iec2c compile failed for dual_axis_platform.st:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        dir.join("POUS.c").exists(),
        "iec2c should produce POUS.c for dual_axis_platform"
    );
    assert!(
        dir.join("POUS.h").exists(),
        "iec2c should produce POUS.h for dual_axis_platform"
    );
    assert!(
        dir.join("Config0.c").exists(),
        "iec2c should produce Config0.c for dual_axis_platform"
    );
    assert!(
        dir.join("Res0.c").exists(),
        "iec2c should produce Res0.c for dual_axis_platform"
    );
}
