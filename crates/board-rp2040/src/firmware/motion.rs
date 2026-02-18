#![cfg(target_os = "none")]

use super::{AllPins, PicoIo};
use crate::firmware::io_map;

use embedded_hal_0_2::digital::v2::OutputPin;
use rp_pico::hal::{
    gpio::{
        DynPinId, FunctionNull, FunctionPio0, FunctionSioOutput, OutputEnableOverride, Pin,
        PullDown,
    },
    pac,
    pio::{PIOBuilder, PIOExt, PinDir, Rx, Running, SM0, SM1, SM2, SM3, StateMachine, Tx},
};

// PIO instruction helpers.
use pio::{Instruction, InstructionOperands, MovDestination, MovOperation, MovSource};

// -------------------------------------------------------------------------------------------------
// Motion channel mapping (dev-stage convention)
// -------------------------------------------------------------------------------------------------
//
// We intentionally use a fixed, explicit mapping so that:
// - the firmware can consume motion commands without growing the DSL/IR yet,
// - feedback signals can be provided as "virtual" DI/AI channels (io_map GPIO = "virtual"),
// - examples/scenarios can be deterministic and self-contained.
//
// Axis0 uses channels near the top of the 0..31 range to reduce collision with "typical" examples.
// Axis1 is offset by +2 for DO and by +2 for AO/AI/DI blocks.
const AXIS0_DO_ENABLE: usize = 24;
const AXIS0_DO_DIR: usize = 25;
const AXIS0_AO_VEL_SPS: usize = 24;

const AXIS0_AI_COUNT: usize = 24;
const AXIS0_AI_SPEED: usize = 25;
const AXIS0_DI_DIR_POSITIVE: usize = 24;

const AXIS1_DO_ENABLE: usize = 26;
const AXIS1_DO_DIR: usize = 27;
const AXIS1_AO_VEL_SPS: usize = 26;

const AXIS1_AI_COUNT: usize = 26;
const AXIS1_AI_SPEED: usize = 27;
const AXIS1_DI_DIR_POSITIVE: usize = 26;

// -------------------------------------------------------------------------------------------------
// Public subsystem
// -------------------------------------------------------------------------------------------------

pub(super) struct Motion {
    axis0: Axis<SM0, SM2>,
    axis1: Axis<SM1, SM3>,
}

impl Motion {
    pub(super) fn initialize(
        pins: &mut AllPins,
        pio0: pac::PIO0,
        resets: &mut pac::RESETS,
        sys_hz: u32,
    ) -> Self {
        let (mut pio, sm0, sm1, sm2, sm3) = pio0.split(resets);

        // STEP generator program (shared across both stepper axes).
        //
        // TX FIFO feeds `half_period_cycles` (u32). The program pulls non-blocking each cycle:
        // - if `half_period_cycles == 0`, it idles with STEP low.
        // - otherwise it toggles STEP high/low with a wait loop based on the half-period.
        let step_program = pio_proc::pio_asm!(
            "loop:",
            "pull noblock",
            "mov x, osr",
            "jmp !x, idle",
            "set pins, 1",
            "mov y, x",
            "high:",
            "jmp y--, high",
            "set pins, 0",
            "mov y, x",
            "low:",
            "jmp y--, low",
            "jmp loop",
            "idle:",
            "set pins, 0",
            "jmp loop"
        );
        let installed_step = pio.install(&step_program.program).expect("install step pio");
        let installed_step_1 = unsafe { installed_step.share() };

        // Quadrature counter program (shared across both encoder axes).
        //
        // This is adapted from the public-domain PIO quadrature example:
        // it keeps the position counter in X and updates it on pin edges.
        // We snapshot X at tick boundaries via `MOV ISR, X; PUSH` from CPU.
        let quad_program = pio_proc::pio_asm!(
            // A-driven program: `wait pin 0` observes the IN base pin, and `jmp PIN` observes jmp_pin.
            "start:",
            "wait 1 pin 0",
            "jmp pin wait_high",
            "mov y, !x",
            "jmp y-- inc1",
            "inc1:",
            "mov x, !y",
            "jmp dec_done1",
            "wait_high:",
            "jmp x-- dec_done1",
            "dec_done1:",
            "wait 0 pin 0",
            "jmp pin wait_low",
            "jmp x-- start",
            "wait_low:",
            "mov y, !x",
            "jmp y-- inc2",
            "inc2:",
            "mov x, !y",
            "jmp start"
        );
        let installed_quad = pio.install(&quad_program.program).expect("install quad pio");
        let installed_quad_1 = unsafe { installed_quad.share() };

        let axis0 = Axis::new(
            0,
            pins,
            sys_hz,
            installed_step,
            sm0,
            installed_quad,
            sm2,
            AxisChannels {
                do_enable: AXIS0_DO_ENABLE,
                do_dir: AXIS0_DO_DIR,
                ao_vel_sps: AXIS0_AO_VEL_SPS,
                ai_count: AXIS0_AI_COUNT,
                ai_speed: AXIS0_AI_SPEED,
                di_dir_positive: AXIS0_DI_DIR_POSITIVE,
            },
        );

        let axis1 = Axis::new(
            1,
            pins,
            sys_hz,
            installed_step_1,
            sm1,
            installed_quad_1,
            sm3,
            AxisChannels {
                do_enable: AXIS1_DO_ENABLE,
                do_dir: AXIS1_DO_DIR,
                ao_vel_sps: AXIS1_AO_VEL_SPS,
                ai_count: AXIS1_AI_COUNT,
                ai_speed: AXIS1_AI_SPEED,
                di_dir_positive: AXIS1_DI_DIR_POSITIVE,
            },
        );

        Self { axis0, axis1 }
    }

