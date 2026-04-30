use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn temp_dir(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "{prefix}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn run_cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .args(args)
        .output()
        .expect("run rust_plc")
}

#[test]
fn root_help_lists_help_entry_and_command_sections() {
    let output = run_cli(&["--help"]);
    assert!(
        output.status.success(),
        "root --help should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Usage:"));
    assert!(stderr.contains("help [command]"));
    assert!(stderr.contains("Commands:"));
    assert!(stderr.contains("Core:"));
    assert!(stderr.contains("Simulation:"));
    assert!(stderr.contains("sim-plc"));
    assert!(stderr.contains("new"));
}

#[test]
fn help_subcommand_prints_target_command_usage() {
    let output = run_cli(&["help", "new"]);
    assert!(
        output.status.success(),
        "help new should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Usage:"));
    assert!(stderr.contains(
        "new <project_dir> [--layout <single-file|structured-fragments>] [--delivery-layer <module|station|line>] [--force]"
    ));
    assert!(stderr.contains("Create a RustPLC project scaffold"));
    assert!(stderr.contains("Options:"));
    assert!(stderr.contains("--layout <single-file|structured-fragments>"));
    assert!(stderr.contains("--delivery-layer <module|station|line>"));
    assert!(stderr.contains("--force"));
    assert!(stderr.contains("Notes:"));
    assert!(stderr.contains("structured-fragments"));
    assert!(stderr.contains("Examples:"));
    assert!(stderr.contains("rust_plc new demo_project"));
    assert!(stderr.contains("rust_plc new wafer_loader --layout structured-fragments"));
    assert!(
        stderr.contains(
            "rust_plc new pick_head --layout structured-fragments --delivery-layer module"
        )
    );
}

#[test]
fn new_help_does_not_create_literal_help_directory() {
    let cwd = temp_dir("rust_plc_new_help");
    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .current_dir(&cwd)
        .arg("new")
        .arg("--help")
        .output()
        .expect("run rust_plc new --help");

    assert!(
        output.status.success(),
        "new --help should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !cwd.join("--help").exists(),
        "new --help must not create a project directory named --help"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Usage:"));
    assert!(stderr.contains(
        "new <project_dir> [--layout <single-file|structured-fragments>] [--delivery-layer <module|station|line>] [--force]"
    ));
}

#[test]
fn positional_subcommand_help_short_circuits_before_reading_inputs() {
    let output = run_cli(&["scenario-validate", "--help"]);
    assert!(
        output.status.success(),
        "scenario-validate --help should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "scenario-validate <source.plc|source.bundle.toml> --scenario <scenario.yaml>"
        )
    );
    assert!(stderr.contains("Validate one scenario YAML against a PLC file."));
    assert!(stderr.contains("Options:"));
    assert!(stderr.contains("--scenario <scenario.yaml>"));
}

#[test]
fn compile_mode_help_short_circuits_before_touching_input_path() {
    let output = run_cli(&["missing_input.txt", "--help"]);
    assert!(
        output.status.success(),
        "compile-mode --help should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("<source.plc|source.bundle.toml>"));
    assert!(stderr.contains("Core options:"));
    assert!(stderr.contains("Examples:"));
    assert!(!stderr.contains("Failed to read"));
    assert!(!stderr.contains("Expected a .plc or .bundle.toml path"));
}

#[test]
fn unknown_token_help_follows_compile_fallback() {
    let output = run_cli(&["not-a-command", "--help"]);
    assert!(
        output.status.success(),
        "compile fallback --help should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("<source.plc|source.bundle.toml>"));
    assert!(stderr.contains("Core options:"));
    assert!(!stderr.contains("Unknown command: not-a-command"));
}

#[test]
fn help_subcommand_unknown_token_follows_compile_fallback() {
    let output = run_cli(&["help", "not-a-command"]);
    assert!(
        output.status.success(),
        "help unknown token should follow compile fallback, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("<source.plc|source.bundle.toml>"));
    assert!(stderr.contains("Core options:"));
    assert!(!stderr.contains("Unknown command: not-a-command"));
}

#[test]
fn detailed_help_for_sim_plc_includes_examples_and_notes() {
    let output = run_cli(&["help", "sim-plc"]);
    assert!(
        output.status.success(),
        "help sim-plc should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(
        "sim-plc <source.plc|source.bundle.toml> --scenario <scenario.yaml> --out <trace.jsonl>"
    ));
    assert!(stderr.contains("Options:"));
    assert!(stderr.contains("--enable-online-force-dev"));
    assert!(stderr.contains("Notes:"));
    assert!(stderr.contains("Online force and online variable controls"));
    assert!(stderr.contains("Examples:"));
    assert!(stderr.contains("rust_plc sim-plc examples/rp2040_motion_minimal.plc"));
}

#[test]
fn detailed_help_for_geometry_export_includes_overlay_options() {
    let output = run_cli(&["help", "geometry-export"]);
    assert!(
        output.status.success(),
        "help geometry-export should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("geometry-export <source.plc|source.bundle.toml> --out <geometry.json>")
    );
    assert!(stderr.contains("Options:"));
    assert!(stderr.contains("--trace <trace.jsonl>"));
    assert!(stderr.contains("--intent-report <report.json>"));
    assert!(stderr.contains("Notes:"));
    assert!(stderr.contains("stable JSON artifact"));
    assert!(stderr.contains("Examples:"));
    assert!(stderr.contains("rust_plc geometry-export examples/rp2040_motion_minimal.plc"));
}
