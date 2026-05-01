#[cfg(test)]
mod tests {
    use super::*;
    use io_traits::{AnalogInputId, Tick};
    use std::{boxed::Box, vec, vec::Vec};

    struct MemIo {
        t: Tick,
        di: [bool; 4],
        do_: [bool; 4],
        ai: [f32; 4],
        ao: [f32; 4],
    }

    impl MemIo {
        fn new() -> Self {
            Self {
                t: Tick(0),
                di: [false; 4],
                do_: [false; 4],
                ai: [0.0; 4],
                ao: [0.0; 4],
            }
        }
    }

    impl Io for MemIo {
        fn tick(&self) -> Tick {
            self.t
        }

        fn advance_tick(&mut self) {
            self.t.0 += 1;
        }

        fn read_digital_input(&self, id: DigitalInputId) -> bool {
            self.di[id.0 as usize]
        }

        fn read_analog_input(&self, id: AnalogInputId) -> f32 {
            self.ai[id.0 as usize]
        }

        fn write_digital_output(&mut self, id: DigitalOutputId, value: bool) {
            self.do_[id.0 as usize] = value;
        }

        fn write_analog_output(&mut self, id: AnalogOutputId, value: f32) {
            self.ao[id.0 as usize] = value;
        }
    }

    fn build_cam_table(periodic: bool, points: &[(f32, f32)]) -> CamTableData {
        let mut master = [0.0f32; MAX_CAM_POINTS];
        let mut slave = [0.0f32; MAX_CAM_POINTS];
        let mut coeffs = [SplineCoeff {
            a: 0.0,
            b: 0.0,
            c: 0.0,
            d: 0.0,
        }; MAX_CAM_POINTS];

        for (idx, (x, y)) in points.iter().copied().enumerate() {
            master[idx] = x;
            slave[idx] = y;
        }
        for i in 0..points.len().saturating_sub(1) {
            let dx = master[i + 1] - master[i];
            let slope = if dx == 0.0 {
                0.0
            } else {
                (slave[i + 1] - slave[i]) / dx
            };
            coeffs[i] = SplineCoeff {
                a: slave[i],
                b: slope,
                c: 0.0,
                d: 0.0,
            };
        }

        CamTableData {
            periodic,
            num_points: points.len() as u16,
            master,
            slave,
            coeffs,
            last_index: 0,
        }
    }

    fn leak_steps(steps: Vec<Step<'static>>) -> &'static [Step<'static>] {
        Box::leak(steps.into_boxed_slice())
    }

    fn build_goto_chain_steps(chain_len: usize) -> &'static [Step<'static>] {
        assert!(chain_len > 0, "chain length should be positive");
        assert!(
            chain_len <= (u16::MAX as usize + 1),
            "chain length should fit in StepId"
        );

        let mut steps = vec![];
        for idx in 0..chain_len {
            let instr = if idx + 1 < chain_len {
                Instr::Goto {
                    target: StepId((idx + 1) as u16),
                }
            } else {
                Instr::Halt
            };
            steps.push(Step {
                name: "chain",
                instr,
            });
        }
        leak_steps(steps)
    }

    include!("runtime_workpiece_tests.rs");
    include!("runtime_scheduler_tests.rs");
    include!("runtime_math_tests.rs");
    include!("runtime_axis_tests.rs");
    include!("runtime_process_device_tests.rs");
    include!("runtime_misc_tests.rs");
}
