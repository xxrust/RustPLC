#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

#[cfg(any(test, target_os = "none"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TickTimingWindow {
    ts_start_us: u64,
    ts_end_us: u64,
    exec_us: u64,
    slack_us: u64,
    overrun: bool,
}

#[cfg(any(test, target_os = "none"))]
fn evaluate_tick_timing(ts_start_us: u64, ts_end_us: u64, deadline_us: u64) -> TickTimingWindow {
    let exec_us = ts_end_us.saturating_sub(ts_start_us);
    let overrun = ts_end_us > deadline_us;
    let slack_us = if overrun {
        0
    } else {
        deadline_us.saturating_sub(ts_end_us)
    };
    TickTimingWindow {
        ts_start_us,
        ts_end_us,
        exec_us,
        slack_us,
        overrun,
    }
}

// Host build: keep workspace builds green and provide instructions.
#[cfg(not(target_os = "none"))]
fn main() {
    println!("board-rp2040 is a firmware target (RP2040 / Raspberry Pi Pico).");
    println!("Build it for Pico with:");
    println!("  rustup target add thumbv6m-none-eabi");
    println!("  cargo build -p board-rp2040 --target thumbv6m-none-eabi");
    println!();
    println!("To inject a generated Program module at build time:");
    println!("  export RUST_PLC_GENERATED_PROGRAM_RS=/path/to/generated_program.rs");
    println!("  export RUST_PLC_IO_MAP_TOML=/path/to/io_map.toml");
    println!("  export RUST_PLC_ANALOG_CONTRACT_TOML=/path/to/analog_contract.toml");
}

// Embedded firmware build (thumbv6m-none-eabi, target_os = "none").
#[cfg(target_os = "none")]
mod firmware {
    use super::evaluate_tick_timing;
    use io_traits::{AnalogInputId, AnalogOutputId, DigitalInputId, DigitalOutputId, Io, Tick};
    use runtime_core::{Runtime, TransitionReason};

    use cortex_m_rt::entry;
    use embedded_hal_0_2::digital::v2::{InputPin, OutputPin};
    use rp_pico::hal::clocks::ClockSource;
    use rp_pico::hal::{
        adc::{Adc, AdcPin},
        clocks::init_clocks_and_plls,
        gpio::{
            DynPinId, FunctionNull, FunctionPwm, FunctionSioInput, FunctionSioOutput,
            OutputEnableOverride, Pin, PullDown, PullUp,
        },
        pac,
        sio::Sio,
        timer::Timer,
        watchdog::Watchdog,
    };

    // defmt logging over RTT + defmt-aware panic output.
    use defmt_rtt as _;
    use panic_probe as _;

    mod hal;
    mod motion;

    mod generated_program {
        // Filled by build.rs; can be overridden via RUST_PLC_GENERATED_PROGRAM_RS.
        include!(concat!(env!("OUT_DIR"), "/generated_program.rs"));
    }

    mod io_map {
        // Filled by build.rs; can be overridden via RUST_PLC_IO_MAP_TOML.
        include!(concat!(env!("OUT_DIR"), "/io_map.rs"));
    }

    // RP2040 needs a 2nd stage bootloader stored at the start of flash.
    #[link_section = ".boot2"]
    #[used]
    static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;
    const AO_PWM_TOP: u16 = 65_535;

    struct AllPins {
        pins: [Option<Pin<DynPinId, FunctionNull, PullDown>>; 30],
    }

