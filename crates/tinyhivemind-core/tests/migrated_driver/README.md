# Integration tests

| File | Purpose |
| --- | --- |
| `review_regressions.rs` | Public completion-driver replay and participant-routing regressions. |
| `scheduling.rs` | Accepted routing order, deterministic queue merging, and completed-recipient pruning. |
| `fuzz_invariants.rs` | Properties that hold under arbitrary event orderings: one open assignment, `settled` never falls, no seat both woken and stalled, a snapshot round trip loses nothing. |
