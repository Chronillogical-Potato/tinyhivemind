# Source layout

| Path | Purpose |
|---|---|
| `lib.rs` | Crate overview and centralized public exports. |
| `error/` | Typed graph, routing, and committed-event failures. |
| `graph/` | The owned one-desk graph and OpenHuman agent bindings. |
| `driver/` | Resumable completion rounds and committed-event folds. |
| `test_support.rs` | Test-only OpenHuman runtime and agent fixtures shared by unit tests. |
