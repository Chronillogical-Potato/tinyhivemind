# `briefing` tests

Unit tests for ephemeral team initialization, grouped by behavior area.

| File | Covers |
| --- | --- |
| `support.rs` | Shared fixtures: a default named ("engineering") conversation with no thread scope, a default viewer briefing, and a helper for building one with a given aside policy. |
| `snapshots.rs` | `TeamBriefing::from_snapshots`: desk-order filtering for a named desk, roster order for General, error propagation from invalid snapshots, and construction without host role types. |
| `rendering.rs` | `system_text` / `system_text_with_dispatch`: coordination rules, the brevity overrun report, aside-grammar teaching, and the mention-dispatch offer withheld or granted depending on run context (policy, hop budget, `MentionDispatchContext`). |
| `context.rs` | `SessionContext` rendering and `initialize_session` / `initialize_session_with_context`: threads, pins, and notes carried beside (never merged into) history; read-failure propagation; and the stated window restating only what a viewer with an elided aside actually received. |
| `wire.rs` | Serde representation of briefing payload types. |

`mod.rs` wires these four behavior-area modules plus `support` into the
module tree; it carries no tests of its own.
