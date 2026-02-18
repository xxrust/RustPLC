#![cfg(target_os = "none")]

use super::evaluate_tick_timing;
use super::PicoIo;
use rp_pico::hal::timer::Timer;

pub struct Hal {
    timer: Timer,
    pub io: PicoIo,
    tick_ms: u64,
    tick_us: u64,
    next_tick_us: u64,
    overrun_count: u64,
}

impl Hal {
    pub fn initialize(timer: Timer, io: PicoIo, tick_ms: u64) -> Self {
        let tick_us = tick_ms.saturating_mul(1000);
        let next_tick_us = timer.get_counter().ticks();
        Self {
            timer,
            io,
            tick_ms,
            tick_us,
            next_tick_us,
            overrun_count: 0,
        }
    }

    /// Sample external inputs and emit the standard `TICK ...` record.
    ///
    /// Returns (tick, tick_start_us, deadline_us) for timing accounting in `update_out`.
    pub fn update_in(&mut self) -> (u64, u64, u64) {
        let tick_start_us = self.timer.get_counter().ticks();
        let deadline_us = self.next_tick_us.saturating_add(self.tick_us);

        // Keep existing behavior: analog inputs are sampled once per tick before runtime evaluation.
        self.io.sample_analog_inputs();

        let tick = self.io.tick().0;
        defmt::info!("TICK tick={} ts_ms={}", tick, tick.saturating_mul(self.tick_ms));
        (tick, tick_start_us, deadline_us)
    }

    /// Apply outputs, emit `TIMING ...`, and pace the loop to the next tick boundary.
    pub fn update_out(&mut self, tick: u64, tick_start_us: u64, deadline_us: u64) {
        // Keep existing behavior: apply analog outputs after runtime evaluation.
        self.io.apply_analog_outputs();

        let tick_end_us = self.timer.get_counter().ticks();
        let timing = evaluate_tick_timing(tick_start_us, tick_end_us, deadline_us);
        if timing.overrun {
            self.overrun_count = self.overrun_count.saturating_add(1);
        }
        defmt::info!(
            "TIMING tick={} ts_start_us={} ts_end_us={} exec_us={} slack_us={} overrun={} overrun_count={}",
            tick,
            timing.ts_start_us,
            timing.ts_end_us,
            timing.exec_us,
            timing.slack_us,
            timing.overrun,
            self.overrun_count
        );

        self.next_tick_us = deadline_us;
        while self.timer.get_counter().ticks() < self.next_tick_us {
            cortex_m::asm::nop();
        }
    }

    /// Best-effort controlled-stop path on runtime failure.
    ///
    /// Note: hard crashes/power-loss must still be handled by the hardware safety chain.
    pub fn finalize_on_error(&mut self, tick: u64) -> ! {
        self.io.enter_safe_state();
        defmt::error!("SAFE_STATE_APPLIED tick={}", tick);
        loop {
            cortex_m::asm::nop();
        }
    }
}
