# relay_hive

A federation of desks that can reach each other only by referral. A support
seat triages a bug, a router over desk-shaped candidates picks the next desk,
`referral` carries it there, and the answer has to find its way back.

```sh
cargo run --bin relay_hive [-- --trace out.jsonl]
```

| File | Prints |
| --- | --- |
| `main.rs` | the support, backend, infra and ghosts desks, and the run order |
| `relay.rs` | the narrated relay, then `ReferralPolicy` swept over `enabled`, `reach`, `returns` and `max_hops` |
| `gallery.rs` | one input per `NoReferralReason`, plus the three ways a referral succeeds |

The trace carries a `turn_started` and `turn_finished` per hop and a `mark`
for each referral decision.

Findings: F31-F34 in the [ledger](../../../../../docs/experiments/2026-10-04-findings.md).
