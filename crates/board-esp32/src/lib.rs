#![no_std]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate std;

use core::convert::Infallible;
use io_traits::{
    AnalogInputId, AnalogOutputId, CyclicIo, DigitalInputId, DigitalOutputId, Io, Tick,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Esp32HalError {
    InvalidGpio(u8),
    InvalidAdcChannel(u8),
    InvalidPwmChannel(u8),
    InvalidAnalogScale { index: usize },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnalogScale {
    pub raw_min: u16,
    pub raw_max: u16,
    pub engineering_min: f32,
    pub engineering_max: f32,
}

impl AnalogScale {
    pub const fn adc_12bit(engineering_min: f32, engineering_max: f32) -> Self {
        Self {
            raw_min: 0,
            raw_max: 4095,
            engineering_min,
            engineering_max,
        }
    }

    pub const fn pwm_16bit(engineering_min: f32, engineering_max: f32) -> Self {
        Self {
            raw_min: 0,
            raw_max: u16::MAX,
            engineering_min,
            engineering_max,
        }
    }

    fn validate(&self, index: usize) -> Result<(), Esp32HalError> {
        if self.raw_max <= self.raw_min
            || self.engineering_max.partial_cmp(&self.engineering_min)
                != Some(core::cmp::Ordering::Greater)
        {
            return Err(Esp32HalError::InvalidAnalogScale { index });
        }
        Ok(())
    }

    fn raw_to_engineering(&self, raw: u16) -> f32 {
        let raw_span = self.raw_max.saturating_sub(self.raw_min) as f32;
        if raw_span <= f32::EPSILON {
            return self.engineering_min;
        }
        let bounded = raw.clamp(self.raw_min, self.raw_max);
        let ratio = (bounded.saturating_sub(self.raw_min) as f32) / raw_span;
        self.engineering_min + ratio * (self.engineering_max - self.engineering_min)
    }

    fn engineering_to_raw(&self, value: f32) -> u16 {
        let raw_span = self.raw_max.saturating_sub(self.raw_min) as f32;
        if raw_span <= f32::EPSILON {
            return self.raw_min;
        }
        let bounded = value.clamp(self.engineering_min, self.engineering_max);
        let ratio =
            (bounded - self.engineering_min) / (self.engineering_max - self.engineering_min);
        let raw = (self.raw_min as f32) + ratio * raw_span + 0.5;
        raw.clamp(self.raw_min as f32, self.raw_max as f32) as u16
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Esp32IoMap<const DI: usize, const DO: usize, const AI: usize, const AO: usize> {
    pub digital_inputs: [Option<u8>; DI],
    pub digital_outputs: [Option<u8>; DO],
    pub analog_inputs: [Option<u8>; AI],
    pub analog_input_scales: [AnalogScale; AI],
    pub analog_outputs: [Option<u8>; AO],
    pub analog_output_scales: [AnalogScale; AO],
}

impl<const DI: usize, const DO: usize, const AI: usize, const AO: usize>
    Esp32IoMap<DI, DO, AI, AO>
{
    pub const fn new(
        digital_inputs: [Option<u8>; DI],
        digital_outputs: [Option<u8>; DO],
        analog_inputs: [Option<u8>; AI],
        analog_input_scales: [AnalogScale; AI],
        analog_outputs: [Option<u8>; AO],
        analog_output_scales: [AnalogScale; AO],
    ) -> Self {
        Self {
            digital_inputs,
            digital_outputs,
            analog_inputs,
            analog_input_scales,
            analog_outputs,
            analog_output_scales,
        }
    }

    pub fn validate(&self) -> Result<(), Esp32HalError> {
        for gpio in self.digital_inputs.iter().flatten() {
            validate_gpio(*gpio)?;
        }
        for gpio in self.digital_outputs.iter().flatten() {
            validate_gpio(*gpio)?;
        }
        for channel in self.analog_inputs.iter().flatten() {
            validate_adc_channel(*channel)?;
        }
        for (index, scale) in self.analog_input_scales.iter().enumerate() {
            scale.validate(index)?;
        }
        for channel in self.analog_outputs.iter().flatten() {
            validate_pwm_channel(*channel)?;
        }
        for (index, scale) in self.analog_output_scales.iter().enumerate() {
            scale.validate(index)?;
        }
        Ok(())
    }
}

pub trait Esp32IoBackend {
    fn read_gpio(&self, gpio: u8) -> bool;
    fn write_gpio(&mut self, gpio: u8, value: bool);
    fn read_adc_raw(&self, channel: u8) -> u16;
    fn write_pwm_raw(&mut self, channel: u8, duty: u16);
}

#[derive(Debug)]
pub struct Esp32Runtime<B, const DI: usize, const DO: usize, const AI: usize, const AO: usize> {
    backend: B,
    map: Esp32IoMap<DI, DO, AI, AO>,
    tick: Tick,
    digital_input_cache: [bool; DI],
    analog_input_cache: [f32; AI],
    digital_output_latches: [bool; DO],
    analog_output_targets: [f32; AO],
}

impl<B, const DI: usize, const DO: usize, const AI: usize, const AO: usize>
    Esp32Runtime<B, DI, DO, AI, AO>
{
    pub fn new(backend: B, map: Esp32IoMap<DI, DO, AI, AO>) -> Result<Self, Esp32HalError> {
        map.validate()?;
        Ok(Self {
            backend,
            map,
            tick: Tick(0),
            digital_input_cache: [false; DI],
            analog_input_cache: [0.0; AI],
            digital_output_latches: [false; DO],
            analog_output_targets: [0.0; AO],
        })
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub fn digital_output_latch(&self, port: usize) -> Option<bool> {
        self.digital_output_latches.get(port).copied()
    }

    pub fn analog_output_target(&self, port: usize) -> Option<f32> {
        self.analog_output_targets.get(port).copied()
    }

    pub fn digital_input_cache(&self, port: usize) -> Option<bool> {
        self.digital_input_cache.get(port).copied()
    }

    pub fn analog_input_cache(&self, port: usize) -> Option<f32> {
        self.analog_input_cache.get(port).copied()
    }
}

impl<B, const DI: usize, const DO: usize, const AI: usize, const AO: usize> Io
    for Esp32Runtime<B, DI, DO, AI, AO>
where
    B: Esp32IoBackend,
{
    fn tick(&self) -> Tick {
        self.tick
    }

    fn advance_tick(&mut self) {
        self.tick.0 = self.tick.0.saturating_add(1);
    }

    fn read_digital_input(&self, id: DigitalInputId) -> bool {
        self.digital_input_cache
            .get(id.0 as usize)
            .copied()
            .unwrap_or(false)
    }

    fn read_analog_input(&self, id: AnalogInputId) -> f32 {
        self.analog_input_cache
            .get(id.0 as usize)
            .copied()
            .unwrap_or(0.0)
    }

    fn write_digital_output(&mut self, id: DigitalOutputId, value: bool) {
        let index = id.0 as usize;
        if let Some(slot) = self.digital_output_latches.get_mut(index) {
            *slot = value;
        }
    }

    fn write_analog_output(&mut self, id: AnalogOutputId, value: f32) {
        let index = id.0 as usize;
        if let Some(slot) = self.analog_output_targets.get_mut(index) {
            *slot = value;
        }
    }
}

impl<B, const DI: usize, const DO: usize, const AI: usize, const AO: usize> CyclicIo
    for Esp32Runtime<B, DI, DO, AI, AO>
where
    B: Esp32IoBackend,
{
    type Error = Infallible;

    fn sync_inputs(&mut self) -> Result<(), Self::Error> {
        for (index, gpio) in self.map.digital_inputs.iter().enumerate() {
            self.digital_input_cache[index] = gpio
                .map(|gpio| self.backend.read_gpio(gpio))
                .unwrap_or(false);
        }
        for (index, channel) in self.map.analog_inputs.iter().enumerate() {
            self.analog_input_cache[index] = channel
                .map(|channel| {
                    let raw = self.backend.read_adc_raw(channel);
                    self.map.analog_input_scales[index].raw_to_engineering(raw)
                })
                .unwrap_or(0.0);
        }
        Ok(())
    }

    fn flush_outputs(&mut self) -> Result<(), Self::Error> {
        for (index, gpio) in self.map.digital_outputs.iter().enumerate() {
            if let Some(gpio) = gpio {
                self.backend
                    .write_gpio(*gpio, self.digital_output_latches[index]);
            }
        }
        for (index, channel) in self.map.analog_outputs.iter().enumerate() {
            if let Some(channel) = channel {
                let duty = self.map.analog_output_scales[index]
                    .engineering_to_raw(self.analog_output_targets[index]);
                self.backend.write_pwm_raw(*channel, duty);
            }
        }
        Ok(())
    }
}

fn validate_gpio(gpio: u8) -> Result<(), Esp32HalError> {
    if gpio <= 39 {
        Ok(())
    } else {
        Err(Esp32HalError::InvalidGpio(gpio))
    }
}

fn validate_adc_channel(channel: u8) -> Result<(), Esp32HalError> {
    if channel <= 19 {
        Ok(())
    } else {
        Err(Esp32HalError::InvalidAdcChannel(channel))
    }
}

fn validate_pwm_channel(channel: u8) -> Result<(), Esp32HalError> {
    if channel <= 15 {
        Ok(())
    } else {
        Err(Esp32HalError::InvalidPwmChannel(channel))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct FakeBackend {
        gpio: [bool; 40],
        adc: [u16; 20],
        pwm: [u16; 16],
    }

    impl Default for FakeBackend {
        fn default() -> Self {
            Self {
                gpio: [false; 40],
                adc: [0; 20],
                pwm: [0; 16],
            }
        }
    }

    impl Esp32IoBackend for FakeBackend {
        fn read_gpio(&self, gpio: u8) -> bool {
            self.gpio[gpio as usize]
        }

        fn write_gpio(&mut self, gpio: u8, value: bool) {
            self.gpio[gpio as usize] = value;
        }

        fn read_adc_raw(&self, channel: u8) -> u16 {
            self.adc[channel as usize]
        }

        fn write_pwm_raw(&mut self, channel: u8, duty: u16) {
            self.pwm[channel as usize] = duty;
        }
    }

    fn test_map() -> Esp32IoMap<2, 2, 1, 1> {
        Esp32IoMap::new(
            [Some(4), Some(5)],
            [Some(18), Some(19)],
            [Some(3)],
            [AnalogScale::adc_12bit(0.0, 10.0)],
            [Some(2)],
            [AnalogScale::pwm_16bit(0.0, 100.0)],
        )
    }

    #[test]
    fn esp32_runtime_maps_digital_and_analog_io() {
        let mut backend = FakeBackend::default();
        backend.gpio[5] = true;
        backend.adc[3] = 2048;

        let mut runtime = Esp32Runtime::new(backend, test_map()).expect("valid map");

        assert_eq!(runtime.tick(), Tick(0));
        assert_eq!(runtime.read_digital_input(DigitalInputId(1)), false);
        assert_eq!(runtime.read_analog_input(AnalogInputId(0)), 0.0);

        runtime.sync_inputs().expect("input sync is infallible");
        assert_eq!(runtime.read_digital_input(DigitalInputId(1)), true);
        assert!((runtime.read_analog_input(AnalogInputId(0)) - 5.0).abs() < 0.01);
        assert_eq!(runtime.digital_input_cache(1), Some(true));
        assert!((runtime.analog_input_cache(0).unwrap() - 5.0).abs() < 0.01);

        runtime.write_digital_output(DigitalOutputId(0), true);
        runtime.write_analog_output(AnalogOutputId(0), 25.0);

        assert_eq!(runtime.digital_output_latch(0), Some(true));
        assert_eq!(runtime.analog_output_target(0), Some(25.0));
        assert_eq!(runtime.backend().gpio[18], false);
        assert_eq!(runtime.backend().pwm[2], 0);

        runtime.flush_outputs().expect("output flush is infallible");
        assert_eq!(runtime.backend().gpio[18], true);
        assert!((runtime.backend().pwm[2] as i32 - 16_384).abs() <= 1);
    }

    #[test]
    fn esp32_cycle_flushes_outputs_syncs_inputs_and_advances_tick() {
        let mut backend = FakeBackend::default();
        backend.gpio[4] = true;
        backend.adc[3] = 4095;
        let mut runtime = Esp32Runtime::new(backend, test_map()).expect("valid map");

        runtime.write_digital_output(DigitalOutputId(1), true);
        runtime.write_analog_output(AnalogOutputId(0), 50.0);
        runtime.cycle().expect("cycle is infallible");

        assert_eq!(runtime.tick(), Tick(1));
        assert_eq!(runtime.read_digital_input(DigitalInputId(0)), true);
        assert!((runtime.read_analog_input(AnalogInputId(0)) - 10.0).abs() < 0.01);
        assert_eq!(runtime.backend().gpio[19], true);
        assert!((runtime.backend().pwm[2] as i32 - 32_768).abs() <= 1);
    }

    #[test]
    fn esp32_runtime_rejects_invalid_mapping() {
        let map = Esp32IoMap::<1, 0, 0, 0>::new([Some(41)], [], [], [], [], []);
        let err = Esp32Runtime::new(FakeBackend::default(), map).expect_err("invalid gpio");
        assert_eq!(err, Esp32HalError::InvalidGpio(41));
    }
}
