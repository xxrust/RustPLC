#![no_std]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate std;

use io_traits::{AnalogInputId, AnalogOutputId, DigitalInputId, DigitalOutputId, Io, Tick};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stm32HalError {
    InvalidGpioPin { port: Stm32GpioPort, pin: u8 },
    InvalidAdcChannel(u8),
    InvalidPwmChannel { timer: u8, channel: u8 },
    InvalidAnalogScale { index: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stm32GpioPort {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stm32GpioPin {
    pub port: Stm32GpioPort,
    pub pin: u8,
}

impl Stm32GpioPin {
    pub const fn new(port: Stm32GpioPort, pin: u8) -> Self {
        Self { port, pin }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stm32PwmChannel {
    pub timer: u8,
    pub channel: u8,
}

impl Stm32PwmChannel {
    pub const fn new(timer: u8, channel: u8) -> Self {
        Self { timer, channel }
    }
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

    fn validate(&self, index: usize) -> Result<(), Stm32HalError> {
        if self.raw_max <= self.raw_min
            || self.engineering_max.partial_cmp(&self.engineering_min)
                != Some(core::cmp::Ordering::Greater)
        {
            return Err(Stm32HalError::InvalidAnalogScale { index });
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
pub struct Stm32IoMap<const DI: usize, const DO: usize, const AI: usize, const AO: usize> {
    pub digital_inputs: [Option<Stm32GpioPin>; DI],
    pub digital_outputs: [Option<Stm32GpioPin>; DO],
    pub analog_inputs: [Option<u8>; AI],
    pub analog_input_scales: [AnalogScale; AI],
    pub analog_outputs: [Option<Stm32PwmChannel>; AO],
    pub analog_output_scales: [AnalogScale; AO],
}

impl<const DI: usize, const DO: usize, const AI: usize, const AO: usize>
    Stm32IoMap<DI, DO, AI, AO>
{
    pub const fn new(
        digital_inputs: [Option<Stm32GpioPin>; DI],
        digital_outputs: [Option<Stm32GpioPin>; DO],
        analog_inputs: [Option<u8>; AI],
        analog_input_scales: [AnalogScale; AI],
        analog_outputs: [Option<Stm32PwmChannel>; AO],
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

    pub fn validate(&self) -> Result<(), Stm32HalError> {
        for pin in self.digital_inputs.iter().flatten() {
            validate_gpio_pin(*pin)?;
        }
        for pin in self.digital_outputs.iter().flatten() {
            validate_gpio_pin(*pin)?;
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

pub trait Stm32IoBackend {
    fn read_gpio(&self, pin: Stm32GpioPin) -> bool;
    fn write_gpio(&mut self, pin: Stm32GpioPin, value: bool);
    fn read_adc_raw(&self, channel: u8) -> u16;
    fn write_pwm_raw(&mut self, channel: Stm32PwmChannel, duty: u16);
}

#[derive(Debug)]
pub struct Stm32Runtime<B, const DI: usize, const DO: usize, const AI: usize, const AO: usize> {
    backend: B,
    map: Stm32IoMap<DI, DO, AI, AO>,
    tick: Tick,
    digital_output_latches: [bool; DO],
    analog_output_targets: [f32; AO],
}

impl<B, const DI: usize, const DO: usize, const AI: usize, const AO: usize>
    Stm32Runtime<B, DI, DO, AI, AO>
{
    pub fn new(backend: B, map: Stm32IoMap<DI, DO, AI, AO>) -> Result<Self, Stm32HalError> {
        map.validate()?;
        Ok(Self {
            backend,
            map,
            tick: Tick(0),
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
}

impl<B, const DI: usize, const DO: usize, const AI: usize, const AO: usize> Io
    for Stm32Runtime<B, DI, DO, AI, AO>
where
    B: Stm32IoBackend,
{
    fn tick(&self) -> Tick {
        self.tick
    }

    fn advance_tick(&mut self) {
        self.tick.0 = self.tick.0.saturating_add(1);
    }

    fn read_digital_input(&self, id: DigitalInputId) -> bool {
        self.map
            .digital_inputs
            .get(id.0 as usize)
            .and_then(|pin| *pin)
            .map(|pin| self.backend.read_gpio(pin))
            .unwrap_or(false)
    }

    fn read_analog_input(&self, id: AnalogInputId) -> f32 {
        let index = id.0 as usize;
        let Some(channel) = self
            .map
            .analog_inputs
            .get(index)
            .and_then(|channel| *channel)
        else {
            return 0.0;
        };
        let raw = self.backend.read_adc_raw(channel);
        self.map.analog_input_scales[index].raw_to_engineering(raw)
    }

    fn write_digital_output(&mut self, id: DigitalOutputId, value: bool) {
        let index = id.0 as usize;
        if let Some(slot) = self.digital_output_latches.get_mut(index) {
            *slot = value;
        }
        if let Some(pin) = self.map.digital_outputs.get(index).and_then(|pin| *pin) {
            self.backend.write_gpio(pin, value);
        }
    }

    fn write_analog_output(&mut self, id: AnalogOutputId, value: f32) {
        let index = id.0 as usize;
        if let Some(slot) = self.analog_output_targets.get_mut(index) {
            *slot = value;
        }
        let Some(channel) = self
            .map
            .analog_outputs
            .get(index)
            .and_then(|channel| *channel)
        else {
            return;
        };
        let duty = self.map.analog_output_scales[index].engineering_to_raw(value);
        self.backend.write_pwm_raw(channel, duty);
    }
}

fn validate_gpio_pin(pin: Stm32GpioPin) -> Result<(), Stm32HalError> {
    if pin.pin <= 15 {
        Ok(())
    } else {
        Err(Stm32HalError::InvalidGpioPin {
            port: pin.port,
            pin: pin.pin,
        })
    }
}

fn validate_adc_channel(channel: u8) -> Result<(), Stm32HalError> {
    if channel <= 18 {
        Ok(())
    } else {
        Err(Stm32HalError::InvalidAdcChannel(channel))
    }
}

fn validate_pwm_channel(channel: Stm32PwmChannel) -> Result<(), Stm32HalError> {
    if (1..=14).contains(&channel.timer) && (1..=4).contains(&channel.channel) {
        Ok(())
    } else {
        Err(Stm32HalError::InvalidPwmChannel {
            timer: channel.timer,
            channel: channel.channel,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct FakeBackend {
        gpio: [[bool; 16]; 9],
        adc: [u16; 19],
        pwm: [[u16; 4]; 15],
    }

    impl Default for FakeBackend {
        fn default() -> Self {
            Self {
                gpio: [[false; 16]; 9],
                adc: [0; 19],
                pwm: [[0; 4]; 15],
            }
        }
    }

    impl Stm32IoBackend for FakeBackend {
        fn read_gpio(&self, pin: Stm32GpioPin) -> bool {
            self.gpio[port_index(pin.port)][pin.pin as usize]
        }

        fn write_gpio(&mut self, pin: Stm32GpioPin, value: bool) {
            self.gpio[port_index(pin.port)][pin.pin as usize] = value;
        }

        fn read_adc_raw(&self, channel: u8) -> u16 {
            self.adc[channel as usize]
        }

        fn write_pwm_raw(&mut self, channel: Stm32PwmChannel, duty: u16) {
            self.pwm[channel.timer as usize][(channel.channel - 1) as usize] = duty;
        }
    }

    fn port_index(port: Stm32GpioPort) -> usize {
        match port {
            Stm32GpioPort::A => 0,
            Stm32GpioPort::B => 1,
            Stm32GpioPort::C => 2,
            Stm32GpioPort::D => 3,
            Stm32GpioPort::E => 4,
            Stm32GpioPort::F => 5,
            Stm32GpioPort::G => 6,
            Stm32GpioPort::H => 7,
            Stm32GpioPort::I => 8,
        }
    }

    fn test_map() -> Stm32IoMap<2, 2, 1, 1> {
        Stm32IoMap::new(
            [
                Some(Stm32GpioPin::new(Stm32GpioPort::A, 0)),
                Some(Stm32GpioPin::new(Stm32GpioPort::B, 1)),
            ],
            [
                Some(Stm32GpioPin::new(Stm32GpioPort::C, 2)),
                Some(Stm32GpioPin::new(Stm32GpioPort::D, 3)),
            ],
            [Some(4)],
            [AnalogScale::adc_12bit(0.0, 10.0)],
            [Some(Stm32PwmChannel::new(3, 2))],
            [AnalogScale::pwm_16bit(0.0, 100.0)],
        )
    }

    #[test]
    fn stm32_runtime_maps_digital_and_analog_io() {
        let mut backend = FakeBackend::default();
        backend.gpio[port_index(Stm32GpioPort::B)][1] = true;
        backend.adc[4] = 2048;

        let mut runtime = Stm32Runtime::new(backend, test_map()).expect("map should validate");

        assert_eq!(runtime.tick(), Tick(0));
        assert_eq!(runtime.read_digital_input(DigitalInputId(1)), true);
        assert!((runtime.read_analog_input(AnalogInputId(0)) - 5.001).abs() < 0.01);

        runtime.write_digital_output(DigitalOutputId(0), true);
        runtime.write_analog_output(AnalogOutputId(0), 25.0);
        runtime.advance_tick();

        assert_eq!(runtime.tick(), Tick(1));
        assert_eq!(runtime.digital_output_latch(0), Some(true));
        assert_eq!(runtime.analog_output_target(0), Some(25.0));
        assert_eq!(
            runtime.backend().gpio[port_index(Stm32GpioPort::C)][2],
            true
        );
        assert_eq!(runtime.backend().pwm[3][1], 16384);
    }

    #[test]
    fn stm32_map_validation_rejects_invalid_channels() {
        let err = Stm32IoMap::<1, 0, 0, 0>::new(
            [Some(Stm32GpioPin::new(Stm32GpioPort::A, 16))],
            [],
            [],
            [],
            [],
            [],
        )
        .validate()
        .expect_err("pin 16 should be rejected");
        assert_eq!(
            err,
            Stm32HalError::InvalidGpioPin {
                port: Stm32GpioPort::A,
                pin: 16,
            }
        );

        let err = Stm32IoMap::<0, 0, 1, 0>::new(
            [],
            [],
            [Some(19)],
            [AnalogScale::adc_12bit(0.0, 1.0)],
            [],
            [],
        )
        .validate()
        .expect_err("ADC channel 19 should be rejected");
        assert_eq!(err, Stm32HalError::InvalidAdcChannel(19));

        let err = Stm32IoMap::<0, 0, 0, 1>::new(
            [],
            [],
            [],
            [],
            [Some(Stm32PwmChannel::new(0, 1))],
            [AnalogScale::pwm_16bit(0.0, 1.0)],
        )
        .validate()
        .expect_err("timer 0 should be rejected");
        assert_eq!(
            err,
            Stm32HalError::InvalidPwmChannel {
                timer: 0,
                channel: 1,
            }
        );
    }
}
