use crate::ir::{StateMachine, TransitionAction};
use petgraph::algo::kosaraju_scc;
use petgraph::graph::DiGraph;
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet};

/// Select root task contexts from the condensed task graph.
///
/// Raw cross-task edges are first collapsed by SCC. Every condensed component
/// without an incoming edge is active at startup. If the condensed graph has no
/// root, the IR initial task is the only explicit fallback.
pub(crate) fn select_root_task_contexts<F>(
    state_machine: &StateMachine,
    mut extra_target_tasks: F,
) -> Vec<String>
where
    F: FnMut(&[TransitionAction]) -> Vec<String>,
{
    let mut declared_tasks = Vec::new();
    let mut declared_set = HashSet::<String>::new();
    for ctx in &state_machine.task_contexts {
        if declared_set.insert(ctx.task_name.clone()) {
            declared_tasks.push(ctx.task_name.clone());
        }
    }

    if declared_tasks.is_empty() {
        return fallback_root_tasks(state_machine, &declared_set, &declared_tasks);
    }

    let mut graph = DiGraph::<String, ()>::new();
    let mut nodes = HashMap::new();
    for task_name in &declared_tasks {
        nodes.insert(task_name.clone(), graph.add_node(task_name.clone()));
    }

    let mut inserted_edges = HashSet::<(String, String)>::new();
    for transition in &state_machine.transitions {
        push_task_edge(
            &mut graph,
            &nodes,
            &declared_set,
            &mut inserted_edges,
            &transition.from.task_name,
            &transition.to.task_name,
        );
        for target_task in extra_target_tasks(&transition.actions) {
            push_task_edge(
                &mut graph,
                &nodes,
                &declared_set,
                &mut inserted_edges,
                &transition.from.task_name,
                &target_task,
            );
        }
    }

    let components = kosaraju_scc(&graph);
    let mut component_by_task = HashMap::<String, usize>::new();
    for (component_idx, component) in components.iter().enumerate() {
        for node in component {
            component_by_task.insert(graph[*node].clone(), component_idx);
        }
    }

    let mut component_has_external_incoming = vec![false; components.len()];
    for edge in graph.edge_references() {
        let source_task = &graph[edge.source()];
        let target_task = &graph[edge.target()];
        let Some(&source_component) = component_by_task.get(source_task) else {
            continue;
        };
        let Some(&target_component) = component_by_task.get(target_task) else {
            continue;
        };
        if source_component != target_component {
            component_has_external_incoming[target_component] = true;
        }
    }

    let mut roots = Vec::new();
    let mut emitted_components = HashSet::<usize>::new();
    for task_name in &declared_tasks {
        let Some(&component_idx) = component_by_task.get(task_name) else {
            continue;
        };
        let is_selected_root = !component_has_external_incoming[component_idx];
        if !is_selected_root || !emitted_components.insert(component_idx) {
            continue;
        }
        roots.push(task_name.clone());
    }

    if roots.is_empty() {
        return fallback_root_tasks(state_machine, &declared_set, &declared_tasks);
    }

    roots
}

fn push_task_edge(
    graph: &mut DiGraph<String, ()>,
    nodes: &HashMap<String, petgraph::graph::NodeIndex>,
    declared_set: &HashSet<String>,
    inserted_edges: &mut HashSet<(String, String)>,
    from_task: &str,
    to_task: &str,
) {
    if from_task == to_task
        || !declared_set.contains(from_task)
        || !declared_set.contains(to_task)
        || !inserted_edges.insert((from_task.to_string(), to_task.to_string()))
    {
        return;
    }
    let Some(&from) = nodes.get(from_task) else {
        return;
    };
    let Some(&to) = nodes.get(to_task) else {
        return;
    };
    graph.add_edge(from, to, ());
}

