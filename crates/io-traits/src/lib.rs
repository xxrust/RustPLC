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
/// io.write_digital_output(DigitalOutputId(0), true);
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
}
