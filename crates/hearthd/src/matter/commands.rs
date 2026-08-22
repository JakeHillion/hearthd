//! Commands the engine can invoke on a cluster.
//!
//! # Attribute writes
//!
//! Matter drives a good deal of device behaviour by *writing attributes*
//! rather than by invoking commands: a thermostat's setpoints and system mode,
//! a fan's speed, and a panel's temperature-display unit are all attribute
//! writes in the specification, and only a handful of genuine commands exist
//! alongside them.
//!
//! hearthd's engine has no attribute-write path — `ToIntegrationMessage`
//! carries `InvokeCommand` and nothing else. Rather than grow one in the same
//! change that adds these clusters, the writes are modelled here as commands
//! whose doc comment names the attribute they stand in for. Variants that
//! correspond to a real Matter command say so explicitly.
//!
//! If an attribute-write path is added later, the `Set*` variants below are
//! what should migrate onto it.

use serde::Deserialize;
use serde::Serialize;

use super::clusters::CLUSTER_ID_DEHUMIDIFICATION_CONTROL;
use super::clusters::CLUSTER_ID_FAN_CONTROL;
use super::clusters::CLUSTER_ID_LEVEL_CONTROL;
use super::clusters::CLUSTER_ID_MEDIA_INPUT;
use super::clusters::CLUSTER_ID_MEDIA_PLAYBACK;
use super::clusters::CLUSTER_ID_MODE_SELECT;
use super::clusters::CLUSTER_ID_ON_OFF;
use super::clusters::CLUSTER_ID_THERMOSTAT;
use super::clusters::CLUSTER_ID_THERMOSTAT_USER_INTERFACE_CONFIGURATION;
use super::clusters::FanMode;
use super::clusters::SystemMode;
use super::clusters::TemperatureDisplayMode;

/// OnOff cluster (0x0006) commands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OnOffCommand {
    /// Command 0x00.
    Off,
    /// Command 0x01.
    On,
    /// Command 0x02.
    Toggle,
}

/// LevelControl cluster (0x0008) commands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LevelControlCommand {
    /// Command 0x00 `MoveToLevel`.
    MoveToLevel {
        level: u8,
        transition_time: Option<u16>,
    },
}

/// Which setpoint(s) a `SetpointRaiseLower` applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetpointMode {
    Heat = 0,
    Cool = 1,
    Both = 2,
}

/// Thermostat cluster (0x0201) commands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ThermostatCommand {
    /// Write to attribute 0x001C `SystemMode`.
    SetSystemMode { mode: SystemMode },

    /// Write to attribute 0x0011 `OccupiedCoolingSetpoint`, in hundredths of a
    /// degree Celsius. In `SystemMode::Auto` this is the range's upper bound.
    SetOccupiedCoolingSetpoint { centi_celsius: i16 },

    /// Write to attribute 0x0012 `OccupiedHeatingSetpoint`, in hundredths of a
    /// degree Celsius. In `SystemMode::Auto` this is the range's lower bound.
    SetOccupiedHeatingSetpoint { centi_celsius: i16 },

    /// Command 0x00 `SetpointRaiseLower` — a real Matter command. `amount` is
    /// a relative adjustment in tenths of a degree Celsius, so it requires a
    /// known current setpoint to apply against.
    SetpointRaiseLower { mode: SetpointMode, amount: i8 },
}

/// FanControl cluster (0x0202) commands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FanControlCommand {
    /// Write to attribute 0x0000 `FanMode`.
    SetFanMode { mode: FanMode },

    /// Write to attribute 0x0002 `PercentSetting` (0-100).
    SetPercentSetting { percent: u8 },

    /// Write to attribute 0x0005 `SpeedSetting` (0 = off, up to `SpeedMax`).
    SetSpeedSetting { speed: u8 },
}

/// DehumidificationControl cluster (0x0203) commands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DehumidificationControlCommand {
    /// Write to attribute 0x0002 `RHDehumidificationSetPoint` (whole percent).
    SetRhDehumidificationSetpoint { percent: u8 },
}

/// ThermostatUserInterfaceConfiguration cluster (0x0204) commands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ThermostatUserInterfaceConfigurationCommand {
    /// Write to attribute 0x0000 `TemperatureDisplayMode`.
    SetTemperatureDisplayMode { mode: TemperatureDisplayMode },
}

/// ModeSelect cluster (0x0050) commands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ModeSelectCommand {
    /// Command 0x00 `ChangeToMode` — a real Matter command. `new_mode` must be
    /// one of the instance's `SupportedModes`.
    ChangeToMode { new_mode: u8 },
}

/// Media Playback cluster (0x0506) commands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MediaPlaybackCommand {
    /// Command 0x00 `Play`.
    Play,

    /// Command 0x01 `Pause`.
    Pause,

    /// Command 0x02 `Stop`.
    Stop,

    /// Command 0x03 `FastForward`.
    FastForward,

    /// Command 0x04 `Rewind`.
    Rewind,

    /// Command 0x05 `SkipForward` / `Next`.
    Next,

    /// Command 0x06 `SkipBackward` / `Previous`.
    Previous,
}

/// Media Input cluster (0x0507) commands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MediaInputCommand {
    /// Command 0x00 `SelectInput`.
    SelectInput { index: u8 },
}

/// A command to invoke on a cluster. JSON representation:
///   `{"cluster": "OnOff", "command": "On"}`
///   `{"cluster": "LevelControl", "command": {"MoveToLevel": {"level": 200, "transition_time": null}}}`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cluster", content = "command")]
pub enum ClusterCommand {
    OnOff(OnOffCommand),
    LevelControl(LevelControlCommand),
    Thermostat(ThermostatCommand),
    FanControl(FanControlCommand),
    DehumidificationControl(DehumidificationControlCommand),
    ThermostatUserInterfaceConfiguration(ThermostatUserInterfaceConfigurationCommand),
    ModeSelect(ModeSelectCommand),
    MediaPlayback(MediaPlaybackCommand),
    MediaInput(MediaInputCommand),
}

impl ClusterCommand {
    /// Cluster this command targets.
    pub fn cluster_id(&self) -> u32 {
        match self {
            ClusterCommand::OnOff(_) => CLUSTER_ID_ON_OFF,
            ClusterCommand::LevelControl(_) => CLUSTER_ID_LEVEL_CONTROL,
            ClusterCommand::Thermostat(_) => CLUSTER_ID_THERMOSTAT,
            ClusterCommand::FanControl(_) => CLUSTER_ID_FAN_CONTROL,
            ClusterCommand::DehumidificationControl(_) => CLUSTER_ID_DEHUMIDIFICATION_CONTROL,
            ClusterCommand::ThermostatUserInterfaceConfiguration(_) => {
                CLUSTER_ID_THERMOSTAT_USER_INTERFACE_CONFIGURATION
            }
            ClusterCommand::ModeSelect(_) => CLUSTER_ID_MODE_SELECT,
            ClusterCommand::MediaPlayback(_) => CLUSTER_ID_MEDIA_PLAYBACK,
            ClusterCommand::MediaInput(_) => CLUSTER_ID_MEDIA_INPUT,
        }
    }
}
