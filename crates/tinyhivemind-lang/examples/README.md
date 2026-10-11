# Language examples

| File | Purpose |
| --- | --- |
| `hive_view/` | Render a package (text, mermaid, JSON) and run it offline through core's `hive::step`; NDJSON frames for the viewer; the math `--compare` |
| `hives/` | The example package gallery: seven complete packages, from a three-seat review board to a fifty-seat math swarm |
| `live_self_edit/` | Provider-failure regression tests and their directory guide |
| `live_self_edit.rs` | Opt-in OpenRouter proposal, exact-match held-out evaluation, negative control and telemetry/contamination guard checks |
| `self_edit.rs` | Propose a prompt delta, guard the candidate, supply a host evaluation, and archive accepted and rejected designs |

Run from the repository root:

```sh
cargo run -p tinyhivemind-lang --example self_edit
cargo run -p tinyhivemind-lang --example hive_view -- crates/tinyhivemind-lang/examples/hives/review-board --run
cargo test -p tinyhivemind-lang --example hive_view
```

`hive_view` is documented in [`hive_view/README.md`](hive_view/README.md),
including its wire format and the measured `--compare` result.
