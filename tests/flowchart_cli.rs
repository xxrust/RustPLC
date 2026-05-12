use std::fs;
use std::path::{Path, PathBuf};
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

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, content).expect("write file");
}

fn run_cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .args(args)
        .output()
        .expect("run rust_plc")
}

#[test]
fn flowchart_command_generates_html_and_json_artifacts() {
    let base = temp_dir("rust_plc_flowchart_cli");
    let plc = base.join("minimal.plc");
    let out_dir = base.join("flowchart_out");
    write(
        &plc,
        r#"
[topology]
variable start_button: bool = false
variable run_latched: bool = false
variable fault_latched: bool = false

[constraints]

[tasks]
task main:
    step wait_start:
        wait: start_button == true
        timeout: 10ms -> goto fault.handle

    step lamp_on:
        action: compute run_latched = true

    on_complete: goto fault.handle

task fault:
    step handle:
        action: compute fault_latched = true
"#,
    );

    let output = run_cli(&[
        "flowchart",
        plc.to_str().expect("utf8 path"),
        "--out-dir",
        out_dir.to_str().expect("utf8 path"),
        "--output",
        "json",
    ]);
    assert!(
        output.status.success(),
        "flowchart command should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let html = fs::read_to_string(out_dir.join("index.html")).expect("html artifact");
    assert!(html.contains("<title>"));
    assert!(html.contains("<svg"));
    assert!(html.contains("atlas-canvas"));
    assert!(html.contains("journey-track"));
    assert!(html.contains("detail-sfc-host"));
    assert!(html.contains("flowchart-model"));
    assert!(html.contains("task-templates"));
    assert!(html.contains("System Atlas"));
    assert!(html.contains("Journey Reel"));
    assert!(html.contains("Task Theater"));
    assert!(html.contains("task-sfc-svg"));
    assert!(html.contains("class=\"task-sfc-svg\" width=\""));
    assert!(!html.contains(".task-sfc-svg { display: block; width: 100%;"));
    assert!(!html.contains("action: compute run_latched = true"));
    assert!(html.contains("goto fault.handle"));
    assert!(html.contains("task-transition-branch-bus"));
    assert!(!html.contains("cdn.jsdelivr.net"));
    assert!(!html.contains("mermaid"));
    assert!(!html.contains("mermaid.min.js"));
    assert!(html.contains("main"));

    let json = fs::read_to_string(out_dir.join("flowchart.json")).expect("json artifact");
    assert!(json.contains("\"tasks\""));
    assert!(json.contains("\"main\""));
    assert!(json.contains("\"fault\""));
    assert!(!json.contains("mermaid"));
}

#[test]
fn flowchart_help_exposes_the_command() {
    let output = run_cli(&["help", "flowchart"]);
    assert!(
        output.status.success(),
        "help should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rendered = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(rendered.contains("flowchart"));
    assert!(rendered.contains("out-dir"));
}