    impl AllPins {
        fn new(p: rp_pico::Pins) -> Self {
            let mut pins: [Option<Pin<DynPinId, FunctionNull, PullDown>>; 30] =
                core::array::from_fn(|_| None);
            pins[0] = Some(p.gpio0.into_dyn_pin());
            pins[1] = Some(p.gpio1.into_dyn_pin());
            pins[2] = Some(p.gpio2.into_dyn_pin());
            pins[3] = Some(p.gpio3.into_dyn_pin());
            pins[4] = Some(p.gpio4.into_dyn_pin());
            pins[5] = Some(p.gpio5.into_dyn_pin());
            pins[6] = Some(p.gpio6.into_dyn_pin());
            pins[7] = Some(p.gpio7.into_dyn_pin());
            pins[8] = Some(p.gpio8.into_dyn_pin());
            pins[9] = Some(p.gpio9.into_dyn_pin());
            pins[10] = Some(p.gpio10.into_dyn_pin());
            pins[11] = Some(p.gpio11.into_dyn_pin());
            pins[12] = Some(p.gpio12.into_dyn_pin());
            pins[13] = Some(p.gpio13.into_dyn_pin());
            pins[14] = Some(p.gpio14.into_dyn_pin());
            pins[15] = Some(p.gpio15.into_dyn_pin());
            pins[16] = Some(p.gpio16.into_dyn_pin());
            pins[17] = Some(p.gpio17.into_dyn_pin());
            pins[18] = Some(p.gpio18.into_dyn_pin());
            pins[19] = Some(p.gpio19.into_dyn_pin());
            pins[20] = Some(p.gpio20.into_dyn_pin());
            pins[21] = Some(p.gpio21.into_dyn_pin());
            pins[22] = Some(p.gpio22.into_dyn_pin());
            pins[23] = Some(p.b_power_save.into_dyn_pin());
            pins[24] = Some(p.vbus_detect.into_dyn_pin());
            pins[25] = Some(p.led.into_dyn_pin());
            pins[26] = Some(p.gpio26.into_dyn_pin());
            pins[27] = Some(p.gpio27.into_dyn_pin());
            pins[28] = Some(p.gpio28.into_dyn_pin());
            pins[29] = Some(p.voltage_monitor.into_dyn_pin());
            Self { pins }
        }

        fn take(&mut self, gpio: u8) -> Option<Pin<DynPinId, FunctionNull, PullDown>> {
            self.pins.get_mut(gpio as usize)?.take()
        }
    }

    struct PicoIo {
        t: Tick,
        di: [Option<Pin<DynPinId, FunctionSioInput, PullUp>>; io_map::MAX_DI],
        do_: [Option<Pin<DynPinId, FunctionSioOutput, PullDown>>; io_map::MAX_DO],
        adc: Adc,
        ai_pins: [Option<AdcPin<Pin<DynPinId, FunctionSioInput, PullDown>>>; io_map::MAX_AI],
        ai: [f32; io_map::MAX_AI],
        pwm: pac::PWM,
        ao_pwm_pins: [Option<Pin<DynPinId, FunctionPwm, PullDown>>; io_map::MAX_AO],
        ao_pwm_bindings: [Option<PwmChannelBinding>; io_map::MAX_AO],
        ao_target: [f32; io_map::MAX_AO],
        ao_current: [f32; io_map::MAX_AO],
        tick_ms: u32,
    }

    #[derive(Clone, Copy)]
    struct PwmChannelBinding {
        slice: u8,
        is_channel_a: bool,
    }

