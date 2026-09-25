//! Publishing a node by handing over the whole thing.
//!
//! Integrations that learn a device's state wholesale, a Snapcast status
//! fetch or an EcoFlow property upload, would otherwise each keep what they
//! last announced and diff it to decide between re-announcing and reporting
//! attribute changes. [`Publisher`] makes that decision once, engine-side.

use std::collections::BTreeSet;
use std::collections::HashMap;

use tokio::sync::Mutex;

use super::integration::IntegrationSender;
use super::integration::StreamClosed;
use crate::matter::EndpointId;
use crate::matter::LocalKey;
use crate::matter::Node;

/// Emits the minimal events that move the engine's view of a node from what
/// was last published to what an integration now holds.
pub struct Publisher {
    sender: IntegrationSender,
    published: Mutex<HashMap<LocalKey, Node>>,
}

/// The parts of a node that only `NodeAdded` can change: its name, and per
/// endpoint its device types and the set of clusters it carries.
type Shape<'a> = (
    Option<&'a str>,
    BTreeSet<(EndpointId, Vec<String>, Vec<&'a str>)>,
);

/// A difference in the shape re-announces the node; anything else is a
/// per-cluster report.
fn shape(node: &Node) -> Shape<'_> {
    let endpoints = node
        .endpoints
        .iter()
        .map(|(id, endpoint)| {
            let device_types: Vec<String> = endpoint
                .device_types
                .iter()
                .map(|t| t.name().to_string())
                .collect();
            let mut clusters: Vec<&str> = endpoint.clusters.keys().map(String::as_str).collect();
            clusters.sort_unstable();
            (*id, device_types, clusters)
        })
        .collect();
    (node.name.as_deref(), endpoints)
}

impl Publisher {
    pub fn new(sender: IntegrationSender) -> Self {
        Self {
            sender,
            published: Mutex::new(HashMap::new()),
        }
    }

    /// Bring the engine up to date with `node`.
    ///
    /// A node the engine has not seen, or whose name, device types or set of
    /// clusters changed, is announced whole. Otherwise only the clusters
    /// whose contents differ are reported, which is what makes the engine
    /// see attribute changes rather than repeated discovery.
    pub async fn publish(&self, node: Node) -> Result<(), StreamClosed> {
        let mut published = self.published.lock().await;
        let previous = published.get(&node.key);

        let reannounce = match previous {
            None => true,
            Some(previous) => shape(previous) != shape(&node),
        };

        if reannounce {
            self.sender.node_added(node.clone()).await?;
        } else if let Some(previous) = previous {
            for (endpoint_id, endpoint) in &node.endpoints {
                for (name, cluster) in &endpoint.clusters {
                    let unchanged = previous
                        .endpoints
                        .get(endpoint_id)
                        .and_then(|e| e.clusters.get(name))
                        .is_some_and(|p| p == cluster);
                    if !unchanged {
                        self.sender
                            .report(&node.key, *endpoint_id, cluster.clone())
                            .await?;
                    }
                }
            }
        }

        published.insert(node.key.clone(), node);
        Ok(())
    }

    /// Withdraw the node called `key`, if it was ever published.
    pub async fn remove(&self, key: &LocalKey) -> Result<(), StreamClosed> {
        let mut published = self.published.lock().await;
        if published.remove(key).is_some() {
            self.sender.node_removed(key).await?;
        }
        Ok(())
    }

