#![no_std]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate std;

/// Logical time base for deterministic runtimes.
///
/// This is intentionally a counter (not milliseconds) so different platforms can
/// choose their own wall-clock mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Tick(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DigitalInputId(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DigitalOutputId(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AnalogInputId(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AnalogOutputId(pub u16);

/// Minimal I/O surface for SIL and embedded backends.
pub trait Io {
    fn tick(&self) -> Tick;

    /// Advance logical time by exactly one tick.
    fn advance_tick(&mut self);

    fn read_digital_input(&self, id: DigitalInputId) -> bool;
    fn read_analog_input(&self, id: AnalogInputId) -> f32;

    fn write_digital_output(&mut self, id: DigitalOutputId, value: bool);
    fn write_analog_output(&mut self, id: AnalogOutputId, value: f32);
}

/// PLC-facing runtime I/O surface shared by board, fieldbus, and SIL backends.
///
/// `Io` is the low-level typed channel contract consumed by `runtime-core`.
/// `PlcRuntime` keeps the public HAL vocabulary at the PLC port layer so
/// backend adapters such as RP2040, STM32, Modbus, or EtherCAT can expose the
/// same simple contract while still satisfying `Io`.
pub trait PlcRuntime: Io {
    fn read_input(&self, port: u16) -> bool {
        self.read_digital_input(DigitalInputId(port))
    }

    fn write_output(&mut self, port: u16, value: bool) {
        self.write_digital_output(DigitalOutputId(port), value);
    }

    fn read_analog(&self, port: u16) -> f32 {
        self.read_analog_input(AnalogInputId(port))
    }

    fn write_analog(&mut self, port: u16, value: f32) {
        self.write_analog_output(AnalogOutputId(port), value);
    }
}

impl<T: Io + ?Sized> PlcRuntime for T {}

/// Fallible scan-cycle contract for fieldbus and other externally synchronized
/// backends.
///
/// Board-local GPIO backends can implement `Io` directly. Fieldbus backends
/// usually need an explicit cycle boundary where cached PLC outputs are flushed,
/// physical inputs are sampled, and the runtime tick advances only after the
/// bus transaction succeeds.
pub trait CyclicIo: PlcRuntime {
    type Error;

    fn sync_inputs(&mut self) -> Result<(), Self::Error>;
    fn flush_outputs(&mut self) -> Result<(), Self::Error>;

    fn cycle(&mut self) -> Result<(), Self::Error> {
        self.flush_outputs()?;
        self.sync_inputs()?;
        self.advance_tick();
        Ok(())
    }
}

/// A tiny in-memory implementation useful for tests and examples.
///
/// ```
/// use io_traits::*;
///
/// struct MemIo {
///     t: Tick,
///     di: [bool; 1],
///     do_: [bool; 1],
///     ai: [f32; 1],
///     ao: [f32; 1],
/// }
///
/// impl MemIo {
///     fn new() -> Self {
///         Self { t: Tick(0), di: [false], do_: [false], ai: [0.0], ao: [0.0] }
///     }
/// }
///
/// impl Io for MemIo {
///     fn tick(&self) -> Tick { self.t }
///     fn advance_tick(&mut self) { self.t.0 += 1; }
///     fn read_digital_input(&self, id: DigitalInputId) -> bool { self.di[id.0 as usize] }
///     fn read_analog_input(&self, id: AnalogInputId) -> f32 { self.ai[id.0 as usize] }
///     fn write_digital_output(&mut self, id: DigitalOutputId, value: bool) { self.do_[id.0 as usize] = value; }
///     fn write_analog_output(&mut self, id: AnalogOutputId, value: f32) { self.ao[id.0 as usize] = value; }
/// }
///
/// let mut io = MemIo::new();
/// assert_eq!(io.tick(), Tick(0));
/// io.advance_tick();
/// assert_eq!(io.tick(), Tick(1));
///
/// assert_eq!(io.read_input(0), false);
/// io.write_digital_output(DigitalOutputId(0), true);
/// io.write_output(0, false);
/// io.write_analog_output(AnalogOutputId(0), 12.5);
/// ```
pub struct _Doc;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mem_io_minimal_impl_works() {
        struct MemIo {
            t: Tick,
            di: [bool; 1],
            do_: [bool; 1],
            ai: [f32; 1],
            ao: [f32; 1],
        }

        impl MemIo {
            fn new() -> Self {
                Self {
                    t: Tick(0),
                    di: [false],
                    do_: [false],
                    ai: [0.0],
                    ao: [0.0],
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

        let mut io = MemIo::new();

        assert_eq!(io.tick(), Tick(0));
        io.advance_tick();
        io.advance_tick();
        assert_eq!(io.tick(), Tick(2));

        assert_eq!(io.read_digital_input(DigitalInputId(0)), false);
        assert_eq!(io.read_analog_input(AnalogInputId(0)), 0.0);

        io.write_digital_output(DigitalOutputId(0), true);
        io.write_analog_output(AnalogOutputId(0), 3.14);

        assert_eq!(io.do_[0], true);
        assert_eq!(io.ao[0], 3.14);
    }

    #[test]
    fn plc_runtime_port_aliases_delegate_to_typed_io() {
        struct MemIo {
            t: Tick,
            di: [bool; 2],
            do_: [bool; 2],
            ai: [f32; 2],
            ao: [f32; 2],
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

        let mut io = MemIo {
            t: Tick(0),
            di: [false, true],
            do_: [false, false],
            ai: [0.0, 42.5],
            ao: [0.0, 0.0],
        };

        assert_eq!(io.read_input(1), true);
        assert_eq!(io.read_analog(1), 42.5);

        io.write_output(0, true);
        io.write_analog(1, 12.25);

        assert_eq!(io.do_[0], true);
        assert_eq!(io.ao[1], 12.25);
    }

    #[test]
    fn cyclic_io_default_cycle_flushes_then_syncs_then_advances_tick() {
        #[derive(Default)]
        struct CycleIo {
            tick: Tick,
            flushed: bool,
            synced_after_flush: bool,
        }

        impl Io for CycleIo {
            fn tick(&self) -> Tick {
                self.tick
            }

            fn advance_tick(&mut self) {
                self.tick.0 += 1;
            }

            fn read_digital_input(&self, _id: DigitalInputId) -> bool {
                false
            }

            fn read_analog_input(&self, _id: AnalogInputId) -> f32 {
                0.0
            }

            fn write_digital_output(&mut self, _id: DigitalOutputId, _value: bool) {}

            fn write_analog_output(&mut self, _id: AnalogOutputId, _value: f32) {}
        }

        impl CyclicIo for CycleIo {
            type Error = ();

            fn sync_inputs(&mut self) -> Result<(), Self::Error> {
                self.synced_after_flush = self.flushed;
                Ok(())
            }

            fn flush_outputs(&mut self) -> Result<(), Self::Error> {
                self.flushed = true;
                Ok(())
            }
        }

        let mut io = CycleIo::default();
        io.cycle().expect("cycle should pass");

        assert_eq!(io.tick(), Tick(1));
        assert!(io.flushed);
        assert!(io.synced_after_flush);
    }
}
