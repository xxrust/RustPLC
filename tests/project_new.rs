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
    assert!(!project_dir.join("rustplc.bundle.toml").exists());

    let manifest =
        fs::read_to_string(project_dir.join("rustplc.project.toml")).expect("read manifest");
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

    let bundle_path = project_dir.join("rustplc.bundle.toml");
    assert!(bundle_path.exists(), "bundle entry should exist");

    assert!(
        project_dir.join("00_topology/controller.plc").exists(),
        "topology controller should exist"
    );
    assert!(
        project_dir.join("00_topology/devices.plc").exists(),
        "topology devices should exist"
    );
    assert!(
        project_dir.join("00_topology/workpieces.plc").exists(),
        "topology workpieces should exist"
    );
    assert!(
        project_dir.join("00_topology/connections.plc").exists(),
        "topology connections should exist"
    );
    let station_protocol = fs::read_to_string(project_dir.join("00_topology/_station_protocol.plc"))
        .expect("read station protocol placeholder");
    assert!(station_protocol.contains("supported by the compiler"));
    assert!(station_protocol.contains("tasks: [st01_cycle]"));
    assert!(!station_protocol.contains("future DSL"));
    assert!(!station_protocol.contains("not yet supported"));
    assert!(
        project_dir.join("01_init/defaults.plc").exists(),
        "init defaults should exist"
    );
    assert!(
        project_dir.join("02_process/main_cycle.plc").exists(),
        "process main_cycle should exist"
    );
    assert!(
        project_dir.join("03_constraints/_placeholder.plc").exists(),
        "constraints placeholder should exist"
    );
    assert!(
        project_dir.join("04_faults/fault_handlers.plc").exists(),
        "fault handlers should exist"
    );
    assert!(
        project_dir.join("05_supervision/_placeholder.plc").exists(),
        "supervision placeholder should exist"
    );
    assert!(
        project_dir.join("06_manual/_placeholder.plc").exists(),
        "manual placeholder should exist"
    );
    assert!(
        project_dir.join("07_hmi/_placeholder.plc").exists(),
        "hmi placeholder should exist"
    );

    let manifest =
        fs::read_to_string(project_dir.join("rustplc.project.toml")).expect("read manifest");
    assert!(manifest.contains("layer = \"station\""));
    assert!(manifest.contains("plc = \"rustplc.bundle.toml\""));
    assert!(manifest.contains("scenario = \"scenarios/nominal/normal.yaml\""));
    assert!(manifest.contains("workpiece = \"config/workpiece.toml\""));
    assert!(project_dir.join("config/workpiece.toml").exists());

    assert!(
        project_dir.join("docs/system.md").exists(),
        "system doc should exist"
    );
    assert!(
        project_dir.join("docs/architecture.md").exists(),
        "architecture doc should exist"
    );
    assert!(
        project_dir.join("docs/verification.md").exists(),
        "verification doc should exist"
    );

    let bundle = fs::read_to_string(&bundle_path).expect("read bundle");
    assert!(bundle.contains("schema_version = 2"));
    assert!(bundle.contains("[phases.00_topology]"));
    assert!(bundle.contains("enabled = false"));

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
fn new_structured_fragments_module_delivery_layer_sets_manifest_metadata() {
    let base = temp_dir("rust_plc_new_structured_module");
    let project_dir = project_path(&base, "pick_head");

    let output = run_cli(&[
        "new",
        project_dir.to_str().expect("utf8 path"),
        "--layout",
        "structured-fragments",
        "--delivery-layer",
        "module",
    ]);
    assert!(
        output.status.success(),
        "new structured-fragments module should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let manifest =
        fs::read_to_string(project_dir.join("rustplc.project.toml")).expect("read manifest");
    assert!(manifest.contains("layer = \"module\""));
    assert!(manifest.contains("plc = \"rustplc.bundle.toml\""));

    assert!(
        project_dir.join("00_topology/controller.plc").exists(),
        "topology should exist regardless of delivery layer"
    );
    assert!(
        project_dir.join("docs/architecture.md").exists(),
        "docs should be at project root"
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
