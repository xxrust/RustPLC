use runtime_core::{Action, Instr, Program};

pub(crate) fn io_sizes_for_program_and_scenario(
    program: &Program<'_>,
    scenario: &sim::Scenario,
) -> (usize, usize, usize, usize) {
    let mut max_di: Option<u16> = None;
    let mut max_do: Option<u16> = None;
    let mut max_ai: Option<u16> = None;
    let mut max_ao: Option<u16> = None;

    for task in program.tasks {
        for step in task.steps {
            match step.instr {
                Instr::WaitDigital { id, .. } => {
                    max_di = Some(max_di.map_or(id.0, |m| m.max(id.0)));
                }
                Instr::WaitAnalog { id, .. } => {
                    max_ai = Some(max_ai.map_or(id.0, |m| m.max(id.0)));
                }
                Instr::WaitAllDigital { conditions, .. } => {
                    for condition in conditions {
                        max_di = Some(max_di.map_or(condition.id.0, |m| m.max(condition.id.0)));
                    }
                }
                Instr::Action { actions, .. } => {
                    for action in actions {
                        match *action {
                            Action::SetDigital { id, .. }
                            | Action::Extend { output: id }
                            | Action::Retract { output: id } => {
                                max_do = Some(max_do.map_or(id.0, |m| m.max(id.0)));
                            }
                            Action::CylinderMotion {
                                output: id,
                                confirm_inputs,
                                opposing_inputs,
                                ..
                            } => {
                                max_do = Some(max_do.map_or(id.0, |m| m.max(id.0)));
                                for id in confirm_inputs {
                                    max_di = Some(max_di.map_or(id.0, |m| m.max(id.0)));
                                }
                                for id in opposing_inputs {
                                    max_di = Some(max_di.map_or(id.0, |m| m.max(id.0)));
                                }
                            }
                            Action::SetAnalog { id, .. } | Action::SetAnalogExpr { id, .. } => {
                                max_ao = Some(max_ao.map_or(id.0, |m| m.max(id.0)));
                            }
                            Action::Compute { .. }
                            | Action::CallExtern { .. }
                            | Action::AxisMove { .. }
                            | Action::ProcessDeviceAction { .. }
                            | Action::CamEngage { .. }
                            | Action::CamDisengage { .. }
                            | Action::CamSwitch { .. }
                            | Action::CamPhase { .. }
                            | Action::WorkpieceAcquire { .. }
                            | Action::WorkpieceTransfer { .. }
                            | Action::WorkpieceFinish { .. }
                            | Action::WorkpieceMount { .. }
                            | Action::WorkpieceUnmount { .. }
                            | Action::WorkpieceTransformCarrier { .. }
                            | Action::WorkpieceSplit { .. }
                            | Action::WorkpieceMerge { .. }
                            | Action::Log { .. } => {}
                        }
                    }
                }
                Instr::WaitExpr { .. }
                | Instr::WaitCamDigital { .. }
                | Instr::WaitCamAnalog { .. }
                | Instr::Delay { .. }
                | Instr::Goto { .. }
                | Instr::Halt => {}
            }
        }
    }
    for cam in program.cam_configs {
        max_ai = Some(max_ai.map_or(cam.master_input.0, |m| m.max(cam.master_input.0)));
        max_ai = Some(max_ai.map_or(cam.slave_feedback.0, |m| m.max(cam.slave_feedback.0)));
        max_ao = Some(max_ao.map_or(cam.slave_output.0, |m| m.max(cam.slave_output.0)));
    }
    for pid in program.pid_loops {
        max_ai = Some(max_ai.map_or(pid.pv.0, |m| m.max(pid.pv.0)));
        max_ao = Some(max_ao.map_or(pid.out.0, |m| m.max(pid.out.0)));
    }

    for input in &scenario.inputs {
        for (&id, _) in &input.set.digital_inputs {
            max_di = Some(max_di.map_or(id, |m| m.max(id)));
        }
        for (&id, _) in &input.set.analog_inputs {
            max_ai = Some(max_ai.map_or(id, |m| m.max(id)));
        }
    }
    for fault in &scenario.faults {
        let id = fault.sensor_stuck.target;
        max_di = Some(max_di.map_or(id, |m| m.max(id)));
    }

    let num_di = max_di.map(|m| m as usize + 1).unwrap_or(0).max(1);
    let num_do = max_do.map(|m| m as usize + 1).unwrap_or(0).max(1);
    let num_ai = max_ai.map(|m| m as usize + 1).unwrap_or(0);
    let num_ao = max_ao.map(|m| m as usize + 1).unwrap_or(0);
    (num_di, num_do, num_ai, num_ao)
}

pub(crate) fn is_halted<'a>(rt: &runtime_core::Runtime<'a>, program: &'a Program<'a>) -> bool {
    if rt.active_task_count() == 0 {
        return false;
    }

    (0..rt.active_task_count()).all(|task_idx| {
        let Ok(ctx) = rt.task_context(task_idx) else {
            return false;
        };
        let Ok(task) = program.task(task_idx) else {
            return false;
        };
        let Some(step) = task.step(ctx.current_step) else {
            return false;
        };
        matches!(step.instr, Instr::Halt)
    })
}
