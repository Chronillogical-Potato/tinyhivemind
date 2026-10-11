# test

Offline tests for `hive_view`, one module per behaviour. No network, no
clock, no temporary files.

| File | Covers |
| --- | --- |
| `mod.rs` | Module wiring |
| `support.rs` | Shared fixtures: gallery paths, knobs, an ending at a seed |
| `packages.rs` | Loading, summaries, the mermaid view, the problem bank |
| `runs.rs` | Scripted, seeded and simulated episodes; budgets; determinism; `sim.json` error weights |
| `routing.rs` | One floor per desk, links, hop limits, disabled policy, asks, the width cap |
| `record.rs` | Recording flush, run labels, and the halt that stops a run on a failed write |
| `wire.rs` | The pinned JSON of every frame |
| `compare.rs` | `--compare` stream and refusals |
| `args.rs` | Knob bounds and the `--live` flags |
| `live.rs` | The request body (no tools/plugins/web), model refusal, prompts, reply parsing |
