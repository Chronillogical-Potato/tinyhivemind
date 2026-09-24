# `episode`

`run_episode`: one episode from its door to quiescence, over a journal the
host owns. It moves rows between the host's log, the runner and the
conductor, one wave at a time -- begin the wave, propose the turns, brief
and run each, record what each called, then take the conductor's steps
until the wave settles -- and holds no rule of its own; every rule is the
conductor's.

`Journal` is what a host implements: its `SessionLog`, `commit` and `note`
to append the conductor's rows and return the sequence a commit was given,
and seven optional hooks -- `display_name` to say what a person calls a
seat, which rows, briefs and the tools' replies use in place of `@id`,
`event` to show what the episode did, `compose`
to put its own context in front of the brief, `turn_done` to see a turn's
reply, refusals and recorded calls, `channels` to name the seat's other
conversations, `released` to say which parked seats the host has settled,
and `checkpoint` to keep the snapshot a restart resumes from. `Report` is what an episode came to.

A turn that comes back `TurnResult::Parked` is recorded with whatever it
called and then held: the conductor stops proposing that seat. When a wave
has nothing to run and seats are parked, the loop asks `released`, which is
where a host blocks on its own approval queue; a host that releases nobody
ends the episode with `Error::Parked` rather than spinning.

`checkpoint` is handed a `ConductorState` after every committed row, and
again at the end of each wave so a wave that only parked or nudged a seat
is durable too. `resume_episode` carries an episode on from the newest one:
the same conversations open, the same seats held, and a wave that was in
progress resumed mid-wave. A host that keeps nothing loses a running
episode to a restart.

The residual window is one row: a crash between a row landing and
`checkpoint` returning leaves the journal ahead of the snapshot, so the
seat that wrote that row runs again. A host that cannot tolerate a
duplicate keys its appends and drops one it has already written.

`channels` names every conversation the seat is in that this episode does
not run. Their newest rows are read through `gather_elsewhere`, bounded by
the same wave watermark as every other read, and carried in
`EpisodeBrief::elsewhere` under a heading that says they are context.

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
| `mod.rs` | `Journal`, `Report`, `Released`, `run_episode`, reading and rendering rows |
| `test/` | the loop over a scripted runner and no model: `flow.rs` (an episode with a conversation, a stalled one, what the journal saw of each), `watermark.rs` (a log numbered from zero, a log that grows under the loop), `parking.rs` (a seat held on the host, and its other conversations in its brief), `journals.rs` (a journal keeping every default), `support.rs` (the runner and the journals) |
