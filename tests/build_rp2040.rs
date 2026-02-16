use std::fs;
use std::process::Command;

const PLC_FIXTURE: &str = r#"
[topology]

device Y0: digital_output
device X0: digital_input

device start_button: digital_input {
    connected_to: X0
}

device valve_A: solenoid_valve {
    connected_to: Y0
}

device cyl_A: cylinder {
    connected_to: valve_A
}

device sensor_ext: sensor {
    connected_to: X0
    detects: cyl_A.extended
}

[constraints]

[tasks]

task main:
    step extend:
        action: extend cyl_A

    step wait_button:
        wait: start_button == true
        timeout: 50ms -> goto fault

    step dwell:
        delay: 20ms

    step retract:
        action: retract cyl_A

    on_complete: goto done

task fault:
    step retract_fault:
        action: retract cyl_A
    step alarm:
        action: log "fault timeout"
    on_complete: goto done

task done:
    step halt:
"#;

#[test]
fn cli_build_rp2040_emits_expected_artifacts() {
    let base = std::env::temp_dir().join(format!(
        "rust_plc_build_rp2040_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");

    let plc_path = base.join("fixture.plc");
    let out_dir = base.join("out");
    fs::write(&plc_path, PLC_FIXTURE).expect("write plc");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("build-rp2040")
        .arg(&plc_path)
        .arg("--out")
        .arg(&out_dir)
        .output()
        .expect("should run rust_plc build-rp2040");

    assert!(
        output.status.success(),
        "build-rp2040 should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let generated_path = out_dir.join("generated_program.rs");
    let meta_path = out_dir.join("build_meta.json");
    let iomap_path = out_dir.join("io_map.template.toml");

    assert!(generated_path.exists());
    assert!(meta_path.exists());
    assert!(iomap_path.exists());

    let generated = fs::read_to_string(&generated_path).expect("read generated");
    assert!(generated.contains("pub mod generated"));
    assert!(generated.contains("pub static PROGRAM"));
    assert!(generated.contains("Action::Log"));

    let meta = fs::read_to_string(&meta_path).expect("read meta");
    let v: serde_json::Value = serde_json::from_str(&meta).expect("meta should be valid JSON");
    assert_eq!(
        v.get("tool_version").and_then(|v| v.as_str()).is_some(),
        true
    );
    assert_eq!(
        v.get("runtime_semver").and_then(|v| v.as_str()).is_some(),
        true
    );
    let sha = v
        .get("plc_sha256")
        .and_then(|v| v.as_str())
        .expect("plc_sha256");
    assert_eq!(sha.len(), 64);

    let iomap = fs::read_to_string(&iomap_path).expect("read io map");
    assert!(iomap.contains("[digital_inputs]"));
    assert!(iomap.contains("[digital_outputs]"));
    assert!(iomap.contains("[analog_outputs]"));
}

#[test]
fn cli_build_rp2040_emit_uf2_requires_explicit_io_map() {
    let base = std::env::temp_dir().join(format!(
        "rust_plc_build_rp2040_emit_uf2_requires_iomap_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");

    let plc_path = base.join("fixture.plc");
    let out_dir = base.join("out");
    let uf2_path = base.join("firmware.uf2");
    fs::write(&plc_path, PLC_FIXTURE).expect("write plc");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("build-rp2040")
        .arg(&plc_path)
        .arg("--out")
        .arg(&out_dir)
        .arg("--emit-uf2")
        .arg(&uf2_path)
        .output()
        .expect("should run rust_plc build-rp2040");

    assert!(
        !output.status.success(),
        "build-rp2040 should fail without --io-map when --emit-uf2 is set"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--emit-uf2 requires --io-map"),
        "stderr should explain io-map requirement, got: {stderr}"
    );
}