    /// Update motion feedback signals before runtime evaluation.
    pub(super) fn update_in(&mut self, io: &mut PicoIo) {
        self.axis0.update_in(io);
        self.axis1.update_in(io);
    }

    /// Apply motion outputs after runtime evaluation.
    pub(super) fn update_out(&mut self, io: &mut PicoIo) {
        self.axis0.update_out(io);
        self.axis1.update_out(io);
    }

    /// Best-effort controlled-stop path on runtime failure.
    pub(super) fn finalize_on_error(&mut self, io: &mut PicoIo) {
        self.axis0.finalize_on_error(io);
        self.axis1.finalize_on_error(io);
    }
}

// -------------------------------------------------------------------------------------------------
// Axis glue
// -------------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct AxisChannels {
    do_enable: usize,
    do_dir: usize,
    ao_vel_sps: usize,
    ai_count: usize,
    ai_speed: usize,
    di_dir_positive: usize,
}

struct Axis<SmStep: rp_pico::hal::pio::StateMachineIndex, SmEnc: rp_pico::hal::pio::StateMachineIndex>
{
    channels: AxisChannels,
    stepper: Option<StepperAxis<SmStep>>,
    encoder: Option<AbEncoderAxis<SmEnc>>,
}

impl<SmStep: rp_pico::hal::pio::StateMachineIndex, SmEnc: rp_pico::hal::pio::StateMachineIndex>
    Axis<SmStep, SmEnc>
{
    fn new(
        idx: u8,
        pins: &mut AllPins,
        sys_hz: u32,
        step_program: rp_pico::hal::pio::InstalledProgram<pac::PIO0>,
        step_sm: rp_pico::hal::pio::UninitStateMachine<(pac::PIO0, SmStep)>,
        enc_program: rp_pico::hal::pio::InstalledProgram<pac::PIO0>,
        enc_sm: rp_pico::hal::pio::UninitStateMachine<(pac::PIO0, SmEnc)>,
        channels: AxisChannels,
    ) -> Self {
        let (stepper, encoder) = match idx {
            0 => (
                StepperAxis::initialize_axis0(pins, sys_hz, step_program, step_sm),
                AbEncoderAxis::initialize_axis0(pins, enc_program, enc_sm),
            ),
            1 => (
                StepperAxis::initialize_axis1(pins, sys_hz, step_program, step_sm),
                AbEncoderAxis::initialize_axis1(pins, enc_program, enc_sm),
            ),
            _ => (None, None),
        };
        let _ = idx;
        Self {
            channels,
            stepper,
            encoder,
        }
    }

    fn update_in(&mut self, io: &mut PicoIo) {
        if let Some(enc) = self.encoder.as_mut() {
            let dt_s = (io.tick_ms() as f32) / 1000.0;
            let (count, delta, speed_cps) = enc.snapshot(dt_s);
            // Publish to virtual AI/DI channels (see mapping constants above).
            if self.channels.ai_count < io_map::MAX_AI {
                io.ai[self.channels.ai_count] = count;
            }
            if self.channels.ai_speed < io_map::MAX_AI {
                io.ai[self.channels.ai_speed] = speed_cps;
            }
            if self.channels.di_dir_positive < io_map::MAX_DI {
                io.write_virtual_digital_input(self.channels.di_dir_positive, delta >= 0);
            }
        }
    }

    fn update_out(&mut self, io: &mut PicoIo) {
        let enabled = io.read_digital_output_latched(self.channels.do_enable);
        let dir_cmd = io.read_digital_output_latched(self.channels.do_dir);
        let vel_cmd_sps = io.read_analog_output_target(self.channels.ao_vel_sps);
        if let Some(step) = self.stepper.as_mut() {
            step.apply_command(io.tick_ms(), enabled, dir_cmd, vel_cmd_sps);
        }
    }

    fn finalize_on_error(&mut self, _io: &mut PicoIo) {
        if let Some(step) = self.stepper.as_mut() {
            step.fail_safe_stop();
        }
    }
}

