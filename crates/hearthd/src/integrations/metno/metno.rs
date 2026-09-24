use std::collections::HashMap;
use std::error::Error;
use std::time::Duration;

use async_trait::async_trait;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;
use tracing::debug;
use tracing::info;
use tracing::warn;

use super::Site;
use super::forecast;
use super::forecast::ForecastResponse;
use crate::engine::FromIntegrationMessage;
use crate::engine::FromIntegrationSender;
use crate::engine::Integration;
use crate::engine::IntegrationRegistry;
use crate::engine::NodeId;
use crate::engine::RegisteredNode;
use crate::engine::ToIntegrationMessage;
use crate::matter::Cluster;
use crate::matter::Endpoint;
use crate::matter::EndpointId;
use crate::matter::Node;

const INTEGRATION_NAME: &str = "metno";

/// The `complete` product rather than `compact`: the extra parameters it
/// carries (dew point, UV index, cloud fractions) are ones hearthd models.
const FORECAST_URL: &str = "https://api.met.no/weatherapi/locationforecast/2.0/complete";

/// The one endpoint every weather node exposes.
const METNO_ENDPOINT: EndpointId = 1;

/// How often each site is polled. met.no data updates roughly hourly and its
/// responses expire after ~30 minutes; polling at that cadence with
/// conditional requests means most polls return a cheap `304 Not Modified`.
const POLL_INTERVAL: Duration = Duration::from_secs(30 * 60);

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// met.no's terms require an identifying User-Agent naming the application and
/// a contact point; requests without one are rejected.
fn user_agent() -> String {
    format!(
        "hearthd/{} (+https://hearthd.dev)",
        env!("CARGO_PKG_VERSION")
    )
}

/// Live per-site state owned by the polling task.
struct SiteState {
    /// Holds the node registration for as long as the site is polled;
    /// dropping it gives up the name.
    node: RegisteredNode,
    site: Site,
    /// Last cluster snapshot published to the engine, keyed by cluster name.
    last: HashMap<String, Cluster>,
    /// `Last-Modified` from the most recent `200` response, echoed back as
    /// `If-Modified-Since` on the next request.
    last_modified: Option<String>,
}

/// met.no weather integration: a read-only poller that publishes one weather
/// node per configured location.
pub struct MetnoIntegration {
    sites: Vec<Site>,
    _task: Option<JoinHandle<()>>,
}

impl MetnoIntegration {
    pub fn new(sites: Vec<Site>) -> Self {
        Self { sites, _task: None }
    }

    fn build_node(name: &str, entity_id: &str) -> Node {
        let clusters = forecast::null_clusters()
            .into_iter()
            .map(|c| (c.name().to_string(), c))
            .collect();

        let mut endpoints = HashMap::new();
        endpoints.insert(METNO_ENDPOINT, Endpoint { clusters });

        Node {
            entity_id: entity_id.to_string(),
            integration: INTEGRATION_NAME.to_string(),
            name: Some(format!("{name} weather")),
            endpoints,
        }
    }

    /// Fetch the forecast for a site.
    ///
    /// `Ok(None)` is a `304 Not Modified`; `Ok(Some(..))` carries the parsed
    /// response and its `Last-Modified` header.
    async fn fetch(
        client: &reqwest::Client,
        site: &Site,
        if_modified_since: Option<&str>,
    ) -> anyhow::Result<Option<(ForecastResponse, Option<String>)>> {
        // met.no rejects coordinates with more than four decimals (they hurt
        // its cache), and wants altitude as an integer number of metres.
        let mut query: Vec<(&str, String)> = vec![
            ("lat", format!("{:.4}", site.latitude)),
            ("lon", format!("{:.4}", site.longitude)),
        ];
        if let Some(elevation) = site.elevation_m {
            query.push(("altitude", (elevation.round() as i64).to_string()));
        }

        let mut request = client.get(FORECAST_URL).query(&query);
        if let Some(since) = if_modified_since {
            request = request.header(reqwest::header::IF_MODIFIED_SINCE, since);
        }

        let response = request.send().await?;
        if response.status() == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(None);
        }
        let response = response.error_for_status()?;

        let last_modified = response
            .headers()
            .get(reqwest::header::LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);

