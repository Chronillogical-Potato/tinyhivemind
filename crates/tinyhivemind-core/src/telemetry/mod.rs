//! Profiling checkpoints: a run's events, stamped and sent to a host sink.
//!
//! The folds in this crate return values — a [`HiveStep`], a conductor
//! [`Event`] — and a host that wants a timeline of a run has had to rebuild one
//! from those. [`Tracer`] does it once: it turns each value into a
//! [`TraceEvent`], stamps it with a run id, a sequence number and the host's
//! [`Clock`], and hands it to a [`TraceSink`].
//!
//! # Pure, and optional
//!
//! Nothing here reads a clock, opens a file or spawns a thread. The host owns
//! the clock and the sink; [`NullSink`] is the default for a host that wants
//! neither, and no fold takes a tracer, so existing callers are unchanged.
//!
//! # Example
//!
//! ```
//! use tinyhivemind_core::telemetry::{ManualClock, MemorySink, TraceEvent, Tracer};
//!
//! let sink = MemorySink::default();
//! let clock = ManualClock::default();
//! let tracer = Tracer::new("run-1", &sink, &clock);
//!
//! clock.set(40);
//! tracer.emit(TraceEvent::TurnStarted { turn: 0, seat: "alice".into() });
//! clock.set(1_240);
//! tracer.emit(TraceEvent::TurnFinished {
//!     turn: 0,
//!     seat: "alice".into(),
//!     input_tokens: 900,
//!     output_tokens: 120,
//!     latency_ms: 1_200,
//! });
//!
//! let events = sink.events();
//! assert_eq!(events.len(), 2);
//! assert_eq!(events[1].seq, 1);
//! assert_eq!(events[1].at_ms, 1_240);
//! ```
//!
//! [`HiveStep`]: crate::hive::HiveStep
//! [`Event`]: crate::driver::Event

#[cfg(test)]
mod test;

mod sink;
mod tracer;
mod types;

pub use sink::{ManualClock, MemorySink, NullSink};
pub use tracer::Tracer;
pub use types::{Clock, RoundSeat, Stamped, TraceEvent, TraceSink};
