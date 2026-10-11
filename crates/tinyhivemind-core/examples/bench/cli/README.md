# `cli/`

Command-line parsing for the bench binary. The flag table itself is in
`main.rs`'s crate doc.

| File | Purpose |
| --- | --- |
| `mod.rs` | `Options`, the parsed argument set every other module reads, and `Mode`, what `--sweep`, `--trace`, `--swarm` and the other modes pick between. |
| `flags.rs` | The per-flag handlers `Options::parse` delegates to, grouped by what they configure: scale, expertise, and the live backends. |
| `test.rs` | Flag parsing regressions: fidelity and fact-noise validation, and how `--api-base` interacts with the explicit modes. |
