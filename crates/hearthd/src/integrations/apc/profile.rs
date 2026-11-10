//! What hearthd knows about each supported PDU model.
//!
//! A model is supported only once it is listed here. Models that expose the
//! same SNMP objects share one table and differ only in what the [`Profile`]
//! records about the unit itself, so adding a sibling model is one variant
//! and one match arm.

/// A supported PDU model, as named by `model` in configuration.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, strum::EnumString, strum::Display, strum::VariantNames,
)]
#[strum(serialize_all = "snake_case")]
pub enum Model {
    ApcAp7921,
}

impl Model {
    pub fn profile(self) -> Profile {
        match self {
            Model::ApcAp7921 => Profile {
                objects: &APC_RPDU_OBJECTS,
                model_number: "AP7921",
                outlets: 8,
            },
        }
    }
}

/// One model: the SNMP objects it exposes and what it says about itself.
#[derive(Debug, Clone, Copy)]
pub struct Profile {
    pub objects: &'static SnmpObjects,

    /// What the model-number object reads on this model.
    pub model_number: &'static str,

    pub outlets: u8,
}

/// Where a family of PDUs exposes each value over SNMP, and how the values
/// are encoded.
///
/// Columns are table OIDs without the row index; outlet `n` is the column
/// with `n` appended.
#[derive(Debug)]
pub struct SnmpObjects {
    /// Scalar holding the device's model number as a string.
    pub model_number: &'static [u64],

    pub outlet_name: &'static [u64],
    pub outlet_state: &'static [u64],
    pub state_on: i64,
    pub state_off: i64,

    /// Column that reads `pending` while a command is still being carried
    /// out.
    pub outlet_command_pending: &'static [u64],
    pub pending: i64,

    pub outlet_command: &'static [u64],
    pub command_on: i64,
    pub command_off: i64,

    /// Scalar holding the whole unit's current draw, in units of
    /// `load_milliamps` mA.
    pub load: &'static [u64],
    pub load_milliamps: i64,
}

/// The objects of APC's legacy rack PDU MIB (PowerNet-MIB `rPDU`,
/// 1.3.6.1.4.1.318.1.1.12), used by the AP79xx switched rack PDUs.
pub static APC_RPDU_OBJECTS: SnmpObjects = SnmpObjects {
    // rPDUIdentModelNumber
    model_number: &[1, 3, 6, 1, 4, 1, 318, 1, 1, 12, 1, 5, 0],
    // rPDUOutletStatusOutletName
    outlet_name: &[1, 3, 6, 1, 4, 1, 318, 1, 1, 12, 3, 5, 1, 1, 2],
    // rPDUOutletStatusOutletState: outletStatusOn(1), outletStatusOff(2)
    outlet_state: &[1, 3, 6, 1, 4, 1, 318, 1, 1, 12, 3, 5, 1, 1, 4],
    state_on: 1,
    state_off: 2,
    // rPDUOutletStatusCommandPending: commandPending(1), noCommandPending(2)
    outlet_command_pending: &[1, 3, 6, 1, 4, 1, 318, 1, 1, 12, 3, 5, 1, 1, 5],
    pending: 1,
    // rPDUOutletControlOutletCommand: immediateOn(1), immediateOff(2)
    outlet_command: &[1, 3, 6, 1, 4, 1, 318, 1, 1, 12, 3, 3, 1, 1, 4],
    command_on: 1,
    command_off: 2,
    // rPDULoadStatusLoad for the unit's only bank, in tenths of an amp
    load: &[1, 3, 6, 1, 4, 1, 318, 1, 1, 12, 2, 3, 1, 1, 2, 1],
    load_milliamps: 100,
};

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn models_are_named_in_snake_case() {
        assert_eq!(Model::from_str("apc_ap7921"), Ok(Model::ApcAp7921));
        assert_eq!(Model::ApcAp7921.to_string(), "apc_ap7921");
        assert!(Model::from_str("ApcAp7921").is_err());
    }
}
