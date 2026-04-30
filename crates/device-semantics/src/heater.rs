#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaterAction {
    HeatTo,
    HoldTemperature,
    StopHeat,
    ResetFault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaterResult {
    Done,
    Timeout,
    Overtemperature,
    SensorFault,
    ThermalRunaway,
    SafetyFault,
}

pub const FAMILY: &str = "heater";
pub const POWER_PORT: &str = "power";
pub const TEMPERATURE_PORT: &str = "temperature";
pub const FAULT_PORT: &str = "fault";
