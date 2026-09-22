# Driver test modules

| File | Purpose |
| --- | --- |
| `broadcast_fallback.rs` | Full-round per-author fallback and preflight failure regressions. |
| `coverage.rs` | Persisted-state, routing, replay, and batch-preflight behavior coverage. |
| `ledger.rs` | The ledger's ordering rules: drain after completion, open asks, budgets, delivery guard, quiescence. |
| `round.rs` | Pending-round snapshot and all-or-nothing batch validation. |
