use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn example_path(name: &str) -> PathBuf {
    repo_root().join("examples").join(name)
}

#[test]
fn quadratic_fit_compiles_successfully() {
    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg(example_path("quadratic_fit.plc"))
        .arg("--no-print-ir")
        .output()
        .expect("should run rust_plc");

    assert!(
        output.status.success(),
        "quadratic_fit.plc should compile successfully:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("验证通过"), "verification should pass");
    assert!(
        stderr.contains("Safety: 完备证明"),
        "safety verification should pass"
    );
    assert!(
        stderr.contains("Liveness: 通过"),
        "liveness verification should pass"
    );
    assert!(
        stderr.contains("Timing: 通过"),
        "timing verification should pass"
    );
    assert!(
        stderr.contains("Causality: 通过"),
        "causality verification should pass"
    );
}

#[test]
fn quadratic_fit_generates_st_code() {
    let temp_dir = std::env::temp_dir();
    let out_st = temp_dir.join(format!("quadratic_fit_{}.st", std::process::id()));

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("gen-st")
        .arg(example_path("quadratic_fit.plc"))
        .arg("--out")
        .arg(&out_st)
        .output()
        .expect("should run rust_plc gen-st");

    assert!(
        output.status.success(),
        "gen-st should succeed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let st_content = fs::read_to_string(&out_st).expect("should read generated ST file");

    // Check that ST code contains expected elements
    assert!(
        st_content.contains("PROGRAM Main"),
        "ST should contain PROGRAM Main"
    );
    assert!(
        st_content.contains("CASE _state OF"),
        "ST should contain state machine CASE"
    );

    // Check that variables are declared
    assert!(
        st_content.contains("a: REAL"),
        "ST should declare variable a"
    );
    assert!(
        st_content.contains("b: REAL"),
        "ST should declare variable b"
    );
    assert!(
        st_content.contains("c: REAL"),
        "ST should declare variable c"
    );
    assert!(
        st_content.contains("sum_x: REAL"),
        "ST should declare variable sum_x"
    );
    assert!(
        st_content.contains("sum_x2: REAL"),
        "ST should declare variable sum_x2"
    );
    assert!(
        st_content.contains("sum_y: REAL"),
        "ST should declare variable sum_y"
    );
    assert!(
        st_content.contains("det: REAL"),
        "ST should declare variable det"
    );

    // Check that states are present
    assert!(
        st_content.contains("main.init"),
        "ST should contain init state"
    );
    assert!(
        st_content.contains("main.accumulate_0"),
        "ST should contain accumulate_0 state"
    );
    assert!(
        st_content.contains("main.solve_system"),
        "ST should contain solve_system state"
    );
    assert!(
        st_content.contains("main.compute_a"),
        "ST should contain compute_a state"
    );
    assert!(
        st_content.contains("main.compute_b"),
        "ST should contain compute_b state"
    );
    assert!(
        st_content.contains("main.compute_c"),
        "ST should contain compute_c state"
    );

    // Clean up
    let _ = fs::remove_file(&out_st);
}

#[test]
fn quadratic_fit_uses_correct_number_of_variables() {
    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg(example_path("quadratic_fit.plc"))
        .output()
        .expect("should run rust_plc");

    assert!(
        output.status.success(),
        "quadratic_fit.plc should compile successfully"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse JSON output to check variable count
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("should parse JSON output");

    let variables = json["topology"]["variables"]
        .as_array()
        .expect("should have variables array");

    // Should have all declared variables (input data, intermediate, results)
    assert!(
        variables.len() <= 64,
        "should not exceed MAX_VARIABLES (64), got {}",
        variables.len()
    );

    // Check that key variables exist
    let var_names: Vec<String> = variables
        .iter()
        .map(|v| v["name"].as_str().unwrap().to_string())
        .collect();

    assert!(
        var_names.contains(&"a".to_string()),
        "should have variable a"
    );
    assert!(
        var_names.contains(&"b".to_string()),
        "should have variable b"
    );
    assert!(
        var_names.contains(&"c".to_string()),
        "should have variable c"
    );
    assert!(
        var_names.contains(&"sum_x".to_string()),
        "should have variable sum_x"
    );
    assert!(
        var_names.contains(&"sum_y".to_string()),
        "should have variable sum_y"
    );
}
