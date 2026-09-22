# `server`

The loopback listener, HTTP/1.1 framing, and the JSON-RPC methods MCP needs:
`initialize`, `notifications/initialized`, `tools/list`, `tools/call`.

| file | holds |
| --- | --- |
| `mod.rs` | `Server`, `serve()`, the session loop, `call()` -- caller, turn, thread, then `interpret` -- and the framing |
| `test.rs` | framing and the refusal shape, without a socket |

A refusal is a tool *result* with `isError`, never a JSON-RPC error: the seat
reads it inside its own turn, while it can still call again. The live wire is
exercised end to end in `../../tests/wire.rs`.
