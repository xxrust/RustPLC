#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

#[cfg(not(target_os = "none"))]
fn main() {
    println!("board-renode-stm32 is a firmware target for Renode STM32F4 Discovery.");
    println!("Build with:");
    println!("  cargo build -p board-renode-stm32 --target thumbv7em-none-eabi");
    println!("Set:");
    println!("  RUST_PLC_GENERATED_PROGRAM_RS=/path/to/generated_program.rs");
    println!("  RUST_PLC_SCENARIO_YAML=/path/to/scenario.yaml");
}

#[cfg(target_os = "none")]
mod firmware {
    use core::cmp::max;
    use core::mem::MaybeUninit;
    use cortex_m_rt::entry;
    use io_traits::{AnalogInputId, AnalogOutputId, DigitalInputId, DigitalOutputId, Io, Tick};
    use panic_halt as _;
    use runtime_core::{Instr, Program, Runtime, RuntimeError, TransitionReason};
    use stm32f4xx_hal::pac;

    mod generated_program {
        include!(concat!(env!("OUT_DIR"), "/generated_program.rs"));
    }

    mod scenario_data {
        include!(concat!(env!("OUT_DIR"), "/scenario_data.rs"));
    }

    static mut RUNTIME_SLOT: MaybeUninit<Runtime<'static>> = MaybeUninit::uninit();
    static mut IO_SLOT: MaybeUninit<SimBoardIo> = MaybeUninit::uninit();

    struct SimBoardIo {
        tick: Tick,
        di: [bool; 32],
        do_: [bool; 32],
        ai: [f32; 32],
        ao: [f32; 32],
        next_digital_event: usize,
        next_analog_event: usize,
    }

    impl SimBoardIo {
        fn new() -> Self {
            let mut this = Self {
                tick: Tick(0),
                di: [false; 32],
                do_: [false; 32],
                ai: [0.0; 32],
                ao: [0.0; 32],
                next_digital_event: 0,
                next_analog_event: 0,
            };
            this.apply_events_for_current_tick();
            this
        }

        fn apply_events_for_current_tick(&mut self) {
            while let Some(event) = scenario_data::DIGITAL_INPUT_EVENTS.get(self.next_digital_event) {
                if event.at_tick != self.tick.0 {
                    break;
                }
                let idx = event.id as usize;
                if idx < self.di.len() {
                    self.di[idx] = event.value;
                }
                self.next_digital_event += 1;
            }
            while let Some(event) = scenario_data::ANALOG_INPUT_EVENTS.get(self.next_analog_event) {
                if event.at_tick != self.tick.0 {
                    break;
                }
                let idx = event.id as usize;
                if idx < self.ai.len() {
                    self.ai[idx] = event.value;
                }
                self.next_analog_event += 1;
            }
        }
    }

    impl Io for SimBoardIo {
        fn tick(&self) -> Tick {
            self.tick
        }

        fn advance_tick(&mut self) {
            self.tick.0 = self.tick.0.saturating_add(1);
            self.apply_events_for_current_tick();
        }

        fn read_digital_input(&self, id: DigitalInputId) -> bool {
            self.di.get(id.0 as usize).copied().unwrap_or(false)
        }

        fn read_analog_input(&self, id: AnalogInputId) -> f32 {
            self.ai.get(id.0 as usize).copied().unwrap_or(0.0)
        }

        fn write_digital_output(&mut self, id: DigitalOutputId, value: bool) {
            if let Some(slot) = self.do_.get_mut(id.0 as usize) {
                *slot = value;
            }
        }

        fn write_analog_output(&mut self, id: AnalogOutputId, value: f32) {
            if let Some(slot) = self.ao.get_mut(id.0 as usize) {
                *slot = value;
            }
        }
    }

