//! Field numbers for the four EcoFlow Wave 3 payload messages.
//!
//! This module is the transcribed schema and nothing else: constants are
//! declared for the fields hearthd reads or writes, and the doc tables below
//! record the rest so the knowledge is not lost when a field is later needed.
//! How those fields are decoded, and which of them reach hearthd's data model,
//! is decided in `codec` and `matter`.
//!
//! Field numbers mean something only within one message. Field 209 is
//! `plug_in_info_ac_in_chg_pow_max` in a display upload and
//! `cfg_power_off_delay_set` in a config write; field 4 is `pow_out_sum_w` in
//! the former and `cfg_main_power` in the latter. Hence one module per message
//! and no shared constants between them.
//!
//! Every field is `optional` with explicit presence. An absent field means
//! "not reported in this frame", never "zero" — that distinction is the whole
//! basis of the delta merge in `super::state`.
//!
//! Naming is EcoFlow's, inconsistencies included: `enBeep` and `lcdLight` sit
//! alongside `cfg_drainage_mode` in the same message. Only field numbers
//! travel, so this is cosmetic.
//!
//! # Attribution
//!
//! EcoFlow does not document this protocol. Every field number and type in
//! this module was reverse-engineered by the `tolwi/hassio-ecoflow-cloud` Home
//! Assistant custom component (Apache-2.0), read at commit `a7ebbba`, and
//! transcribed from
//! `custom_components/ecoflow_cloud/devices/internal/proto/wave3.proto`.
//! Message names have been shortened; the numbers and wire types are unchanged
//! because they are what the device requires.
//!
//! What that project provided is knowledge of the wire format. The Rust that
//! consumes these tables is original to hearthd. No code was copied.

