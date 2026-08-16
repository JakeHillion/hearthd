//! Credential lifetime and reconnect pacing.
//!
//! The bearer token and the MQTT credentials both expire, and neither
//! advertises when. The only signal is the broker refusing a connection, so
//! the policy is: re-run login and certification on any connection failure,
//! and back off before retrying so that a permanently wrong password becomes a
//! slow retry rather than a login loop.
//!
//! Upstream uses a flat 60-second cooldown. Exponential backoff is better
//! behaved: it reconnects promptly after a transient drop, which is the common
//! case, without hammering the API when the credentials are simply wrong.

use std::time::Duration;

/// Delay before the first retry.
const BASE_DELAY: Duration = Duration::from_secs(2);

/// Ceiling on the retry delay.
const MAX_DELAY: Duration = Duration::from_secs(300);

/// Exponential backoff with a ceiling.
#[derive(Debug, Clone)]
pub struct Backoff {
    base: Duration,
    max: Duration,
    attempt: u32,
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new(BASE_DELAY, MAX_DELAY)
    }
}

impl Backoff {
    pub fn new(base: Duration, max: Duration) -> Self {
        Self {
            base,
            max,
            attempt: 0,
        }
    }

    /// Delay for the next retry, doubling each call up to the ceiling.
    pub fn next_delay(&mut self) -> Duration {
        let delay = self
            .base
            .checked_mul(2u32.saturating_pow(self.attempt))
            .unwrap_or(self.max)
            .min(self.max);
        self.attempt = self.attempt.saturating_add(1);
        delay
    }

    /// Call after a connection succeeds, so the next failure retries promptly.
    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    /// Number of consecutive failures so far.
    pub fn attempts(&self) -> u32 {
        self.attempt
    }
}

/// How long a device may go without reporting before its last-known state is
/// worth flagging as possibly out of date.
///
/// Deliberately far longer than the cadence a Wave 3 advertises. It does not
/// keep to that cadence: telemetry arrives in bursts separated by gaps of over
/// an hour while the session stays healthy, so a threshold in minutes would
/// fire constantly on a working device and mean nothing. Two hours is long
/// enough that tripping it says something.
pub const STALE_AFTER: Duration = Duration::from_secs(2 * 60 * 60);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_doubles_up_to_the_ceiling() {
        let mut backoff = Backoff::new(Duration::from_secs(2), Duration::from_secs(60));
        assert_eq!(backoff.next_delay(), Duration::from_secs(2));
        assert_eq!(backoff.next_delay(), Duration::from_secs(4));
        assert_eq!(backoff.next_delay(), Duration::from_secs(8));
        assert_eq!(backoff.next_delay(), Duration::from_secs(16));
        assert_eq!(backoff.next_delay(), Duration::from_secs(32));
        assert_eq!(backoff.next_delay(), Duration::from_secs(60));
        assert_eq!(backoff.next_delay(), Duration::from_secs(60));
    }

    #[test]
    fn a_success_resets_the_delay() {
        let mut backoff = Backoff::default();
        backoff.next_delay();
        backoff.next_delay();
        assert_eq!(backoff.attempts(), 2);

        backoff.reset();
        assert_eq!(backoff.attempts(), 0);
        assert_eq!(backoff.next_delay(), BASE_DELAY);
    }

    #[test]
    fn a_long_outage_does_not_overflow_the_delay() {
        // A device offline for days must not wrap the multiplication.
        let mut backoff = Backoff::default();
        for _ in 0..1000 {
            let delay = backoff.next_delay();
            assert!(delay <= MAX_DELAY);
        }
        assert_eq!(backoff.next_delay(), MAX_DELAY);
    }
}
