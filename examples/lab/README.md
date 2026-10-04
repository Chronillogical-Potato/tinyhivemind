# lab

A standalone Cargo workspace for the hive lab: trace sinks, runnable examples
and the SWE hive. It stays outside the library workspace so core keeps its
pure dependency tree. The root README of the repository describes the library.

| Path | Purpose |
| --- | --- |
| `src/` | shared plumbing (`JsonlSink`, `WallClock`, in-memory log), `bin/` examples, and `swe/`, the SWE hive and baseline |
| `viewer/` | offline viewer for the JSONL traces |
| `harbor/` | Harbor agent wrapper for Terminal-Bench |
| `tests/` | mock model server and the offline end-to-end script |

Contract checks: `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, run from this directory.
