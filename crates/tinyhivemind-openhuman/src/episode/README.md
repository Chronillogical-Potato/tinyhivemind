# `episode`

`run_episode`: one episode from its door to quiescence, over a journal the
host owns. It moves rows between the host's log, the runner and the
conductor, one wave at a time -- begin the wave, propose the turns, brief
and run each, record what each called, then take the conductor's steps
until the wave settles -- and holds no rule of its own; every rule is the
conductor's.

`Journal` is what a host implements: its `SessionLog`, `commit` and `note`
to append the conductor's rows and return the sequence a commit was given,
and three optional hooks -- `event` to show what the episode did, `compose`
to put its own context in front of the brief, `turn_done` to see a turn's
reply, refusals and recorded calls. `Report` is what an episode came to.

Rows for a turn are read from the log through `project_session`, as the
seat, so a row it was not addressed on is withheld the same way it is when
the turn is seeded. One watermark is read per wave, the log's newest row,
and every read for the wave is bounded by it: the host's log may grow while
the turns are prepared, and a seat is recorded as shown through the
watermark, so a row above it waits for the next turn rather than being
shown twice. The rows above the seat's own watermark are its brief; the
conversations it is shown are fetched by root from
`Conductor::shown_conversations`. A seat shown nothing yet has no
watermark, and a sequence is never borrowed to mean that: a host may number
its first row zero.

| file | holds |
| --- | --- |
| `mod.rs` | `Journal`, `Report`, `run_episode`, reading and rendering rows |
| `test/` | the loop over a scripted runner and no model: `flow.rs` (an episode with a conversation, a stalled one, what the journal saw of each), `watermark.rs` (a log numbered from zero, a log that grows under the loop), `journals.rs` (a journal keeping every default), `support.rs` (the runner and the journals) |
