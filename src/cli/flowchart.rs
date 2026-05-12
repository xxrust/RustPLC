use crate::cli_support::common::{
    CliOutputMode, DispatchResult, display_path_relative_to_cwd, write_json_pretty,
};
use crate::cli_support::help::command_usage;
use crate::cli_support::plc_pipeline::{
    format_loaded_plc_errors, parse_loaded_plc_with_required_purpose,
};
use petgraph::visit::EdgeRef;
use rust_plc::ast::{
    ActionStatement, ConditionExpression, EffectKind, EffectStatement, Expression, LiteralValue,
    PlcProgram, StepStatement, WaitCondition,
};
use rust_plc::ir::{
    StateMachine, TopologyGraph, Transition, TransitionAction, TransitionGuard, WorkpieceEffect,
};
use rust_plc::semantic::{
    build_constraint_set, build_state_machine, build_topology_graph,
    preprocess_program_with_library,
};
use rust_plc::source_bundle::{is_supported_plc_source_path, load_plc_source, plc_source_stem};
use rust_plc::topology_semantic_gate::{
    collect_topology_deprecation_warnings, validate_topology_semantics,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn try_dispatch(
    program: &str,
    command: &str,
    remaining: &[String],
) -> Option<DispatchResult> {
    let result = match command {
        "flowchart" => run_flowchart_subcommand(program, remaining.iter().cloned()),
        _ => return None,
    };
    Some(DispatchResult {
        error_prefix: Some("[FLOW-000]"),
        result,
    })
}

#[derive(Debug, Serialize)]
struct FlowchartCliReport {
    schema_version: u32,
    command: &'static str,
    output: &'static str,
    source_plc: String,
    out_dir: String,
    html_path: String,
    json_path: String,
    task_count: usize,
    step_count: usize,
    transition_count: usize,
}

#[derive(Debug, Serialize)]
struct FlowchartArtifact {
    schema_version: u32,
    source_plc: String,
    title: String,
    tasks: Vec<TaskDiagram>,
    topology: TopologySummary,
}

#[derive(Debug, Serialize)]
struct TaskDiagram {
    task_name: String,
    steps: Vec<StepSummary>,
    transitions: Vec<EdgeSummary>,
}

#[derive(Debug, Clone, Serialize)]
struct StepSummary {
    task_name: String,
    step_name: String,
    source: Option<String>,
    line: Option<usize>,
    generated: bool,
    statements: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct EdgeSummary {
    from_task: String,
    from_step: String,
    to_task: String,
    to_step: String,
    label: String,
    guard: String,
    actions: Vec<String>,
    effects: Vec<String>,
}

#[derive(Debug, Serialize)]
struct TopologySummary {
    device_count: usize,
    link_count: usize,
    variables: Vec<String>,
    workpiece_sites: Vec<String>,
    workpiece_holders: Vec<String>,
    workpiece_types: Vec<String>,
    links: Vec<String>,
}

fn run_flowchart_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "flowchart");
    let Some(plc_path) = args.next() else {
        return Err(usage);
    };

    let mut out_dir: Option<PathBuf> = None;
    let mut title: Option<String> = None;
    let mut output_mode = CliOutputMode::Human;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out-dir" => {
                let value = args
                    .next()
                    .ok_or_else(|| "Missing value for --out-dir <dir>".to_string())?;
                out_dir = Some(PathBuf::from(value));
            }
            "--title" => {
                title = Some(
                    args.next()
                        .ok_or_else(|| "Missing value for --title <title>".to_string())?,
                );
            }
            "--output" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --output <human|json>".to_string())?;
                output_mode = CliOutputMode::parse(&raw).ok_or_else(|| {
                    format!("Invalid value for --output `{raw}` (expected human or json)")
                })?;
            }
            "-h" | "--help" => return Err(usage.clone()),
            other => return Err(format!("Unknown argument for flowchart: {other}\n{usage}")),
        }
    }

    let plc_path_ref = Path::new(&plc_path);
    if !is_supported_plc_source_path(plc_path_ref) {
        return Err(format!(
            "flowchart expects a supported PLC source path, got: {plc_path}"
        ));
    }

    let out_dir = out_dir.unwrap_or_else(|| {
        Path::new("out")
            .join("flowchart")
            .join(plc_source_stem(plc_path_ref))
    });
    fs::create_dir_all(&out_dir).map_err(|err| {
        format!(
            "Failed to create output directory {}: {err}",
            out_dir.display()
        )
    })?;

    let loaded = load_plc_source(plc_path_ref)?;
    let (expanded, topology, state_machine) = compile_flowchart_inputs(&loaded)?;
    let model = build_flowchart_artifact(
        &loaded,
        &expanded,
        &topology,
        &state_machine,
        title.unwrap_or_else(|| plc_source_stem(plc_path_ref)),
    );

    let html_path = out_dir.join("index.html");
    let json_path = out_dir.join("flowchart.json");
    fs::write(&html_path, render_html(&model))
        .map_err(|err| format!("Failed to write {}: {err}", html_path.display()))?;
    write_json_pretty(&json_path, &model)?;

    let report = FlowchartCliReport {
        schema_version: 1,
        command: "flowchart",
        output: output_mode.as_str(),
        source_plc: plc_path,
        out_dir: display_path_relative_to_cwd(&out_dir),
        html_path: display_path_relative_to_cwd(&html_path),
        json_path: display_path_relative_to_cwd(&json_path),
        task_count: model.tasks.len(),
        step_count: model.tasks.iter().map(|task| task.steps.len()).sum(),
        transition_count: state_machine.transitions.len(),
    };

    match output_mode {
        CliOutputMode::Human => {
            eprintln!("flowchart: PASS");
            eprintln!("  source: {}", report.source_plc);
            eprintln!("  html: {}", report.html_path);
            eprintln!("  json: {}", report.json_path);
            eprintln!(
                "  tasks/steps/transitions: {}/{}/{}",
                report.task_count, report.step_count, report.transition_count
            );
        }
        CliOutputMode::Json => {
            let mut body = serde_json::to_string_pretty(&report)
                .map_err(|err| format!("Failed to serialize flowchart JSON report: {err}"))?;
            body.push('\n');
            print!("{body}");
        }
    }

    Ok(())
}