    impl PicoIo {
        fn new(pins: &mut AllPins, adc: Adc, pwm: pac::PWM, tick_ms: u32) -> Self {
            let mut di: [Option<Pin<DynPinId, FunctionSioInput, PullUp>>; io_map::MAX_DI] =
                core::array::from_fn(|_| None);
            let mut do_: [Option<Pin<DynPinId, FunctionSioOutput, PullDown>>; io_map::MAX_DO] =
                core::array::from_fn(|_| None);
            let mut ai_pins: [Option<AdcPin<Pin<DynPinId, FunctionSioInput, PullDown>>>;
                io_map::MAX_AI] = core::array::from_fn(|_| None);
            let mut ao_pwm_pins: [Option<Pin<DynPinId, FunctionPwm, PullDown>>; io_map::MAX_AO] =
                core::array::from_fn(|_| None);
            let mut ao_pwm_bindings: [Option<PwmChannelBinding>; io_map::MAX_AO] =
                core::array::from_fn(|_| None);
            let mut configured_slices = [false; 8];

            for (id, &gpio) in io_map::DI_GPIO.iter().enumerate() {
                if gpio == io_map::UNUSED_GPIO {
                    continue;
                }
                if let Some(p) = pins.take(gpio) {
                    let p = p
                        .try_into_function::<FunctionSioInput>()
                        .ok()
                        .unwrap()
                        .into_pull_type::<PullUp>();
                    let mut p = p;
                    p.set_input_enable(true);
                    p.set_output_enable_override(OutputEnableOverride::Disable);
                    di[id] = Some(p);
                }
            }
            for (id, &gpio) in io_map::DO_GPIO.iter().enumerate() {
                if gpio == io_map::UNUSED_GPIO {
                    continue;
                }
                if let Some(p) = pins.take(gpio) {
                    let p = p.try_into_function::<FunctionSioOutput>().ok().unwrap();
                    let mut p = p;
                    p.set_input_enable(false);
                    p.set_output_enable_override(OutputEnableOverride::Enable);
                    let _ = p.set_low();
                    do_[id] = Some(p);
                }
            }
            for (id, &gpio) in io_map::AI_GPIO.iter().enumerate() {
                if gpio == io_map::UNUSED_GPIO {
                    continue;
                }
                if let Some(p) = pins.take(gpio) {
                    let p = p.try_into_function::<FunctionSioInput>().ok().unwrap();
                    match AdcPin::new(p) {
                        Ok(pin) => ai_pins[id] = Some(pin),
                        Err(_) => {
                            defmt::warn!(
                                "io_map ai{}={} is not ADC-capable; skipping this analog input",
                                id,
                                gpio
                            );
                        }
                    }
                }
            }
            for (id, &gpio) in io_map::AO_GPIO.iter().enumerate() {
                if gpio == io_map::UNUSED_GPIO {
                    continue;
                }
                if let Some(p) = pins.take(gpio) {
                    let p = p.try_into_function::<FunctionPwm>().ok().unwrap();
                    ao_pwm_pins[id] = Some(p);
                    let binding = gpio_to_pwm_binding(gpio);
                    ao_pwm_bindings[id] = Some(binding);
                    if !configured_slices[binding.slice as usize] {
                        configure_pwm_slice(&pwm, binding.slice);
                        configured_slices[binding.slice as usize] = true;
                    }
                    set_pwm_channel_duty(&pwm, binding, 0);
                }
            }

            Self {
                t: Tick(0),
                di,
                do_,
                adc,
                ai_pins,
                ai: [0.0; io_map::MAX_AI],
                pwm,
                ao_pwm_pins,
                ao_pwm_bindings,
                ao_target: [0.0; io_map::MAX_AO],
                ao_current: [0.0; io_map::MAX_AO],
                tick_ms,
            }
        }

        fn enter_safe_state(&mut self) {
            // Best-effort software safe state. This is reliable only for "controlled stop" paths.
            // For process crash / power-loss / hard lockups, the machine must still rely on the
            // hardware safety chain (E-Stop, STO, brake wiring, etc.).
            match io_map::SAFE_STATE_MODE {
                0 => self.safe_all_zero(),
                1 => self.safe_profile(),
                _ => self.safe_all_zero(),
            }
        }

        fn safe_all_zero(&mut self) {
            for id in 0..io_map::MAX_DO {
                self.write_digital_output(DigitalOutputId(id as u16), false);
            }
            for id in 0..io_map::MAX_AO {
                self.force_analog_output_immediate(id, 0.0);
            }
        }