fn fallback_root_tasks(
    state_machine: &StateMachine,
    declared_set: &HashSet<String>,
    declared_tasks: &[String],
) -> Vec<String> {
    if declared_set.contains(&state_machine.initial.task_name) {
        vec![state_machine.initial.task_name.clone()]
    } else if let Some(first) = declared_tasks.first() {
        vec![first.clone()]
    } else if !state_machine.initial.task_name.is_empty() {
        vec![state_machine.initial.task_name.clone()]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::select_root_task_contexts;
    use crate::ir::{
        MotionFaultBranch, MotionTimeoutBranch, State, StateMachine, TaskExecutionContext,
        Transition, TransitionAction, TransitionGuard,
    };

    #[test]
    fn selects_scc_representatives_for_fault_recovery_domains() {
        let state_machine = StateMachine {
            states: vec![
                state("startup", "wait"),
                state("startup_fault", "recover"),
                state("supervisor", "wait_start"),
                state("warning", "refresh"),
                state("background", "monitor"),
                state("worker", "run"),
            ],
            transitions: vec![
                transition("startup", "wait", "startup_fault", "recover"),
                transition("startup_fault", "recover", "startup", "wait"),
                transition("supervisor", "wait_start", "warning", "refresh"),
                transition("warning", "refresh", "supervisor", "wait_start"),
                transition("supervisor", "wait_start", "worker", "run"),
            ],
            initial: state("startup", "wait"),
            analog_regions: Default::default(),
            task_contexts: vec![
                task_ctx("startup", "wait"),
                task_ctx("startup_fault", "recover"),
                task_ctx("supervisor", "wait_start"),
                task_ctx("warning", "refresh"),
                task_ctx("background", "monitor"),
                task_ctx("worker", "run"),
            ],
        };

        let roots = select_root_task_contexts(&state_machine, |_| Vec::new());
        assert_eq!(roots, vec!["startup", "supervisor", "background"]);
    }

    #[test]
    fn motion_branch_targets_contribute_to_root_condensation() {
        let state_machine = StateMachine {
            states: vec![state("cycle", "run"), state("fault", "recover")],
            transitions: vec![
                Transition {
                    from: state("cycle", "run"),
                    to: state("cycle", "run"),
                    guard: TransitionGuard::Always,
                    actions: vec![TransitionAction::Extend {
                        target: "cyl".to_string(),
                        port: "self".to_string(),
                        timeout: Some(MotionTimeoutBranch {
                            duration_ms: 100,
                            target_task: "fault".to_string(),
                            target_step: None,
                        }),
                        on_motion_fault: Some(MotionFaultBranch {
                            target_task: "fault".to_string(),
                            target_step: None,
                        }),
                        on_safety_fault: None,
                    }],
                    effects: Vec::new(),
                    timers: Vec::new(),
                },
                Transition {
                    from: state("fault", "recover"),
                    to: state("cycle", "run"),
                    guard: TransitionGuard::Always,
                    actions: Vec::new(),
                    effects: Vec::new(),
                    timers: Vec::new(),
                },
            ],
            initial: state("cycle", "run"),
            analog_regions: Default::default(),
            task_contexts: vec![task_ctx("cycle", "run"), task_ctx("fault", "recover")],
        };

        let roots = select_root_task_contexts(&state_machine, |actions| {
            actions
                .iter()
                .flat_map(|action| match action {
                    TransitionAction::Extend {
                        timeout,
                        on_motion_fault,
                        on_safety_fault,
                        ..
                    } => {
                        let mut targets = Vec::new();
                        if let Some(timeout) = timeout {
                            targets.push(timeout.target_task.clone());
                        }
                        if let Some(on_motion_fault) = on_motion_fault {
                            targets.push(on_motion_fault.target_task.clone());
                        }
                        if let Some(on_safety_fault) = on_safety_fault {
                            targets.push(on_safety_fault.target_task.clone());
                        }
                        targets
                    }
                    _ => Vec::new(),
                })
                .collect()
        });

        assert_eq!(roots, vec!["cycle"]);
    }

    #[test]
    fn root_selection_uses_graph_edges_without_task_name_heuristics() {
        let state_machine = StateMachine {
            states: vec![
                state("supervisor", "wait_start"),
                state("feed_fault", "wait_refresh"),
                state("feed_warning", "wait_refresh"),
            ],
            transitions: vec![
                transition("feed_fault", "wait_refresh", "supervisor", "wait_start"),
                transition("feed_warning", "wait_refresh", "supervisor", "wait_start"),
            ],
            initial: state("supervisor", "wait_start"),
            analog_regions: Default::default(),
            task_contexts: vec![
                task_ctx("supervisor", "wait_start"),
                task_ctx("feed_fault", "wait_refresh"),
                task_ctx("feed_warning", "wait_refresh"),
            ],
        };

        let roots = select_root_task_contexts(&state_machine, |_| Vec::new());
        assert_eq!(roots, vec!["feed_fault", "feed_warning"]);
    }

    fn state(task_name: &str, step_name: &str) -> State {
        State {
            task_name: task_name.to_string(),
            step_name: step_name.to_string(),
        }
    }

    fn task_ctx(task_name: &str, step_name: &str) -> TaskExecutionContext {
        TaskExecutionContext {
            task_name: task_name.to_string(),
            entry_state: state(task_name, step_name),
            current_state: state(task_name, step_name),
            ..Default::default()
        }
    }

    fn transition(from_task: &str, from_step: &str, to_task: &str, to_step: &str) -> Transition {
        Transition {
            from: state(from_task, from_step),
            to: state(to_task, to_step),
            guard: TransitionGuard::Always,
            actions: Vec::new(),
            effects: Vec::new(),
            timers: Vec::new(),
        }
    }
}
