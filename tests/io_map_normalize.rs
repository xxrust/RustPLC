use std::fs;
use std::path::Path;
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

fn write(path: &Path, text: &str) {
    fs::write(path, text).unwrap_or_else(|e| panic!("write failed {path:?}: {e}"));
}

#[test]
fn io_map_normalize_converts_iec_keys_to_native_keys_and_preserves_other_sections() {
    let base = temp_dir("rust_plc_io_map_normalize");
    let input = base.join("io_map.toml");
    let out = base.join("normalized.toml");

    write(
        &input,
        r#"
[digital_inputs]
di0 = 1
"%IX0.1" = 2

[digital_outputs]
do1 = 6
"%QX0.0" = 5

[analog_outputs]
"%QW0" = 12

[safe_state]
mode = "profile"
on_exit_timeout_ms = 50
"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("io-map-normalize")
        .arg("--in")
        .arg(&input)
        .arg("--out")
        .arg(&out)
        .output()
        .expect("run io-map-normalize");

    assert!(
        output.status.success(),
        "io-map-normalize should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let normalized = fs::read_to_string(&out).expect("read normalized");
    assert!(!normalized.contains("%IX"), "should remove IEC keys");
    assert!(!normalized.contains("%QX"), "should remove IEC keys");
    assert!(!normalized.contains("%QW"), "should remove IEC keys");

    // Check expected canonical keys exist.
    assert!(normalized.contains("[digital_inputs]"));
    assert!(normalized.contains("di0 = 1"));
    assert!(normalized.contains("di1 = 2"));
    assert!(normalized.contains("[digital_outputs]"));
    assert!(normalized.contains("do0 = 5"));
    assert!(normalized.contains("do1 = 6"));
    assert!(normalized.contains("[analog_outputs]"));
    assert!(normalized.contains("ao0 = 12"));

    // Preserve safe_state.
    assert!(normalized.contains("[safe_state]"));
    assert!(normalized.contains("mode = \"profile\""));
    assert!(normalized.contains("on_exit_timeout_ms = 50"));
}

#[test]
fn io_map_normalize_reports_conflicts_for_same_logical_channel() {
    let base = temp_dir("rust_plc_io_map_normalize_conflict");
    let input = base.join("io_map.toml");
    let out = base.join("normalized.toml");

    write(
        &input,
        r#"
[digital_outputs]
do0 = 5
"%QX0.0" = 6

[digital_inputs]
di0 = 1
"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("io-map-normalize")
        .arg("--in")
        .arg(&input)
        .arg("--out")
        .arg(&out)
        .output()
        .expect("run io-map-normalize");

    assert!(
        !output.status.success(),
        "io-map-normalize should fail on conflict"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Conflict for do0"),
        "expected conflict error; stderr was:\n{stderr}"
    );
}
