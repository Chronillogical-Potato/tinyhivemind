# memory_hive

What a room remembers. A scripted `Digester`, an in-memory log and hand-built
snapshots; every figure printed is a fold over them.

```sh
cargo run --bin memory_hive [-- --trace out.jsonl]
```

| File | Prints |
| --- | --- |
| `main.rs` | runs the sections in order and emits a `Checkpoint` between them; owns the `eng` conversation and the five-agent `world()` |
| `digest.rs` | `plan_digest` over policies, `refold` to a fixed point, `apply_digest`, every rejection, the typed errors |
| `pins.rs` | `!pin` and `!unpin`, fences, aside visibility per viewer, limits, `pin_note` |
| `sharing.rs` | `prepare_delta` per viewer, `note_present`, each `ReinitializeReason`, the bounded present set |
| `briefing.rs` | `TeamBriefing`, `BrevityPolicy`, dispatch and aside rules, `initialize_session*` |
| `elsewhere.rs` | `gather_elsewhere` across desks, and the thread index |
| `contract.rs` | a log that breaks the `SessionLog` contract eight ways |

Findings: F1-F12 in the [ledger](../../../../../docs/experiments/2026-10-04-findings.md).
