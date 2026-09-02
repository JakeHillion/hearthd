//! Per-device state cache.
//!
//! Property uploads are sparse deltas: a frame carries only the fields that
//! changed, so no single frame describes the device. This module keeps the
//! merged view that everything above the protocol reads from.
//!
//! Three rules, all hearthd's own design:
//!
//! - **Merge, never replace.** A field absent from a frame leaves the cached
//!   value untouched. This applies recursively to the active mode parameters.
//! - **No fabricated defaults.** A value hearthd has never been told stays
//!   `None`. Substituting a plausible 22.0 C or 50 % would produce a reading
//!   that silently disagrees with the hardware, which is worse than admitting
//!   ignorance.
//! - **Freshness is the device's to prove.** The MQTT session is long-lived
//!   and clean, so "connected" says nothing about whether a device is alive.
//!   Only the arrival of a frame does.

use std::time::Duration;
use std::time::Instant;

use super::codec::ConfigWrite;
use super::codec::DisplayProperties;
use super::codec::ModeParamItem;
use super::codec::RuntimeProperties;
use super::semantics;

/// Copy each `Some` field of `$src` over `$dst`, leaving `None` fields alone.
macro_rules! merge_copy_fields {
    ($dst:expr, $src:expr, $($field:ident),+ $(,)?) => {
        $(
            if let Some(value) = $src.$field {
                $dst.$field = Some(value);
            }
        )+
    };
}

/// The merged view of one Wave 3.
#[derive(Debug, Default, Clone)]
pub struct DeviceState {
    display: DisplayProperties,
    runtime: RuntimeProperties,
    /// Parameters of the currently selected mode, merged field by field.
    ///
    /// The device reports saved parameters for every mode at once; this is the
    /// entry that the current mode selects, accumulated across frames.
    active_params: ModeParamItem,
    last_frame_at: Option<Instant>,
}

impl DeviceState {
    pub fn display(&self) -> &DisplayProperties {
        &self.display
    }

    pub fn runtime(&self) -> &RuntimeProperties {
        &self.runtime
    }

    pub fn active_params(&self) -> &ModeParamItem {
        &self.active_params
    }

    /// Merge a display-property delta.
    ///
    /// `cmd_id` 1 and 21 both arrive here: whichever of them is the full
    /// upload, both are deltas and merge identically.
    pub fn apply_display(&mut self, delta: DisplayProperties, now: Instant) {
        // Resolve the active mode parameters before the delta's own
        // `mode_params` is folded in, using this frame's mode if it carries
        // one and the cached mode otherwise.
        let mode = delta
            .wave_operating_mode
            .or(self.display.wave_operating_mode);

        if let (Some(mode), Some(list)) = (mode, delta.mode_params.as_ref()) {
            if let Some(index) = semantics::active_mode_param_index(mode, list.len()) {
                merge_copy_fields!(
                    self.active_params,
                    list[index],
                    submode,
                    airflow_speed,
                    temp_set,
                    humi_set,
                    temp_thermostatic_upper_limit,
                    temp_thermostatic_lower_limit,
                );
            }
        }

        merge_copy_fields!(
            self.display,
            delta,
            temp_ambient,
            humi_ambient,
            wave_operating_mode,
            dev_sleep_state,
            temp_indoor_supply_air,
            in_drainage,
            drainage_mode,
            pow_get_ac,
            pow_get_bms,
            pow_get_pv,
            bms_batt_soc,
            bms_dsg_rem_time,
            bms_chg_rem_time,
            bms_chg_dsg_state,
            en_beep,
            lcd_light,
            user_temp_unit,
            en_pet_care,
            pet_care_warning,
            plug_in_info_ac_in_flag,
            plug_in_info_dcp_in_flag,
        );

        if let Some(list) = delta.mode_params {
            self.display.mode_params = Some(list);
        }

        self.last_frame_at = Some(now);
    }

    /// Merge a runtime-property delta.
    pub fn apply_runtime(&mut self, delta: RuntimeProperties, now: Instant) {
        merge_copy_fields!(
            self.runtime,
            delta,
            temp_outdoor_ambient,
            temp_condenser,
            temp_evaporator,
            temp_compressor_discharge,
            plug_in_info_ac_in_vol,
            plug_in_info_ac_in_amp,
            plug_in_info_pv_vol,
            plug_in_info_pv_amp,
            plug_in_info_dcp_vol,
            plug_in_info_dcp_amp,
            bms_batt_vol,
            bms_batt_amp,
        );

        self.last_frame_at = Some(now);
    }