        fn safe_profile(&mut self) {
            let mut do_done = [false; io_map::MAX_DO];
            let mut ao_done = [false; io_map::MAX_AO];

            // Apply outputs in group order (smaller group first). Same-group outputs are applied
            // in the same pass (no guaranteed intra-group ordering).
            loop {
                let mut next: Option<u16> = None;

                for id in 0..io_map::MAX_DO {
                    if io_map::SAFE_DO_DEFINED[id] && !do_done[id] {
                        let g = io_map::SAFE_DO_GROUP[id];
                        next = Some(match next {
                            Some(cur) if cur <= g => cur,
                            _ => g,
                        });
                    }
                }
                for id in 0..io_map::MAX_AO {
                    if io_map::SAFE_AO_DEFINED[id] && !ao_done[id] {
                        let g = io_map::SAFE_AO_GROUP[id];
                        next = Some(match next {
                            Some(cur) if cur <= g => cur,
                            _ => g,
                        });
                    }
                }

                let Some(group) = next else {
                    break;
                };

                for id in 0..io_map::MAX_DO {
                    if io_map::SAFE_DO_DEFINED[id]
                        && !do_done[id]
                        && io_map::SAFE_DO_GROUP[id] == group
                    {
                        self.write_digital_output(
                            DigitalOutputId(id as u16),
                            io_map::SAFE_DO_VALUE[id],
                        );
                        do_done[id] = true;
                    }
                }
                for id in 0..io_map::MAX_AO {
                    if io_map::SAFE_AO_DEFINED[id]
                        && !ao_done[id]
                        && io_map::SAFE_AO_GROUP[id] == group
                    {
                        self.force_analog_output_immediate(id, io_map::SAFE_AO_VALUE[id]);
                        ao_done[id] = true;
                    }
                }
            }
        }

        fn force_analog_output_immediate(&mut self, id: usize, value: f32) {
            if let Some(slot) = self.ao_target.get_mut(id) {
                *slot = value;
            }
            if let Some(slot) = self.ao_current.get_mut(id) {
                *slot = value;
            }

            if self.ao_pwm_pins.get(id).and_then(|p| p.as_ref()).is_none() {
                return;
            }
            let Some(binding) = self.ao_pwm_bindings.get(id).and_then(|b| *b) else {
                return;
            };
            let duty = engineering_to_pwm(id, value);
            set_pwm_channel_duty(&self.pwm, binding, duty);
        }

        fn sample_analog_inputs(&mut self) {
            for (id, pin) in self.ai_pins.iter_mut().enumerate() {
                let Some(pin) = pin.as_mut() else {
                    continue;
                };
                // Use RP2040 ADC free-running mode to sample the selected channel.
                // We read twice after switching channel so the second sample reflects
                // the new channel after mux settle.
                self.adc.free_running(pin);
                cortex_m::asm::delay(256);
                let _discard = self.adc.read_single();
                cortex_m::asm::delay(256);
                let raw = self.adc.read_single();
                // Convert RP2040 ADC raw counts (12-bit) to volts, then map to engineering units
                // using the per-channel contract embedded from `.plc` ranges.
                let volts = (raw as f32) * (3.3 / 4095.0);
                self.ai[id] = volts_to_engineering(id, volts);
            }
            self.adc.stop();
        }

        fn apply_analog_outputs(&mut self) {
            for id in 0..io_map::MAX_AO {
                if self.ao_pwm_pins[id].is_none() || self.ao_pwm_bindings[id].is_none() {
                    continue;
                }
                let min = io_map::AO_ENG_MIN[id];
                let max = io_map::AO_ENG_MAX[id];
                if !(max > min) {
                    continue;
                }

                let target = self.ao_target[id].clamp(min, max);
                let current = self.ao_current[id];
                let ramp_ms = io_map::AO_RAMP_MS[id];
                let next = if ramp_ms == 0 {
                    target
                } else {
                    let span = (max - min).abs();
                    let delta = (span * (self.tick_ms as f32) / (ramp_ms as f32)).max(1e-6);
                    if (target - current).abs() <= delta {
                        target
                    } else if target > current {
                        current + delta
                    } else {
                        current - delta
                    }
                };
                self.ao_current[id] = next;
                let duty = engineering_to_pwm(id, next);
                if let Some(binding) = self.ao_pwm_bindings[id] {
                    set_pwm_channel_duty(&self.pwm, binding, duty);
                }
            }
        }
    }

