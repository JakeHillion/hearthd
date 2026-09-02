//! Decoding Wave 3 payload messages into Rust, and encoding config writes.
//!
//! The field numbers this consumes are transcribed in `super::fields` and
//! carry attribution there. The representation chosen here — which fields
//! hearthd bothers to decode, their Rust types, and the `Option` shape — is
//! hearthd's own design and carries none.
//!
//! Two rules run through everything below:
//!
//! - **Absent is not zero.** Every decoded field is an `Option`, and a field
//!   missing from a frame means "not reported", never "0". Property uploads
//!   are sparse deltas, so collapsing the distinction would overwrite good
//!   cached values with fabricated zeroes on every incremental upload.
//! - **Unknown fields are skipped, not rejected.** Firmware revisions add
//!   fields. A field whose wire type is not the one expected is skipped for
//!   the same reason, rather than failing the whole frame.

use super::Error;
use super::fields::config_write;
use super::fields::config_write_ack;
use super::fields::display;
use super::fields::mode_param;
use super::fields::runtime;
use crate::integrations::ecoflow::protobuf::Reader;
use crate::integrations::ecoflow::protobuf::WireType;
use crate::integrations::ecoflow::protobuf::Writer;

/// One entry of the per-mode saved-parameter list (display field 514).
///
/// Each item is itself sparse: a field absent from the active item means
/// unchanged, not zero.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ModeParamItem {
    pub submode: Option<u32>,
    pub airflow_speed: Option<u32>,
    pub temp_set: Option<f32>,
    pub humi_set: Option<f32>,
    pub temp_thermostatic_upper_limit: Option<f32>,
    pub temp_thermostatic_lower_limit: Option<f32>,
}

/// The subset of `DisplayPropertyUpload` hearthd surfaces.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct DisplayProperties {
    pub temp_ambient: Option<f32>,
    pub humi_ambient: Option<f32>,
    pub wave_operating_mode: Option<u32>,
    pub dev_sleep_state: Option<u32>,
    pub temp_indoor_supply_air: Option<f32>,
    pub mode_params: Option<Vec<ModeParamItem>>,

    pub in_drainage: Option<bool>,
    pub drainage_mode: Option<u32>,

    // Watts.
    pub pow_get_ac: Option<f32>,
    pub pow_get_bms: Option<f32>,
    pub pow_get_pv: Option<f32>,

    pub bms_batt_soc: Option<f32>,
    pub bms_dsg_rem_time: Option<u32>,
    pub bms_chg_rem_time: Option<u32>,
    pub bms_chg_dsg_state: Option<u32>,

    pub en_beep: Option<bool>,
    pub lcd_light: Option<u32>,
    pub user_temp_unit: Option<u32>,
    pub en_pet_care: Option<bool>,
    pub pet_care_warning: Option<bool>,

    pub plug_in_info_ac_in_flag: Option<bool>,
    pub plug_in_info_dcp_in_flag: Option<bool>,
}

/// The subset of `RuntimePropertyUpload` hearthd surfaces.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct RuntimeProperties {
    // Degrees Celsius.
    pub temp_outdoor_ambient: Option<f32>,
    pub temp_condenser: Option<f32>,
    pub temp_evaporator: Option<f32>,
    pub temp_compressor_discharge: Option<f32>,

    // Volts and amps.
    pub plug_in_info_ac_in_vol: Option<f32>,
    pub plug_in_info_ac_in_amp: Option<f32>,
    pub plug_in_info_pv_vol: Option<f32>,
    pub plug_in_info_pv_amp: Option<f32>,
    pub plug_in_info_dcp_vol: Option<f32>,
    pub plug_in_info_dcp_amp: Option<f32>,
    pub bms_batt_vol: Option<f32>,
    pub bms_batt_amp: Option<f32>,
}

/// A `ConfigWriteAck`. Confirms receipt, not effect.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ConfigWriteAck {
    pub action_id: Option<i32>,
    pub config_ok: Option<bool>,
}

