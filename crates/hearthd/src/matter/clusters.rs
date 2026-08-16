//! Cluster attribute types from the Matter Application Cluster Specification.
//!
//! Each struct models the attributes hearthd actually populates, not the
//! cluster's full attribute set. Attribute IDs are recorded in doc comments so
//! the mapping back to the specification stays checkable.
//!
//! Optional attributes are `Option`-shaped and stay `None` until a device
//! reports a value. Integrations must not invent a plausible-looking default:
//! a fabricated reading that disagrees with the hardware is worse than no
//! reading.

use serde::Deserialize;
use serde::Serialize;

// Cluster IDs from the Matter Application Cluster Specification.
pub const CLUSTER_ID_ON_OFF: u32 = 0x0006;
pub const CLUSTER_ID_LEVEL_CONTROL: u32 = 0x0008;
pub const CLUSTER_ID_POWER_SOURCE: u32 = 0x002F;
pub const CLUSTER_ID_BOOLEAN_STATE: u32 = 0x0045;
pub const CLUSTER_ID_MODE_SELECT: u32 = 0x0050;
pub const CLUSTER_ID_ELECTRICAL_POWER_MEASUREMENT: u32 = 0x0090;
pub const CLUSTER_ID_THERMOSTAT: u32 = 0x0201;
pub const CLUSTER_ID_FAN_CONTROL: u32 = 0x0202;
pub const CLUSTER_ID_DEHUMIDIFICATION_CONTROL: u32 = 0x0203;
pub const CLUSTER_ID_THERMOSTAT_USER_INTERFACE_CONFIGURATION: u32 = 0x0204;
pub const CLUSTER_ID_TEMPERATURE_MEASUREMENT: u32 = 0x0402;
pub const CLUSTER_ID_RELATIVE_HUMIDITY_MEASUREMENT: u32 = 0x0405;
pub const CLUSTER_ID_OCCUPANCY_SENSING: u32 = 0x0406;

pub const CLUSTER_NAME_ON_OFF: &str = "OnOff";
pub const CLUSTER_NAME_LEVEL_CONTROL: &str = "LevelControl";
pub const CLUSTER_NAME_POWER_SOURCE: &str = "PowerSource";
pub const CLUSTER_NAME_BOOLEAN_STATE: &str = "BooleanState";
pub const CLUSTER_NAME_MODE_SELECT: &str = "ModeSelect";
pub const CLUSTER_NAME_ELECTRICAL_POWER_MEASUREMENT: &str = "ElectricalPowerMeasurement";
pub const CLUSTER_NAME_THERMOSTAT: &str = "Thermostat";
pub const CLUSTER_NAME_FAN_CONTROL: &str = "FanControl";
pub const CLUSTER_NAME_DEHUMIDIFICATION_CONTROL: &str = "DehumidificationControl";
pub const CLUSTER_NAME_THERMOSTAT_USER_INTERFACE_CONFIGURATION: &str =
    "ThermostatUserInterfaceConfiguration";
pub const CLUSTER_NAME_TEMPERATURE_MEASUREMENT: &str = "TemperatureMeasurement";
pub const CLUSTER_NAME_RELATIVE_HUMIDITY_MEASUREMENT: &str = "RelativeHumidityMeasurement";
pub const CLUSTER_NAME_OCCUPANCY_SENSING: &str = "OccupancySensing";

/// On/Off cluster (0x0006).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, facet::Facet)]
pub struct OnOffCluster {
    /// Attribute 0x0000 `OnOff`.
    pub on_off: bool,
}

/// Level Control cluster (0x0008).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, facet::Facet)]
pub struct LevelControlCluster {
    /// Attribute 0x0000 `CurrentLevel` (0-254, null if unknown).
    pub current_level: Option<u8>,
}

/// Temperature Measurement cluster (0x0402).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, facet::Facet)]
pub struct TemperatureMeasurementCluster {
    /// Attribute 0x0000 `MeasuredValue` (int16, hundredths of a degree
    /// Celsius, null if unknown).
    pub measured_value: Option<i16>,
}

/// Relative Humidity Measurement cluster (0x0405).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, facet::Facet)]
pub struct RelativeHumidityMeasurementCluster {
    /// Attribute 0x0000 `MeasuredValue` (uint16, hundredths of a percent,
    /// null if unknown).
    pub measured_value: Option<u16>,
}

/// Occupancy Sensing cluster (0x0406).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, facet::Facet)]
pub struct OccupancySensingCluster {
    /// Attribute 0x0000 `Occupancy` (bit 0 = occupied).
    pub occupancy: bool,
}

/// Boolean State cluster (0x0045).
///
/// The specification's general-purpose boolean. hearthd uses it for the
/// read-only "is this happening right now" signals that have no cluster of
/// their own, such as a running condensate drain cycle or an attached charger.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, facet::Facet)]
pub struct BooleanStateCluster {
    /// Attribute 0x0000 `StateValue`.
    pub state_value: bool,
}

