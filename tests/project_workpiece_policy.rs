use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, content).expect("write file");
}

fn run_cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .args(args)
        .output()
        .expect("run rust_plc")
}

#[test]
fn project_sources_require_workpiece_by_default() {
    let project_dir = temp_dir("rust_plc_project_requires_workpiece");
    write(
        &project_dir.join("rustplc.project.toml"),
        "schema_version = 1\n\n[project]\nname = \"No Workpiece\"\nslug = \"no_workpiece\"\n\n[entry]\nsystem = \"plc/main.system.md\"\nplc = \"plc/main.plc\"\nscenario = \"scenarios/nominal/normal.yaml\"\nio_map = \"config/io_map.toml\"\nretain = \"config/retain.toml\"\nworkpiece = \"config/workpiece.toml\"\n",
    );
    write(
        &project_dir.join("plc/main.system.md"),
        "# No Workpiece System\n",
    );
    write(
        &project_dir.join("plc/main.plc"),
        "[topology]\n\ndevice plc_main: plc {\n    purpose: \"demo controller\"\n    model_ref: openplc_softplc\n}\n\ndevice start_button: sensor { purpose: \"start\", subtype: \"push_button\", debounce: 20ms }\nrelation { from: start_button.out, to: plc_main.X0, via: reports_to }\n\n[constraints]\n\n[tasks]\n\ntask main:\n    step wait_start:\n        wait: start_button == true\n        allow_indefinite_wait: true\n",
    );

    let output = run_cli(&[
        project_dir
            .join("plc/main.plc")
            .to_str()
            .expect("utf8 path"),
        "--no-print-ir",
    ]);
    assert!(
        !output.status.success(),
        "compile should fail when project workpiece policy is implicitly required"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("requires first-class workpiece semantics"));
    assert!(stderr.contains("config/workpiece.toml"));
}

#[test]
fn project_sources_reject_placeholder_workpiece_without_effects() {
    let project_dir = temp_dir("rust_plc_project_placeholder_workpiece");
    write(
        &project_dir.join("rustplc.project.toml"),
        "schema_version = 1\n\n[project]\nname = \"Placeholder Workpiece\"\nslug = \"placeholder_workpiece\"\n\n[entry]\nsystem = \"plc/main.system.md\"\nplc = \"plc/main.plc\"\nscenario = \"scenarios/nominal/normal.yaml\"\nio_map = \"config/io_map.toml\"\nretain = \"config/retain.toml\"\nworkpiece = \"config/workpiece.toml\"\n",
    );
    write(
        &project_dir.join("plc/main.system.md"),
        "# Placeholder Workpiece System\n",
    );
    write(
        &project_dir.join("config/workpiece.toml"),
        "schema_version = 1\n\n[workpiece]\nrequired = true\n",
    );
    write(
        &project_dir.join("plc/main.plc"),
        "[topology]\n\nworkpiece part: workpiece_type {\n    normal_terminal_states: [finished]\n    ingress_sites: [infeed]\n    normal_egress_sites: [outfeed]\n}\n\nlocation infeed: workpiece_location { capacity: 1 }\nlocation outfeed: workpiece_location { capacity: 1 }\n\n[constraints]\n\n[tasks]\n\ntask main:\n    step idle:\n        action: log \"placeholder\"\n",
    );

    let output = run_cli(&[
        project_dir
            .join("plc/main.plc")
            .to_str()
            .expect("utf8 path"),
        "--no-print-ir",
    ]);
    assert!(
        !output.status.success(),
        "compile should fail when project declares workpiece topology but no effects"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no task uses any `effect:` statement"));
}

#[test]
fn standalone_compile_warns_on_container_like_single_capacity_location() {
    let project_dir = temp_dir("rust_plc_capacity_warning");
    let plc_path = project_dir.join("capacity_warning.plc");
    write(
        &plc_path,
        "[topology]\n\ndevice operator_empty_confirm: sensor { purpose: \"operator confirms workpiece baseline is empty\" }\n\nworkpiece part: workpiece_type {\n    normal_terminal_states: [finished]\n    abnormal_terminal_states: [rejected]\n    ingress_sites: [storage_box]\n    normal_egress_sites: [outfeed]\n    abnormal_egress_sites: [reject_bin]\n}\n\nlocation storage_box: workpiece_location { capacity: 1 }\nlocation outfeed: workpiece_location { capacity: 1 }\nlocation reject_bin: workpiece_location { capacity: 20 }\n\n[constraints]\n\n[tasks]\n\ntask startup_init:\n    step confirm_empty_baseline:\n        wait: operator_empty_confirm == true\n        allow_indefinite_wait: true\n\n    on_complete: goto main.idle\n\ntask main:\n    step idle:\n",
    );

    let output = run_cli(&[plc_path.to_str().expect("utf8 path"), "--no-print-ir"]);
    assert!(
        output.status.success(),
        "standalone compile should keep the capacity lint as a warning, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("WARNING [topology]"));
    assert!(stderr.contains("WORKPIECE-CAP-001"));
    assert!(stderr.contains("storage_box"));
}

#[test]
fn project_sources_can_opt_out_with_explicit_workpiece_policy() {
    let project_dir = temp_dir("rust_plc_project_opt_out_workpiece");
    write(
        &project_dir.join("rustplc.project.toml"),
        "schema_version = 1\n\n[project]\nname = \"No Workpiece Allowed\"\nslug = \"no_workpiece_allowed\"\n\n[entry]\nsystem = \"plc/main.system.md\"\nplc = \"plc/main.plc\"\nscenario = \"scenarios/nominal/normal.yaml\"\nio_map = \"config/io_map.toml\"\nretain = \"config/retain.toml\"\nworkpiece = \"config/workpiece.toml\"\n",
    );
    write(
        &project_dir.join("plc/main.system.md"),
        "# Explicit No Workpiece System\n",
    );
    write(
        &project_dir.join("config/workpiece.toml"),
        "schema_version = 1\n\n[workpiece]\nrequired = false\n",
    );
    write(
        &project_dir.join("plc/main.plc"),
        "[topology]\n\ndevice plc_main: plc {\n    purpose: \"demo controller\"\n    model_ref: openplc_softplc\n}\n\ndevice start_button: sensor { purpose: \"start\", subtype: \"push_button\", debounce: 20ms }\nrelation { from: start_button.out, to: plc_main.X0, via: reports_to }\n\n[constraints]\n\n[tasks]\n\ntask main:\n    step wait_start:\n        wait: start_button == true\n        allow_indefinite_wait: true\n",
    );

    let output = run_cli(&[
        project_dir
            .join("plc/main.plc")
            .to_str()
            .expect("utf8 path"),
        "--no-print-ir",
    ]);
    assert!(
        output.status.success(),
        "compile should succeed when the project explicitly opts out of workpiece semantics, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
