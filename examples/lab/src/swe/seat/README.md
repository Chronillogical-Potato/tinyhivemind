# seat

One activation of one seat: model calls, `bash` and speaking tools, and telemetry,
running on the seat's `SeatSession` (see `../session/`). A new session opens with
the system prompt and the briefing; a resumed one gets only the desk delta
appended. Compaction is the only thing that removes messages. With memory on, a
pack is recalled at session start (in front of the briefing), on rejoin (after
the delta) and after a summary (kept right after the opening), and what the seat
did (its words and a ledger of commands, exit codes and failed attempts) is
stored at the end of every activation and before every compaction recall.

| File | Purpose |
| --- | --- |
| `mod.rs` | `Env`, `Activation`, `Outcome`, `run` and the model loop; `bash` records the ledger |
| `compact.rs` | `none` / `mask` / `summarize` / `mask+summarize` over the session, the `context` mark, the compaction recall |
| `recall.rs` | opening or resuming the session, the two memory calls and their `memory` marks, ledger lines |
| `test/` | scripted-model tests, see [`test/README.md`](test/README.md) |

Telemetry per activation: a `session` mark (`<seat>: activation N messages M
delta_rows R mode persistent|fresh`), turns and tool calls, an `exec` mark per
command, a `context` mark per compaction and a `memory` mark per memory call
(`<seat>: recall|remember <moment> chars= items= latency_ms= [error=]`).
