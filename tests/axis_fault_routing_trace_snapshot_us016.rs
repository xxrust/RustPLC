use rust_plc::ir::{
    AxisFaultCategory, AxisFaultKind, AxisFaultRouteBranch, AxisFaultRouteKind, StateMachine,
    TransitionAction, resolve_axis_fault_route_target,
};
use rust_plc::parser::parse_plc;
use rust_plc::semantic::{build_state_machine, preprocess_program};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

fn read_example(file_name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(file_name);
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read example {}: {err}", path.display()))
}

fn build_example_state_machine(file_name: &str) -> StateMachine {
    let source = read_example(file_name);
    let program = parse_plc(&source).expect("example should parse");
    let expanded = preprocess_program(&program).expect("example should preprocess");
    build_state_machine(&expanded).expect("example should lower to state machine")
}

fn find_axis_move_action(state_machine: &StateMachine) -> &TransitionAction {
    state_machine
        .transitions
        .iter()
        .flat_map(|transition| transition.actions.iter())
        .find(|action| {
            matches!(
                action,
                TransitionAction::AxisMoveRelative { .. }
                    | TransitionAction::AxisMoveAbsolute { .. }
            )
        })
        .expect("example should contain one axis move action")
}

fn format_target(task: &str, step: Option<&str>) -> String {
    match step {
        Some(step) => format!("{task}.{step}"),
        None => task.to_string(),
    }
}

fn resolve_route_target(
    primary: &rust_plc::ir::AxisFaultBranch,
    routes: &[AxisFaultRouteBranch],
    fault_kind: AxisFaultKind,
    error_code: i32,
) -> String {
    let (task, step) = resolve_axis_fault_route_target(primary, routes, &fault_kind, error_code);
    format_target(task, step)
}

#[test]
fn axis_fault_routing_trace_snapshot_covers_recoverable_nonrecoverable_and_safety_paths() {
    let recoverable_sm = build_example_state_machine("axis_fault_recoverable_path.plc");
    let nonrecoverable_sm = build_example_state_machine("axis_fault_nonrecoverable_path.plc");
    let safety_sm = build_example_state_machine("axis_fault_safety_path.plc");

    let mut rows: Vec<Value> = Vec::new();

    if let TransitionAction::AxisMoveRelative {
        on_reject,
        on_reject_routes,
        ..
    }
    | TransitionAction::AxisMoveAbsolute {
        on_reject,
        on_reject_routes,
        ..
    } = find_axis_move_action(&recoverable_sm)
    {
        rows.push(json!({
            "example": "recoverable",
            "bucket": "reject",
            "kind": "reject",
            "code": 41,
            "target": resolve_route_target(on_reject, on_reject_routes, AxisFaultKind::Reject, 41),
        }));
        rows.push(json!({
            "example": "recoverable",
            "bucket": "reject",
            "kind": "vendor",
            "code": 9911,
            "target": resolve_route_target(
                on_reject,
                on_reject_routes,
                AxisFaultKind::Vendor {
                    category: AxisFaultCategory::Recoverable,
                    vendor_code: 9911,
                },
                9911,
            ),
        }));
        rows.push(json!({
            "example": "recoverable",
            "bucket": "reject",
            "kind": "reject",
            "code": 1201,
            "target": resolve_route_target(on_reject, on_reject_routes, AxisFaultKind::Reject, 1201),
        }));
    }

    if let TransitionAction::AxisMoveRelative {
        on_motion_fault,
        on_motion_fault_routes,
        ..
    }
    | TransitionAction::AxisMoveAbsolute {
        on_motion_fault,
        on_motion_fault_routes,
        ..
    } = find_axis_move_action(&nonrecoverable_sm)
    {
        rows.push(json!({
            "example": "nonrecoverable",
            "bucket": "motion_fault",
            "kind": "motion",
            "code": 88,
            "target": resolve_route_target(on_motion_fault, on_motion_fault_routes, AxisFaultKind::Motion, 88),
        }));
        rows.push(json!({
            "example": "nonrecoverable",
            "bucket": "motion_fault",
            "kind": "vendor",
            "code": 9922,
            "target": resolve_route_target(
                on_motion_fault,
                on_motion_fault_routes,
                AxisFaultKind::Vendor {
                    category: AxisFaultCategory::NonRecoverable,
                    vendor_code: 9922,
                },
                9922,
            ),
        }));
        rows.push(json!({
            "example": "nonrecoverable",
            "bucket": "motion_fault",
            "kind": "motion",
            "code": 2202,
            "target": resolve_route_target(on_motion_fault, on_motion_fault_routes, AxisFaultKind::Motion, 2202),
        }));
    }

    if let TransitionAction::AxisMoveRelative {
        on_safety_fault,
        on_safety_fault_routes,
        ..
    }
    | TransitionAction::AxisMoveAbsolute {
        on_safety_fault,
        on_safety_fault_routes,
        ..
    } = find_axis_move_action(&safety_sm)
    {
        rows.push(json!({
            "example": "safety",
            "bucket": "safety_fault",
            "kind": "safety",
            "code": 99,
            "target": resolve_route_target(on_safety_fault, on_safety_fault_routes, AxisFaultKind::Safety, 99),
        }));
        rows.push(json!({
            "example": "safety",
            "bucket": "safety_fault",
            "kind": "vendor",
            "code": 9933,
            "target": resolve_route_target(
                on_safety_fault,
                on_safety_fault_routes,
                AxisFaultKind::Vendor {
                    category: AxisFaultCategory::Safety,
                    vendor_code: 9933,
                },
                9933,
            ),
        }));
        rows.push(json!({
            "example": "safety",
            "bucket": "safety_fault",
            "kind": "safety",
            "code": 3303,
            "target": resolve_route_target(on_safety_fault, on_safety_fault_routes, AxisFaultKind::Safety, 3303),
        }));
    }

    assert_eq!(
        rows,
        vec![
            json!({"example":"recoverable","bucket":"reject","kind":"reject","code":41,"target":"fault.reject_default"}),
            json!({"example":"recoverable","bucket":"reject","kind":"vendor","code":9911,"target":"fault.reject_vendor"}),
            json!({"example":"recoverable","bucket":"reject","kind":"reject","code":1201,"target":"fault.reject_code_1201"}),
            json!({"example":"nonrecoverable","bucket":"motion_fault","kind":"motion","code":88,"target":"fault.motion_default"}),
            json!({"example":"nonrecoverable","bucket":"motion_fault","kind":"vendor","code":9922,"target":"fault.motion_vendor"}),
            json!({"example":"nonrecoverable","bucket":"motion_fault","kind":"motion","code":2202,"target":"fault.motion_code_2202"}),
            json!({"example":"safety","bucket":"safety_fault","kind":"safety","code":99,"target":"fault.safety_default"}),
            json!({"example":"safety","bucket":"safety_fault","kind":"vendor","code":9933,"target":"fault.safety_vendor"}),
            json!({"example":"safety","bucket":"safety_fault","kind":"safety","code":3303,"target":"fault.safety_code_3303"}),
        ]
    );
}

#[test]
fn axis_fault_route_branch_kind_and_code_matcher_still_acts_as_strict_whitelist() {
    let branch = AxisFaultRouteBranch {
        target_task: "fault".to_string(),
        target_step: Some("motion_vendor".to_string()),
        kind: Some(AxisFaultRouteKind::Vendor),
        code: Some(42),
    };

    assert!(branch.matches(AxisFaultRouteKind::Vendor, 42));
    assert!(!branch.matches(AxisFaultRouteKind::Motion, 42));
    assert!(!branch.matches(AxisFaultRouteKind::Vendor, 43));
}
