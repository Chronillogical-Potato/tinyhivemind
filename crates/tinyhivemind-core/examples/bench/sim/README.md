# Sim

Simulated participants with noisy private evaluations of a shared task: the
room decides between several options, exactly one genuinely best, and no
single member is individually reliable. This is what makes the numbers in
[`../README.md`](../README.md) reproducible — no model, no network, one seed
in and one transcript out.

| file/dir | what it does |
| --- | --- |
| [`mod.rs`](mod.rs) | `Room` and the task definition: truth, options, and the property the whole benchmark measures — whether deliberation pools noisy private signals better than a single responder. |
| [`generation.rs`](generation.rs) | `Room::generate_with`: draws a room's truth, its per-topic expert assignment, and every member's private evaluations from one seed, plus the one-room reproducibility self-check. |
| [`view.rs`](view.rs) | `View` — the read-only projection a turn composes against, folded once through the library's own `resolve`/`standings`. Also the parsing helpers for the private-check grammar and the example's own self-check stand-in for `cargo test` coverage. |
| [`agent/`](agent/README.md) | `SimAgent`, split by concern into state and turn-composition. |
