# `tools`

What the server remembers, per seat: the registered turn, the calls made
during it, and the read window the host last refreshed.

| file | holds |
| --- | --- |
| `mod.rs` | `EpisodeTools`, `Dispatch`, `SeatEvent`; `register`/`clear`, `window`, `drain` |
| `test.rs` | turns are visible until cleared, draining empties, the window is a snapshot |

The host writes the turn and the window and drains the calls; the server
writes the calls and reads the rest. Nothing here reaches into the host.