    /// Apply a command's own values to the cache immediately after a
    /// successful publish, so readers do not lag a full upload period.
    ///
    /// These values are *not* confirmed. The next property upload overwrites
    /// them, and if no upload either confirms or contradicts them within a
    /// couple of upload periods the command was probably lost. Note this
    /// deliberately does not touch `last_frame_at`: an optimistic write is not
    /// evidence that the device is alive.
    pub fn apply_optimistic(&mut self, write: &ConfigWrite) {
        // Standby is its own axis, so pausing must not touch the operating
        // mode. Clearing it here left nothing to restore when the unit was
        // switched back on — `cfg_main_power` carries no mode — and hearthd
        // went on reporting a running unit as off until the next upload,
        // which can be hours away.
        if write.cfg_sys_pause == Some(true) {
            self.display.dev_sleep_state = Some(semantics::SLEEP_STATE_STANDBY);
        }
        if write.cfg_main_power == Some(true) {
            self.display.dev_sleep_state = Some(0);
        }

        if let Some(mode) = write.cfg_wave_operating_mode {
            self.display.wave_operating_mode = Some(mode);
        }
        if let Some(mode) = write.cfg_drainage_mode {
            self.display.drainage_mode = Some(mode);
        }
        if let Some(unit) = write.cfg_user_temp_unit {
            self.display.user_temp_unit = Some(unit);
        }
        if let Some(enabled) = write.cfg_en_pet_care {
            self.display.en_pet_care = Some(enabled);
        }
        if let Some(beep) = write.en_beep {
            // The config field is an int32 and the display field a bool.
            self.display.en_beep = Some(beep != 0);
        }
        if let Some(light) = write.lcd_light {
            self.display.lcd_light = u32::try_from(light).ok();
        }

        if let Some(submode) = write.cfg_wave_operating_submode {
            self.active_params.submode = Some(submode);
        }
        if let Some(speed) = write.cfg_airflow_speed {
            self.active_params.airflow_speed = Some(speed);
        }
        if let Some(temp) = write.cfg_temp_set {
            self.active_params.temp_set = Some(temp);
        }
        if let Some(humi) = write.cfg_humi_set {
            self.active_params.humi_set = Some(humi);
        }
        if let Some(upper) = write.cfg_temp_thermostatic_upper_limit {
            self.active_params.temp_thermostatic_upper_limit = Some(upper);
        }
        if let Some(lower) = write.cfg_temp_thermostatic_lower_limit {
            self.active_params.temp_thermostatic_lower_limit = Some(lower);
        }
    }

    /// Whether any frame has ever arrived for this device.
    pub fn has_data(&self) -> bool {
        self.last_frame_at.is_some()
    }

