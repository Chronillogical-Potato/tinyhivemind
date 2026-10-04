//! The telemetry ports core ships: a memory sink, a manual clock, a null sink.

use tinyhivemind_core::hive::EpisodePolicy;
use tinyhivemind_core::telemetry::{ManualClock, MemorySink, NullSink, Tracer};
use tinyhivemind_lab::{Res, section};

use crate::room::{Room, episode};

pub fn run() -> Res {
    section("telemetry sinks: MemorySink + ManualClock, and NullSink");
    let sink = MemorySink::default();
    let clock = ManualClock::default();
    clock.set(1_000);
    let tracer = Tracer::new("in-memory", &sink, &clock);
    let outcome = episode(&Room::default(), &EpisodePolicy::DEFAULT, &tracer);
    let events = sink.events();
    println!(
        "  one episode ({}) left {} events in the sink; the clock never moved, so every at_ms is {:?}",
        outcome
            .map_err(|e| e.to_string())
            .map_or_else(|e| e, |o| o.end),
        events.len(),
        events
            .iter()
            .map(|e| e.at_ms)
            .collect::<std::collections::BTreeSet<_>>()
    );
    if let Some(first) = events.first() {
        println!(
            "  first event on the wire: {}",
            serde_json::to_string(first)?
        );
    }
    let quiet = Tracer::new("null", &NullSink, &clock);
    let outcome = episode(&Room::default(), &EpisodePolicy::DEFAULT, &quiet);
    println!(
        "  the same episode into NullSink still runs: {}",
        outcome
            .map_err(|e| e.to_string())
            .map_or_else(|e| e, |o| o.end)
    );
    Ok(())
}
