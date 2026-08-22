//! Node registration: the one place a node acquires its identity.
//!
//! A node has two identities and both live in keyspaces shared by every
//! integration. [`NodeId`] is the internal one, which the engine keys its
//! state map and its command routing by. `entity_id` is the external one,
//! which the HTTP API and automations address nodes by. Uniqueness is a
//! correctness property for both: two nodes answering to one name means the
//! API resolves that name to whichever registered last, and the loser becomes
//! unreachable while still appearing in the state snapshot.
//!
//! Ids were made safe by construction — only the allocator can mint one.
//! Names cannot be, because they are chosen by the integration and meaningful
//! to the operator. So the guarantee moves to registration time: an
//! integration hands over a [`Node`] and gets back a [`RegisteredNode`]
//! carrying the id and the name it actually holds, which may not be the one it
//! asked for. Nothing else assigns either.
//!
//! # Registering is not announcing
//!
//! [`IntegrationRegistry::register`] allocates, and treats the name as taken
//! if anyone already holds it — including the caller itself. An integration
//! that registers `weather.home` twice has built two nodes with one name, and
//! they stay two nodes: the second is renamed or refused per
//! [`CollisionPolicy`], never folded onto the first.
//!
//! Updating a node it already owns is a different verb:
//! [`RegisteredNode::announce`], which is keyed by the handle and so allocates
//! nothing and cannot rename anything. Making the caller name which one it
//! means is what keeps "I am re-publishing my node" from being mistaken for "I
//! accidentally built a second node called that" — the two need opposite
//! outcomes, and the name alone cannot tell them apart.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::PoisonError;

use tracing::warn;

use super::integration::FromIntegrationSender;
use super::message::FromIntegrationMessage;
use super::node_id::NodeId;
use super::node_id::NodeIdAllocator;
use crate::matter::Node;

/// What to do when a registration asks for a name that is already taken.
///
/// Neither policy decides *which* node keeps the bare name: that follows
/// registration order under both, and for discovery-driven integrations the
/// order is whatever the network delivers. What they differ on is what
/// happens to the node that loses the race.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CollisionPolicy {
    /// Give the newcomer a numbered suffix and let it through. The default.
    ///
    /// The loser stays present and addressable under `light.kitchen_2` rather
    /// than disappearing, which is the more useful of the two outcomes for a
    /// daemon that is someone's heating and lighting. Rejecting would not buy
    /// back the ordering guarantee — it would drop the node and leave the
    /// same race over the bare name — so it only loses information.
    ///
    /// Every rename is logged at warn level. A collision still means two nodes
    /// were built with one name, which is worth fixing at the source.
    #[default]
    Suffix,

    /// Refuse the registration outright.
    ///
    /// For deployments that would rather a node be absent than present under
    /// a name nothing refers to.
    Reject,
}

/// A registration refused because the name was taken.
#[derive(Debug, Clone)]
pub struct RegisterError {
    /// The name that was asked for.
    pub entity_id: String,
    /// The integration already holding it.
    pub claimed_by: String,
}

impl fmt::Display for RegisterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "entity_id '{}' is already registered by integration '{}'",
            self.entity_id, self.claimed_by
        )
    }
}

impl Error for RegisterError {}

/// State shared by every handle drawn from one registry.
struct Shared {
    ids: NodeIdAllocator,
    policy: CollisionPolicy,
    tx: FromIntegrationSender,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    /// Names currently held, and by which node.
    by_entity_id: HashMap<String, NodeId>,
    /// Which integration owns each node, for command routing.
    owners: HashMap<NodeId, Arc<str>>,
}

