//! APC integration for hearthd.
//!
//! Each enabled outlet of each configured PDU is a node keyed `<pdu>/<n>`,
//! named by the label the PDU gives the outlet, with one `OnOffPlugInUnit`
//! endpoint. The whole unit's meter is a node keyed `<pdu>` with one
//! `ElectricalSensor` endpoint. A task per PDU polls everything in one GET
//! and publishes only what changed; nodes appear once the PDU has answered
//! and confirmed it is the configured model.
//!
//! The meter reports current only. Rack PDUs of this kind compute their
//! "power" from the measured current and a configured nominal voltage, and
//! hearthd does not pass an estimate off as a measurement.

use std::collections::HashMap;
use std::error::Error;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use strum::VariantNames;
use tokio::sync::Mutex;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tracing::error;
use tracing::info;
use tracing::warn;

use super::config::Config;
use super::config::PduConfig;
use super::profile::Model;
use super::profile::Profile;
use super::snmp::Agent;
use super::snmp::Reading;
use crate::engine::Integration;
use crate::engine::IntegrationSender;
use crate::engine::ToIntegrationMessage;
use crate::matter::CLUSTER_NAME_ON_OFF;
use crate::matter::Cluster;
use crate::matter::ClusterCommand;
use crate::matter::DeviceType;
use crate::matter::ElectricalPowerMeasurementCluster;
use crate::matter::Endpoint;
use crate::matter::EndpointId;
use crate::matter::LocalKey;
use crate::matter::Node;
use crate::matter::OnOffCluster;
use crate::matter::OnOffCommand;
use crate::matter::PowerMode;

/// Integration name reported to the engine.
const INTEGRATION_NAME: &str = "apc";

/// The single endpoint on every node.
const ENDPOINT: EndpointId = 1;

/// How soon a PDU is polled again while a command is still pending on it.
const PENDING_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// How long to wait before retrying a PDU that has not answered yet.
const UNREACHABLE_RETRY_INTERVAL: Duration = Duration::from_secs(30);

/// One configured PDU, validated.
#[derive(Debug)]
struct Pdu {
    key: String,
    model: Model,
    profile: Profile,
    agent: Agent,
    read_community: String,
    write_community: String,
    poll_interval: Duration,
    /// Enabled outlets, in number order.
    outlets: Vec<Outlet>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Outlet {
    number: u8,
    switchable: bool,
}

impl Pdu {
    fn from_config(key: &str, config: &PduConfig) -> Result<Self> {
        let model = Model::from_str(&config.model).map_err(|_| {
            anyhow::anyhow!(
                "pdu {key}: unknown model {:?}; supported models are {}",
                config.model,
                Model::VARIANTS.join(", ")
            )
        })?;
        let profile = model.profile();

        let mut outlet_configs = HashMap::new();
        for (number, outlet) in &config.outlets {
            let number = number
                .parse::<u8>()
                .ok()
                .filter(|n| (1..=profile.outlets).contains(n))
                .with_context(|| {
                    format!(
                        "pdu {key}: outlet {number} does not exist; a {model} has outlets 1 to {}",
                        profile.outlets
                    )
                })?;
            outlet_configs.insert(number, outlet);
        }

        let outlets = (1..=profile.outlets)
            .filter_map(|number| match outlet_configs.get(&number) {
                None => Some(Outlet {
                    number,
                    switchable: true,
                }),
                Some(outlet) if outlet.enabled => Some(Outlet {
                    number,
                    switchable: outlet.switchable,
                }),
                Some(_) => None,
            })
            .collect();

        Ok(Self {
            key: key.to_string(),
            model,
            profile,
            agent: Agent::new(
                config.host.clone(),
                config.port,
                Duration::from_millis(config.timeout_ms),
            ),
            read_community: config.read_community.clone(),
            write_community: config.write_community.clone(),
            poll_interval: Duration::from_millis(config.poll_interval_ms),
            outlets,
        })
    }

    fn meter_key(&self) -> LocalKey {
        LocalKey::from(self.key.as_str())
    }

