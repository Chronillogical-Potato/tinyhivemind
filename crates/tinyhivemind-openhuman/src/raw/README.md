# `raw`

`RawRunner`: every brief a `RawSeat`, which builds an `OpenHumanSessionHost`
per turn with the belt, the gate, the memory and the prompt as objects, and
drops it. `prepare` registers the seats as workspace definitions before any
runtime boots; `seat` boots a `LibraryHost` and seats every brief on it.

`LibraryHost` is the core booted as a library host over one route: sessions
built from objects on it, and turns run under its context. It is public for
any host with no core of its own, which is how the example's hosted host
builds its seats.

| file | holds |
| --- | --- |
| `mod.rs` | `RawRunner`, `Route`, `register_seats` (and `prepare`, which names the served belt), `seat`, the per-seat context log |
| `library.rs` | `LibraryHost`: the library-host core, its sessions, and its scope |
| `seat.rs` | `RawSeat`: one session built, seeded, run and dropped |
| `tools.rs` | the served definitions as `tinytools::Tool`s whose execute is `EpisodeTools::call` |
| `policy.rs` | `EpisodeGate`, admitting only the belt, and `NoMemory` |
| `test.rs` | the belt, the gate, the memory, and a refused route |
