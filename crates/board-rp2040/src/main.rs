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
    use defmt::Debug2Format;
    use io_traits::{AnalogInputId, AnalogOutputId, DigitalInputId, DigitalOutputId, Io, Tick};
    use runtime_core::Runtime;

    use cortex_m_rt::entry;
    use rp_pico::hal::{
        clocks::init_clocks_and_plls,
        pac,
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

    // RP2040 needs a 2nd stage bootloader stored at the start of flash.
    #[link_section = ".boot2"]
    #[used]
    static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

    struct DummyIo {
        t: Tick,
        di: [bool; 32],
        do_: [bool; 32],
        ai: [f32; 8],
        ao: [f32; 8],
    }

    impl DummyIo {
        fn new() -> Self {
            Self {
                t: Tick(0),
                di: [false; 32],
                do_: [false; 32],
                ai: [0.0; 8],
                ao: [0.0; 8],
            }
        }
    }

    impl Io for DummyIo {
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
        let mut io = DummyIo::new();

        // Minimal "tick" source: we drive the runtime in a loop paced by a crude busy-wait.
        // Later stories can replace this with a timer interrupt and real GPIO-backed Io.
        loop {
            rt.tick_with_trace(&mut io, |e| {
                defmt::info!("trace={:?}", Debug2Format(&e));
            })
            .unwrap();
            cortex_m::asm::delay(12_000_000);
        }
    }
}