    fn outlet_key(&self, number: u8) -> LocalKey {
        LocalKey::from(format!("{}/{number}", self.key))
    }

    /// The objects one poll reads: the load, then name, state and pending
    /// flag for each enabled outlet.
    fn poll_oids(&self) -> Vec<Vec<u64>> {
        let objects = self.profile.objects;
        let mut oids = vec![objects.load.to_vec()];
        for outlet in &self.outlets {
            for column in [
                objects.outlet_name,
                objects.outlet_state,
                objects.outlet_command_pending,
            ] {
                oids.push(row(column, outlet.number));
            }
        }
        oids
    }

    /// Make sense of the readings `poll_oids` asked for.
    fn parse_poll(&self, readings: &[Reading]) -> Poll {
        let objects = self.profile.objects;
        let load_milliamps = readings[0]
            .integer()
            .map(|load| load * objects.load_milliamps);

        let outlets = self
            .outlets
            .iter()
            .zip(readings[1..].chunks(3))
            .map(|(outlet, row)| {
                let state = row[1].integer();
                OutletPoll {
                    number: outlet.number,
                    label: row[0]
                        .text()
                        .map(str::trim)
                        .filter(|label| !label.is_empty())
                        .map(str::to_string),
                    on: match state {
                        Some(s) if s == objects.state_on => Some(true),
                        Some(s) if s == objects.state_off => Some(false),
                        _ => None,
                    },
                    pending: row[2].integer() == Some(objects.pending),
                }
            })
            .collect();

        Poll {
            load_milliamps,
            outlets,
        }
    }

    /// Every node one poll describes. An outlet whose state did not read as
    /// on or off is left out rather than guessed at.
    fn nodes(&self, poll: &Poll) -> Vec<Node> {
        let mut nodes = vec![node(
            self.meter_key(),
            self.key.clone(),
            Endpoint::from_clusters([Cluster::ElectricalPowerMeasurement(
                ElectricalPowerMeasurementCluster {
                    power_mode: PowerMode::Ac,
                    voltage: None,
                    active_power: None,
                    active_current: poll.load_milliamps,
                    frequency: None,
                },
            )])
            .with_device_types([DeviceType::ElectricalSensor]),
        )];

        for outlet in &poll.outlets {
            let Some(on_off) = outlet.on else { continue };
            nodes.push(node(
                self.outlet_key(outlet.number),
                outlet
                    .label
                    .clone()
                    .unwrap_or_else(|| format!("{} outlet {}", self.key, outlet.number)),
                Endpoint::from_clusters([Cluster::OnOff(OnOffCluster { on_off })])
                    .with_device_types([DeviceType::OnOffPlugInUnit]),
            ));
        }
        nodes
    }

