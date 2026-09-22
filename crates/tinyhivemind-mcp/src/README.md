# Source layout

| Path | Purpose |
|---|---|
| `lib.rs` | Crate overview and the public surface: `Server`, `serve`, and the record re-exported from `tinyhivemind-tools`. |
| `error/` | The crate error: a bind failure. Refusals to a seat are not errors; they travel as tool results. |
| `server/` | The loopback listener, HTTP/1.1 framing, and the JSON-RPC methods MCP needs, over `EpisodeTools::call`. |
