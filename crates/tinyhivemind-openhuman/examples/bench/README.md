# `bench`

What the completion driver's policy costs, measured rather than asserted, with
no model and no credential.

| file | holds |
| --- | --- |
| `main.rs` | the arms, the host loop as a host would write it, the tally, and the Wilson interval |
| `sim.rs` | a stochastic room whose turns land at different times, and a router that returns well-formed evaluations so real acceptance runs |

```sh
cargo run --release -p tinyhivemind-openhuman --example bench
cargo run --release -p tinyhivemind-openhuman --example bench -- --episodes 2000 --members 8
```

Every arm runs the same seeded rooms through the same `CompletionDriver`, so a
difference between rows is the policy. The findings are in
`docs/experiments/2026-09-22-the-driver-under-the-benchmark.md`; the
prototype this replaces, and its `gate` arm, are in
`docs/experiments/2026-09-21-what-the-host-loop-costs.md`.
