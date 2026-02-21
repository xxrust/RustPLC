use std::fs;
use std::path::Path;
use std::process::Command;

fn read_doc() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("docs")
        .join("no_board_playbook.md");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read doc failed: {e}"))
}

#[test]
fn no_board_playbook_mentions_required_commands_and_they_exist_in_cli_usage() {
    let doc = read_doc();

    for needle in [
        "No-RTOS Real-Time Playbook",
        "compile/verify",
        "virtual-board",
        "timing-report",
        "no-board-gate",
        "release-bundle",
    ] {
        assert!(doc.contains(needle), "doc should mention `{needle}`");
    }

    // CLI usage is printed to stderr.
    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .output()
        .expect("run rust_plc without args");
    let stderr = String::from_utf8_lossy(&output.stderr);

    for cmd in [
        "virtual-board",
        "timing-report",
        "no-board-gate",
        "release-bundle",
    ] {
        assert!(
            stderr.contains(cmd),
            "CLI usage should include `{cmd}`; got: {stderr}"
        );
    }
}
