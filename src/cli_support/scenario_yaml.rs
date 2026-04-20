use crate::cli_support::common::display_path_relative_to_cwd;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

const SCENARIO_YAML_MINIMAL_TEMPLATE: &str = r#"tick_ms: 10
duration_ms: 1000
inputs:
  - at_ms: 0
    set:
      digital_inputs:
        0: true
"#;

fn should_skip_suggest_walk_dir(name: &OsStr) -> bool {
    matches!(
        name.to_str(),
        Some(".git")
            | Some("target")
            | Some("out")
            | Some("archive")
            | Some(".codex")
            | Some(".claude")
            | Some(".ralph_logs")
            | Some("node_modules")
    )
}

fn find_similar_yaml_files_by_name(wanted_file_name: &OsStr, max_matches: usize) -> Vec<PathBuf> {
    let Ok(cwd) = env::current_dir() else {
        return Vec::new();
    };

    let mut matches = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = vec![(cwd, 0)];
    let mut entries_seen: usize = 0;
    let max_entries: usize = 20_000;
    let max_depth: usize = 8;

    while let Some((dir, depth)) = stack.pop() {
        if depth > max_depth {
            continue;
        }
        if dir
            .file_name()
            .is_some_and(|name| should_skip_suggest_walk_dir(name))
        {
            continue;
        }
        let Ok(read_dir) = fs::read_dir(&dir) else {
            continue;
        };

        for entry in read_dir {
            entries_seen += 1;
            if entries_seen > max_entries {
                return matches;
            }

            let Ok(entry) = entry else {
                continue;
            };
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();

            if file_type.is_dir() {
                stack.push((path, depth + 1));
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str());
            if !matches!(ext, Some("yaml") | Some("yml")) {
                continue;
            }
            if path.file_name() != Some(wanted_file_name) {
                continue;
            }

            matches.push(path);
            if matches.len() >= max_matches {
                return matches;
            }
        }
    }

    matches
}

fn scenario_yaml_help() -> String {
    let mut msg = String::new();
    msg.push_str("Minimal scenario template:\n");
    msg.push_str(SCENARIO_YAML_MINIMAL_TEMPLATE);
    msg.push('\n');
    msg.push_str("Tips:\n");
    msg.push_str("- `at_ms` must be < `duration_ms` and aligned to `tick_ms`.\n");
    msg.push_str("- IDs are numeric (0 => DI0/AI0, 10 => DI10, ...).\n");
    msg
}

pub(crate) fn read_scenario_yaml_file(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| {
        if err.kind() != std::io::ErrorKind::NotFound {
            return format!(
                "Failed to read scenario YAML file {}: {err}",
                path.display()
            );
        }

        let mut msg = format!("Scenario YAML file not found: {}\n", path.display());
        if let Ok(cwd) = env::current_dir() {
            msg.push_str(&format!("  cwd: {}\n", cwd.display()));
        }

        if let Some(wanted_name) = path.file_name() {
            let suggestions = find_similar_yaml_files_by_name(wanted_name, 6);
            if !suggestions.is_empty() {
                msg.push_str("  similarly named files found:\n");
                for suggestion in suggestions {
                    msg.push_str(&format!(
                        "    - {}\n",
                        display_path_relative_to_cwd(&suggestion)
                    ));
                }
            }
        }

        msg.push('\n');
        msg.push_str(&scenario_yaml_help());
        msg
    })
}

pub(crate) fn parse_scenario_yaml(yaml: &str) -> Result<sim::Scenario, String> {
    sim::Scenario::from_yaml_str(yaml).map_err(|e| {
        format!(
            "Failed to parse scenario YAML: {e}\n\n{}",
            scenario_yaml_help()
        )
    })
}

pub(crate) fn scenario_mismatch_hint_for_example(
    plc_path: &str,
    scenario_path: &Path,
    err: &sim::SimRunError,
    subcommand: &str,
) -> Option<String> {
    if !matches!(
        err,
        sim::SimRunError::Runtime(runtime_core::RuntimeError::TooManyTransitionsInOneTick { .. })
    ) {
        return None;
    }

    scenario_mismatch_hint_for_example_paths(plc_path, scenario_path, subcommand)
}

fn scenario_mismatch_hint_for_example_paths(
    _plc_path: &str,
    scenario_path: &Path,
    subcommand: &str,
) -> Option<String> {
    let scenario_name = scenario_path.file_name().and_then(|s| s.to_str())?;

    if scenario_name == "normal.yaml" {
        let mut msg = String::from(
            "Tip: `normal.yaml` is usually project-specific and should be regenerated for the PLC you are validating.\n\
Known healthy reference: `examples/rp2040_motion_minimal.plc` with `scenarios/rp2040_motion_minimal/normal.yaml`.\n\
Preferred fix: run `scenario-init` for your current PLC and use the generated scenario.",
        );
        if let Some(suggested_cmd) = suggested_example_command(subcommand) {
            msg.push_str("\nSuggested command:\n  ");
            msg.push_str(suggested_cmd);
        }
        return Some(msg);
    }

    None
}

fn suggested_example_command(subcommand: &str) -> Option<&'static str> {
    match subcommand {
        "sim-plc" => Some(
            "cargo run --release --bin rust_plc -- sim-plc examples/rp2040_motion_minimal.plc --scenario scenarios/rp2040_motion_minimal/normal.yaml --out trace.jsonl",
        ),
        "scenario-validate" => Some(
            "cargo run --release --bin rust_plc -- scenario-validate examples/rp2040_motion_minimal.plc --scenario scenarios/rp2040_motion_minimal/normal.yaml",
        ),
        "scenario-doctor" => Some(
            "cargo run --release --bin rust_plc -- scenario-doctor examples/rp2040_motion_minimal.plc --scenario scenarios/rp2040_motion_minimal/normal.yaml",
        ),
        "no-board-gate" => Some(
            "cargo run --release --bin rust_plc -- no-board-gate examples/rp2040_motion_minimal.plc --scenario scenarios/rp2040_motion_minimal/normal.yaml --out-dir out/no_board_gate",
        ),
        "pil-run" => Some(
            "cargo run --release --bin rust_plc -- pil-run examples/rp2040_motion_minimal.plc --scenario scenarios/rp2040_motion_minimal/normal.yaml",
        ),
        "virtual-board" => Some(
            "cargo run --release --bin rust_plc -- virtual-board examples/rp2040_motion_minimal.plc --scenario scenarios/rp2040_motion_minimal/normal.yaml --out-dir out/virtual_board",
        ),
        "release-bundle" => Some(
            "cargo run --release --bin rust_plc -- release-bundle examples/rp2040_motion_minimal.plc --scenario scenarios/rp2040_motion_minimal/normal.yaml --out-dir out/release_bundle",
        ),
        _ => None,
    }
}

pub(crate) fn format_resolve_scenario_yaml_error(
    plc_path: &str,
    scenario_path: &Path,
    subcommand: &str,
    err: &str,
) -> String {
    let mut msg = format!(
        "Failed to resolve device-name inputs in scenario {}:\n{err}",
        scenario_path.display()
    );
    if let Some(hint) =
        scenario_mismatch_hint_for_example_paths(plc_path, scenario_path, subcommand)
    {
        msg.push_str("\n\n");
        msg.push_str(&hint);
    }
    msg
}
