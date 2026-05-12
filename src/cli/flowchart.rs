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

    for idx in 0..step_count {
        if !internal_by_target.contains_key(&idx) {
            continue;
        }
        let center_x = lane_center_x(lanes[idx]);
        let entry_y = layouts[idx].y - 18;
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
                let target_entry_y = layouts[to_idx].y - 18;

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
    let mut task_nav = String::new();
    let mut task_sections = String::new();
    for (idx, task) in model.tasks.iter().enumerate() {
        let tab_id = format!("task-{idx}");
        let _ = writeln!(
            task_nav,
            "<button class=\"tab-button\" data-target=\"{tab_id}\">{}</button>",
            html_escape(&task.task_name)
        );
        let _ = writeln!(
            task_sections,
            "<section id=\"{tab_id}\" class=\"tab-panel\"><h2>{}</h2><div class=\"diagram\">{}</div></section>",
            html_escape(&task.task_name),
            render_task_svg(task)
        );
    }

    format!(
        r#"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title}</title>
  <style>
    :root {{
      --bg: #f5f1e8;
      --ink: #1f2933;
      --muted: #617080;
      --card: #fffaf0;
      --line: #d8c7a5;
      --accent: #0f766e;
      --accent-dark: #134e4a;
      --warn: #b7791f;
    }}
    * {{ box-sizing: border-box; }}
    body {{ margin: 0; font-family: Georgia, "Noto Serif SC", "Songti SC", serif; background: linear-gradient(135deg, #f5f1e8, #e8f2ef); color: var(--ink); }}
    header {{ padding: 28px 34px 18px; border-bottom: 1px solid var(--line); background: rgba(255, 250, 240, 0.88); position: sticky; top: 0; z-index: 2; backdrop-filter: blur(10px); }}
    h1 {{ margin: 0 0 8px; font-size: 30px; letter-spacing: -0.02em; }}
    .subtitle {{ color: var(--muted); font-size: 14px; }}
    main {{ display: grid; grid-template-columns: 280px 1fr; gap: 22px; padding: 22px; }}
    nav {{ align-self: start; position: sticky; top: 112px; background: rgba(255,250,240,0.92); border: 1px solid var(--line); border-radius: 18px; padding: 14px; box-shadow: 0 14px 40px rgba(31,41,51,0.08); }}
    .nav-title {{ color: var(--muted); font-size: 12px; text-transform: uppercase; letter-spacing: 0.14em; margin: 8px 8px 10px; }}
    button.tab-button {{ display: block; width: 100%; border: 0; background: transparent; text-align: left; padding: 10px 12px; border-radius: 12px; color: var(--ink); cursor: pointer; font: inherit; }}
    button.tab-button:hover, button.tab-button.active {{ background: #d9f3ee; color: var(--accent-dark); }}
    .content {{ min-width: 0; }}
    .tab-panel {{ display: none; background: rgba(255,250,240,0.92); border: 1px solid var(--line); border-radius: 22px; padding: 22px; box-shadow: 0 20px 60px rgba(31,41,51,0.08); }}
    .tab-panel.active {{ display: block; animation: rise 160ms ease-out; }}
    @keyframes rise {{ from {{ opacity: 0; transform: translateY(6px); }} to {{ opacity: 1; transform: translateY(0); }} }}
    .diagram {{ overflow: auto; background: #fffdf7; border: 1px solid #eadcc1; border-radius: 16px; padding: 12px; margin: 14px 0 4px; }}
    .flow-svg {{ display: block; min-width: 680px; max-width: none; }}
    .sfc-review {{ width: 100%; }}
    .task-sfc-svg {{ display: block; width: 100%; min-width: 980px; height: auto; }}
    .task-step {{ fill: #fffaf0; stroke: #0f766e; stroke-width: 3; }}
    .task-step.generated {{ fill: #f8fafc; stroke: #94a3b8; stroke-dasharray: 8 6; }}
    .task-step-initial {{ fill: none; stroke: #0f766e; stroke-width: 3; }}
    .task-step-index {{ fill: #94a3b8; font: 700 14px "Cascadia Mono", Consolas, monospace; }}
    .task-step-title {{ fill: #093f3b; font: 700 16px "Cascadia Mono", Consolas, monospace; }}
    .task-step-source {{ fill: #64748b; font: 12px Georgia, "Noto Serif SC", serif; }}
    .task-action-link {{ stroke: #94a3b8; stroke-width: 1.3; }}
    .task-action {{ fill: #f8fbff; stroke: #c9d6e2; stroke-width: 1.2; }}
    .task-action-line {{ fill: #111827; font: 12px "Cascadia Mono", Consolas, monospace; }}
    .task-main-line {{ stroke: #111827; stroke-width: 2; }}
    .task-transition-bar {{ stroke: #111827; stroke-width: 6; stroke-linecap: square; }}
    .task-transition-branch-bus {{ stroke: #7c6750; stroke-width: 1.7; }}
    .task-transition-branch-bar {{ stroke: #111827; stroke-width: 5; stroke-linecap: square; }}
    .task-transition-link {{ stroke: #c7b28a; stroke-width: 1.6; }}
    .task-transition-note {{ fill: #fffdf7; stroke: #d7ccb7; stroke-width: 1.1; }}
    .task-transition-note.external {{ stroke: #d7b674; }}
    .task-transition-note.terminal {{ stroke: #b8c3cf; }}
    .task-transition-guard {{ fill: #111827; font: 700 12px "Cascadia Mono", Consolas, monospace; }}
    .task-transition-fact {{ fill: #475569; font: 11px "Cascadia Mono", Consolas, monospace; }}
    .task-transition-target {{ fill: #134e4a; font: 700 11px "Cascadia Mono", Consolas, monospace; }}
    .task-transition-target.external {{ fill: #b7791f; }}
    .task-transition-target.terminal {{ fill: #64748b; }}
    .flow-node {{ fill: #fffaf0; stroke: #0f766e; stroke-width: 1.6; }}
    .flow-node.generated {{ fill: #f8fafc; stroke: #94a3b8; stroke-dasharray: 6 4; }}
    .flow-node.external {{ fill: #fff3cd; stroke: #b7791f; }}
    .flow-title {{ font: 700 14px "Cascadia Mono", Consolas, monospace; fill: #134e4a; }}
    .flow-subtitle {{ font: 12px Georgia, "Noto Serif SC", serif; fill: #617080; }}
    .flow-edge {{ fill: none; stroke: #64748b; stroke-width: 1.5; marker-end: url(#arrow); }}
    .flow-edge.external {{ stroke: #b7791f; }}
    .flow-label-bg {{ fill: #fffdf7; stroke: #eadcc1; }}
    .flow-label {{ font: 12px "Cascadia Mono", Consolas, monospace; fill: #334155; }}
    table {{ width: 100%; border-collapse: collapse; font-size: 13px; }}
    th, td {{ border-bottom: 1px solid #eadcc1; padding: 9px 8px; vertical-align: top; }}
    th {{ color: var(--muted); text-align: left; font-weight: 600; }}
    code {{ font-family: "Cascadia Mono", Consolas, monospace; background: #eef7f5; color: #0f4f46; border-radius: 6px; padding: 1px 5px; }}
    .pill {{ display: inline-block; margin: 2px 4px 2px 0; padding: 2px 7px; border-radius: 999px; background: #edf7f4; color: var(--accent-dark); font-size: 12px; }}
    .grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 14px; }}
    .card {{ background: #fffdf7; border: 1px solid #eadcc1; border-radius: 16px; padding: 14px; }}
    .card h3 {{ margin-top: 0; }}
    ul.compact {{ margin: 0; padding-left: 18px; }}
    @media (max-width: 900px) {{ main {{ grid-template-columns: 1fr; }} nav {{ position: static; }} header {{ position: static; }} }}
  </style>
</head>
<body>
  <header>
    <h1>{title}</h1>
    <div class="subtitle">source: <code>{source}</code> | tasks: {task_count} | transitions: {transition_count}</div>
  </header>
  <main>
    <nav>
      <div class="nav-title">Project</div>
      <button class="tab-button active" data-target="overview">Overview</button>
      <button class="tab-button" data-target="topology">Topology</button>
      <div class="nav-title">Tasks</div>
      {task_nav}
    </nav>
    <div class="content">
      <section id="overview" class="tab-panel active">
        <h2>Overview</h2>
        <div class="diagram">{overview}</div>
      </section>
      <section id="topology" class="tab-panel">
        <h2>Topology</h2>
        {topology}
      </section>
      {task_sections}
    </div>
  </main>
  <script>
    document.querySelectorAll('.tab-button').forEach(button => {{
      button.addEventListener('click', () => {{
        document.querySelectorAll('.tab-button').forEach(item => item.classList.remove('active'));
        document.querySelectorAll('.tab-panel').forEach(item => item.classList.remove('active'));
        button.classList.add('active');
        const target = document.getElementById(button.dataset.target);
        if (target) target.classList.add('active');
      }});
    }});
  </script>
</body>
</html>
"#,
        title = html_escape(&model.title),
        source = html_escape(&model.source_plc),
        task_count = model.tasks.len(),
        transition_count = model
            .tasks
            .iter()
            .map(|task| task.transitions.len())
            .sum::<usize>(),
        task_nav = task_nav,
        overview = render_overview_svg(model),
        topology = render_topology_html(&model.topology),
        task_sections = task_sections
    )
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
        "<div class=\"grid\"><div class=\"card\"><h3>计数</h3><p>devices: <code>{}</code></p><p>links: <code>{}</code></p></div><div class=\"card\"><h3>Variables</h3>{}</div><div class=\"card\"><h3>Workpieces</h3>{}</div><div class=\"card\"><h3>Links</h3>{}</div></div>",
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
