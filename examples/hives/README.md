# Coordinator examples

These examples use scripted host runners and `MemoryStorage`. They need no model,
credentials, or network connection. The host creates the runner, registers it,
accepts messages, and drains the coordinator; the runner submits explicit
episode actions.

| File | Demonstrates | Run |
| --- | --- | --- |
| `one_hive.rs` | A planner asks a reviewer in a private child conversation, then completes its assignment. | `cargo run --manifest-path examples/hives/Cargo.toml --bin one_hive` |
| `shared_agent.rs` | One agent continues the same session across three hive assignments. | `cargo run --manifest-path examples/hives/Cargo.toml --bin shared_agent` |

For real supplied OpenHuman agents and native tools, see the
[standalone host example](../openhuman/README.md).
