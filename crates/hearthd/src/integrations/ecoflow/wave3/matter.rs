//! Translating between the Wave 3's own vocabulary and hearthd's Matter data
//! model.
//!
//! This file is the integration's boundary: below it everything speaks
//! EcoFlow, above it everything speaks Matter. Nothing here is derived from
//! the reverse-engineering work — the device semantics it consumes are in
//! `super::semantics`, and the choice of clusters, endpoint layout and unit
//! conversions is hearthd's own.
//!
//! # Endpoint layout
//!
//! Matter identifies a cluster instance by (endpoint, cluster ID), so a device
//! reporting six temperatures needs six endpoints. The Wave 3's feature set is
//! fixed and known ahead of time, so the layout is static: every endpoint is
//! always present, and an attribute the device has not reported is null rather
//! than the endpoint being absent. The exception is Boolean State, which has
//! no null — those clusters appear only once the device has reported them.
//!
//! | Endpoint | Purpose |
//! | --- | --- |
//! | 1 | the air conditioner itself |
//! | 2 | internal battery |
//! | 3-7 | outlet air, outdoor, condenser, evaporator, compressor discharge |
//! | 8 | condensate drainage |
//! | 9 | beeper |
//! | 10 | panel backlight |
//! | 11 | pet care |
//! | 20-23 | power: AC input, PV, DC port, battery |
//!
//! An earlier layout also carried aggregate input/output totals, an AC output,
//! USB-A and USB-C, and a return-air temperature. A real unit sends none of
//! the fields behind them, so they were endpoints that could only ever read
//! null; `super::fields` records which field numbers those were and why they
//! went.
//!
//! # Data the device reports that hearthd does not surface
//!
//! Several readings have no standard Matter cluster and hearthd does not
//! define manufacturer-specific ones, so they stop at the protocol layer:
//! condensate tank level, battery state of health and cell-level detail, the
//! SoC charge limits, the screen and standby timeouts, the auto-off countdown,
//! the pet-care temperature threshold, and all error codes, BMS bitfields and
//! firmware versions. Their field numbers are recorded in `super::fields` so
//! the knowledge survives; adding any of them means adding a
//! manufacturer-specific cluster first.

use std::collections::HashMap;

use super::codec::ConfigWrite;
use super::semantics;
use super::semantics::OperatingMode;
use super::semantics::Preset;
use super::semantics::UserTempUnit;
use super::state::DeviceState;
use crate::matter::BooleanStateCluster;
use crate::matter::CLUSTER_ID_THERMOSTAT;
use crate::matter::Cluster;
use crate::matter::ClusterCommand;
use crate::matter::ClusterWrite;
use crate::matter::ControlSequenceOfOperation;
use crate::matter::DehumidificationControlCluster;
use crate::matter::DehumidificationControlCommand;
use crate::matter::ElectricalPowerMeasurementCluster;
use crate::matter::Endpoint;
use crate::matter::EndpointId;
use crate::matter::FanControlCluster;
use crate::matter::FanControlCommand;
use crate::matter::FanMode;
use crate::matter::FanModeSequence;
use crate::matter::LevelControlCluster;
use crate::matter::LevelControlCommand;
use crate::matter::ModeOption;
use crate::matter::ModeSelectCluster;
use crate::matter::ModeSelectCommand;
use crate::matter::OnOffCluster;
use crate::matter::OnOffCommand;
use crate::matter::PowerMode;
use crate::matter::PowerSourceCluster;
use crate::matter::PowerSourceStatus;
use crate::matter::RelativeHumidityMeasurementCluster;
use crate::matter::SetpointMode;
use crate::matter::SystemMode;
use crate::matter::TemperatureDisplayMode;
use crate::matter::TemperatureMeasurementCluster;
use crate::matter::ThermostatCluster;
use crate::matter::ThermostatCommand;
use crate::matter::ThermostatUserInterfaceConfigurationCluster;
use crate::matter::ThermostatUserInterfaceConfigurationCommand;

pub const EP_AIR_CONDITIONER: EndpointId = 1;
pub const EP_BATTERY: EndpointId = 2;
pub const EP_TEMP_OUTLET_AIR: EndpointId = 3;
pub const EP_TEMP_OUTDOOR: EndpointId = 4;
pub const EP_TEMP_CONDENSER: EndpointId = 5;
pub const EP_TEMP_EVAPORATOR: EndpointId = 6;
pub const EP_TEMP_COMPRESSOR_DISCHARGE: EndpointId = 7;
pub const EP_DRAINAGE: EndpointId = 8;
pub const EP_BEEPER: EndpointId = 9;
pub const EP_PANEL: EndpointId = 10;
pub const EP_PET_CARE: EndpointId = 11;
pub const EP_POWER_AC_INPUT: EndpointId = 20;
pub const EP_POWER_PV: EndpointId = 21;
pub const EP_POWER_DC_PORT: EndpointId = 22;
pub const EP_POWER_BATTERY: EndpointId = 23;

/// Highest `CurrentLevel` the Level Control cluster defines.
const MATTER_LEVEL_MAX: u32 = 254;

