//! Configuration for the Wake-on-LAN integration.
//!
//! Each configured host becomes one `switch.<name>` entity whose `OnOff`
//! attribute reflects whether the host answers an ICMP ping. Toggling the
//! switch on (or invoking `Toggle` while the host is offline) sends a magic
//! packet to wake the host. There is deliberately no way to power a host off,
//! so `Off` is a no-op.

use std::collections::HashMap;

use hearthd_config::FromConfig;

/// Configuration for the Wake-on-LAN integration.
#[derive(Debug, Clone, FromConfig)]
pub struct Config {
    /// ICMP timeout: how long a ping may take before the host counts as
    /// offline, in milliseconds.
    #[config(default = 2_000)]
    pub ping_timeout_ms: u64,

    /// Hosts to monitor and wake, keyed by the name they are exposed under.
    pub hosts: HashMap<String, HostConfig>,
}

/// One host to monitor and wake.
#[derive(Debug, Clone, FromConfig)]
pub struct HostConfig {
    /// Host to ping: an IP address or resolvable hostname.
    pub host: String,

    /// MAC address (e.g. "AA:BB:CC:DD:EE:FF") the magic packet is addressed
    /// to. Required, as it is what actually wakes the host.
    pub mac: String,

    /// Human-readable name. Defaults to the configuration key.
    pub name: Option<String>,

    /// UDP port for the magic packet. "Magic packet" packets are sent to port
    /// 9 and "Wake on Wake" ones to port 7 (default: 9).
    #[config(default = 9)]
    pub port: u16,

    /// How often this host is pinged to refresh its switch state, in
    /// milliseconds.
    #[config(default = 30_000)]
    pub ping_interval_ms: u64,

    /// Explicit address the magic packet is sent to (an IPv4 address).
    /// Defaults to the subnet-directed broadcast derived from `host` and
    /// `netmask` (e.g. 192.168.1.255 for 192.168.1.50/24). Set it to target a
    /// specific host or a broadcast that cannot be derived.
    pub broadcast: Option<String>,

    /// Netmask applied to `host` to compute the directed broadcast. Only used
    /// when `broadcast` is unset (default: 255.255.255.0).
    pub netmask: Option<String>,
}
