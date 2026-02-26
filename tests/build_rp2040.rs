use std::fs;
use std::process::Command;

const PLC_FIXTURE: &str = r#"
[topology]

device plc_main: plc {
    purpose: "测试主控器",
    ports: [Y0:digital:producer, X0:digital:consumer, AI0:analog:consumer, AO0:analog:producer]
}

device Y0: digital_output { purpose: "测试数字输出通道" }
device X0: digital_input { purpose: "测试数字输入通道" }

device AI0: analog_input { purpose: "测试压力采样输入", range: 0..100, unit: "bar" }
device AO0: analog_output { purpose: "测试模拟控制输出", range: 0..10, ramp_time: 500ms, unit: "V" }

device valve_A: solenoid_valve { purpose: "测试电磁阀执行器" }

device cyl_A: cylinder { purpose: "测试气缸执行机构" }

device sensor_ext: sensor { purpose: "测试到位传感器" }

relation { from: Y0.out, to: valve_A.coil, via: driven_by }
relation { from: valve_A.out, to: cyl_A.cmd, via: driven_by }
relation { from: cyl_A.extended, to: sensor_ext.sense, via: detects }
relation { from: sensor_ext.out, to: X0.in, via: reports_to }

[constraints]

[tasks]

task main:
    step extend:
        action: extend cyl_A

    step wait_button:
        wait: X0 == true
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
    let analog_contract_path = out_dir.join("analog_contract.toml");
    let analog_cal_template_path = out_dir.join("analog_calibration.template.toml");

    assert!(generated_path.exists());
    assert!(meta_path.exists());
    assert!(iomap_path.exists());
    assert!(analog_contract_path.exists());
    assert!(analog_cal_template_path.exists());

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
    assert_eq!(v.get("git_commit").and_then(|v| v.as_str()).is_some(), true);
    assert_eq!(v.get("git_dirty").and_then(|v| v.as_bool()).is_some(), true);
    let sha = v
        .get("plc_sha256")
        .and_then(|v| v.as_str())
        .expect("plc_sha256");
    assert_eq!(sha.len(), 64);

    let iomap = fs::read_to_string(&iomap_path).expect("read io map");
    assert!(iomap.contains("[digital_inputs]"));
    assert!(iomap.contains("[digital_outputs]"));
    assert!(iomap.contains("[analog_inputs]"));
    assert!(iomap.contains("[analog_outputs]"));
    assert!(
        iomap.contains("[motion.stepper.axis0]") || iomap.contains("motion.stepper.axis0"),
        "io_map.template.toml should include a motion config skeleton"
    );

    let analog_contract = fs::read_to_string(&analog_contract_path).expect("read analog contract");
    assert!(analog_contract.contains("[analog_inputs.ai0]"));
    assert!(analog_contract.contains("[analog_outputs.ao0]"));
    assert!(analog_contract.contains("scale = 1.0"));
    assert!(analog_contract.contains("offset = 0.0"));

    let analog_cal_template =
        fs::read_to_string(&analog_cal_template_path).expect("read analog calibration template");
    assert!(analog_cal_template.contains("[analog_inputs]"));
    assert!(analog_cal_template.contains("[analog_outputs]"));
}

