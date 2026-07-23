use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo_path(p: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(p)
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

fn run_cli(args: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .args(args)
        .output()
        .expect("run rust_plc")
}

#[test]
fn compile_cache_misses_then_hits_same_source_and_devices() {
    let base = temp_dir("rust_plc_compile_cache");
    let cache_dir = base.join("cache");
    let report_a = base.join("report_a.json");
    let report_b = base.join("report_b.json");
    let plc = repo_path("examples/demo.plc");

    let first = run_cli(&[
        plc.to_string_lossy().into_owned(),
        "--cache-dir".to_string(),
        cache_dir.to_string_lossy().into_owned(),
        "--report".to_string(),
        report_a.to_string_lossy().into_owned(),
        "--no-print-ir".to_string(),
    ]);
    assert!(
        first.status.success(),
        "first compile should pass, stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_stderr = String::from_utf8_lossy(&first.stderr);
    assert!(
        first_stderr.contains("ir_cache: miss"),
        "first compile should miss cache, stderr: {first_stderr}"
    );
    assert!(
        first_stderr.contains("ir_cache: stored"),
        "first compile should store cache, stderr: {first_stderr}"
    );
    assert!(report_a.exists(), "first compile should write report");

    let cache_entries = fs::read_dir(&cache_dir)
        .expect("read cache dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
        .count();
    assert_eq!(cache_entries, 1, "one cache entry should be created");

    let second = run_cli(&[
        plc.to_string_lossy().into_owned(),
        "--cache-dir".to_string(),
        cache_dir.to_string_lossy().into_owned(),
        "--report".to_string(),
        report_b.to_string_lossy().into_owned(),
        "--no-print-ir".to_string(),
    ]);
    assert!(
        second.status.success(),
        "second compile should pass, stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    let second_stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        second_stderr.contains("ir_cache: hit"),
        "second compile should hit cache, stderr: {second_stderr}"
    );
    assert!(
        report_b.exists(),
        "cached compile should still write report"
    );
}

#[test]
fn compile_cache_invalidates_when_bundle_manifest_contract_changes() {
    let base = temp_dir("rust_plc_compile_cache_bundle_manifest");
    let cache_dir = base.join("cache");
    let project = base.join("project");
    fs::create_dir_all(project.join("00_topology")).expect("create topology dir");
    fs::create_dir_all(project.join("01_tasks")).expect("create tasks dir");

    fs::write(
        project.join("00_topology").join("controller.plc"),
        "device plc_main: plc { purpose: \"bundle cache controller\", model_ref: openplc_softplc }\n",
    )
    .expect("write topology fragment");
    fs::write(
        project.join("01_tasks").join("main.plc"),
        "task main:\n    step idle:\n        action: log \"ok\"\n",
    )
    .expect("write task fragment");

    let bundle_path = project.join("rustplc.bundle.toml");
    fs::write(
        &bundle_path,
        "schema_version = 2\n\
         [phases.00_topology]\n\
         path = \"00_topology\"\n\
         section = \"topology\"\n\
         exports = [\"plc\"]\n\
         [phases.01_tasks]\n\
         path = \"01_tasks\"\n\
         section = \"tasks\"\n",
    )
    .expect("write initial bundle");

    let first = run_cli(&[
        bundle_path.to_string_lossy().into_owned(),
        "--cache-dir".to_string(),
        cache_dir.to_string_lossy().into_owned(),
        "--no-print-ir".to_string(),
    ]);
    assert!(
        first.status.success(),
        "first bundle compile should pass, stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_stderr = String::from_utf8_lossy(&first.stderr);
    assert!(
        first_stderr.contains("ir_cache: miss"),
        "first compile should miss cache, stderr: {first_stderr}"
    );

    fs::write(
        &bundle_path,
        "schema_version = 2\n\
         [phases.00_topology]\n\
         path = \"00_topology\"\n\
         section = \"topology\"\n\
         exports = [\"plc\"]\n\
         [phases.01_tasks]\n\
         path = \"01_tasks\"\n\
         section = \"tasks\"\n\
         depends_on = [\"00_topology\"]\n",
    )
    .expect("rewrite bundle with dependency contract");

    let second = run_cli(&[
        bundle_path.to_string_lossy().into_owned(),
        "--cache-dir".to_string(),
        cache_dir.to_string_lossy().into_owned(),
        "--no-print-ir".to_string(),
    ]);
    assert!(
        second.status.success(),
        "second bundle compile should pass, stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    let second_stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        second_stderr.contains("ir_cache: miss"),
        "manifest-only dependency contract change should invalidate cache, stderr: {second_stderr}"
    );

    let cache_entries = fs::read_dir(&cache_dir)
        .expect("read cache dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
        .count();
    assert_eq!(
        cache_entries, 2,
        "manifest-only contract change should create a distinct cache entry"
    );
}

#[test]
fn compile_cache_reuses_unchanged_verification_checkers_on_timing_only_change() {
    let base = temp_dir("rust_plc_compile_cache_incremental_verification");
    let cache_dir = base.join("cache");
    let plc = base.join("process_device_demo.plc");
    fs::copy(repo_path("examples/process_device_demo.plc"), &plc).expect("copy fixture plc");

    let first = run_cli(&[
        plc.to_string_lossy().into_owned(),
        "--cache-dir".to_string(),
        cache_dir.to_string_lossy().into_owned(),
        "--no-print-ir".to_string(),
    ]);
    assert!(
        first.status.success(),
        "first compile should pass, stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    let source = fs::read_to_string(&plc).expect("read copied plc");
    fs::write(&plc, source.replace("500ms", "600ms")).expect("rewrite timing budget");

    let second = run_cli(&[
        plc.to_string_lossy().into_owned(),
        "--cache-dir".to_string(),
        cache_dir.to_string_lossy().into_owned(),
        "--no-print-ir".to_string(),
    ]);
    assert!(
        second.status.success(),
        "second compile should pass, stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    let second_stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        second_stderr.contains("ir_cache: miss"),
        "source change should miss exact IR cache, stderr: {second_stderr}"
    );
    assert!(
        second_stderr.contains("verification_cache: candidate"),
        "changed compile should inspect previous verification cache, stderr: {second_stderr}"
    );
    assert!(
        second_stderr
            .contains("verification_cache: reused [safety,liveness,causality,station_protocol], checked [timing]"),
        "timing-only change should reuse unaffected checkers, stderr: {second_stderr}"
    );
}
