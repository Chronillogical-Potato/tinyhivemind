# Session projection

This module walks a host-owned, globally sequenced log without owning storage.
The `SessionLog` port returns newest-first pages through boxed futures so it is
object-safe and independent of an executor. `project_session` validates every
page, counts all raw rows against a fixed scan budget, filters one desk/channel
or direct-child thread, and returns attributed messages chronologically.

The host must treat `before` as exclusive and return `next_before` no newer than
a nonempty page's oldest sequence and strictly older than the requested cursor.
Empty pages end a walk. A malformed page is a typed error rather than a reason
to retry or silently accept incomplete order. Reaching a thread root ends the
walk even when blank-content filtering omits that row from the projection.

## Audience

`SessionQuery` carries a `Viewer`, and a row whose `Audience` does not admit it
is **elided rather than dropped**: it keeps its sequence, its author and its
audience, and loses only its content. Dropping it would leave `^N` citations
naming nothing and would hide from a peer that there is something to ask about.

A run of elided rows from one aside collapses into a single stub carrying the
sequence range, the message count and where the aside settled. Collapsing runs
after the window is filled, so a viewer with asides in view receives fewer
messages than one without — the alternative is backfilling from older history,
which would make two viewers of the same desk disagree about how far back the
window reaches. `project_as` applies the same narrowing to a transcript a
caller already holds.

## Layout

- `mod.rs` — the `SessionLog` port, `project_session`, `project_as`, and the
  channel/thread projection walks and elision-collapsing folds behind them.
- `types.rs` — the wire records this module hands a host: `SessionAuthor`,
  `SessionMessage`, `SessionPage`, `SessionQuery`, `Conversation`, `Elision`,
  and `Sequence`.
- `test/` — unit tests grouped by behavior area, with fixtures factored into
  `test/support.rs` rather than duplicated per file:
  - `wire.rs` — serde round-trips for every payload type, plus the
    `SessionLog` object-safety check.
  - `paging.rs` — `validate_page` and the scan-cap behavior of
    `project_session` under a misbehaving or exhausted host.
  - `channel.rs` — root-and-first-reply narrowing at the desk level.
  - `thread.rs` — walking a reply chain back to its root.
  - `visibility.rs` — private-aside elision, collapsing, and settlement.