    /// The value to write to an outlet's command object for `command`, given
    /// what the outlet was last seen doing.
    fn command_value(
        &self,
        outlet: Outlet,
        command: &ClusterCommand,
        currently_on: Option<bool>,
    ) -> Result<i64> {
        if !outlet.switchable {
            anyhow::bail!(
                "outlet {} on pdu {} is not switchable",
                outlet.number,
                self.key
            );
        }
        let objects = self.profile.objects;
        let on = match command {
            ClusterCommand::OnOff(OnOffCommand::On) => true,
            ClusterCommand::OnOff(OnOffCommand::Off) => false,
            ClusterCommand::OnOff(OnOffCommand::Toggle) => !currently_on.with_context(|| {
                format!(
                    "outlet {} on pdu {} has no known state to toggle",
                    outlet.number, self.key
                )
            })?,
            other => anyhow::bail!("an outlet cannot carry out {other:?}"),
        };
        Ok(if on {
            objects.command_on
        } else {
            objects.command_off
        })
    }
}

/// What one poll of a PDU read.
#[derive(Debug, Clone, PartialEq)]
struct Poll {
    load_milliamps: Option<i64>,
    outlets: Vec<OutletPoll>,
}

#[derive(Debug, Clone, PartialEq)]
struct OutletPoll {
    number: u8,
    label: Option<String>,
    on: Option<bool>,
    pending: bool,
}

impl Poll {
    fn any_pending(&self) -> bool {
        self.outlets.iter().any(|outlet| outlet.pending)
    }
}

fn row(column: &[u64], index: u8) -> Vec<u64> {
    let mut oid = column.to_vec();
    oid.push(u64::from(index));
    oid
}

fn node(key: LocalKey, name: String, endpoint: Endpoint) -> Node {
    Node {
        key,
        name: Some(name),
        endpoints: HashMap::from([(ENDPOINT, endpoint)]),
    }
}

/// Everything shared between the poll tasks and command handling.
struct Shared {
    to_engine: IntegrationSender,
    /// What has been announced or reported for each node.
    published: Mutex<HashMap<LocalKey, Node>>,
}

impl Shared {
    /// Announce nodes that are new or renamed, and report clusters that
    /// changed.
    async fn publish(&self, nodes: Vec<Node>) {
        let mut published = self.published.lock().await;
        for node in nodes {
            let result = match published.get(&node.key) {
                Some(previous) if previous.name == node.name => {
                    let before = &previous.endpoints[&ENDPOINT].clusters;
                    let mut result = Ok(());
                    for (name, cluster) in &node.endpoints[&ENDPOINT].clusters {
                        if before.get(name) != Some(cluster) {
                            result = self
                                .to_engine
                                .report(&node.key, ENDPOINT, cluster.clone())
                                .await;
                            if result.is_err() {
                                break;
                            }
                        }
                    }
                    result
                }
                _ => self.to_engine.node_added(node.clone()).await,
            };
            if result.is_err() {
                warn!("apc: engine channel closed");
                return;
            }
            published.insert(node.key.clone(), node);
        }
    }

    async fn is_on(&self, key: &LocalKey) -> Option<bool> {
        let published = self.published.lock().await;
        match published.get(key)?.endpoints[&ENDPOINT]
            .clusters
            .get(CLUSTER_NAME_ON_OFF)?
        {
            Cluster::OnOff(cluster) => Some(cluster.on_off),
            _ => None,
        }
    }
}

/// APC integration.
pub struct ApcIntegration {
    pdus: Vec<Arc<Pdu>>,
    /// Wakes a PDU's poll task early, keyed by the PDU's config key.
    refresh: HashMap<String, Arc<Notify>>,
    shared: Option<Arc<Shared>>,
    tasks: Vec<JoinHandle<()>>,
}

impl ApcIntegration {
    /// Create the integration, refusing configuration that does not describe
    /// a supported PDU.
    pub fn new(config: &Config) -> Result<Self> {
        let mut pdus = config
            .pdus
            .iter()
            .map(|(key, pdu)| Pdu::from_config(key, pdu).map(Arc::new))
            .collect::<Result<Vec<_>>>()?;
        pdus.sort_by(|a, b| a.key.cmp(&b.key));
        let refresh = pdus
            .iter()
            .map(|pdu| (pdu.key.clone(), Arc::new(Notify::new())))
            .collect();
        Ok(Self {
            pdus,
            refresh,
            shared: None,
            tasks: Vec::new(),
        })
    }

    /// The PDU and outlet the node called `key` stands for.
    fn outlet(&self, key: &LocalKey) -> Option<(&Arc<Pdu>, Outlet)> {
        let (pdu_key, number) = key.as_str().rsplit_once('/')?;
        let number: u8 = number.parse().ok()?;
        let pdu = self.pdus.iter().find(|pdu| pdu.key == pdu_key)?;
        let outlet = pdu.outlets.iter().find(|o| o.number == number)?;
        Some((pdu, *outlet))
    }

