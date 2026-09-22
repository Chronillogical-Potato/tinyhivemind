# Source layout

| Path | Purpose |
|---|---|
| `lib.rs` | Crate overview and the public surface: `run_episode`, `Journal`, `Report`, `SeatRunner`, `RunnerKind`, `HostedRunner`, `EpisodeHost`, `EpisodeBelt`, `EmbedRunner`, `EmbedSeat`, `RawRunner`, `RawSeat`, `LibraryHost`, `Route`, `register_seats`, `offline`. |
| `error/` | What seating or running a seat, or an episode, can fail with. |
| `episode/` | `run_episode` over a `Journal`: one episode from its door to quiescence. |
| `runner/` | The seam: open, run, close; `Lane`, `TurnJob`; which runner the environment names. |
| `journal/` | `MemoryLog`, an in-memory journal that is a real `SessionLog`; always compiled. |
| `hosted/` | Seats as the host's own agents, built through `EpisodeHost`, seeded from the host's log. |
| `embed/` | Seats as `openhuman-embed` agents, tools over MCP. |
| `raw/` | Seats as raw sessions, tools in-process: the belt, the gate, the memory that keeps nothing. |
| `offline/` | The scripted model, the backend stub and the offline config, behind the `offline` feature and in tests. |
