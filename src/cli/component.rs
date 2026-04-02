use crate::cli_support::common::{
    DispatchResult,
    CliOutputMode, display_path_relative_to_cwd, write_json_pretty, write_jsonl,
};
use crate::cli_support::help::command_usage;
use rust_plc::component_diagnostics::{ComponentDiagnosisReport, diagnose_component_sim};
use rust_plc::component_scenario::{parse_component_scenario_json, write_component_scenario_json};
use rust_plc::component_sim::{ComponentSimReport, run_component_simulation};
use rust_plc::component_topology::{
    diff_component_topology_semantics, parse_component_topology_json, write_component_topology_json,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn try_dispatch(
    program: &str,
    command: &str,
    remaining: &[String],
) -> Option<DispatchResult> {
    let (error_prefix, result) = match command {
        "component-topology-validate" => (
            "[CTOP-000]",
            run_component_topology_validate_subcommand(program, remaining.iter().cloned()),
        ),
        "component-topology-diff" => (
            "[CTOPDIFF-000]",
            run_component_topology_diff_subcommand(program, remaining.iter().cloned()),
        ),
        "component-scenario-validate" => (
            "[CSCN-000]",
            run_component_scenario_validate_subcommand(program, remaining.iter().cloned()),
        ),
        "component-sim" => (
            "[CSIM-000]",
            run_component_sim_subcommand(program, remaining.iter().cloned()),
        ),
        _ => return None,
    };
    Some(DispatchResult {
        error_prefix: Some(error_prefix),
        result,
    })
}

pub(crate) fn run_component_topology_validate_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "component-topology-validate");
    let Some(topology_path) = args.next() else {
        return Err(usage);
    };
    let topology_path = PathBuf::from(topology_path);
    let mut output_mode = CliOutputMode::Human;
    let mut normalized_out: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--output" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --output <human|json>".to_string())?;
                output_mode = CliOutputMode::parse(&raw).ok_or_else(|| {
                    format!("Invalid --output value `{raw}` (expected `human` or `json`)")
                })?;
            }
            "--normalized-out" => {
                normalized_out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --normalized-out <normalized_topology.json>".to_string()
                })?));
            }
            "-h" | "--help" => return Err(usage.clone()),
            other => {
                return Err(format!(
                    "Unknown argument for component-topology-validate: {other}"
                ));
            }
        }
    }

    let text = fs::read_to_string(&topology_path)
        .map_err(|err| format!("Failed to read {}: {err}", topology_path.display()))?;
    let topology = parse_component_topology_json(&text)
        .map_err(|err| format_component_topology_validate_error(&topology_path, &err))?;

    if let Some(path) = &normalized_out {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|err| {
                    format!(
                        "Failed to create normalized topology directory {}: {err}",
                        parent.display()
                    )
                })?;
            }
        }
        write_component_topology_json(path, &topology)?;
    }

    if output_mode == CliOutputMode::Json {
        #[derive(Serialize)]
        struct ComponentTopologyValidateJson {
            schema_version: u32,
            command: &'static str,
            output: &'static str,
            status: &'static str,
            topology: String,
            normalized_topology: Option<String>,
            component_count: usize,
            connection_count: usize,
        }

        let mut body = serde_json::to_string_pretty(&ComponentTopologyValidateJson {
            schema_version: 1,
            command: "component-topology-validate",
            output: output_mode.as_str(),
            status: "pass",
            topology: display_path_relative_to_cwd(&topology_path),
            normalized_topology: normalized_out
                .as_ref()
                .map(|path| display_path_relative_to_cwd(path)),
            component_count: topology.components.len(),
            connection_count: topology.connections.len(),
        })
        .map_err(|err| format!("Failed to serialize component-topology JSON output: {err}"))?;
        body.push('\n');
        print!("{body}");
    } else {
        eprintln!(
            "component-topology-validate: PASS (components={}, connections={})",
            topology.components.len(),
            topology.connections.len()
        );
        eprintln!("  topology: {}", topology_path.display());
        if let Some(path) = normalized_out {
            eprintln!("  normalized_topology: {}", path.display());
        }
    }
    Ok(())
}

fn format_component_topology_validate_error(
    path: &Path,
    err: &rust_plc::component_topology::ComponentTopologyValidationError,
) -> String {
    let mut msg = format!(
        "component topology validation failed for {} ({} issue(s))",
        path.display(),
        err.issues.len()
    );
    for issue in err.issues.iter().take(8) {
        msg.push_str(&format!(
            "\n- [{}] {}: {}",
            issue.code, issue.path, issue.message
        ));
    }
    if err.issues.len() > 8 {
        msg.push_str(&format!("\n- ... {} more issue(s)", err.issues.len() - 8));
    }
    msg
}

