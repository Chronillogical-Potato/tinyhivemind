# PE1006 hive support

| File | Purpose |
| --- | --- |
| `episode.rs` | Problem-specific roles, semantic candidates, queue helpers, and validated tool-envelope compatibility. |
| `tools.rs` | Local MCP rendering of `broadcast` and `complete_episode`, plus the host-owned event outbox. |
| `typesafe.rs` | Live System One HTTP transport and routing-request construction. |
| `workspace.rs` | Durable shared-workspace templates, initialization, and exact prompt/reply turn snapshots. |
