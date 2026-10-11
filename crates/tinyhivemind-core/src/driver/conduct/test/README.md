# `driver/conduct/test/`

Unit tests for the conductor, driven by a host that is only an in-memory
journal. `mod.rs` declares the behaviour groups below; each file is one group.

| File | Purpose |
| --- | --- |
| `mod.rs` | Module doc and the submodule declarations. |
| `support.rs` | Shared fixtures: a hive of named seats, a router that always asks for clarification, the journal, and one wave driven the way a host drives it. |
| `door.rs` | Who starts an episode: routed starters, a task at sequence zero, refused starters, and every plan shape. |
| `desk.rs` | Desk work: stalled-seat nudges, placed and unplaced broadcasts, handoffs, the broadcast budget, the turn wall, and the commit protocol. |
| `conversations.rs` | Conversations opened by an ask: running first, concluding to the asker, group asks, silent askees, refused replies, and thread prefetch. |
| `parked.rs` | Parked seats held by the host, released where they stopped, and snapshots taken and refused between waves. |
| `links.rs` | The links a host reads back to draw the desk: conversation roots on rows, and the rows events name. |
| `wire.rs` | Pinned wire forms of turns, notes, commits, events, refusals, snapshots, and the conduct policy. |