// -------------------------------------------------------------------------------------------------
// Stepper axis (Pulse/Dir/EN)
// -------------------------------------------------------------------------------------------------

struct StepperAxis<SM: rp_pico::hal::pio::StateMachineIndex> {
    sys_hz: u32,
    dir_inverted: bool,
    v_max_sps: f32,
    acc_sps2: f32,
    dec_sps2: f32,
    dir_pin: Pin<DynPinId, FunctionSioOutput, PullDown>,
    en_pin: Pin<DynPinId, FunctionSioOutput, PullDown>,
    _step_pin: Pin<DynPinId, FunctionPio0, PullDown>,
    _sm: StateMachine<(pac::PIO0, SM), Running>,
    tx: Tx<(pac::PIO0, SM)>,
    last_half_period_cycles: u32,

    // Profile state.
    current_sps: f32,
    target_sps: f32,
    dir_requested: bool,
    dir_active: bool,
    dir_deadtime_ticks_remaining: u32,
}

impl<SM: rp_pico::hal::pio::StateMachineIndex> StepperAxis<SM> {
    fn initialize_axis0(
        pins: &mut AllPins,
        sys_hz: u32,
        program: rp_pico::hal::pio::InstalledProgram<pac::PIO0>,
        sm: rp_pico::hal::pio::UninitStateMachine<(pac::PIO0, SM)>,
    ) -> Option<Self> {
        if !io_map::MOTION_STEPPER_AXIS0_DEFINED {
            return None;
        }
        Self::initialize_from_cfg(
            pins,
            sys_hz,
            program,
            sm,
            io_map::MOTION_STEPPER_AXIS0_STEP_GPIO,
            io_map::MOTION_STEPPER_AXIS0_DIR_GPIO,
            io_map::MOTION_STEPPER_AXIS0_EN_GPIO,
            io_map::MOTION_STEPPER_AXIS0_DIR_INVERTED,
            io_map::MOTION_STEPPER_AXIS0_V_MAX_SPS,
            io_map::MOTION_STEPPER_AXIS0_ACC_SPS2,
            io_map::MOTION_STEPPER_AXIS0_DEC_SPS2,
        )
    }

