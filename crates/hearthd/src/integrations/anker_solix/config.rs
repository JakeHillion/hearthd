use std::collections::HashMap;
use std::collections::HashSet;

use anyhow::ensure;
use hearthd_config::SubConfig;
use hearthd_config::TryFromPartial;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, TryFromPartial, SubConfig)]
pub struct Config {
    pub devices: HashMap<String, DeviceConfig>,
}

#[derive(Clone, Deserialize, TryFromPartial, SubConfig)]
#[config(no_span)]
pub struct DeviceConfig {
    pub model: String,
    pub adapter: String,
    pub address: String,
    pub serial: String,
    pub client_id: String,
}

impl std::fmt::Debug for DeviceConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceConfig").field("model", &self.model).field("adapter", &self.adapter).field("address", &self.address).field("serial", &self.serial).field("client_id", &"[redacted]").finish()
    }
}

pub fn valid_address(address: &str) -> bool {
    address.len() == 17 && address.split(':').count() == 6 && address.split(':').all(|part| part.len() == 2 && part.bytes().all(|b| b.is_ascii_hexdigit()))
}

impl DeviceConfig {
    pub fn validate(&self) -> anyhow::Result<()> {
        ensure!(self.model == "s2000", "only model s2000 is currently supported");
        ensure!(self.adapter.strip_prefix("hci").is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())), "adapter must name a BlueZ adapter such as hci0");
        ensure!(valid_address(&self.address), "address must be six colon-separated hexadecimal octets");
        ensure!(!self.serial.is_empty() && self.serial.len() <= 32 && self.serial.bytes().all(|b| b.is_ascii_alphanumeric()), "serial must contain 1 to 32 ASCII letters or digits");
        let id = Uuid::parse_str(&self.client_id).map_err(|_| anyhow::anyhow!("client_id must be a UUID"))?;
        ensure!(!id.is_nil() && id.hyphenated().to_string() == self.client_id, "client_id must be a nonzero lowercase hyphenated UUID");
        Ok(())
    }
}

impl Config {
    pub fn validate(&self) -> anyhow::Result<()> {
        let mut addresses = HashSet::new();
        ensure!(!self.devices.is_empty(), "at least one S2000 device must be configured");
        let mut serials = HashSet::new();
        for (name, device) in &self.devices {
            ensure!(!name.is_empty(), "device name must not be empty");
            device.validate().map_err(|error| anyhow::anyhow!("device {name}: {error}"))?;
            ensure!(addresses.insert(device.address.to_ascii_uppercase()), "duplicate physical Bluetooth address");
            ensure!(serials.insert(device.serial.clone()), "duplicate device serial");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn device() -> DeviceConfig {
        DeviceConfig { model: "s2000".into(), adapter: "hci0".into(), address: "00:11:22:33:44:55".into(), serial: "APCE1234567890123".into(), client_id: "11111111-2222-4333-8444-555555555555".into() }
    }

    #[test]
    fn identifiers_and_duplicate_devices_are_validated_without_disclosing_identity() {
        let good = device();
        good.validate().unwrap();
        assert!(!format!("{good:?}").contains(&good.client_id));
        let duplicate = Config { devices: HashMap::from([("a".into(), good.clone()), ("b".into(), good.clone())]) };
        assert!(duplicate.validate().is_err());
        for address in ["00:11:22:33:44", "00:11:22:33:44:GG", "001122334455"] { assert!(!valid_address(address)); }
        let mut bad = good;
        bad.model = "other".into();
        assert!(bad.validate().is_err());
    }

    #[test]
    fn normal_configuration_merging_loads_a_private_client_identity() {
        let directory = tempfile::tempdir().unwrap();
        let base = directory.path().join("base.toml");
        let secret = directory.path().join("secret.toml");
        std::fs::write(&base, "[integrations.anker_solix.devices.rack]\nmodel = 's2000'\nadapter = 'hci0'\naddress = '00:11:22:33:44:55'\nserial = 'TEST1234567890123'\n").unwrap();
        std::fs::write(&secret, "[integrations.anker_solix.devices.rack]\nclient_id = '11111111-2222-4333-8444-555555555555'\n").unwrap();
        let (config, _) = crate::Config::from_files(&[base, secret]).unwrap();
        let integration = config.integrations.anker_solix.as_ref().unwrap();
        integration.validate().unwrap();
        assert_eq!(integration.devices["rack"].client_id, "11111111-2222-4333-8444-555555555555");
        assert!(!format!("{config:?}").contains("11111111-2222-4333-8444-555555555555"));
    }
}
