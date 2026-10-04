//! A clock that advances one millisecond per reading, so traces are repeatable.

use std::sync::atomic::{AtomicU64, Ordering};

use tinyhivemind_core::telemetry::Clock;

/// Reads `0, 1, 2, ...`: every event gets the next millisecond.
///
/// A deterministic example must not stamp wall-clock time, or two runs of the
/// same script would differ. The viewer only needs ordering, which a counter
/// preserves.
#[derive(Debug, Default)]
pub struct TickClock(AtomicU64);

impl Clock for TickClock {
    fn now_ms(&self) -> u64 {
        self.0.fetch_add(1, Ordering::Relaxed)
    }
}
