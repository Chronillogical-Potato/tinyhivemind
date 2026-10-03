# Private asides

This module answers one question: may this authored line be addressed to fewer
readers than its conversation holds, and to exactly whom? It performs no IO,
retains no state, and stores nothing — the row it authorizes is appended
through the same path every other message uses.

`Audience` rides on a stored row. `Audience::Desk` is what every row written
before this mechanism means and what a host writes when it means nothing
special; `Audience::Aside { members }` restricts a row to its author plus the
named agent ids. `Viewer` rides on a read, because a projection that cannot
name its reader cannot narrow for it. `Audience::admits` is the whole of the
rule, and it admits every `Operator` and `Person` unconditionally: privacy here
is between agents and is a deliberation device, never a security boundary.

The decision order is intentionally observable: disabled policy, then snapshot
validation, then an inactive or off-desk author, then the thread requirement,
the message budget and the unsettled-aside check, and only then the addressed
ids in reading order. The first addressed id that cannot be resolved **stops
the decision** rather than being skipped — an audience assembled by quietly
dropping the names it could not resolve is not the audience the author wrote.
Person, desk and everyone mentions address nobody, so an aside cannot be opened
against the room.

`AsidePolicy::DEFAULT` permits nothing. Every bound is the host's: this crate
imposes no ceiling on members or messages, and `must_surface` and
`require_thread` are opt-in shapes a host may insist on.

Two rules give the mechanism its shape and neither is enforced here, because
neither is a decision this fold could make:

- **A redaction is a row, not an absence.** `tinyhivemind`'s projection elides
  content and keeps the row, so a reader outside an audience still sees that
  the exchange happened, who wrote it and to whom. That is what makes an aside
  auditable rather than a covert channel.
- **An aside carries information, never support.** `tinyhivemind_core::hive` drops
  traces from non-`Desk` rows uniformly, for every reader, so a private line
  moves no option toward a decision. To make an aside count, a member spends a
  desk-visible turn saying so in the open.

See [`docs/specs/private-asides.md`](../../../../docs/specs/private-asides.md)
and [ADR 0010](../../../../docs/adr/0010-an-aside-carries-information-never-support.md).
