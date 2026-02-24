use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn read_doc() -> String {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        base.join("docs").join("commissioning_playbook.md"),
        base.join("docs").join("已实现").join("commissioning_playbook.md"),
    ];
    read_first_existing_doc(&candidates)
}

fn read_first_existing_doc(candidates: &[PathBuf]) -> String {
    for path in candidates {
        if path.exists() {
            return fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("read doc failed {}: {e}", path.display()));
        }
    }
    panic!(
        "read doc failed: none of the expected files exist: {}",
        candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
}

#[test]
fn commissioning_playbook_locks_required_headings_and_command_snippets() {
    let doc = read_doc();

    for needle in [
        "Commissioning Playbook",
        "Flow A: Nominal startup rehearsal",
        "Flow B: Fault-injection debug rehearsal",
        "Pass/Fail checkpoint",
        "commissioning-run",
        "commissioning_index.json",
        "scenario-doctor",
        "sim-plc",
        "--retain-config",
        "--online-force-script",
        "--online-var-script",
        "--online-var-bindings",
        "no-board-gate",
        "out/commissioning/",
    ] {
        assert!(doc.contains(needle), "doc should mention `{needle}`");
    }

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .output()
        .expect("run rust_plc without args");
    let stderr = String::from_utf8_lossy(&output.stderr);
    for cmd in [
        "commissioning-run",
        "scenario-doctor",
        "sim-plc",
        "no-board-gate",
    ] {
        assert!(
            stderr.contains(cmd),
            "CLI usage should include `{cmd}`; got: {stderr}"
        );
    }
}
