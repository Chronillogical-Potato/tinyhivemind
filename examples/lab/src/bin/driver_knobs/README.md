# driver_knobs

Completion-driven episodes. Seats are rules over the brief and the host's log,
the router is a keyword counter, and the System One transport is scripted, so a
run needs no model.

```sh
cargo run --bin driver_knobs [-- --trace out.jsonl]
```

| File | Prints |
| --- | --- |
| `main.rs` | runs the sections in order |
| `fixture.rs` | the four-seat engineering desk, its candidates, routing policy and episode |
| `raw.rs` | `CompletionDriver` alone: width, queue depth, broadcast budget, asks, replay, snapshot and revision |
| `fold.rs` | the completion fold, `BoundHive` validation, the rendered `EpisodeBrief` |
| `routing.rs` | `RoutingPolicy` against a scripted router, the bypass rules, `route_broadcast` |
| `jev.rs` | `JevRouter` over a scripted transport: calls, questions and bytes per route |
| `conduct.rs` | whole episodes through `Conductor`, with the host loop, scripted seats and each wall |
| `replay.rs` | resumes every run from every snapshot and compares it with the original |

The trace carries `turn_started`, `turn_finished`, `tool_call` and every
`conducted` event, so the viewer's waterfall shows the concurrency each
`round_width` buys.

Findings: F22-F30 in the [ledger](../../../../../docs/experiments/2026-10-04-findings.md).
