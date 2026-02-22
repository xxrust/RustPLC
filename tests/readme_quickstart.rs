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
fn readme_sim_plc_quickstart_command_succeeds() {
    let plc_path = repo_path("examples/assembly_station.plc");
    let scenario_path = repo_path("scenarios/normal.yaml");
    assert!(plc_path.exists(), "expected example PLC to exist");
    assert!(
        scenario_path.exists(),
        "expected README scenario to exist at scenarios/normal.yaml"
    );

    let base = temp_dir("rust_plc_readme_quickstart");
    let out_trace = base.join("trace.jsonl");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("sim-plc")
        .arg(&plc_path)
        .arg("--scenario")
        .arg(&scenario_path)
        .arg("--out")
        .arg(&out_trace)
        .output()
        .expect("run sim-plc");

    assert!(
        output.status.success(),
        "sim-plc should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let trace = fs::read_to_string(&out_trace).expect("read trace");
    assert!(!trace.trim().is_empty(), "trace should be non-empty");
}