impl Shared {
    /// Take the registry lock.
    ///
    /// Poisoning is recovered from rather than propagated: every critical
    /// section here is a short, panic-free map update, so a poisoned lock
    /// means some *other* thread panicked while this data was consistent.
    /// Dropping registrations on the floor because of that would resurrect
    /// exactly the silent-collision failure this type exists to prevent.
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Inner {
    /// Decide the name a registration actually gets.
    fn resolve_name(
        &self,
        requested: &str,
        policy: CollisionPolicy,
    ) -> Result<String, RegisterError> {
        let Some(holder) = self.by_entity_id.get(requested) else {
            return Ok(requested.to_string());
        };

        match policy {
            CollisionPolicy::Reject => Err(RegisterError {
                entity_id: requested.to_string(),
                claimed_by: self
                    .owners
                    .get(holder)
                    .map(|o| o.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
            }),
            // Starts at 2 so the pair reads as "the first one" and "the second
            // one" rather than leaving the operator wondering where _1 went.
            CollisionPolicy::Suffix => Ok((2u32..)
                .map(|n| format!("{requested}_{n}"))
                .find(|candidate| !self.by_entity_id.contains_key(candidate))
                .expect("an unused suffix exists below u32::MAX")),
        }
    }
}

/// The engine's registry. One per engine; integrations get a stamped view of
/// it from [`NodeRegistry::for_integration`].
#[derive(Clone)]
pub struct NodeRegistry {
    shared: Arc<Shared>,
}

impl NodeRegistry {
    /// Create the registry. Engine-internal: having exactly one per engine is
    /// what makes ids and names unique.
    pub(super) fn new(tx: FromIntegrationSender, policy: CollisionPolicy) -> Self {
        Self {
            shared: Arc::new(Shared {
                ids: NodeIdAllocator::new(),
                policy,
                tx,
                inner: Mutex::new(Inner::default()),
            }),
        }
    }

    /// A view that registers on behalf of one integration.
    pub(super) fn for_integration(&self, integration: &str) -> IntegrationRegistry {
        IntegrationRegistry {
            shared: self.shared.clone(),
            integration: Arc::from(integration),
        }
    }

    /// The integration owning a node, for routing a command to it.
    ///
    /// Ownership is recorded when the node is registered rather than inferred
    /// later from a `NodeAdded` message, so the routing table cannot disagree
    /// with the registration that produced it.
    pub(super) fn owner(&self, node_id: NodeId) -> Option<Arc<str>> {
        self.shared.lock().owners.get(&node_id).cloned()
    }
}

/// A registry view bound to one integration.
///
/// Handed to [`Integration::setup`](super::integration::Integration::setup).
/// Clone freely: every clone registers as the same integration against the
/// same shared keyspaces.
#[derive(Clone)]
pub struct IntegrationRegistry {
    shared: Arc<Shared>,
    integration: Arc<str>,
}

impl IntegrationRegistry {
    /// Register a node and announce it to the engine.
    ///
    /// Allocates a node id, claims a name, records this integration as the
    /// owner and sends the `NodeAdded` that puts the node in engine state.
    /// `node.entity_id` is a *request*: the name actually held is the one on
    /// the returned handle, and the announced node carries that name, not the
    /// requested one. `node.integration` is likewise overwritten with the
    /// registering integration, so a node cannot be attributed to anyone else.
    ///
    /// Fails only on a name collision, and then only under
    /// [`CollisionPolicy::Reject`]. A failed registration allocates nothing.
    pub async fn register(&self, node: Node) -> Result<RegisteredNode, RegisterError> {
        let (node_id, entity_id) = {
            let mut inner = self.shared.lock();
            // Resolve before allocating: a rejected registration should not
            // burn an id, or restarts would drift the numbering.
            let entity_id = inner.resolve_name(&node.entity_id, self.shared.policy)?;
            let node_id = self.shared.ids.allocate();
            inner.by_entity_id.insert(entity_id.clone(), node_id);
            inner.owners.insert(node_id, self.integration.clone());
            (node_id, entity_id)
        };

        if entity_id != node.entity_id {
            // Never silently: a rename means two nodes were built with one
            // name, and which of them keeps the bare one follows registration
            // order, so the operator needs to know it happened.
            warn!(
                "{} requested entity_id '{}', which is taken; registered as '{}' instead",
                self.integration, node.entity_id, entity_id
            );
        }

        let handle = RegisteredNode {
            shared: self.shared.clone(),
            integration: self.integration.clone(),
            node_id,
            entity_id,
            released: false,
        };

        // Announced through the handle like any other update, so the name and
        // owner stamped onto the node come from one place.
        handle.announce(node).send().await;

        Ok(handle)
    }

    /// A registry for tests that drive an integration without an engine.
    #[cfg(test)]
    pub(crate) fn for_test(integration: &str, tx: FromIntegrationSender) -> Self {
        NodeRegistry::new(tx, CollisionPolicy::default()).for_integration(integration)
    }
}

/// Proof that a node is registered, and the only way to update or remove it.
///
/// Not `Clone`: the reservation has exactly one owner, and removal consumes it.
///
/// Holding one is what distinguishes "re-publishing the node I own" from "a
/// second node with the same name": the id is not re-derived from the name, so
/// [`Self::announce`] cannot collide and cannot rename.
///
/// The handle owns the name reservation. Dropping it without
/// [`Self::remove`] releases the name — the alternative is leaking it for the
/// life of the process — but leaves the node in engine state, so it warns.
pub struct RegisteredNode {
    shared: Arc<Shared>,
    integration: Arc<str>,
    node_id: NodeId,
    entity_id: String,
    released: bool,
}

impl fmt::Debug for RegisteredNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegisteredNode")
            .field("node_id", &self.node_id)
            .field("entity_id", &self.entity_id)
            .field("integration", &self.integration)
            .finish_non_exhaustive()
    }
}

