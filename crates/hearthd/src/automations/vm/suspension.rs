//! Whether a `sleep_unique` was superseded.
//!
//! The machine knows how long a [`Pending`] waits for and nothing more. What
//! it cannot know is whether a `sleep_unique` has been superseded, because
//! "superseded" means another instance of the same automation started, and
//! the machine has no notion of an automation or of there being more than one
//! of it. So the answer comes from the driver instead, through this trait.

use std::time::Duration;

use async_trait::async_trait;

/// Serves the waits a body suspends on.
///
/// One method per suspending builtin, each returning what the checker typed
/// that builtin as: `sleep` is `Future<()>` and returns nothing, and
/// `sleep_unique` is `Future<Bool>` and returns the bool the automation's
/// `else` branch hangs off. The machine is what turns those into register
/// values, so an implementation cannot put a value of the wrong type into a
/// running body, and a builtin added later cannot be answered by accident —
/// it arrives as a method every implementation has to make a decision about.
#[async_trait]
pub trait Suspension: Send + Sync {
    /// `sleep_unique(d)`: wait `d` out, returning whether it finished.
    ///
    /// `false` means a newer instance of the same automation exists, which
    /// is the one thing here the machine cannot work out for itself. An
    /// implementation may answer differently each call — a wait that would
    /// have returned `true` a moment ago returns `false` once a newer
    /// instance starts.
    async fn sleep_unique(&self, duration: Duration) -> bool;

    /// `sleep(d)`: wait `d` out.
    ///
    /// Defaulted because nothing may cut a `sleep` short, so there is no
    /// decision left for a driver to make: a body waiting on one runs to its
    /// end even when the automation fires again, and the new firing runs
    /// beside it. Override only to observe the wait, never to shorten it.
    async fn sleep(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}
