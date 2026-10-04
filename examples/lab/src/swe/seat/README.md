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
| `recall.rs` | opening or resuming the session, core `Recall` / `Remember` calls framed with core `frame_recalled`, their typed events, ledger lines |
| `test/` | scripted-model tests, see [`test/README.md`](test/README.md) |

Telemetry per activation: core's typed `session_resumed { seat, messages,
delta_rows }` when a session resumes, turns and tool calls, an `exec` mark per
command, a `context` mark per compaction, `recalled { seat, moment, notes,
chars, latency_ms }` and `remembered { seat, entries, latency_ms }` per memory
call, and a `memory` mark (`<seat>: recall|remember <moment> error=...`) only
when a call fails, since the typed events carry no reason.

Memory goes through core's ports (`tinyhivemind_core::runtime::recall`): a
seat builds a `RecallRequest` at a `RecallMoment` and a `RememberRequest` of
`MemoryEntry` values (a command that exited 0 is an `Observation`, any other a
`FailedAttempt`; the seat's post is an `Outcome`, otherwise its last text a
`Note`), and drives the port's future with the lab's `block_on`.