/// Thermostat `SystemMode` (attribute 0x001C) values.
///
/// Values are the specification's, not sequential: 2 is unused and the gap is
/// intentional.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, facet::Facet)]
#[repr(u8)]
pub enum SystemMode {
    Off = 0,
    Auto = 1,
    Cool = 3,
    Heat = 4,
    EmergencyHeat = 5,
    Precooling = 6,
    FanOnly = 7,
    Dry = 8,
    Sleep = 9,
}

/// Thermostat `ControlSequenceOfOperation` (attribute 0x001B) values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, facet::Facet)]
#[repr(u8)]
pub enum ControlSequenceOfOperation {
    CoolingOnly = 0,
    CoolingWithReheat = 1,
    HeatingOnly = 2,
    HeatingWithReheat = 3,
    #[default]
    CoolingAndHeating = 4,
    CoolingAndHeatingWithReheat = 5,
}

/// Thermostat cluster (0x0201).
///
/// Setpoints are int16 in hundredths of a degree Celsius, matching
/// `TemperatureMeasurementCluster::measured_value`.
///
/// In `SystemMode::Auto` the two occupied setpoints bound a range rather than
/// naming a single target: `occupied_cooling_setpoint` is the upper bound and
/// `occupied_heating_setpoint` the lower.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, facet::Facet)]
pub struct ThermostatCluster {
    /// Attribute 0x0000 `LocalTemperature` (null if unknown).
    pub local_temperature: Option<i16>,

    /// Attribute 0x0011 `OccupiedCoolingSetpoint`.
    pub occupied_cooling_setpoint: Option<i16>,

    /// Attribute 0x0012 `OccupiedHeatingSetpoint`.
    pub occupied_heating_setpoint: Option<i16>,

    /// Attribute 0x001B `ControlSequenceOfOperation`.
    pub control_sequence_of_operation: ControlSequenceOfOperation,

    /// Attribute 0x001C `SystemMode`. `None` until the device reports one.
    pub system_mode: Option<SystemMode>,

    /// Attribute 0x0005 `AbsMinHeatSetpointLimit`.
    pub abs_min_heat_setpoint_limit: Option<i16>,

    /// Attribute 0x0006 `AbsMaxHeatSetpointLimit`.
    pub abs_max_heat_setpoint_limit: Option<i16>,

    /// Attribute 0x0007 `AbsMinCoolSetpointLimit`.
    pub abs_min_cool_setpoint_limit: Option<i16>,

    /// Attribute 0x0008 `AbsMaxCoolSetpointLimit`.
    pub abs_max_cool_setpoint_limit: Option<i16>,
}

/// Fan Control `FanMode` (attribute 0x0000) values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, facet::Facet)]
#[repr(u8)]
pub enum FanMode {
    Off = 0,
    Low = 1,
    Medium = 2,
    High = 3,
    On = 4,
    Auto = 5,
    Smart = 6,
}

/// Fan Control `FanModeSequence` (attribute 0x0001) values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, facet::Facet)]
#[repr(u8)]
pub enum FanModeSequence {
    #[default]
    OffLowMedHigh = 0,
    OffLowHigh = 1,
    OffLowMedHighAuto = 2,
    OffLowHighAuto = 3,
    OffHighAuto = 4,
    OffHigh = 5,
}

/// Fan Control cluster (0x0202).
///
/// A fan with more discrete steps than `FanMode`'s low/medium/high carries the
/// real setting in `speed_setting` (1..=`speed_max`); `fan_mode` is then a
/// coarse summary of it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, facet::Facet)]
pub struct FanControlCluster {
    /// Attribute 0x0000 `FanMode`.
    pub fan_mode: Option<FanMode>,

    /// Attribute 0x0001 `FanModeSequence`.
    pub fan_mode_sequence: FanModeSequence,

    /// Attribute 0x0002 `PercentSetting` (0-100, null if unknown).
    pub percent_setting: Option<u8>,

    /// Attribute 0x0003 `PercentCurrent` (0-100).
    pub percent_current: Option<u8>,

    /// Attribute 0x0004 `SpeedMax` (highest valid `speed_setting`).
    pub speed_max: Option<u8>,

    /// Attribute 0x0005 `SpeedSetting` (0 = off, 1..=`speed_max`).
    pub speed_setting: Option<u8>,

    /// Attribute 0x0006 `SpeedCurrent`.
    pub speed_current: Option<u8>,
}

/// Dehumidification Control cluster (0x0203).
///
/// Carried over from the Zigbee Cluster Library and marked provisional in
/// Matter, so controller support is thin. hearthd uses it anyway: it is the
/// specification's own home for a relative-humidity setpoint, and hearthd does
/// not speak Matter on the wire, so thin controller support costs us nothing
/// that a manufacturer-specific cluster would not also cost.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, facet::Facet)]
pub struct DehumidificationControlCluster {
    /// Attribute 0x0000 `RelativeHumidity` (whole percent, null if unknown).
    pub relative_humidity: Option<u8>,

