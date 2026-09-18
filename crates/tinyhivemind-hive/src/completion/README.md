# Completion-driven episodes

This module is the pure state machine for episodes whose termination is an
explicit agent action rather than an inference from rounds or quorum.

| File | Purpose |
| --- | --- |
| `mod.rs` | Stable state, status, completion, and routed-assignment folds. |
| `test.rs` | Completion, reopening, validation, idempotency, and wire tests. |

The host turns a `complete_episode` tool call into `apply_completion`. A
`broadcast` is first routed and validated by the embedding layer; only the
accepted recipients enter `apply_assignment`. The module performs no model
call, IO, transcript parsing, or scheduling.
