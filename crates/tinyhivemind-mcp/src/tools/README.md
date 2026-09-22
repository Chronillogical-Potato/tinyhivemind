# `tools`

What the server remembers, per seat: the registered turn, the calls made
during it, and the read window the host last refreshed.

| file | holds |
| --- | --- |
| `mod.rs` | `EpisodeTools`, `Dispatch`, `SeatEvent`; `register`/`clear`, `window`, `drain`, and `call` -- caller, turn, thread, then `interpret`, then the record |
| `test.rs` | turns are visible until cleared, draining empties, the window is a snapshot, and the in-process call refuses where the wire refuses |

The host writes the turn and the window and drains the calls; a call writes
the record, whether it came over the wire or was made in-process by a host
whose harness takes native tools. `call` is the one place that decides, so the
server is framing around it and nothing here reaches into the host.