/// A config write, built field by field.
///
/// Only the fields set here are emitted; the device applies what is present
/// and ignores the rest. Combining several fields in one write is normal —
/// powering on into a specific mode is exactly that.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ConfigWrite {
    pub cfg_main_power: Option<bool>,
    pub en_beep: Option<i32>,
    pub lcd_light: Option<i32>,
    pub active_display_property_full_upload: Option<bool>,
    pub active_runtime_property_full_upload: Option<bool>,
    pub cfg_wave_operating_mode: Option<u32>,
    pub cfg_wave_operating_submode: Option<u32>,
    pub cfg_airflow_speed: Option<u32>,
    pub cfg_temp_set: Option<f32>,
    pub cfg_humi_set: Option<f32>,
    pub cfg_temp_thermostatic_upper_limit: Option<f32>,
    pub cfg_temp_thermostatic_lower_limit: Option<f32>,
    pub cfg_drainage_mode: Option<u32>,
    pub cfg_en_pet_care: Option<bool>,
    pub cfg_user_temp_unit: Option<u32>,
    pub cfg_sys_pause: Option<bool>,
}

impl ConfigWrite {
    /// True when no field is set, so there is nothing worth publishing.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Serialise to `pdata` bytes, in ascending field-number order.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();

        if let Some(v) = self.cfg_main_power {
            w.write_bool(config_write::CFG_MAIN_POWER, v);
        }
        if let Some(v) = self.en_beep {
            w.write_i32(config_write::EN_BEEP, v);
        }
        if let Some(v) = self.lcd_light {
            w.write_i32(config_write::LCD_LIGHT, v);
        }
        if let Some(v) = self.active_display_property_full_upload {
            w.write_bool(config_write::ACTIVE_DISPLAY_PROPERTY_FULL_UPLOAD, v);
        }
        if let Some(v) = self.active_runtime_property_full_upload {
            w.write_bool(config_write::ACTIVE_RUNTIME_PROPERTY_FULL_UPLOAD, v);
        }
        if let Some(v) = self.cfg_wave_operating_mode {
            w.write_u32(config_write::CFG_WAVE_OPERATING_MODE, v);
        }
        if let Some(v) = self.cfg_wave_operating_submode {
            w.write_u32(config_write::CFG_WAVE_OPERATING_SUBMODE, v);
        }
        if let Some(v) = self.cfg_airflow_speed {
            w.write_u32(config_write::CFG_AIRFLOW_SPEED, v);
        }
        if let Some(v) = self.cfg_temp_set {
            w.write_f32(config_write::CFG_TEMP_SET, v);
        }
        if let Some(v) = self.cfg_humi_set {
            w.write_f32(config_write::CFG_HUMI_SET, v);
        }
        if let Some(v) = self.cfg_temp_thermostatic_upper_limit {
            w.write_f32(config_write::CFG_TEMP_THERMOSTATIC_UPPER_LIMIT, v);
        }
        if let Some(v) = self.cfg_temp_thermostatic_lower_limit {
            w.write_f32(config_write::CFG_TEMP_THERMOSTATIC_LOWER_LIMIT, v);
        }
        if let Some(v) = self.cfg_drainage_mode {
            w.write_u32(config_write::CFG_DRAINAGE_MODE, v);
        }
        if let Some(v) = self.cfg_en_pet_care {
            w.write_bool(config_write::CFG_EN_PET_CARE, v);
        }
        if let Some(v) = self.cfg_user_temp_unit {
            w.write_u32(config_write::CFG_USER_TEMP_UNIT, v);
        }
        if let Some(v) = self.cfg_sys_pause {
            w.write_bool(config_write::CFG_SYS_PAUSE, v);
        }

        w.into_vec()
    }
}

