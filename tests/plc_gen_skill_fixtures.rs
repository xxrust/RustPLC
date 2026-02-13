use rust_plc::error::PlcError;
use rust_plc::parser::parse_plc;
use rust_plc::semantic::{
    build_constraint_set, build_state_machine, build_timing_model, build_topology_graph,
    preprocess_program,
};
use rust_plc::verification::verify_all;
use std::fs;
use std::path::{Path, PathBuf};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(".codex")
        .join("skills")
        .join("plc-gen")
        .join("fixtures")
        .join("valid")
}

fn collect_plc_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = match fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) == Some("plc") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
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

fn compile_and_verify(source: &str) -> Result<(), Vec<String>> {
    let program = parse_plc(source).map_err(|err| vec![err.to_string()])?;
    let expanded_program = preprocess_program(&program).map_err(|errors| {
        errors
            .into_iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
    })?;

    let mut errors = Vec::new();
    let topology = collect_stage(build_topology_graph(&expanded_program), &mut errors);
    let state_machine = collect_stage(build_state_machine(&expanded_program), &mut errors);
    let constraints = collect_stage(build_constraint_set(&expanded_program), &mut errors);
    let _timing_model = collect_stage(build_timing_model(&expanded_program), &mut errors);

    if !errors.is_empty() {
        return Err(errors.into_iter().map(|error| error.to_string()).collect());
    }

    let topology = topology.expect("topology exists when semantic errors are empty");
    let state_machine = state_machine.expect("state machine exists when semantic errors are empty");
    let constraints = constraints.expect("constraints exist when semantic errors are empty");

    let summary = verify_all(&expanded_program, &topology, &constraints, &state_machine)
        .map_err(|diagnostics| {
            diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.to_string())
                .collect::<Vec<_>>()
        })?;

    assert!(
        matches!(summary.safety.level.as_str(), "完备证明" | "有界验证"),
        "unexpected safety level: {}",
        summary.safety.level
    );
    assert_eq!(summary.liveness.level, "通过");
    assert_eq!(summary.timing.level, "通过");
    assert_eq!(summary.causality.level, "通过");

    Ok(())
}

#[test]
fn plc_gen_skill_valid_fixtures_compile_and_verify() {
    let dir = fixtures_dir();
    let files = collect_plc_files(&dir);
    assert!(
        !files.is_empty(),
        "expected at least one plc-gen fixture under {}",
        dir.display()
    );

    let mut failures = Vec::new();
    for file in files {
        let source = match fs::read_to_string(&file) {
            Ok(source) => source,
            Err(err) => {
                failures.push(format!("{}: failed to read: {err}", file.display()));
                continue;
            }
        };

        if let Err(errors) = compile_and_verify(&source) {
            failures.push(format!(
                "{}:\n{}",
                file.display(),
                errors.join("\n")
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "plc-gen fixtures should compile & verify:\n{}",
        failures.join("\n\n")
    );
}

