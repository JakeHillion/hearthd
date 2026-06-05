//! What an `await` evaluates to.
//!
//! The machine knows how long a [`Pending`] waits for and nothing more. What
//! it cannot know is whether a `sleep_unique` has been superseded, because
//! "superseded" means another instance of the same automation started, and
//! the machine has no notion of an automation or of there being more than one
//! of it. So the answer comes from the driver instead, through this trait.
//!
//! The two builtins differ only here:
//!
//! - `sleep` is typed `Future<()>` and always completes. Nothing may cut it
//!   short — a body waiting on one runs to its end even when the automation
//!   fires again, and the new firing runs alongside it.
//! - `sleep_unique` is typed `Future<Bool>` precisely so it can fail. A newer
//!   instance makes it resolve `false`, and the body carries on into its
//!   `else` branch rather than being killed. The `Bool` is the whole point:
//!   an automation writes `if await sleep_unique(5min) { … } else { … }` and
//!   the `else` is what "a newer firing beat me to it" looks like.
//!
//! [`Timer`] is the implementation for a driver with nothing to supersede it:
//! every wait runs to its end. The runner supplies one that knows about
//! instances.

use async_trait::async_trait;

use super::value::Pending;
use super::value::Value;

/// Resolves one suspension: waits out a [`Pending`] and produces the value
/// its `await` evaluates to.
///
/// Called once per `Await` the body reaches, so an implementation may answer
/// differently each time — a `sleep_unique` that would have resolved `true`
/// a moment ago resolves `false` once a newer instance exists.
#[async_trait]
pub trait Suspension: Send + Sync {
    async fn resolve(&self, pending: Pending) -> Value;
}

/// Waits every suspension out in full.
///
/// Nothing supersedes anything, so `sleep_unique` always resolves `true`.
/// That is the right answer for a driver running one body with no notion of
/// a second instance, and the wrong one for the runner, which has exactly
/// that notion — see `automations::runner`.
pub struct Timer;

#[async_trait]
impl Suspension for Timer {
    async fn resolve(&self, pending: Pending) -> Value {
        tokio::time::sleep(pending.duration()).await;
        match pending {
            Pending::Sleep(_) => Value::Unit,
            Pending::SleepUnique(_) => Value::Bool(true),
        }
    }
}
