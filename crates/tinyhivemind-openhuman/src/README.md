# Adapter modules

| Path | Responsibility |
| --- | --- |
| `host/` | Supplied handle registration, continuing sessions, host hooks and management |
| `tools/` | Stable native specifications, argument validation and bound execution |
| `journal/` | Optional in-memory host log used by standalone research examples |
| `offline/` | Feature gated loopback backend and host runtime configuration fixtures |
| `error.rs` | Typed adapter errors |
| `lib.rs` | Public exports and registration example |

Agent construction, MCP connections, skills, memory and original prompts belong
to the host. Durable scheduling and episode state belong to `tinyhivemind-hives`.
