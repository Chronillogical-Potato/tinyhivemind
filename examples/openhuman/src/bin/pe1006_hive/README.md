# PE1006 hive support

| File | Purpose |
| --- | --- |
| `episode.rs` | Problem-specific roles, semantic candidates, queue helpers, and validated tool-envelope compatibility. |
| `tools.rs` | Local MCP rendering of `broadcast` and `complete_episode`, plus the host-owned event outbox. |
| `typesafe.rs` | Live System One HTTP transport and routing-request construction. |
| `workspace.rs` | Durable shared-workspace templates, initialization, and exact prompt/reply turn snapshots. |
| `test.rs` | Pins the exact 25-turn budget at concurrent-round boundaries. |

Before any pending or missing-tool retry round is launched, the runner requires
the complete concurrent round to fit within the remaining 25-turn budget. It
never launches a partial round merely to consume the remainder.
