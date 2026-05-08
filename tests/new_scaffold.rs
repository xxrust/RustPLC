use serde_json::Value;
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
        ".gitignore",
        "rustplc.project.toml",
        "plc/main.system.md",
        "plc/main.plc",
        "scenarios/nominal/normal.yaml",
        "scenarios/faults/.gitkeep",
        "scenarios/generated/.gitkeep",
        "config/io_map.toml",
        "config/retain.toml",
        "config/workpiece.toml",
        "config/state_proof.toml",
        "docs/project-layout.md",
        "out/ir/.gitkeep",
        "out/sim/.gitkeep",
        "out/gate/.gitkeep",
        "out/codegen/.gitkeep",
        "out/rp2040/.gitkeep",
        "out/release/.gitkeep",
        ".github/workflows/no_board_gate.yml",
        ".vscode/tasks.json",
        ".vscode/settings.json",
        ".vscode/extensions.json",
        ".vscode/plc.code-snippets",
        ".vscode/README.md",
    ] {
        assert!(
            project_dir.join(rel).exists(),
            "expected generated file to exist: {rel}"
        );
    }

    let tasks_json =
        fs::read_to_string(project_dir.join(".vscode/tasks.json")).expect("read tasks.json");
    assert!(
        tasks_json.contains("RustPLC: project-check")
            && tasks_json.contains("out/project_check/normal")
            && tasks_json.contains("RustPLC: sim-plc")
            && tasks_json.contains("RustPLC: no-board-gate"),
        "tasks.json should contain the scaffold quick command entries"
    );

    let settings_json =
        fs::read_to_string(project_dir.join(".vscode/settings.json")).expect("read settings.json");
    assert!(
        settings_json.contains("\"*.plc\"") && settings_json.contains("\"ini\""),
        "settings.json should define PLC file association"
    );

    let snippets = fs::read_to_string(project_dir.join(".vscode/plc.code-snippets"))
        .expect("read plc.code-snippets");
    assert!(
        snippets.contains("plc-skeleton") && snippets.contains("[topology]"),
        "plc.code-snippets should include PLC skeleton snippet"
    );

    let readme = fs::read_to_string(project_dir.join("README.md")).expect("read README.md");
    assert!(
        readme.contains("# Demo Project")
            && readme.contains("Project slug: `demo_project`")
            && readme.contains("project-check"),
        "README should include derived project name and slug"
    );

    let system_doc =
        fs::read_to_string(project_dir.join("plc/main.system.md")).expect("read main.system.md");
    assert!(
        system_doc.contains("# Demo Project System Description")
            && system_doc.contains("- Name: Demo Project")
            && system_doc.contains("`demo_project`"),
        "system doc should include derived project identity"
    );

    let manifest = fs::read_to_string(project_dir.join("rustplc.project.toml"))
        .expect("read rustplc.project.toml");
    assert!(
        manifest.contains("name = \"Demo Project\"")
            && manifest.contains("slug = \"demo_project\"")
            && manifest.contains("scenario = \"scenarios/nominal/normal.yaml\"")
            && manifest.contains("workpiece = \"config/workpiece.toml\"")
            && manifest.contains("codegen = \"out/codegen\""),
        "manifest should include project identity and default paths"
    );

    let self_check = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("project-check")
        .arg(project_dir.join("plc/main.plc"))
        .arg("--scenario")
        .arg(project_dir.join("scenarios/nominal/normal.yaml"))
        .arg("--out-dir")
        .arg(project_dir.join("out/project_check/normal"))
        .arg("--output")
        .arg("json")
        .output()
        .expect("run project-check");
    assert!(
        self_check.status.success(),
        "generated project-check should pass, stderr: {}",
        String::from_utf8_lossy(&self_check.stderr)
    );
    assert!(
        project_dir
            .join("out/project_check/normal/project_check_report.json")
            .exists(),
        "project-check should write the aggregated report"
    );
    let report_text =
        fs::read_to_string(project_dir.join("out/project_check/normal/project_check_report.json"))
            .expect("read project_check_report.json");
    let report: Value = serde_json::from_str(&report_text).expect("project-check report JSON");
    let steps = report
        .get("steps")
        .and_then(Value::as_array)
        .expect("steps array");
    assert!(
        steps.iter().any(|step| {
            step.get("name").and_then(Value::as_str) == Some("state_proof_check")
                && step.get("status").and_then(Value::as_str) == Some("pass")
        }),
        "generated project-check should include a passing state_proof_check step"
    );

    let validate = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("scenario-validate")
        .arg(project_dir.join("plc/main.plc"))
        .arg("--scenario")
        .arg(project_dir.join("scenarios/nominal/normal.yaml"))
        .arg("--output")
        .arg("json")
        .output()
        .expect("run scenario-validate");
    assert!(
        validate.status.success(),
        "generated scenario-validate should pass, stderr: {}",
        String::from_utf8_lossy(&validate.stderr)
    );

    let doctor = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("scenario-doctor")
        .arg(project_dir.join("plc/main.plc"))
        .arg("--scenario")
        .arg(project_dir.join("scenarios/nominal/normal.yaml"))
        .arg("--output")
        .arg("json")
        .output()
        .expect("run scenario-doctor");
    assert!(
        doctor.status.success(),
        "generated scenario-doctor should pass, stderr: {}",
        String::from_utf8_lossy(&doctor.stderr)
    );

    let gate = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("no-board-gate")
        .arg(project_dir.join("plc/main.plc"))
        .arg("--scenario")
        .arg(project_dir.join("scenarios/nominal/normal.yaml"))
        .arg("--out-dir")
        .arg(project_dir.join("out/gate/no_board/normal"))
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
