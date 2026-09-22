# `conduct`

One completion-driven episode as a host steps it: the desk episode, a child
episode for every conversation an `ask` opens, and the rules between them
that no single fold can hold.

| file | holds |
| --- | --- |
| `mod.rs` | `Conductor`, `ConductPolicy`, `Door`, `starters`; opening the desk, beginning a wave, proposing turns, opening a turn with its brief, recording what it called |
| `wave.rs` | After a wave: the phase machine that hands the host one `Step` at a time -- commits in conversations, silent askees, commits on the desk with their consequences, conclusions, the turn wall |
| `child.rs` | A conversation: its root, its two seats, its own driver state, its turns, its nudge; and one that concluded |
| `steps.rs` | `Turn`, `Note`, `Commit`, `Event`, `Refusal`, `Step` |
| `test.rs` | Every rule, driven by a host that is only a journal |

The rules, each with the decision it comes from:

- **A conversation runs first** (ADR 0023): it is what unblocks a desk turn.
  It concludes when the seat asked completes, at `child_turn_wall`, or when
  nothing is due anywhere; its outcome reaches the asker as a private row,
  which releases the asker's hold. The seats that had it are shown it whole
  once, on their next desk turn.
- **Nudges** (ADR 0024): a desk seat that holds open work, ran for it and
  has been shown everything is told once per assignment and owed a turn. A
  seat asked that took its turn without answering is told once and owed a
  turn; a second silence stands.
- **Sorting**: a broadcast or an ask made inside a conversation is desk
  work; only a post or a completion is a row of the conversation.
- **Refusals**: a completion the ledger refuses is explained to the seat on
  the desk. A spent broadcast budget completes the seat with the work.
- **Walls**: turns per conversation, turns per episode.

The conductor appends nothing. It hands the host a `Note` to append, a
`Commit` to append and report the sequence of, or an `Event` to log, and
takes the sequence back through `committed`. The host owns the journal,
the rendering of a row, the prompt, and running the turn.