/// Summarise every field present in a payload, with its value.
///
/// A decoding problem against real hardware is nearly always "which field
/// number actually carries this reading", and the decoders above cannot answer
/// that: they silently skip anything unmapped. This walks the payload
/// generically and renders `field=type(value)` for everything in it, so an
/// unexpected reading can be traced back to a field number.
///
/// Diagnostic only, and not cheap — call it behind a trace-level guard.
pub fn field_census(bytes: &[u8]) -> Result<String, Error> {
    let mut out = String::new();
    let mut reader = Reader::new(bytes);

    while let Some((field, wire)) = reader.read_tag()? {
        if !out.is_empty() {
            out.push(' ');
        }
        match wire {
            WireType::Fixed32 => {
                out.push_str(&format!("{field}=f32({})", reader.read_f32()?));
            }
            WireType::Varint => {
                out.push_str(&format!("{field}=int({})", reader.read_varint()?));
            }
            WireType::Len => {
                let body = reader.read_len_slice()?;
                // Nested messages carry the mode parameter list, which is
                // where the active setpoints live. A string can occasionally
                // parse as a message by chance; in a diagnostic that is worth
                // the risk of an odd-looking line.
                match field_census(body) {
                    Ok(nested) if !nested.is_empty() => {
                        out.push_str(&format!("{field}=[{nested}]"));
                    }
                    _ => out.push_str(&format!("{field}=len({}B)", body.len())),
                }
            }
            WireType::Fixed64 => {
                reader.skip(wire)?;
                out.push_str(&format!("{field}=fixed64"));
            }
        }
    }

    Ok(out)
}

pub fn decode_display(bytes: &[u8]) -> Result<DisplayProperties, Error> {
    let mut out = DisplayProperties::default();
    let mut reader = Reader::new(bytes);

    while let Some((field, wire)) = reader.read_tag()? {
        match (field, wire) {
            (display::TEMP_AMBIENT, WireType::Fixed32) => {
                out.temp_ambient = Some(reader.read_f32()?)
            }
            (display::HUMI_AMBIENT, WireType::Fixed32) => {
                out.humi_ambient = Some(reader.read_f32()?)
            }
            (display::WAVE_OPERATING_MODE, WireType::Varint) => {
                out.wave_operating_mode = Some(reader.read_u32()?)
            }
            (display::DEV_SLEEP_STATE, WireType::Varint) => {
                out.dev_sleep_state = Some(reader.read_u32()?)
            }
            (display::TEMP_INDOOR_SUPPLY_AIR, WireType::Fixed32) => {
                out.temp_indoor_supply_air = Some(reader.read_f32()?)
            }
            (display::WAVE_MODE_INFO, WireType::Len) => {
                out.mode_params = Some(decode_mode_param_list(reader.read_len_slice()?)?)
            }

            (display::IN_DRAINAGE, WireType::Varint) => out.in_drainage = Some(reader.read_bool()?),
            (display::DRAINAGE_MODE, WireType::Varint) => {
                out.drainage_mode = Some(reader.read_u32()?)
            }

            (display::POW_GET_AC, WireType::Fixed32) => out.pow_get_ac = Some(reader.read_f32()?),
            (display::POW_GET_BMS, WireType::Fixed32) => out.pow_get_bms = Some(reader.read_f32()?),
            (display::POW_GET_PV, WireType::Fixed32) => out.pow_get_pv = Some(reader.read_f32()?),

            (display::BMS_BATT_SOC, WireType::Fixed32) => {
                out.bms_batt_soc = Some(reader.read_f32()?)
            }
            (display::BMS_DSG_REM_TIME, WireType::Varint) => {
                out.bms_dsg_rem_time = Some(reader.read_u32()?)
            }
            (display::BMS_CHG_REM_TIME, WireType::Varint) => {
                out.bms_chg_rem_time = Some(reader.read_u32()?)
            }
            (display::BMS_CHG_DSG_STATE, WireType::Varint) => {
                out.bms_chg_dsg_state = Some(reader.read_u32()?)
            }

            (display::EN_BEEP, WireType::Varint) => out.en_beep = Some(reader.read_bool()?),
            (display::LCD_LIGHT, WireType::Varint) => out.lcd_light = Some(reader.read_u32()?),
            (display::USER_TEMP_UNIT, WireType::Varint) => {
                out.user_temp_unit = Some(reader.read_u32()?)
            }
            (display::EN_PET_CARE, WireType::Varint) => out.en_pet_care = Some(reader.read_bool()?),
            (display::PET_CARE_WARNING, WireType::Varint) => {
                out.pet_care_warning = Some(reader.read_bool()?)
            }

            (display::PLUG_IN_INFO_AC_IN_FLAG, WireType::Varint) => {
                out.plug_in_info_ac_in_flag = Some(reader.read_u32()? != 0)
            }
            (display::PLUG_IN_INFO_DCP_IN_FLAG, WireType::Varint) => {
                out.plug_in_info_dcp_in_flag = Some(reader.read_bool()?)
            }

            _ => reader.skip(wire)?,
        }
    }

    Ok(out)
}