/// Degrees Celsius to Matter's int16 hundredths, saturating.
fn celsius_to_centi(celsius: f32) -> i16 {
    (f64::from(celsius) * 100.0)
        .round()
        .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

/// Matter's int16 hundredths back to degrees Celsius.
fn centi_to_celsius(centi: i16) -> f32 {
    f32::from(centi) / 100.0
}

/// Percent to Matter's uint16 hundredths, saturating.
fn percent_to_centi(percent: f32) -> u16 {
    (f64::from(percent) * 100.0)
        .round()
        .clamp(0.0, f64::from(u16::MAX)) as u16
}

/// A float in some unit to int64 milli-units, saturating.
fn to_milli(value: f32) -> i64 {
    (f64::from(value) * 1000.0).round() as i64
}

/// Percent to Power Source's half-percent `BatPercentRemaining` (0-200).
fn percent_to_half_percent(percent: f32) -> u8 {
    (f64::from(percent) * 2.0).round().clamp(0.0, 200.0) as u8
}

/// Whole percent, saturating, for the cluster attributes that use it.
fn to_whole_percent(percent: f32) -> u8 {
    f64::from(percent).round().clamp(0.0, 100.0) as u8
}

fn minutes_to_seconds(minutes: u32) -> u32 {
    minutes.saturating_mul(60)
}

/// Panel brightness percent to Matter's 0-254 `CurrentLevel`.
fn percent_to_level(percent: u32) -> u8 {
    let percent = percent.min(100);
    ((percent * MATTER_LEVEL_MAX + 50) / 100) as u8
}

/// Matter's 0-254 `CurrentLevel` back to panel brightness percent.
fn level_to_percent(level: u8) -> u32 {
    (u32::from(level) * 100 + MATTER_LEVEL_MAX / 2) / MATTER_LEVEL_MAX
}

/// Resolve the mode to report, honouring standby as a separate axis.
///
/// `dev_sleep_state` overrides the operating mode: when the unit is in standby
/// it is off regardless of the mode it still reports.
fn system_mode(state: &DeviceState) -> Option<SystemMode> {
    if state.display().dev_sleep_state == Some(semantics::SLEEP_STATE_STANDBY) {
        return Some(SystemMode::Off);
    }

    match OperatingMode::from_wire(state.display().wave_operating_mode?)? {
        OperatingMode::Off => Some(SystemMode::Off),
        OperatingMode::Cool => Some(SystemMode::Cool),
        OperatingMode::Heat => Some(SystemMode::Heat),
        OperatingMode::FanOnly => Some(SystemMode::FanOnly),
        OperatingMode::Dry => Some(SystemMode::Dry),
        OperatingMode::Auto => Some(SystemMode::Auto),
    }
}

/// The mode the device is configured for, ignoring standby.
///
/// Command translation needs this rather than `system_mode`: which field a
/// setpoint write targets depends on the configured mode even when the unit
/// happens to be paused.
fn configured_mode(state: &DeviceState) -> Option<OperatingMode> {
    OperatingMode::from_wire(state.display().wave_operating_mode?)
}

fn thermostat(state: &DeviceState) -> ThermostatCluster {
    let params = state.active_params();

    // Matter carries one setpoint pair. Which of the device's saved values it
    // holds depends on the mode: auto bounds a range, cool and heat each name
    // a single target, and the remaining modes have no temperature target.
    //
    // Selected by the configured mode rather than the reported one, so that a
    // unit in standby still shows the target it will resume to. Reading
    // `system_mode` here made the setpoint vanish whenever the unit was
    // paused, leaving a client with nothing to display or adjust.
    let (cooling, heating) = match configured_mode(state) {
        Some(OperatingMode::Auto) => (
            params.temp_thermostatic_upper_limit,
            params.temp_thermostatic_lower_limit,
        ),
        Some(OperatingMode::Cool) => (params.temp_set, None),
        Some(OperatingMode::Heat) => (None, params.temp_set),
        _ => (None, None),
    };

    let mode = system_mode(state);

    ThermostatCluster {
        local_temperature: state.display().temp_ambient.map(celsius_to_centi),
        occupied_cooling_setpoint: cooling.map(celsius_to_centi),
        occupied_heating_setpoint: heating.map(celsius_to_centi),
        control_sequence_of_operation: ControlSequenceOfOperation::CoolingAndHeating,
        system_mode: mode,
        abs_min_heat_setpoint_limit: Some(celsius_to_centi(semantics::TEMP_SET_MIN_C)),
        abs_max_heat_setpoint_limit: Some(celsius_to_centi(semantics::TEMP_SET_MAX_C)),
        abs_min_cool_setpoint_limit: Some(celsius_to_centi(semantics::TEMP_SET_MIN_C)),
        abs_max_cool_setpoint_limit: Some(celsius_to_centi(semantics::TEMP_SET_MAX_C)),
    }
}

/// Summarise a five-step fan as Matter's coarse low/medium/high.
///
/// `speed_setting` carries the real value; this exists because `FanMode` is a
/// mandatory attribute of the cluster.
fn fan_mode_for_step(step: u8) -> FanMode {
    match step {
        0 => FanMode::Off,
        1 => FanMode::Low,
        2 | 3 => FanMode::Medium,
        _ => FanMode::High,
    }
}

fn fan_control(state: &DeviceState) -> FanControlCluster {
    let percent = state.active_params().airflow_speed;
    let step = percent.map(semantics::fan_percent_to_step);

    FanControlCluster {
        fan_mode: step.map(fan_mode_for_step),
        fan_mode_sequence: FanModeSequence::OffLowMedHigh,
        percent_setting: percent.map(|p| p.min(100) as u8),
        percent_current: percent.map(|p| p.min(100) as u8),
        speed_max: Some(semantics::FAN_STEP_MAX),
        speed_setting: step,
        speed_current: step,
    }
}

fn dehumidification(state: &DeviceState) -> DehumidificationControlCluster {
    DehumidificationControlCluster {
        relative_humidity: state.display().humi_ambient.map(to_whole_percent),
        rh_dehumidification_setpoint: state.active_params().humi_set.map(to_whole_percent),
    }
}

fn user_interface(state: &DeviceState) -> ThermostatUserInterfaceConfigurationCluster {
    let mode = state
        .display()
        .user_temp_unit
        .and_then(UserTempUnit::from_wire)
        .and_then(|unit| match unit {
            UserTempUnit::Celsius => Some(TemperatureDisplayMode::Celsius),
            UserTempUnit::Fahrenheit => Some(TemperatureDisplayMode::Fahrenheit),
            // "Unset" is not a display mode; report nothing rather than
            // guessing which unit the panel is showing.
            UserTempUnit::Unset => None,
        });

    ThermostatUserInterfaceConfigurationCluster {
        temperature_display_mode: mode,
    }
}

fn preset_mode_select(state: &DeviceState) -> ModeSelectCluster {
    ModeSelectCluster {
        description: "Preset".to_string(),
        supported_modes: Preset::ALL
            .iter()
            .map(|preset| ModeOption {
                label: preset.label().to_string(),
                mode: preset.to_wire() as u8,
            })
            .collect(),
        // An unrecognised submode is reported as unknown rather than mapped
        // onto a neighbouring preset.
        current_mode: state
            .active_params()
            .submode
            .and_then(Preset::from_wire)
            .map(|preset| preset.to_wire() as u8),
    }
}

fn power_source(state: &DeviceState) -> PowerSourceCluster {
    let soc = state.display().bms_batt_soc;

    PowerSourceCluster {
        status: if soc.is_some() {
            PowerSourceStatus::Active
        } else {
            PowerSourceStatus::Unspecified
        },
        order: 0,
        description: "Internal battery".to_string(),
        bat_voltage: state
            .runtime()
            .bms_batt_vol
            .map(|volts| to_milli(volts).clamp(0, i64::from(u32::MAX)) as u32),
        bat_percent_remaining: soc.map(percent_to_half_percent),
        bat_time_remaining: state
            .display()
            .bms_dsg_rem_time
            .and_then(semantics::plausible_remaining_minutes)
            .map(minutes_to_seconds),
        // Matter's BatChargeLevel is a three-way ok/warning/critical judgement
        // with no thresholds on the wire, and BatChargeState needs the meaning
        // of bms_chg_dsg_state, whose encoding is unknown. Both stay null
        // rather than being invented.
        bat_charge_level: None,
        bat_charge_state: None,
        bat_time_to_full_charge: state
            .display()
            .bms_chg_rem_time
            .and_then(semantics::plausible_remaining_minutes)
            .map(minutes_to_seconds),
    }
}

fn temperature_endpoint(celsius: Option<f32>) -> Endpoint {
    Endpoint::from_clusters([Cluster::TemperatureMeasurement(
        TemperatureMeasurementCluster {
            measured_value: celsius.map(celsius_to_centi),
        },
    )])
}

fn power_cluster(
    mode: PowerMode,
    watts: Option<f32>,
    volts: Option<f32>,
    amps: Option<f32>,
    hertz: Option<u32>,
) -> Cluster {
    Cluster::ElectricalPowerMeasurement(ElectricalPowerMeasurementCluster {
        power_mode: mode,
        voltage: volts.map(to_milli),
        active_power: watts.map(to_milli),
        active_current: amps.map(to_milli),
        frequency: hertz.map(|hz| i64::from(hz) * 1000),
    })
}

/// A Boolean State cluster, or nothing if the device has not reported.
///
/// The cluster has no null: a `state_value` of false is a positive claim. On
/// hardware this fabricated "no charger attached" while the unit was running
/// off the mains, because the flag it read is one this device never sends.
/// Omitting the cluster until there is a value keeps that from happening
/// again.
fn boolean(value: Option<bool>) -> Option<Cluster> {
    value.map(|state_value| Cluster::BooleanState(BooleanStateCluster { state_value }))
}

/// Build the full endpoint map for a device.
///
/// Every endpoint is always present and unreported attributes are null, so a
/// node has a stable shape from the moment it is declared rather than growing
/// as telemetry trickles in. Boolean State clusters are the exception: the
/// cluster carries no null, so it is omitted until there is a value.
pub fn build_endpoints(state: &DeviceState) -> HashMap<EndpointId, Endpoint> {
    let display = state.display();
    let runtime = state.runtime();

    let mut endpoints = HashMap::new();

    endpoints.insert(
        EP_AIR_CONDITIONER,
        Endpoint::from_clusters([
            Cluster::OnOff(OnOffCluster {
                on_off: system_mode(state).is_some_and(|mode| mode != SystemMode::Off),
            }),
            Cluster::Thermostat(thermostat(state)),
            Cluster::FanControl(fan_control(state)),
            Cluster::TemperatureMeasurement(TemperatureMeasurementCluster {
                measured_value: display.temp_ambient.map(celsius_to_centi),
            }),
            Cluster::RelativeHumidityMeasurement(RelativeHumidityMeasurementCluster {
                measured_value: display.humi_ambient.map(percent_to_centi),
            }),
            Cluster::DehumidificationControl(dehumidification(state)),
            Cluster::ThermostatUserInterfaceConfiguration(user_interface(state)),
            Cluster::ModeSelect(preset_mode_select(state)),
        ]),
    );

    endpoints.insert(
        EP_BATTERY,
        Endpoint::from_clusters([Cluster::PowerSource(power_source(state))]),
    );

    endpoints.insert(
        EP_TEMP_OUTLET_AIR,
        temperature_endpoint(display.temp_indoor_supply_air),
    );
    endpoints.insert(
        EP_TEMP_OUTDOOR,
        temperature_endpoint(runtime.temp_outdoor_ambient),
    );
    endpoints.insert(
        EP_TEMP_CONDENSER,
        temperature_endpoint(runtime.temp_condenser),
    );
    endpoints.insert(
        EP_TEMP_EVAPORATOR,
        temperature_endpoint(runtime.temp_evaporator),
    );
    endpoints.insert(
        EP_TEMP_COMPRESSOR_DISCHARGE,
        temperature_endpoint(runtime.temp_compressor_discharge),
    );

    endpoints.insert(
        EP_DRAINAGE,
        Endpoint::from_clusters(
            [
                Some(Cluster::OnOff(OnOffCluster {
                    on_off: display.drainage_mode == Some(semantics::DRAINAGE_MODE_ON),
                })),
                boolean(display.in_drainage),
            ]
            .into_iter()
            .flatten(),
        ),
    );

    endpoints.insert(
        EP_BEEPER,
        Endpoint::from_clusters([Cluster::OnOff(OnOffCluster {
            on_off: display.en_beep.unwrap_or(false),
        })]),
    );

    endpoints.insert(
        EP_PANEL,
        Endpoint::from_clusters([Cluster::LevelControl(LevelControlCluster {
            current_level: display.lcd_light.map(percent_to_level),
        })]),
    );

    endpoints.insert(
        EP_PET_CARE,
        Endpoint::from_clusters(
            [
                Some(Cluster::OnOff(OnOffCluster {
                    on_off: display.en_pet_care.unwrap_or(false),
                })),
                boolean(display.pet_care_warning),
            ]
            .into_iter()
            .flatten(),
        ),
    );

    endpoints.insert(
        EP_POWER_AC_INPUT,
        Endpoint::from_clusters(
            [
                // `pow_get_ac` rather than `pow_get_ac_in`: the latter is never
                // sent, and this figure matches both the app's reported draw
                // and the volts times amps decoded from the same rail.
                Some(power_cluster(
                    PowerMode::Ac,
                    display.pow_get_ac,
                    runtime.plug_in_info_ac_in_vol,
                    runtime.plug_in_info_ac_in_amp,
                    None,
                )),
                boolean(display.plug_in_info_ac_in_flag),
            ]
            .into_iter()
            .flatten(),
        ),
    );
    endpoints.insert(
        EP_POWER_PV,
        Endpoint::from_clusters([power_cluster(
            PowerMode::Dc,
            display.pow_get_pv,
            runtime.plug_in_info_pv_vol,
            runtime.plug_in_info_pv_amp,
            None,
        )]),
    );
    endpoints.insert(
        EP_POWER_DC_PORT,
        Endpoint::from_clusters(
            [
                // No power figure: `pow_get_dcp` is never sent, though the
                // port's own volts and amps are.
                Some(power_cluster(
                    PowerMode::Dc,
                    None,
                    runtime.plug_in_info_dcp_vol,
                    runtime.plug_in_info_dcp_amp,
                    None,
                )),
                boolean(display.plug_in_info_dcp_in_flag),
            ]
            .into_iter()
            .flatten(),
        ),
    );
    endpoints.insert(
        EP_POWER_BATTERY,
        // The sign convention of pow_get_bms is assumed positive-in,
        // negative-out and is unconfirmed against hardware.
        Endpoint::from_clusters([power_cluster(
            PowerMode::Dc,
            display.pow_get_bms,
            runtime.bms_batt_vol,
            runtime.bms_batt_amp,
            None,
        )]),
    );

    endpoints
}

/// Why a cluster command could not be turned into a device command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    /// The endpoint does not carry the cluster the command targets.
    UnsupportedOnEndpoint {
        endpoint: EndpointId,
        cluster_id: u32,
    },
    /// The cluster is modelled but this particular value has no device
    /// equivalent.
    Unsupported(&'static str),
    /// The command needs a cached value the device has never reported.
    NotYetKnown(&'static str),
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandError::UnsupportedOnEndpoint {
                endpoint,
                cluster_id,
            } => write!(
                f,
                "endpoint {endpoint} does not support cluster 0x{cluster_id:04X}"
            ),
            CommandError::Unsupported(what) => write!(f, "{what}"),
            CommandError::NotYetKnown(what) => {
                write!(f, "{what} is not known yet; wait for the device to report")
            }
        }
    }
}

