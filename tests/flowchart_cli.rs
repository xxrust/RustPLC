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
    assert!(html.contains("Review Cockpit"));
    assert!(html.contains("Review Counts"));
    assert!(html.contains("Task Inventory"));
    assert!(html.contains("System Contract"));
    assert!(html.contains("atlas-node-name"));
    assert!(html.contains("atlas-journey-hit"));
    assert!(html.contains("parallel station occupancy map"));
    assert!(html.contains("control projection"));
    assert!(html.contains("pipeline-wave"));
    assert!(html.contains("material-token-palette"));
    assert!(html.contains("cycle color key"));
    assert!(html.contains("steady-state pipeline wave"));
    assert!(html.contains("not a single-wafer trace"));
    assert!(html.contains("Effect Reel"));
    assert!(html.contains("Task Theater"));
    assert!(html.contains("SFC keeps step identity and transitions only"));
    assert!(html.contains("task-sfc-svg"));
    assert!(html.contains("class=\"task-sfc-svg\" width=\""));
    assert!(html.contains("integrated review cockpit"));
    assert!(!html.contains("ts_tailwind_review_app"));
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
    assert!(json.contains("\"devices\""));
    assert!(!json.contains("mermaid"));
    assert!(!out_dir.join("ts_tailwind_review_app").exists());
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

#[test]
fn gen_keyence_emits_review_package_without_claiming_compile() {
    let base = temp_dir("rust_plc_gen_keyence_cli");
    let plc = base.join("minimal.plc");
    let out_dir = base.join("keyence_out");
    write(
        &plc,
        r#"
[topology]
variable start_button: bool = false
variable run_latched: bool = false

[constraints]

[tasks]
task main:
    step wait_start:
        wait: start_button == true
        timeout: 10ms -> goto main.lamp_on

    step lamp_on:
        action: compute run_latched = true
"#,
    );

    let output = run_cli(&[
        "gen-keyence",
        plc.to_str().expect("utf8 path"),
        "--out-dir",
        out_dir.to_str().expect("utf8 path"),
        "--output",
        "json",
    ]);
    assert!(
        output.status.success(),
        "gen-keyence command should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let mnm = fs::read_to_string(out_dir.join("mnm/Main.mnm")).expect("mnm draft");
    assert!(mnm.contains("draft_unverified_requires_kv_studio_import_and_compile"));
    let vars = fs::read_to_string(out_dir.join("variables/variables.csv")).expect("variables");
    assert!(vars.contains("start_button"));
    assert!(vars.contains("run_latched"));
    let fb = fs::read_to_string(out_dir.join("fb/fb_manifest.md")).expect("fb manifest");
    assert!(fb.contains("Official FBs Imported Directly"));
    let report =
        fs::read_to_string(out_dir.join("validation_report.md")).expect("validation report");
    assert!(report.contains("has not been imported into KV STUDIO"));
}
