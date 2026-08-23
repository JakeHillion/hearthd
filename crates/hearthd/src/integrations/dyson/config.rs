//! Configuration for the Dyson integration.
//!
//! Devices are declared in TOML with the credentials obtained from
//! `hearthd integration dyson login`.

use std::collections::HashMap;

use hearthd_config::SubConfig;
use hearthd_config::TryFromPartial;
use serde::Deserialize;

/// Configuration for the Dyson integration.
#[derive(Debug, Clone, Deserialize, TryFromPartial, SubConfig)]
pub struct Config {
    /// Declared devices, keyed by the name they are exposed under.
    pub devices: HashMap<String, DeviceConfig>,
}

/// One declared Dyson device.
#[derive(Debug, Clone, Deserialize, TryFromPartial, SubConfig)]
#[config(no_span)]
pub struct DeviceConfig {
    /// Device serial number (MQTT username).
    pub serial: String,

    /// Local MQTT credential (MQTT password).
    pub credential: String,

    /// Dyson product type code (e.g. `438` for TP07 Pure Cool).
    pub device_type: String,

    /// Static IP or hostname. Required in this implementation; mDNS is not
    /// supported yet.
    pub host: String,

    /// Human-readable name. Defaults to the configuration key.
    pub name: Option<String>,
}
