# run

`mod.rs` runs one arm and builds the `Summary` that becomes `result.json` (`mode, model, tokens_in, tokens_out, wall_ms, turns, completed`, rounds, `context_policy`, `seat_session`, `memory` and per-seat usage). It owns the run's `Sessions` and borrows the memory the binary opened. `test.rs` runs both arms against a scripted model.
