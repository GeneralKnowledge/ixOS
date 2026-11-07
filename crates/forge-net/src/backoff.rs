//! How long to wait between retries.
//!
//! A [`Backoff`] starts at an initial delay and doubles after each failure up
//! to a cap. [`reset`](Backoff::reset) after a success starts it over. With
//! [`with_jitter`](Backoff::with_jitter), each delay is shortened by a random
//! fraction, so many clients that failed together (a server restart, a phone
//! leaving a tunnel) do not all retry at the same instant. Jitter only ever
//! shortens a delay, so the cap is still the longest wait.
//!
//! ```
//! use std::time::Duration;
//! use forge_net::{Backoff, CancelHandle};
//!
//! let cancel = CancelHandle::new();
//! let mut backoff = Backoff::new(Duration::from_millis(1), Duration::from_millis(4));
//! for attempt in 0..3 {
//!     let succeeded = attempt == 2; // try the request here
//!     if succeeded {
//!         backoff.reset();
//!         break;
//!     }
//!     if !backoff.wait(&cancel) {
//!         break; // cancelled while waiting
//!     }
//! }
//! ```

use std::time::Duration;

use crate::CancelHandle;

/// An exponential retry delay with an optional jitter. See the
/// [module documentation](self).
#[derive(Debug, Clone)]
pub struct Backoff {
    initial: Duration,
    max: Duration,
    jitter: f64,
    current: Duration,
    failures: u32,
    seed: u64,
}

impl Backoff {
    /// Starts at `initial` and doubles up to `max`. A `max` below `initial`
    /// is raised to `initial`.
    pub fn new(initial: Duration, max: Duration) -> Self {
        Self {
            initial,
            max: max.max(initial),
            jitter: 0.0,
            current: initial,
            failures: 0,
            seed: seed(),
        }
    }

    /// Shortens each delay by a random part of up to `fraction` of it
    /// (clamped to 0 to 1). `0.2` gives delays between 80% and 100% of the
    /// exponential value.
    pub fn with_jitter(mut self, fraction: f64) -> Self {
        self.jitter = if fraction.is_finite() {
            fraction.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self
    }

    /// The delay to wait now, after a failure. Each call doubles the next
    /// one, up to the cap.
    pub fn next_delay(&mut self) -> Duration {
        let base = self.current;
        self.current = self.current.saturating_mul(2).min(self.max);
        self.failures = self.failures.saturating_add(1);
        if self.jitter == 0.0 {
            return base;
        }
        let cut =
            Duration::try_from_secs_f64(base.as_secs_f64() * self.jitter * self.random_unit())
                .unwrap_or_default();
        base.saturating_sub(cut)
    }

    /// Waits for [`next_delay`](Self::next_delay), waking early if `cancel`
    /// is cancelled. Returns `false` if it was, so the caller stops retrying.
    pub fn wait(&mut self, cancel: &CancelHandle) -> bool {
        let delay = self.next_delay();
        !cancel.wait_timeout(delay)
    }

    /// Starts over at the initial delay. Call it after a success.
    pub fn reset(&mut self) {
        self.current = self.initial;
        self.failures = 0;
    }

    /// Failures since the last [`reset`](Self::reset): how many delays have
    /// been handed out.
    pub fn failures(&self) -> u32 {
        self.failures
    }

    /// The delay the next failure will get, before jitter.
    pub fn peek(&self) -> Duration {
        self.current
    }

    /// A number in [0, 1), from a small xorshift generator. Retry jitter
    /// needs spread, not cryptographic randomness.
    fn random_unit(&mut self) -> f64 {
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.seed = x;
        (x >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// A seed that differs between processes and between backoffs in one
/// process, so clients that start together do not jitter together.
fn seed() -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default(),
    );
    hasher.finish() | 1
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn doubles_to_the_cap_and_resets() {
        let mut b = Backoff::new(500 * MS, 5000 * MS);
        let delays: Vec<u64> = (0..7).map(|_| b.next_delay().as_millis() as u64).collect();
        assert_eq!(delays, [500, 1000, 2000, 4000, 5000, 5000, 5000]);
        assert_eq!(b.failures(), 7);
        assert_eq!(b.peek(), 5000 * MS);
        b.reset();
        assert_eq!(b.failures(), 0);
        assert_eq!(b.next_delay(), 500 * MS);
    }

    #[test]
    fn a_cap_below_the_start_is_raised() {
        let mut b = Backoff::new(10 * MS, MS);
        assert_eq!(b.next_delay(), 10 * MS);
        assert_eq!(b.next_delay(), 10 * MS);
    }

    #[test]
    fn huge_delays_do_not_overflow() {
        let mut b = Backoff::new(Duration::MAX / 2, Duration::MAX);
        for _ in 0..4 {
            b.next_delay();
        }
        assert_eq!(b.peek(), Duration::MAX);
    }

    #[test]
    fn jitter_only_shortens_and_spreads() {
        let mut b = Backoff::new(1000 * MS, 1000 * MS).with_jitter(0.2);
        let delays: Vec<Duration> = (0..200).map(|_| b.next_delay()).collect();
        assert!(delays.iter().all(|d| *d >= 800 * MS && *d <= 1000 * MS));
        let distinct: std::collections::BTreeSet<_> = delays.iter().collect();
        assert!(distinct.len() > 50, "{}", distinct.len());

        for odd in [-1.0, f64::NAN, 7.0] {
            let mut b = Backoff::new(10 * MS, 10 * MS).with_jitter(odd);
            assert!(b.next_delay() <= 10 * MS);
        }
    }

    #[test]
    fn wait_stops_when_cancelled() {
        let cancel = CancelHandle::new();
        let mut b = Backoff::new(MS, MS);
        assert!(b.wait(&cancel));
        cancel.cancel();
        let mut slow = Backoff::new(Duration::from_secs(60), Duration::from_secs(60));
        let started = std::time::Instant::now();
        assert!(!slow.wait(&cancel));
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
