use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn assert_keyence_relay_bits_are_valid(mnm: &str) {
    for token in mnm
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| token.len() >= 3 && token.starts_with('R'))
    {
        let digits = &token[1..];
        if !digits.chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }
        let bit = if digits.len() <= 2 {
            digits.parse::<usize>().expect("relay bit digits parse")
        } else {
            digits[digits.len() - 2..]
                .parse::<usize>()
                .expect("relay bit digits parse")
        };
        assert!(
            bit < 16,
            "KEYENCE relay address `{token}` uses invalid bit position {bit}"
        );
    }
}

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
fn flowchart_artifact_exposes_station_protocol_summary() {
    let base = temp_dir("rust_plc_flowchart_station_protocol");
    let plc = base.join("station_protocol.plc");
    let out_dir = base.join("flowchart_out");
    write(
        &plc,
        r#"
[topology]
device plc_a: plc { purpose: "load station controller", model_ref: openplc_softplc }
device plc_b: plc { purpose: "press station controller", model_ref: openplc_softplc }
device cyl_load: cylinder { purpose: "load station actuator" }
device cyl_press: cylinder { purpose: "press station actuator" }
workpiece part: workpiece_type {
    ingress_sites: [handoff]
}
site handoff: workpiece_location { capacity: 1 }

station st01 { owns: [plc_a, cyl_load], tasks: [load_cycle] }
station st02 { owns: [plc_b, cyl_press], tasks: [press_cycle] }
handshake st01_to_st02 {
    from: st01,
    to: st02,
    request: st01_request,
    allow: st02_allow,
    complete: st01_complete,
    timeout: 5s -> goto fault.timeout
}
transfer_point load_to_press {
    from_station: st01,
    to_station: st02,
    site: handoff,
    handshake: st01_to_st02
}

[constraints]

[tasks]
task load_cycle:
    step idle:
task press_cycle:
    step idle:
task fault:
    step timeout:
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
        "flowchart station protocol command should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let html = fs::read_to_string(out_dir.join("index.html")).expect("html artifact");
    assert!(html.contains("Station Protocol"));
    assert!(html.contains("st01_to_st02"));
    assert!(html.contains("load_to_press"));

    let json = fs::read_to_string(out_dir.join("flowchart.json")).expect("json artifact");
    assert!(json.contains("\"station_count\": 2"));
    assert!(json.contains("\"handshake_count\": 1"));
    assert!(json.contains("\"transfer_point_count\": 1"));
    assert!(json.contains("st01_to_st02"));
    assert!(json.contains("load_to_press"));
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
        allow_indefinite_wait: true

    step lamp_on:
        action: compute run_latched = true

    step finish:
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
    assert!(mnm.contains("mnm_subset_unverified_requires_kv_studio_roundtrip_and_compile"));
    assert!(mnm.contains("DEVICE:60"));
    assert!(mnm.contains(";MODULE_TYPE:0"));
    assert!(mnm.contains("LD R900"));
    assert!(mnm.contains("AND R000"));
    assert!(mnm.contains("SET R2001"));
    assert!(mnm.contains("LD R2001"));
    assert!(mnm.contains("SET R901"));
    assert!(mnm.contains("RES R2001"));
    assert!(mnm.contains("SET R500"));
    assert!(mnm.contains("END\nENDH"));
    assert_keyence_relay_bits_are_valid(&mnm);
    let vars = fs::read_to_string(out_dir.join("variables/variables.csv")).expect("variables");
    assert!(vars.contains("start_button"));
    assert!(vars.contains("start_button,BOOL,R000"));
    assert!(vars.contains("run_latched"));
    assert!(vars.contains("run_latched,BOOL,R500"));
    let fb = fs::read_to_string(out_dir.join("fb/fb_manifest.md")).expect("fb manifest");
    assert!(fb.contains("Official FBs Imported Directly"));
    let report =
        fs::read_to_string(out_dir.join("validation_report.md")).expect("validation report");
    assert!(report.contains("has not been imported into KV STUDIO"));
}

#[test]
fn gen_keyence_falls_back_to_review_package_for_unsupported_subset() {
    let base = temp_dir("rust_plc_gen_keyence_blocked_cli");
    let plc = base.join("timer.plc");
    let out_dir = base.join("keyence_out");
    write(
        &plc,
        r#"
[topology]
variable done: bool = false

[constraints]

[tasks]
task main:
    step delay_first:
        delay: 10ms

    step finish:
        action: compute done = true
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
        "gen-keyence command should succeed with conservative fallback, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let mnm = fs::read_to_string(out_dir.join("mnm/Main.mnm")).expect("mnm draft");
    assert!(mnm.contains("draft_unverified_requires_kv_studio_import_and_compile"));
    assert!(mnm.contains("ST reference body follows"));
    assert!(mnm.contains("unsupported guard for KEYENCE executable MNM subset"));
    assert!(!mnm.contains("DEVICE:60\n;MODULE:Main\n;MODULE_TYPE:0\nLD R900"));
    let report =
        fs::read_to_string(out_dir.join("validation_report.md")).expect("validation report");
    assert!(report.contains("KEYENCE Validation Report"));
    assert!(report.contains("unsupported guard for KEYENCE executable MNM subset"));
}
