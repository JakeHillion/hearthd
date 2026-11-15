use std::collections::HashMap;

use hearthd_config::FromConfig;
use hearthd_config::Validate;
use hearthd_config::Validation;

use super::profile::Model;

/// Configuration for the APC integration.
#[derive(Debug, Clone, FromConfig)]
pub struct Config {
    /// PDUs to monitor and switch, keyed by the name their nodes are keyed
    /// under.
    pub pdus: HashMap<String, PduConfig>,
}

/// One PDU.
#[derive(Debug, Clone, FromConfig)]
pub struct PduConfig {
    /// Which model this is, e.g. "apc_ap7921". The device must report the
    /// same model before the integration will talk to it further.
    pub model: Model,

    /// IP address or hostname of the PDU's management card.
    pub host: String,

    /// SNMP agent UDP port (default: 161).
    #[config(default = 161)]
    pub port: u16,

    /// Community for reads (default: "public").
    #[config(default = "public")]
    pub read_community: String,

    /// Community for switching outlets.
    pub write_community: String,

    /// How often outlet state and load are read, in milliseconds.
    #[config(default = 10_000)]
    pub poll_interval_ms: u64,

    /// How long one SNMP request may take before it is retried, in
    /// milliseconds.
    #[config(default = 2_000)]
    pub timeout_ms: u64,

    /// Per-outlet settings, keyed by outlet number (from 1). Outlets not
    /// listed are enabled and switchable.
    pub outlets: HashMap<u8, OutletConfig>,
}

impl Validate for PduConfig {
    fn validate(&self, validation: &mut Validation<'_>) {
        let outlets = self.model.profile().outlets;
        let mut out_of_range: Vec<u8> = self
            .outlets
            .keys()
            .copied()
            .filter(|number| !(1..=outlets).contains(number))
            .collect();
        out_of_range.sort_unstable();

        let mut field = validation.field("outlets");
        for number in out_of_range {
            field.key(number).error(format!(
                "{} has outlets 1 to {outlets}, not {number}",
                self.model
            ));
        }
    }
}

/// Settings for one outlet.
#[derive(Debug, Clone, FromConfig)]
pub struct OutletConfig {
    /// Whether the outlet gets a node at all (default: true).
    #[config(default = true)]
    pub enabled: bool,

    /// Whether commands may switch the outlet (default: true). When false the
    /// node still reports state, and no write is ever sent for the outlet.
    #[config(default = true)]
    pub switchable: bool,
}
