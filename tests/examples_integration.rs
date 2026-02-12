use rust_plc::error::PlcError;
use rust_plc::parser::parse_plc;
use rust_plc::semantic::{
    build_constraint_set, build_state_machine, build_timing_model, build_topology_graph,
};
use rust_plc::verification::verify_all;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn example_path(file_name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(file_name)
}

fn read_example(file_name: &str) -> String {
    let path = example_path(file_name);
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read example {}: {err}", path.display()))
}

fn collect_stage<T>(result: Result<T, Vec<PlcError>>, errors: &mut Vec<PlcError>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(mut stage_errors) => {
            errors.append(&mut stage_errors);
            None
        }
    }
}

fn compile_source_to_json(source: &str) -> Result<Value, Vec<String>> {
    let program = parse_plc(source).map_err(|err| vec![err.to_string()])?;

    let mut errors = Vec::new();
    let topology = collect_stage(build_topology_graph(&program), &mut errors);
    let state_machine = collect_stage(build_state_machine(&program), &mut errors);
    let constraints = collect_stage(build_constraint_set(&program), &mut errors);
    let timing_model = collect_stage(build_timing_model(&program), &mut errors);

    if !errors.is_empty() {
        return Err(errors.into_iter().map(|error| error.to_string()).collect());
    }

    let topology = topology.expect("topology exists when semantic errors are empty");
    let state_machine = state_machine.expect("state machine exists when semantic errors are empty");
    let constraints = constraints.expect("constraints exist when semantic errors are empty");
    let timing_model = timing_model.expect("timing model exists when semantic errors are empty");

    let verification =
        verify_all(&program, &topology, &constraints, &state_machine).map_err(|diagnostics| {
            diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.to_string())
                .collect::<Vec<_>>()
        })?;

    let payload = json!({
        "topology": topology,
        "state_machine": state_machine,
        "constraints": constraints,
        "timing_model": timing_model,
        "verification": verification,
    });

    let serialized = serde_json::to_string_pretty(&payload).expect("IR payload should serialize");
    let decoded: Value =
        serde_json::from_str(&serialized).expect("serialized IR payload should be valid JSON");

    Ok(decoded)
}

#[test]
fn parses_two_cylinder_example_into_verified_ir_json() {
    let source = read_example("two_cylinder.plc");
    let ir_json = compile_source_to_json(&source).expect("two_cylinder example should compile");

    assert!(ir_json.get("topology").is_some());
    assert!(ir_json.get("state_machine").is_some());
    assert!(ir_json.get("constraints").is_some());
    assert!(ir_json.get("timing_model").is_some());

    let states = ir_json["state_machine"]["states"]
        .as_array()
        .expect("state machine should include states array");
    assert!(!states.is_empty(), "state machine should have states");

    let safety_level = ir_json["verification"]["safety"]["level"]
        .as_str()
        .expect("verification.safety.level should be present");
    assert!(
        matches!(safety_level, "完备证明" | "有界验证"),
        "safety level should report proof quality"
    );
    assert_eq!(
        ir_json["verification"]["liveness"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["timing"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["causality"]["level"],
        Value::String("通过".to_string())
    );
}

#[test]
fn parses_half_rotation_example_into_verified_ir_json() {
    let source = read_example("half_rotation.plc");
    let ir_json = compile_source_to_json(&source).expect("half_rotation example should compile");

    let transitions = ir_json["state_machine"]["transitions"]
        .as_array()
        .expect("state machine should include transitions array");
    assert!(
        !transitions.is_empty(),
        "state machine should have transitions"
    );

    let timing_rules = ir_json["constraints"]["timing"]
        .as_array()
        .expect("constraints should include timing array");
    assert_eq!(
        timing_rules.len(),
        1,
        "half_rotation should define one timing rule"
    );

    assert_eq!(
        ir_json["verification"]["liveness"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["timing"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["causality"]["level"],
        Value::String("通过".to_string())
    );
}

#[test]
fn parses_delay_demo_example_into_verified_ir_json() {
    let source = read_example("delay_demo.plc");
    let ir_json = compile_source_to_json(&source).expect("delay_demo example should compile");

    let transitions = ir_json["state_machine"]["transitions"]
        .as_array()
        .expect("state machine should include transitions array");
    assert!(
        transitions.iter().any(|transition| {
            transition["guard"]["kind"] == Value::String("delay".to_string())
                && transition["guard"]["duration_ms"] == Value::Number(2000u64.into())
        }),
        "delay_demo should include a delay guard transition"
    );

    assert_eq!(
        ir_json["verification"]["liveness"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["timing"]["level"],
        Value::String("通过".to_string())
    );
    assert_eq!(
        ir_json["verification"]["causality"]["level"],
        Value::String("通过".to_string())
    );
}

#[test]
fn reports_undefined_device_for_error_example() {
    let source = read_example("error_missing_device.plc");
    let errors = compile_source_to_json(&source)
        .expect_err("error_missing_device should fail semantic checks");

    assert!(
        errors.iter().any(|error| error.contains("未定义设备 Y9")),
        "error output should include missing device name"
    );
}

#[test]
fn reports_all_four_verifier_failures_for_combined_error_example() {
    let source = read_example("error_all_verifiers.plc");
    let errors = compile_source_to_json(&source)
        .expect_err("combined verifier error example should fail verification");

    let joined = errors.join("\n\n");
    assert!(
        joined.contains("ERROR [safety]"),
        "should report safety error"
    );
    assert!(
        joined.contains("ERROR [liveness]"),
        "should report liveness error"
    );
    assert!(
        joined.contains("ERROR [timing]"),
        "should report timing error"
    );
    assert!(
        joined.contains("ERROR [causality]"),
        "should report causality error"
    );
    assert!(
        joined.contains("位置:"),
        "errors should include source location"
    );
    assert!(
        joined.contains("建议:"),
        "errors should include fix suggestions"
    );
}

#[test]
fn cli_prints_verified_json_and_summary_for_two_cylinder_example() {
    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg(example_path("two_cylinder.plc"))
        .output()
        .expect("should run rust_plc binary");

    assert!(
        output.status.success(),
        "CLI should succeed for valid example, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let decoded: Value =
        serde_json::from_slice(&output.stdout).expect("CLI stdout should be valid JSON");
    assert!(decoded.get("topology").is_some());
    assert!(decoded.get("state_machine").is_some());
    assert!(decoded.get("constraints").is_some());
    assert!(decoded.get("timing_model").is_some());
    assert!(decoded.get("verification").is_some());

    let stderr_text = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr_text.contains("验证通过"),
        "CLI should print success summary to stderr"
    );
    assert!(
        stderr_text.contains("Safety:"),
        "CLI summary should include safety proof level"
    );
}
