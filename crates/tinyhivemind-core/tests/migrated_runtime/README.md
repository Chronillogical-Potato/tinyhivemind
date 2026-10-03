# Runtime integration tests

These tests use only the public `tinyhivemind` API.

| File | Coverage |
| --- | --- |
| `public_api.rs` | Crate root exports for session records, sharing state, core algebra, pins, briefing budgets, and digests. |
| `utterance_surface.rs` | A fixed speech script through `interpret` and `commit_utterance`, including row attribution and audience. |
| `joining_a_folded_room.rs` | The account and live rows given to a seat joining a long-running room. |

Module tests under `src/` cover the detailed behavior. The public API tests
check that a host can reach those types and folds without using private paths.
