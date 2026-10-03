# Continuous sharing

This module prepares attributed transcript additions between a caller-owned
watermark and the next triggering sequence. It reuses the session module's log
port, page validation, and channel/thread filtering.

The module stores nothing. A host owns `SharingState`, calls `prepare_delta`,
accepts the returned messages and current trigger into its agent session, and
only then commits `next_state` under its own serialization or compare-and-swap.
A failed acceptance or lost CAS leaves the prior state reusable for a safe
retry. Reinitialization directs the host back through P4 briefing and history.
Restored state is validated during deserialization, while every public
operation independently rejects a manually constructed present set over the
64-entry bound before reading or mutating anything.

Every raw row counts against the bounded scan even when it belongs to another
conversation. Sequence gaps are valid because the host sequence is global.
The walk succeeds only after observing a row at or below the old watermark;
otherwise it distinguishes an excessive gap from unavailable retained history.

## Audience

`SharingQuery` carries a `Viewer`, and the delta applies the same predicate the
projection does. If it did not, an agent that re-seeded would see a different
transcript from one that stayed incremental and nothing would heal the
difference.

Elided rows collapse within one delta only: a run split across two ticks cannot
merge, because the earlier half is already in the agent's context and this
crate holds no memory of having sent it. Note also that a narrow-audience
viewer crosses `SCAN_LIMIT` sooner — the walk counts raw rows, not delivered
ones — so `GapTooLarge` and its full re-seed arrive more often.

## Layout

- `mod.rs` — `initialized_state`, `note_present`, and `prepare_delta`.
- `types.rs` — the wire records: `SharingState` (with its bound-checked
  `Deserialize`), `SharingQuery`, `SessionDelta`, `ReinitializeReason`, and
  `SharingPlan`.
- `test/` — unit tests grouped by behavior area, with fixtures factored into
  `test/support.rs` rather than duplicated per file:
  - `wire.rs` — serde round-trips, including the oversized-present-set
    rejection.
  - `state.rs` — `initialized_state` and `note_present` bookkeeping.
  - `reinit.rs` — `Conversation::equivalent_to` and the three conditions that
    send `prepare_delta` back to a full P4 initialization.
  - `delta.rs` — the delta walk itself: attribution, chronological order,
    desk/thread scoping, paging mechanics, and the retry/CAS contract.
  - `visibility.rs` — private-aside narrowing applied to a delta.
