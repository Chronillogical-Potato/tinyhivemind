# Source layout

| Path | Purpose |
|---|---|
| `lib.rs` | Crate overview and centralized public exports. |
| `error/` | Typed graph, routing, and committed-event failures. |
| `graph/` | The owned one-desk graph, `BoundAgent`, and the bindings to its seats. |
| `driver/` | Resumable completion rounds, the ledger, the brief, and committed-event folds. |
| `conduct/` | The conductor: the desk episode with its conversations, nudges, sorting, refusals and walls, stepped by a host that appends the rows. |
| `test_support.rs` | Test-only seat fixtures shared by unit tests: a name, and an executor. |
