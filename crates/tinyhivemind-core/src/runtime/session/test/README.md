# `session` tests

Unit tests for validated attributed projection, grouped by behavior area.

| File | Covers |
| --- | --- |
| `support.rs` | The scriptable `FakeLog` (`SessionLog` impl over an in-memory `VecDeque` of pages), and builders for a `Conversation`, `SessionQuery`, raw `LogMessage`, and `SessionPage` used by every submodule below. |
| `channel.rs` | Channel-level projection: root-and-first-reply narrowing, per-root promotion, and the window/scan bookkeeping that stops a channel read. |
| `paging.rs` | `validate_page` and the scan-cap behavior of `project_session` when a host returns a malformed page or a projection exhausts `SCAN_LIMIT` before its window is met. |
| `thread.rs` | Thread-scoped projection: walking a reply chain back to its root, stopping at a blank root or the window, and propagating read failures. |
| `visibility.rs` | Private-aside visibility: what each viewer reads, how a run of elided rows collapses into one stub, and how a stub settles. |
| `wire.rs` | Serde representation of every session payload type, and the object-safety of the `SessionLog` port. |

`mod.rs` wires these five behavior-area modules plus `support` into the
module tree; it carries no tests of its own.
