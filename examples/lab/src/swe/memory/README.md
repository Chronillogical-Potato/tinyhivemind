# memory

Hive memory: the lab is the reference host for core's memory ports
(`tinyhivemind_core::runtime::recall`, see `docs/specs/hive-memory.md`).
`HiveMemory` implements core `Recall` and `Remember` over tinymemory: CortexDB
with `--memory cortex` (`--memory-url` or `CORTEX_DB_URL`, key from
`CORTEX_DB_KEY` only; see [`../../../docker/cortex/`](../../../docker/cortex/README.md)),
the in-memory reference engine in tests.

Each run writes below its own root `team:<run-id>` (`--run-id`, else generated
per process), which is also the `conversation` every request must name; a
request for any other namespace is refused. Each seat is a tinymemory agent at
`team:<run-id>/agent:<seat>` with one thread named after the seat.

| `RecallMoment` | tinymemory call | Notes |
| --- | --- | --- |
| `SessionStart` | `AgentMemory::start_session` | the seat's own thread first, then learnings, brain, history, team |
| `Rejoin` | `holistic_recall`: learnings plus one section per teammate | only items this seat was not shown before; never its own history (the session has it) |
| `Compaction { dropped }` | `AgentMemory::recall_for_compaction` | a summary of the seat's thread plus related memory |

Each pack section becomes a core `BriefingNote`; the seat frames the notes with
core `frame_recalled` within `--memory-budget` tokens (four characters each).

`Remember` stores a `RememberRequest`'s entries as one conversation turn
(`- [FAILED attempt] ...`, `- [observation] ...`, `- [outcome] ...`), laid out
as `AgentMemory::post_turn` lays out its own, but written with
`WriteOptions::visible` (CortexDB: `POST /v1/experience?wait=indexed`) so a
teammate's very next recall finds it; `post_turn` only waits for acceptance,
which let an immediate rejoin miss the turn. The indexed write gets most of
the 4 s bound (measured 1.2-2.3 s on the local server, accepted-only ~3 ms);
if the index is slower, the turn is kept with an accepted-only write in the
rest of the bound and the end of the run reports how many turns that hit.

Recall is bounded at 4 s too. Failures and timeouts are `Error::Recall` /
`Error::Remember` carrying the reason; the seat degrades them to no memory.
Belief builds (every 5 turns of a seat) run on the runtime's worker threads and
are reported once, bounded, at the end of the run. Each port call spawns its
work on `HiveMemory`'s own tokio runtime and returns a future that only awaits
the task, so any executor (the lab's `block_on`) can drive it.

| File | Purpose |
| --- | --- |
| `mod.rs` | `HiveMemory`: runtime, engine, layout, per-seat turn counters and seen items; `impl Recall`, `impl Remember`, `impl SeatMemory`; `run_root`, `generated_run_id` |
| `notes.rs` | tinymemory pack → core notes, core entries → stored turn text and item |
| `types.rs` | `LedgerEntry` (and its core `MemoryEntry`), `SeatMemory` (core's two ports plus namespace, budget and the end-of-run drain), `Timeouts` |
| `test.rs` | reference-engine tests, a recording engine (indexed writes, the slow-index fallback), a silent server for timeouts, and `live_cortex_memory_round_trip` (runs only with `CORTEX_DB_URL`) |
