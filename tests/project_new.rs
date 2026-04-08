use std::fs;
use std::path::{Path, PathBuf};
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

fn project_path(base: &Path, name: &str) -> PathBuf {
    base.join(name)
}

#[test]
fn new_single_file_layout_still_creates_main_plc() {
    let base = temp_dir("rust_plc_new_single_file");
    let project_dir = project_path(&base, "demo_single");

    let output = run_cli(&["new", project_dir.to_str().expect("utf8 path")]);
    assert!(
        output.status.success(),
        "new single-file should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(project_dir.join("plc/main.plc").exists());
    assert!(!project_dir.join("plc/main.target_semantics.bundle.toml").exists());

    let manifest = fs::read_to_string(project_dir.join("rustplc.project.toml"))
        .expect("read manifest");
    assert!(manifest.contains("plc = \"plc/main.plc\""));
    assert!(project_dir.join("config/workpiece.toml").exists());
}

#[test]
fn new_structured_fragments_layout_creates_bundle_project_that_compiles() {
    let base = temp_dir("rust_plc_new_structured");
    let project_dir = project_path(&base, "demo_structured");

    let output = run_cli(&[
        "new",
        project_dir.to_str().expect("utf8 path"),
        "--layout",
        "structured-fragments",
    ]);
    assert!(
        output.status.success(),
        "new structured-fragments should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bundle_path = project_dir.join("plc/main.target_semantics.bundle.toml");
    assert!(bundle_path.exists(), "bundle entry should exist");
    assert!(
        project_dir
            .join("plc/target_semantics_fragments/architecture/startup_and_supervision.plcfrag")
            .exists(),
        "architecture fragment should exist"
    );
    assert!(
        project_dir
            .join("plc/target_semantics_fragments/manual/manual_actions.plcfrag")
            .exists(),
        "manual placeholder fragment should exist"
    );
    assert!(
        project_dir
            .join("plc/target_semantics_fragments/operator_interface/commands_indicators_alarms.plcfrag")
            .exists(),
        "operator interface sidecar should exist"
    );
    assert!(
        project_dir
            .join("plc/target_semantics_fragments/io/aliases.plcfrag")
            .exists(),
        "io alias sidecar should exist"
    );
    assert!(
        project_dir
            .join("plc/target_semantics_fragments/optimization/candidate_evaluation.plcfrag")
            .exists(),
        "optimization sidecar should exist"
    );
    assert!(
        project_dir
            .join("plc/target_semantics_fragments/step/step_cycles.plcfrag")
            .exists(),
        "step-mode sidecar should exist"
    );

    let manifest = fs::read_to_string(project_dir.join("rustplc.project.toml"))
        .expect("read manifest");
    assert!(manifest.contains("plc = \"plc/main.target_semantics.bundle.toml\""));
    assert!(manifest.contains("workpiece = \"config/workpiece.toml\""));
    assert!(project_dir.join("config/workpiece.toml").exists());

    let bundle = fs::read_to_string(&bundle_path).expect("read bundle");
    assert!(!bundle.contains("manual/manual_actions.plcfrag"));
    assert!(!bundle.contains("operator_interface/commands_indicators_alarms.plcfrag"));
    assert!(!bundle.contains("optimization/candidate_evaluation.plcfrag"));
    assert!(!bundle.contains("step/step_cycles.plcfrag"));

    let compile = run_cli(&[
        bundle_path.to_str().expect("utf8 bundle path"),
        "--no-print-ir",
    ]);
    assert!(
        compile.status.success(),
        "structured bundle scaffold should compile, stderr: {}",
        String::from_utf8_lossy(&compile.stderr)
    );
}

#[test]
fn new_rejects_unknown_layout_value() {
    let base = temp_dir("rust_plc_new_bad_layout");
    let project_dir = project_path(&base, "demo_bad_layout");

    let output = run_cli(&[
        "new",
        project_dir.to_str().expect("utf8 path"),
        "--layout",
        "spaghetti",
    ]);
    assert!(
        !output.status.success(),
        "new should reject an unknown layout"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Unknown layout `spaghetti`"));
    assert!(stderr.contains("single-file"));
    assert!(stderr.contains("structured-fragments"));
}
