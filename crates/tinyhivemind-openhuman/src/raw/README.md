# `raw`

`RawRunner`: every brief a `RawSeat`, which builds an `OpenHumanSessionHost`
per turn with the belt, the gate, the memory and the prompt as objects, and
drops it. `prepare` registers the seats as workspace definitions before any
runtime boots; `seat` boots the library-host context and resolves the route.

| file | holds |
| --- | --- |
| `mod.rs` | `RawRunner`, `Route`, `prepare`, `seat`, the per-seat context log |
| `seat.rs` | `RawSeat`: one session built, seeded, run and dropped |
| `tools.rs` | the served definitions as `tinytools::Tool`s whose execute is `EpisodeTools::call` |
| `policy.rs` | `EpisodeGate`, admitting only the belt, and `NoMemory` |
| `test.rs` | the belt, the gate, the memory, and a refused route |
