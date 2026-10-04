# telemetry

Profiling checkpoints for a run: stamped events and the ports a host receives
them on. Pure — no clock, file, or thread; the host supplies the [`Clock`] and
the [`TraceSink`].

| File | What it does |
| --- | --- |
| `mod.rs` | Module root, overview, and a runnable example. |
| `types.rs` | `TraceEvent`, `Stamped`, `RoundSeat`, and the `TraceSink` and `Clock` ports. |
| `tracer.rs` | `Tracer`: stamps events and derives them from a `HiveStep` or conductor `Event`. |
| `sink.rs` | `NullSink`, `MemorySink`, and `ManualClock`. |
| `test.rs` | Unit tests, including the pinned serde wire form. |

Events serialize as one flat JSON object per line (`run`, `seq`, `at_ms`,
`event`, then the variant's fields), which is the format the `examples/lab`
viewer reads. No fold takes a tracer, so adopting it changes no existing call.

Three events record a seat's memory lifecycle (see
[`runtime/recall`](../runtime/recall/README.md)): `recalled` (seat, moment,
notes, chars, latency), `remembered` (seat, entries, latency), and
`session_resumed` (seat, messages already held, desk rows delivered). The host
measures their latency; the core has no clock.