pub(crate) fn run_component_topology_diff_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "component-topology-diff");
    let Some(before_path) = args.next() else {
        return Err(usage);
    };
    let Some(after_path) = args.next() else {
        return Err(usage);
    };
    let before_path = PathBuf::from(before_path);
    let after_path = PathBuf::from(after_path);
    let mut out: Option<PathBuf> = None;
    let mut output_mode = CliOutputMode::Human;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => {
                out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --out <semantic_diff.json>".to_string()
                })?));
            }
            "--output" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --output <human|json>".to_string())?;
                output_mode = CliOutputMode::parse(&raw).ok_or_else(|| {
                    format!("Invalid --output value `{raw}` (expected `human` or `json`)")
                })?;
            }
            "-h" | "--help" => return Err(usage.clone()),
            other => {
                return Err(format!(
                    "Unknown argument for component-topology-diff: {other}"
                ));
            }
        }
    }

    let out = out.ok_or_else(|| usage.clone())?;

    let before_text = fs::read_to_string(&before_path)
        .map_err(|err| format!("Failed to read {}: {err}", before_path.display()))?;
    let before_topology = parse_component_topology_json(&before_text)
        .map_err(|err| format_component_topology_validate_error(&before_path, &err))?;

    let after_text = fs::read_to_string(&after_path)
        .map_err(|err| format!("Failed to read {}: {err}", after_path.display()))?;
    let after_topology = parse_component_topology_json(&after_text)
        .map_err(|err| format_component_topology_validate_error(&after_path, &err))?;

    let report = diff_component_topology_semantics(&before_topology, &after_topology);
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "Failed to create semantic diff output dir {}: {err}",
                    parent.display()
                )
            })?;
        }
    }
    let mut report_json = serde_json::to_string_pretty(&report)
        .map_err(|err| format!("Failed to serialize semantic diff report: {err}"))?;
    report_json.push('\n');
    fs::write(&out, report_json).map_err(|err| {
        format!(
            "Failed to write semantic diff report {}: {err}",
            out.display()
        )
    })?;

    if output_mode == CliOutputMode::Json {
        #[derive(Serialize)]
        struct ComponentTopologyDiffJson {
            schema_version: u32,
            command: &'static str,
            output: &'static str,
            status: &'static str,
            before_topology: String,
            after_topology: String,
            semantic_diff: String,
            changes_detected: bool,
            node_changes: usize,
            port_changes: usize,
            relation_changes: usize,
            tag_changes: usize,
            impact_nodes: usize,
            impact_relations: usize,
        }

        let mut body = serde_json::to_string_pretty(&ComponentTopologyDiffJson {
            schema_version: 1,
            command: "component-topology-diff",
            output: output_mode.as_str(),
            status: "pass",
            before_topology: display_path_relative_to_cwd(&before_path),
            after_topology: display_path_relative_to_cwd(&after_path),
            semantic_diff: display_path_relative_to_cwd(&out),
            changes_detected: !report.is_match,
            node_changes: report.summary.node_changes,
            port_changes: report.summary.port_changes,
            relation_changes: report.summary.relation_changes,
            tag_changes: report.summary.tag_changes,
            impact_nodes: report.impact.blast_radius_nodes.len(),
            impact_relations: report.impact.blast_radius_relations.len(),
        })
        .map_err(|err| format!("Failed to serialize component-topology-diff JSON output: {err}"))?;
        body.push('\n');
        print!("{body}");
    } else {
        eprintln!(
            "component-topology-diff: PASS (changes_detected={}, node_changes={}, port_changes={}, relation_changes={}, tag_changes={})",
            !report.is_match,
            report.summary.node_changes,
            report.summary.port_changes,
            report.summary.relation_changes,
            report.summary.tag_changes
        );
        eprintln!("  before_topology: {}", before_path.display());
        eprintln!("  after_topology: {}", after_path.display());
        eprintln!("  semantic_diff: {}", out.display());
    }

    Ok(())
}

