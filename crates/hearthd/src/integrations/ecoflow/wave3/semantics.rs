//! EcoFlow Wave 3 control semantics: what the values on the wire mean.
//!
//! Field numbers live in `super::fields`; this module covers the encodings and
//! behavioural rules that give those fields meaning — the mode and preset
//! enumerations, the fan-speed mapping, the setpoint ranges, the beeper flag,
//! and the rule for finding the currently active setpoints.
//!
//! Everything here is stated in the device's own terms. Translation into
//! hearthd's Matter data model happens in `super::matter`.
//!
//! # Attribution
//!
//! The operating-mode and submode encodings, the airflow-percentage-to-step
//! mapping, the `cfg_sys_pause` / `cfg_main_power` power sequence, the
//! mode-parameter-list indexing rule, the beeper flag and the setpoint
//! ranges were reverse-engineered by the `tolwi/hassio-ecoflow-cloud` Home
//! Assistant custom component (Apache-2.0), read at commit `a7ebbba`, in
//! `custom_components/ecoflow_cloud/devices/internal/wave3.py`.
//!
//! What that project provided is knowledge of how the device behaves. The Rust
//! below, and the handling of the edge cases it does not cover, are original
//! to hearthd. No code was copied.

/// Operating mode, as reported in display field 486 and commanded by
/// `cfg_wave_operating_mode` (config field 153).
///
/// Which controls apply depends on the mode: target temperature in cool and
/// heat, target humidity in dry, the upper/lower limit pair in auto, fan speed
/// in every running mode, and presets in cool and heat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatingMode {
    Off = 0,
    Cool = 1,
    Heat = 2,
    FanOnly = 3,
    Dry = 4,
    /// Thermostatic: uses the upper and lower limit pair rather than a single
    /// setpoint.
    Auto = 5,
}

impl OperatingMode {
    pub fn from_wire(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Off),
            1 => Some(Self::Cool),
            2 => Some(Self::Heat),
            3 => Some(Self::FanOnly),
            4 => Some(Self::Dry),
            5 => Some(Self::Auto),
            _ => None,
        }
    }

    pub fn to_wire(self) -> u32 {
        self as u32
    }
}

/// Preset within a mode, commanded by `cfg_wave_operating_submode` (config
/// field 154) and reported per-mode in the mode parameter list.
///
/// An unrecognised submode is reported as unknown rather than mapped onto a
/// neighbour.
///
/// # Two values mean "no preset"
///
/// The upstream reverse-engineering records 0 as normal and describes 1 as
/// never observed, on the grounds that the app never sends it. That is a claim
/// about what the *app writes*; the *device reports* 1. Hardware with no
/// preset selected reports submode 1 in every mode that carries one, and
/// selecting eco moves it to 4, so both values are accepted here and mean the
/// same thing. 0 has never been seen on the wire, and is kept as the value
/// hearthd sends because that is the one the app is documented to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Normal = 0,
    /// Maximum output.
    Boost = 2,
    Sleep = 3,
    Eco = 4,
}

impl Preset {
    pub fn from_wire(value: u32) -> Option<Self> {
        match value {
            0 | 1 => Some(Self::Normal),
            2 => Some(Self::Boost),
            3 => Some(Self::Sleep),
            4 => Some(Self::Eco),
            _ => None,
        }
    }

    pub fn to_wire(self) -> u32 {
        self as u32
    }

    /// Every preset, in the order hearthd advertises them.
    pub const ALL: [Preset; 4] = [Preset::Normal, Preset::Boost, Preset::Eco, Preset::Sleep];

    pub fn label(self) -> &'static str {
        match self {
            Preset::Normal => "Normal",
            Preset::Boost => "Boost",
            Preset::Sleep => "Sleep",
            Preset::Eco => "Eco",
        }
    }
}

/// Display of `user_temp_unit` (display field 512, config field 166). Affects
/// the physical panel only; every value on the wire stays in Celsius.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserTempUnit {
    Unset = 0,
    Celsius = 1,
    Fahrenheit = 2,
}

impl UserTempUnit {
    pub fn from_wire(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Unset),
            1 => Some(Self::Celsius),
            2 => Some(Self::Fahrenheit),
            _ => None,
        }
    }

    pub fn to_wire(self) -> u32 {
        self as u32
    }
}

/// `dev_sleep_state` (display field 212) value meaning "in standby".
///
/// This is a separate axis from the operating mode: when it is 1 the unit is
/// in standby regardless of what `wave_operating_mode` still reports. Treat it
/// as "off" first and only then consult the mode.
pub const SLEEP_STATE_STANDBY: u32 = 1;

/// `drainage_mode` (display field 506) value meaning automatic drainage is
/// enabled.
pub const DRAINAGE_MODE_ON: u32 = 1;

/// The five discrete fan speeds, as the percentages the device expects.
///
/// `cfg_airflow_speed` is expressed as a percentage but the unit has only
/// these five steps, and only these five values may be sent.
pub const FAN_SPEED_PERCENTS: [u32; 5] = [20, 40, 60, 80, 100];

