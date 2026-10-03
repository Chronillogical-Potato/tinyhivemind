# Dynamic coordinator

| File | Responsibility |
| --- | --- |
| `mod.rs` | Shared handle, dynamic APIs, authorization snapshots, and transactions |
| `types.rs` | Host runner port and public payloads |
| `messaging.rs` | Atomic acceptance, retry IDs, attribution, and private reads |
| `conduct.rs` | Actual `CompletionDriver`/`Conductor` checkpoint lifecycle |
| `scheduler.rs` | FIFO reservations, concurrent futures, shutdown, cancellation |
| `test/` | Deterministic contract fixtures and behavior tests |

All clones of `Coordinator` share one scheduler and state. Transactions clone
state and replace it only after the storage CAS succeeds. Runner futures execute
outside the state lock. Conductor folding uses copied snapshots and retries if a
concurrent synchronous API changed the revision; no external tool is repeated.

Work is ordered by accepted sequence, then agent ID within a round. Shared
agents retain their queued positions and never run two turns concurrently.
Each hive has one active episode; the real conductor supplies child conversations,
nudges, broadcasts, completion admission, approval parking, and turn walls.
Normal runner return means its turn ended; only `EpisodeAction::Complete` closes
an assignment. A reply is recorded as a post.

`register_agent` accepts handles from the current runtime. Existing durable IDs
are reattached after restart without overwriting session bindings. Optional
`register_agent_in_session` atomically commits the host's conversation before
publishing its runner, including when recovered work is already pending and a
scheduler is running. A failed commit changes neither the session nor the live
runner. `bind_session` remains available for separately registered agents before
their first claim.
Joining/leaving affects later turns; active turns retain captured membership.
Removing membership cancels unstarted deliveries while preserving transcript.

Private child posts and completions inherit the ask root's participant list.
Reads also verify every ancestor thread's visibility. Messages addressed to an
existing thread keep that root in the execution context and all ordinary outputs;
private threads admit only their original participants, including after restart.

`run_until_idle` drains eligible work and returns with unattached/parked work
retained. `run` waits on notifications. Shutdown stops claims and waits for
active runner futures to return. Dropping a drain interrupts its durable running
reservations. Recovery records crashed running turns without replaying uncertain
external effects; pending turns which never started remain eligible.

A failed host finalizer can follow a successfully committed agent turn. A
`Failed` outcome with a nonempty matching session binds that conversation in
the same transaction as interruption. Its input is not acknowledged, and its
reply and staged episode actions are discarded. Later inputs continue that
conversation, including after SQLite reopen. Empty or changed session IDs fail
validation without replacing a prior binding.
