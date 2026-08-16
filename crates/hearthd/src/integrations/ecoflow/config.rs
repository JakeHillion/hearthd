//! Configuration for the EcoFlow integration.
//!
//! # Why devices are declared rather than discovered
//!
//! The consumer API cannot enumerate devices: the endpoint that would do so
//! returns nothing for private-API clients. There is therefore no discovery
//! step, and every device is named in configuration by its serial number,
//! which is printed on the unit and shown in the EcoFlow app.
//!
//! Nothing about a device's capabilities is discovered at runtime either. The
//! Wave 3's feature set is fixed and known ahead of time, so the clusters a
//! device exposes are a static property of its declared type.
//!
//! # Secrets
//!
//! `email` and `password` are the account credentials, and the account
//! password grants full control of every device on it. hearthd merges several
//! TOML files into one configuration, so the intended arrangement is to keep
//! these two fields in a separate, tightly permissioned file listed alongside
//! the main config rather than inline with everything else.

use std::collections::HashMap;

use hearthd_config::SubConfig;
use hearthd_config::TryFromPartial;
use serde::Deserialize;

fn default_api_host() -> String {
    super::cloud::auth::DEFAULT_API_HOST.to_string()
}

/// Configuration for the EcoFlow integration.
#[derive(Debug, Clone, Deserialize, TryFromPartial, SubConfig)]
pub struct Config {
    /// Private-API host. Not region-selected: EcoFlow routes by account, so
    /// the default suits every account.
    ///
    /// This is not the public developer API. `api-e.ecoflow.com` and
    /// `api-a.ecoflow.com` speak a different protocol that does not expose the
    /// Wave 3.
    #[config(default = "default_api_host")]
    pub api_host: String,

    /// EcoFlow account email.
    pub email: String,

    /// EcoFlow account password, in plaintext. A secret; keep it in a
    /// separately loaded file.
    pub password: String,

    /// Declared devices, keyed by the name they are exposed under.
    pub devices: HashMap<String, DeviceConfig>,
}

/// One declared device.
#[derive(Debug, Clone, Deserialize, TryFromPartial, SubConfig)]
// Required: the toml deserializer cannot wrap fields in Spanned when the
// parent map is a HashMap.
#[config(no_span)]
pub struct DeviceConfig {
    /// Serial number, printed on the unit and shown in the EcoFlow app.
    pub serial: String,

    /// Human-readable name. Defaults to the configuration key.
    pub name: Option<String>,
}
