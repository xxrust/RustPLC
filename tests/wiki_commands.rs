use std::fs;
use std::path::Path;
use std::process::Command;

fn repo_path(p: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(p)
}

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
fn no_board_playbook_command_chain_succeeds() {
    let base = temp_dir("rust_plc_wiki_no_board");
    let plc = repo_path("examples/realtime_stress/stress_case.plc");
    let scenario = repo_path("examples/realtime_stress/scenarios/safe.yaml");

    let verify_report = base.join("verification_report.json");
    let vb_dir = base.join("virtual_board");
    let gate_dir = base.join("gate");
    let release_dir = base.join("release");

    let verify = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg(&plc)
        .arg("--report")
        .arg(&verify_report)
        .arg("--budget-max-time-estimate-us")
        .arg("2000")
        .output()
        .expect("run compile/verify");
    assert!(
        verify.status.success(),
        "compile/verify should pass, stderr: {}",
        String::from_utf8_lossy(&verify.stderr)
    );
    assert!(verify_report.exists());

    let vb = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("virtual-board")
        .arg(&plc)
        .arg("--scenario")
        .arg(&scenario)
        .arg("--out-dir")
        .arg(&vb_dir)
        .output()
        .expect("run virtual-board");
    assert!(
        vb.status.success(),
        "virtual-board should pass, stderr: {}",
        String::from_utf8_lossy(&vb.stderr)
    );

    let timing_report_path = vb_dir.join("timing_report.json");
    let tr = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("timing-report")
        .arg("--in")
        .arg(vb_dir.join("tick_timing.jsonl"))
        .arg("--out")
        .arg(&timing_report_path)
        .output()
        .expect("run timing-report");
    assert!(
        tr.status.success(),
        "timing-report should pass, stderr: {}",
        String::from_utf8_lossy(&tr.stderr)
    );
    assert!(timing_report_path.exists());

    let gate = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("no-board-gate")
        .arg(&plc)
        .arg("--scenario")
        .arg(&scenario)
        .arg("--out-dir")
        .arg(&gate_dir)
        .arg("--max-p99-exec-us")
        .arg("250")
        .arg("--max-overrun-count")
        .arg("0")
        .output()
        .expect("run no-board-gate");
    assert!(
        gate.status.success(),
        "no-board-gate should pass, stderr: {}",
        String::from_utf8_lossy(&gate.stderr)
    );

    let release = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("release-bundle")
        .arg(&plc)
        .arg("--scenario")
        .arg(&scenario)
        .arg("--out-dir")
        .arg(&release_dir)
        .arg("--max-p99-exec-us")
        .arg("250")
        .arg("--max-overrun-count")
        .arg("0")
        .output()
        .expect("run release-bundle");
    assert!(
        release.status.success(),
        "release-bundle should pass, stderr: {}",
        String::from_utf8_lossy(&release.stderr)
    );
    assert!(release_dir.join("manifest.json").exists());
}

#[test]
fn sim_pid_kpi_doc_command_succeeds() {
    let base = temp_dir("rust_plc_wiki_pid_kpi");
    let out_path = base.join("pid_kpi.json");
    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("sim-pid-kpi")
        .arg(repo_path("examples/pid_loop.plc"))
        .arg("--scenario")
        .arg(repo_path("examples/pid_kpi_scenario.yaml"))
        .arg("--out")
        .arg(&out_path)
        .output()
        .expect("run sim-pid-kpi");

    assert!(
        output.status.success(),
        "sim-pid-kpi should pass, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let kpi: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&out_path).expect("read KPI json"))
            .expect("KPI JSON");
    assert!(kpi.get("kpi").is_some());
}
