#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

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
}

// Embedded firmware build (thumbv6m-none-eabi, target_os = "none").
#[cfg(target_os = "none")]
mod firmware {
    use io_traits::{AnalogInputId, AnalogOutputId, DigitalInputId, DigitalOutputId, Io, Tick};
    use runtime_core::{Runtime, TransitionReason};

    use embedded_hal::digital::v2::{InputPin, OutputPin};
    use cortex_m_rt::entry;
    use rp_pico::hal::{
        clocks::init_clocks_and_plls,
        gpio::{
            DynPinId, FunctionNull, FunctionSioInput, FunctionSioOutput, OutputEnableOverride,
            Pin, PullDown, PullUp,
        },
        pac,
        sio::Sio,
        watchdog::Watchdog,
    };
    use rp_pico::hal::clocks::ClockSource;

    // defmt logging over RTT + defmt-aware panic output.
    use defmt_rtt as _;
    use panic_probe as _;

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
        ai: [f32; 8],
        ao: [f32; 8],
    }

    impl PicoIo {
        fn new(pins: &mut AllPins) -> Self {
            let mut di: [Option<Pin<DynPinId, FunctionSioInput, PullUp>>; io_map::MAX_DI] =
                core::array::from_fn(|_| None);
            let mut do_: [Option<Pin<DynPinId, FunctionSioOutput, PullDown>>; io_map::MAX_DO] =
                core::array::from_fn(|_| None);

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
                    let p = p
                        .try_into_function::<FunctionSioOutput>()
                        .ok()
                        .unwrap();
                    let mut p = p;
                    p.set_input_enable(false);
                    p.set_output_enable_override(OutputEnableOverride::Enable);
                    let _ = p.set_low();
                    do_[id] = Some(p);
                }
            }

            Self {
                t: Tick(0),
                di,
                do_,
                ai: [0.0; 8],
                ao: [0.0; 8],
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
            self.ai[id.0 as usize]
        }

        fn write_digital_output(&mut self, id: DigitalOutputId, value: bool) {
            if let Some(p) = self
                .do_
                .get_mut(id.0 as usize)
                .and_then(|p| p.as_mut())
            {
                if value {
                    let _ = p.set_high();
                } else {
                    let _ = p.set_low();
                }
            }
        }

        fn write_analog_output(&mut self, id: AnalogOutputId, value: f32) {
            self.ao[id.0 as usize] = value;
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
        let mut io = PicoIo::new(&mut all_pins);

        // Minimal "tick" source: we drive the runtime in a loop paced by a crude busy-wait.
        // Later stories can replace this with a timer interrupt and real GPIO-backed Io.
        const TICK_MS: u64 = 1;
        loop {
            let tick = io.tick().0;
            defmt::info!("TICK tick={} ts_ms={}", tick, tick.saturating_mul(TICK_MS));
            rt.tick_with_trace(&mut io, |e| {
                defmt::info!(
                    "TRACE tick={} task={} from={} to={} reason={} ts_ms={}",
                    e.tick.0,
                    e.task,
                    e.from.0,
                    e.to.0,
                    reason_str(e.reason),
                    e.tick.0.saturating_mul(TICK_MS)
                );
            })
            .unwrap();
            cortex_m::asm::delay(12_000_000);
        }
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