impl std::error::Error for CommandError {}

fn unsupported(endpoint: EndpointId, command: &ClusterCommand) -> CommandError {
    CommandError::UnsupportedOnEndpoint {
        endpoint,
        cluster_id: command.cluster_id(),
    }
}

/// Translate a set of Matter attribute writes into the config write that performs them.
///
/// Returns the write rather than publishing it, so the caller can log it,
/// apply it optimistically and frame it.
pub fn writes_to_config_write(
    state: &DeviceState,
    endpoint: EndpointId,
    writes: &[ClusterWrite],
) -> Result<ConfigWrite, CommandError> {
    if endpoint != EP_AIR_CONDITIONER {
        return Err(CommandError::UnsupportedOnEndpoint {
            endpoint,
            cluster_id: CLUSTER_ID_THERMOSTAT,
        });
    }

    let mut result = ConfigWrite::default();
    for write in writes {
        let partial = thermostat_write(state, write)?;
        result = merge_writes(result, partial);
    }
    Ok(result)
}

fn merge_writes(mut left: ConfigWrite, right: ConfigWrite) -> ConfigWrite {
    macro_rules! merge_opt {
        ($field:ident) => {
            if right.$field.is_some() {
                left.$field = right.$field;
            }
        };
    }

    merge_opt!(cfg_main_power);
    merge_opt!(cfg_sys_pause);
    merge_opt!(cfg_wave_operating_mode);
    merge_opt!(cfg_wave_operating_submode);
    merge_opt!(cfg_temp_set);
    merge_opt!(cfg_temp_thermostatic_upper_limit);
    merge_opt!(cfg_temp_thermostatic_lower_limit);
    merge_opt!(cfg_airflow_speed);
    merge_opt!(cfg_humi_set);
    merge_opt!(cfg_drainage_mode);
    merge_opt!(cfg_en_pet_care);
    merge_opt!(cfg_user_temp_unit);
    merge_opt!(en_beep);
    merge_opt!(lcd_light);

    left
}