fn compile_flowchart_inputs(
    loaded: &rust_plc::source_bundle::LoadedPlcSource,
) -> Result<(PlcProgram, TopologyGraph, StateMachine), String> {
    let parsed = parse_loaded_plc_with_required_purpose(loaded)?;
    for warning in collect_topology_deprecation_warnings(&parsed.topology) {
        eprintln!("WARNING [topology] {warning}");
    }

    let devices_dir = Path::new("devices");
    let device_library =
        rust_plc::device_library::DeviceLibrary::load(devices_dir).map_err(|errors| {
            errors
                .into_iter()
                .map(|err| err.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        })?;
    let expanded = preprocess_program_with_library(
        &parsed,
        if device_library.is_empty() {
            None
        } else {
            Some(&device_library)
        },
    )
    .map_err(|errors| format_loaded_plc_errors(errors, loaded).join("\n"))?;
    validate_topology_semantics(&expanded.topology).map_err(|gate_error| gate_error.to_string())?;

    let mut errors = Vec::new();
    let topology = match build_topology_graph(&expanded) {
        Ok(value) => Some(value),
        Err(mut stage_errors) => {
            errors.append(&mut stage_errors);
            None
        }
    };
    let state_machine = match build_state_machine(&expanded) {
        Ok(value) => Some(value),
        Err(mut stage_errors) => {
            errors.append(&mut stage_errors);
            None
        }
    };
    if let Err(mut stage_errors) = build_constraint_set(&expanded) {
        errors.append(&mut stage_errors);
    }
    if !errors.is_empty() {
        return Err(format_loaded_plc_errors(errors, loaded).join("\n"));
    }

    Ok((
        expanded,
        topology.expect("topology exists when semantic errors are empty"),
        state_machine.expect("state machine exists when semantic errors are empty"),
    ))
}

fn build_flowchart_artifact(
    loaded: &rust_plc::source_bundle::LoadedPlcSource,
    program: &PlcProgram,
    topology: &TopologyGraph,
    state_machine: &StateMachine,
    title: String,
) -> FlowchartArtifact {
    let step_lookup = build_step_lookup(loaded, program);
    let mut task_names = BTreeSet::new();
    for task in &program.tasks.tasks {
        task_names.insert(task.name.clone());
    }
    for state in &state_machine.states {
        task_names.insert(state.task_name.clone());
    }

    let mut transitions_by_task = BTreeMap::<String, Vec<EdgeSummary>>::new();
    for transition in &state_machine.transitions {
        let edge = summarize_transition(transition);
        transitions_by_task
            .entry(edge.from_task.clone())
            .or_default()
            .push(edge);
    }

    let mut tasks = Vec::new();
    for task_name in task_names {
        let mut step_order = Vec::new();
        let mut seen_steps = BTreeSet::new();
        for task in program
            .tasks
            .tasks
            .iter()
            .filter(|task| task.name == task_name)
        {
            for step in &task.steps {
                if seen_steps.insert(step.name.clone()) {
                    step_order.push(step.name.clone());
                }
            }
        }
        let mut generated_step_names = BTreeSet::new();
        for state in state_machine
            .states
            .iter()
            .filter(|state| state.task_name == task_name)
        {
            if !seen_steps.contains(&state.step_name) {
                generated_step_names.insert(state.step_name.clone());
            }
        }
        step_order.extend(generated_step_names);

        let steps = step_order
            .into_iter()
            .map(|step_name| {
                step_lookup
                    .get(&(task_name.clone(), step_name.clone()))
                    .cloned()
                    .unwrap_or_else(|| StepSummary {
                        task_name: task_name.clone(),
                        step_name,
                        source: None,
                        line: None,
                        generated: true,
                        statements: vec!["generated semantic state".to_string()],
                    })
            })
            .collect::<Vec<_>>();
        let transitions = transitions_by_task
            .remove(&task_name)
            .unwrap_or_default()
            .into_iter()
            .filter(|edge| edge.from_task == task_name)
            .collect::<Vec<_>>();
        tasks.push(TaskDiagram {
            task_name,
            steps,
            transitions,
        });
    }

    FlowchartArtifact {
        schema_version: 1,
        source_plc: loaded.requested_path.display().to_string(),
        title,
        tasks,
        topology: summarize_topology(program, topology),
    }
}

fn build_step_lookup(
    loaded: &rust_plc::source_bundle::LoadedPlcSource,
    program: &PlcProgram,
) -> HashMap<(String, String), StepSummary> {
    let mut lookup = HashMap::new();
    for task in &program.tasks.tasks {
        for step in &task.steps {
            let location = loaded.source_map.remap_location(step.line.max(1), 1);
            lookup.insert(
                (task.name.clone(), step.name.clone()),
                StepSummary {
                    task_name: task.name.clone(),
                    step_name: step.name.clone(),
                    source: location.as_ref().map(|loc| loc.file.clone()),
                    line: location.map(|loc| loc.line),
                    generated: false,
                    statements: step
                        .statements
                        .iter()
                        .map(summarize_step_statement)
                        .collect(),
                },
            );
        }
    }
    lookup
}

fn summarize_transition(transition: &Transition) -> EdgeSummary {
    let guard = summarize_guard(&transition.guard);
    let actions = transition
        .actions
        .iter()
        .map(summarize_transition_action)
        .collect::<Vec<_>>();
    let effects = transition
        .effects
        .iter()
        .map(summarize_workpiece_effect)
        .collect::<Vec<_>>();
    let mut parts = Vec::new();
    if guard != "always" {
        parts.push(guard.clone());
    }
    parts.extend(actions.iter().take(2).cloned());
    parts.extend(effects.iter().take(2).cloned());
    let label = if parts.is_empty() {
        "next".to_string()
    } else {
        parts.join("; ")
    };
    EdgeSummary {
        from_task: transition.from.task_name.clone(),
        from_step: transition.from.step_name.clone(),
        to_task: transition.to.task_name.clone(),
        to_step: transition.to.step_name.clone(),
        label,
        guard,
        actions,
        effects,
    }
}

fn summarize_topology(program: &PlcProgram, topology: &TopologyGraph) -> TopologySummary {
    let mut links = Vec::new();
    for edge in topology.graph.edge_references() {
        let from = &topology.graph[edge.source()].name;
        let to = &topology.graph[edge.target()].name;
        links.push(format!("{from} -> {to} ({:?})", edge.weight()));
    }
    for link in &topology.links {
        links.push(format!("{} -> {} ({:?})", link.from, link.to, link.kind));
    }
    links.sort();
    links.dedup();

    TopologySummary {
        device_count: topology.graph.node_count(),
        link_count: links.len(),
        variables: program
            .topology
            .variables
            .iter()
            .map(|var| format!("{}:{:?}={}", var.name, var.var_type, var.initial_value))
            .collect(),
        workpiece_sites: program
            .topology
            .workpiece_sites
            .iter()
            .map(|site| format!("{}:{:?}:capacity={}", site.name, site.kind, site.capacity))
            .collect(),
        workpiece_holders: program
            .topology
            .workpiece_holders
            .iter()
            .map(|holder| format!("{}:capacity={}", holder.name, holder.capacity))
            .collect(),
        workpiece_types: program
            .topology
            .workpiece_types
            .iter()
            .map(|ty| ty.name.clone())
            .collect(),
        links,
    }
}

fn render_task_svg(task: &TaskDiagram) -> String {
    #[derive(Clone)]
    struct TransitionLayout {
        guard_lines: Vec<String>,
        fact_lines: Vec<String>,
        target_lines: Vec<String>,
        show_note: bool,
        target_external: bool,
        terminal: bool,
        note_h: i32,
        slot_h: i32,
    }

    struct StepLayout {
        step_name_lines: Vec<String>,
        source_lines: Vec<String>,
        body_lines: Vec<String>,
        transitions: Vec<TransitionLayout>,
        step_h: i32,
        action_h: i32,
        core_h: i32,
        y: i32,
    }

    let step_index = task
        .steps
        .iter()
        .enumerate()
        .map(|(idx, step)| (step.step_name.clone(), idx))
        .collect::<HashMap<_, _>>();
    let step_count = task.steps.len();

    let mut internal_by_source = BTreeMap::<usize, Vec<&EdgeSummary>>::new();
    let mut internal_by_target = BTreeMap::<usize, Vec<&EdgeSummary>>::new();
    let mut outgoing_by_source = BTreeMap::<usize, Vec<&EdgeSummary>>::new();
    for edge in &task.transitions {
        let Some(&from_idx) = step_index.get(&edge.from_step) else {
            continue;
        };
        outgoing_by_source.entry(from_idx).or_default().push(edge);
        if edge.to_task == task.task_name {
            let Some(&to_idx) = step_index.get(&edge.to_step) else {
                continue;
            };
            internal_by_source.entry(from_idx).or_default().push(edge);
            internal_by_target.entry(to_idx).or_default().push(edge);
        }
    }
    for edges in outgoing_by_source.values_mut() {
        edges.sort_by_key(|edge| {
            let to_idx = if edge.to_task == task.task_name {
                step_index.get(&edge.to_step).copied().unwrap_or(usize::MAX)
            } else {
                usize::MAX
            };
            (
                edge.to_task != task.task_name,
                to_idx,
                edge.guard.contains("timeout"),
                edge.guard.clone(),
                edge.to_task.clone(),
                edge.to_step.clone(),
            )
        });
    }
    for edges in internal_by_source.values_mut() {
        edges.sort_by_key(|edge| {
            (
                step_index.get(&edge.to_step).copied().unwrap_or(usize::MAX),
                edge.guard.clone(),
                edge.to_step.clone(),
            )
        });
    }

    let mut lanes = vec![0i32; step_count];
    let mut lane_assigned = vec![false; step_count];
    if step_count > 0 {
        lane_assigned[0] = true;
    }
    let mut next_lane = 1i32;
    for idx in 0..step_count {
        if !lane_assigned[idx] {
            let pred_lanes = internal_by_target
                .get(&idx)
                .into_iter()
                .flat_map(|edges| edges.iter())
                .filter_map(|edge| {
                    let from_idx = step_index.get(&edge.from_step).copied()?;
                    lane_assigned[from_idx].then_some(lanes[from_idx])
                })
                .collect::<Vec<_>>();
            lanes[idx] = pred_lanes.iter().min().copied().unwrap_or(0);
            lane_assigned[idx] = true;
        }

        let mut unique_forward_targets = Vec::new();
        let mut seen_targets = BTreeSet::new();
        if let Some(edges) = internal_by_source.get(&idx) {
            for edge in edges {
                let Some(&to_idx) = step_index.get(&edge.to_step) else {
                    continue;
                };
                if to_idx > idx && seen_targets.insert(to_idx) {
                    unique_forward_targets.push(to_idx);
                }
            }
        }
        unique_forward_targets.sort_unstable();
        for (order, to_idx) in unique_forward_targets.into_iter().enumerate() {
            if lane_assigned[to_idx] {
                continue;
            }
            lanes[to_idx] = if order == 0 { lanes[idx] } else { next_lane };
            if order != 0 {
                next_lane += 1;
            }
            lane_assigned[to_idx] = true;
        }
    }

    let step_w = 220;
    let action_w = 280;
    let cluster_gap = 28;
    let lane_gap = 560;
    let left_margin = 72;
    let top_y = 72;
    let max_lane = lanes.iter().copied().max().unwrap_or(0);
    let chart_w = left_margin + max_lane * lane_gap + step_w + cluster_gap + action_w;

    let mut layouts = Vec::new();
    let mut cursor_y = top_y;
    for (idx, step) in task.steps.iter().enumerate() {
        let source = match (&step.source, step.line) {
            (Some(source), Some(line)) => format!("{}:{}", short_source(source), line),
            _ if step.generated => "generated semantic state".to_string(),
            _ => "source".to_string(),
        };
        let step_name_lines = wrap_text_lines(&step.step_name, 21, 3);
        let source_lines = wrap_text_lines(&source, 34, 2);
        let step_h =
            30 + (step_name_lines.len() as i32 * 22) + (source_lines.len() as i32 * 16) + 18;

        let body_lines = flatten_statement_lines(&step_body_lines(&step.statements), 34, 10);
        let action_h = if body_lines.is_empty() {
            0
        } else {
            18 + (body_lines.len() as i32 * 16) + 16
        };

        let outgoing = outgoing_by_source.get(&idx).cloned().unwrap_or_default();
        let has_multiple_outgoing = outgoing.len() > 1;
        let mut transition_layouts = outgoing
            .into_iter()
            .map(|edge| {
                let fact_lines = transition_fact_lines(edge, &step.statements);
                let guard_lines = if edge.guard == "always" {
                    Vec::new()
                } else {
                    wrap_text_lines(&edge.guard, 34, 3)
                };
                let internal_target_idx = if edge.to_task == task.task_name {
                    step_index.get(&edge.to_step).copied()
                } else {
                    None
                };
                let show_target = if edge.to_task != task.task_name {
                    true
                } else {
                    match internal_target_idx {
                        Some(to_idx) => {
                            to_idx <= idx
                                || to_idx != idx + 1
                                || lanes[to_idx] != lanes[idx]
                                || has_multiple_outgoing
                        }
                        None => true,
                    }
                };
                let target_lines = if show_target {
                    wrap_text_lines(&format!("goto {}.{}", edge.to_task, edge.to_step), 34, 2)
                } else {
                    Vec::new()
                };
                let show_note =
                    !guard_lines.is_empty() || !fact_lines.is_empty() || !target_lines.is_empty();
                let mut note_h = 0;
                if show_note {
                    note_h += 14;
                    note_h += guard_lines.len() as i32 * 16;
                    if !fact_lines.is_empty() {
                        if note_h > 14 {
                            note_h += 6;
                        }
                        note_h += fact_lines.len() as i32 * 14;
                    }
                    if !target_lines.is_empty() {
                        if note_h > 14 {
                            note_h += 6;
                        }
                        note_h += target_lines.len() as i32 * 14;
                    }
                    note_h += 12;
                }
                TransitionLayout {
                    guard_lines,
                    fact_lines,
                    target_lines,
                    show_note,
                    target_external: edge.to_task != task.task_name,
                    terminal: false,
                    note_h,
                    slot_h: if show_note { note_h + 30 } else { 30 },
                }
            })
            .collect::<Vec<_>>();
        if transition_layouts.is_empty() {
            transition_layouts.push(TransitionLayout {
                guard_lines: Vec::new(),
                fact_lines: Vec::new(),
                target_lines: vec!["END".to_string()],
                show_note: true,
                target_external: false,
                terminal: true,
                note_h: 40,
                slot_h: 70,
            });
        }
        let transition_total_h = transition_layouts
            .iter()
            .map(|item| item.slot_h)
            .sum::<i32>();
        let core_h = step_h.max(action_h);
        let row_h = core_h + 18 + transition_total_h + 24;
        layouts.push(StepLayout {
            step_name_lines,
            source_lines,
            body_lines,
            transitions: transition_layouts,
            step_h,
            action_h,
            core_h,
            y: cursor_y,
        });
        cursor_y += row_h;
    }

    let width = chart_w + 56;
    let height = cursor_y + 16;
    let mut connectors = String::new();
    let mut content = String::new();

    let lane_center_x = |lane: i32| left_margin + lane * lane_gap + step_w / 2;
    let step_left_x = |lane: i32| left_margin + lane * lane_gap;
    let note_x = |step_x: i32| step_x + step_w + cluster_gap;
    let entry_gap = |idx: usize| {
        let incoming = internal_by_target.get(&idx).map_or(0, Vec::len);
        if incoming > 1 {
            54
        } else if incoming == 1 {
            22
        } else {
            0
        }
    };

    for idx in 0..step_count {
        if !internal_by_target.contains_key(&idx) {
            continue;
        }
        let center_x = lane_center_x(lanes[idx]);
        let entry_y = layouts[idx].y - entry_gap(idx);
        let _ = writeln!(
            connectors,
            r#"<line class="task-main-line" x1="{center_x}" y1="{entry_y}" x2="{center_x}" y2="{}"/>"#,
            layouts[idx].y
        );
        if internal_by_target.get(&idx).map_or(0, Vec::len) > 1 {
            let _ = writeln!(
                connectors,
                r#"<line class="task-transition-branch-bar" x1="{}" y1="{entry_y}" x2="{}" y2="{entry_y}"/>"#,
                center_x - 24,
                center_x + 24
            );
        }
    }

    for idx in 0..step_count {
        let Some(edges) = outgoing_by_source.get(&idx) else {
            continue;
        };
        let source_center_x = lane_center_x(lanes[idx]);
        let source_bottom_y = layouts[idx].y + layouts[idx].step_h;
        let mut transition_cursor = layouts[idx].y + layouts[idx].core_h + 18;
        let last_branch_y = transition_cursor
            + layouts[idx]
                .transitions
                .iter()
                .map(|item| item.slot_h)
                .sum::<i32>()
            - layouts[idx]
                .transitions
                .last()
                .map(|item| item.slot_h)
                .unwrap_or(0)
            + 12;
        let _ = writeln!(
            connectors,
            r#"<line class="task-main-line" x1="{source_center_x}" y1="{source_bottom_y}" x2="{source_center_x}" y2="{last_branch_y}"/>"#
        );

        for (edge_idx, edge) in edges.iter().enumerate() {
            let transition = &layouts[idx].transitions[edge_idx];
            let branch_y = transition_cursor + 12;
            let note_left = note_x(step_left_x(lanes[idx]));

            let _ = writeln!(
                connectors,
                r#"<line class="task-transition-bar" x1="{}" y1="{branch_y}" x2="{}" y2="{branch_y}"/>"#,
                source_center_x - 38,
                source_center_x + 38
            );

            if edge.to_task == task.task_name {
                let Some(&to_idx) = step_index.get(&edge.to_step) else {
                    transition_cursor += transition.slot_h;
                    continue;
                };
                let target_center_x = lane_center_x(lanes[to_idx]);
                let target_entry_y = layouts[to_idx].y - entry_gap(to_idx);

                if to_idx > idx {
                    if target_center_x != source_center_x {
                        let _ = writeln!(
                            connectors,
                            r#"<line class="task-transition-branch-bus" x1="{}" y1="{branch_y}" x2="{}" y2="{branch_y}"/>"#,
                            source_center_x.min(target_center_x),
                            source_center_x.max(target_center_x)
                        );
                    }
                    if branch_y != target_entry_y {
                        let _ = writeln!(
                            connectors,
                            r#"<line class="task-main-line" x1="{target_center_x}" y1="{branch_y}" x2="{target_center_x}" y2="{target_entry_y}"/>"#
                        );
                    }
                } else {
                    let loop_x = left_margin - 42 - lanes[idx] * 24 - edge_idx as i32 * 12;
                    let _ = writeln!(
                        connectors,
                        r#"<line class="task-transition-branch-bus" x1="{loop_x}" y1="{branch_y}" x2="{source_center_x}" y2="{branch_y}"/>"#
                    );
                    let _ = writeln!(
                        connectors,
                        r#"<line class="task-main-line" x1="{loop_x}" y1="{target_entry_y}" x2="{loop_x}" y2="{branch_y}"/>"#
                    );
                    let _ = writeln!(
                        connectors,
                        r#"<line class="task-transition-branch-bus" x1="{loop_x}" y1="{target_entry_y}" x2="{target_center_x}" y2="{target_entry_y}"/>"#
                    );
                }
            } else if transition.show_note {
                let _ = writeln!(
                    connectors,
                    r#"<line class="task-transition-link" x1="{}" y1="{branch_y}" x2="{}" y2="{branch_y}"/>"#,
                    source_center_x + 40,
                    note_left - 12
                );
            }
            transition_cursor += transition.slot_h;
        }
    }

    for (idx, step) in task.steps.iter().enumerate() {
        let layout = &layouts[idx];
        let step_x = step_left_x(lanes[idx]);
        let step_y = layout.y;
        let action_x = note_x(step_x);
        let action_y = layout.y + (layout.core_h - layout.action_h) / 2;
        let step_center_y = step_y + layout.step_h / 2 + 4;

        if idx == 0 {
            let _ = writeln!(
                content,
                r#"<rect class="task-step-initial" x="{}" y="{}" width="{}" height="{}" rx="10"/>"#,
                step_x - 8,
                step_y - 8,
                step_w + 16,
                layout.step_h + 16
            );
        }

        let step_class = if step.generated {
            "task-step generated"
        } else {
            "task-step"
        };
        let _ = writeln!(
            content,
            r#"<rect class="{step_class}" x="{step_x}" y="{step_y}" width="{step_w}" height="{}" rx="10"/>"#,
            layout.step_h
        );
        let _ = writeln!(
            content,
            r#"<text class="task-step-index" x="{}" y="{step_center_y}">{}</text>"#,
            step_x - 48,
            idx + 1
        );
        render_svg_lines(
            &mut content,
            step_x + 16,
            step_y + 28,
            &layout.step_name_lines,
            "task-step-title",
            20,
            "start",
        );
        render_svg_lines(
            &mut content,
            step_x + 16,
            step_y + 56 + ((layout.step_name_lines.len() as i32 - 1).max(0) * 20),
            &layout.source_lines,
            "task-step-source",
            15,
            "start",
        );

        if layout.action_h > 0 {
            let _ = writeln!(
                content,
                r#"<line class="task-action-link" x1="{}" y1="{}" x2="{}" y2="{}"/>"#,
                step_x + step_w,
                step_y + layout.step_h / 2,
                action_x,
                action_y + layout.action_h / 2
            );
            let _ = writeln!(
                content,
                r#"<rect class="task-action" x="{action_x}" y="{action_y}" width="{action_w}" height="{}" rx="10"/>"#,
                layout.action_h
            );
            render_svg_lines(
                &mut content,
                action_x + 16,
                action_y + 28,
                &layout.body_lines,
                "task-action-line",
                16,
                "start",
            );
        }

        let mut transition_cursor = layout.y + layout.core_h + 18;
        for transition in &layout.transitions {
            if transition.show_note {
                let note_top = transition_cursor + 20;
                let note_class = if transition.target_external {
                    "task-transition-note external"
                } else if transition.terminal {
                    "task-transition-note terminal"
                } else {
                    "task-transition-note"
                };
                let _ = writeln!(
                    content,
                    r#"<rect class="{note_class}" x="{action_x}" y="{note_top}" width="{action_w}" height="{}" rx="10"/>"#,
                    transition.note_h
                );
                let mut line_y = note_top + 18;
                if !transition.guard_lines.is_empty() {
                    render_svg_lines(
                        &mut content,
                        action_x + 16,
                        line_y,
                        &transition.guard_lines,
                        "task-transition-guard",
                        16,
                        "start",
                    );
                    line_y += transition.guard_lines.len() as i32 * 16 + 6;
                }
                if !transition.fact_lines.is_empty() {
                    render_svg_lines(
                        &mut content,
                        action_x + 16,
                        line_y,
                        &transition.fact_lines,
                        "task-transition-fact",
                        14,
                        "start",
                    );
                    line_y += transition.fact_lines.len() as i32 * 14 + 6;
                }
                let target_class = if transition.target_external {
                    "task-transition-target external"
                } else if transition.terminal {
                    "task-transition-target terminal"
                } else {
                    "task-transition-target"
                };
                render_svg_lines(
                    &mut content,
                    action_x + 16,
                    line_y,
                    &transition.target_lines,
                    target_class,
                    14,
                    "start",
                );
            }
            transition_cursor += transition.slot_h;
        }
    }

    let mut out = String::new();
    let _ = write!(
        out,
        r#"<div class="sfc-review"><svg class="task-sfc-svg" viewBox="0 0 {width} {height}" role="img" aria-label="{} SFC chart" xmlns="http://www.w3.org/2000/svg">"#,
        html_escape(&task.task_name)
    );
    out.push_str(&connectors);
    out.push_str(&content);
    out.push_str("</svg></div>");
    out
}

#[allow(dead_code)]
fn render_overview_svg(model: &FlowchartArtifact) -> String {
    let node_w = 260;
    let node_h = 62;
    let col_left_x = 52;
    let col_right_x = 420;
    let gap_y = 38;
    let top_y = 44;
    let rows = (model.tasks.len() + 1) / 2;
    let width = col_right_x + node_w + 80;
    let height = top_y + rows.max(1) as i32 * (node_h + gap_y) + 80;

    let mut out = String::new();
    let _ = write!(
        out,
        r#"<svg class="flow-svg" viewBox="0 0 {width} {height}" role="img" aria-label="project overview flowchart" xmlns="http://www.w3.org/2000/svg">"#
    );
    out.push_str(
        r##"<defs><marker id="arrow" markerWidth="12" markerHeight="12" refX="10" refY="6" orient="auto" markerUnits="strokeWidth"><path d="M0,0 L12,6 L0,12 z" fill="#64748b"/></marker></defs>"##,
    );

    let mut positions: HashMap<String, (i32, i32)> = HashMap::new();
    for (idx, task) in model.tasks.iter().enumerate() {
        let col = idx % 2;
        let row = idx / 2;
        let x = if col == 0 { col_left_x } else { col_right_x };
        let y = top_y + row as i32 * (node_h + gap_y);
        positions.insert(task.task_name.clone(), (x, y));
        render_svg_node(
            &mut out,
            x,
            y,
            node_w,
            node_h,
            &task.task_name,
            "task",
            None,
        );
    }

    // Side-lane X coordinates for routing same-column edges without going off-screen
    let left_lane_x = col_left_x - 32;
    let right_lane_x = col_right_x + node_w + 32;

    let mut drawn_edges = BTreeSet::new();
    for task in &model.tasks {
        for edge in &task.transitions {
            if edge.from_task == edge.to_task {
                continue;
            }
            let key = (edge.from_task.clone(), edge.to_task.clone());
            if !drawn_edges.insert(key.clone()) {
                continue;
            }
            let Some((from_x, from_y)) = positions.get(&key.0).copied() else {
                continue;
            };
            let Some((to_x, to_y)) = positions.get(&key.1).copied() else {
                continue;
            };
            let from_cy = from_y + node_h / 2;
            let to_cy = to_y + node_h / 2;
            let same_col = from_x == to_x;
            let path = if same_col {
                // Route through a side lane to avoid overlapping nodes
                let lane_x = if from_x == col_left_x {
                    left_lane_x
                } else {
                    right_lane_x
                };
                let fx = from_x + node_w / 2;
                let tx = to_x + node_w / 2;
                format!("M {fx} {from_cy} C {lane_x} {from_cy}, {lane_x} {to_cy}, {tx} {to_cy}")
            } else {
                let from_cx = from_x + node_w;
                let to_cx = to_x;
                let mid_x = (from_cx + to_cx) / 2;
                format!(
                    "M {from_cx} {from_cy} C {mid_x} {from_cy}, {mid_x} {to_cy}, {to_cx} {to_cy}"
                )
            };
            let _ = writeln!(out, r#"<path class="flow-edge external" d="{path}"/>"#);
            let label_x = if same_col {
                from_x + node_w / 2
            } else {
                (from_x + node_w + to_x) / 2
            };
            render_svg_label(
                &mut out,
                label_x,
                ((from_cy + to_cy) / 2) - 10,
                &format!("{} \u{2192} {}", key.0, key.1),
            );
        }
    }

    out.push_str("</svg>");
    out
}

fn transition_fact_lines(edge: &EdgeSummary, step_statements: &[String]) -> Vec<String> {
    let mut facts = Vec::<(&str, &str)>::new();
    facts.extend(edge.actions.iter().map(|item| ("action", item.as_str())));
    facts.extend(edge.effects.iter().map(|item| ("effect", item.as_str())));
    let normalized_step_lines = step_statements
        .iter()
        .map(|item| canonicalize_flowchart_compare_key(item))
        .collect::<Vec<_>>();
    let mut lines = Vec::new();
    for (kind, fact) in facts {
        let normalized_fact = canonicalize_flowchart_compare_key(fact);
        if normalized_step_lines
            .iter()
            .any(|step_line| step_line.contains(&normalized_fact))
        {
            continue;
        }
        lines.extend(wrap_text_lines(&format!("{kind}: {fact}"), 34, 3));
    }
    lines
}

fn step_body_lines(statements: &[String]) -> Vec<String> {
    statements
        .iter()
        .filter(|item| {
            let normalized = canonicalize_flowchart_text(item);
            !(normalized.starts_with("wait ")
                || normalized.starts_with("delay ")
                || normalized.starts_with("timeout ")
                || normalized.starts_with("goto "))
        })
        .cloned()
        .collect()
}

fn canonicalize_flowchart_compare_key(value: &str) -> String {
    canonicalize_flowchart_text(value)
        .replace("some(", "")
        .replace('(', "")
        .replace(')', "")
        .replace(".0", "")
}

fn canonicalize_flowchart_text(value: &str) -> String {
    value
        .to_lowercase()
        .replace(" add ", " + ")
        .replace(" subtract ", " - ")
        .replace(" multiply ", " * ")
        .replace(" divide ", " / ")
        .replace(" true", " true")
        .replace(" false", " false")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn wrap_text_lines(value: &str, max_chars: usize, max_lines: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for ch in value.chars() {
        current.push(ch);
        if current.chars().count() >= max_chars {
            lines.push(current);
            current = String::new();
            if lines.len() >= max_lines {
                break;
            }
        }
    }
    if lines.len() < max_lines && !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push("-".to_string());
    }
    if value.chars().count() > max_chars * max_lines {
        if let Some(last) = lines.last_mut() {
            if !last.ends_with("...") {
                last.push_str("...");
            }
        }
    }
    lines
}

fn flatten_statement_lines(values: &[String], max_chars: usize, max_lines: usize) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        let wrapped = wrap_text_lines(value, max_chars, max_lines.saturating_sub(out.len()));
        for line in wrapped {
            if out.len() >= max_lines {
                break;
            }
            out.push(line);
        }
        if out.len() >= max_lines {
            break;
        }
    }
    out
}

