# `elsewhere`

What a seat's *other* conversations hold, for the turn it is taking in this
one.

A turn is shown its own channel by whoever runs it. This module answers the
rest: for a seat about to speak in one conversation, the newest rows of
every other conversation it is in, read as that seat. It is the read a host
would otherwise write itself, and writing it in the host is how a seat ends
up reading rows it was never addressed on — so it lives here, over the same
`SessionLog` port and the same projection as every other read.

`gather_elsewhere` takes an `ElsewhereQuery`: the seat, every conversation
it is in, the one its turn is in (skipped, and `None` skips nothing), an
exclusive `before` bound, and a window. It returns one `Elsewhere` per
conversation read, each holding that conversation's projected rows.

Three properties are the point:

- **Narrowed to the seat.** Every read projects as `Viewer::Agent`, so a
  private row reaches the seat elsewhere exactly when it would reach it
  there — never more.
- **One moment.** `before` bounds every conversation's read, so a turn's
  context is a snapshot rather than a set of reads drifting row by row
  while the log grows.
- **Nothing is dropped.** A conversation the seat may read nothing of comes
  back with no rows rather than being left out, so a caller that listed its
  channels gets the same list back and can say "nothing new" about one.

This module stores nothing and decides nothing. What the rows mean for a
turn is the caller's: `tinyhivemind-driver` carries them in
`EpisodeBrief::elsewhere` and renders them under a heading that says they
are context, not work.

`render_row` is the one-line rendering every caller uses to put a row in
front of a model — `@author: content`, and nothing for a row the viewer may
not read.

| file | holds |
| --- | --- |
| `mod.rs` | `gather_elsewhere`, `render_row` |
| `types.rs` | `ElsewhereQuery`, `Elsewhere` |
| `test.rs` | the skip, the narrowing, the bound, a thread as its own conversation, and a failed read |
