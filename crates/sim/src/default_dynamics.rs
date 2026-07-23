use std::collections::{BTreeMap, BTreeSet};

use runtime_core::{Action, AxisMotionCommand, AxisMotionResult, AxisMoveKind, Instr, Program};

use crate::{CylinderConfig, LimitKind, LimitSensorConfig, Plant, SimIo, SolenoidValveConfig};

const DEFAULT_VALVE_RESPONSE_TICKS: u64 = 1;
const DEFAULT_CYLINDER_STROKE_TICKS: u64 = 3;
const DEFAULT_CYLINDER_RETRACT_TICKS: u64 = 3;
const DEFAULT_SENSOR_DEBOUNCE_TICKS: u64 = 1;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct InferredCylinder {
    output: usize,
    extended_inputs: BTreeSet<u16>,
    retracted_inputs: BTreeSet<u16>,
}

pub fn attach_inferred_plant_from_program<'a>(io: &mut SimIo, program: &'a Program<'a>) -> bool {
    if io.has_plant() {
        return false;
    }

    let mut cylinders: BTreeMap<u16, InferredCylinder> = BTreeMap::new();
    for task in program.tasks {
        for step in task.steps {
            let Instr::Action { actions, .. } = step.instr else {
                continue;
            };
            for action in actions {
                let Action::CylinderMotion {
                    output,
                    expect_extended,
                    confirm_inputs,
                    opposing_inputs,
                    ..
                } = *action
                else {
                    continue;
                };
                let entry = cylinders
                    .entry(output.0)
                    .or_insert_with(|| InferredCylinder {
                        output: output.0 as usize,
                        ..InferredCylinder::default()
                    });
                if expect_extended {
                    entry
                        .extended_inputs
                        .extend(confirm_inputs.iter().map(|id| id.0));
                    entry
                        .retracted_inputs
                        .extend(opposing_inputs.iter().map(|id| id.0));
                } else {
                    entry
                        .retracted_inputs
                        .extend(confirm_inputs.iter().map(|id| id.0));
                    entry
                        .extended_inputs
                        .extend(opposing_inputs.iter().map(|id| id.0));
                }
            }
        }
    }

    if cylinders.is_empty() {
        return false;
    }

    let mut plant = Plant::new();
    for cylinder in cylinders.into_values() {
        let valve = plant.add_solenoid_valve(SolenoidValveConfig {
            extend_coil: cylinder.output,
            retract_coil: None,
            response_ticks: DEFAULT_VALVE_RESPONSE_TICKS,
        });
        let cyl = plant.add_cylinder(CylinderConfig {
            valve,
            stroke_ticks: DEFAULT_CYLINDER_STROKE_TICKS,
            retract_ticks: DEFAULT_CYLINDER_RETRACT_TICKS,
        });
        for input in cylinder.extended_inputs {
            plant.add_limit_sensor(LimitSensorConfig {
                cylinder: cyl,
                kind: LimitKind::Extended,
                digital_input: io_traits::DigitalInputId(input),
                debounce_ticks: DEFAULT_SENSOR_DEBOUNCE_TICKS,
            });
        }
        for input in cylinder.retracted_inputs {
            plant.add_limit_sensor(LimitSensorConfig {
                cylinder: cyl,
                kind: LimitKind::Retracted,
                digital_input: io_traits::DigitalInputId(input),
                debounce_ticks: DEFAULT_SENSOR_DEBOUNCE_TICKS,
            });
        }
    }

    io.attach_plant(plant);
    true
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PendingAxisMotion {
    kind: AxisMoveKind,
    value: f32,
    speed: f32,
    destination: f32,
    remaining_polls: u64,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct DeterministicAxisDriver {
    positions: BTreeMap<String, f32>,
    pending: BTreeMap<String, PendingAxisMotion>,
}

impl DeterministicAxisDriver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle(&mut self, command: AxisMotionCommand) -> AxisMotionResult {
        if let Some(pending) = self.pending.get_mut(command.target) {
            if same_axis_motion(*pending, command) {
                if pending.remaining_polls <= 1 {
                    self.positions
                        .insert(command.target.to_string(), pending.destination);
                    self.pending.remove(command.target);
                    return AxisMotionResult::Done;
                }
                pending.remaining_polls -= 1;
                return AxisMotionResult::Pending;
            }
        }

        let current = self.positions.get(command.target).copied().unwrap_or(0.0);
        let destination = match command.kind {
            AxisMoveKind::Relative => current + command.value,
            AxisMoveKind::Absolute => command.value,
        };
        let polls = estimate_axis_polls(current, destination, command);
        if polls <= 1 {
            self.positions
                .insert(command.target.to_string(), destination);
            return AxisMotionResult::Done;
        }

        self.pending.insert(
            command.target.to_string(),
            PendingAxisMotion {
                kind: command.kind,
                value: command.value,
                speed: command.speed,
                destination,
                remaining_polls: polls - 1,
            },
        );
        AxisMotionResult::Pending
    }
}

fn same_axis_motion(pending: PendingAxisMotion, command: AxisMotionCommand) -> bool {
    pending.kind == command.kind && pending.value == command.value && pending.speed == command.speed
}

fn estimate_axis_polls(current: f32, destination: f32, command: AxisMotionCommand) -> u64 {
    let distance = (destination - current).abs();
    let raw = if distance <= f32::EPSILON {
        1
    } else {
        (distance / command.speed.max(0.001)).ceil() as u64
    };
    let capped = match command.timeout {
        Some(timeout) if timeout.after_ticks > 0 => {
            raw.min(timeout.after_ticks.saturating_sub(1).max(1))
        }
        _ => raw,
    };
    capped.max(1)
}
