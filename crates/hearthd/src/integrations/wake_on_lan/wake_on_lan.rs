//! Wake-on-LAN integration for hearthd.
//!
//! Each configured host is exposed as a `switch.<name>` entity whose `OnOff`
//! attribute mirrors whether the host answers an ICMP ping. A background task
//! pings each host on the configured interval and republishes the cluster only
//! when reachability flips, so the switch state is a live "is it up" sensor.
//!
//! Commands on the switch cannot power a host down — there is no standard,
//! unprivileged way to do that — so only the "on" direction does anything:
//! `On` (and `Toggle` while the host is offline) sends a Wake-on-LAN magic
//! packet to the host's MAC address. `Off`, and `Toggle` while it is already
//! up, are no-ops.
//!
//! Pings use [`surge_ping`], which on Linux can run unprivileged over `DGRAM`
//! ICMP sockets subject to `net.ipv4.ping_group_range`, falling back to raw
//! sockets only where necessary.

use std::collections::HashMap;
use std::error::Error;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use surge_ping::Client;
use surge_ping::Config as PingConfig;
use surge_ping::PingIdentifier;
use surge_ping::PingSequence;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::debug;
use tracing::info;
use tracing::warn;

use super::config::Config;
use super::config::HostConfig;
use crate::engine::Event;
use crate::engine::EventSender;
use crate::engine::Integration;
use crate::engine::NodeId;
use crate::engine::NodeIdAllocator;
use crate::engine::ToIntegrationMessage;
use crate::matter::Cluster;
use crate::matter::ClusterCommand;
use crate::matter::DeviceType;
use crate::matter::Endpoint;
use crate::matter::EndpointId;
use crate::matter::Node;
use crate::matter::OnOffCluster;
use crate::matter::OnOffCommand;

/// Integration name reported to the engine.
const INTEGRATION_NAME: &str = "wake_on_lan";

/// Endpoint ID assigned to every WoL switch (the standard Matter root
/// application endpoint).
const WOL_ENDPOINT: EndpointId = 1;

/// Mutable runtime view of one configured host.
#[derive(Debug, Clone)]
struct Host {
    /// Configuration key, used for the entity id (`switch.<key>`).
    key: String,
    config: HostConfig,
    node_id: NodeId,
    /// Display name.
    name: String,
    /// Whether the host last answered a ping.
    on_off: bool,
}

/// Everything built during `setup` and shared with the background tasks.
struct State {
    /// One shared ICMP socket, cloneable across the per-host ping tasks.
    client: Client,
    /// Socket magic packets are sent through. Bound without a peer so each
    /// send targets its own computed broadcast address.
    socket: Arc<UdpSocket>,
    to_engine: EventSender,
    hosts: Mutex<Vec<Host>>,
    ping_timeout: Duration,
}

/// Wake-on-LAN integration.
pub struct WolIntegration {
    config: Config,
    state: Option<Arc<State>>,
    tasks: Vec<JoinHandle<()>>,
}

impl WolIntegration {
    /// Create a new integration from configuration.
    ///
    /// Node ids are not allocated here: the engine hands the allocator to
    /// `setup`, and that one is the only one whose ids are unique across
    /// integrations.
    pub fn new(config: Config) -> Self {
        Self {
            config,
            state: None,
            tasks: Vec::new(),
        }
    }

    /// Read what the engine last published for `node_id`.
    async fn is_on(&self, state: &Arc<State>, node_id: NodeId) -> bool {
        state
            .hosts
            .lock()
            .await
            .iter()
            .any(|h| h.node_id == node_id && h.on_off)
    }

    /// Send a magic packet to wake the host owning `node_id`.
    async fn wake(&self, state: &Arc<State>, node_id: NodeId) -> Result<()> {
        let (socket, host) = {
            let hosts = state.hosts.lock().await;
            let host = hosts
                .iter()
                .find(|h| h.node_id == node_id)
                .context("no WoL host for node")?
                .clone();
            (state.socket.clone(), host)
        };

        let target = wake_target(&host.config).await?;
        let mac = parse_mac(&host.config.mac).context("invalid MAC address in config")?;

        socket.send_to(&build_magic_packet(&mac), target).await?;
        info!(
            "WoL magic packet sent to {} for {node_id} ({})",
            target, host.name
        );
        Ok(())
    }

