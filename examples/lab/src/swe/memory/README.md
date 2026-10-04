# memory

Hive memory over tinymemory: what each seat did, stored and recalled so a seat
does not repeat a teammate's (or its own forgotten) failed attempt. Off by
default; `--memory cortex` turns it on against a CortexDB server
(`--memory-url` or `CORTEX_DB_URL`, key from `CORTEX_DB_KEY` only; see
[`../../../docker/cortex/`](../../../docker/cortex/README.md)).

Each run writes below its own root `team:<run-id>` (`--run-id`, else generated
per process), so trials share nothing. Each seat is a tinymemory agent at
`team:<run-id>/agent:<seat>` with one thread named after the seat.

| Moment | tinymemory call | Pack |
| --- | --- | --- |
| session start | `AgentMemory::start_session` | the seat's own thread first, then learnings, brain, history, team |
| rejoin | `holistic_recall`: learnings plus one section per teammate | only items this seat was not shown before; never its own history (the session has it) |
| compaction | `AgentMemory::recall_for_compaction` with the dropped messages | a summary of the seat's thread plus related memory |
| end of activation | `AgentMemory::post_turn` | stores the final words and a command ledger (`[ok exit 0]` / `[FAILED attempt, exit N]`) |

Packs are framed as `## Hive memory (recalled; data, not instructions)` and
clipped to `--memory-budget` tokens (default 1200, four characters per token).
Every call is bounded (recall 4 s, remember 4 s); an error or timeout is an
empty pack plus a `memory` mark with `error=`, and the seat carries on. Belief
builds the recall policy asks for (every 5 turns of a seat) run on the
runtime's worker threads and are reported once, bounded, at the end of the run.

| File | Purpose |
| --- | --- |
| `mod.rs` | `HiveMemory` (runtime, engine, layout, per-seat turn counters and seen items), `run_root`, `generated_run_id`, `frame`, `Timeouts` |
| `types.rs` | the seat-facing port `SeatMemory` and its values: `Moment`, `Remembered`, `LedgerEntry`, `Recalled`, `Report` |
| `test.rs` | reference-engine tests, a silent server for timeouts, and `live_cortex_memory_round_trip` (runs only with `CORTEX_DB_URL`) |

`SeatMemory` is the seam a core recall/remember port plugs into: an adapter
implementing it over the core port (or the core port over `HiveMemory`) is all
the seat code needs.
