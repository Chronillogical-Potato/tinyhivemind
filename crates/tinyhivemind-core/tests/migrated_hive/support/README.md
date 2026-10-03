# Test support

Shared, non-`#[test]` fixtures for the integration suite in `tests/`. Both
files are `#![allow(dead_code)]` since not every test binary uses every item.

| file | what it does |
| --- | --- |
| [`hive_harness.rs`](hive_harness.rs) | A small in-memory host that drives one deliberation episode: owns the journal and the `HiveAgent` trait, runs the `step` loop, and reports why the episode stopped (`Outcome`). Mirrors exactly what a consuming host must do — append, never wait, never call back into the library. |
| [`scripted_agent.rs`](scripted_agent.rs) | A deterministic `HiveAgent` that replays a fixed script of lines, recording which sequences each of its turns was actually shown, and falls back to a filler line once the script runs out. |