    /// Carry out one message from the engine.
    async fn invoke(&self, msg: ToIntegrationMessage) -> Result<()> {
        let state = self
            .state
            .as_ref()
            .context("wake_on_lan integration is not set up")?;

        match msg {
            ToIntegrationMessage::InvokeCommand {
                node_id,
                endpoint_id,
                command,
            } => {
                if endpoint_id != WOL_ENDPOINT {
                    anyhow::bail!("unknown endpoint {endpoint_id} on node {node_id}");
                }

                match command {
                    ClusterCommand::OnOff(OnOffCommand::On) => self.wake(state, node_id).await,
                    ClusterCommand::OnOff(OnOffCommand::Toggle) => {
                        // A toggle is only meaningful in the "on" direction:
                        // there is no way to power a host off, so a toggle on
                        // an already-up host does nothing.
                        if self.is_on(state, node_id).await {
                            Ok(())
                        } else {
                            self.wake(state, node_id).await
                        }
                    }
                    ClusterCommand::OnOff(OnOffCommand::Off) => {
                        // No standard, unprivileged way to power a host down:
                        // a no-op, as turning a WoL switch "off" must not send
                        // anything.
                        debug!("wake_on_lan: Off on {node_id} is a no-op");
                        Ok(())
                    }
                    other => anyhow::bail!("no WoL mapping for node {node_id} command {other:?}"),
                }
            }
        }
    }

    /// Shared `setup` body, so the trait boundary can box the error once.
    async fn setup_inner(&mut self, tx: EventSender, node_ids: NodeIdAllocator) -> Result<()> {
        let client =
            Client::new(&PingConfig::default()).context("failed to open the ICMP ping socket")?;

        // Magic packets are UDP broadcasts, so the socket needs SO_BROADCAST,
        // and it must be nonblocking for tokio's `UdpSocket::from_std`.
        let std_sock = std::net::UdpSocket::bind("0.0.0.0:0")
            .context("failed to bind the Wake-on-LAN socket")?;
        std_sock
            .set_broadcast(true)
            .context("failed to enable broadcast on the Wake-on-LAN socket")?;
        std_sock
            .set_nonblocking(true)
            .context("failed to set the Wake-on-LAN socket nonblocking")?;
        let socket = Arc::new(UdpSocket::from_std(std_sock)?);

        let hosts: Vec<Host> = self
            .config
            .hosts
            .iter()
            .map(|(key, cfg)| Host {
                key: key.clone(),
                config: cfg.clone(),
                node_id: node_ids.allocate(),
                name: cfg.name.clone().unwrap_or_else(|| key.clone()),
                on_off: false,
            })
            .collect();

        // Announce every host. They start offline until the first ping says
        // otherwise, and flip up individually as their own ping task fires.
        for host in &hosts {
            tx.send(Event::NodeAdded {
                node_id: host.node_id,
                node: node_for(host),
            })
            .await
            .context("engine channel closed")?;
        }

        let state = Arc::new(State {
            client,
            socket,
            to_engine: tx,
            hosts: Mutex::new(hosts),
            ping_timeout: Duration::from_millis(self.config.ping_timeout_ms),
        });

        // One task per host, each sharing the single ICMP socket.
        for host in state.hosts.lock().await.iter().cloned() {
            self.tasks.push(spawn_ping_task(state.clone(), host));
        }

        self.state = Some(state);
        info!(
            "WoL integration started with {} host(s)",
            self.config.hosts.len()
        );
        Ok(())
    }
}

/// Resolve a configured host to the IPv4 address the magic packet should reach.
///
/// When no explicit `broadcast` is set, wake a host from its own address by
/// computing the subnet-directed broadcast from the configured netmask, which
/// is how Wake on LAN is conventionally sent. A hostname is resolved only when
/// a broadcast must be derived; an explicit broadcast never needs resolving.
async fn wake_target(config: &HostConfig) -> Result<SocketAddr> {
    let ip = match config.broadcast.as_deref() {
        Some(broadcast) => broadcast
            .parse::<Ipv4Addr>()
            .context("invalid broadcast address in config")?,
        None => {
            let host_ip = resolve_ipv4(&config.host).await?;
            directed_broadcast(host_ip, parse_netmask(config.netmask.as_deref())?)
        }
    };
    Ok(SocketAddr::new(IpAddr::V4(ip), config.port))
}