/// `ConfigWrite`, the payload of `cmd_id` 17 — the only message the client
/// sends.
///
/// Set only the fields you intend to change: the device applies what is
/// present and ignores the rest. Several fields may be combined in one write,
/// and a mode change does exactly that.
///
/// # Climate control
///
/// | Field | # | Type | Meaning |
/// | --- | --- | --- | --- |
/// | `cfg_main_power` | 4 | bool | true powers the unit up out of standby |
/// | `cfg_sys_pause` | 172 | bool | true puts the unit into standby — this is "off" |
/// | `cfg_wave_operating_mode` | 153 | uint32 | operating mode |
/// | `cfg_wave_operating_submode` | 154 | uint32 | preset within the mode |
/// | `cfg_airflow_speed` | 155 | uint32 | fan speed percentage: 20/40/60/80/100 |
/// | `cfg_temp_set` | 156 | float | target temperature, C, 16.0-30.0 |
/// | `cfg_humi_set` | 157 | float | target relative humidity, %, 40-80, dry mode |
/// | `cfg_temp_thermostatic_upper_limit` | 158 | float | upper bound, C, auto mode |
/// | `cfg_temp_thermostatic_lower_limit` | 159 | float | lower bound, C, auto mode |
///
/// # Water and unit configuration
///
/// | Field | # | Type | Meaning |
/// | --- | --- | --- | --- |
/// | `cfg_drainage_mode` | 160 | uint32 | 1 enables automatic condensate drainage |
/// | `enBeep` | 9 | int32 | beeper: 0 silent, 1 audible |
/// | `lcdLight` | 14 | int32 | panel brightness, 0-100 |
/// | `screenOffTime` | 12 | int32 | screen blank timeout in seconds; app offers 0, 10, 30, 60, 300, 600 |
/// | `devStandbyTime` | 13 | int32 | auto-standby timeout in minutes; app offers 0, 30, 60, 120, 240, 360, 720, 1440 |
/// | `cfg_power_off_delay_set` | 209 | uint32 | auto-off countdown in minutes; app offers 0, 30, 60, 120, 180, 240, 360, 480, 720, 1440 |
/// | `cfg_mood_light_mode` | 161 | uint32 | ambient light mode |
/// | `cfg_lcd_show_temp_type` | 162 | uint32 | which temperature the panel displays |
/// | `cfg_user_temp_unit` | 166 | enum | 0 unset, 1 Celsius, 2 Fahrenheit — display only |
/// | `cfg_en_pet_care` | 163 | bool | pet-care mode |
/// | `cfg_temp_pet_care_warning` | 164 | float | pet-care alarm threshold, C |
///
/// # Telemetry cadence and snapshots
///
/// | Field | # | Type | Meaning |
/// | --- | --- | --- | --- |
/// | `active_display_property_full_upload` | 71 | bool | ask for a full display upload now |
/// | `active_runtime_property_full_upload` | 72 | bool | ask for a full runtime upload now |
/// | `cfg_display_property_full_upload_period` | 67 | int32 | ms between full display uploads |
/// | `cfg_display_property_incremental_upload_period` | 68 | int32 | ms between incremental display uploads |
/// | `cfg_runtime_property_full_upload_period` | 69 | int32 | ms between full runtime uploads |
/// | `cfg_runtime_property_incremental_upload_period` | 70 | int32 | ms between incremental runtime uploads |
///
/// # Fields present but unused here
///
/// Clock and timezone (`cfg_utc_time` 6, `cfg_utc_timezone` 7,
/// `cfg_utc_timezone_id` 135, `cfg_utc_set_mode` 136), battery SoC limits
/// (`cmsMaxChgSoc` 33, `cmsMinDsgSoc` 34), AC input charge power limit
/// (`cfg_plug_in_info_ac_in_chg_pow_max` 54), PV input current limit
/// (`cfg_plug_in_info_pv_dc_amp_max` 87), scheduled tasks (`set_time_task` 39,
/// `cfg_time_task_v2_item` 127, `active_selected_time_task_v2` 128), BMS push
/// configuration (`cfg_bms_push` 32) and SoC calibration (`cfg_soc_cali` 31).
///
/// `cfgPowerOff` (3) looks like a power command but the app does not use it
/// for this device; `cfg_sys_pause` is the tested path. Leave it alone.
///
/// hearthd does not implement scheduled tasks: their message shapes are known
/// but the behaviour is untested, and hearthd schedules automations itself.
pub mod config_write {
    pub const CFG_MAIN_POWER: u32 = 4;
    /// Inverted: 0 is audible, 1 is muted.
    pub const EN_BEEP: u32 = 9;
    pub const LCD_LIGHT: u32 = 14;
    pub const ACTIVE_DISPLAY_PROPERTY_FULL_UPLOAD: u32 = 71;
    pub const ACTIVE_RUNTIME_PROPERTY_FULL_UPLOAD: u32 = 72;
    pub const CFG_WAVE_OPERATING_MODE: u32 = 153;
    pub const CFG_WAVE_OPERATING_SUBMODE: u32 = 154;
    pub const CFG_AIRFLOW_SPEED: u32 = 155;
    pub const CFG_TEMP_SET: u32 = 156;
    pub const CFG_HUMI_SET: u32 = 157;
    pub const CFG_TEMP_THERMOSTATIC_UPPER_LIMIT: u32 = 158;
    pub const CFG_TEMP_THERMOSTATIC_LOWER_LIMIT: u32 = 159;
    pub const CFG_DRAINAGE_MODE: u32 = 160;
    pub const CFG_EN_PET_CARE: u32 = 163;
    pub const CFG_USER_TEMP_UNIT: u32 = 166;
    pub const CFG_SYS_PAUSE: u32 = 172;
}

/// `ConfigWriteAck`, the payload of `cmd_id` 18.
///
/// Mirrors `ConfigWrite`'s field numbering — field 156 is still `cfg_temp_set`
/// — and adds two fields of its own:
///
/// | Field | # | Type | Meaning |
/// | --- | --- | --- | --- |
/// | `actionId` | 1 | int32 | |
/// | `configOk` | 2 | bool | whether the write was accepted |
///
/// An ack confirms receipt, not effect. The authoritative state is always the
/// next property upload.
pub mod config_write_ack {
    pub const ACTION_ID: u32 = 1;
    pub const CONFIG_OK: u32 = 2;
}