fn render_svg_lines(
    out: &mut String,
    x: i32,
    y: i32,
    lines: &[String],
    class_name: &str,
    line_height: i32,
    anchor: &str,
) {
    for (idx, line) in lines.iter().enumerate() {
        let yy = y + idx as i32 * line_height;
        let _ = writeln!(
            out,
            r#"<text class="{class_name}" text-anchor="{anchor}" x="{x}" y="{yy}">{}</text>"#,
            html_escape(line)
        );
    }
}

#[allow(dead_code)]
fn render_svg_node(
    out: &mut String,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    title: &str,
    subtitle: &str,
    class_suffix: Option<&str>,
) {
    let class = match class_suffix {
        Some(suffix) if !suffix.is_empty() => format!("flow-node {suffix}"),
        _ => "flow-node".to_string(),
    };
    let _ = writeln!(
        out,
        r#"<g><rect class="{class}" x="{x}" y="{y}" width="{width}" height="{height}" rx="16" ry="16"/>"#
    );
    let text_x = x + width / 2;
    let title_y = y + 23;
    let _ = writeln!(
        out,
        r#"<text text-anchor="middle" x="{text_x}" y="{title_y}"><tspan class="flow-title" x="{text_x}" dy="0">{}</tspan><tspan class="flow-subtitle" x="{text_x}" dy="20">{}</tspan></text></g>"#,
        html_escape(title),
        html_escape(subtitle)
    );
}