/// Highest valid fan step.
pub const FAN_STEP_MAX: u8 = 5;

/// Convert a fan step (1..=5) to the percentage to send. Returns `None` for a
/// step outside that range.
pub fn fan_step_to_percent(step: u8) -> Option<u32> {
    if step == 0 || step > FAN_STEP_MAX {
        return None;
    }
    Some(FAN_SPEED_PERCENTS[usize::from(step) - 1])
}

/// Snap a reported airflow percentage to the nearest fan step.
///
/// The device may report an intermediate value while ramping, or after a
/// change made in the EcoFlow app. Snapping is for display only: the snapped
/// value must never be echoed back as a command.
///
/// A reported 0 maps to step 0, meaning "not running". That reading is
/// hearthd's — the protocol says nothing about 0 — and it lines up with
/// Matter's `SpeedSetting`, where 0 is off.
pub fn fan_percent_to_step(percent: u32) -> u8 {
    if percent == 0 {
        return 0;
    }
    // Round to the nearest multiple of 20, half away from zero, then clamp.
    (((percent + 10) / 20) as u8).clamp(1, FAN_STEP_MAX)
}

/// Inclusive target-temperature range in degrees Celsius, in steps of 1.0.
pub const TEMP_SET_MIN_C: f32 = 16.0;
pub const TEMP_SET_MAX_C: f32 = 30.0;

/// Inclusive target relative-humidity range in whole percent, dry mode.
pub const HUMI_SET_MIN_PERCENT: u32 = 40;
pub const HUMI_SET_MAX_PERCENT: u32 = 80;

/// Inclusive panel brightness range in whole percent.
pub const LCD_LIGHT_MIN: u32 = 0;
pub const LCD_LIGHT_MAX: u32 = 100;

/// Clamp a target temperature to the range the device accepts.
///
/// All temperatures on this wire are degrees Celsius as `f32`, unscaled: there
/// is no tenths-of-a-degree integer encoding anywhere in the schema.
pub fn clamp_temp_set(celsius: f32) -> f32 {
    celsius.clamp(TEMP_SET_MIN_C, TEMP_SET_MAX_C)
}

/// Clamp a target humidity to the range the device accepts.
pub fn clamp_humi_set(percent: u32) -> u32 {
    percent.clamp(HUMI_SET_MIN_PERCENT, HUMI_SET_MAX_PERCENT)
}

/// Clamp a panel brightness to the range the device accepts.
pub fn clamp_lcd_light(percent: u32) -> u32 {
    percent.clamp(LCD_LIGHT_MIN, LCD_LIGHT_MAX)
}

/// The beeper flag as the `int32` the config write expects.
///
/// | Wire value | Audible? |
/// | --- | --- |
/// | `en_beep = false` / `enBeep = 0` | no |
/// | `en_beep = true` / `enBeep = 1` | yes |
///
/// The name means what it says, so nothing is inverted and the display field
/// needs no conversion at all.
///
/// # This contradicts the upstream reverse-engineering
///
/// Upstream treats the field as a *mute* flag and inverts it. Hardware says
/// otherwise: with `en_beep` reported as false the panel was silent under
/// button presses, and writing 1 made it beep again. Inverting cost nothing on
/// the wire but made the endpoint report the opposite of the truth.
pub fn beeper_wire_from_audible(audible: bool) -> i32 {
    i32::from(audible)
}

/// Largest remaining-time reading, in minutes, that is worth believing.
///
/// The battery remaining-time fields report implausible values — very large,
/// or a sentinel — when the unit is neither charging nor discharging.
const REMAINING_TIME_MAX_MINUTES: u32 = 5000;

/// Accept a battery remaining-time reading, or reject it as not applicable.
///
/// Returning `None` rather than a clamped number matters: a clamped 5000 would
/// be indistinguishable from a real reading, whereas absent is honest.
pub fn plausible_remaining_minutes(minutes: u32) -> Option<u32> {
    (minutes <= REMAINING_TIME_MAX_MINUTES).then_some(minutes)
}