    fn initialize_axis1(
        pins: &mut AllPins,
        sys_hz: u32,
        program: rp_pico::hal::pio::InstalledProgram<pac::PIO0>,
        sm: rp_pico::hal::pio::UninitStateMachine<(pac::PIO0, SM)>,
    ) -> Option<Self> {
        if !io_map::MOTION_STEPPER_AXIS1_DEFINED {
            return None;
        }
        Self::initialize_from_cfg(
            pins,
            sys_hz,
            program,
            sm,
            io_map::MOTION_STEPPER_AXIS1_STEP_GPIO,
            io_map::MOTION_STEPPER_AXIS1_DIR_GPIO,
            io_map::MOTION_STEPPER_AXIS1_EN_GPIO,
            io_map::MOTION_STEPPER_AXIS1_DIR_INVERTED,
            io_map::MOTION_STEPPER_AXIS1_V_MAX_SPS,
            io_map::MOTION_STEPPER_AXIS1_ACC_SPS2,
            io_map::MOTION_STEPPER_AXIS1_DEC_SPS2,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn initialize_from_cfg(
        pins: &mut AllPins,
        sys_hz: u32,
        program: rp_pico::hal::pio::InstalledProgram<pac::PIO0>,
        sm: rp_pico::hal::pio::UninitStateMachine<(pac::PIO0, SM)>,
        step_gpio: u8,
        dir_gpio: u8,
        en_gpio: u8,
        dir_inverted: bool,
        v_max_sps: u32,
        acc_sps2: u32,
        dec_sps2: u32,
    ) -> Option<Self> {
        let step = take_pin(pins, step_gpio)?;
        let dir = take_pin(pins, dir_gpio)?;
        let en = take_pin(pins, en_gpio)?;

        let step: Pin<DynPinId, FunctionPio0, PullDown> = step
            .try_into_function::<FunctionPio0>()
            .ok()
            .unwrap();
        let step_pin_id = step.id().num;

        let mut dir: Pin<DynPinId, FunctionSioOutput, PullDown> =
            dir.try_into_function::<FunctionSioOutput>().ok().unwrap();
        dir.set_input_enable(false);
        dir.set_output_enable_override(OutputEnableOverride::Enable);
        let _ = dir.set_low();

        let mut en: Pin<DynPinId, FunctionSioOutput, PullDown> =
            en.try_into_function::<FunctionSioOutput>().ok().unwrap();
        en.set_input_enable(false);
        en.set_output_enable_override(OutputEnableOverride::Enable);
        let _ = en.set_low();

        let (mut sm, _rx, mut tx) = PIOBuilder::from_installed_program(program)
            .set_pins(step_pin_id, 1)
            .clock_divisor_fixed_point(1, 0)
            .build(sm);
        sm.set_pindirs([(step_pin_id, PinDir::Output)]);
        let sm = sm.start();

        // Start in "idle" (STEP low).
        let _ = tx.write(0);

        let v_max_sps = if v_max_sps == 0 { 20_000 } else { v_max_sps };
        let acc_sps2 = if acc_sps2 == 0 { 40_000 } else { acc_sps2 };
        let dec_sps2 = if dec_sps2 == 0 { 40_000 } else { dec_sps2 };

        Some(Self {
            sys_hz,
            dir_inverted,
            v_max_sps: v_max_sps as f32,
            acc_sps2: acc_sps2 as f32,
            dec_sps2: dec_sps2 as f32,
            dir_pin: dir,
            en_pin: en,
            _step_pin: step,
            _sm: sm,
            tx,
            last_half_period_cycles: 0,
            current_sps: 0.0,
            target_sps: 0.0,
            dir_requested: false,
            dir_active: false,
            dir_deadtime_ticks_remaining: 0,
        })
    }

    fn apply_command(&mut self, tick_ms: u32, enabled: bool, dir_cmd: bool, vel_cmd_sps: f32) {
        let dt_s = (tick_ms as f32) / 1000.0;
        let mut dir_cmd = dir_cmd;
        if self.dir_inverted {
            dir_cmd = !dir_cmd;
        }

        if !enabled {
            self.target_sps = 0.0;
            self.dir_requested = dir_cmd;
            let _ = self.en_pin.set_low();
        } else {
            let vel = if vel_cmd_sps.is_finite() {
                vel_cmd_sps.clamp(0.0, self.v_max_sps)
            } else {
                0.0
            };
            self.dir_requested = dir_cmd;
            self.target_sps = vel;
            let _ = self.en_pin.set_high();
        }

        // Direction switching protection:
        // - if a direction change is requested while moving, first decelerate to 0.
        // - once stopped, enforce a short deadtime before flipping DIR, then ramp up again.
        if self.dir_requested != self.dir_active {
            self.target_sps = 0.0;
            if self.current_sps <= 0.1 {
                if self.dir_deadtime_ticks_remaining == 0 {
                    self.dir_deadtime_ticks_remaining = 2;
                }
            }
        }

        if self.dir_deadtime_ticks_remaining > 0 {
            self.dir_deadtime_ticks_remaining -= 1;
            self.target_sps = 0.0;
            if self.dir_deadtime_ticks_remaining == 0 {
                self.dir_active = self.dir_requested;
            }
        }

        // Apply DIR output (even while stopped).
        if self.dir_active {
            let _ = self.dir_pin.set_high();
        } else {
            let _ = self.dir_pin.set_low();
        }

        // Trapezoid ramp on speed magnitude.
        if self.target_sps > self.current_sps {
            self.current_sps = (self.current_sps + self.acc_sps2 * dt_s).min(self.target_sps);
        } else if self.target_sps < self.current_sps {
            self.current_sps = (self.current_sps - self.dec_sps2 * dt_s).max(self.target_sps);
        }
        if self.current_sps < 0.0 {
            self.current_sps = 0.0;
        }

        let half_period = self.half_period_cycles_for_sps(self.current_sps);
        if half_period != self.last_half_period_cycles {
            if self.tx.write(half_period) {
                self.last_half_period_cycles = half_period;
            }
        }
    }

    fn half_period_cycles_for_sps(&self, sps: f32) -> u32 {
        if !(sps > 0.0) {
            return 0;
        }
        // Each full step pulse cycle is high+low. The PIO program waits `half_period_cycles`
        // iterations for each half. We approximate:
        //   half_period_cycles ~= sys_hz / (2 * sps)
        let hp = (self.sys_hz as f32) / (2.0 * sps);
        if !hp.is_finite() || hp < 1.0 {
            1
        } else if hp > (u32::MAX as f32) {
            u32::MAX
        } else {
            hp as u32
        }
    }

    fn fail_safe_stop(&mut self) {
        let _ = self.en_pin.set_low();
        let _ = self.tx.write(0);
        self.last_half_period_cycles = 0;
        self.current_sps = 0.0;
        self.target_sps = 0.0;
    }
}

// -------------------------------------------------------------------------------------------------
// AB encoder axis (PIO counter in X)
// -------------------------------------------------------------------------------------------------

struct AbEncoderAxis<SM: rp_pico::hal::pio::StateMachineIndex> {
    count_sign_inverted: bool,
    scale: f32,
    _a_pin: Pin<DynPinId, FunctionPio0, PullDown>,
    _b_pin: Pin<DynPinId, FunctionPio0, PullDown>,
    sm: StateMachine<(pac::PIO0, SM), Running>,
    rx: Rx<(pac::PIO0, SM)>,
    last_count_raw: i32,
}

impl<SM: rp_pico::hal::pio::StateMachineIndex> AbEncoderAxis<SM> {
    fn initialize_axis0(
        pins: &mut AllPins,
        program: rp_pico::hal::pio::InstalledProgram<pac::PIO0>,
        sm: rp_pico::hal::pio::UninitStateMachine<(pac::PIO0, SM)>,
    ) -> Option<Self> {
        if !io_map::MOTION_ENCODER_AXIS0_DEFINED {
            return None;
        }
        Self::initialize_from_cfg(
            pins,
            program,
            sm,
            io_map::MOTION_ENCODER_AXIS0_A_GPIO,
            io_map::MOTION_ENCODER_AXIS0_B_GPIO,
            io_map::MOTION_ENCODER_AXIS0_COUNT_SIGN_INVERTED,
            io_map::MOTION_ENCODER_AXIS0_SCALE,
        )
    }

    fn initialize_axis1(
        pins: &mut AllPins,
        program: rp_pico::hal::pio::InstalledProgram<pac::PIO0>,
        sm: rp_pico::hal::pio::UninitStateMachine<(pac::PIO0, SM)>,
    ) -> Option<Self> {
        if !io_map::MOTION_ENCODER_AXIS1_DEFINED {
            return None;
        }
        Self::initialize_from_cfg(
            pins,
            program,
            sm,
            io_map::MOTION_ENCODER_AXIS1_A_GPIO,
            io_map::MOTION_ENCODER_AXIS1_B_GPIO,
            io_map::MOTION_ENCODER_AXIS1_COUNT_SIGN_INVERTED,
            io_map::MOTION_ENCODER_AXIS1_SCALE,
        )
    }

    fn initialize_from_cfg(
        pins: &mut AllPins,
        program: rp_pico::hal::pio::InstalledProgram<pac::PIO0>,
        sm: rp_pico::hal::pio::UninitStateMachine<(pac::PIO0, SM)>,
        a_gpio: u8,
        b_gpio: u8,
        count_sign_inverted: bool,
        scale: f32,
    ) -> Option<Self> {
        let a = take_pin(pins, a_gpio)?;
        let b = take_pin(pins, b_gpio)?;

        let a: Pin<DynPinId, FunctionPio0, PullDown> = a
            .try_into_function::<FunctionPio0>()
            .ok()
            .unwrap();
        let b: Pin<DynPinId, FunctionPio0, PullDown> = b
            .try_into_function::<FunctionPio0>()
            .ok()
            .unwrap();
        let a_pin_id = a.id().num;
        let b_pin_id = b.id().num;

        let (mut sm, rx, _tx) = PIOBuilder::from_installed_program(program)
            .in_pin_base(a_pin_id)
            .jmp_pin(b_pin_id)
            .clock_divisor_fixed_point(1, 0)
            .build(sm);
        sm.set_pindirs([
            (a_pin_id, PinDir::Input),
            (b_pin_id, PinDir::Input),
        ]);
        let sm = sm.start();

        Some(Self {
            count_sign_inverted,
            scale: if scale.is_finite() && scale > 0.0 { scale } else { 1.0 },
            _a_pin: a,
            _b_pin: b,
            sm,
            rx,
            last_count_raw: 0,
        })
    }

    /// Returns (count_eng, delta_raw, speed_eng_per_s).
    fn snapshot(&mut self, dt_s: f32) -> (f32, i32, f32) {
        // Best-effort: snapshot X into RX FIFO and read it back.
        // Note: these instructions must not stall; we use non-blocking PUSH.
        let mov_isr_x = Instruction {
            operands: InstructionOperands::MOV {
                destination: MovDestination::ISR,
                op: MovOperation::None,
                source: MovSource::X,
            },
            delay: 0,
            side_set: None,
        };
        let push = Instruction {
            operands: InstructionOperands::PUSH {
                if_full: false,
                block: false,
            },
            delay: 0,
            side_set: None,
        };
        self.sm.exec_instruction(mov_isr_x);
        self.sm.exec_instruction(push);

        let raw_u32 = self.rx.read().unwrap_or(self.last_count_raw as u32);
        let mut raw = raw_u32 as i32;
        if self.count_sign_inverted {
            raw = raw.wrapping_neg();
        }
        let delta = raw.wrapping_sub(self.last_count_raw);
        self.last_count_raw = raw;

        let dt_s = if dt_s > 1e-6 { dt_s } else { 1e-3 };
        let speed = (delta as f32) / dt_s;

        let count_eng = (raw as f32) * self.scale;
        let speed_eng = speed * self.scale;
        (count_eng, delta, speed_eng)
    }
}

// -------------------------------------------------------------------------------------------------
// Helpers
// -------------------------------------------------------------------------------------------------

fn take_pin(pins: &mut AllPins, gpio: u8) -> Option<Pin<DynPinId, FunctionNull, PullDown>> {
    pins.take(gpio)
}
