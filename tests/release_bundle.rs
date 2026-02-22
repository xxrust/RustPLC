use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::process::Command;

const PLC_FIXTURE: &str = r#"
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
"#;

const SCENARIO_FIXTURE: &str = r#"
tick_ms: 10
duration_ms: 40
inputs:
  - at_ms: 10
    set:
      digital_inputs:
        0: true
"#;

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

fn sha256_file(path: &Path) -> String {
    let bytes = fs::read(path).expect("read artifact bytes");
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

#[test]
fn release_bundle_emits_manifest_with_consistent_hashes() {
    let base = temp_dir("rust_plc_release_bundle");
    let plc_path = base.join("fixture.plc");
    let scenario_path = base.join("scenario.yaml");
    let out_dir = base.join("out");

    fs::write(&plc_path, PLC_FIXTURE).expect("write plc");
    fs::write(&scenario_path, SCENARIO_FIXTURE).expect("write scenario");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("release-bundle")
        .arg(&plc_path)
        .arg("--scenario")
        .arg(&scenario_path)
        .arg("--out-dir")
        .arg(&out_dir)
        .output()
        .expect("run release-bundle");

    assert!(
        output.status.success(),
        "release-bundle should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let manifest_path = out_dir.join("manifest.json");
    assert!(manifest_path.exists(), "manifest.json should exist");
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read manifest json"))
            .expect("manifest should be valid JSON");

    let artifacts = manifest
        .get("artifacts")
        .and_then(|v| v.as_array())
        .expect("artifacts array");
    assert!(
        !artifacts.is_empty(),
        "manifest should contain packaged artifacts"
    );

    let mut found_paths = std::collections::BTreeSet::new();
    for artifact in artifacts {
        let path = artifact
            .get("path")
            .and_then(|v| v.as_str())
            .expect("artifact.path");
        let sha = artifact
            .get("sha256")
            .and_then(|v| v.as_str())
            .expect("artifact.sha256");
        let size = artifact
            .get("size_bytes")
            .and_then(|v| v.as_u64())
            .expect("artifact.size_bytes");
        let abs = out_dir.join(path);
        assert!(abs.exists(), "artifact {} should exist on disk", path);
        assert_eq!(sha256_file(&abs), sha, "sha mismatch for {}", path);
        assert_eq!(
            fs::metadata(&abs).expect("artifact metadata").len(),
            size,
            "size mismatch for {}",
            path
        );
        found_paths.insert(path.to_string());
    }

    for required in [
        "program.plc",
        "scenario.yaml",
        "io_map.toml",
        "generated_program.rs",
        "io_map.template.toml",
        "sil_trace.jsonl",
        "sim_report.json",
        "tick_timing.jsonl",
        "timing_report.json",
        "gate_summary.json",
        "verification_report.json",
        "build_meta.json",
    ] {
        assert!(
            found_paths.contains(required),
            "manifest should include {required}"
        );
    }

    let build_meta: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(out_dir.join("build_meta.json")).expect("read build_meta"),
    )
    .expect("build_meta should be valid JSON");
    assert!(
        build_meta
            .get("generated_at")
            .and_then(|v| v.as_str())
            .is_some(),
        "build_meta should include generated_at"
    );
    assert!(
        build_meta
            .get("git_commit")
            .and_then(|v| v.as_str())
            .is_some(),
        "build_meta should include git_commit"
    );
    assert!(
        build_meta
            .get("git_dirty")
            .and_then(|v| v.as_bool())
            .is_some(),
        "build_meta should include git_dirty"
    );
    assert!(
        build_meta
            .get("realtime_profile")
            .and_then(|v| v.as_object())
            .is_some(),
        "build_meta should include realtime_profile"
    );
    assert_eq!(
        build_meta["realtime_profile"]["tick_ms"].as_u64(),
        Some(10),
        "realtime_profile tick_ms should come from scenario"
    );
    assert!(
        build_meta["realtime_profile"]["overrun_count"]
            .as_u64()
            .is_some(),
        "realtime_profile should include overrun_count"
    );
}
