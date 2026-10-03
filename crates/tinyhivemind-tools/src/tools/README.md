# `tools`

What the record remembers, per seat: the registered turn, the calls made
during it, and the read window the host last refreshed.

| file | holds |
| --- | --- |
| `mod.rs` | `EpisodeTools`, `Dispatch`, `SeatEvent`; `register`/`clear`, `window`, `drain`, and `call` -- caller, turn, thread, then `interpret`, then the record |
| `test.rs` | turns are visible until cleared, draining empties, the window is a snapshot, and invalid calls are refused |

The host writes the turn and the window and drains the calls. A native tool
calls `EpisodeTools::call`, which validates the call and writes the record.
Nothing here reaches into the host.
