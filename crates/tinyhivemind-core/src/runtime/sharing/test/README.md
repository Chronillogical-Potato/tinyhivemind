# `sharing` tests

Unit tests for caller-owned watermark sharing, grouped by behavior area.

| File | Covers |
| --- | --- |
| `support.rs` | The scriptable `FakeLog`, small conversation/row/page builders, and the `plan`/`delta` helpers every submodule uses to drive `prepare_delta` and unwrap its happy path. |
| `state.rs` | `initialized_state` and `note_present`: watermark seeding, and the present-above-watermark bookkeeping used to record a row already accepted through a concurrent path. |
| `delta.rs` | `prepare_delta`'s walk: attribution and chronological order, watermark/present-set filtering, desk and thread scoping across General aliases, paging mechanics reusing P4 page validation, and the retry/compare-and-swap contract a host relies on. |
| `reinit.rs` | `Conversation::equivalent_to`, and the three conditions under which `prepare_delta` gives up on an incremental delta and asks for a full P4 initialization instead. |
| `visibility.rs` | Private-aside visibility for `prepare_delta`: it applies the same audience predicate the projection does, a member reads its own aside, and a re-seed never hands a member less than the delta already did. |
| `wire.rs` | Serde representation of sharing payload types. |

`mod.rs` wires these five behavior-area modules plus `support` into the
module tree; it carries no tests of its own.