    /// The keys of every node currently published.
    pub async fn published_keys(&self) -> Vec<LocalKey> {
        self.published.lock().await.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;

    use super::*;
    use crate::engine::Event;
    use crate::engine::Stamped;
    use crate::matter::Cluster;
    use crate::matter::DeviceType;
    use crate::matter::Endpoint;
    use crate::matter::LevelControlCluster;
    use crate::matter::OnOffCluster;

    fn publisher() -> (Publisher, mpsc::Receiver<Stamped>) {
        let (tx, rx) = mpsc::channel(16);
        (Publisher::new(IntegrationSender::new("test", tx)), rx)
    }

    fn drain(rx: &mut mpsc::Receiver<Stamped>) -> Vec<Event> {
        let mut events = Vec::new();
        while let Ok(stamped) = rx.try_recv() {
            events.push(stamped.event);
        }
        events
    }

    fn speaker(name: &str, on_off: bool) -> Node {
        let endpoint = Endpoint::from_clusters([Cluster::OnOff(OnOffCluster { on_off })])
            .with_device_types([DeviceType::Speaker]);
        Node {
            key: LocalKey::from("client/a"),
            name: Some(name.to_string()),
            endpoints: HashMap::from([(1, endpoint)]),
        }
    }

    #[tokio::test]
    async fn a_node_the_engine_has_not_seen_is_announced_whole() {
        let (publisher, mut rx) = publisher();

        publisher.publish(speaker("A", true)).await.unwrap();

        assert!(matches!(
            drain(&mut rx).as_slice(),
            [Event::NodeAdded { node, .. }] if node.key == LocalKey::from("client/a")
        ));
    }

    #[tokio::test]
    async fn republishing_an_identical_node_says_nothing() {
        let (publisher, mut rx) = publisher();

        publisher.publish(speaker("A", true)).await.unwrap();
        drain(&mut rx);
        publisher.publish(speaker("A", true)).await.unwrap();

        assert!(drain(&mut rx).is_empty());
    }

    #[tokio::test]
    async fn only_the_clusters_that_differ_are_reported() {
        let (publisher, mut rx) = publisher();

        publisher.publish(speaker("A", true)).await.unwrap();
        drain(&mut rx);
        publisher.publish(speaker("A", false)).await.unwrap();

        match drain(&mut rx).as_slice() {
            [
                Event::Report {
                    endpoint_id: 1,
                    cluster: Cluster::OnOff(c),
                    ..
                },
            ] => assert!(!c.on_off),
            other => panic!("expected one OnOff report, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_renamed_node_is_reannounced() {
        // A report has no field for the name, so a device renamed upstream
        // can only be reported by announcing it again under the same key.
        let (publisher, mut rx) = publisher();

        publisher.publish(speaker("A", true)).await.unwrap();
        drain(&mut rx);
        publisher.publish(speaker("Kitchen", true)).await.unwrap();

        match drain(&mut rx).as_slice() {
            [Event::NodeAdded { node, .. }] => {
                assert_eq!(node.name.as_deref(), Some("Kitchen"));
            }
            other => panic!("expected a re-announcement, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_node_that_gains_a_cluster_is_reannounced() {
        // The engine only learns a node's shape from NodeAdded; a report
        // for a cluster it has never seen on the endpoint would be applied
        // without the device type that should come with it.
        let (publisher, mut rx) = publisher();

        publisher.publish(speaker("A", true)).await.unwrap();
        drain(&mut rx);

        let mut grown = speaker("A", true);
        grown.endpoints.get_mut(&1).unwrap().clusters.insert(
            crate::matter::CLUSTER_NAME_LEVEL_CONTROL.to_string(),
            Cluster::LevelControl(LevelControlCluster::default()),
        );
        publisher.publish(grown).await.unwrap();

        assert!(matches!(
            drain(&mut rx).as_slice(),
            [Event::NodeAdded { .. }]
        ));
    }

    #[tokio::test]
    async fn removal_withdraws_a_published_node_once() {
        let (publisher, mut rx) = publisher();
        let key = LocalKey::from("client/a");

        publisher.remove(&key).await.unwrap();
        assert!(drain(&mut rx).is_empty());

        publisher.publish(speaker("A", true)).await.unwrap();
        drain(&mut rx);
        publisher.remove(&key).await.unwrap();
        publisher.remove(&key).await.unwrap();

        assert!(matches!(
            drain(&mut rx).as_slice(),
            [Event::NodeRemoved { .. }]
        ));
        assert!(publisher.published_keys().await.is_empty());
    }
}
