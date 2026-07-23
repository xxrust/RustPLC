#![forbid(unsafe_code)]

use io_traits::{
    AnalogInputId, AnalogOutputId, CyclicIo, DigitalInputId, DigitalOutputId, Io, Tick,
};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EtherCatError {
    #[error("PDO entry for port {port} is not mapped")]
    UnmappedPort { port: u16 },
    #[error(
        "process image access out of range: offset={byte_offset}, width={width_bits} bits, image_len={image_len}"
    )]
    OutOfRange {
        byte_offset: usize,
        width_bits: u8,
        image_len: usize,
    },
    #[error("unsupported PDO width {0} bits")]
    UnsupportedWidth(u8),
    #[error("PDO entry has direction {actual:?}, expected {expected:?}")]
    DirectionMismatch {
        expected: PdoDirection,
        actual: PdoDirection,
    },
    #[error("analog scale must not be zero")]
    ZeroScale,
    #[error("EtherCAT network has no slaves")]
    NoSlaves,
    #[error("EtherCAT slave at position {position} is {state:?}, expected Operational")]
    SlaveNotOperational {
        position: u16,
        state: EtherCatSlaveState,
    },
    #[error(
        "{direction:?} process image size mismatch: expected {expected_len} bytes from slaves, actual {actual_len} bytes"
    )]
    ProcessImageSizeMismatch {
        direction: PdoDirection,
        expected_len: usize,
        actual_len: usize,
    },
}