pub(crate) fn run_component_scenario_validate_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "component-scenario-validate");
    let Some(scenario_path) = args.next() else {
        return Err(usage);
    };
    let scenario_path = PathBuf::from(scenario_path);
    let mut output_mode = CliOutputMode::Human;
    let mut normalized_out: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--output" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --output <human|json>".to_string())?;
                output_mode = CliOutputMode::parse(&raw).ok_or_else(|| {
                    format!("Invalid --output value `{raw}` (expected `human` or `json`)")
                })?;
            }
            "--normalized-out" => {
                normalized_out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --normalized-out <normalized_scenario.json>".to_string()
                })?));
            }
            "-h" | "--help" => return Err(usage.clone()),
            other => {
                return Err(format!(
                    "Unknown argument for component-scenario-validate: {other}"
                ));
            }
        }
    }

    let text = fs::read_to_string(&scenario_path)
        .map_err(|err| format!("Failed to read {}: {err}", scenario_path.display()))?;
    let scenario = parse_component_scenario_json(&text)
        .map_err(|err| format_component_scenario_validate_error(&scenario_path, &err))?;

    if let Some(path) = &normalized_out {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|err| {
                    format!(
                        "Failed to create normalized scenario directory {}: {err}",
                        parent.display()
                    )
                })?;
            }
        }
        write_component_scenario_json(path, &scenario)?;
    }

    if output_mode == CliOutputMode::Json {
        #[derive(Serialize)]
        struct ComponentScenarioValidateJson {
            schema_version: u32,
            command: &'static str,
            output: &'static str,
            status: &'static str,
            scenario: String,
            normalized_scenario: Option<String>,
            tick_ms: u64,
            duration_ms: u64,
            switch_event_count: usize,
            sensor_event_count: usize,
            component_fault_count: usize,
        }

        let mut body = serde_json::to_string_pretty(&ComponentScenarioValidateJson {
            schema_version: 1,
            command: "component-scenario-validate",
            output: output_mode.as_str(),
            status: "pass",
            scenario: display_path_relative_to_cwd(&scenario_path),
            normalized_scenario: normalized_out
                .as_ref()
                .map(|path| display_path_relative_to_cwd(path)),
            tick_ms: scenario.tick_ms,
            duration_ms: scenario.duration_ms,
            switch_event_count: scenario.switch_events.len(),
            sensor_event_count: scenario.sensor_events.len(),
            component_fault_count: scenario.component_faults.len(),
        })
        .map_err(|err| format!("Failed to serialize component-scenario JSON output: {err}"))?;
        body.push('\n');
        print!("{body}");
    } else {
        eprintln!(
            "component-scenario-validate: PASS (switch_events={}, sensor_events={}, component_faults={})",
            scenario.switch_events.len(),
            scenario.sensor_events.len(),
            scenario.component_faults.len()
        );
        eprintln!("  scenario: {}", scenario_path.display());
        if let Some(path) = normalized_out {
            eprintln!("  normalized_scenario: {}", path.display());
        }
    }
    Ok(())
}

fn format_component_scenario_validate_error(
    path: &Path,
    err: &rust_plc::component_scenario::ComponentScenarioValidationError,
) -> String {
    let mut msg = format!(
        "component scenario validation failed for {} ({} issue(s))",
        path.display(),
        err.issues.len()
    );
    for issue in err.issues.iter().take(10) {
        msg.push_str(&format!(
            "\n- [{}] {}: {}",
            issue.code, issue.path, issue.message
        ));
    }
    if err
        .issues
        .iter()
        .any(|issue| issue.code.starts_with("CSCN-MIG-"))
    {
        msg.push_str(
            "\nMigration hint: replace legacy `faults.sensor_stuck` and `forces` with `component_faults` events targeted by component ID.",
        );
    }
    if err.issues.len() > 10 {
        msg.push_str(&format!("\n- ... {} more issue(s)", err.issues.len() - 10));
    }
    msg
}