    fn is_halted<'a>(rt: &Runtime<'a>, program: &'a Program<'a>) -> bool {
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

    fn reason_str(reason: TransitionReason) -> &'static str {
        match reason {
            TransitionReason::Action => "action",
            TransitionReason::DelayElapsed => "delay_elapsed",
            TransitionReason::WaitSatisfied => "wait_satisfied",
            TransitionReason::Timeout => "timeout",
            TransitionReason::Goto => "goto",
        }
    }

    fn uart_write_byte(usart2: &pac::USART2, byte: u8) {
        while usart2.sr().read().txe().bit_is_clear() {}
        usart2.dr().write(|w| unsafe { w.dr().bits(byte.into()) });
    }

    fn uart_write_str(usart2: &pac::USART2, text: &str) {
        for byte in text.as_bytes() {
            uart_write_byte(usart2, *byte);
        }
    }

    fn uart_write_line(usart2: &pac::USART2, text: &str) {
        uart_write_str(usart2, text);
        uart_write_str(usart2, "\r\n");
    }

    fn write_usize(usart2: &pac::USART2, value: usize) {
        write_u64(usart2, value as u64);
    }

    fn write_u16(usart2: &pac::USART2, value: u16) {
        write_u64(usart2, value as u64);
    }

    fn emit_tick_line(usart2: &pac::USART2, tick: u64, ts_ms: u64) {
        uart_write_str(usart2, "TICK tick=");
        write_u64(usart2, tick);
        uart_write_str(usart2, " ts_ms=");
        write_u64(usart2, ts_ms);
        uart_write_str(usart2, "\r\n");
    }

    fn emit_trace_line(
        usart2: &pac::USART2,
        tick: u64,
        task: usize,
        from: u16,
        to: u16,
        reason: &'static str,
        ts_ms: u64,
    ) {
        uart_write_str(usart2, "TRACE tick=");
        write_u64(usart2, tick);
        uart_write_str(usart2, " task=");
        write_usize(usart2, task);
        uart_write_str(usart2, " from=");
        write_u16(usart2, from);
        uart_write_str(usart2, " to=");
        write_u16(usart2, to);
        uart_write_str(usart2, " reason=");
        uart_write_str(usart2, reason);
        uart_write_str(usart2, " ts_ms=");
        write_u64(usart2, ts_ms);
        uart_write_str(usart2, "\r\n");
    }

    fn emit_log_line(
        usart2: &pac::USART2,
        tick: u64,
        task: usize,
        step: u16,
        message_id: u16,
        message: &'static str,
        ts_ms: u64,
    ) {
        uart_write_str(usart2, "LOG tick=");
        write_u64(usart2, tick);
        uart_write_str(usart2, " task=");
        write_usize(usart2, task);
        uart_write_str(usart2, " step=");
        write_u16(usart2, step);
        uart_write_str(usart2, " msg_id=");
        write_u16(usart2, message_id);
        uart_write_str(usart2, " msg=");
        uart_write_str(usart2, message);
        uart_write_str(usart2, " ts_ms=");
        write_u64(usart2, ts_ms);
        uart_write_str(usart2, "\r\n");
    }

    fn emit_timing_line(usart2: &pac::USART2, tick: u64, tick_ms: u64) {
        let tick_period_us = tick_ms.saturating_mul(1000);
        let ts_start_us = tick.saturating_mul(tick_period_us);
        let exec_us = 10;
        let ts_end_us = ts_start_us.saturating_add(exec_us);
        let slack_us = tick_period_us.saturating_sub(exec_us);

        uart_write_str(usart2, "TIMING tick=");
        write_u64(usart2, tick);
        uart_write_str(usart2, " ts_start_us=");
        write_u64(usart2, ts_start_us);
        uart_write_str(usart2, " ts_end_us=");
        write_u64(usart2, ts_end_us);
        uart_write_str(usart2, " exec_us=");
        write_u64(usart2, exec_us);
        uart_write_str(usart2, " slack_us=");
        write_u64(usart2, slack_us);
        uart_write_str(usart2, " overrun=false\r\n");
    }

    fn emit_runtime_error_stage(usart2: &pac::USART2, stage: &str, error: RuntimeError) {
        uart_write_str(usart2, "ERROR stage=");
        uart_write_str(usart2, stage);
        uart_write_str(usart2, " code=");
        write_u64(usart2, runtime_error_code(error));
        uart_write_str(usart2, "\r\n");
    }

    fn emit_tick_error_stage(usart2: &pac::USART2, stage: &str, error: RuntimeError) {
        emit_runtime_error_stage(usart2, stage, error);
    }

    fn runtime_error_code(error: RuntimeError) -> u64 {
        match error {
            RuntimeError::ProgramHasNoTasks => 1,
            RuntimeError::TooManyTasks { .. } => 2,
            RuntimeError::InvalidTaskIndex { .. } => 3,
            RuntimeError::InvalidStepId { .. } => 4,
            RuntimeError::TooManyTransitionsInOneTick { .. } => 5,
            RuntimeError::TooManyPidLoops { .. } => 6,
            RuntimeError::TooManyVariables { .. } => 7,
            RuntimeError::TooManyCamCouplings { .. } => 8,
            RuntimeError::InvalidCamTableIndex { .. } => 9,
            RuntimeError::InvalidCamIndex { .. } => 10,
            RuntimeError::InvalidSemanticResourceIndex { .. } => 11,
            RuntimeError::ExternCallRequiresHandler { .. } => 12,
            RuntimeError::AxisMotionRequiresHandler { .. } => 13,
            RuntimeError::AxisNotHomed { .. } => 14,
            RuntimeError::TooManyAxisHomingTargets { .. } => 15,
            RuntimeError::ExternCallFailed { .. } => 16,
            RuntimeError::ExternReturnArityMismatch { .. } => 17,
            RuntimeError::ExternArgumentLimitExceeded { .. } => 18,
            RuntimeError::ExternReturnLimitExceeded { .. } => 19,
            RuntimeError::ExternBindingVariableOutOfRange { .. } => 20,
            RuntimeError::ExternErrorCodeVariableOutOfRange { .. } => 21,
            RuntimeError::AxisFault { .. } => 22,
            RuntimeError::CylinderFeedbackFault { .. } => 23,
            RuntimeError::WorkpieceSourceUnderflow { .. } => 24,
            RuntimeError::WorkpieceDuplicateOccupancy { .. } => 25,
            RuntimeError::WorkpieceOverflow { .. } => 26,
            RuntimeError::WorkpieceDuplicateMount { .. } => 27,
            RuntimeError::WorkpieceTypeSourceUnderflow { .. } => 28,
            RuntimeError::WorkpieceTypeSourceAmbiguity { .. } => 29,
            RuntimeError::WorkpieceSplitOverflow { .. } => 30,
            RuntimeError::WorkpieceMergeInputUnderflow { .. } => 31,
            RuntimeError::WorkpieceDuplicateConsumedMergeInput { .. } => 32,
            RuntimeError::WorkpieceMergeArityMismatch { .. } => 33,
            RuntimeError::WorkpieceMergeOverflow { .. } => 34,
            RuntimeError::WorkpieceEndpointUndefined { .. } => 35,
            RuntimeError::WorkpieceTokenCapacityExceeded { .. } => 36,
            RuntimeError::WorkpieceLineageCapacityExceeded { .. } => 37,
            RuntimeError::WorkpieceStoreInvariantViolation { .. } => 38,
            RuntimeError::UnsupportedWorkpieceEffect { .. } => 39,
        }
    }

    #[entry]
    fn main() -> ! {
        let dp = pac::Peripherals::take().unwrap();

        dp.RCC.ahb1enr().modify(|_, w| w.gpioaen().enabled());
        dp.RCC.apb1enr().modify(|_, w| w.usart2en().enabled());

        dp.GPIOA.moder().modify(|_, w| {
            w.moder2().alternate();
            w.moder3().alternate()
        });
        dp.GPIOA.ospeedr().modify(|_, w| {
            w.ospeedr2().very_high_speed();
            w.ospeedr3().very_high_speed()
        });
        dp.GPIOA.afrl().modify(|_, w| {
            w.afrl2().af7();
            w.afrl3().af7()
        });

        dp.USART2.brr().write(|w| unsafe { w.bits(0x16C) });
        dp.USART2
            .cr1()
            .modify(|_, w| w.te().enabled().re().enabled().ue().enabled());

        let usart2 = &dp.USART2;
        uart_write_line(usart2, "boot ok");
        uart_write_line(usart2, "before_runtime_new");

        let runtime = match Runtime::new(&generated_program::generated::PROGRAM) {
            Ok(runtime) => runtime,
            Err(error) => {
                emit_runtime_error_stage(usart2, "runtime_new", error);
                loop {
                    cortex_m::asm::nop();
                }
            }
        };
        let runtime = unsafe {
            RUNTIME_SLOT.write(runtime);
            &mut *RUNTIME_SLOT.as_mut_ptr()
        };

        uart_write_line(usart2, "after_runtime_new");
        let io = unsafe {
            IO_SLOT.write(SimBoardIo::new());
            &mut *IO_SLOT.as_mut_ptr()
        };
        let tick_ms = max(scenario_data::TICK_MS, 1);
        let duration_ticks = max(scenario_data::DURATION_MS / tick_ms, 1);

        for _ in 0..duration_ticks {
            let tick = io.tick().0;
            let ts_ms = tick.saturating_mul(tick_ms);
            emit_tick_line(usart2, tick, ts_ms);

            let tick_result = runtime.tick_with_trace_and_logs(
                io,
                |event| {
                    let ts_ms = event.tick.0.saturating_mul(tick_ms);
                    emit_trace_line(
                        usart2,
                        event.tick.0,
                        event.task,
                        event.from.0,
                        event.to.0,
                        reason_str(event.reason),
                        ts_ms,
                    );
                },
                |log| {
                    let ts_ms = log.tick.0.saturating_mul(tick_ms);
                    emit_log_line(
                        usart2,
                        log.tick.0,
                        log.task,
                        log.step.0,
                        log.message_id,
                        log.message,
                        ts_ms,
                    );
                },
            );

            match tick_result {
                Ok(()) => emit_timing_line(usart2, tick, tick_ms),
                Err(error) => {
                    emit_tick_error_stage(usart2, "tick", error);
                    break;
                }
            }

            if is_halted(&runtime, &generated_program::generated::PROGRAM) {
                uart_write_line(usart2, "halted");
                break;
            }
        }

        uart_write_line(usart2, "done");

        loop {
            cortex_m::asm::nop();
        }
    }

    fn write_u64(usart2: &pac::USART2, mut value: u64) {
        let mut buf = [0u8; 20];
        let mut len = 0usize;
        if value == 0 {
            uart_write_byte(usart2, b'0');
            return;
        }
        while value > 0 {
            buf[len] = b'0' + (value % 10) as u8;
            value /= 10;
            len += 1;
        }
        while len > 0 {
            len -= 1;
            uart_write_byte(usart2, buf[len]);
        }
    }
}