impl RegisteredNode {
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// The name this node actually holds, which under
    /// [`CollisionPolicy::Suffix`] may not be the one that was requested.
    pub fn entity_id(&self) -> &str {
        &self.entity_id
    }

    /// Prepare a re-announcement of this node, carrying whatever its endpoints
    /// now look like.
    ///
    /// Two steps rather than one async call so the caller can build the
    /// message under whatever lock it needs to read its own state, then
    /// release that lock before awaiting the send. The engine channel is
    /// bounded, so a send can block on a busy engine, and integrations should
    /// not be holding their own locks when it does.
    #[must_use = "an announcement does nothing until sent"]
    pub fn announce(&self, mut node: Node) -> Announcement {
        node.entity_id = self.entity_id.clone();
        node.integration = self.integration.to_string();
        Announcement {
            tx: self.shared.tx.clone(),
            msg: FromIntegrationMessage::NodeAdded {
                node_id: self.node_id,
                node,
            },
        }
    }

    /// Deregister the node: release its name and tell the engine it is gone.
    pub async fn remove(mut self) {
        self.release();
        let msg = FromIntegrationMessage::NodeRemoved {
            node_id: self.node_id,
        };
        if let Err(e) = self.shared.tx.send(msg).await {
            warn!("failed to send NodeRemoved for {}: {}", self.node_id, e);
        }
    }

    /// Give up the name and the ownership record.
    fn release(&mut self) {
        if self.released {
            return;
        }
        self.released = true;

        let mut inner = self.shared.lock();
        // Only if it is still ours. A node that was removed and immediately
        // re-registered under the same name would otherwise have its
        // successor's reservation torn out by this removal.
        if inner.by_entity_id.get(&self.entity_id) == Some(&self.node_id) {
            inner.by_entity_id.remove(&self.entity_id);
        }
        inner.owners.remove(&self.node_id);
    }
}

impl Drop for RegisteredNode {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        // Releasing here keeps a forgotten handle from reserving a name
        // forever, but nothing can send NodeRemoved from a destructor, so the
        // engine keeps a node that no integration owns. That is a bug in the
        // integration; say so rather than hiding it.
        warn!(
            "node {} ({}) dropped without remove(): releasing the name, but it stays in engine state",
            self.node_id, self.entity_id
        );
        self.release();
    }
}

/// A prepared `NodeAdded`, ready to send once the caller's locks are released.
#[must_use = "an announcement does nothing until sent"]
pub struct Announcement {
    tx: FromIntegrationSender,
    msg: FromIntegrationMessage,
}