pub(crate) fn run_component_sim_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "component-sim");
    let Some(topology_path) = args.next() else {
        return Err(usage);
    };
    let topology_path = PathBuf::from(topology_path);
    let mut scenario_path: Option<PathBuf> = None;
    let mut trace_out: Option<PathBuf> = None;
    let mut fault_audit_out: Option<PathBuf> = None;
    let mut diagnosis_out: Option<PathBuf> = None;
    let mut keypoints_out: Option<PathBuf> = None;
    let mut output_mode = CliOutputMode::Human;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenario" => {
                scenario_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --scenario <scenario.json>".to_string()
                    })?));
            }
            "--out" => {
                trace_out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --out <component_trace.jsonl>".to_string()
                })?));
            }
            "--fault-audit-out" => {
                fault_audit_out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --fault-audit-out <fault_audit.jsonl>".to_string()
                })?));
            }
            "--diagnosis-out" => {
                diagnosis_out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --diagnosis-out <component_diagnosis.json>".to_string()
                })?));
            }
            "--keypoints-out" => {
                keypoints_out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --keypoints-out <component_keypoints.json>".to_string()
                })?));
            }
            "--output" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --output <human|json>".to_string())?;
                output_mode = CliOutputMode::parse(&raw).ok_or_else(|| {
                    format!("Invalid --output value `{raw}` (expected `human` or `json`)")
                })?;
            }
            "-h" | "--help" => return Err(usage.clone()),
            other => return Err(format!("Unknown argument for component-sim: {other}")),
        }
    }

    let scenario_path = scenario_path.ok_or_else(|| usage.clone())?;
    let topology_text = fs::read_to_string(&topology_path)
        .map_err(|err| format!("Failed to read {}: {err}", topology_path.display()))?;
    let topology = parse_component_topology_json(&topology_text)
        .map_err(|err| format_component_topology_validate_error(&topology_path, &err))?;

    let scenario_text = fs::read_to_string(&scenario_path)
        .map_err(|err| format!("Failed to read {}: {err}", scenario_path.display()))?;
    let scenario = parse_component_scenario_json(&scenario_text)
        .map_err(|err| format_component_scenario_validate_error(&scenario_path, &err))?;

    let report = run_component_simulation(&topology, &scenario)
        .map_err(|err| format_component_sim_error(&err))?;

    let diagnosis = diagnose_component_sim(&report);
    let keypoints = collect_component_keypoints(&scenario, &report, &diagnosis);

    if let Some(path) = &trace_out {
        write_jsonl(path, report.ticks.iter())?;
    }
    if let Some(path) = &fault_audit_out {
        write_jsonl(path, report.fault_audit.iter())?;
    }
    if let Some(path) = &diagnosis_out {
        write_json_pretty(path, &diagnosis)?;
    }
    if let Some(path) = &keypoints_out {
        let payload = ComponentKeypointArtifact {
            schema_version: 1,
            tick_ms: scenario.tick_ms,
            keypoints: keypoints.clone(),
        };
        write_json_pretty(path, &payload)?;
    }

    if output_mode == CliOutputMode::Json {
        #[derive(Serialize)]
        struct ComponentSimJson {
            schema_version: u32,
            command: &'static str,
            output: &'static str,
            status: &'static str,
            topology: String,
            scenario: String,
            tick_count: usize,
            fault_audit_count: usize,
            diagnosis_count: usize,
            keypoint_count: usize,
            trace_out: Option<String>,
            fault_audit_out: Option<String>,
            diagnosis_out: Option<String>,
            keypoints_out: Option<String>,
        }

        let mut body = serde_json::to_string_pretty(&ComponentSimJson {
            schema_version: 1,
            command: "component-sim",
            output: output_mode.as_str(),
            status: "pass",
            topology: display_path_relative_to_cwd(&topology_path),
            scenario: display_path_relative_to_cwd(&scenario_path),
            tick_count: report.ticks.len(),
            fault_audit_count: report.fault_audit.len(),
            diagnosis_count: diagnosis.candidates.len(),
            keypoint_count: keypoints.len(),
            trace_out: trace_out
                .as_ref()
                .map(|path| display_path_relative_to_cwd(path)),
            fault_audit_out: fault_audit_out
                .as_ref()
                .map(|path| display_path_relative_to_cwd(path)),
            diagnosis_out: diagnosis_out
                .as_ref()
                .map(|path| display_path_relative_to_cwd(path)),
            keypoints_out: keypoints_out
                .as_ref()
                .map(|path| display_path_relative_to_cwd(path)),
        })
        .map_err(|err| format!("Failed to serialize component-sim JSON output: {err}"))?;
        body.push('\n');
        print!("{body}");
    } else {
        eprintln!(
            "component-sim: PASS (ticks={}, fault_audit={}, diagnosis={}, keypoints={})",
            report.ticks.len(),
            report.fault_audit.len(),
            diagnosis.candidates.len(),
            keypoints.len(),
        );
        eprintln!("  topology: {}", topology_path.display());
        eprintln!("  scenario: {}", scenario_path.display());
        if let Some(path) = trace_out {
            eprintln!("  trace: {}", path.display());
        }
        if let Some(path) = fault_audit_out {
            eprintln!("  fault_audit: {}", path.display());
        }
        if let Some(path) = diagnosis_out {
            eprintln!("  diagnosis: {}", path.display());
        }
        if let Some(path) = keypoints_out {
            eprintln!("  keypoints: {}", path.display());
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ComponentKeypointArtifact {
    schema_version: u32,
    tick_ms: u64,
    keypoints: Vec<ComponentKeypoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ComponentKeypoint {
    tick: u64,
    at_ms: u64,
    category: String,
    source: String,
    label: String,
}

fn collect_component_keypoints(
    scenario: &rust_plc::component_scenario::ComponentScenario,
    report: &ComponentSimReport,
    diagnosis: &ComponentDiagnosisReport,
) -> Vec<ComponentKeypoint> {
    let tick_ms = scenario.tick_ms.max(1);
    let mut out = Vec::<ComponentKeypoint>::new();
    let mut seen = BTreeSet::<String>::new();
    let mut push_unique = |tick: u64, at_ms: u64, category: &str, source: &str, label: String| {
        let key = format!("{tick}|{category}|{label}");
        if seen.insert(key) {
            out.push(ComponentKeypoint {
                tick,
                at_ms,
                category: category.to_string(),
                source: source.to_string(),
                label,
            });
        }
    };

    for event in &scenario.switch_events {
        push_unique(
            event.at_ms / tick_ms,
            event.at_ms,
            "switch_event",
            "scenario",
            format!("switch `{}` -> {}", event.target, event.value),
        );
    }
    for event in &scenario.sensor_events {
        push_unique(
            event.at_ms / tick_ms,
            event.at_ms,
            "sensor_event",
            "scenario",
            format!("sensor `{}` -> {}", event.target, event.value),
        );
    }
    for event in &scenario.component_faults {
        push_unique(
            event.at_ms / tick_ms,
            event.at_ms,
            "fault_planned",
            "scenario",
            format!(
                "fault `{}` on `{}`",
                serde_json::to_string(&event.fault_kind)
                    .unwrap_or_else(|_| "\"unknown\"".to_string())
                    .trim_matches('"'),
                event.target_component_id
            ),
        );
    }
    for entry in &report.fault_audit {
        push_unique(
            entry.tick,
            entry.at_ms,
            if entry.action == "activated" {
                "fault_activated"
            } else {
                "fault_expired"
            },
            "runtime_fault_audit",
            format!(
                "fault `{}` {} on `{}`",
                serde_json::to_string(&entry.fault_kind)
                    .unwrap_or_else(|_| "\"unknown\"".to_string())
                    .trim_matches('"'),
                entry.action,
                entry.target_component_id
            ),
        );
    }
    for candidate in &diagnosis.candidates {
        let Some(context) = &candidate.fault_context else {
            continue;
        };
        push_unique(
            context.start_ms / tick_ms,
            context.start_ms,
            "diagnosis_fault_context_start",
            "diagnosis",
            format!("{} ({})", context.component_id, candidate.issue_code),
        );
        if let Some(end_ms) = context.end_ms {
            push_unique(
                end_ms / tick_ms,
                end_ms,
                "diagnosis_fault_context_end",
                "diagnosis",
                format!("{} ({})", context.component_id, candidate.issue_code),
            );
        }
    }

    out.sort_by(|a, b| {
        a.tick
            .cmp(&b.tick)
            .then_with(|| {
                keypoint_category_priority(&a.category)
                    .cmp(&keypoint_category_priority(&b.category))
            })
            .then_with(|| a.label.cmp(&b.label))
    });
    out
}

fn keypoint_category_priority(category: &str) -> u8 {
    match category {
        "switch_event" | "sensor_event" => 1,
        "fault_planned" => 2,
        "fault_activated" => 3,
        "fault_expired" => 4,
        "diagnosis_fault_context_start" | "diagnosis_fault_context_end" => 5,
        _ => 6,
    }
}

fn format_component_sim_error(err: &rust_plc::component_sim::ComponentSimError) -> String {
    let mut msg = format!(
        "component simulation failed ({} issue(s))",
        err.issues.len()
    );
    for issue in err.issues.iter().take(10) {
        msg.push_str(&format!(
            "\n- [{}] {}: {}",
            issue.code, issue.path, issue.message
        ));
    }
    if err.issues.len() > 10 {
        msg.push_str(&format!("\n- ... {} more issue(s)", err.issues.len() - 10));
    }
    msg
}
