//! Shared plumbing for the lab's examples: a JSONL trace sink and a wall clock.
//!
//! Core owns the event types and the sink port but no I/O. This crate is the
//! host side of that boundary: it writes each event as one JSON line, the
//! format `viewer/viewer.html` reads.

use std::io::Write;
use std::sync::Mutex;
use std::time::Instant;

use tinyhivemind_core::telemetry::{Clock, Stamped, TraceSink};

/// Writes each event as one JSON line to any writer.
pub struct JsonlSink<W: Write + Send> {
    out: Mutex<W>,
}

impl<W: Write + Send> JsonlSink<W> {
    /// Wrap a writer.
    pub fn new(out: W) -> Self {
        Self {
            out: Mutex::new(out),
        }
    }
}

impl<W: Write + Send> TraceSink for JsonlSink<W> {
    fn record(&self, event: &Stamped) {
        if let (Ok(mut out), Ok(line)) = (self.out.lock(), serde_json::to_string(event)) {
            // A failed trace write must not fail the run it describes.
            let _ = writeln!(out, "{line}");
        }
    }
}

/// Milliseconds since the clock was created.
pub struct WallClock(Instant);

impl Default for WallClock {
    fn default() -> Self {
        Self(Instant::now())
    }
}

impl Clock for WallClock {
    fn now_ms(&self) -> u64 {
        u64::try_from(self.0.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

mod cli;
mod exec;
mod log;
mod tick;

pub use cli::TraceRig;
pub use exec::block_on;
pub use log::{MemoryLog, agent, person, row};
pub use tick::TickClock;
mod world;

pub use world::World;
mod report;

pub use report::{Res, section};
