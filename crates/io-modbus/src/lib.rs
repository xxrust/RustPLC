#![forbid(unsafe_code)]

use io_traits::{
    AnalogInputId, AnalogOutputId, CyclicIo, DigitalInputId, DigitalOutputId, Io, Tick,
};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::net::TcpStream;
use thiserror::Error;

pub const MODBUS_TCP_HEADER_LEN: usize = 7;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ModbusError {
    #[error("modbus transport error: {0}")]
    Transport(String),
    #[error("modbus protocol error: {0}")]
    Protocol(String),
    #[error("modbus exception {code:#04x}")]
    Exception { code: u8 },
    #[error("modbus address {address} is not mapped")]
    UnmappedAddress { address: u16 },
    #[error("modbus response unit id {actual} did not match request unit id {expected}")]
    UnitIdMismatch { expected: u8, actual: u8 },
    #[error(
        "modbus response function {actual:#04x} did not match request function {expected:#04x}"
    )]
    FunctionMismatch { expected: u8, actual: u8 },
}

pub type ModbusResult<T> = Result<T, ModbusError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoolArea {
    Coil,
    DiscreteInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterArea {
    HoldingRegister,
    InputRegister,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterEncoding {
    U16,
    I16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoolMapping {
    pub area: BoolArea,
    pub address: u16,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegisterMapping {
    pub area: RegisterArea,
    pub address: u16,
    pub encoding: RegisterEncoding,
    pub scale: f32,
    pub offset: f32,
}

impl RegisterMapping {
    pub fn u16(area: RegisterArea, address: u16) -> Self {
        Self {
            area,
            address,
            encoding: RegisterEncoding::U16,
            scale: 1.0,
            offset: 0.0,
        }
    }

    pub fn i16_scaled(area: RegisterArea, address: u16, scale: f32, offset: f32) -> Self {
        Self {
            area,
            address,
            encoding: RegisterEncoding::I16,
            scale,
            offset,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModbusMapping {
    digital_inputs: BTreeMap<u16, BoolMapping>,
    digital_outputs: BTreeMap<u16, BoolMapping>,
    analog_inputs: BTreeMap<u16, RegisterMapping>,
    analog_outputs: BTreeMap<u16, RegisterMapping>,
}

impl ModbusMapping {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn map_digital_input(mut self, port: u16, area: BoolArea, address: u16) -> Self {
        self.digital_inputs
            .insert(port, BoolMapping { area, address });
        self
    }

    pub fn map_digital_output(mut self, port: u16, area: BoolArea, address: u16) -> Self {
        self.digital_outputs
            .insert(port, BoolMapping { area, address });
        self
    }

    pub fn map_analog_input(mut self, port: u16, mapping: RegisterMapping) -> Self {
        self.analog_inputs.insert(port, mapping);
        self
    }

    pub fn map_analog_output(mut self, port: u16, mapping: RegisterMapping) -> Self {
        self.analog_outputs.insert(port, mapping);
        self
    }
}

pub trait ModbusClient {
    fn read_coils(&mut self, unit_id: u8, address: u16, count: u16) -> ModbusResult<Vec<bool>>;
    fn read_discrete_inputs(
        &mut self,
        unit_id: u8,
        address: u16,
        count: u16,
    ) -> ModbusResult<Vec<bool>>;
    fn read_holding_registers(
        &mut self,
        unit_id: u8,
        address: u16,
        count: u16,
    ) -> ModbusResult<Vec<u16>>;
    fn read_input_registers(
        &mut self,
        unit_id: u8,
        address: u16,
        count: u16,
    ) -> ModbusResult<Vec<u16>>;
    fn write_single_coil(&mut self, unit_id: u8, address: u16, value: bool) -> ModbusResult<()>;
    fn write_single_register(&mut self, unit_id: u8, address: u16, value: u16) -> ModbusResult<()>;
}

#[derive(Debug)]
pub struct ModbusRuntime<C> {
    client: C,
    unit_id: u8,
    mapping: ModbusMapping,
    tick: Tick,
    digital_inputs: BTreeMap<u16, bool>,
    digital_outputs: BTreeMap<u16, bool>,
    analog_inputs: BTreeMap<u16, f32>,
    analog_outputs: BTreeMap<u16, f32>,
    dirty_digital_outputs: BTreeSet<u16>,
    dirty_analog_outputs: BTreeSet<u16>,
}

impl<C> ModbusRuntime<C> {
    pub fn new(client: C, unit_id: u8, mapping: ModbusMapping) -> Self {
        Self {
            client,
            unit_id,
            mapping,
            tick: Tick(0),
            digital_inputs: BTreeMap::new(),
            digital_outputs: BTreeMap::new(),
            analog_inputs: BTreeMap::new(),
            analog_outputs: BTreeMap::new(),
            dirty_digital_outputs: BTreeSet::new(),
            dirty_analog_outputs: BTreeSet::new(),
        }
    }

    pub fn client(&self) -> &C {
        &self.client
    }

    pub fn client_mut(&mut self) -> &mut C {
        &mut self.client
    }

    pub fn into_client(self) -> C {
        self.client
    }
}

impl<C: ModbusClient> ModbusRuntime<C> {
    pub fn sync_inputs(&mut self) -> ModbusResult<()> {
        for (&port, mapping) in &self.mapping.digital_inputs {
            let value = match mapping.area {
                BoolArea::Coil => read_one_bool(
                    self.client.read_coils(self.unit_id, mapping.address, 1),
                    mapping.address,
                )?,
                BoolArea::DiscreteInput => read_one_bool(
                    self.client
                        .read_discrete_inputs(self.unit_id, mapping.address, 1),
                    mapping.address,
                )?,
            };
            self.digital_inputs.insert(port, value);
        }

        for (&port, mapping) in &self.mapping.analog_inputs {
            let raw = match mapping.area {
                RegisterArea::HoldingRegister => read_one_register(
                    self.client
                        .read_holding_registers(self.unit_id, mapping.address, 1),
                    mapping.address,
                )?,
                RegisterArea::InputRegister => read_one_register(
                    self.client
                        .read_input_registers(self.unit_id, mapping.address, 1),
                    mapping.address,
                )?,
            };
            self.analog_inputs
                .insert(port, decode_register(*mapping, raw));
        }

        Ok(())
    }

    pub fn flush_outputs(&mut self) -> ModbusResult<()> {
        let digital_ports = self
            .dirty_digital_outputs
            .iter()
            .copied()
            .collect::<Vec<_>>();
        for port in digital_ports {
            let mapping = self
                .mapping
                .digital_outputs
                .get(&port)
                .ok_or(ModbusError::UnmappedAddress { address: port })?;
            if mapping.area != BoolArea::Coil {
                return Err(ModbusError::Protocol(
                    "digital outputs must map to Modbus coils".to_string(),
                ));
            }
            let value = self.digital_outputs.get(&port).copied().unwrap_or(false);
            self.client
                .write_single_coil(self.unit_id, mapping.address, value)?;
            self.dirty_digital_outputs.remove(&port);
        }

        let analog_ports = self
            .dirty_analog_outputs
            .iter()
            .copied()
            .collect::<Vec<_>>();
        for port in analog_ports {
            let mapping = self
                .mapping
                .analog_outputs
                .get(&port)
                .ok_or(ModbusError::UnmappedAddress { address: port })?;
            if mapping.area != RegisterArea::HoldingRegister {
                return Err(ModbusError::Protocol(
                    "analog outputs must map to Modbus holding registers".to_string(),
                ));
            }
            let value = self.analog_outputs.get(&port).copied().unwrap_or(0.0);
            self.client.write_single_register(
                self.unit_id,
                mapping.address,
                encode_register(*mapping, value),
            )?;
            self.dirty_analog_outputs.remove(&port);
        }

        Ok(())
    }
}

impl<C> Io for ModbusRuntime<C> {
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
        self.digital_outputs.insert(id.0, value);
        self.dirty_digital_outputs.insert(id.0);
    }

    fn write_analog_output(&mut self, id: AnalogOutputId, value: f32) {
        self.analog_outputs.insert(id.0, value);
        self.dirty_analog_outputs.insert(id.0);
    }
}

impl<C: ModbusClient> CyclicIo for ModbusRuntime<C> {
    type Error = ModbusError;

    fn sync_inputs(&mut self) -> Result<(), Self::Error> {
        ModbusRuntime::sync_inputs(self)
    }

    fn flush_outputs(&mut self) -> Result<(), Self::Error> {
        ModbusRuntime::flush_outputs(self)
    }
}

pub trait ModbusTcpTransport {
    fn transact(&mut self, request: &[u8]) -> ModbusResult<Vec<u8>>;
}

pub struct TcpStreamTransport {
    stream: TcpStream,
}

impl TcpStreamTransport {
    pub fn new(stream: TcpStream) -> Self {
        Self { stream }
    }

    pub fn connect(addr: impl std::net::ToSocketAddrs) -> ModbusResult<Self> {
        TcpStream::connect(addr)
            .map(Self::new)
            .map_err(|err| ModbusError::Transport(err.to_string()))
    }
}

impl ModbusTcpTransport for TcpStreamTransport {
    fn transact(&mut self, request: &[u8]) -> ModbusResult<Vec<u8>> {
        self.stream
            .write_all(request)
            .map_err(|err| ModbusError::Transport(err.to_string()))?;

        let mut header = [0u8; MODBUS_TCP_HEADER_LEN];
        self.stream
            .read_exact(&mut header)
            .map_err(|err| ModbusError::Transport(err.to_string()))?;
        let length = u16::from_be_bytes([header[4], header[5]]) as usize;
        if length == 0 {
            return Err(ModbusError::Protocol(
                "Modbus TCP length must include unit id".to_string(),
            ));
        }
        let mut rest = vec![0u8; length - 1];
        self.stream
            .read_exact(&mut rest)
            .map_err(|err| ModbusError::Transport(err.to_string()))?;

        let mut response = header.to_vec();
        response.extend(rest);
        Ok(response)
    }
}

#[derive(Debug)]
pub struct ModbusTcpClient<T> {
    transport: T,
    next_transaction_id: u16,
}

impl<T> ModbusTcpClient<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            next_transaction_id: 1,
        }
    }

    pub fn into_transport(self) -> T {
        self.transport
    }
}

impl<T: ModbusTcpTransport> ModbusClient for ModbusTcpClient<T> {
    fn read_coils(&mut self, unit_id: u8, address: u16, count: u16) -> ModbusResult<Vec<bool>> {
        self.read_bits(unit_id, 0x01, address, count)
    }

    fn read_discrete_inputs(
        &mut self,
        unit_id: u8,
        address: u16,
        count: u16,
    ) -> ModbusResult<Vec<bool>> {
        self.read_bits(unit_id, 0x02, address, count)
    }

    fn read_holding_registers(
        &mut self,
        unit_id: u8,
        address: u16,
        count: u16,
    ) -> ModbusResult<Vec<u16>> {
        self.read_registers(unit_id, 0x03, address, count)
    }

    fn read_input_registers(
        &mut self,
        unit_id: u8,
        address: u16,
        count: u16,
    ) -> ModbusResult<Vec<u16>> {
        self.read_registers(unit_id, 0x04, address, count)
    }

    fn write_single_coil(&mut self, unit_id: u8, address: u16, value: bool) -> ModbusResult<()> {
        let raw = if value { 0xff00u16 } else { 0x0000u16 };
        self.write_single(unit_id, 0x05, address, raw)
    }

    fn write_single_register(&mut self, unit_id: u8, address: u16, value: u16) -> ModbusResult<()> {
        self.write_single(unit_id, 0x06, address, value)
    }
}

impl<T: ModbusTcpTransport> ModbusTcpClient<T> {
    fn read_bits(
        &mut self,
        unit_id: u8,
        function: u8,
        address: u16,
        count: u16,
    ) -> ModbusResult<Vec<bool>> {
        let pdu = read_request_pdu(function, address, count);
        let response = self.transact_pdu(unit_id, &pdu)?;
        verify_response_function(function, &response)?;
        if response.len() < 2 {
            return Err(ModbusError::Protocol("short bit-read response".to_string()));
        }
        Ok(unpack_bits(&response[2..], count as usize))
    }

    fn read_registers(
        &mut self,
        unit_id: u8,
        function: u8,
        address: u16,
        count: u16,
    ) -> ModbusResult<Vec<u16>> {
        let pdu = read_request_pdu(function, address, count);
        let response = self.transact_pdu(unit_id, &pdu)?;
        verify_response_function(function, &response)?;
        if response.len() < 2 {
            return Err(ModbusError::Protocol(
                "short register-read response".to_string(),
            ));
        }
        let byte_count = response[1] as usize;
        if response.len() != 2 + byte_count || byte_count % 2 != 0 {
            return Err(ModbusError::Protocol(
                "invalid register byte count".to_string(),
            ));
        }
        Ok(response[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
            .collect())
    }

    fn write_single(
        &mut self,
        unit_id: u8,
        function: u8,
        address: u16,
        value: u16,
    ) -> ModbusResult<()> {
        let pdu = write_single_request_pdu(function, address, value);
        let response = self.transact_pdu(unit_id, &pdu)?;
        verify_response_function(function, &response)?;
        if response != pdu {
            return Err(ModbusError::Protocol(
                "write-single response did not echo request".to_string(),
            ));
        }
        Ok(())
    }

    fn transact_pdu(&mut self, unit_id: u8, pdu: &[u8]) -> ModbusResult<Vec<u8>> {
        let transaction_id = self.next_transaction_id;
        self.next_transaction_id = self.next_transaction_id.wrapping_add(1);
        let request = encode_tcp_frame(transaction_id, unit_id, pdu)?;
        let response = self.transport.transact(&request)?;
        let frame = decode_tcp_frame(&response)?;
        if frame.transaction_id != transaction_id {
            return Err(ModbusError::Protocol("transaction id mismatch".to_string()));
        }
        if frame.unit_id != unit_id {
            return Err(ModbusError::UnitIdMismatch {
                expected: unit_id,
                actual: frame.unit_id,
            });
        }
        Ok(frame.pdu)
    }
}

pub trait ModbusRtuTransport {
    fn transact(&mut self, request: &[u8]) -> ModbusResult<Vec<u8>>;
}

#[derive(Debug)]
pub struct ModbusRtuClient<T> {
    transport: T,
}

impl<T> ModbusRtuClient<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    pub fn into_transport(self) -> T {
        self.transport
    }
}

impl<T: ModbusRtuTransport> ModbusClient for ModbusRtuClient<T> {
    fn read_coils(&mut self, unit_id: u8, address: u16, count: u16) -> ModbusResult<Vec<bool>> {
        self.read_bits(unit_id, 0x01, address, count)
    }

    fn read_discrete_inputs(
        &mut self,
        unit_id: u8,
        address: u16,
        count: u16,
    ) -> ModbusResult<Vec<bool>> {
        self.read_bits(unit_id, 0x02, address, count)
    }

    fn read_holding_registers(
        &mut self,
        unit_id: u8,
        address: u16,
        count: u16,
    ) -> ModbusResult<Vec<u16>> {
        self.read_registers(unit_id, 0x03, address, count)
    }

    fn read_input_registers(
        &mut self,
        unit_id: u8,
        address: u16,
        count: u16,
    ) -> ModbusResult<Vec<u16>> {
        self.read_registers(unit_id, 0x04, address, count)
    }

    fn write_single_coil(&mut self, unit_id: u8, address: u16, value: bool) -> ModbusResult<()> {
        let raw = if value { 0xff00u16 } else { 0x0000u16 };
        self.write_single(unit_id, 0x05, address, raw)
    }

    fn write_single_register(&mut self, unit_id: u8, address: u16, value: u16) -> ModbusResult<()> {
        self.write_single(unit_id, 0x06, address, value)
    }
}

impl<T: ModbusRtuTransport> ModbusRtuClient<T> {
    fn read_bits(
        &mut self,
        unit_id: u8,
        function: u8,
        address: u16,
        count: u16,
    ) -> ModbusResult<Vec<bool>> {
        let pdu = read_request_pdu(function, address, count);
        let response = self.transact_pdu(unit_id, &pdu)?;
        verify_response_function(function, &response)?;
        if response.len() < 2 {
            return Err(ModbusError::Protocol("short bit-read response".to_string()));
        }
        Ok(unpack_bits(&response[2..], count as usize))
    }

    fn read_registers(
        &mut self,
        unit_id: u8,
        function: u8,
        address: u16,
        count: u16,
    ) -> ModbusResult<Vec<u16>> {
        let pdu = read_request_pdu(function, address, count);
        let response = self.transact_pdu(unit_id, &pdu)?;
        verify_response_function(function, &response)?;
        if response.len() < 2 {
            return Err(ModbusError::Protocol(
                "short register-read response".to_string(),
            ));
        }
        let byte_count = response[1] as usize;
        if response.len() != 2 + byte_count || byte_count % 2 != 0 {
            return Err(ModbusError::Protocol(
                "invalid register byte count".to_string(),
            ));
        }
        Ok(response[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
            .collect())
    }

    fn write_single(
        &mut self,
        unit_id: u8,
        function: u8,
        address: u16,
        value: u16,
    ) -> ModbusResult<()> {
        let pdu = write_single_request_pdu(function, address, value);
        let response = self.transact_pdu(unit_id, &pdu)?;
        verify_response_function(function, &response)?;
        if response != pdu {
            return Err(ModbusError::Protocol(
                "write-single response did not echo request".to_string(),
            ));
        }
        Ok(())
    }

    fn transact_pdu(&mut self, unit_id: u8, pdu: &[u8]) -> ModbusResult<Vec<u8>> {
        let request = encode_rtu_frame(unit_id, pdu)?;
        let response = self.transport.transact(&request)?;
        let frame = decode_rtu_frame(&response)?;
        if frame.unit_id != unit_id {
            return Err(ModbusError::UnitIdMismatch {
                expected: unit_id,
                actual: frame.unit_id,
            });
        }
        Ok(frame.pdu)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModbusTcpFrame {
    pub transaction_id: u16,
    pub unit_id: u8,
    pub pdu: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModbusRtuFrame {
    pub unit_id: u8,
    pub pdu: Vec<u8>,
}

pub fn encode_tcp_frame(transaction_id: u16, unit_id: u8, pdu: &[u8]) -> ModbusResult<Vec<u8>> {
    if pdu.is_empty() {
        return Err(ModbusError::Protocol("empty Modbus PDU".to_string()));
    }
    if pdu.len() > u16::MAX as usize - 1 {
        return Err(ModbusError::Protocol(
            "Modbus TCP PDU too large".to_string(),
        ));
    }
    let length = (pdu.len() + 1) as u16;
    let mut frame = Vec::with_capacity(MODBUS_TCP_HEADER_LEN + pdu.len());
    frame.extend(transaction_id.to_be_bytes());
    frame.extend(0u16.to_be_bytes());
    frame.extend(length.to_be_bytes());
    frame.push(unit_id);
    frame.extend(pdu);
    Ok(frame)
}

pub fn decode_tcp_frame(frame: &[u8]) -> ModbusResult<ModbusTcpFrame> {
    if frame.len() < MODBUS_TCP_HEADER_LEN + 1 {
        return Err(ModbusError::Protocol("short Modbus TCP frame".to_string()));
    }
    let transaction_id = u16::from_be_bytes([frame[0], frame[1]]);
    let protocol_id = u16::from_be_bytes([frame[2], frame[3]]);
    if protocol_id != 0 {
        return Err(ModbusError::Protocol(format!(
            "unsupported Modbus TCP protocol id {protocol_id}"
        )));
    }
    let length = u16::from_be_bytes([frame[4], frame[5]]) as usize;
    if length == 0 || frame.len() != MODBUS_TCP_HEADER_LEN + length - 1 {
        return Err(ModbusError::Protocol(
            "Modbus TCP length does not match frame".to_string(),
        ));
    }
    Ok(ModbusTcpFrame {
        transaction_id,
        unit_id: frame[6],
        pdu: frame[7..].to_vec(),
    })
}

pub fn encode_rtu_frame(unit_id: u8, pdu: &[u8]) -> ModbusResult<Vec<u8>> {
    if pdu.is_empty() {
        return Err(ModbusError::Protocol("empty Modbus PDU".to_string()));
    }
    let mut frame = Vec::with_capacity(1 + pdu.len() + 2);
    frame.push(unit_id);
    frame.extend(pdu);
    let crc = crc16_modbus(&frame);
    frame.extend(crc.to_le_bytes());
    Ok(frame)
}

pub fn decode_rtu_frame(frame: &[u8]) -> ModbusResult<ModbusRtuFrame> {
    if frame.len() < 4 {
        return Err(ModbusError::Protocol("short Modbus RTU frame".to_string()));
    }
    let payload_len = frame.len() - 2;
    let expected = u16::from_le_bytes([frame[payload_len], frame[payload_len + 1]]);
    let actual = crc16_modbus(&frame[..payload_len]);
    if expected != actual {
        return Err(ModbusError::Protocol("Modbus RTU CRC mismatch".to_string()));
    }
    Ok(ModbusRtuFrame {
        unit_id: frame[0],
        pdu: frame[1..payload_len].to_vec(),
    })
}

pub fn crc16_modbus(bytes: &[u8]) -> u16 {
    let mut crc = 0xffffu16;
    for byte in bytes {
        crc ^= *byte as u16;
        for _ in 0..8 {
            if crc & 0x0001 != 0 {
                crc = (crc >> 1) ^ 0xa001;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}

fn read_request_pdu(function: u8, address: u16, count: u16) -> Vec<u8> {
    let mut pdu = Vec::with_capacity(5);
    pdu.push(function);
    pdu.extend(address.to_be_bytes());
    pdu.extend(count.to_be_bytes());
    pdu
}

fn write_single_request_pdu(function: u8, address: u16, value: u16) -> Vec<u8> {
    let mut pdu = Vec::with_capacity(5);
    pdu.push(function);
    pdu.extend(address.to_be_bytes());
    pdu.extend(value.to_be_bytes());
    pdu
}

fn verify_response_function(expected: u8, response: &[u8]) -> ModbusResult<()> {
    let Some(&actual) = response.first() else {
        return Err(ModbusError::Protocol(
            "empty Modbus response PDU".to_string(),
        ));
    };
    if actual == expected {
        return Ok(());
    }
    if actual == expected | 0x80 {
        let code = response.get(1).copied().unwrap_or(0);
        return Err(ModbusError::Exception { code });
    }
    Err(ModbusError::FunctionMismatch { expected, actual })
}

fn unpack_bits(bytes: &[u8], count: usize) -> Vec<bool> {
    let mut bits = Vec::with_capacity(count);
    for index in 0..count {
        let byte = bytes.get(index / 8).copied().unwrap_or(0);
        bits.push(byte & (1 << (index % 8)) != 0);
    }
    bits
}

fn read_one_bool(result: ModbusResult<Vec<bool>>, address: u16) -> ModbusResult<bool> {
    result?
        .into_iter()
        .next()
        .ok_or(ModbusError::UnmappedAddress { address })
}

fn read_one_register(result: ModbusResult<Vec<u16>>, address: u16) -> ModbusResult<u16> {
    result?
        .into_iter()
        .next()
        .ok_or(ModbusError::UnmappedAddress { address })
}

fn decode_register(mapping: RegisterMapping, raw: u16) -> f32 {
    let value = match mapping.encoding {
        RegisterEncoding::U16 => raw as f32,
        RegisterEncoding::I16 => i16::from_ne_bytes(raw.to_ne_bytes()) as f32,
    };
    value * mapping.scale + mapping.offset
}

fn encode_register(mapping: RegisterMapping, value: f32) -> u16 {
    let normalized = if mapping.scale == 0.0 {
        0.0
    } else {
        (value - mapping.offset) / mapping.scale
    };
    match mapping.encoding {
        RegisterEncoding::U16 => normalized.round().clamp(0.0, u16::MAX as f32) as u16,
        RegisterEncoding::I16 => {
            let raw = normalized.round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
            u16::from_ne_bytes(raw.to_ne_bytes())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MockModbusClient {
        coils: BTreeMap<u16, bool>,
        discrete_inputs: BTreeMap<u16, bool>,
        holding_registers: BTreeMap<u16, u16>,
        input_registers: BTreeMap<u16, u16>,
        written_coils: Vec<(u8, u16, bool)>,
        written_registers: Vec<(u8, u16, u16)>,
    }

    impl ModbusClient for MockModbusClient {
        fn read_coils(
            &mut self,
            _unit_id: u8,
            address: u16,
            count: u16,
        ) -> ModbusResult<Vec<bool>> {
            Ok((0..count)
                .map(|offset| {
                    self.coils
                        .get(&(address + offset))
                        .copied()
                        .unwrap_or(false)
                })
                .collect())
        }

        fn read_discrete_inputs(
            &mut self,
            _unit_id: u8,
            address: u16,
            count: u16,
        ) -> ModbusResult<Vec<bool>> {
            Ok((0..count)
                .map(|offset| {
                    self.discrete_inputs
                        .get(&(address + offset))
                        .copied()
                        .unwrap_or(false)
                })
                .collect())
        }

        fn read_holding_registers(
            &mut self,
            _unit_id: u8,
            address: u16,
            count: u16,
        ) -> ModbusResult<Vec<u16>> {
            Ok((0..count)
                .map(|offset| {
                    self.holding_registers
                        .get(&(address + offset))
                        .copied()
                        .unwrap_or(0)
                })
                .collect())
        }

        fn read_input_registers(
            &mut self,
            _unit_id: u8,
            address: u16,
            count: u16,
        ) -> ModbusResult<Vec<u16>> {
            Ok((0..count)
                .map(|offset| {
                    self.input_registers
                        .get(&(address + offset))
                        .copied()
                        .unwrap_or(0)
                })
                .collect())
        }

        fn write_single_coil(
            &mut self,
            unit_id: u8,
            address: u16,
            value: bool,
        ) -> ModbusResult<()> {
            self.coils.insert(address, value);
            self.written_coils.push((unit_id, address, value));
            Ok(())
        }

        fn write_single_register(
            &mut self,
            unit_id: u8,
            address: u16,
            value: u16,
        ) -> ModbusResult<()> {
            self.holding_registers.insert(address, value);
            self.written_registers.push((unit_id, address, value));
            Ok(())
        }
    }

    #[test]
    fn tcp_frame_codec_round_trips_mbap_and_pdu() {
        let frame = encode_tcp_frame(0x1234, 7, &[0x03, 0x00, 0x10, 0x00, 0x02])
            .expect("tcp frame should encode");
        assert_eq!(
            frame,
            vec![
                0x12, 0x34, 0x00, 0x00, 0x00, 0x06, 0x07, 0x03, 0x00, 0x10, 0x00, 0x02
            ]
        );

        let decoded = decode_tcp_frame(&frame).expect("tcp frame should decode");
        assert_eq!(decoded.transaction_id, 0x1234);
        assert_eq!(decoded.unit_id, 7);
        assert_eq!(decoded.pdu, vec![0x03, 0x00, 0x10, 0x00, 0x02]);
    }

    #[test]
    fn rtu_frame_codec_appends_and_checks_crc() {
        let frame = encode_rtu_frame(0x11, &[0x03, 0x00, 0x6b, 0x00, 0x03])
            .expect("rtu frame should encode");
        assert_eq!(frame, vec![0x11, 0x03, 0x00, 0x6b, 0x00, 0x03, 0x76, 0x87]);

        let decoded = decode_rtu_frame(&frame).expect("rtu frame should decode");
        assert_eq!(decoded.unit_id, 0x11);
        assert_eq!(decoded.pdu, vec![0x03, 0x00, 0x6b, 0x00, 0x03]);

        let mut bad = frame;
        bad[2] ^= 0x01;
        assert!(matches!(
            decode_rtu_frame(&bad),
            Err(ModbusError::Protocol(message)) if message.contains("CRC")
        ));
    }

    #[test]
    fn modbus_runtime_syncs_inputs_and_flushes_dirty_outputs() {
        let mut client = MockModbusClient::default();
        client.discrete_inputs.insert(10, true);
        client.input_registers.insert(30, 1234);

        let mapping = ModbusMapping::new()
            .map_digital_input(0, BoolArea::DiscreteInput, 10)
            .map_digital_output(1, BoolArea::Coil, 20)
            .map_analog_input(0, RegisterMapping::u16(RegisterArea::InputRegister, 30))
            .map_analog_output(
                1,
                RegisterMapping::i16_scaled(RegisterArea::HoldingRegister, 40, 0.1, 0.0),
            );
        let mut runtime = ModbusRuntime::new(client, 3, mapping);

        runtime.sync_inputs().expect("input sync should succeed");
        assert_eq!(runtime.read_digital_input(DigitalInputId(0)), true);
        assert_eq!(runtime.read_analog_input(AnalogInputId(0)), 1234.0);

        runtime.write_digital_output(DigitalOutputId(1), true);
        runtime.write_analog_output(AnalogOutputId(1), 12.3);
        runtime
            .flush_outputs()
            .expect("output flush should succeed");

        let client = runtime.into_client();
        assert_eq!(client.written_coils, vec![(3, 20, true)]);
        assert_eq!(client.written_registers, vec![(3, 40, 123)]);
    }

    #[test]
    fn modbus_runtime_implements_unified_cyclic_io() {
        let mut client = MockModbusClient::default();
        client.discrete_inputs.insert(10, true);
        client.input_registers.insert(30, 4321);

        let mapping = ModbusMapping::new()
            .map_digital_input(0, BoolArea::DiscreteInput, 10)
            .map_digital_output(1, BoolArea::Coil, 20)
            .map_analog_input(0, RegisterMapping::u16(RegisterArea::InputRegister, 30))
            .map_analog_output(
                1,
                RegisterMapping::i16_scaled(RegisterArea::HoldingRegister, 40, 0.1, 0.0),
            );
        let mut runtime = ModbusRuntime::new(client, 3, mapping);

        runtime.write_digital_output(DigitalOutputId(1), true);
        runtime.write_analog_output(AnalogOutputId(1), 12.3);
        CyclicIo::cycle(&mut runtime).expect("fieldbus cycle should flush and sync");

        assert_eq!(runtime.tick(), Tick(1));
        assert_eq!(runtime.read_digital_input(DigitalInputId(0)), true);
        assert_eq!(runtime.read_analog_input(AnalogInputId(0)), 4321.0);

        let client = runtime.into_client();
        assert_eq!(client.written_coils, vec![(3, 20, true)]);
        assert_eq!(client.written_registers, vec![(3, 40, 123)]);
    }

    #[derive(Default)]
    struct EchoTcpTransport {
        requests: Vec<Vec<u8>>,
    }

    impl ModbusTcpTransport for EchoTcpTransport {
        fn transact(&mut self, request: &[u8]) -> ModbusResult<Vec<u8>> {
            self.requests.push(request.to_vec());
            let frame = decode_tcp_frame(request)?;
            let response_pdu = match frame.pdu[0] {
                0x01 => vec![0x01, 0x01, 0b0000_0101],
                0x06 => frame.pdu,
                other => {
                    return Err(ModbusError::Protocol(format!(
                        "unexpected function {other:#04x}"
                    )));
                }
            };
            encode_tcp_frame(frame.transaction_id, frame.unit_id, &response_pdu)
        }
    }

    #[test]
    fn tcp_client_uses_transport_frames_for_read_and_write() {
        let transport = EchoTcpTransport::default();
        let mut client = ModbusTcpClient::new(transport);

        let bits = client
            .read_coils(2, 0, 3)
            .expect("read coils should parse packed bits");
        assert_eq!(bits, vec![true, false, true]);

        client
            .write_single_register(2, 9, 42)
            .expect("write register should parse echo");

        let transport = client.into_transport();
        assert_eq!(transport.requests.len(), 2);
        assert_eq!(decode_tcp_frame(&transport.requests[0]).unwrap().unit_id, 2);
        assert_eq!(
            decode_tcp_frame(&transport.requests[1]).unwrap().pdu,
            vec![0x06, 0x00, 0x09, 0x00, 0x2a]
        );
    }

    #[test]
    fn exception_response_reports_modbus_exception_code() {
        let err = verify_response_function(0x03, &[0x83, 0x02])
            .expect_err("exception response should fail");
        assert_eq!(err, ModbusError::Exception { code: 0x02 });
    }
}
