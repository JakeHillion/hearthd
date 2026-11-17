use std::collections::HashMap;
use std::collections::HashSet;

use hearthd_config::FromConfig;
use hearthd_config::Validate;
use hearthd_config::Validation;

use super::types::AdapterName;
use super::types::BluetoothAddress;
use super::types::ClientId;
use super::types::DeviceName;
use super::types::Model;
use super::types::Serial;

#[derive(Debug, Clone, FromConfig)]
pub struct Config {
    /// Names supply display labels; each device's serial supplies its stable node key.
    pub devices: HashMap<DeviceName, DeviceConfig>,
}

#[derive(Clone, FromConfig)]
pub struct DeviceConfig {
    pub model: Model,
    /// Powered system BlueZ adapter, accessible to the daemon over system D-Bus.
    pub adapter: AdapterName,
    pub address: BluetoothAddress,
    pub serial: Serial,
    /// Enrolled identity. Keep it private and reuse it across reconnects;
    /// generating another UUID requires a new physical enrollment.
    pub client_id: ClientId,
}

impl std::fmt::Debug for DeviceConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceConfig")
            .field("model", &self.model)
            .field("adapter", &self.adapter)
            .field("address", &self.address)
            .field("serial", &self.serial)
            .field("client_id", &"[redacted]")
            .finish()
    }
}

impl Validate for Config {
    fn validate(&self, validation: &mut Validation<'_>) {
        let mut addresses = HashSet::new();
        let mut serials = HashSet::new();
        for (name, device) in &self.devices {
            if !addresses.insert(device.address) {
                validation
                    .field("devices")
                    .field(name)
                    .field("address")
                    .error("duplicate physical Bluetooth address");
            }
            if !serials.insert(&device.serial) {
                validation
                    .field("devices")
                    .field(name)
                    .field("serial")
                    .error("duplicate device serial");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn device() -> DeviceConfig {
        DeviceConfig {
            model: Model::S2000,
            adapter: "hci0".parse().unwrap(),
            address: "00:11:22:33:44:55".parse().unwrap(),
            serial: "APCE1234567890123".parse().unwrap(),
            client_id: "11111111-2222-4333-8444-555555555555".parse().unwrap(),
        }
    }

    #[test]
    fn typed_identifiers_parse_without_disclosing_identity() {
        let good = device();
        assert!(!format!("{good:?}").contains(&good.client_id.to_string()));
        for address in ["00:11:22:33:44", "00:11:22:33:44:GG", "001122334455"] {
            assert!(address.parse::<BluetoothAddress>().is_err());
        }
        assert!(
            "00000000-0000-0000-0000-000000000000"
                .parse::<ClientId>()
                .is_err()
        );
    }

    #[test]
    fn normal_configuration_merging_loads_a_private_client_identity() {
        let directory = tempfile::tempdir().unwrap();
        let base = directory.path().join("base.local.toml");
        let secret = directory.path().join("secret.local.toml");
        std::fs::write(&base, "[integrations.anker_solix.devices.rack]\nmodel = 's2000'\nadapter = 'hci0'\naddress = '00:11:22:33:44:55'\nserial = 'TEST1234567890123'\n").unwrap();
        std::fs::write(&secret, "[integrations.anker_solix.devices.rack]\nclient_id = '11111111-2222-4333-8444-555555555555'\n").unwrap();
        let (config, diagnostics) = crate::Config::from_files(&[base, secret]).unwrap();
        assert!(diagnostics.0.is_empty());
        let integration = config.integrations.anker_solix.as_ref().unwrap();
        assert_eq!(
            integration.devices[&"rack".parse::<DeviceName>().unwrap()]
                .client_id
                .to_string(),
            "11111111-2222-4333-8444-555555555555"
        );
        assert!(!format!("{config:?}").contains("11111111-2222-4333-8444-555555555555"));
    }

    const DEVICE: &str = "[integrations.anker_solix.devices.rack]\nmodel = 's2000'\nadapter = 'hci0'\naddress = '00:11:22:33:44:55'\nserial = 'TEST1234567890123'\nclient_id = '11111111-2222-4333-8444-555555555555'\n";

    fn load(
        contents: &str,
    ) -> Result<(crate::Config, hearthd_config::Diagnostics), hearthd_config::Diagnostics> {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.local.toml");
        std::fs::write(&path, contents).unwrap();
        crate::Config::from_files(&[path])
    }

    #[test]
    fn file_loading_reports_all_invalid_device_fields_at_their_sources() {
        let invalid = DEVICE
            .replace("'s2000'", "'other'")
            .replace("'hci0'", "'radio'")
            .replace("'00:11:22:33:44:55'", "'bad-address'")
            .replace("'TEST1234567890123'", "'bad serial'")
            .replace("'11111111-2222-4333-8444-555555555555'", "'not-a-uuid'");
        let diagnostics = load(&invalid).unwrap_err();
        let rendered = hearthd_config::format_diagnostics_plain(&diagnostics.0);
        for field in ["model", "adapter", "address", "serial", "client_id"] {
            assert!(
                rendered.contains(&format!("'integrations.anker_solix.devices.rack.{field}'")),
                "{rendered}"
            );
        }
        assert_eq!(
            diagnostics
                .0
                .iter()
                .filter(|diagnostic| diagnostic.is_error())
                .count(),
            5
        );
    }

    #[test]
    fn file_loading_rejects_duplicate_devices_and_empty_names() {
        let first = DEVICE.replace("'00:11:22:33:44:55'", "'AA:11:22:33:44:55'");
        let second = first
            .replace("devices.rack", "devices.other")
            .replace("'AA:11:22:33:44:55'", "'aa:11:22:33:44:55'");
        let duplicate = format!("{first}\n{second}");
        let diagnostics = load(&duplicate).unwrap_err();
        let rendered = hearthd_config::format_diagnostics_plain(&diagnostics.0);
        assert!(
            rendered.contains("duplicate physical Bluetooth address"),
            "{rendered}"
        );
        assert!(rendered.contains("duplicate device serial"), "{rendered}");
        let diagnostics = load(&DEVICE.replace("devices.rack", "devices.\"\"")).unwrap_err();
        assert!(
            hearthd_config::format_diagnostics_plain(&diagnostics.0)
                .contains("device name must not be empty")
        );
    }

    #[test]
    fn an_empty_table_loads_without_creating_an_integration() {
        let (config, diagnostics) = load("[integrations.anker_solix]").unwrap();
        assert!(diagnostics.0.is_empty());
        assert!(
            config
                .integrations
                .anker_solix
                .as_ref()
                .unwrap()
                .devices
                .is_empty()
        );
        let context = crate::engine::IntegrationContext { config: &config };
        assert!(super::super::init(&context).unwrap().is_none());
    }

    #[test]
    fn unknown_device_keys_warn_and_missing_client_ids_fail() {
        let (_, diagnostics) = load(&format!("{DEVICE}adaptor = 'hci1'\n")).unwrap();
        assert_eq!(diagnostics.0.len(), 1);
        assert!(diagnostics.0[0].is_warning());
        let rendered = hearthd_config::format_diagnostics_plain(&diagnostics.0);
        assert!(rendered.contains("adaptor"), "{rendered}");
        let diagnostics =
            load(&DEVICE.replace("client_id = '11111111-2222-4333-8444-555555555555'\n", ""))
                .unwrap_err();
        let rendered = hearthd_config::format_diagnostics_plain(&diagnostics.0);
        assert!(rendered.contains("client_id is required"), "{rendered}");
    }
}