    /// Carry out one message from the engine.
    async fn invoke(&self, msg: ToIntegrationMessage) -> Result<()> {
        let shared = self
            .shared
            .as_ref()
            .context("apc integration is not set up")?;

        match msg {
            ToIntegrationMessage::InvokeCommand {
                key,
                endpoint_id,
                command,
            } => {
                let (pdu, outlet) = self
                    .outlet(&key)
                    .with_context(|| format!("no outlet node {key}"))?;
                if endpoint_id != ENDPOINT {
                    anyhow::bail!("unknown endpoint {endpoint_id} on {key}");
                }

                let value = pdu.command_value(outlet, &command, shared.is_on(&key).await)?;
                pdu.agent
                    .set_integer(
                        &pdu.write_community,
                        &row(pdu.profile.objects.outlet_command, outlet.number),
                        value,
                    )
                    .await
                    .with_context(|| format!("switching {key} failed"))?;
                info!("apc: {command:?} sent to {key}");

                if let Some(refresh) = self.refresh.get(&pdu.key) {
                    refresh.notify_one();
                }
                Ok(())
            }
            ToIntegrationMessage::WriteAttribute {
                key,
                endpoint_id,
                write,
            } => anyhow::bail!(
                "apc does not accept attribute writes: {key} endpoint {endpoint_id} {write:?}"
            ),
        }
    }
}

/// Poll `pdu` for as long as the integration runs.
///
/// Nothing is published until the device has confirmed it is the configured
/// model; a mismatch stops the task for good, as every OID after that would
/// be read against the wrong table.
fn spawn_poll_task(pdu: Arc<Pdu>, shared: Arc<Shared>, refresh: Arc<Notify>) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match pdu
                .agent
                .get(
                    &pdu.read_community,
                    &[pdu.profile.objects.model_number.to_vec()],
                )
                .await
            {
                Ok(readings) => {
                    let reported = readings[0].text().unwrap_or_default().trim().to_string();
                    if reported != pdu.profile.model_number {
                        error!(
                            "apc: pdu {} is configured as a {} but reports model {reported:?}; not using it",
                            pdu.key, pdu.model
                        );
                        return;
                    }
                    info!("apc: pdu {} is a {}", pdu.key, pdu.model);
                    break;
                }
                Err(e) => {
                    warn!("apc: pdu {} did not answer: {e:#}", pdu.key);
                    tokio::time::sleep(UNREACHABLE_RETRY_INTERVAL).await;
                }
            }
        }

        let oids = pdu.poll_oids();
        loop {
            let wait = match pdu.agent.get(&pdu.read_community, &oids).await {
                Ok(readings) => {
                    let poll = pdu.parse_poll(&readings);
                    shared.publish(pdu.nodes(&poll)).await;
                    if poll.any_pending() {
                        PENDING_POLL_INTERVAL
                    } else {
                        pdu.poll_interval
                    }
                }
                Err(e) => {
                    warn!("apc: polling pdu {} failed: {e:#}", pdu.key);
                    pdu.poll_interval
                }
            };
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                () = refresh.notified() => {}
            }
        }
    })
}

#[async_trait]
impl Integration for ApcIntegration {
    fn name(&self) -> &str {
        INTEGRATION_NAME
    }

