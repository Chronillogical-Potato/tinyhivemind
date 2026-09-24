# Source layout

| Path | Purpose |
|---|---|
| `lib.rs` | Crate overview and the public surface: `EpisodeTools`, `SeatEvent`, `Dispatch`, `Refusal`, `served_specs`, `tool_definitions`, `raw_arguments`. |
| `tools/` | What the record remembers: registered turns, per-seat inboxes and refusals, the read window the host refreshes, the seats' display names, and `call`. |
| `render/` | `tool_specs()` as tool definitions, and JSON arguments onto `CallArguments`. |
