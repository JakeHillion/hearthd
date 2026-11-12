use anyhow::ensure;

use super::wire::tlvs;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    /// Presence is separate from a valid reading: a malformed/unknown value
    /// in a reported group invalidates its previous value on merge.
    pub present: u8,
    pub serial: Option<String>,
    pub model: Option<String>,
    pub soc: Option<u8>,
    pub health: Option<u8>,
    pub temperature: Option<i8>,
    pub flow: Option<u8>,
    pub remaining_seconds: Option<u32>,
    pub total_watts: Option<u16>,
    pub grid_watts: Option<u16>,
    pub dc_watts: Option<u16>,
    pub usb_watts: Option<u16>,
    pub ac_watts: Option<u16>,
    pub ac_on: Option<bool>,
    pub pass_through_watts: Option<u16>,
    pub mode: Option<u8>,
    pub backup_soc: Option<u8>,
    pub max_soc: Option<u8>,
    pub min_soc: Option<u8>,
    pub charge_cap_watts: Option<u16>,
    pub max_charge_cap_watts: Option<u16>,
}

fn word(bytes: &[u8], offset: usize) -> Option<u16> {
    let value = u16::from_le_bytes(bytes.get(offset..offset + 2)?.try_into().ok()?);
    (value <= i16::MAX as u16).then_some(value)
}

fn percent(bytes: &[u8], offset: usize) -> Option<u8> {
    bytes.get(offset).copied().filter(|value| *value <= 100)
}

fn packed<'a>(values: &std::collections::HashMap<u8, &'a [u8]>, tag: u8) -> anyhow::Result<&'a [u8]> {
    let Some(value) = values.get(&tag) else { return Ok(&[]) };
    ensure!(value.first() == Some(&4), "invalid packed telemetry type for tag {tag:02x}");
    Ok(&value[1..])
}

impl Snapshot {
    pub fn decode(body: &[u8]) -> anyhow::Result<Self> {
        let body = body.strip_prefix(&[0]).unwrap_or(body);
        let values = tlvs(body)?;
        ensure!(values.get(&0xa1).copied() == Some(&[0x31][..]), "missing or unexpected telemetry route");
        let mut snapshot = Self::default();
        for (tag, bit) in [(0xa3, 1), (0xa4, 2), (0xa5, 4), (0xa6, 8), (0xa7, 16), (0xaa, 32), (0xd9, 64)] { if values.contains_key(&tag) { snapshot.present |= bit; } }
        if let Some(identity) = values.get(&0xa2) {
            ensure!(identity.len() >= 3 && identity[..2] == [6, 0x20], "unsupported identity envelope");
            let serial_len = identity[2] as usize;
            let end = 3 + serial_len;
            ensure!(identity.len() >= end + 2 && identity[end] == 0, "truncated identity serial");
            let model_len = identity[end + 1] as usize;
            ensure!(identity.len() >= end + 2 + model_len, "truncated identity model");
            snapshot.serial = Some(std::str::from_utf8(&identity[3..end])?.to_string());
            snapshot.model = Some(std::str::from_utf8(&identity[end + 2..end + 2 + model_len])?.to_string());
        }
        let a3 = packed(&values, 0xa3)?;
        snapshot.max_charge_cap_watts = word(a3, 4);
        let a4 = packed(&values, 0xa4)?;
        snapshot.charge_cap_watts = word(a4, 4);
        snapshot.max_soc = percent(a4, 23);
        snapshot.min_soc = percent(a4, 24);
        let a5 = packed(&values, 0xa5)?;
        snapshot.temperature = a5.first().map(|value| *value as i8);
        snapshot.flow = a5.get(1).copied().filter(|value| *value <= 2);
        snapshot.soc = percent(a5, 2);
        snapshot.health = percent(a5, 3);
        let a6 = packed(&values, 0xa6)?;
        snapshot.total_watts = word(a6, 0);
        snapshot.grid_watts = word(a6, 2);
        snapshot.dc_watts = word(a6, 4);
        // A tenth of an hour is 360 seconds. Zero is a reported value.
        snapshot.remaining_seconds = a6.get(6..8).map(|bytes| u32::from(u16::from_le_bytes(bytes.try_into().unwrap())) * 360);
        let a7 = packed(&values, 0xa7)?;
        snapshot.ac_on = a7.first().and_then(|value| match value { 0 => Some(false), 1 => Some(true), _ => None });
        snapshot.ac_watts = word(a7, 1);
        snapshot.pass_through_watts = word(a7, 4);
        snapshot.usb_watts = word(packed(&values, 0xaa)?, 1);
        let d9 = packed(&values, 0xd9)?;
        snapshot.mode = d9.get(1).copied();
        snapshot.backup_soc = percent(d9, 2);
        snapshot.max_soc = percent(d9, 3).or(snapshot.max_soc);
        snapshot.min_soc = percent(d9, 4).or(snapshot.min_soc);
        Ok(snapshot)
    }

    pub fn check_identity(&self, serial: &str) -> anyhow::Result<()> {
        ensure!(self.serial.as_deref().is_none_or(|value| value == serial), "telemetry serial does not match intended unit");
        ensure!(self.model.as_deref().is_none_or(|value| value == "AS220"), "device is not an AS220 / SOLIX S2000");
        Ok(())
    }

    pub fn identified(&self) -> bool { self.serial.is_some() && self.model.as_deref() == Some("AS220") }

    pub fn merge(&mut self, update: &Self) {
        macro_rules! merge {
            ($($field:ident),* $(,)?) => { $(if update.$field.is_some() { self.$field = update.$field.clone(); })* };
        }
        merge!(serial, model);
        if update.present & 1 != 0 { self.max_charge_cap_watts = update.max_charge_cap_watts; }
        if update.present & 2 != 0 { self.charge_cap_watts = update.charge_cap_watts; self.max_soc = update.max_soc; self.min_soc = update.min_soc; }
        if update.present & 4 != 0 { self.soc = update.soc; self.health = update.health; self.temperature = update.temperature; self.flow = update.flow; }
        if update.present & 8 != 0 { self.remaining_seconds = update.remaining_seconds; self.total_watts = update.total_watts; self.grid_watts = update.grid_watts; self.dc_watts = update.dc_watts; }
        if update.present & 16 != 0 { self.ac_on = update.ac_on; self.ac_watts = update.ac_watts; self.pass_through_watts = update.pass_through_watts; }
        if update.present & 32 != 0 { self.usb_watts = update.usb_watts; }
        if update.present & 64 != 0 { self.mode = update.mode; self.backup_soc = update.backup_soc; self.max_soc = update.max_soc; self.min_soc = update.min_soc; }
        self.present |= update.present;
    }
}
