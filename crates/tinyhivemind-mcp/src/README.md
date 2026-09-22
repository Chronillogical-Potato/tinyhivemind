# Source layout

| Path | Purpose |
|---|---|
| `lib.rs` | Crate overview and the public surface: `EpisodeTools`, `SeatEvent`, `Dispatch`, `Server`, `serve`. |
| `error/` | The crate error: a bind failure. Refusals to a seat are not errors; they travel as tool results. |
| `tools/` | What the server remembers: registered turns, per-seat inboxes, and the read window the host refreshes. |
| `render/` | `tool_specs()` as MCP tool definitions, and MCP arguments onto `CallArguments`. |
| `server/` | The loopback listener, HTTP/1.1 framing, and the JSON-RPC methods MCP needs. |