#[allow(dead_code)]
fn render_svg_label(out: &mut String, x: i32, y: i32, label: &str) {
    let clipped = truncate_label(label, 62);
    let text_len = clipped.chars().count() as i32;
    let label_w = (text_len * 7 + 18).clamp(90, 360);
    let label_h = 24;
    let left = x - label_w / 2;
    let top = y - label_h / 2;
    let _ = writeln!(
        out,
        r#"<g><rect class="flow-label-bg" x="{left}" y="{top}" width="{label_w}" height="{label_h}" rx="10" ry="10"/><text class="flow-label" text-anchor="middle" x="{x}" y="{y}">{}</text></g>"#,
        html_escape(&clipped)
    );
}

#[allow(dead_code)]
fn truncate_label(value: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for ch in value.chars().take(max_chars) {
        out.push(ch);
    }
    if value.chars().count() > max_chars {
        out.push_str("...");
    }
    out
}

fn short_source(value: &str) -> String {
    let normalized = value.replace('\\', "/");
    let parts = normalized.split('/').collect::<Vec<_>>();
    if parts.len() >= 2 {
        return format!("{}/{}", parts[parts.len() - 2], parts[parts.len() - 1]);
    }
    normalized
}

fn render_html(model: &FlowchartArtifact) -> String {
    let style = r####"
    :root {
      --bg: #f4efe5;
      --paper: rgba(255, 250, 240, 0.93);
      --panel: rgba(255, 252, 246, 0.96);
      --line: #d8c6a1;
      --ink: #1e293b;
      --muted: #6b7280;
      --accent: #0f766e;
      --accent-dark: #134e4a;
      --process: #0f4c81;
      --startup: #0f766e;
      --supervisor: #7c2d12;
      --service: #7c3aed;
      --fault: #b45309;
      --glow: rgba(15, 118, 110, 0.18);
    }
    * { box-sizing: border-box; }
    html { scroll-behavior: smooth; }
    body {
      margin: 0;
      color: var(--ink);
      font-family: Cambria, Georgia, "Noto Serif SC", "Songti SC", serif;
      background:
        radial-gradient(circle at top left, rgba(15, 118, 110, 0.10), transparent 24%),
        radial-gradient(circle at top right, rgba(15, 76, 129, 0.08), transparent 22%),
        linear-gradient(180deg, #f8f4ec 0%, #f1eadf 100%);
    }
    body::before {
      content: "";
      position: fixed;
      inset: 0;
      pointer-events: none;
      background-image:
        linear-gradient(rgba(116, 97, 68, 0.04) 1px, transparent 1px),
        linear-gradient(90deg, rgba(116, 97, 68, 0.04) 1px, transparent 1px);
      background-size: 32px 32px;
      mask-image: linear-gradient(180deg, rgba(0,0,0,0.4), transparent 78%);
    }
    header.hero {
      position: sticky;
      top: 0;
      z-index: 20;
      padding: 26px 34px 18px;
      border-bottom: 1px solid var(--line);
      background: rgba(247, 241, 230, 0.88);
      backdrop-filter: blur(14px);
    }
    .hero h1 {
      margin: 0;
      font-size: 48px;
      letter-spacing: -0.03em;
      font-family: Bahnschrift, "Cascadia Code", Consolas, monospace;
    }
    .hero-summary {
      margin-top: 8px;
      display: flex;
      flex-wrap: wrap;
      gap: 10px;
      color: var(--muted);
      font-size: 14px;
    }
    .hero-summary code {
      font-family: "Cascadia Code", Consolas, monospace;
      background: rgba(15, 118, 110, 0.08);
      border-radius: 999px;
      padding: 2px 10px;
      color: var(--accent-dark);
    }
    .atlas-app {
      display: grid;
      grid-template-columns: 280px minmax(0, 1fr);
      gap: 24px;
      padding: 24px;
      align-items: start;
    }
    .command-rail {
      position: sticky;
      top: 116px;
      background: rgba(255, 250, 240, 0.9);
      border: 1px solid var(--line);
      border-radius: 24px;
      padding: 18px;
      box-shadow: 0 22px 50px rgba(37, 43, 56, 0.10);
    }
    .rail-title {
      margin: 0 0 8px;
      color: var(--muted);
      font-size: 12px;
      letter-spacing: 0.18em;
      text-transform: uppercase;
    }
    .section-jumps,
    .task-nav {
      display: grid;
      gap: 8px;
    }
    .rail-button,
    .task-button {
      width: 100%;
      border: 1px solid transparent;
      border-radius: 14px;
      background: transparent;
      color: var(--ink);
      padding: 11px 12px;
      text-align: left;
      cursor: pointer;
      font: inherit;
      transition: background-color 160ms ease, border-color 160ms ease, transform 160ms ease;
    }
    .rail-button:hover,
    .task-button:hover,
    .task-button.active {
      background: rgba(15, 118, 110, 0.11);
      border-color: rgba(15, 118, 110, 0.18);
      transform: translateX(2px);
    }
    .task-button small {
      display: block;
      margin-top: 4px;
      color: var(--muted);
      font-size: 11px;
      letter-spacing: 0.06em;
      text-transform: uppercase;
    }
    .scene-stack {
      display: grid;
      gap: 24px;
      min-width: 0;
    }
    .scene-panel {
      background: var(--paper);
      border: 1px solid var(--line);
      border-radius: 28px;
      padding: 22px;
      box-shadow: 0 20px 48px rgba(37, 43, 56, 0.09);
    }
    .scene-head {
      display: flex;
      justify-content: space-between;
      align-items: flex-end;
      gap: 18px;
      margin-bottom: 18px;
    }
    .scene-head h2 {
      margin: 0;
      font-size: 34px;
      letter-spacing: -0.02em;
      font-family: Bahnschrift, "Cascadia Code", Consolas, monospace;
    }
    .scene-head p {
      margin: 6px 0 0;
      color: var(--muted);
      font-size: 14px;
      max-width: 720px;
      line-height: 1.5;
    }
    .atlas-frame {
      position: relative;
      overflow: hidden;
      background:
        radial-gradient(circle at 20% 0%, rgba(15, 118, 110, 0.10), transparent 32%),
        radial-gradient(circle at 100% 20%, rgba(15, 76, 129, 0.08), transparent 28%),
        linear-gradient(180deg, rgba(255,255,255,0.60), rgba(255,252,246,0.96));
      border: 1px solid #e5d8bf;
      border-radius: 22px;
      padding: 16px;
    }
    .atlas-caption {
      display: flex;
      flex-wrap: wrap;
      gap: 8px;
      align-items: center;
      margin-bottom: 14px;
      min-height: 34px;
    }
    .caption-chip {
      display: inline-flex;
      align-items: center;
      gap: 8px;
      padding: 6px 12px;
      border-radius: 999px;
      background: rgba(255, 250, 240, 0.92);
      border: 1px solid #dfcfaf;
      font-family: "Cascadia Code", Consolas, monospace;
      font-size: 12px;
      color: var(--ink);
    }
    .atlas-canvas {
      overflow: auto;
      border-radius: 18px;
      background: rgba(255,255,255,0.48);
    }
    .atlas-svg {
      display: block;
      width: 100%;
      min-width: 1460px;
      height: auto;
    }
    .journey-strip {
      display: grid;
      grid-auto-flow: column;
      grid-auto-columns: minmax(240px, 280px);
      gap: 14px;
      overflow-x: auto;
      padding-bottom: 4px;
      scroll-snap-type: x proximity;
    }
    .journey-card {
      border: 1px solid #e0cfaf;
      background: linear-gradient(180deg, rgba(255,255,255,0.78), rgba(255,248,237,0.95));
      border-radius: 20px;
      padding: 16px;
      cursor: pointer;
      scroll-snap-align: start;
      transition: transform 160ms ease, border-color 160ms ease, box-shadow 160ms ease;
      box-shadow: 0 10px 28px rgba(37, 43, 56, 0.05);
    }
    .journey-card:hover,
    .journey-card.active {
      transform: translateY(-2px);
      border-color: #c09a54;
      box-shadow: 0 16px 32px rgba(192, 154, 84, 0.18);
    }
    .journey-kicker {
      color: var(--muted);
      font-size: 11px;
      letter-spacing: 0.16em;
      text-transform: uppercase;
      font-family: "Cascadia Code", Consolas, monospace;
    }
    .journey-title {
      margin-top: 10px;
      font-size: 24px;
      font-family: Bahnschrift, "Cascadia Code", Consolas, monospace;
      line-height: 1.05;
    }
    .journey-meta {
      margin-top: 10px;
      color: var(--muted);
      font-size: 13px;
      line-height: 1.45;
    }
    .detail-grid {
      display: grid;
      grid-template-columns: 260px minmax(0, 1fr) 320px;
      gap: 18px;
      align-items: start;
    }
    .detail-rail,
    .detail-side {
      display: grid;
      gap: 14px;
    }
    .detail-card,
    .step-card {
      background: var(--panel);
      border: 1px solid #e5d8bf;
      border-radius: 18px;
      padding: 14px;
    }
    .step-card {
      cursor: pointer;
      transition: transform 160ms ease, border-color 160ms ease;
    }
    .step-card:hover {
      transform: translateX(2px);
      border-color: rgba(15, 118, 110, 0.35);
    }
    .step-card-index {
      color: var(--muted);
      font-size: 11px;
      letter-spacing: 0.16em;
      text-transform: uppercase;
      font-family: "Cascadia Code", Consolas, monospace;
    }
    .step-card h3,
    .detail-card h3 {
      margin: 8px 0 8px;
      font-size: 20px;
      font-family: Bahnschrift, "Cascadia Code", Consolas, monospace;
      line-height: 1.1;
    }
    .step-card p,
    .detail-card p {
      margin: 0;
      color: var(--muted);
      font-size: 13px;
      line-height: 1.5;
    }
    .detail-meta,
    .chip-cloud {
      display: flex;
      flex-wrap: wrap;
      gap: 8px;
    }
    .meta-pill,
    .chip {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      padding: 5px 10px;
      border-radius: 999px;
      background: rgba(15, 118, 110, 0.09);
      border: 1px solid rgba(15, 118, 110, 0.14);
      color: var(--accent-dark);
      font-size: 12px;
      font-family: "Cascadia Code", Consolas, monospace;
    }
    .chip.role-supervisor { background: rgba(124, 45, 18, 0.10); border-color: rgba(124, 45, 18, 0.16); color: #7c2d12; }
    .chip.role-startup { background: rgba(15, 118, 110, 0.10); border-color: rgba(15, 118, 110, 0.16); color: #134e4a; }
    .chip.role-process { background: rgba(15, 76, 129, 0.10); border-color: rgba(15, 76, 129, 0.16); color: #0f4c81; }
    .chip.role-service { background: rgba(124, 58, 237, 0.10); border-color: rgba(124, 58, 237, 0.16); color: #6d28d9; }
    .chip.role-fault { background: rgba(180, 83, 9, 0.10); border-color: rgba(180, 83, 9, 0.16); color: #b45309; }
    .detail-diagram {
      overflow: auto;
      background: #fffdf7;
      border: 1px solid #eadcc1;
      border-radius: 20px;
      padding: 12px;
      min-height: 720px;
    }
    .topology-grid {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
      gap: 14px;
    }
    .topology-grid .card {
      background: var(--panel);
      border: 1px solid #e5d8bf;
      border-radius: 18px;
      padding: 14px;
    }
    .topology-grid .card h3 {
      margin-top: 0;
      font-family: Bahnschrift, "Cascadia Code", Consolas, monospace;
    }
    .topology-grid ul {
      margin: 0;
      padding-left: 18px;
      color: var(--muted);
      font-size: 13px;
      line-height: 1.55;
    }
    .task-templates { display: none; }
    .sfc-review { width: 100%; }
    .task-sfc-svg { display: block; width: 100%; min-width: 980px; height: auto; }
    .task-step { fill: #fffaf0; stroke: #0f766e; stroke-width: 3; }
    .task-step.generated { fill: #f8fafc; stroke: #94a3b8; stroke-dasharray: 8 6; }
    .task-step-initial { fill: none; stroke: #0f766e; stroke-width: 3; }
    .task-step-index { fill: #94a3b8; font: 700 14px "Cascadia Code", Consolas, monospace; }
    .task-step-title { fill: #093f3b; font: 700 16px "Cascadia Code", Consolas, monospace; }
    .task-step-source { fill: #64748b; font: 12px Cambria, Georgia, serif; }
    .task-action-link { stroke: #94a3b8; stroke-width: 1.3; }
    .task-action { fill: #f8fbff; stroke: #c9d6e2; stroke-width: 1.2; }
    .task-action-line { fill: #111827; font: 12px "Cascadia Code", Consolas, monospace; }
    .task-main-line { stroke: #111827; stroke-width: 2; }
    .task-transition-bar { stroke: #111827; stroke-width: 6; stroke-linecap: square; }
    .task-transition-branch-bus { stroke: #7c6750; stroke-width: 1.7; }
    .task-transition-branch-bar { stroke: #111827; stroke-width: 5; stroke-linecap: square; }
    .task-transition-link { stroke: #c7b28a; stroke-width: 1.6; }
    .task-transition-note { fill: #fffdf7; stroke: #d7ccb7; stroke-width: 1.1; }
    .task-transition-note.external { stroke: #d7b674; }
    .task-transition-note.terminal { stroke: #b8c3cf; }
    .task-transition-guard { fill: #111827; font: 700 12px "Cascadia Code", Consolas, monospace; }
    .task-transition-fact { fill: #475569; font: 11px "Cascadia Code", Consolas, monospace; }
    .task-transition-target { fill: #134e4a; font: 700 11px "Cascadia Code", Consolas, monospace; }
    .task-transition-target.external { fill: #b7791f; }
    .task-transition-target.terminal { fill: #64748b; }
    code {
      font-family: "Cascadia Code", Consolas, monospace;
      background: rgba(15, 118, 110, 0.08);
      color: var(--accent-dark);
      border-radius: 999px;
      padding: 2px 9px;
    }
    @media (max-width: 1280px) {
      .atlas-app { grid-template-columns: 1fr; }
      .command-rail { position: static; }
      .detail-grid { grid-template-columns: 1fr; }
      .detail-diagram { min-height: auto; }
      .hero { position: static; }
    }
    "####;

    let script = r####"
    (() => {
      const model = JSON.parse(document.getElementById('flowchart-model').textContent);
      const taskTemplates = new Map(
        Array.from(document.querySelectorAll('#task-templates template')).map((tpl) => [
          tpl.dataset.task,
          tpl.innerHTML,
        ])
      );

      const roleLabels = {
        supervisor: 'Control Gate',
        startup: 'Startup / Self-check',
        process: 'Process / Motion',
        service: 'Manual / Maintenance',
        fault: 'Fault / Warning',
      };
      const roleClasses = {
        supervisor: 'role-supervisor',
        startup: 'role-startup',
        process: 'role-process',
        service: 'role-service',
        fault: 'role-fault',
      };
      const roleColors = {
        supervisor: '#7c2d12',
        startup: '#0f766e',
        process: '#0f4c81',
        service: '#6d28d9',
        fault: '#b45309',
      };

      const taskButtons = Array.from(document.querySelectorAll('.task-button'));
      const topologyNodes = buildPhysicalNodes(model);
      const nodeMap = new Map(topologyNodes.map((node, index) => [node.name, { ...node, index }]));
      const journey = buildJourney(model);
      const taskMeta = buildTaskMeta(model);
      const taskMetaMap = new Map(taskMeta.map((task) => [task.task_name, task]));
      const initialSelection = chooseInitialSelection(taskMeta, journey, model.tasks);
      const state = {
        selectedTask: initialSelection.taskName,
        activeJourneyIndex: initialSelection.journeyIndex,
      };

      bindInteractions();
      renderAll();

      if (journey.length > 1) {
        setInterval(() => {
          if (document.hidden) return;
          state.activeJourneyIndex = (state.activeJourneyIndex + 1) % journey.length;
          renderAtlas();
          renderJourney();
          renderCaption();
        }, 2600);
      }

      function bindInteractions() {
        document.querySelectorAll('.rail-button').forEach((button) => {
          button.addEventListener('click', () => {
            const target = document.getElementById(button.dataset.section);
            if (target) target.scrollIntoView({ behavior: 'smooth', block: 'start' });
          });
        });
        taskButtons.forEach((button) => {
          button.addEventListener('click', () => selectTask(button.dataset.task, true));
        });
      }

      function buildPhysicalNodes(model) {
        const seen = new Set();
        const ordered = [];
        const push = (name, kind) => {
          if (!name || seen.has(name)) return;
          seen.add(name);
          ordered.push({ name, kind: kind || classifyNode(name) });
        };

        model.tasks.forEach((task) => {
          task.transitions.forEach((edge) => {
            edge.effects.forEach((effect) => {
              const parsed = parseEffect(effect);
              if (!parsed) return;
              push(parsed.from, classifyNode(parsed.from));
              push(parsed.to, classifyNode(parsed.to));
            });
          });
        });

        model.topology.workpiece_sites.forEach((entry) => push(entry.split(':')[0], 'site'));
        model.topology.workpiece_holders.forEach((entry) => push(entry.split(':')[0], 'holder'));

        return ordered;
      }

      function classifyNode(name) {
        const lower = name.toLowerCase();
        if (
          lower.includes('nozzle') ||
          lower.includes('holder') ||
          lower.includes('ejector')
        ) {
          return 'holder';
        }
        if (lower.includes('bin')) return 'terminal';
        return 'site';
      }

      function parseEffect(effect) {
        let match;
        if ((match = /^acquire (.+) from (.+)$/.exec(effect))) {
          return { kind: 'acquire', from: match[2], to: match[1], label: `${match[2]} → ${match[1]}` };
        }
        if ((match = /^transfer (.+) -> (.+)$/.exec(effect))) {
          return { kind: 'transfer', from: match[1], to: match[2], label: `${match[1]} → ${match[2]}` };
        }
        if ((match = /^finish (.+) as (.+)$/.exec(effect))) {
          return { kind: 'finish', from: match[1], to: match[2], label: `${match[1]} → ${match[2]}` };
        }
        if ((match = /^mount (.+) at (.+)$/.exec(effect))) {
          return { kind: 'mount', from: match[1], to: match[2], label: `${match[1]} @ ${match[2]}` };
        }
        if ((match = /^unmount (.+) from (.+) to (.+)$/.exec(effect))) {
          return { kind: 'unmount', from: match[2], to: match[3], label: `${match[2]} → ${match[3]}` };
        }
        return null;
      }

      function buildJourney(model) {
        const events = [];
        model.tasks.forEach((task) => {
          task.transitions.forEach((edge, edgeIndex) => {
            edge.effects.forEach((effect, effectIndex) => {
              const parsed = parseEffect(effect);
              if (!parsed) return;
              events.push({
                id: `${task.task_name}-${edge.from_step}-${edgeIndex}-${effectIndex}`,
                task_name: task.task_name,
                from_step: edge.from_step,
                to_step: edge.to_step,
                from_task: edge.from_task,
                to_task: edge.to_task,
                guard: edge.guard,
                effect,
                ...parsed,
              });
            });
          });
        });
        return events;
      }

      function roleForTask(taskName) {
        const lower = taskName.toLowerCase();
        if (lower.includes('fault') || lower.includes('warning')) return 'fault';
        if (lower.includes('manual') || lower.includes('maintenance') || lower.includes('check')) return 'service';
        if (lower.includes('startup')) return 'startup';
        if (lower.includes('supervisor') || lower.includes('monitor') || lower.includes('architecture')) return 'supervisor';
        return 'process';
      }

      function collectTouches(task) {
        const haystack = JSON.stringify(task).toLowerCase();
        return topologyNodes
          .filter((node) => haystack.includes(node.name.toLowerCase()))
          .map((node) => node.name);
      }

      function buildTaskMeta(model) {
        return model.tasks.map((task, taskIndex) => {
          const touches = collectTouches(task);
          const touchIndices = touches
            .map((name) => nodeMap.get(name)?.index)
            .filter((value) => value !== undefined);
          const handoffs = [];
          task.transitions.forEach((edge) => {
            edge.effects.forEach((effect) => {
              const parsed = parseEffect(effect);
              if (parsed) handoffs.push({ effect, ...parsed });
            });
          });
          const externalRoutes = task.transitions.filter((edge) => edge.to_task !== task.task_name);
          const anchor = touchIndices.length
            ? touchIndices.reduce((sum, value) => sum + value, 0) / touchIndices.length
            : taskIndex;
          return {
            ...task,
            role: roleForTask(task.task_name),
            touches,
            handoffs,
            externalRoutes,
            anchor,
          };
        });
      }

      function chooseInitialSelection(taskMeta, journey, tasks) {
        const preferredTask =
          taskMeta.find((task) => task.role === 'process' && task.handoffs.length) ||
          taskMeta.find((task) => task.handoffs.length) ||
          taskMeta.find((task) => task.role === 'process') ||
          taskMeta[0] ||
          tasks[0] ||
          null;
        if (!preferredTask) {
          return { taskName: null, journeyIndex: 0 };
        }
        const journeyIndex = journey.findIndex((event) => event.task_name === preferredTask.task_name);
        return {
          taskName: preferredTask.task_name,
          journeyIndex: journeyIndex >= 0 ? journeyIndex : 0,
        };
      }

      function selectTask(taskName, scrollIntoView) {
        if (!taskName) return;
        state.selectedTask = taskName;
        taskButtons.forEach((button) => button.classList.toggle('active', button.dataset.task === taskName));
        renderDetail();
        renderAtlas();
        renderJourney();
        if (scrollIntoView) {
          document.getElementById('section-detail').scrollIntoView({ behavior: 'smooth', block: 'start' });
        }
      }

      function renderAll() {
        renderAtlas();
        renderJourney();
        renderDetail();
        renderCaption();
        taskButtons.forEach((button) => button.classList.toggle('active', button.dataset.task === state.selectedTask));
      }

      function renderCaption() {
        const caption = document.getElementById('atlas-caption');
        const active = journey[state.activeJourneyIndex];
        const selected = taskMetaMap.get(state.selectedTask);
        const chips = [];
        if (active) {
          chips.push(`<span class="caption-chip">active journey: ${escapeHtml(active.label)} via ${escapeHtml(active.task_name)}</span>`);
        }
        if (selected) {
          chips.push(`<span class="caption-chip">selected task: ${escapeHtml(selected.task_name)} / ${escapeHtml(roleLabels[selected.role])}</span>`);
          chips.push(`<span class="caption-chip">touched nodes: ${selected.touches.length}</span>`);
        }
        caption.innerHTML = chips.join('');
      }

      function renderAtlas() {
        const host = document.getElementById('atlas-canvas');
        const width = Math.max(1540, host.clientWidth || 1540);
        const height = 860;
        const routeY = 110;
        const left = 120;
        const right = width - 120;
        const spacing = topologyNodes.length > 1 ? (right - left) / (topologyNodes.length - 1) : 0;
        const positions = new Map(
          topologyNodes.map((node, index) => [node.name, { x: left + index * spacing, y: routeY }])
        );

        const bands = [
          { key: 'supervisor', y: 250 },
          { key: 'startup', y: 380 },
          { key: 'process', y: 540 },
          { key: 'service', y: 690 },
          { key: 'fault', y: 780 },
        ];

        const selected = taskMetaMap.get(state.selectedTask);
        const active = journey[state.activeJourneyIndex];
        const taskPositions = new Map();
        const groupMarkup = [];

        bands.forEach((band) => {
          const cards = taskMeta.filter((task) => task.role === band.key);
          const placed = layoutBand(cards, band.y, left + 40, right - 40);
          groupMarkup.push(`
            <text x="30" y="${band.y + 8}" fill="${roleColors[band.key]}" font-size="14" font-family="Cascadia Code, Consolas, monospace">${escapeHtml(roleLabels[band.key])}</text>
          `);
          placed.forEach((item) => {
            taskPositions.set(item.task_name, item);
          });
        });

        const routeEdges = [];
        for (let index = 0; index < topologyNodes.length - 1; index += 1) {
          const from = positions.get(topologyNodes[index].name);
          const to = positions.get(topologyNodes[index + 1].name);
          const isActive =
            active &&
            ((active.from === topologyNodes[index].name && active.to === topologyNodes[index + 1].name) ||
              (active.to === topologyNodes[index].name && active.from === topologyNodes[index + 1].name));
          routeEdges.push(`
            <line x1="${from.x}" y1="${from.y}" x2="${to.x}" y2="${to.y}"
              stroke="${isActive ? '#c08a2f' : '#d2c1a0'}"
              stroke-width="${isActive ? '7' : '4'}"
              stroke-linecap="round"
              opacity="${isActive ? '1' : '0.8'}" />
          `);
        }

        const nodeMarkup = topologyNodes
          .map((node) => {
            const pos = positions.get(node.name);
            const isTouched = selected && selected.touches.includes(node.name);
            const isActiveFrom = active && active.from === node.name;
            const isActiveTo = active && active.to === node.name;
            const stroke = node.kind === 'holder' ? '#0f4c81' : node.kind === 'terminal' ? '#b45309' : '#0f766e';
            const fill = isActiveFrom || isActiveTo ? 'rgba(255,246,220,0.98)' : 'rgba(255,252,246,0.98)';
            return `
              <g>
                <rect x="${pos.x - 88}" y="${pos.y - 32}" width="176" height="64" rx="20"
                  fill="${fill}" stroke="${stroke}" stroke-width="${isTouched ? '3.6' : '2.2'}" />
                <text x="${pos.x}" y="${pos.y - 4}" text-anchor="middle"
                  fill="${stroke}" font-size="16" font-family="Bahnschrift, Cascadia Code, Consolas, monospace">${escapeHtml(node.name)}</text>
                <text x="${pos.x}" y="${pos.y + 18}" text-anchor="middle"
                  fill="#6b7280" font-size="11" font-family="Cascadia Code, Consolas, monospace">${node.kind}</text>
              </g>
            `;
          })
          .join('');

        const taskLinkMarkup = [];
        const taskCardMarkup = [];
        taskMeta.forEach((task) => {
          const pos = taskPositions.get(task.task_name);
          if (!pos) return;
          const selectedTask = state.selectedTask === task.task_name;
          const cardX = pos.x - 98;
          const cardY = pos.y - 34;
          const accent = roleColors[task.role];
          task.touches.forEach((touchName, touchIndex) => {
            const nodePos = positions.get(touchName);
            if (!nodePos) return;
            const opacity = selectedTask ? 0.42 : 0.14;
            const path = [
              `M ${pos.x} ${cardY}`,
              `C ${pos.x} ${pos.y - 92}, ${nodePos.x} ${routeY + 72}, ${nodePos.x} ${routeY + 34}`,
            ].join(' ');
            taskLinkMarkup.push(`
              <path d="${path}" fill="none" stroke="${accent}" stroke-width="${selectedTask ? '2.2' : '1.2'}" opacity="${opacity}" />
            `);
          });

          if (selectedTask) {
            task.externalRoutes.forEach((route, routeIndex) => {
              const target = taskPositions.get(route.to_task);
              if (!target) return;
              const sx = pos.x + 98;
              const sy = pos.y;
              const tx = target.x - 98;
              const ty = target.y;
              const mx = (sx + tx) / 2;
              taskLinkMarkup.push(`
                <path d="M ${sx} ${sy} C ${mx} ${sy - 60 - routeIndex * 14}, ${mx} ${ty - 60 - routeIndex * 14}, ${tx} ${ty}"
                  fill="none" stroke="#c08a2f" stroke-width="1.8" stroke-dasharray="8 6" opacity="0.72" />
              `);
            });
          }

          taskCardMarkup.push(`
            <g class="atlas-task-hit" data-task="${escapeHtml(task.task_name)}" style="cursor:pointer">
              <rect x="${cardX}" y="${cardY}" width="196" height="68" rx="18"
                fill="${selectedTask ? 'rgba(255,255,255,0.98)' : 'rgba(255,250,242,0.94)'}"
                stroke="${accent}" stroke-width="${selectedTask ? '3.2' : '1.8'}" />
              <text x="${pos.x}" y="${cardY + 28}" text-anchor="middle"
                fill="${accent}" font-size="18" font-family="Bahnschrift, Cascadia Code, Consolas, monospace">${escapeHtml(task.task_name)}</text>
              <text x="${pos.x}" y="${cardY + 49}" text-anchor="middle"
                fill="#6b7280" font-size="11" font-family="Cascadia Code, Consolas, monospace">${task.steps.length} steps · ${task.transitions.length} transitions</text>
            </g>
          `);
        });

        const activePulse = active && positions.get(active.from) && positions.get(active.to)
          ? (() => {
              const from = positions.get(active.from);
              const to = positions.get(active.to);
              return `
                <circle cx="${from.x}" cy="${from.y}" r="9" fill="#c08a2f">
                  <animate attributeName="cx" values="${from.x};${to.x}" dur="2.2s" repeatCount="indefinite" />
                  <animate attributeName="cy" values="${from.y};${to.y}" dur="2.2s" repeatCount="indefinite" />
                </circle>
              `;
            })()
          : '';

        host.innerHTML = `
          <svg class="atlas-svg" viewBox="0 0 ${width} ${height}" role="img" aria-label="system atlas">
            <defs>
              <filter id="atlasGlow" x="-40%" y="-40%" width="180%" height="180%">
                <feDropShadow dx="0" dy="0" stdDeviation="12" flood-color="rgba(15,118,110,0.18)" />
              </filter>
            </defs>
            <rect x="0" y="0" width="${width}" height="${height}" rx="24" fill="rgba(255,255,255,0.32)" />
            <text x="${left}" y="54" fill="#6b7280" font-size="12" font-family="Cascadia Code, Consolas, monospace">physical skeleton</text>
            ${routeEdges.join('')}
            ${nodeMarkup}
            ${groupMarkup.join('')}
            ${taskLinkMarkup.join('')}
            ${taskCardMarkup.join('')}
            ${activePulse}
          </svg>
        `;

        host.querySelectorAll('.atlas-task-hit').forEach((node) => {
          node.addEventListener('click', () => selectTask(node.dataset.task, true));
        });
      }

      function layoutBand(cards, y, left, right) {
        const sorted = cards
          .slice()
          .sort((a, b) => a.anchor - b.anchor || a.task_name.localeCompare(b.task_name));
        const minGap = 220;
        let cursor = left;
        const placed = sorted.map((task, index) => {
          const anchorRatio = topologyNodes.length > 1 ? task.anchor / (topologyNodes.length - 1) : 0.5;
          const desired = left + anchorRatio * (right - left);
          const x = Math.max(cursor, desired);
          cursor = x + minGap;
          return { ...task, x, y: y + (cards.length > 4 && index % 2 ? 88 : 0) };
        });
        const overflow = cursor - minGap - right;
        if (overflow > 0 && placed.length > 1) {
          placed.forEach((task, index) => {
            task.x -= overflow * (index / (placed.length - 1));
          });
        }
        return placed;
      }

      function renderJourney() {
        const host = document.getElementById('journey-track');
        host.innerHTML = journey
          .map((event, index) => {
            const active = index === state.activeJourneyIndex;
            return `
              <button class="journey-card ${active ? 'active' : ''}" data-index="${index}">
                <div class="journey-kicker">${escapeHtml(event.task_name)} · ${escapeHtml(event.from_step)}</div>
                <div class="journey-title">${escapeHtml(event.label)}</div>
                <div class="journey-meta">${escapeHtml(event.effect)}<br>${escapeHtml(event.guard)}</div>
              </button>
            `;
          })
          .join('');

        host.querySelectorAll('.journey-card').forEach((card) => {
          card.addEventListener('click', () => {
            const index = Number(card.dataset.index);
            state.activeJourneyIndex = index;
            selectTask(journey[index].task_name, false);
            renderAtlas();
            renderJourney();
            renderCaption();
          });
        });
      }

      function renderDetail() {
        const task = taskMetaMap.get(state.selectedTask);
        if (!task) return;
        document.getElementById('detail-title').textContent = task.task_name;
        document.getElementById('detail-summary').textContent =
          `This theater keeps one control task in focus while the atlas above keeps the global machine skeleton visible.`;
        document.getElementById('detail-meta').innerHTML = `
          <span class="meta-pill ${roleClasses[task.role]}">${escapeHtml(roleLabels[task.role])}</span>
          <span class="meta-pill">${task.steps.length} steps</span>
          <span class="meta-pill">${task.transitions.length} transitions</span>
          <span class="meta-pill">${task.handoffs.length} material handoffs</span>
        `;

        document.getElementById('detail-step-rail').innerHTML = task.steps
          .map((step, index) => {
            const summary = step.statements.slice(0, 2).join(' · ') || 'generated semantic state';
            return `
              <div class="step-card">
                <div class="step-card-index">step ${index + 1}</div>
                <h3>${escapeHtml(step.step_name)}</h3>
                <p>${escapeHtml(summary)}</p>
              </div>
            `;
          })
          .join('');

        document.getElementById('detail-sfc-host').innerHTML =
          taskTemplates.get(task.task_name) || '<p>missing task template</p>';

        document.getElementById('detail-touches').innerHTML =
          task.touches.length
            ? `<div class="chip-cloud">${task.touches
                .map((touch) => `<span class="chip">${escapeHtml(touch)}</span>`)
                .join('')}</div>`
            : '<p>no direct workpiece site/holder references</p>';

        document.getElementById('detail-handoffs').innerHTML =
          task.handoffs.length
            ? task.handoffs
                .map(
                  (handoff) => `
                    <div class="detail-card">
                      <h3>${escapeHtml(handoff.label)}</h3>
                      <p>${escapeHtml(handoff.effect)}</p>
                    </div>
                  `
                )
                .join('')
            : '<p>no explicit workpiece effect in this task</p>';

        document.getElementById('detail-external').innerHTML =
          task.externalRoutes.length
            ? task.externalRoutes
                .map(
                  (route) => `
                    <div class="detail-card">
                      <h3>${escapeHtml(route.guard)}</h3>
                      <p>${escapeHtml(`goto ${route.to_task}.${route.to_step}`)}</p>
                    </div>
                  `
                )
                .join('')
            : '<p>no cross-task route from this task</p>';
      }

      function escapeHtml(value) {
        return String(value)
          .replaceAll('&', '&amp;')
          .replaceAll('<', '&lt;')
          .replaceAll('>', '&gt;')
          .replaceAll('"', '&quot;')
          .replaceAll("'", '&#39;');
      }
    })();
    "####;

    let mut task_nav = String::new();
    for task in &model.tasks {
        let _ = writeln!(
            task_nav,
            "<button class=\"task-button\" data-task=\"{}\">{}<small>{}</small></button>",
            html_escape(&task.task_name),
            html_escape(&task.task_name),
            html_escape(task_role_label(&task.task_name))
        );
    }

    let mut task_templates = String::new();
    for task in &model.tasks {
        let _ = writeln!(
            task_templates,
            "<template data-task=\"{}\">{}</template>",
            html_escape(&task.task_name),
            render_task_svg(task)
        );
    }

    let model_json = embed_json_for_html(model);
    let mut out = String::new();
    out.push_str("<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    let _ = write!(
        out,
        "<title>{}</title><style>{style}</style></head><body>",
        html_escape(&model.title)
    );
    out.push_str("<header class=\"hero\">");
    let _ = write!(out, "<h1>{}</h1>", html_escape(&model.title));
    let _ = write!(
        out,
        "<div class=\"hero-summary\"><span>source: <code>{}</code></span><span>tasks: {}</span><span>transitions: {}</span></div>",
        html_escape(&model.source_plc),
        model.tasks.len(),
        model
            .tasks
            .iter()
            .map(|task| task.transitions.len())
            .sum::<usize>()
    );
    out.push_str("</header>");
    out.push_str("<main class=\"atlas-app\">");
    out.push_str("<aside class=\"command-rail\">");
    out.push_str("<p class=\"rail-title\">Sections</p>");
    out.push_str(
        "<div class=\"section-jumps\">\
          <button class=\"rail-button\" data-section=\"section-atlas\">System Atlas</button>\
          <button class=\"rail-button\" data-section=\"section-journey\">Journey Reel</button>\
          <button class=\"rail-button\" data-section=\"section-detail\">Task Theater</button>\
          <button class=\"rail-button\" data-section=\"section-topology\">Topology</button>\
        </div>",
    );
    out.push_str(
        "<p class=\"rail-title\" style=\"margin-top:18px\">Tasks</p><div class=\"task-nav\">",
    );
    out.push_str(&task_nav);
    out.push_str("</div></aside>");
    out.push_str("<div class=\"scene-stack\">");
    out.push_str(
        "<section id=\"section-atlas\" class=\"scene-panel\">\
          <div class=\"scene-head\">\
            <div><h2>System Atlas</h2><p>One global skeleton for stations, holders, control bands, and task-to-material coupling. The moving caption is not decoration: it is the current material handoff extracted from explicit workpiece effects.</p></div>\
          </div>\
          <div class=\"atlas-frame\">\
            <div id=\"atlas-caption\" class=\"atlas-caption\"></div>\
            <div id=\"atlas-canvas\" class=\"atlas-canvas\"></div>\
          </div>\
        </section>"
    );
    out.push_str(
        "<section id=\"section-journey\" class=\"scene-panel\">\
          <div class=\"scene-head\">\
            <div><h2>Journey Reel</h2><p>This strip is the machine narrative in handoff order. Selecting a card drives the atlas highlight and the task theater below.</p></div>\
          </div>\
          <div id=\"journey-track\" class=\"journey-strip\"></div>\
        </section>"
    );
    out.push_str(
        "<section id=\"section-detail\" class=\"scene-panel\">\
          <div class=\"scene-head\">\
            <div><h2 id=\"detail-title\">Task Theater</h2><p id=\"detail-summary\"></p></div>\
            <div id=\"detail-meta\" class=\"detail-meta\"></div>\
          </div>\
          <div class=\"detail-grid\">\
            <div id=\"detail-step-rail\" class=\"detail-rail\"></div>\
            <div id=\"detail-sfc-host\" class=\"detail-diagram\"></div>\
            <div class=\"detail-side\">\
              <div class=\"detail-card\"><h3>Material Touches</h3><div id=\"detail-touches\"></div></div>\
              <div class=\"detail-card\"><h3>Handoffs</h3><div id=\"detail-handoffs\"></div></div>\
              <div class=\"detail-card\"><h3>External Routes</h3><div id=\"detail-external\"></div></div>\
            </div>\
          </div>\
        </section>"
    );
    out.push_str("<section id=\"section-topology\" class=\"scene-panel\"><div class=\"scene-head\"><div><h2>Topology Ledger</h2><p>Keep the raw audit tables, but move them to the end. They support the atlas instead of pretending to be the atlas.</p></div></div>");
    out.push_str(&render_topology_html(&model.topology));
    out.push_str("</section>");
    out.push_str("</div></main>");
    let _ = write!(
        out,
        "<script id=\"flowchart-model\" type=\"application/json\">{}</script>",
        model_json
    );
    let _ = write!(
        out,
        "<div id=\"task-templates\" class=\"task-templates\">{}</div>",
        task_templates
    );
    let _ = write!(out, "<script>{script}</script>");
    out.push_str("</body></html>");
    out
}

fn embed_json_for_html(model: &FlowchartArtifact) -> String {
    serde_json::to_string(model)
        .expect("flowchart model should serialize")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
}

fn task_role_label(task_name: &str) -> &'static str {
    let lower = task_name.to_ascii_lowercase();
    if lower.contains("fault") || lower.contains("warning") {
        "fault / warning"
    } else if lower.contains("manual") || lower.contains("maintenance") || lower.contains("check") {
        "manual / maintenance"
    } else if lower.contains("startup") {
        "startup / self-check"
    } else if lower.contains("supervisor")
        || lower.contains("monitor")
        || lower.contains("architecture")
    {
        "control gate"
    } else {
        "process / motion"
    }
}

#[allow(dead_code)]
fn render_task_table(task: &TaskDiagram) -> String {
    let mut out = String::new();
    out.push_str("<h3>Step 明细</h3><table><thead><tr><th>Step</th><th>Source</th><th>Statements</th></tr></thead><tbody>");
    for step in &task.steps {
        let source = match (&step.source, step.line) {
            (Some(source), Some(line)) => format!("{source}:{line}"),
            _ if step.generated => "generated semantic state".to_string(),
            _ => "-".to_string(),
        };
        let statements = if step.statements.is_empty() {
            "-".to_string()
        } else {
            step.statements
                .iter()
                .map(|statement| format!("<span class=\"pill\">{}</span>", html_escape(statement)))
                .collect::<Vec<_>>()
                .join("")
        };
        let _ = write!(
            out,
            "<tr><td><code>{}.{}</code></td><td>{}</td><td>{}</td></tr>",
            html_escape(&task.task_name),
            html_escape(&step.step_name),
            html_escape(&source),
            statements
        );
    }
    out.push_str("</tbody></table><h3>Transition 明细</h3><table><thead><tr><th>From</th><th>To</th><th>Guard</th><th>Actions / Effects</th></tr></thead><tbody>");
    for edge in &task.transitions {
        let mut facts = edge.actions.clone();
        facts.extend(edge.effects.clone());
        let rendered_facts = if facts.is_empty() {
            "-".to_string()
        } else {
            facts
                .iter()
                .map(|fact| format!("<span class=\"pill\">{}</span>", html_escape(fact)))
                .collect::<Vec<_>>()
                .join("")
        };
        let _ = write!(
            out,
            "<tr><td><code>{}.{}</code></td><td><code>{}.{}</code></td><td>{}</td><td>{}</td></tr>",
            html_escape(&edge.from_task),
            html_escape(&edge.from_step),
            html_escape(&edge.to_task),
            html_escape(&edge.to_step),
            html_escape(&edge.guard),
            rendered_facts
        );
    }
    out.push_str("</tbody></table>");
    out
}

fn render_topology_html(topology: &TopologySummary) -> String {
    format!(
        "<div class=\"topology-grid\"><div class=\"card\"><h3>Counts</h3><p>devices: <code>{}</code></p><p>links: <code>{}</code></p></div><div class=\"card\"><h3>Variables</h3>{}</div><div class=\"card\"><h3>Workpieces</h3>{}</div><div class=\"card\"><h3>Links</h3>{}</div></div>",
        topology.device_count,
        topology.link_count,
        render_compact_list(&topology.variables),
        render_compact_list(
            &topology
                .workpiece_types
                .iter()
                .chain(topology.workpiece_sites.iter())
                .chain(topology.workpiece_holders.iter())
                .cloned()
                .collect::<Vec<_>>()
        ),
        render_compact_list(&topology.links)
    )
}

fn render_compact_list(values: &[String]) -> String {
    if values.is_empty() {
        return "<p>-</p>".to_string();
    }
    let mut out = String::from("<ul class=\"compact\">");
    for value in values.iter().take(80) {
        let _ = write!(out, "<li>{}</li>", html_escape(value));
    }
    if values.len() > 80 {
        let _ = write!(out, "<li>... {} more</li>", values.len() - 80);
    }
    out.push_str("</ul>");
    out
}

fn summarize_step_statement(statement: &StepStatement) -> String {
    match statement {
        StepStatement::Action(action) => summarize_action(action),
        StepStatement::Effect(effect) => summarize_effect_statement(effect),
        StepStatement::Wait(wait) => format!("wait {}", summarize_wait_condition(&wait.condition)),
        StepStatement::IfElse {
            condition,
            then_goto,
            else_goto,
        } => format!(
            "if {} then {} else {}",
            summarize_condition(condition),
            summarize_goto(then_goto),
            summarize_goto(else_goto)
        ),
        StepStatement::Delay { duration_ms } => format!("delay {duration_ms}ms"),
        StepStatement::Repeat { count, .. } => format!("repeat {count}"),
        StepStatement::Timeout(timeout) => format!(
            "timeout {} -> {}",
            summarize_duration(&timeout.duration),
            summarize_goto(&timeout.target)
        ),
        StepStatement::Goto(goto) => format!("goto {}", summarize_goto(goto)),
        StepStatement::Parallel(block) => format!("parallel {} branches", block.branches.len()),
        StepStatement::Race(block) => format!("race {} branches", block.branches.len()),
        StepStatement::AllowIndefiniteWait(value) => format!("allow_indefinite_wait {value}"),
    }
}

fn summarize_action(action: &ActionStatement) -> String {
    match action {
        ActionStatement::Extend { target, .. } => format!("extend {target}"),
        ActionStatement::Retract { target, .. } => format!("retract {target}"),
        ActionStatement::Set { target, value } => format!("set {target} {value}"),
        ActionStatement::SetAnalog { target, value } => format!("set_analog {target} {value}"),
        ActionStatement::SetAnalogExpr { target, expr } => {
            format!("set_analog_expr {target} {}", summarize_expr(expr))
        }
        ActionStatement::Compute { target, expr } => {
            format!("compute {target} = {}", summarize_expr(expr))
        }
        ActionStatement::Call {
            function, binding, ..
        } => format!("call {function} -> {binding:?}"),
        ActionStatement::CamEngage { target } => format!("cam engage {target}"),
        ActionStatement::CamDisengage { target } => format!("cam disengage {target}"),
        ActionStatement::CamSwitch { target, new_table } => {
            format!("cam switch {target} {new_table}")
        }
        ActionStatement::CamPhase { target, offset } => {
            format!("cam phase {target} {}", summarize_expr(offset))
        }
        ActionStatement::DeviceAction {
            family,
            action_name,
            target,
            ..
        } => format!("{family}.{action_name} {target}"),
        ActionStatement::AxisMoveRelative {
            target,
            distance,
            speed,
            ..
        } => format!("axis.move_relative {target} distance={distance} speed={speed:?}"),
        ActionStatement::AxisMoveAbsolute {
            target,
            position,
            speed,
            ..
        } => format!("axis.move_absolute {target} position={position} speed={speed:?}"),
        ActionStatement::Log { message } => format!("log {message}"),
    }
}

fn summarize_effect_statement(effect: &EffectStatement) -> String {
    match &effect.kind {
        EffectKind::Acquire { holder, from } => format!("acquire {holder} from {from}"),
        EffectKind::Transfer { from, to } => format!("transfer {from} -> {to}"),
        EffectKind::Finish { at, terminal_state } => format!("finish {at} as {terminal_state}"),
        EffectKind::Mount {
            workpiece_type,
            slot,
        } => format!("mount {workpiece_type} at {slot}"),
        EffectKind::Unmount {
            workpiece_type,
            slot,
            to,
        } => format!("unmount {workpiece_type} from {slot} to {to}"),
        EffectKind::Split {
            source_type,
            target_type,
            count,
            consumed,
        } => format!("split {source_type} -> {count} {target_type} consumed={consumed}"),
        EffectKind::Merge {
            inputs,
            target_type,
            consumed_inputs,
        } => format!("merge {inputs:?} -> {target_type} consumed={consumed_inputs}"),
        EffectKind::TransformCarrier { carrier, frame } => {
            format!("transform carrier {carrier} frame={frame}")
        }
    }
}

fn summarize_guard(guard: &TransitionGuard) -> String {
    match guard {
        TransitionGuard::Always => "always".to_string(),
        TransitionGuard::Condition { expression } => format!("when {expression}"),
        TransitionGuard::Edge { edge, operand } => format!("{edge:?}_edge({operand})"),
        TransitionGuard::Timeout { duration_ms } => format!("timeout {duration_ms}ms"),
        TransitionGuard::Delay { duration_ms } => format!("delay {duration_ms}ms"),
    }
}

fn summarize_transition_action(action: &TransitionAction) -> String {
    match action {
        TransitionAction::Extend { target, .. } => format!("extend {target}"),
        TransitionAction::Retract { target, .. } => format!("retract {target}"),
        TransitionAction::Set {
            target,
            port,
            value,
        } => format!("set {target}.{port} {value:?}"),
        TransitionAction::SetAnalog { target, port, .. } => format!("set_analog {target}.{port}"),
        TransitionAction::SetAnalogExpr {
            target,
            port,
            expr_raw,
        } => format!("set_analog_expr {target}.{port} = {expr_raw}"),
        TransitionAction::Compute { target, expr_raw } => format!("compute {target} = {expr_raw}"),
        TransitionAction::CallExtern { function, .. } => format!("call {function}"),
        TransitionAction::CamEngage { target } => format!("cam engage {target}"),
        TransitionAction::CamDisengage { target } => format!("cam disengage {target}"),
        TransitionAction::CamSwitch { target, new_table } => {
            format!("cam switch {target} {new_table}")
        }
        TransitionAction::CamPhase {
            target,
            offset_expr_raw,
        } => format!("cam phase {target} {offset_expr_raw}"),
        TransitionAction::DeviceAction {
            family,
            action_name,
            target,
            ..
        } => format!("{family}.{action_name} {target}"),
        TransitionAction::AxisMoveRelative {
            target,
            distance_raw,
            speed_raw,
            ..
        } => format!("axis.move_relative {target} distance={distance_raw} speed={speed_raw}"),
        TransitionAction::AxisMoveAbsolute {
            target,
            position_raw,
            speed_raw,
            ..
        } => format!("axis.move_absolute {target} position={position_raw} speed={speed_raw}"),
        TransitionAction::Log { message } => format!("log {message}"),
    }
}

fn summarize_workpiece_effect(effect: &WorkpieceEffect) -> String {
    match effect {
        WorkpieceEffect::Acquire { holder, from } => format!("acquire {holder} from {from}"),
        WorkpieceEffect::Transfer { from, to } => format!("transfer {from} -> {to}"),
        WorkpieceEffect::Finish { at, terminal_state } => {
            format!("finish {at} as {terminal_state}")
        }
        WorkpieceEffect::Mount {
            workpiece_type,
            slot,
        } => format!("mount {workpiece_type} at {slot}"),
        WorkpieceEffect::Unmount {
            workpiece_type,
            slot,
            to,
        } => format!("unmount {workpiece_type} from {slot} to {to}"),
        WorkpieceEffect::Split {
            source_type,
            target_type,
            count,
            consumed,
        } => format!("split {source_type} -> {count} {target_type} consumed={consumed}"),
        WorkpieceEffect::Merge {
            inputs,
            target_type,
            consumed_inputs,
        } => format!("merge {inputs:?} -> {target_type} consumed={consumed_inputs}"),
        WorkpieceEffect::TransformCarrier { carrier, frame } => {
            format!("transform carrier {carrier} frame={frame}")
        }
    }
}

fn summarize_wait_condition(condition: &WaitCondition) -> String {
    match condition {
        WaitCondition::Single(cond) => summarize_condition(cond),
        WaitCondition::And(conds) => conds
            .iter()
            .map(summarize_condition)
            .collect::<Vec<_>>()
            .join(" AND "),
        WaitCondition::Or(conds) => conds
            .iter()
            .map(summarize_condition)
            .collect::<Vec<_>>()
            .join(" OR "),
        WaitCondition::Edge(edge) => format!("{:?}_edge({})", edge.edge, edge.operand),
    }
}

fn summarize_condition(condition: &ConditionExpression) -> String {
    if let Some((left, right)) = condition.expression_pair() {
        return format!(
            "{} {} {}",
            summarize_expr(left),
            summarize_compare_op(&condition.operator),
            summarize_expr(right)
        );
    }
    format!(
        "{} {} {}",
        condition.left,
        summarize_compare_op(&condition.operator),
        summarize_literal(&condition.right)
    )
}

fn summarize_expr(expr: &Expression) -> String {
    match expr {
        Expression::Literal(value) => trim_float(*value),
        Expression::Boolean(value) => value.to_string(),
        Expression::Variable(name) => name.clone(),
        Expression::UnaryNeg(inner) => format!("-{}", summarize_expr(inner)),
        Expression::UnaryNot(inner) => format!("NOT {}", summarize_expr(inner)),
        Expression::BinaryOp { op, left, right } => format!(
            "({} {} {})",
            summarize_expr(left),
            summarize_binary_op(*op),
            summarize_expr(right)
        ),
        Expression::FunctionCall { name, args } => format!(
            "{}({})",
            name,
            args.iter()
                .map(summarize_expr)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn summarize_binary_op(op: rust_plc::ast::BinaryOperator) -> &'static str {
    match op {
        rust_plc::ast::BinaryOperator::Add => "+",
        rust_plc::ast::BinaryOperator::Sub => "-",
        rust_plc::ast::BinaryOperator::Mul => "*",
        rust_plc::ast::BinaryOperator::Div => "/",
        rust_plc::ast::BinaryOperator::Mod => "%",
        rust_plc::ast::BinaryOperator::Eq => "==",
        rust_plc::ast::BinaryOperator::Neq => "!=",
        rust_plc::ast::BinaryOperator::Gt => ">",
        rust_plc::ast::BinaryOperator::Lt => "<",
        rust_plc::ast::BinaryOperator::Gte => ">=",
        rust_plc::ast::BinaryOperator::Lte => "<=",
        rust_plc::ast::BinaryOperator::And => "AND",
        rust_plc::ast::BinaryOperator::Or => "OR",
    }
}

fn summarize_literal(value: &LiteralValue) -> String {
    match value {
        LiteralValue::Boolean(value) => value.to_string(),
        LiteralValue::Number(value) => trim_float(*value),
        LiteralValue::Measured(value) => format!("{}{}", trim_float(value.value), value.unit),
        LiteralValue::String(value) => format!("\"{value}\""),
        LiteralValue::State(state) => format!("{}.{} == {}", state.device, state.port, state.state),
    }
}

fn summarize_compare_op(op: &rust_plc::ast::ComparisonOperator) -> &'static str {
    match op {
        rust_plc::ast::ComparisonOperator::Eq => "==",
        rust_plc::ast::ComparisonOperator::Neq => "!=",
        rust_plc::ast::ComparisonOperator::Gt => ">",
        rust_plc::ast::ComparisonOperator::Lt => "<",
        rust_plc::ast::ComparisonOperator::Gte => ">=",
        rust_plc::ast::ComparisonOperator::Lte => "<=",
    }
}

fn summarize_duration(duration: &rust_plc::ast::DurationValue) -> String {
    format!("{}{:?}", duration.value, duration.unit).to_lowercase()
}

fn summarize_goto(goto: &rust_plc::ast::GotoDirective) -> String {
    match &goto.step {
        Some(step) => format!("{}.{}", goto.task, step),
        None => goto.task.clone(),
    }
}

fn trim_float(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