    impl Io for PicoIo {
        fn tick(&self) -> Tick {
            self.t
        }

        fn advance_tick(&mut self) {
            self.t.0 += 1;
        }

        fn read_digital_input(&self, id: DigitalInputId) -> bool {
            self.di
                .get(id.0 as usize)
                .and_then(|p| p.as_ref())
                .and_then(|p| p.is_high().ok())
                .unwrap_or(false)
        }

        fn read_analog_input(&self, id: AnalogInputId) -> f32 {
            self.ai.get(id.0 as usize).copied().unwrap_or(0.0)
        }

        fn write_digital_output(&mut self, id: DigitalOutputId, value: bool) {
            if let Some(p) = self.do_.get_mut(id.0 as usize).and_then(|p| p.as_mut()) {
                if value {
                    let _ = p.set_high();
                } else {
                    let _ = p.set_low();
                }
            }
        }

        fn write_analog_output(&mut self, id: AnalogOutputId, value: f32) {
            if let Some(slot) = self.ao_target.get_mut(id.0 as usize) {
                *slot = value;
            }
        }
    }

    #[entry]
    fn main() -> ! {
        let mut pac = pac::Peripherals::take().unwrap();

        let mut watchdog = Watchdog::new(pac.WATCHDOG);
        let clocks = init_clocks_and_plls(
            rp_pico::XOSC_CRYSTAL_FREQ,
            pac.XOSC,
            pac.CLOCKS,
            pac.PLL_SYS,
            pac.PLL_USB,
            &mut pac.RESETS,
            &mut watchdog,
        )
        .ok()
        .unwrap();

        defmt::info!(
            "boot ok; sys_clk={}Hz",
            clocks.system_clock.get_freq().to_Hz()
        );

        let timer = Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);
        let adc = Adc::new(pac.ADC, &mut pac.RESETS);
        let program = &generated_program::generated::PROGRAM;
        let mut rt = Runtime::new(program).unwrap();

        let sio = Sio::new(pac.SIO);
        let pins = rp_pico::Pins::new(
            pac.IO_BANK0,
            pac.PADS_BANK0,
            sio.gpio_bank0,
            &mut pac.RESETS,
        );
        let mut all_pins = AllPins::new(pins);
        const TICK_MS: u64 = 1;
        let io = PicoIo::new(&mut all_pins, adc, pac.PWM, TICK_MS as u32);
        let mut hal = hal::Hal::initialize(timer, io, TICK_MS);

