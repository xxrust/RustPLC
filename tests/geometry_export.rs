use serde_json::Value;
use std::fs;
use std::path::PathBuf;
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

fn write_fixture_plc(path: &PathBuf) {
    let plc = r#"
[topology]
device plc_main: plc {
    purpose: "geometry export fixture controller"
    model_ref: rp2040_softplc
}

[constraints]

[tasks]
task main:
    step wait_start:
        wait: X0 == true
        allow_indefinite_wait: true
    step run:
        action: set Y0 on
"#;
    fs::write(path, plc).expect("write fixture plc");
}

#[test]
fn geometry_export_writes_artifact_and_json_summary() {
    let base = temp_dir("rust_plc_geometry_export");
    let plc = base.join("fixture.plc");
    let out = base.join("geometry.json");
    write_fixture_plc(&plc);

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("geometry-export")
        .arg(&plc)
        .arg("--out")
        .arg(&out)
        .arg("--output")
        .arg("json")
        .output()
        .expect("run geometry-export");

    assert!(
        output.status.success(),
        "geometry-export should pass, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(out.exists(), "geometry artifact should be written");

    let summary: Value =
        serde_json::from_slice(&output.stdout).expect("geometry-export should print JSON");
    assert_eq!(
        summary.get("command").and_then(Value::as_str),
        Some("geometry-export")
    );
    assert_eq!(
        summary
            .get("observed_transition_count")
            .and_then(Value::as_u64),
        Some(0)
    );

    let artifact_text = fs::read_to_string(&out).expect("read geometry artifact");
    let artifact: Value = serde_json::from_str(&artifact_text).expect("artifact json");
    assert_eq!(
        artifact.get("artifact_kind").and_then(Value::as_str),
        Some("semantic_twin_geometry")
    );
    assert_eq!(
        artifact
            .get("summary")
            .and_then(|value| value.get("task_count"))
            .and_then(Value::as_u64),
        Some(1)
    );

    let nodes = artifact
        .get("nodes")
        .and_then(Value::as_array)
        .expect("nodes array");
    assert!(nodes
        .iter()
        .any(|node| { node.get("id").and_then(Value::as_str) == Some("task:main") }));
    assert!(nodes
        .iter()
        .any(|node| { node.get("id").and_then(Value::as_str) == Some("step:main.wait_start") }));
}