/// The configured netmask, or the conventional /24 home-LAN default.
fn parse_netmask(netmask: Option<&str>) -> Result<Ipv4Addr> {
    match netmask {
        None => Ok(Ipv4Addr::new(255, 255, 255, 0)),
        Some(s) => s.parse().context("invalid netmask in config"),
    }
}

/// The subnet-directed broadcast for `ip` under `netmask`.
fn directed_broadcast(ip: Ipv4Addr, netmask: Ipv4Addr) -> Ipv4Addr {
    let network = u32::from(ip) & u32::from(netmask);
    let broadcast = network | !u32::from(netmask);
    Ipv4Addr::from(broadcast)
}

/// Resolve `host` to an IPv4 address, short-circuiting a literal.
async fn resolve_ipv4(host: &str) -> Result<Ipv4Addr> {
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return Ok(ip);
    }
    let addrs = tokio::net::lookup_host((host, 0))
        .await
        .with_context(|| format!("failed to resolve {host}"))?;
    addrs
        .filter_map(|addr| match addr.ip() {
            IpAddr::V4(v4) => Some(v4),
            _ => None,
        })
        .next()
        .with_context(|| format!("no IPv4 address for {host}"))
}

/// Parse a MAC address in `AA:BB:CC:DD:EE:FF` (or dash-separated) form.
fn parse_mac(input: &str) -> Option<[u8; 6]> {
    let separator = if input.contains(':') { ':' } else { '-' };
    let octets: Vec<u8> = input
        .split(separator)
        .map(|s| u8::from_str_radix(s, 16))
        .collect::<std::result::Result<_, _>>()
        .ok()?;
    octets.try_into().ok()
}

/// Build a Wake-on-LAN magic packet: 6 `0xff` bytes followed by the MAC
/// repeated 16 times.
fn build_magic_packet(mac: &[u8; 6]) -> [u8; 102] {
    let mut packet = [0xff; 102];
    for block in 0..16 {
        packet[6 + block * 6..6 + (block + 1) * 6].copy_from_slice(mac);
    }
    packet
}

/// Build the Matter `Node` snapshot for a host.
fn node_for(host: &Host) -> Node {
    let mut endpoint = Endpoint::default().with_device_types([DeviceType::OnOffPlugInUnit]);
    endpoint.clusters.insert(
        crate::matter::CLUSTER_NAME_ON_OFF.to_string(),
        Cluster::OnOff(OnOffCluster {
            on_off: host.on_off,
        }),
    );
    let mut endpoints = HashMap::new();
    endpoints.insert(WOL_ENDPOINT, endpoint);

    Node {
        entity_id: format!("switch.{}", host.key),
        integration: INTEGRATION_NAME.to_string(),
        name: Some(host.name.clone()),
        endpoints,
    }
}

/// Ping `host` on its interval and republish the switch when its reachability
/// flips.
fn spawn_ping_task(state: Arc<State>, host: Host) -> JoinHandle<()> {
    tokio::spawn(async move {
        let client = state.client.clone();
        // A single in-flight ping per host, so the sequence only needs to
        // differ from its own previous value.
        let mut seq = 0u16;
        loop {
            match resolve_ipv4(&host.config.host).await {
                Ok(ip) => {
                    let mut pinger = client.pinger(IpAddr::V4(ip), PingIdentifier(1)).await;
                    pinger.timeout(state.ping_timeout);
                    let result = pinger.ping(PingSequence(seq), &[0u8; 8]).await;
                    seq = seq.wrapping_add(1);
                    let online = result.is_ok();
                    debug!(
                        "wake_on_lan: pinged {} ({ip}): {}",
                        host.name,
                        if online { "online" } else { "offline" }
                    );
                    report_reachability(&state, host.node_id, online).await;
                }
                Err(e) => warn!("wake_on_lan: failed to resolve {}: {e}", host.config.host),
            }
            tokio::time::sleep(Duration::from_millis(host.config.ping_interval_ms)).await;
        }
    })
}

