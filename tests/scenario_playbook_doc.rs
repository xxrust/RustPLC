use std::fs;
use std::path::Path;
use std::process::Command;

fn read_doc(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read doc failed {rel}: {e}"))
}

#[test]
fn scenario_playbook_mentions_required_commands_and_they_exist_in_cli_usage() {
    let doc = read_doc("docs/scenario_playbook.md");

    for needle in [
        "Scenario Playbook",
        "scenario-init",
        "scenario-validate",
        "scenario-expand",
        "sim-plc",
        "scenario-gen",
        "sim-regress",
        "--minimize-failure",
        "no-board-gate",
    ] {
        assert!(doc.contains(needle), "doc should mention `{needle}`");
    }

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .output()
        .expect("run rust_plc without args");
    let stderr = String::from_utf8_lossy(&output.stderr);
    for cmd in [
        "scenario-init",
        "scenario-validate",
        "scenario-expand",
        "scenario-gen",
        "sim-plc",
        "sim-regress",
        "no-board-gate",
    ] {
        assert!(
            stderr.contains(cmd),
            "CLI usage should include `{cmd}`; got: {stderr}"
        );
    }
}

