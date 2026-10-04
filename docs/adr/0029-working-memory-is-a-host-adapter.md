# 29. Working memory is a host adapter behind a narrow port

- **Status:** Accepted
- **Date:** 2026-10-04

## Context

Seats lose their context between activations (issues #98, #99, #104). An
audit of the OpenHuman seats found a real memory stack that hive turns barely
reach: auto-recall never opens for long JSON turns, session ids and runtime
workspaces are not stable across runs, and the shared `MEMORY.md` is not the
one the agent injects. Fixing that by choosing one memory engine would bind the
hive to it, but hosts differ: a markdown file, a vector store (tinycortex), a
hosted memory service.

## Decision

The hive does not own a memory engine. `tinyhivemind-core` defines the
`WorkingMemory` port (`recall`, `record`, `forget`) with bounded inputs and a
pure fold that renders recalled entries as one `BriefingNote`. The host
implements the port over whatever it likes. `tinyhivemind-tools` serves the
seat-facing tools (`hive_memory_recall`, `hive_memory_note`, `hive_memory_forget`) over the
host's implementation and takes the seat id from the host, not the call.
Examples keep only the tool wiring and one minimal markdown reference adapter.

## Consequences

- No storage format, index or ranking model appears in core or tools; the
  purity script is unaffected.
- A missing or failing engine costs recall only. Tool errors never carry the
  engine's own message.
- Entry scope (`hive` or `seat`) is stated by the hive and honored by the engine
  as it can.
- Repeat-command guards and failed-attempt observations (#99, #98) can be
  producers of entries into this port rather than a second store.
