# session

Seat conversations that outlive one activation. With `--seat-session
persistent` (the default) a seat's messages are kept for the whole run and a
later activation appends only the desk delta; compaction is the only thing that
removes messages. `--seat-session fresh` restores the old behaviour, where each
activation starts from the briefing.

| File | Purpose |
| --- | --- |
| `mod.rs` | `Sessions`: the per-run store, `take` / `put` around each activation |
| `types.rs` | `SeatSession` (messages, `read_through` watermark, activations, pins seen, last prompt) and `SessionMode` |
| `test.rs` | the store keeps or drops sessions by mode |
