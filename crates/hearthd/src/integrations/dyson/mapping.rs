//! Map between Dyson state / commands and `hearthd`'s Matter-shaped model.
//!
//! The TP07 exposes its main fan + sensor clusters on endpoint 1, and one
//! generic `ModeSelectCluster` per binary toggle (oscillation, night mode,
//! continuous monitoring) on its own endpoint so Matter's one-cluster-per-
//! endpoint rule holds. The sleep timer is a generic `CountdownTimerCluster`.

use std::collections::HashMap;

use serde_json::json;

use super::config::DeviceConfig;
use super::state::PureCoolState;
use crate::matter::AirQualityCluster;
use crate::matter::AirflowDirection;
use crate::matter::Cluster;
use crate::matter::ClusterCommand;
use crate::matter::CountdownTimerCluster;
use crate::matter::CountdownTimerCommand;
use crate::matter::Endpoint;
use crate::matter::EndpointId;
use crate::matter::FanControlCluster;
use crate::matter::FanControlCommand;
use crate::matter::FanMode;
use crate::matter::FanModeSequence;
use crate::matter::ModeOption;
use crate::matter::ModeSelectCluster;
use crate::matter::ModeSelectCommand;
use crate::matter::Node;
use crate::matter::OnOffCluster;
use crate::matter::OnOffCommand;
use crate::matter::PercentageMeasurementCluster;
use crate::matter::RelativeHumidityMeasurementCluster;
use crate::matter::TemperatureMeasurementCluster;

/// Endpoint carrying the fan and its sensors.
pub const EP_MAIN: EndpointId = 1;
/// Endpoint carrying the oscillation toggle.
pub const EP_OSCILLATION: EndpointId = 2;
/// Endpoint carrying the night-mode toggle.
pub const EP_NIGHT_MODE: EndpointId = 3;
/// Endpoint carrying the continuous-monitoring toggle.
pub const EP_MONITORING: EndpointId = 4;
/// Endpoint carrying the sleep timer.
pub const EP_SLEEP_TIMER: EndpointId = 5;

const MODE_OFF: u8 = 0;
const MODE_ON: u8 = 1;

fn toggle_cluster(on: bool) -> ModeSelectCluster {
    ModeSelectCluster {
        description: String::new(),
        supported_modes: vec![
            ModeOption {
                label: "Off".to_string(),
                mode: MODE_OFF,
            },
            ModeOption {
                label: "On".to_string(),
                mode: MODE_ON,
            },
        ],
        current_mode: Some(if on { MODE_ON } else { MODE_OFF }),
    }
}

fn main_endpoint(state: &PureCoolState) -> Endpoint {
    Endpoint::from_clusters([
        Cluster::OnOff(OnOffCluster {
            on_off: state.fan_power.unwrap_or(false),
        }),
        Cluster::FanControl(FanControlCluster {
            fan_mode: fan_mode(state),
            fan_mode_sequence: FanModeSequence::OffLowMedHigh,
            percent_setting: percent_setting(state),
            percent_current: percent_setting(state),
            speed_max: Some(10),
            speed_setting: state.fan_speed,
            speed_current: state.fan_speed,
            airflow_direction: state.front_airflow.map(airflow_direction),
        }),
        Cluster::TemperatureMeasurement(TemperatureMeasurementCluster {
            measured_value: state
                .temperature_kelvin
                .map(|k| ((k - 273.15) * 100.0) as i16),
        }),
        Cluster::RelativeHumidityMeasurement(RelativeHumidityMeasurementCluster {
            measured_value: state.humidity_percent.map(|h| h as u16 * 100),
        }),
        Cluster::AirQuality(AirQualityCluster {
            pm2_5: state.pm2_5,
            pm10: state.pm10,
            no2: state.no2,
            voc: state.voc,
        }),
        Cluster::PercentageMeasurement(PercentageMeasurementCluster {
            measured_value: state.filter_life,
        }),
    ])
}

fn fan_mode(state: &PureCoolState) -> Option<FanMode> {
    if state.auto_mode.unwrap_or(false) {
        Some(FanMode::Auto)
    } else if state.fan_power.unwrap_or(false) {
        Some(if state.fan_speed == Some(0) {
            FanMode::Off
        } else {
            FanMode::On
        })
    } else {
        Some(FanMode::Off)
    }
}

fn percent_setting(state: &PureCoolState) -> Option<u8> {
    if state.auto_mode.unwrap_or(false) {
        None
    } else {
        state.fan_speed.map(|speed| speed.saturating_mul(10))
    }
}

fn airflow_direction(front: bool) -> AirflowDirection {
    if front {
        AirflowDirection::Forward
    } else {
        AirflowDirection::Reverse
    }
}