fn decode_mode_param_list(bytes: &[u8]) -> Result<Vec<ModeParamItem>, Error> {
    let mut items = Vec::new();
    let mut reader = Reader::new(bytes);

    while let Some((field, wire)) = reader.read_tag()? {
        match (field, wire) {
            // A repeated message field arrives as several occurrences of the
            // same length-delimited tag; append each one.
            (mode_param::LIST_INFO, WireType::Len) => {
                items.push(decode_mode_param_item(reader.read_len_slice()?)?)
            }
            _ => reader.skip(wire)?,
        }
    }

    Ok(items)
}

fn decode_mode_param_item(bytes: &[u8]) -> Result<ModeParamItem, Error> {
    let mut out = ModeParamItem::default();
    let mut reader = Reader::new(bytes);

    while let Some((field, wire)) = reader.read_tag()? {
        match (field, wire) {
            (mode_param::SUBMODE, WireType::Varint) => out.submode = Some(reader.read_u32()?),
            (mode_param::AIRFLOW_SPEED, WireType::Varint) => {
                out.airflow_speed = Some(reader.read_u32()?)
            }
            (mode_param::TEMP_SET, WireType::Fixed32) => out.temp_set = Some(reader.read_f32()?),
            (mode_param::HUMI_SET, WireType::Fixed32) => out.humi_set = Some(reader.read_f32()?),
            (mode_param::TEMP_THERMOSTATIC_UPPER_LIMIT, WireType::Fixed32) => {
                out.temp_thermostatic_upper_limit = Some(reader.read_f32()?)
            }
            (mode_param::TEMP_THERMOSTATIC_LOWER_LIMIT, WireType::Fixed32) => {
                out.temp_thermostatic_lower_limit = Some(reader.read_f32()?)
            }
            _ => reader.skip(wire)?,
        }
    }

    Ok(out)
}

pub fn decode_runtime(bytes: &[u8]) -> Result<RuntimeProperties, Error> {
    let mut out = RuntimeProperties::default();
    let mut reader = Reader::new(bytes);

    while let Some((field, wire)) = reader.read_tag()? {
        match (field, wire) {
            (runtime::TEMP_OUTDOOR_AMBIENT, WireType::Fixed32) => {
                out.temp_outdoor_ambient = Some(reader.read_f32()?)
            }
            (runtime::TEMP_CONDENSER, WireType::Fixed32) => {
                out.temp_condenser = Some(reader.read_f32()?)
            }
            (runtime::TEMP_EVAPORATOR, WireType::Fixed32) => {
                out.temp_evaporator = Some(reader.read_f32()?)
            }
            (runtime::TEMP_COMPRESSOR_DISCHARGE, WireType::Fixed32) => {
                out.temp_compressor_discharge = Some(reader.read_f32()?)
            }

            (runtime::PLUG_IN_INFO_AC_IN_VOL, WireType::Fixed32) => {
                out.plug_in_info_ac_in_vol = Some(reader.read_f32()?)
            }
            (runtime::PLUG_IN_INFO_AC_IN_AMP, WireType::Fixed32) => {
                out.plug_in_info_ac_in_amp = Some(reader.read_f32()?)
            }
            (runtime::PLUG_IN_INFO_PV_VOL, WireType::Fixed32) => {
                out.plug_in_info_pv_vol = Some(reader.read_f32()?)
            }
            (runtime::PLUG_IN_INFO_PV_AMP, WireType::Fixed32) => {
                out.plug_in_info_pv_amp = Some(reader.read_f32()?)
            }
            (runtime::PLUG_IN_INFO_DCP_VOL, WireType::Fixed32) => {
                out.plug_in_info_dcp_vol = Some(reader.read_f32()?)
            }
            (runtime::PLUG_IN_INFO_DCP_AMP, WireType::Fixed32) => {
                out.plug_in_info_dcp_amp = Some(reader.read_f32()?)
            }
            (runtime::BMS_BATT_VOL, WireType::Fixed32) => {
                out.bms_batt_vol = Some(reader.read_f32()?)
            }
            (runtime::BMS_BATT_AMP, WireType::Fixed32) => {
                out.bms_batt_amp = Some(reader.read_f32()?)
            }

            _ => reader.skip(wire)?,
        }
    }

    Ok(out)
}

