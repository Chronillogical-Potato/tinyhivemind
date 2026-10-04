# 31. Expose host seams over async, incremental storage

- **Status:** Accepted
- **Date:** 2026-10-04
- **Specification:** [`../specs/dynamic-hives.md`](../specs/dynamic-hives.md)
- **Host guide:** [`../opencompany-migration.md`](../opencompany-migration.md)

## Context

OpenCompany drives the `tinyhivemind-hives` `Coordinator` through
`tinyhivemind-openhuman` and found eight seams missing:

- Hooks received only an agent id, so live progress, approvals and file
  cards could not be attributed to a hive, an episode or a thread.
- Agent reads reject `HOST_ID`, so the replies to `send_as_host` (stored to
  `Agent(HOST_ID)`) were unreadable. Nothing signalled a commit, an episode
  settling, or an interruption.
- A hive message started every reader. The only narrowing was `only_for`,
  which also hides the message.
- `release` carried no decision back to the agent it released.
- An agent's handle could not be replaced after the host rebuilt it.
- The 300-second turn wall was fixed, and no hook could set a turn's
  working directory.
- No host policy sat between the model and the four outbound tools.
- `Storage` was synchronous and was called under the coordinator's
  `std::sync::Mutex` with the whole `StoredState`, transcript included. An async
  `MongoDB` client cannot implement it, and one document is capped at 16 MB.

## Decision

Storage is an async, object-safe port (`StorageFuture`, boxed and `Send`, no
executor named). A commit carries the next bounded *state row* and only the
transcript rows appended since the expected revision. `StoredState` skips
`messages` and `accepted` in serde, so the row a store rewrites on each commit
does not grow with the conversation. `load` reassembles the transcript with
`StoredState::append`. SQLite moves to schema 2: a state row plus an
append-only `hivemind_messages` table, written in one transaction. A
version-one database is migrated on open. `RetentionPolicy` (default: keep
all) bounds settled episodes, acknowledged deliveries, and interruption records
(interrupted deliveries and `InterruptedTurn`s). It never prunes the
transcript, an episode that a running turn still reports to, or a pending
delivery: pending deliveries are live work, so `pending_per_agent` instead
refuses a send that would overfill one agent's inbox with `Error::InboxFull`.

The coordinator copies live state, mutates the copy, and persists it while
holding an async writer gate rather than the live lock, so reads never wait on
storage I/O. The gate makes in-process writers serial.

A store has exactly one writer. `StoredState.writer_epoch` names it:
`Coordinator::new` loads the store and commits `writer_epoch + 1`, and that
commit is its claim (recovering the previous owner's running turns as
interruptions happens in the same commit). Every later commit carries the
coordinator's epoch. A store holding a higher epoch rejects it with
`Error::Fenced`; the fenced coordinator stops scheduling and every write it
attempts fails the same way, with no reload-and-retry. Two live coordinators
over one store were the source of every cross-process race the review found
(remote reservations, interrupting another process's turn, commits a
subscriber never sees); fencing removes the case instead of patching each one.
A host runs one coordinator per store, and a rolling deploy's new process
fences the old one. `Drop` cannot await, so a cancelled
drain applies its interruptions to live state at once and the next commit
persists them. A crash before that commit is still recovered as an interrupted
running turn. As a consequence, every mutating coordinator API is now `async`.

The host seams are additive. Each keeps the previous behaviour as its default:

- `TurnHooks` receive a `TurnScope` (agent, episode, message ids, senders,
  destination, thread), and gain `prepare(&TurnScope) -> TurnOptions { cwd }`,
  which is applied through `Turn::cwd`.
- `read_transcript(after)` is host-scoped and unfiltered.
  `subscribe() -> watch::Receiver<u64>` publishes the committed *revision*, and
  `episodes()` reports each `EpisodePhase`. A revision is the one counter that
  every observable change advances: messages, settlements, interruptions,
  claims. The latest message sequence would miss a settlement that appended
  nothing.
- `SendMessage.starters` picks who starts an episode without hiding the
  message. Empty starts everyone, and the field is omitted from the wire when
  empty.
- `release_with(agent, note)` stores a note that is delivered once, as
  `TurnRequest::resumption`. The OpenHuman runner renders it ahead of the
  attributed context.
- `replace_agent(agent_id, build)` waits for a running turn, drops the
  adapter's handle, then builds the replacement. OpenHuman keeps agent ids
  unique while any clone is alive, so a host cannot build the replacement
  first and pass it in.
- `with_turn_timeout` makes the turn wall configurable and nonzero.
  `with_send_policy(SendAuthorizer)` gates the four outbound tools, and a
  refusal comes back as tool error text.

## Consequences

- Hosts await registration, sends, membership changes, actions and releases;
  `Coordinator::new` is async. Reads stay synchronous.
- A host storage implementation persists two shapes: one bounded row, and an
  append-only transcript ordered by sequence. Out-of-order appends fail with
  `Error::TranscriptOutOfOrder`.
- A stale coordinator no longer returns `RevisionConflict` on its first write;
  it reloads. Persistent conflicts still surface after the retries.
- The coordinator still holds the full transcript in memory and clones live
  state per transaction. Incrementality bounds what is written, not what is
  held.
- Fixing the stall path while adding `episodes()` exposed a pre-existing
  defect: a conductor stall was overwritten by the checkpoint, so the episode
  was re-prepared forever. A stalled episode now settles as failed.