    /// Whether the device has gone quiet for longer than `timeout`.
    ///
    /// A device that has never reported is stale: serving an empty state as
    /// though it were current would be the same mistake as serving an old one.
    pub fn is_stale(&self, now: Instant, timeout: Duration) -> bool {
        match self.last_frame_at {
            Some(seen) => now.duration_since(seen) >= timeout,
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> Instant {
        Instant::now()
    }

    #[test]
    fn absent_fields_do_not_clobber_cached_values() {
        let mut state = DeviceState::default();

        state.apply_display(
            DisplayProperties {
                temp_ambient: Some(21.5),
                humi_ambient: Some(55.0),
                ..Default::default()
            },
            now(),
        );

        // An incremental frame carrying only the temperature.
        state.apply_display(
            DisplayProperties {
                temp_ambient: Some(22.0),
                ..Default::default()
            },
            now(),
        );

        assert_eq!(state.display().temp_ambient, Some(22.0));
        assert_eq!(state.display().humi_ambient, Some(55.0));
    }

    #[test]
    fn a_reported_zero_overwrites_a_previous_value() {
        let mut state = DeviceState::default();
        state.apply_display(
            DisplayProperties {
                pow_get_ac: Some(850.0),
                ..Default::default()
            },
            now(),
        );
        state.apply_display(
            DisplayProperties {
                pow_get_ac: Some(0.0),
                ..Default::default()
            },
            now(),
        );
        assert_eq!(state.display().pow_get_ac, Some(0.0));
    }

    #[test]
    fn nothing_is_invented_before_the_device_reports() {
        let state = DeviceState::default();
        assert_eq!(state.display().temp_ambient, None);
        assert_eq!(state.active_params().temp_set, None);
        assert_eq!(state.active_params().humi_set, None);
        assert!(!state.has_data());
    }

    /// A six-entry list indexed by mode, as the device sends it.
    fn mode_list() -> Vec<ModeParamItem> {
        vec![
            // 0: off, carries nothing useful.
            ModeParamItem::default(),
            // 1: cool
            ModeParamItem {
                submode: Some(0),
                airflow_speed: Some(40),
                temp_set: Some(22.0),
                ..Default::default()
            },
            // 2: heat
            ModeParamItem {
                submode: Some(3),
                airflow_speed: Some(60),
                temp_set: Some(26.0),
                ..Default::default()
            },
            ModeParamItem::default(),
            // 4: dry
            ModeParamItem {
                humi_set: Some(55.0),
                ..Default::default()
            },
            // 5: auto
            ModeParamItem {
                temp_thermostatic_upper_limit: Some(24.0),
                temp_thermostatic_lower_limit: Some(20.0),
                ..Default::default()
            },
        ]
    }

    #[test]
    fn active_parameters_are_indexed_by_the_current_mode() {
        let mut state = DeviceState::default();
        state.apply_display(
            DisplayProperties {
                wave_operating_mode: Some(1),
                mode_params: Some(mode_list()),
                ..Default::default()
            },
            now(),
        );

        assert_eq!(state.active_params().temp_set, Some(22.0));
        assert_eq!(state.active_params().airflow_speed, Some(40));
    }

    #[test]
    fn switching_mode_swaps_the_whole_active_parameter_set() {
        let mut state = DeviceState::default();
        state.apply_display(
            DisplayProperties {
                wave_operating_mode: Some(1),
                mode_params: Some(mode_list()),
                ..Default::default()
            },
            now(),
        );
        assert_eq!(state.active_params().temp_set, Some(22.0));

        // Switching cool to heat makes the setpoint jump. That is correct: the
        // unit stores per-mode setpoints.
        state.apply_display(
            DisplayProperties {
                wave_operating_mode: Some(2),
                mode_params: Some(mode_list()),
                ..Default::default()
            },
            now(),
        );
        assert_eq!(state.active_params().temp_set, Some(26.0));
        assert_eq!(state.active_params().submode, Some(3));
    }

    #[test]
    fn the_mode_comes_from_the_cache_when_a_frame_omits_it() {
        let mut state = DeviceState::default();
        state.apply_display(
            DisplayProperties {
                wave_operating_mode: Some(2),
                ..Default::default()
            },
            now(),
        );
        // A later frame carries the list but not the mode.
        state.apply_display(
            DisplayProperties {
                mode_params: Some(mode_list()),
                ..Default::default()
            },
            now(),
        );
        assert_eq!(state.active_params().temp_set, Some(26.0));
    }

    #[test]
    fn mode_zero_and_out_of_range_modes_leave_active_parameters_alone() {
        let mut state = DeviceState::default();
        state.apply_display(
            DisplayProperties {
                wave_operating_mode: Some(1),
                mode_params: Some(mode_list()),
                ..Default::default()
            },
            now(),
        );
        assert_eq!(state.active_params().temp_set, Some(22.0));

        // Mode 0 indexes the "off" entry, which carries nothing.
        state.apply_display(
            DisplayProperties {
                wave_operating_mode: Some(0),
                mode_params: Some(mode_list()),
                ..Default::default()
            },
            now(),
        );
        assert_eq!(state.active_params().temp_set, Some(22.0));

        // A mode past the end of the list means the frame cannot resolve
        // settings at all.
        state.apply_display(
            DisplayProperties {
                wave_operating_mode: Some(9),
                mode_params: Some(mode_list()),
                ..Default::default()
            },
            now(),
        );
        assert_eq!(state.active_params().temp_set, Some(22.0));
    }

    #[test]
    fn active_parameters_merge_rather_than_replace() {
        let mut state = DeviceState::default();
        state.apply_display(
            DisplayProperties {
                wave_operating_mode: Some(1),
                mode_params: Some(mode_list()),
                ..Default::default()
            },
            now(),
        );

        // A later list whose active entry reports only the fan speed must not
        // wipe the cached setpoint.
        let mut sparse = mode_list();
        sparse[1] = ModeParamItem {
            airflow_speed: Some(100),
            ..Default::default()
        };
        state.apply_display(
            DisplayProperties {
                mode_params: Some(sparse),
                ..Default::default()
            },
            now(),
        );

        assert_eq!(state.active_params().airflow_speed, Some(100));
        assert_eq!(state.active_params().temp_set, Some(22.0));
    }

    #[test]
    fn runtime_deltas_merge_independently_of_display() {
        let mut state = DeviceState::default();
        state.apply_display(
            DisplayProperties {
                temp_ambient: Some(21.5),
                ..Default::default()
            },
            now(),
        );
        state.apply_runtime(
            RuntimeProperties {
                temp_condenser: Some(41.0),
                ..Default::default()
            },
            now(),
        );
        state.apply_runtime(
            RuntimeProperties {
                temp_evaporator: Some(8.0),
                ..Default::default()
            },
            now(),
        );

        assert_eq!(state.display().temp_ambient, Some(21.5));
        assert_eq!(state.runtime().temp_condenser, Some(41.0));
        assert_eq!(state.runtime().temp_evaporator, Some(8.0));
    }

    #[test]
    fn optimistic_pause_sets_standby_and_keeps_the_mode() {
        let mut state = DeviceState::default();
        state.apply_display(
            DisplayProperties {
                wave_operating_mode: Some(1),
                dev_sleep_state: Some(0),
                ..Default::default()
            },
            now(),
        );

        state.apply_optimistic(&ConfigWrite {
            cfg_sys_pause: Some(true),
            ..Default::default()
        });

        assert_eq!(state.display().dev_sleep_state, Some(1));
        // The mode survives, so switching back on restores cool rather than
        // leaving a running unit reported as off.
        assert_eq!(state.display().wave_operating_mode, Some(1));

        state.apply_optimistic(&ConfigWrite {
            cfg_main_power: Some(true),
            ..Default::default()
        });
        assert_eq!(state.display().dev_sleep_state, Some(0));
        assert_eq!(state.display().wave_operating_mode, Some(1));
    }

    #[test]
    fn optimistic_power_on_into_a_mode_sets_both_axes() {
        let mut state = DeviceState::default();
        state.apply_optimistic(&ConfigWrite {
            cfg_main_power: Some(true),
            cfg_wave_operating_mode: Some(2),
            ..Default::default()
        });

        assert_eq!(state.display().dev_sleep_state, Some(0));
        assert_eq!(state.display().wave_operating_mode, Some(2));
    }

    #[test]
    fn optimistic_setpoints_land_on_the_active_parameters() {
        let mut state = DeviceState::default();
        state.apply_optimistic(&ConfigWrite {
            cfg_temp_set: Some(24.0),
            cfg_airflow_speed: Some(80),
            en_beep: Some(1),
            ..Default::default()
        });

        assert_eq!(state.active_params().temp_set, Some(24.0));
        assert_eq!(state.active_params().airflow_speed, Some(80));
        assert_eq!(state.display().en_beep, Some(true));
    }

    #[test]
    fn a_real_report_overrides_an_optimistic_value() {
        let mut state = DeviceState::default();
        state.apply_optimistic(&ConfigWrite {
            cfg_temp_set: Some(24.0),
            ..Default::default()
        });
        assert_eq!(state.active_params().temp_set, Some(24.0));

        // The device says otherwise; the device wins.
        let mut list = mode_list();
        list[1].temp_set = Some(19.0);
        state.apply_display(
            DisplayProperties {
                wave_operating_mode: Some(1),
                mode_params: Some(list),
                ..Default::default()
            },
            now(),
        );
        assert_eq!(state.active_params().temp_set, Some(19.0));
    }

    #[test]
    fn an_optimistic_write_is_not_evidence_of_liveness() {
        let mut state = DeviceState::default();
        state.apply_optimistic(&ConfigWrite {
            cfg_temp_set: Some(24.0),
            ..Default::default()
        });
        assert!(!state.has_data());
        assert!(state.is_stale(Instant::now(), Duration::from_secs(60)));
    }

    #[test]
    fn staleness_tracks_the_last_frame() {
        let mut state = DeviceState::default();
        let start = Instant::now();

        // A device that has never reported is stale.
        assert!(state.is_stale(start, Duration::from_secs(60)));

        state.apply_display(DisplayProperties::default(), start);
        assert!(state.has_data());
        assert!(!state.is_stale(start + Duration::from_secs(30), Duration::from_secs(60)));
        assert!(state.is_stale(start + Duration::from_secs(90), Duration::from_secs(60)));
    }
}
