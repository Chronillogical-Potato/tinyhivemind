# Source layout

| Path | Purpose |
|---|---|
| `lib.rs` | Crate overview and the public surface: `SeatRunner`, `RunnerKind`, `HostedRunner`, `EpisodeHost`, `EpisodeBelt`, `EmbedRunner`, `EmbedSeat`, `RawRunner`, `RawSeat`, `LibraryHost`, `Route`, `offline`. |
| `error/` | What seating or running a seat can fail with. |
| `runner/` | The seam: open, run, close; `Lane`, `TurnJob`; which runner the environment names. |
| `hosted/` | Seats as the host's own agents, built through `EpisodeHost`, seeded from the host's log. |
| `embed/` | Seats as `openhuman-embed` agents, tools over MCP. |
| `raw/` | Seats as raw sessions, tools in-process: the belt, the gate, the memory that keeps nothing. |
| `offline/` | The scripted model, the backend stub, the offline config and an in-memory journal that is a real `SessionLog`, behind the `offline` feature and in tests. |
