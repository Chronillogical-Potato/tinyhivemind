# Working memory: carrying what a seat learned across activations

**Status:** Draft; the port and tools are implemented, host adapters are not
**Owner:** tinyhivemind maintainers
**Decision:** [ADR 0029](../adr/0029-working-memory-is-a-host-adapter.md)
**Related:** [`seat-continuity.md`](seat-continuity.md), [`long-horizon-tasks.md`](long-horizon-tasks.md); issues #98, #99, #104

## Problem

A seat re-reads, re-runs and re-derives on every activation. Seats under the
OpenHuman adapter keep a session transcript, but memory is not wired for hive
turns: auto-recall's gate (short "my/I" questions under 600 characters) never
opens for the JSON a seat is handed; the DeepSWE example opens a fresh session
per attempt; and the `pe1006` example's session id carries a PID and its runtime
workspace is fresh per run, so the `MEMORY.md` the agent injects is never the
shared one.

## Goals

1. A seat can record a finding, a failed attempt or a decision, and any seat can
   recall it on a later activation.
2. The host chooses the engine. Nothing in core or tools names a format.
3. What reaches a turn is bounded, whatever the engine returns.

## Non-goals

- A second journal, or an engine in this repository.
- Per-seat private memory inside the host agent (OpenHuman's `MEMORY.md`,
  tinycortex). That stays the host's. `Seat` scope only states intent.

## Proposed behavior

- `WorkingMemory::{recall, record, forget}` in
  `tinyhivemind_core::runtime::memory`; notes <= 1000 characters; recalls <= 12
  entries; engine order trusted.
- `hive_memory_note(entries, budget)` folds entries into one `BriefingNote` for the
  turn's briefing, so a seat need not spend a call to read it.
- `tinyhivemind_tools::MemoryTools` serves `hive_memory_recall`, `hive_memory_note` and
  `hive_memory_forget`; the seat id comes from the host.

## Invariants

- Entries never exceed the budget the briefing was given.
- An engine failure degrades recall only and never reaches the seat verbatim.
- A seat cannot write as, or read the private entries of, another seat.

## Acceptance

Unit tests in `runtime/memory/test.rs` and `tools/src/memory/test.rs`; a host
adapter proves the port by recording from one seat and recalling from another.

## Open questions

- Who injects `hive_memory_note` into the briefing: the hives coordinator
  (`conduct.rs`) or the host?
- Whether the repeat-command guard (#99) and observation ledger (#98) are
  producers into this port or separate folds feeding it.
