use std::fs;
use std::path::Path;
use std::process::Command;

fn read_doc(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read doc failed {rel}: {e}"))
}

#[test]
fn commissioning_playbook_locks_required_headings_and_command_snippets() {
    let doc = read_doc("docs/commissioning_playbook.md");

    for needle in [
        "Commissioning Playbook",
        "Flow A: Nominal startup rehearsal",
        "Flow B: Fault-injection debug rehearsal",
        "Pass/Fail checkpoint",
        "scenario-doctor",
        "sim-plc",
        "--retain-config",
        "--online-force-script",
        "--online-var-script",
        "no-board-gate",
        "out/commissioning/",
    ] {
        assert!(doc.contains(needle), "doc should mention `{needle}`");
    }

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .output()
        .expect("run rust_plc without args");
    let stderr = String::from_utf8_lossy(&output.stderr);
    for cmd in ["scenario-doctor", "sim-plc", "no-board-gate"] {
        assert!(
            stderr.contains(cmd),
            "CLI usage should include `{cmd}`; got: {stderr}"
        );
    }
}