/// `DisplayPropertyUpload`, the payload of `cmd_id` 1 and 21.
///
/// # Climate
///
/// | Field | # | Type | Unit / meaning |
/// | --- | --- | --- | --- |
/// | `temp_ambient` | 484 | float | C — room temperature |
/// | `humi_ambient` | 485 | float | % RH — room humidity |
/// | `wave_operating_mode` | 486 | uint32 | current mode |
/// | `dev_sleep_state` | 212 | uint32 | 1 means standby |
/// | `wave_mode_info` | 514 | message | per-mode saved parameters |
/// | `temp_indoor_supply_air` | 494 | float | C — air leaving the unit |
///
/// # Water
///
/// | Field | # | Type | Unit / meaning |
/// | --- | --- | --- | --- |
/// | `condensate_water_level` | 504 | float | % tank level |
/// | `in_drainage` | 505 | bool | drain cycle running now |
/// | `drainage_mode` | 506 | uint32 | auto-drain configured |
///
/// # Power (all watts as floats, unscaled)
///
/// | Field | # | Unit / meaning |
/// | --- | --- | --- |
/// | `pow_in_sum_w` | 3 | W — total input |
/// | `pow_out_sum_w` | 4 | W — total output |
/// | `pow_get_ac` | 53 | W — AC output |
/// | `pow_get_ac_in` | 54 | W — AC input |
/// | `pow_get_bms` | 158 | W — battery; sign indicates direction |
/// | `pow_get_pv` | 361 | W — solar input |
/// | `pow_get_dcp` | 425 | W — DC port |
/// | `pow_get_self_consume` | 777 | W — self consumption |
/// | `pow_get_qcusb1` | 9 | W — USB-A output |
/// | `pow_get_typec1` | 11 | W — USB-C output |
///
/// The sign convention of `pow_get_bms` is assumed positive-in, negative-out
/// and is unconfirmed against hardware.
///
/// # Battery
///
/// | Field | # | Type | Unit / meaning |
/// | --- | --- | --- | --- |
/// | `bms_batt_soc` | 242 | float | % charge — the headline battery level |
/// | `bms_batt_soh` | 243 | float | % state of health |
/// | `bms_dsg_rem_time` | 254 | uint32 | minutes of discharge remaining |
/// | `bms_chg_rem_time` | 255 | uint32 | minutes to full charge |
/// | `bms_design_cap` | 248 | uint32 | design capacity, mAh assumed, unconfirmed |
/// | `bms_min_cell_temp` / `bms_max_cell_temp` | 258 / 259 | int32 | C |
/// | `bms_min_mos_temp` / `bms_max_mos_temp` | 260 / 261 | int32 | C |
/// | `cms_batt_soc` / `cms_batt_soh` | 262 / 263 | float | % — system-level view |
/// | `cms_dsg_rem_time` / `cms_chg_rem_time` | 268 / 269 | uint32 | minutes |
/// | `cms_max_chg_soc` / `cms_min_dsg_soc` | 270 / 271 | uint32 | % charge limits |
/// | `bms_chg_dsg_state` / `cms_chg_dsg_state` | 281 / 282 | uint32 | charge/discharge state |
/// | `bms_main_sn` | 392 | string | battery pack serial |
///
/// The remaining-time fields report implausible values — very large, or a
/// sentinel — when the unit is neither charging nor discharging.
///
/// # Panel and unit configuration
///
/// | Field | # | Type | Unit / meaning |
/// | --- | --- | --- | --- |
/// | `en_beep` | 195 | bool | true when the beeper is audible |
/// | `lcd_light` | 5 | uint32 | % brightness |
/// | `screen_off_time` | 18 | uint32 | seconds |
/// | `dev_standby_time` | 17 | uint32 | minutes |
/// | `power_off_delay_set` | 778 | uint32 | minutes configured |
/// | `power_off_delay_remaining` | 779 | uint32 | minutes left on the countdown |
/// | `mood_light_mode` | 507 | uint32 | |
/// | `lcd_show_temp_type` | 508 | uint32 | |
/// | `user_temp_unit` | 512 | enum | 0 unset, 1 C, 2 F — display only |
/// | `en_pet_care` | 509 | bool | |
/// | `temp_pet_care_warning` | 510 | float | C |
/// | `pet_care_warning` | 513 | bool | alarm currently raised |
///
/// # Diagnostics and inputs
///
/// | Field | # | Type | Meaning |
/// | --- | --- | --- | --- |
/// | `errcode` | 1 | uint32 | general error code |
/// | `bms_err_code` | 140 | uint32 | battery error code |
/// | `pd_err_code` | 213 | uint32 | power-distribution error code |
/// | `dev_errcode_list` | 627 | message | 1: repeated uint32 `dev_errcode`, packed |
/// | `plug_in_info_ac_in_flag` | 61 | uint32 | AC input present |
/// | `plug_in_info_ac_in_feq` | 62 | uint32 | AC input frequency, whole Hz assumed |
/// | `plug_in_info_ac_charger_flag` | 202 | bool | AC charger attached |
/// | `plug_in_info_ac_in_chg_pow_max` | 209 | uint32 | W limit |
/// | `plug_in_info_ac_out_dsg_pow_max` | 238 | uint32 | W limit |
/// | `plug_in_info_ac_in_chg_hal_pow_max` | 458 | uint32 | W limit |
/// | `plug_in_info_pv_charger_flag` | 364 | bool | PV attached |
/// | `plug_in_info_pv_type` | 363 | uint32 | |
/// | `plug_in_info_pv_chg_amp_max` / `_vol_max` | 365 / 366 | uint32 | |
/// | `plug_in_info_pv_dc_amp_max` | 356 | uint32 | |
/// | `plug_in_info_dcp_in_flag` | 426 | bool | DC port input present |
/// | `plug_in_info_dcp_charger_flag` | 435 | bool | |
/// | `plug_in_info_dcp_type` / `_detail` | 427 / 428 | uint32 | |
/// | `plug_in_info_dcp_dsg_chg_type` | 431 | uint32 | |
/// | `plug_in_info_dcp_sn` | 433 | string | |
/// | `plug_in_info_dcp_firm_ver` | 434 | uint32 | |
/// | `plug_in_info_dcp_run_state` | 436 | uint32 | |
/// | `plug_in_info_dcp_err_code` | 438 | uint32 | |
/// | `plug_in_info_dcp_resv` | 432 | message | 1: repeated uint32 `resv_info`, packed |
/// | `flow_info_*` | 13, 15, 45, 47, 152, 153, 360, 423, 424 | uint32 | per-port energy-flow direction |
/// | `pcs_fan_level` | 30 | uint32 | internal converter fan level |
/// | `cms_bms_run_state` | 275 | uint32 | |
/// | `utc_timezone` / `utc_timezone_id` / `utc_set_mode` | 133 / 134 / 135 | int32 / string / bool | |
/// | `current_time_task_v2_item` | 126 | message | active scheduled task |
/// | `time_task_conflict_flag` / `time_task_change_cnt` | 285 / 286 | uint32 | |
/// # Observed on a Wave 3
///
/// The fields below marked as not sent were absent from every frame a real
/// unit produced over an hour of telemetry. They are still documented above,
/// because absence on one firmware is not proof they are never used, but
/// hearthd does not decode them and does not expose an endpoint that would
/// only ever be null:
///
/// - `pow_in_sum_w` (3) and `pow_out_sum_w` (4) — the aggregate totals.
/// - `pow_get_ac_in` (54). `pow_get_ac` (53) is the one that arrives, and it
///   matches the unit's measured draw: 484.00888 W against 484 W in the app,
///   on a rail independently decoded as 243.0 V and 2.0 A. The Wave 3 has no
///   AC outlet, so 53 is consumption; the schema carries both names because it
///   is shared with EcoFlow power stations, which do invert.
/// - `pow_get_qcusb1` (9) and `pow_get_typec1` (11) — this device has no USB
///   ports.
/// - `pow_get_dcp` (425), though the DC port's flags and identity (426, 427,
///   428, 433) do arrive.
/// - `plug_in_info_ac_charger_flag` (202). `plug_in_info_ac_in_flag` (61) is
///   what reports mains presence.
/// - `plug_in_info_ac_in_feq` (62).
pub mod display {
    pub const LCD_LIGHT: u32 = 5;
    pub const POW_GET_AC: u32 = 53;
    pub const PLUG_IN_INFO_AC_IN_FLAG: u32 = 61;
    pub const POW_GET_BMS: u32 = 158;
    /// Inverted mute flag: false is audible, true is muted.
    pub const EN_BEEP: u32 = 195;
    pub const DEV_SLEEP_STATE: u32 = 212;
    pub const BMS_BATT_SOC: u32 = 242;
    pub const BMS_DSG_REM_TIME: u32 = 254;
    pub const BMS_CHG_REM_TIME: u32 = 255;
    pub const BMS_CHG_DSG_STATE: u32 = 281;
    pub const POW_GET_PV: u32 = 361;
    pub const PLUG_IN_INFO_DCP_IN_FLAG: u32 = 426;
    pub const TEMP_AMBIENT: u32 = 484;
    pub const HUMI_AMBIENT: u32 = 485;
    pub const WAVE_OPERATING_MODE: u32 = 486;
    pub const TEMP_INDOOR_SUPPLY_AIR: u32 = 494;
    pub const IN_DRAINAGE: u32 = 505;
    pub const DRAINAGE_MODE: u32 = 506;
    pub const EN_PET_CARE: u32 = 509;
    pub const USER_TEMP_UNIT: u32 = 512;
    pub const PET_CARE_WARNING: u32 = 513;
    pub const WAVE_MODE_INFO: u32 = 514;
}