/// Record a ping result and, if reachability changed, publish it to the engine.
async fn report_reachability(state: &Arc<State>, node_id: NodeId, on_off: bool) {
    let changed = {
        let mut hosts = state.hosts.lock().await;
        if let Some(host) = hosts.iter_mut().find(|h| h.node_id == node_id) {
            if host.on_off != on_off {
                host.on_off = on_off;
                Some(OnOffCluster { on_off })
            } else {
                None
            }
        } else {
            None
        }
    };

    if let Some(cluster) = changed {
        if state
            .to_engine
            .send(Event::Report {
                node_id,
                endpoint_id: WOL_ENDPOINT,
                cluster: Cluster::OnOff(cluster),
            })
            .await
            .is_err()
        {
            warn!("wake_on_lan: engine channel closed");
        }
    }
}

#[async_trait]
impl Integration for WolIntegration {
    fn name(&self) -> &str {
        INTEGRATION_NAME
    }

    async fn setup(
        &mut self,
        tx: EventSender,
        node_ids: NodeIdAllocator,
    ) -> Result<(), Box<dyn Error + Send>> {
        // Boxed once here rather than at every `?`: `Box<dyn Error + Send>`
        // has no blanket `From` impl, so a typed error inside keeps the body
        // free of per-site boxing. `anyhow::Error` converts through its own
        // `From<Error> for Box<dyn Error + Send>` impl.
        self.setup_inner(tx, node_ids)
            .await
            .map_err(|e| -> Box<dyn Error + Send> { e.into() })
    }

    async fn handle_message(
        &mut self,
        msg: ToIntegrationMessage,
    ) -> Result<(), Box<dyn Error + Send>> {
        self.invoke(msg)
            .await
            .map_err(|e| -> Box<dyn Error + Send> { e.into() })
    }

    async fn shutdown(&mut self) -> Result<(), Box<dyn Error + Send>> {
        for task in self.tasks.drain(..) {
            task.abort();
        }
        self.state = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_is_a_conformant_on_off_plug_in_unit() {
        let host = Host {
            key: "desktop".to_string(),
            config: HostConfig {
                host: "192.168.1.50".to_string(),
                mac: "AA:BB:CC:DD:EE:FF".to_string(),
                name: None,
                port: 9,
                ping_interval_ms: 30_000,
                broadcast: None,
                netmask: None,
            },
            node_id: NodeId::from_raw(1),
            name: "Desktop".to_string(),
            on_off: false,
        };

        let node = node_for(&host);
        let endpoint = &node.endpoints[&WOL_ENDPOINT];
        assert_eq!(endpoint.device_types, [DeviceType::OnOffPlugInUnit]);
        assert_eq!(endpoint.missing_mandatory_clusters(), []);
    }

    #[test]
    fn build_magic_packet_is_6_ffs_then_16_repeats_of_the_mac() {
        let mac = [0xde, 0xad, 0xbe, 0xef, 0x00, 0x01];
        let packet = build_magic_packet(&mac);

        assert_eq!(packet.len(), 102);
        assert_eq!(&packet[..6], &[0xff; 6]);
        for block in 0..16 {
            assert_eq!(&packet[6 + block * 6..6 + (block + 1) * 6], &mac);
        }
    }

    #[test]
    fn parse_mac_accepts_colons_and_dashes() {
        assert_eq!(
            parse_mac("AA:BB:CC:DD:EE:FF"),
            Some([0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff])
        );
        assert_eq!(
            parse_mac("de-ad-be-ef-00-01"),
            Some([0xde, 0xad, 0xbe, 0xef, 0x00, 0x01])
        );
        assert_eq!(parse_mac("nonsense"), None);
        assert_eq!(parse_mac("AA:BB:CC:DD:EE"), None);
    }

    #[test]
    fn directed_broadcast_or_includes_the_broadcast_bits() {
        let ip: Ipv4Addr = "192.168.1.50".parse().unwrap();
        let netmask: Ipv4Addr = "255.255.255.0".parse().unwrap();
        assert_eq!(
            directed_broadcast(ip, netmask),
            "192.168.1.255".parse::<Ipv4Addr>().unwrap()
        );
    }
}
