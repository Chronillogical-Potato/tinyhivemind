# `seed`

What a seat's turn is *given* as its history, read from the host's journal as
that seat. Shared by the two runners that hold a session across an episode:
`hosted` and `embed`. A raw seat keeps its own log and seeds from that.

| file | holds |
| --- | --- |
| `mod.rs` | `history`, the projection of the host's log into `(role, content)` pairs as one seat reads it; and `with_persona`, which puts that seat's standing prompt at the head of them |

Seeding rather than resuming is the point. `openhuman-embed`'s `Turn::seed`
takes the path that drops whatever the session composed and puts these rows in
its place, with the durable transcript's autoload suppressed. A turn that
*resumes* instead is the runtime's own continuity, which binds a transcript to
the session the first time it commits and refuses a later turn whose target is
not the same binding -- so a seat that spoke twice failed on its second turn.