/// Index of the currently active entry in the mode parameter list.
///
/// The Wave 3 reports saved parameters for *every* mode and expects the reader
/// to index that list with the current mode. Index 0 corresponds to "off" and
/// carries no useful parameters, so a valid index is `1 <= mode < len`.
///
/// `None` means this frame cannot resolve active settings and the previously
/// cached values must be left in place.
///
/// Because a mode change swaps the whole active parameter set, the target
/// temperature can appear to jump when the user switches from cool to heat.
/// That is correct: the unit stores per-mode setpoints.
pub fn active_mode_param_index(mode: u32, list_len: usize) -> Option<usize> {
    let index = usize::try_from(mode).ok()?;
    (index >= 1 && index < list_len).then_some(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operating_mode_round_trips() {
        for mode in [
            OperatingMode::Off,
            OperatingMode::Cool,
            OperatingMode::Heat,
            OperatingMode::FanOnly,
            OperatingMode::Dry,
            OperatingMode::Auto,
        ] {
            assert_eq!(OperatingMode::from_wire(mode.to_wire()), Some(mode));
        }
        assert_eq!(OperatingMode::from_wire(6), None);
        assert_eq!(OperatingMode::from_wire(u32::MAX), None);
    }

    #[test]
    fn preset_round_trips() {
        for preset in Preset::ALL {
            assert_eq!(Preset::from_wire(preset.to_wire()), Some(preset));
        }
        assert_eq!(Preset::from_wire(5), None);
        assert_eq!(Preset::from_wire(u32::MAX), None);
    }

    #[test]
    fn both_no_preset_values_decode_to_normal() {
        // Hardware reports 1 when no preset is selected, which the upstream
        // notes describe as never observed because the app sends 0. Rejecting
        // it left the preset permanently unknown on a real unit.
        assert_eq!(Preset::from_wire(0), Some(Preset::Normal));
        assert_eq!(Preset::from_wire(1), Some(Preset::Normal));
        // What hearthd sends stays 0, the value the app is documented to use.
        assert_eq!(Preset::Normal.to_wire(), 0);
    }

    #[test]
    fn fan_steps_map_to_the_five_permitted_percentages() {
        assert_eq!(fan_step_to_percent(1), Some(20));
        assert_eq!(fan_step_to_percent(2), Some(40));
        assert_eq!(fan_step_to_percent(3), Some(60));
        assert_eq!(fan_step_to_percent(4), Some(80));
        assert_eq!(fan_step_to_percent(5), Some(100));
        assert_eq!(fan_step_to_percent(0), None);
        assert_eq!(fan_step_to_percent(6), None);
    }

    #[test]
    fn exact_fan_percentages_snap_to_their_own_step() {
        for (index, percent) in FAN_SPEED_PERCENTS.iter().enumerate() {
            assert_eq!(fan_percent_to_step(*percent), index as u8 + 1);
        }
    }

    #[test]
    fn intermediate_fan_percentages_snap_to_the_nearest_step() {
        assert_eq!(fan_percent_to_step(0), 0);
        assert_eq!(fan_percent_to_step(1), 1);
        assert_eq!(fan_percent_to_step(25), 1);
        // Equidistant values round away from zero.
        assert_eq!(fan_percent_to_step(30), 2);
        assert_eq!(fan_percent_to_step(35), 2);
        assert_eq!(fan_percent_to_step(71), 4);
        assert_eq!(fan_percent_to_step(99), 5);
        // Out of range readings clamp rather than overflow the step space.
        assert_eq!(fan_percent_to_step(200), 5);
        assert_eq!(fan_percent_to_step(u32::MAX - 10), 5);
    }

    #[test]
    fn snapping_a_reported_value_is_never_a_valid_command() {
        // A snapped step must be converted back through fan_step_to_percent
        // before being sent, which always yields one of the five values.
        let reported = 71;
        let step = fan_percent_to_step(reported);
        let to_send = fan_step_to_percent(step).unwrap();
        assert!(FAN_SPEED_PERCENTS.contains(&to_send));
        assert_ne!(to_send, reported);
    }

    #[test]
    fn beeper_flag_is_not_inverted() {
        assert_eq!(beeper_wire_from_audible(true), 1);
        assert_eq!(beeper_wire_from_audible(false), 0);
    }

    #[test]
    fn setpoints_clamp_to_the_documented_ranges() {
        assert_eq!(clamp_temp_set(22.0), 22.0);
        assert_eq!(clamp_temp_set(5.0), TEMP_SET_MIN_C);
        assert_eq!(clamp_temp_set(45.0), TEMP_SET_MAX_C);
        assert_eq!(clamp_humi_set(55), 55);
        assert_eq!(clamp_humi_set(10), HUMI_SET_MIN_PERCENT);
        assert_eq!(clamp_humi_set(95), HUMI_SET_MAX_PERCENT);
        assert_eq!(clamp_lcd_light(200), LCD_LIGHT_MAX);
    }

    #[test]
    fn implausible_remaining_times_are_rejected_not_clamped() {
        assert_eq!(plausible_remaining_minutes(0), Some(0));
        assert_eq!(plausible_remaining_minutes(120), Some(120));
        assert_eq!(plausible_remaining_minutes(5000), Some(5000));
        assert_eq!(plausible_remaining_minutes(5001), None);
        assert_eq!(plausible_remaining_minutes(u32::MAX), None);
    }

    #[test]
    fn mode_param_index_rejects_off_and_out_of_range() {
        // A six-entry list covers modes 0..=5.
        assert_eq!(active_mode_param_index(1, 6), Some(1));
        assert_eq!(active_mode_param_index(5, 6), Some(5));
        // Index 0 is "off" and carries nothing useful.
        assert_eq!(active_mode_param_index(0, 6), None);
        // Mode beyond the list means the frame cannot resolve settings.
        assert_eq!(active_mode_param_index(6, 6), None);
        assert_eq!(active_mode_param_index(3, 2), None);
        assert_eq!(active_mode_param_index(1, 0), None);
    }
}
