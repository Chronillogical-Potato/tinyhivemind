# `memory`

What a hive asks of whatever remembers for it, without choosing what that is.

## Why it exists

A seat loses its context between activations: observations, claims and dead
ends vanish with the turn (issues #98, #104). The remedy is a memory the whole
hive can write and recall — and that memory is an engine the *host* picks: a
markdown file, a vector store, a memory service. This module is the narrow
seam between the two, so the hive stays unopinionated about storage.

## Public surface

| Item | What it is |
| --- | --- |
| `WorkingMemory` | the port: `recall`, `record`, `forget`; object-safe, executor-neutral |
| `MemoryNote` / `MemoryQuery` / `MemoryEntry` | what crosses the port |
| `MemoryScope` | `Hive` (shared) or `Seat` (private); the engine decides how to honor it |
| `recall(..)` / `record(..)` | bounded, validated calls over a port |
| `validate_note(note)` | rejects a blank or over-long note |
| `memory_note(entries, budget)` | entries as one `BriefingNote`, cut to a character budget |
| `MEMORY_LIMIT` / `MEMORY_NOTE_CHARS` | 12 / 1000 |

## Constraints worth knowing

- Nothing here names a storage format, index or ranking model. The engine's
  order is trusted.
- Everything crossing the port is bounded, so a generous engine cannot spend a
  seat's window.
- A missing or failing memory costs recall only; callers degrade the briefing.
- The seat-facing tools (`memory_recall`, `memory_note`, `memory_forget`) live
  in `tinyhivemind-tools`; a host implements `WorkingMemory` and hands it over.

| File | What it holds |
| --- | --- |
| `mod.rs` | the port, bounded calls, validation, and the briefing fold |
| `types.rs` | note, query, entry and scope |
| `test.rs` | behavior tests over an in-memory engine |
