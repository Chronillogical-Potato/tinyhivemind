# 30. A host memory port feeds seat sessions

- **Status:** Accepted
- **Date:** 2026-10-04
- **Specification:** [`../specs/hive-memory.md`](../specs/hive-memory.md)
- **Supersedes:** the "no new port" and "no index" non-goals of
  [`../specs/recall.md`](../specs/recall.md), for host-owned memory only

## Context

`recall.md` made the transcript queryable without a new port, an index, or
embeddings: search and pins are bounded folds over `SessionLog`. That kept
core free of a second store, and it holds for the transcript.

Terminal-Bench runs showed it is not enough for a seat. The hive lost to a
single agent because each seat's conversation was discarded between
activations; what a seat learned — commands, exit codes, dead ends — survived
only if it was posted and still in the window. Recovering it needs retrieval
over what seats concluded, ranked by relevance rather than recency, which is
an index. Charter rule 1 forbids core from owning one. It does not forbid the
host from owning one behind a port, which is how the session log already
works.

## Decision

A seat keeps one session for its whole run; only compaction erases from it,
and a rejoining seat receives the desk rows after its watermark
(`desk_delta`).

Core adds two object-safe ports in `runtime::recall`: `Recall`, called at
session start, on rejoin, and after compaction, and `Remember`, called after
each activation. The host owns the store, its index, its embeddings, and a
per-run namespace. Core adds only pure helpers: `frame_recalled` (data
framing within a character budget), `desk_delta`, and
`initialize_session_with_recall`, which degrades a failed recall to no memory.

`recall.md`'s rules carry over unchanged: a summary stands in only for a
contiguous range, and recalled context is framed as data, not instructions.

## Consequences

- Core gains two ports and two error variants (`Error::Recall`,
  `Error::Remember`) and still opens no storage; `assert-pure.sh` is
  unchanged.
- The OpenHuman adapter and the lab implement the ports over CortexDB through
  `tinymemory`; a host without a store passes an implementation that returns
  no notes.
- A store outage costs a seat its memory, never its turn.
- `recall.md`'s non-goals still bind the transcript: search and pins remain
  folds over `SessionLog`, with no index.
- This port sits beside [ADR 0029](0029-working-memory-is-a-host-adapter.md)'s
  `WorkingMemory`: 0029 is a tool the seat pulls from, this is context the
  session is pushed. A host whose OpenHuman agents already share one memory
  root through OpenHuman's own memory lifecycle (OpenCompany binds every
  teammate to `team:<company>`) gets that sharing from OpenHuman and needs
  neither port.