#[test]
fn cli_build_rp2040_accepts_virtual_gpio_bindings_in_io_map() {
    let base = std::env::temp_dir().join(format!(
        "rust_plc_build_rp2040_virtual_iomap_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");

    let plc_path = base.join("fixture.plc");
    let out_dir = base.join("out");
    let io_map_path = base.join("io_map.toml");
    fs::write(&plc_path, PLC_FIXTURE).expect("write plc");
    fs::write(
        &io_map_path,
        r#"
[digital_inputs]
di0 = "virtual"

[digital_outputs]
do0 = "virtual"

[analog_inputs]
ai0 = "virtual"

[analog_outputs]
ao0 = "virtual"
"#,
    )
    .expect("write io_map");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("build-rp2040")
        .arg(&plc_path)
        .arg("--out")
        .arg(&out_dir)
        .arg("--io-map")
        .arg(&io_map_path)
        .output()
        .expect("should run rust_plc build-rp2040");

    assert!(
        output.status.success(),
        "build-rp2040 should accept virtual io_map mappings, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(out_dir.join("generated_program.rs").exists());
    assert!(out_dir.join("io_map.template.toml").exists());
    assert!(out_dir.join("analog_contract.toml").exists());
}

#[test]
fn cli_build_rp2040_applies_analog_calibration_overrides() {
    let base = std::env::temp_dir().join(format!(
        "rust_plc_build_rp2040_calibration_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");

    let plc_path = base.join("fixture.plc");
    let out_dir = base.join("out");
    let cal_path = base.join("analog_calibration.toml");
    fs::write(&plc_path, PLC_FIXTURE).expect("write plc");
    fs::write(
        &cal_path,
        r#"
[analog_inputs]
ai0 = { scale = 1.01, offset = -0.2 }

[analog_outputs]
ao0 = { scale = 0.98, offset = 0.15 }
"#,
    )
    .expect("write calibration");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("build-rp2040")
        .arg(&plc_path)
        .arg("--out")
        .arg(&out_dir)
        .arg("--analog-calibration")
        .arg(&cal_path)
        .output()
        .expect("run build-rp2040");

    assert!(
        output.status.success(),
        "build-rp2040 should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let analog_contract =
        fs::read_to_string(out_dir.join("analog_contract.toml")).expect("read contract");
    let parsed: toml::Value = toml::from_str(&analog_contract).expect("valid TOML");

    let ai0 = parsed
        .get("analog_inputs")
        .and_then(|v| v.get("ai0"))
        .expect("analog_inputs.ai0");
    let ai0_scale = ai0
        .get("scale")
        .and_then(|v| v.as_float())
        .expect("ai0.scale");
    let ai0_offset = ai0
        .get("offset")
        .and_then(|v| v.as_float())
        .expect("ai0.offset");
    assert!((ai0_scale - 1.01).abs() < 1e-6);
    assert!((ai0_offset - (-0.2)).abs() < 1e-6);

    let ao0 = parsed
        .get("analog_outputs")
        .and_then(|v| v.get("ao0"))
        .expect("analog_outputs.ao0");
    let ao0_scale = ao0
        .get("scale")
        .and_then(|v| v.as_float())
        .expect("ao0.scale");
    let ao0_offset = ao0
        .get("offset")
        .and_then(|v| v.as_float())
        .expect("ao0.offset");
    assert!((ao0_scale - 0.98).abs() < 1e-6);
    assert!((ao0_offset - 0.15).abs() < 1e-6);
}

#[test]
fn cli_build_rp2040_rejects_unknown_calibration_channel() {
    let base = std::env::temp_dir().join(format!(
        "rust_plc_build_rp2040_calibration_unknown_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");

    let plc_path = base.join("fixture.plc");
    let out_dir = base.join("out");
    let cal_path = base.join("analog_calibration.toml");
    fs::write(&plc_path, PLC_FIXTURE).expect("write plc");
    fs::write(
        &cal_path,
        r#"
[analog_outputs]
ao99 = { scale = 1.0, offset = 0.0 }
"#,
    )
    .expect("write calibration");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("build-rp2040")
        .arg(&plc_path)
        .arg("--out")
        .arg(&out_dir)
        .arg("--analog-calibration")
        .arg(&cal_path)
        .output()
        .expect("run build-rp2040");

    assert!(
        !output.status.success(),
        "build-rp2040 should fail on unknown calibration channel"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("analog calibration key not found in contract"),
        "stderr should explain calibration mismatch, got: {stderr}"
    );
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

#[cfg(unix)]
fn make_executable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut p = fs::metadata(path).expect("metadata").permissions();
    p.set_mode(0o755);
    fs::set_permissions(path, p).expect("chmod");
}

#[cfg(not(unix))]
fn make_executable(_path: &std::path::Path) {}

fn write_io_map(path: &std::path::Path) {
    fs::write(
        path,
        r#"
[digital_inputs]
di0 = 2

[digital_outputs]
do0 = 16
"#,
    )
    .expect("write io map");
}

#[test]
fn cli_build_rp2040_emit_uf2_with_mocked_toolchain_succeeds() {
    let base = std::env::temp_dir().join(format!(
        "rust_plc_build_rp2040_emit_uf2_success_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");
    let scripts = base.join("scripts");
    fs::create_dir_all(&scripts).expect("create scripts dir");

    let fake_cargo = scripts.join("fake_cargo.sh");
    fs::write(
        &fake_cargo,
        "#!/usr/bin/env bash\nset -eu\nTARGET_DIR=\"${CARGO_TARGET_DIR:-target}\"\nmkdir -p \"$TARGET_DIR/thumbv6m-none-eabi/release\"\n: > \"$TARGET_DIR/thumbv6m-none-eabi/release/board-rp2040\"\n",
    )
    .expect("write fake cargo");
    make_executable(&fake_cargo);

    let fake_elf2uf2 = scripts.join("fake_elf2uf2.sh");
    fs::write(
        &fake_elf2uf2,
        "#!/usr/bin/env bash\nset -eu\ncp \"$1\" \"$2\"\n",
    )
    .expect("write fake elf2uf2");
    make_executable(&fake_elf2uf2);

    let plc_path = base.join("fixture.plc");
    let out_dir = base.join("out");
    let io_map = base.join("io_map.toml");
    let uf2_path = base.join("firmware.uf2");
    let target_dir = base.join("target");
    fs::write(&plc_path, PLC_FIXTURE).expect("write plc");
    write_io_map(&io_map);

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("build-rp2040")
        .arg(&plc_path)
        .arg("--out")
        .arg(&out_dir)
        .arg("--io-map")
        .arg(&io_map)
        .arg("--emit-uf2")
        .arg(&uf2_path)
        .env("RUST_PLC_CARGO_BIN", &fake_cargo)
        .env("RUST_PLC_ELF2UF2_BIN", &fake_elf2uf2)
        .env("CARGO_TARGET_DIR", &target_dir)
        .output()
        .expect("run build-rp2040");

    assert!(
        output.status.success(),
        "build-rp2040 --emit-uf2 should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(uf2_path.exists(), "UF2 artifact should exist");
}

#[test]
fn cli_build_rp2040_emit_uf2_reports_missing_converter_tool() {
    let base = std::env::temp_dir().join(format!(
        "rust_plc_build_rp2040_emit_uf2_missing_tool_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");
    let scripts = base.join("scripts");
    fs::create_dir_all(&scripts).expect("create scripts dir");

    let fake_cargo = scripts.join("fake_cargo.sh");
    fs::write(
        &fake_cargo,
        "#!/usr/bin/env bash\nset -eu\nTARGET_DIR=\"${CARGO_TARGET_DIR:-target}\"\nmkdir -p \"$TARGET_DIR/thumbv6m-none-eabi/release\"\n: > \"$TARGET_DIR/thumbv6m-none-eabi/release/board-rp2040\"\n",
    )
    .expect("write fake cargo");
    make_executable(&fake_cargo);

    let plc_path = base.join("fixture.plc");
    let out_dir = base.join("out");
    let io_map = base.join("io_map.toml");
    let uf2_path = base.join("firmware.uf2");
    let target_dir = base.join("target");
    fs::write(&plc_path, PLC_FIXTURE).expect("write plc");
    write_io_map(&io_map);

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("build-rp2040")
        .arg(&plc_path)
        .arg("--out")
        .arg(&out_dir)
        .arg("--io-map")
        .arg(&io_map)
        .arg("--emit-uf2")
        .arg(&uf2_path)
        .env("RUST_PLC_CARGO_BIN", &fake_cargo)
        .env("RUST_PLC_ELF2UF2_BIN", scripts.join("not-found-tool"))
        .env("CARGO_TARGET_DIR", &target_dir)
        .output()
        .expect("run build-rp2040");

    assert!(
        !output.status.success(),
        "build-rp2040 should fail when converter tool is missing"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Failed to run"),
        "stderr should mention converter execution failure, got: {stderr}"
    );
}

#[test]
fn cli_build_rp2040_emit_uf2_reports_missing_elf_output() {
    let base = std::env::temp_dir().join(format!(
        "rust_plc_build_rp2040_emit_uf2_missing_elf_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");
    let scripts = base.join("scripts");
    fs::create_dir_all(&scripts).expect("create scripts dir");

    let fake_cargo = scripts.join("fake_cargo_no_elf.sh");
    fs::write(
        &fake_cargo,
        "#!/usr/bin/env bash\nset -eu\nTARGET_DIR=\"${CARGO_TARGET_DIR:-target}\"\nmkdir -p \"$TARGET_DIR/thumbv6m-none-eabi/release\"\n# intentionally no ELF\n",
    )
        .expect("write fake cargo");
    make_executable(&fake_cargo);

    let fake_elf2uf2 = scripts.join("fake_elf2uf2.sh");
    fs::write(
        &fake_elf2uf2,
        "#!/usr/bin/env bash\nset -eu\ncp \"$1\" \"$2\"\n",
    )
    .expect("write fake elf2uf2");
    make_executable(&fake_elf2uf2);

    let plc_path = base.join("fixture.plc");
    let out_dir = base.join("out");
    let io_map = base.join("io_map.toml");
    let uf2_path = base.join("firmware.uf2");
    let target_dir = base.join("target");
    fs::write(&plc_path, PLC_FIXTURE).expect("write plc");
    write_io_map(&io_map);

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("build-rp2040")
        .arg(&plc_path)
        .arg("--out")
        .arg(&out_dir)
        .arg("--io-map")
        .arg(&io_map)
        .arg("--emit-uf2")
        .arg(&uf2_path)
        .env("RUST_PLC_CARGO_BIN", &fake_cargo)
        .env("RUST_PLC_ELF2UF2_BIN", &fake_elf2uf2)
        .env("CARGO_TARGET_DIR", &target_dir)
        .output()
        .expect("run build-rp2040");

    assert!(
        !output.status.success(),
        "build-rp2040 should fail when ELF is missing after build"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Expected firmware ELF does not exist"),
        "stderr should mention missing ELF, got: {stderr}"
    );
}
