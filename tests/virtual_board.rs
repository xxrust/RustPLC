use std::fs;
use std::process::Command;

fn temp_dir(prefix: &str) -> std::path::PathBuf {
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
fn virtual_board_emits_board_log_and_trace_artifacts() {
    let base = temp_dir("rust_plc_virtual_board");
    let plc_path = base.join("fixture.plc");
    let scenario_path = base.join("scenario.yaml");
    let out_dir = base.join("out");

    let plc = r#"
[topology]
device X0: digital_input
device Y0: digital_output

[constraints]

[tasks]
task main:
    step wait_start:
        wait: X0 == true
        timeout: 20ms -> goto done
    step run:
        action: set Y0 on

task done:
    step halt:
        action: log "done"
"#;
    fs::write(&plc_path, plc).expect("write plc");

    let scenario = r#"
tick_ms: 10
duration_ms: 40
inputs:
  - at_ms: 10
    set:
      digital_inputs:
        0: true
"#;
    fs::write(&scenario_path, scenario).expect("write scenario");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("virtual-board")
        .arg(&plc_path)
        .arg("--scenario")
        .arg(&scenario_path)
        .arg("--out-dir")
        .arg(&out_dir)
        .output()
        .expect("run virtual-board");

    assert!(
        output.status.success(),
        "virtual-board should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let board_log = out_dir.join("board.log");
    let board_trace = out_dir.join("board_trace.jsonl");
    let meta = out_dir.join("virtual_board_meta.json");
    assert!(board_log.exists(), "board.log should exist");
    assert!(board_trace.exists(), "board_trace.jsonl should exist");
    assert!(meta.exists(), "virtual_board_meta.json should exist");

    let log_text = fs::read_to_string(&board_log).expect("read board.log");
    assert!(log_text.contains("TRACE "), "board log should contain TRACE lines");

    let trace_text = fs::read_to_string(&board_trace).expect("read board trace");
    assert!(
        !trace_text.trim().is_empty(),
        "board trace jsonl should contain at least one row"
    );

    let parsed_out = out_dir.join("parsed_trace.jsonl");
    let parse_output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("trace-parse")
        .arg("--in")
        .arg(&board_log)
        .arg("--out")
        .arg(&parsed_out)
        .output()
        .expect("run trace-parse");
    assert!(
        parse_output.status.success(),
        "trace-parse should succeed on virtual board logs, stderr: {}",
        String::from_utf8_lossy(&parse_output.stderr)
    );
    assert!(parsed_out.exists(), "trace-parse output should exist");
}
