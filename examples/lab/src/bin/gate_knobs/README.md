# gate_knobs

The pure gates at the bottom of the stack, driven through every branch with
hand-built snapshots. No transcript, no model and no trace.

```sh
cargo run --bin gate_knobs
```

| File | Prints |
| --- | --- |
| `main.rs` | runs the four tables |
| `identity.rs` | the four spellings of the default desk, the desk overlay, every invalid snapshot, the roster's three states |
| `dispatch.rs` | one input per `NoDispatchReason`, built by hand where `resolve` would pre-empt it |
| `responder.rs` | the responder ladder rung by rung, `accept_selection`, `accept_evaluation` |
| `approval.rs` | `approve`: policy rules, approvers, malformed requests, grants and refusals |

Findings: F35, F36 in the [ledger](../../../../../docs/experiments/2026-10-04-findings.md).