pub type EtherCatResult<T> = Result<T, EtherCatError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdoDirection {
    RxPdo,
    TxPdo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EtherCatSlaveState {
    Init,
    PreOperational,
    SafeOperational,
    Operational,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EtherCatSlaveInfo {
    pub position: u16,
    pub vendor_id: u32,
    pub product_code: u32,
    pub state: EtherCatSlaveState,
    pub rxpdo_len: usize,
    pub txpdo_len: usize,
}

impl EtherCatSlaveInfo {
    pub fn new(
        position: u16,
        vendor_id: u32,
        product_code: u32,
        state: EtherCatSlaveState,
        rxpdo_len: usize,
        txpdo_len: usize,
    ) -> Self {
        Self {
            position,
            vendor_id,
            product_code,
            state,
            rxpdo_len,
            txpdo_len,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EtherCatNetwork {
    slaves: Vec<EtherCatSlaveInfo>,
}

impl EtherCatNetwork {
    pub fn new(slaves: Vec<EtherCatSlaveInfo>) -> Self {
        Self { slaves }
    }

    pub fn slaves(&self) -> &[EtherCatSlaveInfo] {
        &self.slaves
    }

    pub fn expected_rxpdo_len(&self) -> usize {
        self.slaves.iter().map(|slave| slave.rxpdo_len).sum()
    }

    pub fn expected_txpdo_len(&self) -> usize {
        self.slaves.iter().map(|slave| slave.txpdo_len).sum()
    }

    pub fn validate_operational_process_image(&self, image: &ProcessImage) -> EtherCatResult<()> {
        if self.slaves.is_empty() {
            return Err(EtherCatError::NoSlaves);
        }

        for slave in &self.slaves {
            if slave.state != EtherCatSlaveState::Operational {
                return Err(EtherCatError::SlaveNotOperational {
                    position: slave.position,
                    state: slave.state,
                });
            }
        }

        let expected_rxpdo_len = self.expected_rxpdo_len();
        if image.rxpdo().len() != expected_rxpdo_len {
            return Err(EtherCatError::ProcessImageSizeMismatch {
                direction: PdoDirection::RxPdo,
                expected_len: expected_rxpdo_len,
                actual_len: image.rxpdo().len(),
            });
        }

        let expected_txpdo_len = self.expected_txpdo_len();
        if image.txpdo().len() != expected_txpdo_len {
            return Err(EtherCatError::ProcessImageSizeMismatch {
                direction: PdoDirection::TxPdo,
                expected_len: expected_txpdo_len,
                actual_len: image.txpdo().len(),
            });
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoolPdoEntry {
    pub direction: PdoDirection,
    pub byte_offset: usize,
    pub bit_offset: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdoIntegerEncoding {
    U16Le,
    I16Le,
    U32Le,
    I32Le,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnalogPdoEntry {
    pub direction: PdoDirection,
    pub byte_offset: usize,
    pub encoding: PdoIntegerEncoding,
    pub scale: f32,
    pub offset: f32,
}

impl AnalogPdoEntry {
    pub fn u16_le(direction: PdoDirection, byte_offset: usize) -> Self {
        Self {
            direction,
            byte_offset,
            encoding: PdoIntegerEncoding::U16Le,
            scale: 1.0,
            offset: 0.0,
        }
    }

    pub fn i16_scaled(
        direction: PdoDirection,
        byte_offset: usize,
        scale: f32,
        offset: f32,
    ) -> Self {
        Self {
            direction,
            byte_offset,
            encoding: PdoIntegerEncoding::I16Le,
            scale,
            offset,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct EtherCatMapping {
    digital_inputs: BTreeMap<u16, BoolPdoEntry>,
    digital_outputs: BTreeMap<u16, BoolPdoEntry>,
    analog_inputs: BTreeMap<u16, AnalogPdoEntry>,
    analog_outputs: BTreeMap<u16, AnalogPdoEntry>,
}

impl EtherCatMapping {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn map_digital_input(mut self, port: u16, entry: BoolPdoEntry) -> Self {
        self.digital_inputs.insert(port, entry);
        self
    }

    pub fn map_digital_output(mut self, port: u16, entry: BoolPdoEntry) -> Self {
        self.digital_outputs.insert(port, entry);
        self
    }

    pub fn map_analog_input(mut self, port: u16, entry: AnalogPdoEntry) -> Self {
        self.analog_inputs.insert(port, entry);
        self
    }

    pub fn map_analog_output(mut self, port: u16, entry: AnalogPdoEntry) -> Self {
        self.analog_outputs.insert(port, entry);
        self
    }
}

#[derive(Debug, Clone)]
pub struct ProcessImage {
    rxpdo: Vec<u8>,
    txpdo: Vec<u8>,
}

impl ProcessImage {
    pub fn new(rxpdo_len: usize, txpdo_len: usize) -> Self {
        Self {
            rxpdo: vec![0; rxpdo_len],
            txpdo: vec![0; txpdo_len],
        }
    }

    pub fn from_images(rxpdo: Vec<u8>, txpdo: Vec<u8>) -> Self {
        Self { rxpdo, txpdo }
    }

    pub fn rxpdo(&self) -> &[u8] {
        &self.rxpdo
    }

    pub fn txpdo(&self) -> &[u8] {
        &self.txpdo
    }

    pub fn rxpdo_mut(&mut self) -> &mut [u8] {
        &mut self.rxpdo
    }

    pub fn txpdo_mut(&mut self) -> &mut [u8] {
        &mut self.txpdo
    }

    fn image(&self, direction: PdoDirection) -> &[u8] {
        match direction {
            PdoDirection::RxPdo => &self.rxpdo,
            PdoDirection::TxPdo => &self.txpdo,
        }
    }

    fn image_mut(&mut self, direction: PdoDirection) -> &mut [u8] {
        match direction {
            PdoDirection::RxPdo => &mut self.rxpdo,
            PdoDirection::TxPdo => &mut self.txpdo,
        }
    }
}

pub trait EtherCatMaster {
    fn exchange(&mut self, rxpdo: &[u8], txpdo: &mut [u8]) -> EtherCatResult<()>;
}

#[derive(Debug)]
pub struct EtherCatRuntime<M> {
    master: M,
    mapping: EtherCatMapping,
    image: ProcessImage,
    tick: Tick,
    digital_inputs: BTreeMap<u16, bool>,
    analog_inputs: BTreeMap<u16, f32>,
    dirty_digital_outputs: BTreeSet<u16>,
    dirty_analog_outputs: BTreeSet<u16>,
}

impl<M> EtherCatRuntime<M> {
    pub fn new(master: M, mapping: EtherCatMapping, image: ProcessImage) -> Self {
        Self {
            master,
            mapping,
            image,
            tick: Tick(0),
            digital_inputs: BTreeMap::new(),
            analog_inputs: BTreeMap::new(),
            dirty_digital_outputs: BTreeSet::new(),
            dirty_analog_outputs: BTreeSet::new(),
        }
    }

    pub fn new_checked(
        master: M,
        mapping: EtherCatMapping,
        image: ProcessImage,
        network: &EtherCatNetwork,
    ) -> EtherCatResult<Self> {
        network.validate_operational_process_image(&image)?;
        Ok(Self::new(master, mapping, image))
    }

    pub fn process_image(&self) -> &ProcessImage {
        &self.image
    }

    pub fn process_image_mut(&mut self) -> &mut ProcessImage {
        &mut self.image
    }

    pub fn master(&self) -> &M {
        &self.master
    }

    pub fn into_master(self) -> M {
        self.master
    }
}

impl<M: EtherCatMaster> EtherCatRuntime<M> {
    pub fn exchange(&mut self) -> EtherCatResult<()> {
        self.master
            .exchange(&self.image.rxpdo, &mut self.image.txpdo)?;
        self.refresh_inputs_from_txpdo()
    }
}

impl<M> EtherCatRuntime<M> {
    pub fn refresh_inputs_from_txpdo(&mut self) -> EtherCatResult<()> {
        for (&port, entry) in &self.mapping.digital_inputs {
            ensure_direction(*entry, PdoDirection::TxPdo)?;
            let value = read_bool(&self.image, *entry)?;
            self.digital_inputs.insert(port, value);
        }

        for (&port, entry) in &self.mapping.analog_inputs {
            ensure_analog_direction(*entry, PdoDirection::TxPdo)?;
            let raw = read_analog(&self.image, *entry)?;
            self.analog_inputs.insert(port, raw);
        }
        Ok(())
    }

    pub fn flush_outputs_to_rxpdo(&mut self) -> EtherCatResult<()> {
        let digital_ports = self
            .dirty_digital_outputs
            .iter()
            .copied()
            .collect::<Vec<_>>();
        for port in digital_ports {
            let entry = self
                .mapping
                .digital_outputs
                .get(&port)
                .copied()
                .ok_or(EtherCatError::UnmappedPort { port })?;
            ensure_direction(entry, PdoDirection::RxPdo)?;
            let value = read_cached_output_bool(&self.image, entry).unwrap_or(false);
            write_bool(&mut self.image, entry, value)?;
            self.dirty_digital_outputs.remove(&port);
        }

        let analog_ports = self
            .dirty_analog_outputs
            .iter()
            .copied()
            .collect::<Vec<_>>();
        for port in analog_ports {
            let entry = self
                .mapping
                .analog_outputs
                .get(&port)
                .copied()
                .ok_or(EtherCatError::UnmappedPort { port })?;
            ensure_analog_direction(entry, PdoDirection::RxPdo)?;
            let value = read_cached_output_analog(&self.image, entry).unwrap_or(0.0);
            write_analog(&mut self.image, entry, value)?;
            self.dirty_analog_outputs.remove(&port);
        }
        Ok(())
    }
}

impl<M> Io for EtherCatRuntime<M> {
    fn tick(&self) -> Tick {
        self.tick
    }

    fn advance_tick(&mut self) {
        self.tick.0 += 1;
    }

    fn read_digital_input(&self, id: DigitalInputId) -> bool {
        self.digital_inputs.get(&id.0).copied().unwrap_or(false)
    }

    fn read_analog_input(&self, id: AnalogInputId) -> f32 {
        self.analog_inputs.get(&id.0).copied().unwrap_or(0.0)
    }

    fn write_digital_output(&mut self, id: DigitalOutputId, value: bool) {
        if let Some(entry) = self.mapping.digital_outputs.get(&id.0).copied() {
            let _ = write_bool(&mut self.image, entry, value);
            self.dirty_digital_outputs.insert(id.0);
        }
    }

    fn write_analog_output(&mut self, id: AnalogOutputId, value: f32) {
        if let Some(entry) = self.mapping.analog_outputs.get(&id.0).copied() {
            let _ = write_analog(&mut self.image, entry, value);
            self.dirty_analog_outputs.insert(id.0);
        }
    }
}

impl<M: EtherCatMaster> CyclicIo for EtherCatRuntime<M> {
    type Error = EtherCatError;

    fn sync_inputs(&mut self) -> Result<(), Self::Error> {
        self.exchange()
    }

    fn flush_outputs(&mut self) -> Result<(), Self::Error> {
        self.flush_outputs_to_rxpdo()
    }
}

pub fn read_bool(image: &ProcessImage, entry: BoolPdoEntry) -> EtherCatResult<bool> {
    if entry.bit_offset > 7 {
        return Err(EtherCatError::UnsupportedWidth(entry.bit_offset));
    }
    let bytes = image.image(entry.direction);
    let byte = *bytes
        .get(entry.byte_offset)
        .ok_or(EtherCatError::OutOfRange {
            byte_offset: entry.byte_offset,
            width_bits: 1,
            image_len: bytes.len(),
        })?;
    Ok(byte & (1 << entry.bit_offset) != 0)
}

pub fn write_bool(
    image: &mut ProcessImage,
    entry: BoolPdoEntry,
    value: bool,
) -> EtherCatResult<()> {
    if entry.bit_offset > 7 {
        return Err(EtherCatError::UnsupportedWidth(entry.bit_offset));
    }
    let bytes = image.image_mut(entry.direction);
    let image_len = bytes.len();
    let byte = bytes
        .get_mut(entry.byte_offset)
        .ok_or(EtherCatError::OutOfRange {
            byte_offset: entry.byte_offset,
            width_bits: 1,
            image_len,
        })?;
    let mask = 1 << entry.bit_offset;
    if value {
        *byte |= mask;
    } else {
        *byte &= !mask;
    }
    Ok(())
}

pub fn read_analog(image: &ProcessImage, entry: AnalogPdoEntry) -> EtherCatResult<f32> {
    let raw = read_integer(image, entry.direction, entry.byte_offset, entry.encoding)?;
    Ok(raw as f32 * entry.scale + entry.offset)
}

pub fn write_analog(
    image: &mut ProcessImage,
    entry: AnalogPdoEntry,
    value: f32,
) -> EtherCatResult<()> {
    if entry.scale == 0.0 {
        return Err(EtherCatError::ZeroScale);
    }
    let raw = ((value - entry.offset) / entry.scale).round();
    write_integer(
        image,
        entry.direction,
        entry.byte_offset,
        entry.encoding,
        raw,
    )
}

fn read_integer(
    image: &ProcessImage,
    direction: PdoDirection,
    byte_offset: usize,
    encoding: PdoIntegerEncoding,
) -> EtherCatResult<i64> {
    let width = integer_width(encoding);
    let bytes = image.image(direction);
    let end = byte_offset.saturating_add(width);
    let slice = bytes
        .get(byte_offset..end)
        .ok_or(EtherCatError::OutOfRange {
            byte_offset,
            width_bits: (width * 8) as u8,
            image_len: bytes.len(),
        })?;
    Ok(match encoding {
        PdoIntegerEncoding::U16Le => u16::from_le_bytes([slice[0], slice[1]]) as i64,
        PdoIntegerEncoding::I16Le => i16::from_le_bytes([slice[0], slice[1]]) as i64,
        PdoIntegerEncoding::U32Le => {
            u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]) as i64
        }
        PdoIntegerEncoding::I32Le => {
            i32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]) as i64
        }
    })
}

fn write_integer(
    image: &mut ProcessImage,
    direction: PdoDirection,
    byte_offset: usize,
    encoding: PdoIntegerEncoding,
    raw: f32,
) -> EtherCatResult<()> {
    let width = integer_width(encoding);
    let bytes = image.image_mut(direction);
    let image_len = bytes.len();
    let end = byte_offset.saturating_add(width);
    let slice = bytes
        .get_mut(byte_offset..end)
        .ok_or(EtherCatError::OutOfRange {
            byte_offset,
            width_bits: (width * 8) as u8,
            image_len,
        })?;
    match encoding {
        PdoIntegerEncoding::U16Le => {
            slice.copy_from_slice(&(raw.clamp(0.0, u16::MAX as f32) as u16).to_le_bytes());
        }
        PdoIntegerEncoding::I16Le => {
            slice.copy_from_slice(
                &(raw.clamp(i16::MIN as f32, i16::MAX as f32) as i16).to_le_bytes(),
            );
        }
        PdoIntegerEncoding::U32Le => {
            slice.copy_from_slice(&(raw.clamp(0.0, u32::MAX as f32) as u32).to_le_bytes());
        }
        PdoIntegerEncoding::I32Le => {
            slice.copy_from_slice(
                &(raw.clamp(i32::MIN as f32, i32::MAX as f32) as i32).to_le_bytes(),
            );
        }
    }
    Ok(())
}

fn integer_width(encoding: PdoIntegerEncoding) -> usize {
    match encoding {
        PdoIntegerEncoding::U16Le | PdoIntegerEncoding::I16Le => 2,
        PdoIntegerEncoding::U32Le | PdoIntegerEncoding::I32Le => 4,
    }
}

fn ensure_direction(entry: BoolPdoEntry, expected: PdoDirection) -> EtherCatResult<()> {
    if entry.direction == expected {
        Ok(())
    } else {
        Err(EtherCatError::DirectionMismatch {
            expected,
            actual: entry.direction,
        })
    }
}

fn ensure_analog_direction(entry: AnalogPdoEntry, expected: PdoDirection) -> EtherCatResult<()> {
    if entry.direction == expected {
        Ok(())
    } else {
        Err(EtherCatError::DirectionMismatch {
            expected,
            actual: entry.direction,
        })
    }
}

fn read_cached_output_bool(image: &ProcessImage, entry: BoolPdoEntry) -> EtherCatResult<bool> {
    read_bool(image, entry)
}

fn read_cached_output_analog(image: &ProcessImage, entry: AnalogPdoEntry) -> EtherCatResult<f32> {
    read_analog(image, entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default)]
    struct EchoMaster {
        exchanges: usize,
    }

    impl EtherCatMaster for EchoMaster {
        fn exchange(&mut self, rxpdo: &[u8], txpdo: &mut [u8]) -> EtherCatResult<()> {
            self.exchanges += 1;
            if !txpdo.is_empty() && !rxpdo.is_empty() {
                txpdo[0] = rxpdo[0];
            }
            if txpdo.len() >= 4 {
                txpdo[2..4].copy_from_slice(&1234u16.to_le_bytes());
            }
            Ok(())
        }
    }

    #[test]
    fn pdo_bit_and_analog_access_round_trips_process_image() {
        let mut image = ProcessImage::new(4, 4);
        let output = BoolPdoEntry {
            direction: PdoDirection::RxPdo,
            byte_offset: 0,
            bit_offset: 2,
        };
        let analog = AnalogPdoEntry::i16_scaled(PdoDirection::RxPdo, 2, 0.1, 0.0);

        write_bool(&mut image, output, true).expect("bit write should fit");
        write_analog(&mut image, analog, 12.3).expect("analog write should fit");

        assert_eq!(image.rxpdo()[0], 0b0000_0100);
        assert_eq!(read_bool(&image, output).unwrap(), true);
        assert_eq!(
            i16::from_le_bytes([image.rxpdo()[2], image.rxpdo()[3]]),
            123
        );
    }

    #[test]
    fn ethercat_runtime_maps_plc_ports_to_pdo_image() {
        let mapping = EtherCatMapping::new()
            .map_digital_output(
                0,
                BoolPdoEntry {
                    direction: PdoDirection::RxPdo,
                    byte_offset: 0,
                    bit_offset: 0,
                },
            )
            .map_digital_input(
                0,
                BoolPdoEntry {
                    direction: PdoDirection::TxPdo,
                    byte_offset: 0,
                    bit_offset: 0,
                },
            )
            .map_analog_input(1, AnalogPdoEntry::u16_le(PdoDirection::TxPdo, 2));
        let mut runtime =
            EtherCatRuntime::new(EchoMaster::default(), mapping, ProcessImage::new(4, 4));

        runtime.write_digital_output(DigitalOutputId(0), true);
        runtime
            .flush_outputs_to_rxpdo()
            .expect("dirty outputs should flush into RxPDO");
        assert_eq!(runtime.process_image().rxpdo()[0], 0b0000_0001);

        runtime
            .exchange()
            .expect("master exchange should refresh TxPDO");
        assert_eq!(runtime.read_digital_input(DigitalInputId(0)), true);
        assert_eq!(runtime.read_analog_input(AnalogInputId(1)), 1234.0);
        assert_eq!(runtime.master().exchanges, 1);
    }

    #[test]
    fn checked_runtime_requires_operational_slaves_and_matching_process_image() {
        let network = EtherCatNetwork::new(vec![
            EtherCatSlaveInfo::new(
                0,
                0x0000_0002,
                0x1000_0001,
                EtherCatSlaveState::Operational,
                2,
                4,
            ),
            EtherCatSlaveInfo::new(
                1,
                0x0000_0002,
                0x1000_0002,
                EtherCatSlaveState::Operational,
                2,
                0,
            ),
        ]);
        let runtime = EtherCatRuntime::new_checked(
            EchoMaster::default(),
            EtherCatMapping::new(),
            ProcessImage::new(4, 4),
            &network,
        )
        .expect("matching operational network should construct runtime");

        assert_eq!(runtime.process_image().rxpdo().len(), 4);
        assert_eq!(runtime.process_image().txpdo().len(), 4);
    }

    #[test]
    fn checked_runtime_rejects_non_operational_slave() {
        let network = EtherCatNetwork::new(vec![EtherCatSlaveInfo::new(
            3,
            0x0000_0002,
            0x1000_0001,
            EtherCatSlaveState::SafeOperational,
            1,
            1,
        )]);
        let err = EtherCatRuntime::new_checked(
            EchoMaster::default(),
            EtherCatMapping::new(),
            ProcessImage::new(1, 1),
            &network,
        )
        .expect_err("SafeOp slave should not enter runtime scan");

        assert_eq!(
            err,
            EtherCatError::SlaveNotOperational {
                position: 3,
                state: EtherCatSlaveState::SafeOperational,
            }
        );
    }

    #[test]
    fn checked_runtime_rejects_process_image_size_mismatch() {
        let network = EtherCatNetwork::new(vec![EtherCatSlaveInfo::new(
            0,
            0x0000_0002,
            0x1000_0001,
            EtherCatSlaveState::Operational,
            8,
            4,
        )]);
        let err = EtherCatRuntime::new_checked(
            EchoMaster::default(),
            EtherCatMapping::new(),
            ProcessImage::new(4, 4),
            &network,
        )
        .expect_err("RxPDO length mismatch should be reported");

        assert_eq!(
            err,
            EtherCatError::ProcessImageSizeMismatch {
                direction: PdoDirection::RxPdo,
                expected_len: 8,
                actual_len: 4,
            }
        );
    }

    #[test]
    fn ethercat_runtime_implements_unified_cyclic_io() {
        let mapping = EtherCatMapping::new()
            .map_digital_output(
                0,
                BoolPdoEntry {
                    direction: PdoDirection::RxPdo,
                    byte_offset: 0,
                    bit_offset: 0,
                },
            )
            .map_digital_input(
                0,
                BoolPdoEntry {
                    direction: PdoDirection::TxPdo,
                    byte_offset: 0,
                    bit_offset: 0,
                },
            )
            .map_analog_input(1, AnalogPdoEntry::u16_le(PdoDirection::TxPdo, 2));
        let mut runtime =
            EtherCatRuntime::new(EchoMaster::default(), mapping, ProcessImage::new(4, 4));

        runtime.write_digital_output(DigitalOutputId(0), true);
        CyclicIo::cycle(&mut runtime).expect("EtherCAT cycle should flush, exchange, and sync");

        assert_eq!(runtime.tick(), Tick(1));
        assert_eq!(runtime.process_image().rxpdo()[0], 0b0000_0001);
        assert_eq!(runtime.read_digital_input(DigitalInputId(0)), true);
        assert_eq!(runtime.read_analog_input(AnalogInputId(1)), 1234.0);
        assert_eq!(runtime.master().exchanges, 1);
    }

    #[test]
    fn process_image_reports_bounds_errors() {
        let image = ProcessImage::new(0, 0);
        let err = read_bool(
            &image,
            BoolPdoEntry {
                direction: PdoDirection::TxPdo,
                byte_offset: 2,
                bit_offset: 0,
            },
        )
        .expect_err("missing byte should be reported");
        assert_eq!(
            err,
            EtherCatError::OutOfRange {
                byte_offset: 2,
                width_bits: 1,
                image_len: 0,
            }
        );
    }

    #[test]
    fn runtime_reports_direction_mismatch_in_mapping() {
        let mapping = EtherCatMapping::new().map_digital_input(
            0,
            BoolPdoEntry {
                direction: PdoDirection::RxPdo,
                byte_offset: 0,
                bit_offset: 0,
            },
        );
        let mut runtime =
            EtherCatRuntime::new(EchoMaster::default(), mapping, ProcessImage::new(1, 1));

        let err = runtime
            .refresh_inputs_from_txpdo()
            .expect_err("input mapped to RxPDO should be rejected");
        assert_eq!(
            err,
            EtherCatError::DirectionMismatch {
                expected: PdoDirection::TxPdo,
                actual: PdoDirection::RxPdo,
            }
        );
    }
}
