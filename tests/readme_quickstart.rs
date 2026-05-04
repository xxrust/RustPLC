use std::path::Path;
use std::process::Command;

fn repo_path(p: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(p)
}

#[test]
fn readme_compile_quickstart_command_succeeds() {
    let plc_path = repo_path("examples/project_scaffold_demo/plc/main.plc");
    assert!(plc_path.exists(), "expected README example PLC to exist");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg(&plc_path)
        .arg("--no-print-ir")
        .output()
        .expect("run README quickstart compile command");

    assert!(
        output.status.success(),
        "README compile quickstart should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
