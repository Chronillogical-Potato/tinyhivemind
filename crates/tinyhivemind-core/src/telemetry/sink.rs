//! Ready-made sinks and clocks: a discard, an in-memory buffer, a manual clock.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use super::types::{Clock, Stamped, TraceSink};

/// A sink that drops every event.
#[derive(Clone, Copy, Debug, Default)]
pub struct NullSink;

impl TraceSink for NullSink {
    fn record(&self, _event: &Stamped) {}
}

/// A sink that keeps every event in memory, for tests and short runs.
#[derive(Debug, Default)]
pub struct MemorySink {
    events: Mutex<Vec<Stamped>>,
}

impl MemorySink {
    /// A copy of everything recorded so far, in order.
    #[must_use]
    pub fn events(&self) -> Vec<Stamped> {
        self.events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }
}

impl TraceSink for MemorySink {
    fn record(&self, event: &Stamped) {
        if let Ok(mut events) = self.events.lock() {
            events.push(event.clone());
        }
    }
}

/// A clock the caller sets, which makes a trace deterministic.
#[derive(Debug, Default)]
pub struct ManualClock {
    now: AtomicU64,
}

impl ManualClock {
    /// Move the clock to `ms`.
    pub fn set(&self, ms: u64) {
        self.now.store(ms, Ordering::Relaxed);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.now.load(Ordering::Relaxed)
    }
}
