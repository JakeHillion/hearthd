//! Commands the engine can invoke on a cluster.
//!
//! This module contains only genuine Matter commands. Attribute writes (such as
//! a thermostat's setpoints and system mode) live in [`super::writes`] and are
//! dispatched through `ToIntegrationMessage::WriteAttributes`.
//!
//! [`super::writes`]: crate::matter::writes

use serde::Deserialize;
use serde::Serialize;

use super::clusters::CLUSTER_ID_COLOR_CONTROL;
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

/// Color Control cluster (0x0300) commands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ColorControlCommand {
    /// Command 0x00 `MoveToHue`.
    MoveToHue {
        hue: u8,
        transition_time: Option<u16>,
    },

    /// Command 0x01 `MoveToSaturation`.
    MoveToSaturation {
        saturation: u8,
        transition_time: Option<u16>,
    },

    /// Command 0x06 `MoveToHueAndSaturation`.
    MoveToHueAndSaturation {
        hue: u8,
        saturation: u8,
        transition_time: Option<u16>,
    },

    /// Command 0x07 `MoveToColor`.
    MoveToColor {
        x: u16,
        y: u16,
        transition_time: Option<u16>,
    },

    /// Command 0x0A `MoveToColorTemperature`.
    MoveToColorTemperature {
        color_temperature_mireds: u16,
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
///
/// The writable thermostat attributes (system mode and setpoints) are modelled
/// as [`ClusterWrite::Thermostat`](crate::matter::writes::ClusterWrite::Thermostat)
/// rather than as commands, because Matter specifies them as attribute writes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ThermostatCommand {
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

    /// Command 0x04 `Previous`. Distinct from `SkipBackward` (0x09), which
    /// steps backwards within the current track by an offset.
    Previous,

    /// Command 0x05 `Next`. Distinct from `SkipForward` (0x08), which steps
    /// forwards within the current track by an offset.
    Next,

    /// Command 0x06 `Rewind`.
    Rewind,

    /// Command 0x07 `FastForward`.
    FastForward,
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
    ColorControl(ColorControlCommand),
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
            ClusterCommand::ColorControl(_) => CLUSTER_ID_COLOR_CONTROL,
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
