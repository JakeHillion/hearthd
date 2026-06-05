//! Pluggable dispatch seam from the engine to a downstream automation
//! consumer.
//!
//! The engine doesn't know how automations are evaluated — it only
//! produces `Event`s. The `AutomationRunner` trait is the seam: anyone
//! who wants to react to engine events (the real automations runtime,
//! or a recording fake for tests) implements `dispatch`, and the engine
//! calls into it on each `AttributeChanged`.
//!
//! Dispatch is synchronous so the engine pays no task-spawn cost on the
//! hot path. Implementations are expected to be cheap (e.g. evaluate
//! filter bytecodes only) and spawn their own tasks for any heavy
//! work.

use std::sync::Arc;

use super::Event;
use super::State;

/// Receives every event the engine constructs, alongside a snapshot of
/// the state at dispatch time. `Send + Sync` so a single runner can be
/// shared across threads and stored in the engine.
pub trait AutomationRunner: Send + Sync {
    /// Process one event. Implementations must not block; they may
    /// spawn tokio tasks for any work that needs to await.
    fn dispatch(&self, event: &Event, state: &Arc<State>);
}