        loop {
            let (tick, tick_start_us, deadline_us) = hal.update_in();
            let tick_res = rt.tick_with_trace_and_logs(
                hal.io_mut(),
                |e| {
                    defmt::info!(
                        "TRACE tick={} task={} from={} to={} reason={} ts_ms={}",
                        e.tick.0,
                        e.task,
                        e.from.0,
                        e.to.0,
                        reason_str(e.reason),
                        e.tick.0.saturating_mul(TICK_MS)
                    );
                },
                |log| {
                    defmt::info!(
                        "LOG tick={} task={} step={} msg_id={} msg={} ts_ms={}",
                        log.tick.0,
                        log.task,
                        log.step.0,
                        log.message_id,
                        log.message,
                        log.tick.0.saturating_mul(TICK_MS)
                    );
                },
            );
            if let Err(err) = tick_res {
                defmt::error!(
                    "RUNTIME_ERR tick={} err={:?} -> entering safe state",
                    tick,
                    defmt::Debug2Format(&err)
                );
                hal.finalize_on_error(tick);
            }

            hal.update_out(tick, tick_start_us, deadline_us);
        }
    }

    fn volts_to_engineering(ai_idx: usize, volts: f32) -> f32 {
        // Default mapping assumes 0.0..3.3V corresponds to `min..max` from the DSL.
        // If min==max (misconfig), fall back to volts.
        let min = *io_map::AI_ENG_MIN.get(ai_idx).unwrap_or(&0.0);
        let max = *io_map::AI_ENG_MAX.get(ai_idx).unwrap_or(&3.3);
        if (max - min).abs() < 1e-9 {
            return volts;
        }
        let r = (volts / 3.3).clamp(0.0, 1.0);
        let eng = min + r * (max - min);
        let scale = *io_map::AI_CAL_SCALE.get(ai_idx).unwrap_or(&1.0);
        let offset = *io_map::AI_CAL_OFFSET.get(ai_idx).unwrap_or(&0.0);
        eng * scale + offset
    }

    fn engineering_to_pwm(ao_idx: usize, value: f32) -> u16 {
        let min = *io_map::AO_ENG_MIN.get(ao_idx).unwrap_or(&0.0);
        let max = *io_map::AO_ENG_MAX.get(ao_idx).unwrap_or(&10.0);
        if (max - min).abs() < 1e-9 {
            return 0;
        }
        let scale = *io_map::AO_CAL_SCALE.get(ao_idx).unwrap_or(&1.0);
        let offset = *io_map::AO_CAL_OFFSET.get(ao_idx).unwrap_or(&0.0);
        let calibrated = value * scale + offset;
        let r = ((calibrated - min) / (max - min)).clamp(0.0, 1.0);
        // no_std: avoid `f32::round()` (requires libm on some targets).
        let duty = r * (AO_PWM_TOP as f32) + 0.5;
        let duty = if duty < 0.0 { 0.0 } else { duty };
        let duty = if duty > (AO_PWM_TOP as f32) {
            AO_PWM_TOP as f32
        } else {
            duty
        };
        duty as u16
    }

    fn gpio_to_pwm_binding(gpio: u8) -> PwmChannelBinding {
        PwmChannelBinding {
            slice: gpio / 2,
            is_channel_a: gpio % 2 == 0,
        }
    }

    fn configure_pwm_slice(pwm: &pac::PWM, slice: u8) {
        let ch = pwm.ch(slice as usize);
        ch.div().write(|w| unsafe {
            w.int().bits(1);
            w.frac().bits(0)
        });
        ch.top().write(|w| unsafe { w.top().bits(AO_PWM_TOP) });
        ch.csr().modify(|_, w| {
            w.divmode().div();
            w.en().set_bit()
        });
    }

    fn set_pwm_channel_duty(pwm: &pac::PWM, binding: PwmChannelBinding, duty: u16) {
        let ch = pwm.ch(binding.slice as usize);
        let current = ch.cc().read().bits();
        let next = if binding.is_channel_a {
            (current & 0xFFFF_0000) | (duty as u32)
        } else {
            (current & 0x0000_FFFF) | ((duty as u32) << 16)
        };
        ch.cc().write(|w| unsafe { w.bits(next) });
    }

    fn reason_str(r: TransitionReason) -> &'static str {
        match r {
            TransitionReason::Action => "action",
            TransitionReason::DelayElapsed => "delay_elapsed",
            TransitionReason::WaitSatisfied => "wait_satisfied",
            TransitionReason::Timeout => "timeout",
            TransitionReason::Goto => "goto",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::evaluate_tick_timing;

    #[test]
    fn evaluate_tick_timing_reports_non_overrun_with_positive_slack() {
        let timing = evaluate_tick_timing(100, 250, 1_100);
        assert_eq!(timing.exec_us, 150);
        assert_eq!(timing.slack_us, 850);
        assert!(!timing.overrun);
    }

    #[test]
    fn evaluate_tick_timing_reports_overrun_with_zero_slack() {
        let timing = evaluate_tick_timing(1_000, 2_250, 2_000);
        assert_eq!(timing.exec_us, 1_250);
        assert_eq!(timing.slack_us, 0);
        assert!(timing.overrun);
    }
}