/// Translate a Matter cluster command into the config write that performs it.
///
/// Returns the write rather than publishing it, so the caller can log it,
/// apply it optimistically and frame it.
pub fn command_to_config_write(
    state: &DeviceState,
    endpoint: EndpointId,
    command: &ClusterCommand,
) -> Result<ConfigWrite, CommandError> {
    match (endpoint, command) {
        (EP_AIR_CONDITIONER, ClusterCommand::OnOff(cmd)) => air_conditioner_power(state, cmd),
        (EP_AIR_CONDITIONER, ClusterCommand::Thermostat(cmd)) => thermostat_command(state, cmd),
        (EP_AIR_CONDITIONER, ClusterCommand::FanControl(cmd)) => fan_command(cmd),
        (EP_AIR_CONDITIONER, ClusterCommand::DehumidificationControl(cmd)) => {
            let DehumidificationControlCommand::SetRhDehumidificationSetpoint { percent } = cmd;
            Ok(ConfigWrite {
                cfg_humi_set: Some(semantics::clamp_humi_set(u32::from(*percent)) as f32),
                ..Default::default()
            })
        }
        (EP_AIR_CONDITIONER, ClusterCommand::ThermostatUserInterfaceConfiguration(cmd)) => {
            let ThermostatUserInterfaceConfigurationCommand::SetTemperatureDisplayMode { mode } =
                cmd;
            let unit = match mode {
                TemperatureDisplayMode::Celsius => UserTempUnit::Celsius,
                TemperatureDisplayMode::Fahrenheit => UserTempUnit::Fahrenheit,
            };
            Ok(ConfigWrite {
                cfg_user_temp_unit: Some(unit.to_wire()),
                ..Default::default()
            })
        }
        (EP_AIR_CONDITIONER, ClusterCommand::ModeSelect(cmd)) => {
            let ModeSelectCommand::ChangeToMode { new_mode } = cmd;
            let preset = Preset::from_wire(u32::from(*new_mode))
                .ok_or(CommandError::Unsupported("unknown preset"))?;
            Ok(ConfigWrite {
                cfg_wave_operating_submode: Some(preset.to_wire()),
                ..Default::default()
            })
        }

        (EP_DRAINAGE, ClusterCommand::OnOff(cmd)) => {
            let on = resolve_on_off(cmd, state.display().drainage_mode == Some(1));
            Ok(ConfigWrite {
                cfg_drainage_mode: Some(u32::from(on)),
                ..Default::default()
            })
        }

        (EP_BEEPER, ClusterCommand::OnOff(cmd)) => {
            let audible = resolve_on_off(cmd, state.display().en_beep.unwrap_or(false));
            Ok(ConfigWrite {
                en_beep: Some(semantics::beeper_wire_from_audible(audible)),
                ..Default::default()
            })
        }

        (EP_PET_CARE, ClusterCommand::OnOff(cmd)) => {
            let on = resolve_on_off(cmd, state.display().en_pet_care.unwrap_or(false));
            Ok(ConfigWrite {
                cfg_en_pet_care: Some(on),
                ..Default::default()
            })
        }

        (
            EP_PANEL,
            ClusterCommand::LevelControl(LevelControlCommand::MoveToLevel { level, .. }),
        ) => Ok(ConfigWrite {
            lcd_light: Some(semantics::clamp_lcd_light(level_to_percent(*level)) as i32),
            ..Default::default()
        }),

        _ => Err(unsupported(endpoint, command)),
    }
}

/// Resolve On/Off/Toggle against the current cached value.
fn resolve_on_off(command: &OnOffCommand, current: bool) -> bool {
    match command {
        OnOffCommand::On => true,
        OnOffCommand::Off => false,
        OnOffCommand::Toggle => !current,
    }
}

/// Power the unit on or off.
///
/// Off is `cfg_sys_pause`, not `cfgPowerOff`: standby is the tested path and
/// the app never uses the other field for this device. On without a mode
/// resumes whatever mode the unit was last in.
fn air_conditioner_power(
    state: &DeviceState,
    command: &OnOffCommand,
) -> Result<ConfigWrite, CommandError> {
    let running = system_mode(state).is_some_and(|mode| mode != SystemMode::Off);

    if resolve_on_off(command, running) {
        Ok(ConfigWrite {
            cfg_main_power: Some(true),
            ..Default::default()
        })
    } else {
        Ok(ConfigWrite {
            cfg_sys_pause: Some(true),
            ..Default::default()
        })
    }
}

fn thermostat_command(
    state: &DeviceState,
    command: &ThermostatCommand,
) -> Result<ConfigWrite, CommandError> {
    match command {
        ThermostatCommand::SetpointRaiseLower { mode, amount } => {
            // `amount` is a relative adjustment in tenths of a degree, so it
            // needs a setpoint to adjust.
            let delta = f32::from(*amount) / 10.0;
            let params = state.active_params();

            let (base, cooling) = match mode {
                SetpointMode::Cool => (
                    params.temp_set.or(params.temp_thermostatic_upper_limit),
                    true,
                ),
                SetpointMode::Heat => (
                    params.temp_set.or(params.temp_thermostatic_lower_limit),
                    false,
                ),
                SetpointMode::Both => {
                    return Err(CommandError::Unsupported(
                        "the Wave 3 stores one setpoint per mode, so Both is ambiguous",
                    ));
                }
            };

            let base = base.ok_or(CommandError::NotYetKnown("the current setpoint"))?;
            Ok(setpoint_write(state, base + delta, cooling))
        }
    }
}

fn thermostat_write(
    state: &DeviceState,
    write: &ClusterWrite,
) -> Result<ConfigWrite, CommandError> {
    let ClusterWrite::Thermostat(t) = write;

    let mut result = ConfigWrite::default();

    if let Some(mode) = t.system_mode {
        result = merge_writes(result, set_system_mode(mode)?);
    }

    if let Some(centi_celsius) = t.occupied_cooling_setpoint {
        result = merge_writes(
            result,
            setpoint_write(state, centi_to_celsius(centi_celsius), true),
        );
    }

    if let Some(centi_celsius) = t.occupied_heating_setpoint {
        result = merge_writes(
            result,
            setpoint_write(state, centi_to_celsius(centi_celsius), false),
        );
    }

    Ok(result)
}

/// Build the write for a new absolute setpoint.
///
/// In auto mode the pair bounds a range, so a cooling setpoint is the upper
/// limit and a heating setpoint the lower. In every other mode the device
/// stores a single target and both map onto it.
fn setpoint_write(state: &DeviceState, celsius: f32, cooling: bool) -> ConfigWrite {
    let celsius = semantics::clamp_temp_set(celsius);

    if configured_mode(state) == Some(OperatingMode::Auto) {
        if cooling {
            ConfigWrite {
                cfg_temp_thermostatic_upper_limit: Some(celsius),
                ..Default::default()
            }
        } else {
            ConfigWrite {
                cfg_temp_thermostatic_lower_limit: Some(celsius),
                ..Default::default()
            }
        }
    } else {
        ConfigWrite {
            cfg_temp_set: Some(celsius),
            ..Default::default()
        }
    }
}

fn set_system_mode(mode: SystemMode) -> Result<ConfigWrite, CommandError> {
    // If a system-mode write does not change the mode, it should not force a
    // power-on or a pause. The integration only sends non-empty writes, so a
    // no-op is naturally suppressed higher up.

    let operating = match mode {
        SystemMode::Off => {
            return Ok(ConfigWrite {
                cfg_sys_pause: Some(true),
                ..Default::default()
            });
        }
        SystemMode::Cool => OperatingMode::Cool,
        SystemMode::Heat => OperatingMode::Heat,
        SystemMode::FanOnly => OperatingMode::FanOnly,
        SystemMode::Dry => OperatingMode::Dry,
        SystemMode::Auto => OperatingMode::Auto,
        SystemMode::EmergencyHeat | SystemMode::Precooling | SystemMode::Sleep => {
            return Err(CommandError::Unsupported(
                "the Wave 3 has no equivalent operating mode",
            ));
        }
    };

    // Powering up and selecting the mode must ride in one write.
    Ok(ConfigWrite {
        cfg_main_power: Some(true),
        cfg_wave_operating_mode: Some(operating.to_wire()),
        ..Default::default()
    })
}

