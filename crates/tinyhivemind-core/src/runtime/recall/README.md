# Recall: the host memory port

A seat keeps one session for its whole life, and only compaction erases from
it. This module defines what a host-owned memory store supplies to that
session, and when. The store itself — CortexDB through `tinymemory`, or
anything else — is the host's: core opens no storage and holds no index
(charter rule 1). The accepted behavior is in
[`docs/specs/hive-memory.md`](../../../../../docs/specs/hive-memory.md); why a
port exists at all is
[ADR 0030](../../../../../docs/adr/0030-a-host-memory-port-feeds-seat-sessions.md).

## Public surface

| Item | What it is |
| --- | --- |
| `Recall`, `RecallFuture` | Read port. One call per `RecallRequest`; returns `BriefingNote`s. |
| `Remember`, `RememberFuture` | Write port. One call per activation's `RememberRequest`. |
| `RecallMoment` | `SessionStart`, `Rejoin` (other seats' new memory only), `Compaction { dropped }`. `label()` is the wire tag. |
| `RecallRequest` | Seat, per-run conversation namespace, optional focus, moment, character budget. |
| `RememberEntry`, `EntryKind` | One remembered fact: `Observation`, `FailedAttempt`, `Outcome`, or `Note`. |
| `RememberRequest` | Seat, namespace, last desk row seen (`through`), entries. |
| `frame_recalled` | Pure. Renders notes under `RECALL_HEADING` as data, clipped to the budget on a character boundary; `None` when empty or the budget cannot hold the header. |
| `DeskWatermark`, `DeskDelta`, `desk_delta` | Pure. The projected rows after a seat's inclusive watermark, and the advanced watermark. The seat's own and elided rows advance it too. |
| `initialize_session_with_recall`, `RecalledSession` | `initialize_session_with_context` plus a session-start recall. A recall failure degrades to no memory and is returned in `failure`, never as an error. |

Failures are `runtime::Error::Recall` and `runtime::Error::Remember`, which a
port implementation constructs around its own error.

## Constraints

- Recalled notes are returned beside `SessionContext::notes`, never merged
  into them: the framed block is the single place their untrusted origin is
  stated, and the host injects it as its own system text.
- The budget is a hard cap on the framed block, in characters.
- No clock: latency for the `Recalled`/`Remembered` trace events is measured by
  the host.

## Files

| File | What it does |
| --- | --- |
| `mod.rs` | Module overview, the two ports, `frame_recalled`, `desk_delta`, and `initialize_session_with_recall`. |
| `types.rs` | Wire records: `RecallMoment`, `RecallRequest`, `EntryKind`, `RememberEntry`, `RememberRequest`, `DeskWatermark`, `DeskDelta`, and the call-only `RecalledSession`. |
| `test.rs` | Framing and clipping, the desk delta, the degrade-on-failure path, both error variants, and pinned wire forms. |