        let forecast = response.json::<ForecastResponse>().await?;
        Ok(Some((forecast, last_modified)))
    }

    /// Poll every site forever, emitting an `AttributeChanged` for each cluster
    /// whose value differs from the last one published.
    async fn run(
        client: reqwest::Client,
        mut sites: Vec<SiteState>,
        to_engine: FromIntegrationSender,
    ) {
        // The first tick fires immediately, so sites are fetched at startup.
        let mut interval = tokio::time::interval(POLL_INTERVAL);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            interval.tick().await;

            for state in sites.iter_mut() {
                match Self::fetch(&client, &state.site, state.last_modified.as_deref()).await {
                    Ok(None) => {
                        debug!("metno: {} not modified", state.site.name);
                    }
                    Ok(Some((forecast, last_modified))) => {
                        state.last_modified = last_modified;
                        for cluster in forecast.current_clusters() {
                            let name = cluster.name();
                            if state.last.get(name) == Some(&cluster) {
                                continue;
                            }
                            state.last.insert(name.to_string(), cluster.clone());
                            Self::send_attribute_changed(state.node.node_id(), cluster, &to_engine)
                                .await;
                        }
                    }
                    Err(e) => {
                        warn!("metno: fetch for {} failed: {}", state.site.name, e);
                    }
                }
            }
        }
    }

    async fn send_attribute_changed(
        node_id: NodeId,
        cluster: Cluster,
        to_engine: &FromIntegrationSender,
    ) {
        if let Err(e) = to_engine
            .send(FromIntegrationMessage::AttributeChanged {
                node_id,
                endpoint_id: METNO_ENDPOINT,
                cluster,
            })
            .await
        {
            warn!("metno: failed to send AttributeChanged: {}", e);
        }
    }
}

#[async_trait]
impl Integration for MetnoIntegration {
    fn name(&self) -> &str {
        INTEGRATION_NAME
    }

    async fn setup(
        &mut self,
        tx: FromIntegrationSender,
        nodes: IntegrationRegistry,
    ) -> Result<(), Box<dyn Error + Send>> {
        // Left to itself reqwest has no crypto provider at all under
        // `rustls-no-provider`, and builds its roots from the host's trust
        // store, which is absent in a build sandbox or a minimal container.
        let tls = crate::tls::client_config()
            .map_err(|e| -> Box<dyn Error + Send> { Box::new(std::io::Error::other(e)) })?;

        let client = reqwest::Client::builder()
            .user_agent(user_agent())
            .timeout(REQUEST_TIMEOUT)
            .use_preconfigured_tls((*tls).clone())
            .https_only(true)
            .build()
            .map_err(|e| -> Box<dyn Error + Send> {
                Box::new(std::io::Error::other(e.to_string()))
            })?;

        let mut sites = Vec::with_capacity(self.sites.len());
        for site in self.sites.drain(..) {
            let requested = format!("weather.{}", site.name);
            let declared = Self::build_node(&site.name, &requested);
            // Seed the diff baseline with the same clusters we announce.
            let last = declared.endpoints[&METNO_ENDPOINT].clusters.clone();

            // A rejected site is skipped rather than failing setup: the other
            // configured locations are unaffected by one bad name.
            let node = match nodes.register(declared).await {
                Ok(node) => node,
                Err(e) => {
                    warn!("metno: cannot register a node for '{}': {}", site.name, e);
                    continue;
                }
            };

            info!(
                "metno: discovered weather node {} ({})",
                node.entity_id(),
                node.node_id()
            );

            sites.push(SiteState {
                node,
                site,
                last,
                last_modified: None,
            });
        }

        let task = tokio::spawn(async move {
            Self::run(client, sites, tx).await;
        });
        self._task = Some(task);

        info!("metno integration setup complete");
        Ok(())
    }

    async fn handle_message(
        &mut self,
        msg: ToIntegrationMessage,
    ) -> Result<(), Box<dyn Error + Send>> {
        match msg {
            ToIntegrationMessage::InvokeCommand { node_id, .. } => {
                Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("metno node {node_id} is read-only"),
                )))
            }
        }
    }

    async fn shutdown(&mut self) -> Result<(), Box<dyn Error + Send>> {
        info!("metno integration shutting down");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_node_advertises_all_weather_clusters() {
        let node = MetnoIntegration::build_node("home", "weather.home");
        assert_eq!(node.entity_id, "weather.home");
        assert_eq!(node.integration, "metno");
        let endpoint = node.endpoints.get(&METNO_ENDPOINT).unwrap();
        assert_eq!(endpoint.clusters.len(), 9);
        assert!(endpoint.clusters.contains_key("WeatherCondition"));
        assert!(endpoint.clusters.contains_key("WindMeasurement"));
    }

    /// reqwest is built with `rustls-no-provider`, so a client built without an
    /// explicit provider panics rather than failing: setup has to supply one.
    #[tokio::test]
    async fn setup_builds_a_client() {
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        // No sites: the poll task starts but makes no requests.
        let mut integration = MetnoIntegration::new(Vec::new());

        integration
            .setup(
                tx.clone(),
                IntegrationRegistry::for_test(INTEGRATION_NAME, tx),
            )
            .await
            .expect("setup should build a client");
    }

    #[tokio::test]
    async fn rejects_commands_as_read_only() {
        use crate::matter::ClusterCommand;
        use crate::matter::OnOffCommand;

        let mut integration = MetnoIntegration::new(Vec::new());
        let result = integration
            .handle_message(ToIntegrationMessage::InvokeCommand {
                node_id: NodeId::from_raw(1),
                endpoint_id: METNO_ENDPOINT,
                command: ClusterCommand::OnOff(OnOffCommand::On),
            })
            .await;
        assert!(result.is_err());
    }
}