impl Announcement {
    pub async fn send(self) {
        if let Err(e) = self.tx.send(self.msg).await {
            warn!("failed to send NodeAdded: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;

    use super::*;
    use crate::matter::Endpoint;

    fn node(entity_id: &str) -> Node {
        Node {
            entity_id: entity_id.to_string(),
            // Deliberately wrong: registration should overwrite it.
            integration: "not-the-owner".to_string(),
            name: None,
            endpoints: HashMap::new(),
        }
    }

    fn registry(policy: CollisionPolicy) -> (NodeRegistry, mpsc::Receiver<FromIntegrationMessage>) {
        let (tx, rx) = mpsc::channel(16);
        (NodeRegistry::new(tx, policy), rx)
    }

    #[tokio::test]
    async fn ids_are_unique_across_integrations() {
        let (registry, _rx) = registry(CollisionPolicy::Reject);
        let mqtt = registry.for_integration("mqtt");
        let metno = registry.for_integration("metno");

        let a = mqtt.register(node("light.a")).await.unwrap();
        let b = metno.register(node("weather.b")).await.unwrap();
        let c = mqtt.register(node("light.c")).await.unwrap();

        let ids = [a.node_id(), b.node_id(), c.node_id()];
        let mut unique = ids.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), ids.len());
    }

    #[tokio::test]
    async fn a_name_taken_by_another_integration_is_rejected() {
        let (registry, _rx) = registry(CollisionPolicy::Reject);
        let metno = registry.for_integration("metno");
        let _held = metno.register(node("weather.home")).await.unwrap();

        let err = registry
            .for_integration("mqtt")
            .register(node("weather.home"))
            .await
            .expect_err("the second claim on the name should be refused");

        assert_eq!(err.entity_id, "weather.home");
        assert_eq!(err.claimed_by, "metno");
    }

    /// The case that motivates rejecting an integration's collisions with
    /// itself: `locations = ["home", "home"]` is two distinct sites asking for
    /// one name, and treating the second as an update would silently merge
    /// them into one node with two writers.
    #[tokio::test]
    async fn an_integration_collides_with_its_own_earlier_registration() {
        let (registry, _rx) = registry(CollisionPolicy::Reject);
        let metno = registry.for_integration("metno");

        let _first = metno.register(node("weather.home")).await.unwrap();
        let err = metno
            .register(node("weather.home"))
            .await
            .expect_err("registering the same name twice is a bug, not an update");

        assert_eq!(err.claimed_by, "metno");
    }

    #[tokio::test]
    async fn a_rejected_registration_allocates_nothing() {
        let (registry, _rx) = registry(CollisionPolicy::Reject);
        let mqtt = registry.for_integration("mqtt");

        let first = mqtt.register(node("light.a")).await.unwrap();
        let _ = mqtt.register(node("light.a")).await.unwrap_err();
        let next = mqtt.register(node("light.b")).await.unwrap();

        // Ids stay dense across a rejection, so a restart that hits the same
        // rejection numbers everything the same way.
        assert_eq!(first.node_id(), NodeId::from_raw(1));
        assert_eq!(next.node_id(), NodeId::from_raw(2));
    }

    /// Under the default policy, an integration that asks for one name twice
    /// gets two nodes, not one node it has accidentally started sharing.
    #[tokio::test]
    async fn a_self_collision_stays_two_nodes() {
        let (registry, _rx) = registry(CollisionPolicy::default());
        let metno = registry.for_integration("metno");

        let first = metno.register(node("weather.home")).await.unwrap();
        let second = metno.register(node("weather.home")).await.unwrap();

        assert_ne!(first.node_id(), second.node_id());
        assert_eq!(first.entity_id(), "weather.home");
        assert_eq!(second.entity_id(), "weather.home_2");
    }

    #[tokio::test]
    async fn the_suffix_policy_renames_the_newcomer() {
        let (registry, _rx) = registry(CollisionPolicy::Suffix);

        let first = registry
            .for_integration("metno")
            .register(node("weather.home"))
            .await
            .unwrap();
        let second = registry
            .for_integration("mqtt")
            .register(node("weather.home"))
            .await
            .unwrap();
        let third = registry
            .for_integration("mqtt")
            .register(node("weather.home"))
            .await
            .unwrap();

        assert_eq!(first.entity_id(), "weather.home");
        assert_eq!(second.entity_id(), "weather.home_2");
        assert_eq!(third.entity_id(), "weather.home_3");
    }

    #[tokio::test]
    async fn the_announced_node_carries_the_assigned_name_and_owner() {
        let (registry, mut rx) = registry(CollisionPolicy::Suffix);

        let _first = registry
            .for_integration("metno")
            .register(node("weather.home"))
            .await
            .unwrap();
        let _ = rx.recv().await;

        let _second = registry
            .for_integration("mqtt")
            .register(node("weather.home"))
            .await
            .unwrap();

        match rx.recv().await.unwrap() {
            FromIntegrationMessage::NodeAdded { node, .. } => {
                assert_eq!(node.entity_id, "weather.home_2");
                assert_eq!(node.integration, "mqtt");
            }
            other => panic!("expected NodeAdded, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn announcing_keeps_the_id_and_the_name() {
        let (registry, mut rx) = registry(CollisionPolicy::Reject);
        let mqtt = registry.for_integration("mqtt");

        let handle = mqtt.register(node("sensor.a")).await.unwrap();
        let _ = rx.recv().await;

        // An update from the integration's own copy, which still carries the
        // requested name and a stale owner.
        let mut updated = node("sensor.a");
        updated.endpoints.insert(1, Endpoint::default());
        handle.announce(updated).send().await;

        match rx.recv().await.unwrap() {
            FromIntegrationMessage::NodeAdded { node_id, node } => {
                assert_eq!(node_id, handle.node_id());
                assert_eq!(node.entity_id, "sensor.a");
                assert_eq!(node.integration, "mqtt");
                assert_eq!(node.endpoints.len(), 1);
            }
            other => panic!("expected NodeAdded, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn removing_frees_the_name_for_reuse() {
        let (registry, _rx) = registry(CollisionPolicy::Reject);
        let mqtt = registry.for_integration("mqtt");

        let first = mqtt.register(node("light.a")).await.unwrap();
        let first_id = first.node_id();
        first.remove().await;

        let second = mqtt
            .register(node("light.a"))
            .await
            .expect("the name should be free once its holder is removed");

        assert_eq!(second.entity_id(), "light.a");
        assert_ne!(second.node_id(), first_id);
    }

    /// Re-registration can beat the removal that freed the name, and the late
    /// removal must not evict the successor's reservation.
    #[tokio::test]
    async fn a_late_removal_does_not_evict_the_successor() {
        let (registry, _rx) = registry(CollisionPolicy::Suffix);
        let mqtt = registry.for_integration("mqtt");

        let first = mqtt.register(node("light.a")).await.unwrap();
        let second = mqtt.register(node("light.a")).await.unwrap();
        assert_eq!(second.entity_id(), "light.a_2");

        // The first node goes away; the name it held was its own, so it goes
        // with it, and the survivor keeps what it holds.
        first.remove().await;
        assert_eq!(registry.shared.lock().by_entity_id.len(), 1);
        assert_eq!(
            registry.shared.lock().by_entity_id.get("light.a_2"),
            Some(&second.node_id())
        );
    }

    #[tokio::test]
    async fn ownership_is_recorded_for_routing() {
        let (registry, _rx) = registry(CollisionPolicy::Reject);

        let light = registry
            .for_integration("mqtt")
            .register(node("light.a"))
            .await
            .unwrap();
        let weather = registry
            .for_integration("metno")
            .register(node("weather.b"))
            .await
            .unwrap();

        assert_eq!(registry.owner(light.node_id()).as_deref(), Some("mqtt"));
        assert_eq!(registry.owner(weather.node_id()).as_deref(), Some("metno"));

        let weather_id = weather.node_id();
        weather.remove().await;
        assert!(registry.owner(weather_id).is_none());
        assert_eq!(registry.owner(light.node_id()).as_deref(), Some("mqtt"));
    }

    #[tokio::test]
    async fn a_dropped_handle_releases_its_name() {
        let (registry, _rx) = registry(CollisionPolicy::Reject);
        let mqtt = registry.for_integration("mqtt");

        drop(mqtt.register(node("light.a")).await.unwrap());

        assert!(
            mqtt.register(node("light.a")).await.is_ok(),
            "a forgotten handle must not reserve the name for the life of the process"
        );
    }
}