/// Build the full endpoint map reflecting a state snapshot.
pub fn build_endpoints(state: &PureCoolState) -> HashMap<EndpointId, Endpoint> {
    let mut endpoints = HashMap::new();
    endpoints.insert(EP_MAIN, main_endpoint(state));
    endpoints.insert(
        EP_OSCILLATION,
        Endpoint::from_clusters([Cluster::ModeSelect(toggle_cluster(
            state.oscillation.unwrap_or(false),
        ))]),
    );
    endpoints.insert(
        EP_NIGHT_MODE,
        Endpoint::from_clusters([Cluster::ModeSelect(toggle_cluster(
            state.night_mode.unwrap_or(false),
        ))]),
    );
    endpoints.insert(
        EP_MONITORING,
        Endpoint::from_clusters([Cluster::ModeSelect(toggle_cluster(
            state.continuous_monitoring.unwrap_or(false),
        ))]),
    );
    endpoints.insert(
        EP_SLEEP_TIMER,
        Endpoint::from_clusters([Cluster::CountdownTimer(CountdownTimerCluster {
            seconds_remaining: state.sleep_timer.map(|minutes| minutes as u32 * 60),
        })]),
    );
    endpoints
}

/// Build the initial `Node` for a declared device (all clusters at defaults).
pub fn node_for_device(name: &str, device: &DeviceConfig) -> Node {
    Node {
        entity_id: format!("fan.{}", name),
        integration: "dyson".to_string(),
        name: device.name.clone().or_else(|| Some(name.to_string())),
        endpoints: build_endpoints(&PureCoolState::default()),
    }
}

/// Translate an engine command (and the endpoint it targets) into a Dyson
/// `STATE-SET` data map.
pub fn command_to_state_set_data(
    endpoint_id: EndpointId,
    command: &ClusterCommand,
) -> Option<HashMap<String, String>> {
    let mut data = HashMap::new();
    match command {
        ClusterCommand::OnOff(cmd) => match cmd {
            OnOffCommand::On => {
                data.insert("fpwr".to_string(), "ON".to_string());
            }
            OnOffCommand::Off => {
                data.insert("fpwr".to_string(), "OFF".to_string());
            }
            OnOffCommand::Toggle => {
                // Toggle is not natively supported by Dyson; caller must know current state.
                return None;
            }
        },
        ClusterCommand::FanControl(cmd) => match cmd {
            FanControlCommand::SetFanMode { mode } => match mode {
                FanMode::Off => {
                    data.insert("fpwr".to_string(), "OFF".to_string());
                }
                FanMode::Auto => {
                    data.insert("auto".to_string(), "ON".to_string());
                }
                _ => {
                    data.insert("auto".to_string(), "OFF".to_string());
                    // Keep current speed if any, otherwise set a default.
                    data.insert("fnsp".to_string(), "0005".to_string());
                }
            },
            FanControlCommand::SetPercentSetting { percent } => {
                let speed = (*percent / 10).clamp(1, 10);
                data.insert("auto".to_string(), "OFF".to_string());
                data.insert("fnsp".to_string(), format!("{:04}", speed));
            }
            FanControlCommand::SetSpeedSetting { speed } => {
                let speed = (*speed).clamp(0, 10);
                data.insert("auto".to_string(), "OFF".to_string());
                data.insert("fnsp".to_string(), format!("{:04}", speed));
            }
            FanControlCommand::SetAirflowDirection { direction } => {
                data.insert(
                    "fdir".to_string(),
                    match direction {
                        AirflowDirection::Forward => "ON".to_string(),
                        AirflowDirection::Reverse => "OFF".to_string(),
                    },
                );
            }
        },
        ClusterCommand::ModeSelect(ModeSelectCommand::ChangeToMode { new_mode })
            if endpoint_id == EP_OSCILLATION =>
        {
            data.insert(
                "oson".to_string(),
                if *new_mode == MODE_ON {
                    "ON".to_string()
                } else {
                    "OIOF".to_string()
                },
            );
        }
        ClusterCommand::ModeSelect(ModeSelectCommand::ChangeToMode { new_mode })
            if endpoint_id == EP_NIGHT_MODE =>
        {
            data.insert(
                "nmod".to_string(),
                if *new_mode == MODE_ON {
                    "ON".to_string()
                } else {
                    "OFF".to_string()
                },
            );
        }
        ClusterCommand::ModeSelect(ModeSelectCommand::ChangeToMode { new_mode })
            if endpoint_id == EP_MONITORING =>
        {
            data.insert(
                "rhtm".to_string(),
                if *new_mode == MODE_ON {
                    "ON".to_string()
                } else {
                    "OFF".to_string()
                },
            );
        }
        ClusterCommand::CountdownTimer(CountdownTimerCommand::SetCountdown { seconds }) => {
            if *seconds == 0 {
                data.insert("sltm".to_string(), "OFF".to_string());
            } else {
                let minutes = (seconds / 60).clamp(1, 540);
                data.insert("sltm".to_string(), format!("{:04}", minutes));
            }
        }
        _ => return None,
    }
    Some(data)
}

/// Build a `STATE-SET` MQTT payload from a command.
pub fn state_set_payload(
    endpoint_id: EndpointId,
    command: &ClusterCommand,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let data = command_to_state_set_data(endpoint_id, command)?;
    let data_json: serde_json::Map<String, serde_json::Value> =
        data.into_iter().map(|(k, v)| (k, json!(v))).collect();

    let mut payload = serde_json::Map::new();
    payload.insert("msg".to_string(), json!("STATE-SET"));
    payload.insert("mode-reason".to_string(), json!("LAPP"));
    payload.insert("data".to_string(), serde_json::Value::Object(data_json));
    Some(payload)
}
