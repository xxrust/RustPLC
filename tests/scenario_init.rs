use sim::Scenario;
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
fn scenario_init_generates_parseable_yaml() {
    let base = temp_dir("rust_plc_scenario_init");

    let cases = [
        ("examples/assembly_station.plc", "normal"),
        ("examples/two_cylinder.plc", "minimal"),
    ];

    for (rel_plc, preset) in cases {
        let plc = repo_path(rel_plc);
        assert!(plc.exists(), "expected PLC example to exist: {rel_plc}");

        let out = base.join(format!(
            "{}_{preset}.yaml",
            plc.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("scenario")
        ));

        let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
            .arg("scenario-init")
            .arg(&plc)
            .arg("--out")
            .arg(&out)
            .arg("--preset")
            .arg(preset)
            .output()
            .expect("run scenario-init");

        assert!(
            output.status.success(),
            "scenario-init should succeed, stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(out.exists(), "expected output file to exist at {out:?}");

        let yaml = fs::read_to_string(&out).expect("read generated YAML");
        assert!(yaml.contains("tick_ms:"), "expected tick_ms field");
        assert!(yaml.contains("duration_ms:"), "expected duration_ms field");

        let scenario = Scenario::from_yaml_str(&yaml).expect("generated scenario should parse");
        assert!(scenario.tick_ms > 0, "tick_ms should be > 0");
        assert!(scenario.duration_ms > 0, "duration_ms should be > 0");
    }
}
