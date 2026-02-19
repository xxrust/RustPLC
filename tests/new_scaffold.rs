use std::fs;
use std::path::PathBuf;
use std::process::Command;

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

#[test]
fn rust_plc_new_generates_bootstrap_project_and_quick_checks_pass() {
    let base = temp_dir("rust_plc_new_scaffold");
    let project_dir = base.join("demo_project");

    let create = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("new")
        .arg(&project_dir)
        .output()
        .expect("run rust_plc new");
    assert!(
        create.status.success(),
        "rust_plc new should succeed, stderr: {}",
        String::from_utf8_lossy(&create.stderr)
    );

    for rel in [
        "README.md",
        "plc/main.plc",
        "scenarios/normal.yaml",
        "io_map.toml",
        ".github/workflows/no_board_gate.yml",
        ".vscode/tasks.json",
        ".vscode/settings.json",
        ".vscode/extensions.json",
    ] {
        assert!(
            project_dir.join(rel).exists(),
            "expected generated file to exist: {rel}"
        );
    }

    let tasks_json =
        fs::read_to_string(project_dir.join(".vscode/tasks.json")).expect("read tasks.json");
    assert!(
        tasks_json.contains("RustPLC: scenario-validate")
            && tasks_json.contains("RustPLC: no-board-gate"),
        "tasks.json should contain quick command entries"
    );

    let validate = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("scenario-validate")
        .arg(project_dir.join("plc/main.plc"))
        .arg("--scenario")
        .arg(project_dir.join("scenarios/normal.yaml"))
        .arg("--output")
        .arg("json")
        .output()
        .expect("run scenario-validate");
    assert!(
        validate.status.success(),
        "generated scenario-validate should pass, stderr: {}",
        String::from_utf8_lossy(&validate.stderr)
    );

    let gate = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("no-board-gate")
        .arg(project_dir.join("plc/main.plc"))
        .arg("--scenario")
        .arg(project_dir.join("scenarios/normal.yaml"))
        .arg("--out-dir")
        .arg(project_dir.join("out/no_board_gate"))
        .arg("--output")
        .arg("json")
        .output()
        .expect("run no-board-gate");
    assert!(
        gate.status.success(),
        "generated no-board-gate should pass, stderr: {}",
        String::from_utf8_lossy(&gate.stderr)
    );
}
