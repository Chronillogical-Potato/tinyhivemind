//! Smallest traced run: emits a few events as JSONL on stdout.

use tinyhivemind_core::telemetry::{TraceEvent, Tracer};
use tinyhivemind_lab::{JsonlSink, WallClock};

fn main() {
    let sink = JsonlSink::new(std::io::stdout());
    let clock = WallClock::default();
    let tracer = Tracer::new("hello", &sink, &clock);
    tracer.emit(TraceEvent::TurnStarted {
        seat: "alice".into(),
    });
    tracer.emit(TraceEvent::TurnFinished {
        turn: 0,
        seat: "alice".into(),
        input_tokens: 10,
        output_tokens: 2,
        latency_ms: 1,
    });
}
