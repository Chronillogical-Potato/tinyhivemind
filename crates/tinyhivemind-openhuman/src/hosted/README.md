# `hosted`

`HostedRunner`: every seat is the host's own agent. The host implements
`EpisodeHost` -- its `SessionLog`, a `build_seat` that adds an `EpisodeBelt`
to the agent it already builds, and a `wrap_turn` that installs what its
tools read while a turn runs -- and the runner does the rest. Each seat is
built once per episode, because `OpenHuman` fixes a belt at build time, and
reused every turn: cleared, seeded from the host's log as the seat up to its
watermark, run on the brief, its usage kept.

Seeding reads nothing above the watermark. The rows above it are the turn's
new rows, which reach the seat in its brief, so the seat sees every row once,
and a row a peer wrote in the same wave reaches it through neither.

`EpisodeBelt::admit` wraps the host's own gate: the episode's tools are
admitted, everything else is the host's gate's to decide, and with no host
gate everything else is denied.

| file | holds |
| --- | --- |
| `mod.rs` | `EpisodeHost`, `HostedTurn`, `EpisodeBelt`, `HostedSeat`, `HostedRunner` |
| `seed.rs` | a seat's history from the host's log, as `(role, content)` pairs |
| `admission.rs` | the gate that admits the episode's tools over the host's |
| `test.rs` | seeding, withholding, the watermark, a thread, the memory log's pages, the belt and its gate |
