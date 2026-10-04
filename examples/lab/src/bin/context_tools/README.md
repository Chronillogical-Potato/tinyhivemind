# context_tools

What a seat can say and what it can see. `tinyhivemind-tools` is not a
dependency of the lab, so the tool surface is core's own `runtime::speech`.

```sh
cargo run --bin context_tools [-- --trace out.jsonl]
```

| File | Prints |
| --- | --- |
| `main.rs` | runs the two halves; the trace is one `tool_call` per scripted call |
| `tools.rs` | `tool_specs`, `interpret` and its rejections, `commit_utterance` under three `AsidePolicy` settings, every `NoAsideReason`, `fence::extract_post` |
| `projection.rs` | one transcript projected for five viewers, thread scope, `project_as`, masking, mention resolution, `MENTION_CAP` |

Findings: F8, F13, F14 in the [ledger](../../../../../docs/experiments/2026-10-04-findings.md).
