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
the turn is seeded. The rows above the seat's watermark are its brief; the
conversations it is shown are fetched by root from
`Conductor::shown_conversations`.

| file | holds |
| --- | --- |
| `mod.rs` | `Journal`, `Report`, `run_episode`, reading and rendering rows |
| `test.rs` | an episode with a conversation, a stalled one, and what the journal saw of each, over a scripted runner and no model |