pub fn decode_config_write_ack(bytes: &[u8]) -> Result<ConfigWriteAck, Error> {
    let mut out = ConfigWriteAck::default();
    let mut reader = Reader::new(bytes);

    while let Some((field, wire)) = reader.read_tag()? {
        match (field, wire) {
            (config_write_ack::ACTION_ID, WireType::Varint) => {
                out.action_id = Some(reader.read_i32()?)
            }
            (config_write_ack::CONFIG_OK, WireType::Varint) => {
                out.config_ok = Some(reader.read_bool()?)
            }
            _ => reader.skip(wire)?,
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_write_emits_only_the_fields_that_are_set() {
        let write = ConfigWrite {
            cfg_temp_set: Some(22.0),
            ..Default::default()
        };
        assert_eq!(write.encode(), vec![0xE5, 0x09, 0x00, 0x00, 0xB0, 0x41]);
    }

    #[test]
    fn empty_config_write_encodes_to_nothing() {
        let write = ConfigWrite::default();
        assert!(write.is_empty());
        assert!(write.encode().is_empty());
        assert!(
            !ConfigWrite {
                cfg_sys_pause: Some(true),
                ..Default::default()
            }
            .is_empty()
        );
    }

    #[test]
    fn powering_on_into_a_mode_is_a_single_combined_write() {
        // Both fields must ride in one message, in ascending field order:
        // cfg_main_power is 4, cfg_wave_operating_mode is 153.
        let write = ConfigWrite {
            cfg_main_power: Some(true),
            cfg_wave_operating_mode: Some(1),
            ..Default::default()
        };
        let bytes = write.encode();
        assert_eq!(bytes, vec![0x20, 0x01, 0xC8, 0x09, 0x01]);
    }

    #[test]
    fn display_decodes_a_sparse_frame_leaving_everything_else_absent() {
        let mut w = Writer::new();
        w.write_f32(display::TEMP_AMBIENT, 21.5);
        w.write_u32(display::WAVE_OPERATING_MODE, 1);

        let decoded = decode_display(&w.into_vec()).unwrap();
        assert_eq!(decoded.temp_ambient, Some(21.5));
        assert_eq!(decoded.wave_operating_mode, Some(1));
        // Everything not in the frame stays absent rather than defaulting.
        assert_eq!(decoded.humi_ambient, None);
        assert_eq!(decoded.bms_batt_soc, None);
        assert_eq!(decoded.mode_params, None);
    }

    #[test]
    fn a_reported_zero_is_distinct_from_an_absent_field() {
        let mut w = Writer::new();
        w.write_f32(display::POW_GET_AC, 0.0);
        let decoded = decode_display(&w.into_vec()).unwrap();
        assert_eq!(decoded.pow_get_ac, Some(0.0));
        assert_eq!(decoded.pow_get_pv, None);
    }

    #[test]
    fn display_skips_unknown_fields_from_newer_firmware() {
        let mut w = Writer::new();
        w.write_f32(display::TEMP_AMBIENT, 21.5);
        // Fields hearthd does not model, one of each wire type.
        w.write_u32(999, 1);
        w.write_f32(998, 2.0);
        w.write_string(997, "new in firmware 2.0");
        w.write_f32(display::HUMI_AMBIENT, 55.0);

        let decoded = decode_display(&w.into_vec()).unwrap();
        assert_eq!(decoded.temp_ambient, Some(21.5));
        assert_eq!(decoded.humi_ambient, Some(55.0));
    }

    #[test]
    fn a_field_with_an_unexpected_wire_type_is_skipped_not_fatal() {
        let mut w = Writer::new();
        // temp_ambient is a float, but send it as a varint.
        w.write_u32(display::TEMP_AMBIENT, 21);
        w.write_f32(display::HUMI_AMBIENT, 55.0);

        let decoded = decode_display(&w.into_vec()).unwrap();
        assert_eq!(decoded.temp_ambient, None);
        assert_eq!(decoded.humi_ambient, Some(55.0));
    }

    #[test]
    fn duplicate_scalar_fields_take_the_last_value() {
        let mut w = Writer::new();
        w.write_f32(display::TEMP_AMBIENT, 21.5);
        w.write_f32(display::TEMP_AMBIENT, 23.0);
        let decoded = decode_display(&w.into_vec()).unwrap();
        assert_eq!(decoded.temp_ambient, Some(23.0));
    }

    fn mode_param_item_bytes(submode: u32, speed: u32, temp: f32) -> Vec<u8> {
        let mut w = Writer::new();
        w.write_u32(mode_param::SUBMODE, submode);
        w.write_u32(mode_param::AIRFLOW_SPEED, speed);
        w.write_f32(mode_param::TEMP_SET, temp);
        w.into_vec()
    }

    #[test]
    fn mode_param_list_collects_repeated_message_occurrences() {
        let mut list = Writer::new();
        // Index 0 is "off" and carries nothing useful, but still occupies a slot.
        list.write_bytes(mode_param::LIST_INFO, &[]);
        list.write_bytes(mode_param::LIST_INFO, &mode_param_item_bytes(0, 40, 22.0));
        list.write_bytes(mode_param::LIST_INFO, &mode_param_item_bytes(3, 60, 24.0));

        let mut w = Writer::new();
        w.write_bytes(display::WAVE_MODE_INFO, &list.into_vec());

        let decoded = decode_display(&w.into_vec()).unwrap();
        let params = decoded.mode_params.unwrap();
        assert_eq!(params.len(), 3);
        assert_eq!(params[0], ModeParamItem::default());
        assert_eq!(params[1].submode, Some(0));
        assert_eq!(params[1].airflow_speed, Some(40));
        assert_eq!(params[1].temp_set, Some(22.0));
        assert_eq!(params[2].temp_set, Some(24.0));
        // Fields absent from an item stay absent.
        assert_eq!(params[2].humi_set, None);
    }

    #[test]
    fn runtime_decodes_the_diagnostic_temperature_pair() {
        let mut w = Writer::new();
        w.write_f32(runtime::TEMP_CONDENSER, 41.5);
        w.write_f32(runtime::TEMP_EVAPORATOR, 8.25);
        w.write_f32(runtime::BMS_BATT_VOL, 51.2);

        let decoded = decode_runtime(&w.into_vec()).unwrap();
        assert_eq!(decoded.temp_condenser, Some(41.5));
        assert_eq!(decoded.temp_evaporator, Some(8.25));
        assert_eq!(decoded.bms_batt_vol, Some(51.2));
        assert_eq!(decoded.temp_compressor_discharge, None);
    }

    #[test]
    fn config_write_ack_decodes() {
        let mut w = Writer::new();
        w.write_i32(config_write_ack::ACTION_ID, 7);
        w.write_bool(config_write_ack::CONFIG_OK, true);

        let decoded = decode_config_write_ack(&w.into_vec()).unwrap();
        assert_eq!(decoded.action_id, Some(7));
        assert_eq!(decoded.config_ok, Some(true));
    }

    #[test]
    fn a_rejected_config_write_is_distinguishable_from_a_silent_one() {
        let mut w = Writer::new();
        w.write_bool(config_write_ack::CONFIG_OK, false);
        let decoded = decode_config_write_ack(&w.into_vec()).unwrap();
        assert_eq!(decoded.config_ok, Some(false));

        let empty = decode_config_write_ack(&[]).unwrap();
        assert_eq!(empty.config_ok, None);
    }

    #[test]
    fn truncated_payloads_produce_an_error_rather_than_partial_state() {
        // A float field cut short mid-value.
        let bytes = vec![0xE2, 0x0F, 0x00, 0x00];
        assert!(decode_display(&bytes).is_err());
    }
}
