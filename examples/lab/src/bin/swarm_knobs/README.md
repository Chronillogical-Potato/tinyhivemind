# swarm_knobs

The deliberation algebra, one knob at a time. Five scripted seats argue two
plans and a third; their lines are a pure function of what the projection lets
them read, so the table is the same on every run.

```sh
cargo run --bin swarm_knobs [-- --trace out.jsonl]
```

| File | Prints |
| --- | --- |
| `main.rs` | runs the sweep, then the mechanism tables, then the sinks |
| `room.rs` | the cast, the scripted seats and `episode`, which drives `hive::step` and traces every step |
| `sweep.rs` | rounds, turns, widest round and outcome for each `EpisodePolicy` setting, and the policies the fold refuses |
| `market.rs` | shared transcripts and the order the mechanism tables run in |
| `attention.rs` | `bids`, `floor_holder`, `floor_round`, `salience`, `Horizon` |
| `division.rs` | `directory` under each `DirectoryPolicy`, and `divide` with and without it |
| `consensus.rs` | exchange rounds, quorum by evaluation, the trace grammar |
| `sinks.rs` | `MemorySink`, `ManualClock`, `NullSink` |

Findings: F15-F21 in the [ledger](../../../../../docs/experiments/2026-10-04-findings.md).