fn fan_command(command: &FanControlCommand) -> Result<ConfigWrite, CommandError> {
    let step = match command {
        FanControlCommand::SetSpeedSetting { speed } => *speed,
        // Snap first: the device accepts only the five discrete percentages,
        // so an arbitrary percentage has to be resolved to a step and then
        // back to a permitted value.
        FanControlCommand::SetPercentSetting { percent } => {
            semantics::fan_percent_to_step(u32::from(*percent))
        }
        FanControlCommand::SetFanMode { mode } => match mode {
            FanMode::Low => 1,
            FanMode::Medium => 3,
            FanMode::High | FanMode::On => 5,
            FanMode::Off => 0,
            FanMode::Auto | FanMode::Smart => {
                return Err(CommandError::Unsupported(
                    "the Wave 3 has no automatic fan mode",
                ));
            }
        },
    };

    let percent = semantics::fan_step_to_percent(step).ok_or(CommandError::Unsupported(
        "fan speed 0 stops the unit; use the OnOff cluster instead",
    ))?;

    Ok(ConfigWrite {
        cfg_airflow_speed: Some(percent),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Instant;

    use super::*;
    use crate::integrations::ecoflow::wave3::codec::DisplayProperties;
    use crate::integrations::ecoflow::wave3::codec::ModeParamItem;
    use crate::integrations::ecoflow::wave3::codec::RuntimeProperties;
    use crate::matter::ThermostatWrite;

    /// A six-entry mode parameter list, as the device sends it.
    fn mode_list() -> Vec<ModeParamItem> {
        vec![
            ModeParamItem::default(),
            ModeParamItem {
                submode: Some(0),
                airflow_speed: Some(40),
                temp_set: Some(22.0),
                ..Default::default()
            },
            ModeParamItem {
                submode: Some(3),
                airflow_speed: Some(60),
                temp_set: Some(26.0),
                ..Default::default()
            },
            ModeParamItem::default(),
            ModeParamItem {
                humi_set: Some(55.0),
                ..Default::default()
            },
            ModeParamItem {
                temp_thermostatic_upper_limit: Some(24.0),
                temp_thermostatic_lower_limit: Some(20.0),
                ..Default::default()
            },
        ]
    }

    fn state_in_mode(mode: u32) -> DeviceState {
        let mut state = DeviceState::default();
        state.apply_display(
            DisplayProperties {
                wave_operating_mode: Some(mode),
                dev_sleep_state: Some(0),
                mode_params: Some(mode_list()),
                temp_ambient: Some(21.5),
                humi_ambient: Some(55.25),
                ..Default::default()
            },
            Instant::now(),
        );
        state
    }
    #[test]
    fn celsius_converts_to_matter_hundredths() {
        assert_eq!(celsius_to_centi(22.0), 2200);
        assert_eq!(celsius_to_centi(-5.5), -550);
        assert_eq!(celsius_to_centi(21.5), 2150);
        // The device sends f32, so a value like this is not exactly
        // representable: 21.345f32 is 21.34499..., which rounds down.
        assert_eq!(celsius_to_centi(21.345), 2134);
        // Out-of-range readings saturate rather than wrapping.
        assert_eq!(celsius_to_centi(40000.0), i16::MAX);
        assert_eq!(celsius_to_centi(-40000.0), i16::MIN);
    }

    #[test]
    fn centi_celsius_round_trips() {
        assert_eq!(centi_to_celsius(celsius_to_centi(22.0)), 22.0);
        assert_eq!(centi_to_celsius(celsius_to_centi(16.5)), 16.5);
    }

    #[test]
    fn battery_percent_uses_matter_half_percent_units() {
        assert_eq!(percent_to_half_percent(100.0), 200);
        assert_eq!(percent_to_half_percent(50.0), 100);
        assert_eq!(percent_to_half_percent(0.0), 0);
        assert_eq!(percent_to_half_percent(101.0), 200);
        assert_eq!(percent_to_half_percent(-1.0), 0);
    }

    #[test]
    fn power_measurements_use_milli_units() {
        assert_eq!(to_milli(850.0), 850_000);
        // Negative power is meaningful: it indicates direction.
        assert_eq!(to_milli(-120.5), -120_500);
    }

    #[test]
    fn panel_brightness_round_trips_through_matter_levels() {
        assert_eq!(percent_to_level(0), 0);
        assert_eq!(percent_to_level(100), 254);
        // Above 100 clamps rather than overflowing the u8.
        assert_eq!(percent_to_level(200), 254);

        for percent in [0u32, 1, 25, 50, 75, 99, 100] {
            let round_tripped = level_to_percent(percent_to_level(percent));
            assert_eq!(round_tripped, percent, "percent {percent}");
        }
    }
    #[test]
    fn standby_overrides_the_reported_operating_mode() {
        let mut state = DeviceState::default();
        // The unit still reports "cool" while paused; standby wins.
        state.apply_display(
            DisplayProperties {
                wave_operating_mode: Some(1),
                dev_sleep_state: Some(1),
                ..Default::default()
            },
            Instant::now(),
        );
        assert_eq!(system_mode(&state), Some(SystemMode::Off));

        // The configured mode is still cool, which is what commands need.
        assert_eq!(configured_mode(&state), Some(OperatingMode::Cool));
    }

    #[test]
    fn a_paused_unit_still_reports_the_setpoint_it_will_resume_to() {
        // Hardware: turning the unit off and on again left hearthd reporting a
        // running unit as off with no setpoint at all, because standby had
        // been folded into the operating mode.
        let mut state = state_in_mode(1);
        state.apply_display(
            DisplayProperties {
                dev_sleep_state: Some(semantics::SLEEP_STATE_STANDBY),
                ..Default::default()
            },
            Instant::now(),
        );

        let cluster = thermostat(&state);
        assert_eq!(cluster.system_mode, Some(SystemMode::Off));
        assert_eq!(cluster.occupied_cooling_setpoint, Some(2200));
    }

    #[test]
    fn every_operating_mode_maps_to_a_matter_system_mode() {
        for (wire, expected) in [
            (0, SystemMode::Off),
            (1, SystemMode::Cool),
            (2, SystemMode::Heat),
            (3, SystemMode::FanOnly),
            (4, SystemMode::Dry),
            (5, SystemMode::Auto),
        ] {
            assert_eq!(system_mode(&state_in_mode(wire)), Some(expected));
        }
    }

    #[test]
    fn an_unreported_mode_is_null_rather_than_a_guess() {
        let state = DeviceState::default();
        assert_eq!(system_mode(&state), None);
        assert_eq!(thermostat(&state).system_mode, None);
        assert_eq!(thermostat(&state).local_temperature, None);
    }

    #[test]
    fn cool_mode_puts_the_setpoint_on_the_cooling_attribute() {
        let cluster = thermostat(&state_in_mode(1));
        assert_eq!(cluster.occupied_cooling_setpoint, Some(2200));
        assert_eq!(cluster.occupied_heating_setpoint, None);
    }

    #[test]
    fn heat_mode_puts_the_setpoint_on_the_heating_attribute() {
        let cluster = thermostat(&state_in_mode(2));
        assert_eq!(cluster.occupied_heating_setpoint, Some(2600));
        assert_eq!(cluster.occupied_cooling_setpoint, None);
    }

    #[test]
    fn auto_mode_maps_the_limits_onto_the_setpoint_pair() {
        // Matter's auto semantics: cooling setpoint is the upper bound,
        // heating setpoint the lower.
        let cluster = thermostat(&state_in_mode(5));
        assert_eq!(cluster.occupied_cooling_setpoint, Some(2400));
        assert_eq!(cluster.occupied_heating_setpoint, Some(2000));
    }

    #[test]
    fn fan_only_and_dry_modes_have_no_temperature_target() {
        for mode in [3, 4] {
            let cluster = thermostat(&state_in_mode(mode));
            assert_eq!(cluster.occupied_cooling_setpoint, None, "mode {mode}");
            assert_eq!(cluster.occupied_heating_setpoint, None, "mode {mode}");
        }
    }

    #[test]
    fn thermostat_advertises_the_devices_setpoint_range() {
        let cluster = thermostat(&state_in_mode(1));
        assert_eq!(cluster.abs_min_cool_setpoint_limit, Some(1600));
        assert_eq!(cluster.abs_max_cool_setpoint_limit, Some(3000));
        assert_eq!(cluster.abs_min_heat_setpoint_limit, Some(1600));
        assert_eq!(cluster.abs_max_heat_setpoint_limit, Some(3000));
    }

    #[test]
    fn fan_control_reports_both_the_step_and_the_percentage() {
        let cluster = fan_control(&state_in_mode(1));
        assert_eq!(cluster.percent_setting, Some(40));
        assert_eq!(cluster.speed_setting, Some(2));
        assert_eq!(cluster.speed_max, Some(5));
        assert_eq!(cluster.fan_mode, Some(FanMode::Medium));
    }

    #[test]
    fn dry_mode_setpoint_lands_on_the_dehumidification_cluster() {
        let cluster = dehumidification(&state_in_mode(4));
        assert_eq!(cluster.rh_dehumidification_setpoint, Some(55));
        assert_eq!(cluster.relative_humidity, Some(55));
    }

    #[test]
    fn an_unset_temperature_unit_is_reported_as_unknown() {
        let mut state = DeviceState::default();
        state.apply_display(
            DisplayProperties {
                user_temp_unit: Some(0),
                ..Default::default()
            },
            Instant::now(),
        );
        assert_eq!(user_interface(&state).temperature_display_mode, None);

        state.apply_display(
            DisplayProperties {
                user_temp_unit: Some(2),
                ..Default::default()
            },
            Instant::now(),
        );
        assert_eq!(
            user_interface(&state).temperature_display_mode,
            Some(TemperatureDisplayMode::Fahrenheit)
        );
    }

    #[test]
    fn no_preset_selected_reads_as_normal() {
        // Hardware reports submode 1 when no preset is active. Rejecting it
        // left the preset permanently unknown on a real unit.
        let mut state = DeviceState::default();
        let mut list = mode_list();
        list[1].submode = Some(1);
        state.apply_display(
            DisplayProperties {
                wave_operating_mode: Some(1),
                mode_params: Some(list),
                ..Default::default()
            },
            Instant::now(),
        );
        assert_eq!(preset_mode_select(&state).current_mode, Some(0));
    }

    #[test]
    fn an_unrecognised_preset_is_unknown_rather_than_a_neighbour() {
        let mut state = DeviceState::default();
        let mut list = mode_list();
        list[1].submode = Some(9);
        state.apply_display(
            DisplayProperties {
                wave_operating_mode: Some(1),
                mode_params: Some(list),
                ..Default::default()
            },
            Instant::now(),
        );
        assert_eq!(preset_mode_select(&state).current_mode, None);
        // The options are still advertised, so a client can still pick one.
        assert_eq!(preset_mode_select(&state).supported_modes.len(), 4);
    }

    #[test]
    fn battery_time_uses_seconds_and_rejects_sentinel_readings() {
        let mut state = DeviceState::default();
        state.apply_display(
            DisplayProperties {
                bms_batt_soc: Some(76.0),
                bms_dsg_rem_time: Some(120),
                // Implausible: the unit is not charging.
                bms_chg_rem_time: Some(59_940),
                ..Default::default()
            },
            Instant::now(),
        );

        let cluster = power_source(&state);
        assert_eq!(cluster.bat_percent_remaining, Some(152));
        assert_eq!(cluster.bat_time_remaining, Some(7200));
        assert_eq!(cluster.bat_time_to_full_charge, None);
        assert_eq!(cluster.status, PowerSourceStatus::Active);
    }

    #[test]
    fn battery_charge_state_is_left_unknown_rather_than_invented() {
        let mut state = DeviceState::default();
        state.apply_display(
            DisplayProperties {
                bms_batt_soc: Some(5.0),
                bms_chg_dsg_state: Some(2),
                ..Default::default()
            },
            Instant::now(),
        );
        let cluster = power_source(&state);
        // The encoding of bms_chg_dsg_state is unknown, and BatChargeLevel
        // would need thresholds that are nowhere on the wire.
        assert_eq!(cluster.bat_charge_state, None);
        assert_eq!(cluster.bat_charge_level, None);
    }

    #[test]
    fn the_beeper_endpoint_reports_the_flag_as_it_stands() {
        // Hardware: reported false was silent under button presses, and
        // writing 1 restored the beep. The flag is not inverted.
        let mut state = DeviceState::default();
        state.apply_display(
            DisplayProperties {
                en_beep: Some(false),
                ..Default::default()
            },
            Instant::now(),
        );
        let endpoints = build_endpoints(&state);
        let beeper = &endpoints[&EP_BEEPER].clusters["OnOff"];
        assert_eq!(beeper, &Cluster::OnOff(OnOffCluster { on_off: false }));

        state.apply_display(
            DisplayProperties {
                en_beep: Some(true),
                ..Default::default()
            },
            Instant::now(),
        );
        let endpoints = build_endpoints(&state);
        assert_eq!(
            endpoints[&EP_BEEPER].clusters["OnOff"],
            Cluster::OnOff(OnOffCluster { on_off: true })
        );
    }

    #[test]
    fn every_endpoint_exists_before_any_telemetry_arrives() {
        // A declared device has a stable shape from the moment it is
        // announced; attributes fill in later.
        let endpoints = build_endpoints(&DeviceState::default());
        for endpoint in [
            EP_AIR_CONDITIONER,
            EP_BATTERY,
            EP_TEMP_OUTLET_AIR,
            EP_TEMP_OUTDOOR,
            EP_TEMP_CONDENSER,
            EP_TEMP_EVAPORATOR,
            EP_TEMP_COMPRESSOR_DISCHARGE,
            EP_DRAINAGE,
            EP_BEEPER,
            EP_PANEL,
            EP_PET_CARE,
            EP_POWER_AC_INPUT,
            EP_POWER_PV,
            EP_POWER_DC_PORT,
            EP_POWER_BATTERY,
        ] {
            assert!(endpoints.contains_key(&endpoint), "endpoint {endpoint}");
        }
    }

    #[test]
    fn each_endpoint_holds_at_most_one_instance_of_a_cluster() {
        // Matter identifies a cluster instance by (endpoint, cluster id), so
        // this is the invariant that forces six thermistors onto six
        // endpoints.
        for (endpoint_id, endpoint) in build_endpoints(&DeviceState::default()) {
            let mut ids: Vec<u32> = endpoint.clusters.values().map(Cluster::id).collect();
            let before = ids.len();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), before, "endpoint {endpoint_id} has a duplicate");
        }
    }

    #[test]
    fn diagnostic_temperatures_land_on_their_own_endpoints() {
        let mut state = DeviceState::default();
        state.apply_runtime(
            RuntimeProperties {
                temp_condenser: Some(41.5),
                temp_evaporator: Some(8.25),
                ..Default::default()
            },
            Instant::now(),
        );

        let endpoints = build_endpoints(&state);
        assert_eq!(
            endpoints[&EP_TEMP_CONDENSER].clusters["TemperatureMeasurement"],
            Cluster::TemperatureMeasurement(TemperatureMeasurementCluster {
                measured_value: Some(4150)
            })
        );
        assert_eq!(
            endpoints[&EP_TEMP_EVAPORATOR].clusters["TemperatureMeasurement"],
            Cluster::TemperatureMeasurement(TemperatureMeasurementCluster {
                measured_value: Some(825)
            })
        );
    }

    #[test]
    fn a_full_upload_produces_a_stable_node_shape() {
        let mut state = DeviceState::default();
        state.apply_display(
            DisplayProperties {
                temp_ambient: Some(21.5),
                humi_ambient: Some(55.25),
                wave_operating_mode: Some(1),
                dev_sleep_state: Some(0),
                temp_indoor_supply_air: Some(14.0),
                mode_params: Some(mode_list()),
                in_drainage: Some(false),
                drainage_mode: Some(1),
                pow_get_ac: Some(484.0),
                pow_get_bms: Some(-120.5),
                pow_get_pv: Some(0.0),
                bms_batt_soc: Some(76.0),
                bms_dsg_rem_time: Some(120),
                bms_chg_rem_time: Some(59_940),
                bms_chg_dsg_state: Some(1),
                en_beep: Some(false),
                lcd_light: Some(50),
                user_temp_unit: Some(1),
                en_pet_care: Some(true),
                pet_care_warning: Some(false),
                plug_in_info_ac_in_flag: Some(true),
                plug_in_info_dcp_in_flag: Some(false),
            },
            Instant::now(),
        );
        state.apply_runtime(
            RuntimeProperties {
                temp_outdoor_ambient: Some(31.0),
                temp_condenser: Some(41.5),
                temp_evaporator: Some(8.25),
                temp_compressor_discharge: Some(68.0),
                plug_in_info_ac_in_vol: Some(230.5),
                plug_in_info_ac_in_amp: Some(5.2),
                plug_in_info_pv_vol: Some(0.0),
                plug_in_info_pv_amp: Some(0.0),
                plug_in_info_dcp_vol: Some(0.0),
                plug_in_info_dcp_amp: Some(0.0),
                bms_batt_vol: Some(51.2),
                bms_batt_amp: Some(-2.35),
            },
            Instant::now(),
        );

        // One line per cluster, ordered, so the whole node shape is legible in
        // a diff. HashMap iteration order is not stable, hence the BTreeMaps.
        let endpoints: BTreeMap<EndpointId, BTreeMap<String, Cluster>> = build_endpoints(&state)
            .into_iter()
            .map(|(id, endpoint)| (id, endpoint.clusters.into_iter().collect()))
            .collect();

        let rendered = endpoints
            .iter()
            .flat_map(|(endpoint_id, clusters)| {
                clusters.iter().map(move |(name, cluster)| {
                    format!(
                        "{endpoint_id:>2} {name}: {}",
                        serde_json::to_string(cluster).unwrap()
                    )
                })
            })
            .collect::<Vec<_>>()
            .join("\n");

        insta::assert_snapshot!(rendered, @r#"
         1 DehumidificationControl: {"cluster":"DehumidificationControl","relative_humidity":55,"rh_dehumidification_setpoint":null}
         1 FanControl: {"cluster":"FanControl","fan_mode":"Medium","fan_mode_sequence":"OffLowMedHigh","percent_setting":40,"percent_current":40,"speed_max":5,"speed_setting":2,"speed_current":2}
         1 ModeSelect: {"cluster":"ModeSelect","description":"Preset","supported_modes":[{"label":"Normal","mode":0},{"label":"Boost","mode":2},{"label":"Eco","mode":4},{"label":"Sleep","mode":3}],"current_mode":0}
         1 OnOff: {"cluster":"OnOff","on_off":true}
         1 RelativeHumidityMeasurement: {"cluster":"RelativeHumidityMeasurement","measured_value":5525}
         1 TemperatureMeasurement: {"cluster":"TemperatureMeasurement","measured_value":2150}
         1 Thermostat: {"cluster":"Thermostat","local_temperature":2150,"occupied_cooling_setpoint":2200,"occupied_heating_setpoint":null,"control_sequence_of_operation":"CoolingAndHeating","system_mode":"Cool","abs_min_heat_setpoint_limit":1600,"abs_max_heat_setpoint_limit":3000,"abs_min_cool_setpoint_limit":1600,"abs_max_cool_setpoint_limit":3000}
         1 ThermostatUserInterfaceConfiguration: {"cluster":"ThermostatUserInterfaceConfiguration","temperature_display_mode":"Celsius"}
         2 PowerSource: {"cluster":"PowerSource","status":"Active","order":0,"description":"Internal battery","bat_voltage":51200,"bat_percent_remaining":152,"bat_time_remaining":7200,"bat_charge_level":null,"bat_charge_state":null,"bat_time_to_full_charge":null}
         3 TemperatureMeasurement: {"cluster":"TemperatureMeasurement","measured_value":1400}
         4 TemperatureMeasurement: {"cluster":"TemperatureMeasurement","measured_value":3100}
         5 TemperatureMeasurement: {"cluster":"TemperatureMeasurement","measured_value":4150}
         6 TemperatureMeasurement: {"cluster":"TemperatureMeasurement","measured_value":825}
         7 TemperatureMeasurement: {"cluster":"TemperatureMeasurement","measured_value":6800}
         8 BooleanState: {"cluster":"BooleanState","state_value":false}
         8 OnOff: {"cluster":"OnOff","on_off":true}
         9 OnOff: {"cluster":"OnOff","on_off":false}
        10 LevelControl: {"cluster":"LevelControl","current_level":127}
        11 BooleanState: {"cluster":"BooleanState","state_value":false}
        11 OnOff: {"cluster":"OnOff","on_off":true}
        20 BooleanState: {"cluster":"BooleanState","state_value":true}
        20 ElectricalPowerMeasurement: {"cluster":"ElectricalPowerMeasurement","power_mode":"Ac","voltage":230500,"active_power":484000,"active_current":5200,"frequency":null}
        21 ElectricalPowerMeasurement: {"cluster":"ElectricalPowerMeasurement","power_mode":"Dc","voltage":0,"active_power":0,"active_current":0,"frequency":null}
        22 BooleanState: {"cluster":"BooleanState","state_value":false}
        22 ElectricalPowerMeasurement: {"cluster":"ElectricalPowerMeasurement","power_mode":"Dc","voltage":0,"active_power":null,"active_current":0,"frequency":null}
        23 ElectricalPowerMeasurement: {"cluster":"ElectricalPowerMeasurement","power_mode":"Dc","voltage":51200,"active_power":-120500,"active_current":-2350,"frequency":null}
        "#);
    }
    fn write_for_command(
        state: &DeviceState,
        endpoint: EndpointId,
        command: ClusterCommand,
    ) -> ConfigWrite {
        command_to_config_write(state, endpoint, &command).expect("command should translate")
    }

    fn write_for_attributes(
        state: &DeviceState,
        endpoint: EndpointId,
        writes: &[ClusterWrite],
    ) -> ConfigWrite {
        writes_to_config_write(state, endpoint, writes).expect("write should translate")
    }

    #[test]
    fn turning_off_pauses_rather_than_powering_off() {
        // cfg_sys_pause is the tested path; cfgPowerOff exists but the app
        // never uses it for this device.
        let write = write_for_command(
            &state_in_mode(1),
            EP_AIR_CONDITIONER,
            ClusterCommand::OnOff(OnOffCommand::Off),
        );
        assert_eq!(write.cfg_sys_pause, Some(true));
        assert_eq!(write.cfg_main_power, None);
    }

    #[test]
    fn turning_on_resumes_the_previous_mode() {
        let write = write_for_command(
            &state_in_mode(1),
            EP_AIR_CONDITIONER,
            ClusterCommand::OnOff(OnOffCommand::On),
        );
        assert_eq!(write.cfg_main_power, Some(true));
        assert_eq!(write.cfg_wave_operating_mode, None);
    }

    #[test]
    fn toggle_uses_the_cached_running_state() {
        let mut standby = DeviceState::default();
        standby.apply_display(
            DisplayProperties {
                wave_operating_mode: Some(1),
                dev_sleep_state: Some(1),
                ..Default::default()
            },
            Instant::now(),
        );
        let write = write_for_command(
            &standby,
            EP_AIR_CONDITIONER,
            ClusterCommand::OnOff(OnOffCommand::Toggle),
        );
        assert_eq!(write.cfg_main_power, Some(true));

        let write = write_for_command(
            &state_in_mode(1),
            EP_AIR_CONDITIONER,
            ClusterCommand::OnOff(OnOffCommand::Toggle),
        );
        assert_eq!(write.cfg_sys_pause, Some(true));
    }

    #[test]
    fn selecting_a_mode_powers_up_in_one_write() {
        // Both fields must ride together, or the unit stays in standby.
        let write = write_for_attributes(
            &state_in_mode(1),
            EP_AIR_CONDITIONER,
            &[ClusterWrite::Thermostat(ThermostatWrite {
                system_mode: Some(SystemMode::Heat),
                ..Default::default()
            })],
        );
        assert_eq!(write.cfg_main_power, Some(true));
        assert_eq!(write.cfg_wave_operating_mode, Some(2));
    }

    #[test]
    fn selecting_off_pauses_the_unit() {
        let write = write_for_attributes(
            &state_in_mode(1),
            EP_AIR_CONDITIONER,
            &[ClusterWrite::Thermostat(ThermostatWrite {
                system_mode: Some(SystemMode::Off),
                ..Default::default()
            })],
        );
        assert_eq!(write.cfg_sys_pause, Some(true));
        assert_eq!(write.cfg_main_power, None);
    }

    #[test]
    fn matter_modes_the_wave_3_lacks_are_refused() {
        for mode in [
            SystemMode::EmergencyHeat,
            SystemMode::Precooling,
            SystemMode::Sleep,
        ] {
            let result = writes_to_config_write(
                &state_in_mode(1),
                EP_AIR_CONDITIONER,
                &[ClusterWrite::Thermostat(ThermostatWrite {
                    system_mode: Some(mode),
                    ..Default::default()
                })],
            );
            assert!(matches!(result, Err(CommandError::Unsupported(_))));
        }
    }

    #[test]
    fn a_setpoint_targets_the_single_value_outside_auto_mode() {
        let write = write_for_attributes(
            &state_in_mode(1),
            EP_AIR_CONDITIONER,
            &[ClusterWrite::Thermostat(ThermostatWrite {
                occupied_cooling_setpoint: Some(2350),
                ..Default::default()
            })],
        );
        assert_eq!(write.cfg_temp_set, Some(23.5));
        assert_eq!(write.cfg_temp_thermostatic_upper_limit, None);
    }

    #[test]
    fn setpoints_target_the_limit_pair_in_auto_mode() {
        let state = state_in_mode(5);

        let write = write_for_attributes(
            &state,
            EP_AIR_CONDITIONER,
            &[ClusterWrite::Thermostat(ThermostatWrite {
                occupied_cooling_setpoint: Some(2500),
                ..Default::default()
            })],
        );
        assert_eq!(write.cfg_temp_thermostatic_upper_limit, Some(25.0));
        assert_eq!(write.cfg_temp_set, None);

        let write = write_for_attributes(
            &state,
            EP_AIR_CONDITIONER,
            &[ClusterWrite::Thermostat(ThermostatWrite {
                occupied_heating_setpoint: Some(1900),
                ..Default::default()
            })],
        );
        assert_eq!(write.cfg_temp_thermostatic_lower_limit, Some(19.0));
    }

    #[test]
    fn setpoints_outside_the_devices_range_are_clamped() {
        let write = write_for_attributes(
            &state_in_mode(1),
            EP_AIR_CONDITIONER,
            &[ClusterWrite::Thermostat(ThermostatWrite {
                occupied_cooling_setpoint: Some(500),
                ..Default::default()
            })],
        );
        assert_eq!(write.cfg_temp_set, Some(16.0));
    }

    #[test]
    fn a_relative_setpoint_change_applies_to_the_cached_value() {
        // +2.5 C on a cached 22.0.
        let write = write_for_command(
            &state_in_mode(1),
            EP_AIR_CONDITIONER,
            ClusterCommand::Thermostat(ThermostatCommand::SetpointRaiseLower {
                mode: SetpointMode::Cool,
                amount: 25,
            }),
        );
        assert_eq!(write.cfg_temp_set, Some(24.5));
    }

    #[test]
    fn a_relative_setpoint_change_needs_a_known_setpoint() {
        let result = command_to_config_write(
            &DeviceState::default(),
            EP_AIR_CONDITIONER,
            &ClusterCommand::Thermostat(ThermostatCommand::SetpointRaiseLower {
                mode: SetpointMode::Cool,
                amount: 10,
            }),
        );
        assert!(matches!(result, Err(CommandError::NotYetKnown(_))));
    }

    #[test]
    fn adjusting_both_setpoints_at_once_is_refused_as_ambiguous() {
        let result = command_to_config_write(
            &state_in_mode(1),
            EP_AIR_CONDITIONER,
            &ClusterCommand::Thermostat(ThermostatCommand::SetpointRaiseLower {
                mode: SetpointMode::Both,
                amount: 10,
            }),
        );
        assert!(matches!(result, Err(CommandError::Unsupported(_))));
    }

    #[test]
    fn fan_speed_steps_map_to_the_permitted_percentages() {
        for (step, percent) in [(1u8, 20u32), (2, 40), (3, 60), (4, 80), (5, 100)] {
            let write = write_for_command(
                &state_in_mode(1),
                EP_AIR_CONDITIONER,
                ClusterCommand::FanControl(FanControlCommand::SetSpeedSetting { speed: step }),
            );
            assert_eq!(write.cfg_airflow_speed, Some(percent), "step {step}");
        }
    }

    #[test]
    fn an_arbitrary_fan_percentage_is_snapped_before_it_is_sent() {
        // The device accepts only five values, so 71 % must not go out as-is.
        let write = write_for_command(
            &state_in_mode(1),
            EP_AIR_CONDITIONER,
            ClusterCommand::FanControl(FanControlCommand::SetPercentSetting { percent: 71 }),
        );
        assert_eq!(write.cfg_airflow_speed, Some(80));
    }

    #[test]
    fn fan_speed_zero_is_refused_with_a_pointer_to_on_off() {
        let result = command_to_config_write(
            &state_in_mode(1),
            EP_AIR_CONDITIONER,
            &ClusterCommand::FanControl(FanControlCommand::SetSpeedSetting { speed: 0 }),
        );
        assert!(matches!(result, Err(CommandError::Unsupported(_))));
    }

    #[test]
    fn automatic_fan_modes_are_refused() {
        for mode in [FanMode::Auto, FanMode::Smart] {
            let result = command_to_config_write(
                &state_in_mode(1),
                EP_AIR_CONDITIONER,
                &ClusterCommand::FanControl(FanControlCommand::SetFanMode { mode }),
            );
            assert!(matches!(result, Err(CommandError::Unsupported(_))));
        }
    }

    #[test]
    fn a_humidity_setpoint_is_clamped_to_the_dry_mode_range() {
        let write = write_for_command(
            &state_in_mode(4),
            EP_AIR_CONDITIONER,
            ClusterCommand::DehumidificationControl(
                DehumidificationControlCommand::SetRhDehumidificationSetpoint { percent: 95 },
            ),
        );
        assert_eq!(write.cfg_humi_set, Some(80.0));
    }

    #[test]
    fn selecting_a_preset_writes_the_submode() {
        let write = write_for_command(
            &state_in_mode(1),
            EP_AIR_CONDITIONER,
            ClusterCommand::ModeSelect(ModeSelectCommand::ChangeToMode { new_mode: 4 }),
        );
        assert_eq!(write.cfg_wave_operating_submode, Some(4));
    }

    #[test]
    fn an_unknown_preset_is_refused() {
        let result = command_to_config_write(
            &state_in_mode(1),
            EP_AIR_CONDITIONER,
            &ClusterCommand::ModeSelect(ModeSelectCommand::ChangeToMode { new_mode: 9 }),
        );
        assert!(matches!(result, Err(CommandError::Unsupported(_))));
    }

    #[test]
    fn the_beeper_command_carries_the_flag_straight_through() {
        let write = write_for_command(
            &state_in_mode(1),
            EP_BEEPER,
            ClusterCommand::OnOff(OnOffCommand::On),
        );
        assert_eq!(write.en_beep, Some(1));

        let write = write_for_command(
            &state_in_mode(1),
            EP_BEEPER,
            ClusterCommand::OnOff(OnOffCommand::Off),
        );
        assert_eq!(write.en_beep, Some(0));
    }

    #[test]
    fn drainage_and_pet_care_map_to_their_own_config_fields() {
        let write = write_for_command(
            &state_in_mode(1),
            EP_DRAINAGE,
            ClusterCommand::OnOff(OnOffCommand::On),
        );
        assert_eq!(write.cfg_drainage_mode, Some(1));

        let write = write_for_command(
            &state_in_mode(1),
            EP_PET_CARE,
            ClusterCommand::OnOff(OnOffCommand::Off),
        );
        assert_eq!(write.cfg_en_pet_care, Some(false));
    }

    #[test]
    fn panel_brightness_converts_from_the_matter_level_scale() {
        let write = write_for_command(
            &state_in_mode(1),
            EP_PANEL,
            ClusterCommand::LevelControl(LevelControlCommand::MoveToLevel {
                level: 254,
                transition_time: None,
            }),
        );
        assert_eq!(write.lcd_light, Some(100));

        let write = write_for_command(
            &state_in_mode(1),
            EP_PANEL,
            ClusterCommand::LevelControl(LevelControlCommand::MoveToLevel {
                level: 127,
                transition_time: None,
            }),
        );
        assert_eq!(write.lcd_light, Some(50));
    }

    #[test]
    fn a_cluster_on_the_wrong_endpoint_is_refused() {
        // Thermostat lives on endpoint 1, not on the panel.
        let result = writes_to_config_write(
            &state_in_mode(1),
            EP_PANEL,
            &[ClusterWrite::Thermostat(ThermostatWrite {
                system_mode: Some(SystemMode::Cool),
                ..Default::default()
            })],
        );
        assert!(matches!(
            result,
            Err(CommandError::UnsupportedOnEndpoint { .. })
        ));
    }

    #[test]
    fn a_command_to_a_read_only_endpoint_is_refused() {
        let result = command_to_config_write(
            &state_in_mode(1),
            EP_TEMP_CONDENSER,
            &ClusterCommand::OnOff(OnOffCommand::On),
        );
        assert!(matches!(
            result,
            Err(CommandError::UnsupportedOnEndpoint { .. })
        ));
    }
}
