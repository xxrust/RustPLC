#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

// Host build: keep workspace builds green and provide instructions.
#[cfg(not(target_os = "none"))]
fn main() {
    println!("board-rp2040 is a firmware target (RP2040 / Raspberry Pi Pico).");
    println!("Build it for Pico with:");
    println!("  rustup target add thumbv6m-none-eabi");
    println!("  cargo build -p board-rp2040 --target thumbv6m-none-eabi");
}

// Embedded firmware build (thumbv6m-none-eabi, target_os = "none").
#[cfg(target_os = "none")]
mod firmware {
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

    // RP2040 needs a 2nd stage bootloader stored at the start of flash.
    #[link_section = ".boot2"]
    #[used]
    static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

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

        // Minimal "tick" source: an incrementing counter paced by a crude busy-wait.
        // This is good enough as a skeleton; later stories can replace it with a timer ISR.
        let mut tick: u64 = 0;
        loop {
            tick += 1;
            defmt::info!("tick={}", tick);
            cortex_m::asm::delay(12_000_000);
        }
    }
}