    /// Attribute 0x0002 `RHDehumidificationSetPoint` (whole percent).
    pub rh_dehumidification_setpoint: Option<u8>,
}

/// Thermostat User Interface Configuration `TemperatureDisplayMode`
/// (attribute 0x0000) values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, facet::Facet)]
#[repr(u8)]
pub enum TemperatureDisplayMode {
    Celsius = 0,
    Fahrenheit = 1,
}

/// Thermostat User Interface Configuration cluster (0x0204).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, facet::Facet)]
pub struct ThermostatUserInterfaceConfigurationCluster {
    /// Attribute 0x0000 `TemperatureDisplayMode`. Affects the physical panel
    /// only; every value hearthd exchanges stays in Celsius.
    pub temperature_display_mode: Option<TemperatureDisplayMode>,
}

/// Power Source `Status` (attribute 0x0000) values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, facet::Facet)]
#[repr(u8)]
pub enum PowerSourceStatus {
    #[default]
    Unspecified = 0,
    Active = 1,
    Standby = 2,
    Unavailable = 3,
}

/// Power Source `BatChargeLevel` (attribute 0x000E) values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, facet::Facet)]
#[repr(u8)]
pub enum BatChargeLevel {
    Ok = 0,
    Warning = 1,
    Critical = 2,
}

/// Power Source `BatChargeState` (attribute 0x001A) values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, facet::Facet)]
#[repr(u8)]
pub enum BatChargeState {
    Unknown = 0,
    IsCharging = 1,
    IsAtFullCharge = 2,
    IsNotCharging = 3,
}

/// Power Source cluster (0x002F).
///
/// Note the specification's two unusual units: `bat_percent_remaining` is in
/// *half* percent (0-200), and both time attributes are in seconds even where
/// the underlying hardware reports minutes.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, facet::Facet)]
pub struct PowerSourceCluster {
    /// Attribute 0x0000 `Status`.
    pub status: PowerSourceStatus,

    /// Attribute 0x0001 `Order` (preference when several sources exist).
    pub order: u8,

    /// Attribute 0x0002 `Description`.
    pub description: String,

    /// Attribute 0x000B `BatVoltage` (millivolts).
    pub bat_voltage: Option<u32>,

    /// Attribute 0x000C `BatPercentRemaining` (half percent, 0-200).
    pub bat_percent_remaining: Option<u8>,

    /// Attribute 0x000D `BatTimeRemaining` (seconds).
    pub bat_time_remaining: Option<u32>,

    /// Attribute 0x000E `BatChargeLevel`.
    pub bat_charge_level: Option<BatChargeLevel>,

    /// Attribute 0x001A `BatChargeState`.
    pub bat_charge_state: Option<BatChargeState>,

    /// Attribute 0x001B `BatTimeToFullCharge` (seconds).
    pub bat_time_to_full_charge: Option<u32>,
}

/// Electrical Power Measurement `PowerMode` (attribute 0x0000) values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, facet::Facet)]
#[repr(u8)]
pub enum PowerMode {
    #[default]
    Unknown = 0,
    Dc = 1,
    Ac = 2,
}

/// Electrical Power Measurement cluster (0x0090).
///
/// Every measurement is an int64 in milli-units, so a watt is 1000 and a sign
/// carries direction where the source reports one.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, facet::Facet)]
pub struct ElectricalPowerMeasurementCluster {
    /// Attribute 0x0000 `PowerMode`.
    pub power_mode: PowerMode,

    /// Attribute 0x0004 `Voltage` (millivolts).
    pub voltage: Option<i64>,

    /// Attribute 0x0005 `ActivePower` (milliwatts).
    pub active_power: Option<i64>,

    /// Attribute 0x0006 `ActiveCurrent` (milliamps).
    pub active_current: Option<i64>,

    /// Attribute 0x000C `Frequency` (millihertz).
    pub frequency: Option<i64>,
}

/// One entry of Mode Select's `SupportedModes` (attribute 0x0002).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, facet::Facet)]
pub struct ModeOption {
    /// Human-readable name for the mode.
    pub label: String,

    /// Value `current_mode` takes when this option is selected.
    pub mode: u8,
}

/// Mode Select cluster (0x0050).
///
/// The specification's generic "pick one of these named options" cluster, for
/// device features that are a closed set of choices with no cluster of their
/// own.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, facet::Facet)]
pub struct ModeSelectCluster {
    /// Attribute 0x0000 `Description` (what this instance selects).
    pub description: String,

    /// Attribute 0x0002 `SupportedModes`.
    pub supported_modes: Vec<ModeOption>,

    /// Attribute 0x0003 `CurrentMode`. `None` until the device reports one, or
    /// when it reports a value absent from `supported_modes`.
    pub current_mode: Option<u8>,
}
