# Source layout

| Path | Purpose |
|---|---|
| `lib.rs` | Crate overview and the public surface: `SeatRunner`, `RunnerKind`, `EmbedRunner`, `EmbedSeat`, `RawRunner`, `RawSeat`, `Route`, `offline`. |
| `error/` | What seating or running a seat can fail with. |
| `runner/` | The seam: open, run, close; `Lane`, `TurnJob`; which runner the environment names. |
| `embed/` | Seats as `openhuman-embed` agents, tools over MCP. |
| `raw/` | Seats as raw sessions, tools in-process: the belt, the gate, the memory that keeps nothing. |
| `offline/` | The scripted model, the backend stub and the offline config, behind the `offline` feature and in tests. |