    async fn setup(&mut self, tx: IntegrationSender) -> Result<(), Box<dyn Error + Send>> {
        let shared = Arc::new(Shared {
            to_engine: tx,
            published: Mutex::new(HashMap::new()),
        });
        for pdu in &self.pdus {
            self.tasks.push(spawn_poll_task(
                pdu.clone(),
                shared.clone(),
                self.refresh[&pdu.key].clone(),
            ));
        }
        self.shared = Some(shared);
        info!("apc integration started with {} PDU(s)", self.pdus.len());
        Ok(())
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
        self.shared = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::apc::config::OutletConfig;

    fn pdu_config(outlets: &[(&str, bool, bool)]) -> PduConfig {
        PduConfig {
            model: "apc_ap7921".to_string(),
            host: "10.0.0.2".to_string(),
            port: 161,
            read_community: "public".to_string(),
            write_community: "secret".to_string(),
            poll_interval_ms: 10_000,
            timeout_ms: 2_000,
            outlets: outlets
                .iter()
                .map(|(n, enabled, switchable)| {
                    (
                        n.to_string(),
                        OutletConfig {
                            enabled: *enabled,
                            switchable: *switchable,
                        },
                    )
                })
                .collect(),
        }
    }

    fn rack(outlets: &[(&str, bool, bool)]) -> Pdu {
        Pdu::from_config("rack", &pdu_config(outlets)).expect("valid config")
    }

    /// Readings for every enabled outlet of `pdu`: labelled `outlet<n>`, on
    /// for odd numbers, nothing pending.
    fn readings(pdu: &Pdu, load_deciamps: i64) -> Vec<Reading> {
        let mut readings = vec![Reading::Integer(load_deciamps)];
        for outlet in &pdu.outlets {
            readings.push(Reading::Text(format!("outlet{}", outlet.number)));
            readings.push(Reading::Integer(if outlet.number % 2 == 1 { 1 } else { 2 }));
            readings.push(Reading::Integer(2));
        }
        readings
    }

    #[test]
    fn unlisted_outlets_are_enabled_and_switchable() {
        let pdu = rack(&[("3", true, false), ("8", false, true)]);

        assert_eq!(
            pdu.outlets,
            [1, 2, 3, 4, 5, 6, 7]
                .map(|number| Outlet {
                    number,
                    switchable: number != 3
                })
                .to_vec()
        );
    }

    #[test]
    fn an_unknown_model_is_refused_with_the_supported_ones() {
        let mut config = pdu_config(&[]);
        config.model = "apc_ap9999".to_string();

        let err = Pdu::from_config("rack", &config).unwrap_err().to_string();
        assert!(err.contains("apc_ap7921"), "{err}");
    }

    #[test]
    fn an_outlet_the_model_does_not_have_is_refused() {
        for number in ["0", "9", "three", "-1"] {
            let err = Pdu::from_config("rack", &pdu_config(&[(number, true, true)]))
                .unwrap_err()
                .to_string();
            assert!(
                err.contains(&format!("outlet {number} does not exist")),
                "{err}"
            );
        }
    }

    #[test]
    fn a_poll_reads_the_load_then_three_columns_per_enabled_outlet() {
        let pdu = rack(&[("8", false, true)]);
        let oids = pdu.poll_oids();

        assert_eq!(oids.len(), 1 + 3 * 7);
        assert_eq!(oids[0], pdu.profile.objects.load);
        assert_eq!(oids[1], row(pdu.profile.objects.outlet_name, 1));
        assert_eq!(oids[2], row(pdu.profile.objects.outlet_state, 1));
        assert_eq!(oids[3], row(pdu.profile.objects.outlet_command_pending, 1));
        assert_eq!(oids[21], row(pdu.profile.objects.outlet_command_pending, 7));
    }

    #[test]
    fn a_poll_parses_load_labels_states_and_pending() {
        let pdu = rack(&[]);
        let mut readings = readings(&pdu, 13);
        // Outlet 2: blank label, unreadable state, command pending.
        readings[4] = Reading::Text("  ".to_string());
        readings[5] = Reading::Missing;
        readings[6] = Reading::Integer(1);

        let poll = pdu.parse_poll(&readings);

        assert_eq!(poll.load_milliamps, Some(1_300));
        assert_eq!(
            poll.outlets[0],
            OutletPoll {
                number: 1,
                label: Some("outlet1".to_string()),
                on: Some(true),
                pending: false,
            }
        );
        assert_eq!(
            poll.outlets[1],
            OutletPoll {
                number: 2,
                label: None,
                on: None,
                pending: true,
            }
        );
        assert!(poll.any_pending());
    }

    #[test]
    fn nodes_are_conformant_and_keyed_by_pdu_and_outlet() {
        let pdu = rack(&[("8", false, true)]);
        let nodes = pdu.nodes(&pdu.parse_poll(&readings(&pdu, 10)));

        assert_eq!(nodes.len(), 1 + 7);
        for node in &nodes {
            assert_eq!(node.endpoints[&ENDPOINT].missing_mandatory_clusters(), []);
        }

        let meter = &nodes[0];
        assert_eq!(meter.key, LocalKey::from("rack"));
        assert_eq!(
            meter.endpoints[&ENDPOINT].device_types,
            [DeviceType::ElectricalSensor]
        );
        assert_eq!(
            meter.endpoints[&ENDPOINT].clusters["ElectricalPowerMeasurement"],
            Cluster::ElectricalPowerMeasurement(ElectricalPowerMeasurementCluster {
                power_mode: PowerMode::Ac,
                voltage: None,
                active_power: None,
                active_current: Some(1_000),
                frequency: None,
            })
        );

        let outlet = &nodes[3];
        assert_eq!(outlet.key, LocalKey::from("rack/3"));
        assert_eq!(outlet.name.as_deref(), Some("outlet3"));
        assert_eq!(
            outlet.endpoints[&ENDPOINT].device_types,
            [DeviceType::OnOffPlugInUnit]
        );
        assert_eq!(
            outlet.endpoints[&ENDPOINT].clusters["OnOff"],
            Cluster::OnOff(OnOffCluster { on_off: true })
        );
    }

    #[test]
    fn an_outlet_with_an_unreadable_state_gets_no_node() {
        let pdu = rack(&[]);
        let mut readings = readings(&pdu, 0);
        readings[5] = Reading::Missing;

        let nodes = pdu.nodes(&pdu.parse_poll(&readings));

        assert!(
            !nodes
                .iter()
                .any(|node| node.key == LocalKey::from("rack/2"))
        );
    }

    #[test]
    fn an_unlabelled_outlet_is_named_after_its_pdu() {
        let pdu = rack(&[]);
        let mut readings = readings(&pdu, 0);
        readings[1] = Reading::Text(String::new());

        let nodes = pdu.nodes(&pdu.parse_poll(&readings));

        assert_eq!(nodes[1].name.as_deref(), Some("rack outlet 1"));
    }

    #[test]
    fn commands_map_to_immediate_on_and_off() {
        let pdu = rack(&[]);
        let outlet = pdu.outlets[0];
        let on = ClusterCommand::OnOff(OnOffCommand::On);
        let off = ClusterCommand::OnOff(OnOffCommand::Off);
        let toggle = ClusterCommand::OnOff(OnOffCommand::Toggle);

        assert_eq!(pdu.command_value(outlet, &on, None).unwrap(), 1);
        assert_eq!(pdu.command_value(outlet, &off, None).unwrap(), 2);
        assert_eq!(pdu.command_value(outlet, &toggle, Some(true)).unwrap(), 2);
        assert_eq!(pdu.command_value(outlet, &toggle, Some(false)).unwrap(), 1);
        assert!(pdu.command_value(outlet, &toggle, None).is_err());
    }

    #[test]
    fn a_non_switchable_outlet_refuses_every_command() {
        let pdu = rack(&[("3", true, false)]);
        let outlet = pdu.outlets[2];

        for command in [OnOffCommand::On, OnOffCommand::Off, OnOffCommand::Toggle] {
            let err = pdu
                .command_value(outlet, &ClusterCommand::OnOff(command), Some(true))
                .unwrap_err()
                .to_string();
            assert!(err.contains("not switchable"), "{err}");
        }
    }

    #[test]
    fn only_enabled_outlets_resolve_from_their_node_key() {
        let integration = ApcIntegration::new(&Config {
            pdus: HashMap::from([("rack".to_string(), pdu_config(&[("8", false, true)]))]),
        })
        .unwrap();

        let (pdu, outlet) = integration.outlet(&LocalKey::from("rack/3")).unwrap();
        assert_eq!((pdu.key.as_str(), outlet.number), ("rack", 3));
        assert!(integration.outlet(&LocalKey::from("rack/8")).is_none());
        assert!(integration.outlet(&LocalKey::from("rack")).is_none());
        assert!(integration.outlet(&LocalKey::from("other/3")).is_none());
    }
}