/// `WaveOperatingModeParamList`, the message carried in display field 514, and
/// its repeated item.
///
/// ```text
/// WaveOperatingModeParamList {
///   1: repeated WaveOperatingModeParamItem list_info
/// }
///
/// WaveOperatingModeParamItem {
///   1: uint32 submode
///   2: uint32 airflow_speed
///   3: float  temp_set
///   4: float  humi_set
///   5: float  temp_thermostatic_upper_limit
///   6: float  temp_thermostatic_lower_limit
/// }
/// ```
///
/// The Wave 3 does not report a single "current target temperature". It
/// reports the saved parameters for *every* mode and expects the reader to
/// index that list with the current mode. See `super::semantics` for the
/// indexing rule.
pub mod mode_param {
    /// Repeated `WaveOperatingModeParamItem` inside the list message.
    pub const LIST_INFO: u32 = 1;

    pub const SUBMODE: u32 = 1;
    pub const AIRFLOW_SPEED: u32 = 2;
    pub const TEMP_SET: u32 = 3;
    pub const HUMI_SET: u32 = 4;
    pub const TEMP_THERMOSTATIC_UPPER_LIMIT: u32 = 5;
    pub const TEMP_THERMOSTATIC_LOWER_LIMIT: u32 = 6;
}

/// `RuntimePropertyUpload`, the payload of `cmd_id` 22.
///
/// Lower-level engineering data on its own cadence.
///
/// # Refrigeration circuit temperatures (all float, C)
///
/// | Field | # |
/// | --- | --- |
/// | `temp_condenser` | 496 |
/// | `temp_evaporator` | 499 |
/// | `temp_compressor_discharge` | 503 |
/// | `temp_indoor_return_air` | 493 — not return air; see below |
/// | `temp_outdoor_ambient` | 495 |
/// | `temp_pv` | 379 |
/// | `temp_pcs_dc` / `temp_pcs_ac` | 26 / 27 |
///
/// Condenser and evaporator are the useful diagnostic pair: together they say
/// whether the compressor is actually doing work, which the operating mode
/// alone does not.
///
/// **Field 493 is not return air.** On a real unit it reports the same value,
/// bit for bit, as `temp_indoor_supply_air` (display field 494) — 14.740002 in
/// both — while the room was at 28.5 C. Intake air cannot be 14 C in a 28 C
/// room, and the app labels 494 "air outlet temperature". Whatever 493 is, it
/// is a second copy of the outlet sensor, so hearthd does not surface it.
///
/// # Electrical
///
/// | Field | # | Type | Unit |
/// | --- | --- | --- | --- |
/// | `plug_in_info_ac_in_vol` | 68 | float | V |
/// | `plug_in_info_ac_in_amp` | 223 | float | A |
/// | `plug_in_info_pv_vol` / `_amp` | 380 / 381 | float | V / A |
/// | `plug_in_info_dcp_vol` / `_amp` | 443 / 448 | float | V / A |
/// | `plug_in_info_bms_vol` | 169 | float | V |
/// | `bms_batt_vol` / `bms_batt_amp` | 244 / 245 | float | V / A |
/// | `cms_batt_vol` / `cms_batt_amp` | 264 / 265 | float | V / A |
/// | `cms_chg_req_vol` / `cms_chg_req_amp` | 266 / 267 | float | V / A |
/// | `bms_min_cell_vol` / `bms_max_cell_vol` | 256 / 257 | uint32 | mV assumed, unconfirmed |
/// | `bms_full_cap` / `bms_remain_cap` | 247 / 249 | uint32 | mAh assumed, unconfirmed |
///
/// # Status, faults and firmware
///
/// | Field | # | Type | Meaning |
/// | --- | --- | --- | --- |
/// | `bms_bal_state` | 246 | uint32 | cell balancing |
/// | `bms_alm_state` / `bms_alm_state_2` | 250 / 291 | uint32 | alarm bitfields |
/// | `bms_pro_state` / `bms_pro_state_2` | 251 / 292 | uint32 | protection bitfields |
/// | `bms_flt_state` | 252 | uint32 | fault bitfield |
/// | `bms_err_code` | 253 | uint32 | |
/// | `bms_overload_icon` etc. | 276-280 | uint32 | panel warning indicators |
/// | `pd_mppt_comm_err` etc. | 172-175 | int32 | internal bus comms failures |
/// | `pd_firm_ver` etc. | 176-179, 241 | uint32 | packed firmware versions |
/// | `display_property_full_upload_period` | 293 | int32 | ms — current cadence |
/// | `display_property_incremental_upload_period` | 294 | int32 | ms |
/// | `runtime_property_full_upload_period` | 295 | int32 | ms |
/// | `runtime_property_incremental_upload_period` | 296 | int32 | ms |
///
/// These are milliseconds, not seconds. A real unit reports 120000, 2000,
/// 300000 and 60000, which as seconds would be 33 hours and 16 hours; as
/// milliseconds they are a 120 s full display upload, 2 s incremental, 300 s
/// full runtime and 60 s incremental. Observed cadence does not match those
/// figures either — see the note on telemetry in `super`.
///
/// The alarm, protection and fault fields are bitfields whose bit meanings are
/// not known. They would have to be surfaced as opaque integers; hearthd does
/// not surface them at all rather than invent bit names.
pub mod runtime {
    pub const PLUG_IN_INFO_AC_IN_VOL: u32 = 68;
    pub const PLUG_IN_INFO_AC_IN_AMP: u32 = 223;
    pub const BMS_BATT_VOL: u32 = 244;
    pub const BMS_BATT_AMP: u32 = 245;
    pub const PLUG_IN_INFO_PV_VOL: u32 = 380;
    pub const PLUG_IN_INFO_PV_AMP: u32 = 381;
    pub const PLUG_IN_INFO_DCP_VOL: u32 = 443;
    pub const PLUG_IN_INFO_DCP_AMP: u32 = 448;
    pub const TEMP_OUTDOOR_AMBIENT: u32 = 495;
    pub const TEMP_CONDENSER: u32 = 496;
    pub const TEMP_EVAPORATOR: u32 = 499;
    pub const TEMP_COMPRESSOR_DISCHARGE: u32 = 503;
}
