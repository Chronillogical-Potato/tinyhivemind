# lab/src

The lab library and its binaries. The library is the host side of core's
ports: everything core refuses to own (a log, an executor, a clock, a router,
a sink) written once so every example can share it.

| Path | What it is |
| --- | --- |
| `lib.rs` | `JsonlSink` and `WallClock` for telemetry, and the re-exports of the modules below |
| `cli.rs` | `TraceRig`: parses `--trace out.jsonl` and hands out a `Tracer` over a `JsonlSink` |
| `tick.rs` | `TickClock`: one millisecond per reading, so traces repeat exactly |
| `exec.rs` | `block_on`: a poll loop for the ports that return futures; every future here resolves at once |
| `log.rs` | `MemoryLog`, an in-memory `SessionLog`; `row`, `agent`, `person` builders |
| `world.rs` | `World`: owns the roster and desk records that core's borrowed `Roster` and `DeskSet` view |
| `router.rs` | `KeywordRouter`: a model-free `Router` with knobs for unsure, stale and failing answers |
| `report.rs` | `Res` and `section`, the output helpers every example shares |
| `bin/` | the runnable examples, see [`bin/README.md`](bin/README.md) |
| `swe/` | the SWE hive and its single-agent baseline, see [`swe/README.md`](swe/README.md) |

Nothing here is part of core. Core stays pure; this crate may use `serde_json`
and the standard library only.
