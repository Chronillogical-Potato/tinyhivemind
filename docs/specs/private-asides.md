# Private asides

> **Current API note.** This proposal may mention the retired directory, deferral, refutation, or evidential policy paths. [ADR 0027](../adr/0027-retire-unused-adapters-and-optional-mechanisms.md) records their removal. Use the current crate API for active policy fields.


**Status:** Draft
**Owner:** tinyhivemind maintainers

## Problem

Nothing in this workspace can be said to fewer than everybody. `SessionMessage`
carries an author and no audience, `SessionQuery` carries a conversation and no
viewer, and `matches_conversation` decides membership of a projection from the
row and the conversation alone. Two agents on the same desk therefore receive
byte-identical projections, and there is no fold that could make them differ.

Addressing is not privacy. `@auditor` selects one responder and can start one
bounded child turn; the message it travels in is written into the desk channel
and read by every member. A host that wants a pair of agents to compare notes
before either commits to a position has no move except to give up the desk.

That is a real loss, and the reason is measured rather than aesthetic. A room in
which everyone reads everything converges on what everyone already shares.
[ADR 0005](../adr/0005-a-blind-round-may-be-concurrent.md) records the size of
the effect inside this very harness: a blind opening decides 99.4% of rooms at
82.1% correct, against 100.0% and 58.0% at full visibility — a 24-point gap
bought entirely by withholding peers' positions for one round. `Visibility` buys
that once, crudely, for the whole room and only at the start of an episode.
There is no way to buy it for two members, about one question, in the middle.

The reading behind this is
[`../research/context-in-agent-teams.md`](../research/context-in-agent-teams.md),
which also carries the counter-evidence: privacy in a system that already pools
information badly can make the pooling worse.

## Goals

- Let a message name an audience narrower than its conversation, and let a
  projection differ per reader as a result.
- Keep every private exchange **auditable**: its existence, author, addressees
  and extent are visible to every member of the desk, and its content is visible
  in full to any person.
- Make a bounded private exchange owe the room a **settlement** written in the
  open, so that what it produced becomes ordinary desk state.
- Keep the room's counting single-valued. Quorum, the attention market and the
  directory answer for the room, not for a reader.
- Cost a reader with a sliding window as little as possible: one row per aside,
  carrying a pointer to where it settled.
- Preserve today's behaviour exactly for a log that contains no aside.

## Non-goals

- **A second journal.** An aside is rows in the host's log, addressed by
  sequence, like everything else.
- **A shared mutable memory block.** Attaching one block to several agents so a
  write by one appears in every other's context is a second store that has to be
  invalidated, which the charter's first rule forbids.
- **Confidentiality against a person.** Operators and people read aside content
  in full. The privacy here is between agents and is a deliberation device, not
  a security boundary.
- **A generated summary of an aside.** Condensing one agent's words for another
  loses the reasoning and puts words in an author's mouth in the shared record.
  A participant writes its own settlement or there is none.
- **Mutation.** An audience is fixed when the row is appended.

## Proposed behavior

### The audience

```rust
pub enum Audience {
    Desk,
    Aside { members: Vec<String> },
}
```

`Audience::Desk` is what every row written before this spec means, and what a
host writes when it means nothing special. `Aside { members }` restricts the row
to its author plus the named agent ids.

A **closed thread** is the idiom rather than a second mechanism: a thread whose
root carries `Aside { .. }`, with every reply carrying the same audience.
`AsidePolicy::require_thread` lets a host insist on that shape.

### The viewer

```rust
pub enum Viewer {
    Operator,
    Person { id: String },
    Agent { id: String },
}
```

`SessionQuery` gains one, because a projection that cannot name its reader
cannot narrow for it. `Operator` and `Person` read every row in full — read-only
audit access, never membership: a person reading an aside does not join it, is
not counted among its members, and does not change what its agents are told.

`Audience::admits` is the whole of the rule:

```rust
pub fn admits(&self, viewer: &Viewer, author: &SessionAuthor) -> bool
```

`Desk` admits everyone. `Aside` admits `Operator`, any `Person`, the row's own
author, and an `Agent` named in `members`. Nobody else.

### The projection

A row a viewer is not admitted to is **not dropped**. It is projected at its own
sequence, keeping its author and its audience, with its content elided.
`SessionMessage` gains `audience` and `elided`, and one accessor that every
caller quoting content goes through:

```rust
/// The authored text, or `None` when this row was elided for the viewer.
pub fn readable(&self) -> Option<&str>
```

Three reasons the row stays. **Auditability**: two agents cannot exchange
anything without leaving an attributed, sequenced row, which is precisely the
covert channel the literature names. **Latent asymmetry**: an agent that cannot
see that a peer knows something has no reason to ask, and that is the documented
failure of collective reasoning under distributed information. **Citations**: the
trace grammar addresses messages by sequence, and silently removing rows leaves
`^N` naming nothing.

### Reading an aside under a context budget

The reader is a language model with a sliding window, so a correct filter is not
sufficient — a run of elided rows spends the window to say nothing, and spends it
in the middle, which [`long-context.md`](../research/long-context.md) records as
the worst place to spend it.

**Consecutive elided rows from one aside collapse into a single projected row.**
It carries the sequence range, the members, the message count, and where the
aside settled:

```text
[7-10] @planner → @auditor · aside, 4 messages · settled at [11]
```

The settlement pointer is what makes a hole in a reader's context recoverable. A
non-member does not need the content; it needs to know that the outcome is at
`[11]` and that `[11]` is a row it may read. An unsettled aside says so, which is
an actionable prompt rather than a gap. A citation `^9` landing inside the range
resolves to the collapsed row, so the reader learns that its citation names
something it may not read — the honest answer, at one row instead of four.

Members keep recall inside their own aside. `search_messages`, `search_threads`
and `!pin ^N` all respect the audience in both directions: they never surface a
row to a viewer who is not admitted, and they do surface one to a viewer who is.
Without the second half an agent cannot re-find what it said privately once the
window has moved past it, which is worse than never having said it.

### An aside carries information, never support

There is exactly one transcript for counting and it is the same for every
reader. `step` continues to fold the whole transcript, and `project_for`
continues to be the only thing that narrows — the asymmetry `Visibility` already
established. A per-viewer fold would make `quorum::standings`, `attention::bids`
and `directory` return well-formed wrong answers with no error path: a quorum
that one member can see and another cannot, a floor-holder that differs by
reader when there is one floor, and a per-reader belief about who knows what.
Their order-independence is idempotent under redelivery and reordering, and
never under omission.

What an aside changes is a uniform semantic rule, applied identically for every
reader: **a trace deposited in a row whose audience is not `Desk` contributes
nothing.** It adds no supporter, moves no option toward a decision, silences no
advocate, and earns no directory credit. To make an aside count for anything, a
member spends a desk-visible turn saying so in the open.

This is the rule [ADR 0006](../adr/0006-a-referral-crosses-one-channel-at-a-time.md)
already accepted at a channel boundary, applied inside one desk: what crosses a
visibility boundary carries information, never a vote.

### The policy and the fold

```rust
pub struct AsidePolicy {
    pub enabled: bool,
    pub max_members: usize,
    pub max_messages: usize,
    pub must_surface: bool,
    pub require_thread: bool,
}
```

`AsidePolicy::DEFAULT` has `enabled: false` and every bound at zero, so a host
that constructs one by default gets no asides at all.

`aside(policy, input, roster, desks) -> AsideDecision` evaluates in a fixed
order, each rung checked before the next:

1. a disabled policy returns `Disabled`;
2. the roster and desk snapshots are validated;
3. an inactive author returns `SourceInactive`;
4. an author that is not an effective active member of the conversation's desk
   returns `AuthorNotOnDesk`;
5. the addressed members are resolved in reading order; an empty set returns
   `NoAudience`, a set naming only the author returns `SelfOnly`, and a set
   larger than `max_members` returns `AudienceTooLarge`;
6. any addressed id that is not an effective active member of this desk returns
   `TargetNotOnDesk`, and an inactive one returns `TargetInactive`. A later
   candidate is never used as a fallback;
7. `require_thread` with a channel-level conversation returns `ThreadRequired`;
8. an aside already at `max_messages` returns `BudgetSpent`, and under
   `must_surface` an unsettled prior aside between the same members returns
   `UnsettledAside`.

Everything fails closed, and every refusal has a name.

### The authored grammar

Two markers, in the shape of the existing `!pin` line, taught in the briefing
**only when `AsidePolicy::enabled`** — a grammar is a fixed cost paid in every
agent's system text on every turn, and teaching a move nobody may make spends
that budget for nothing:

```text
!aside @peer      then what you need from them, privately
!surface          then what the room needs to know from it
```

The briefing also gains a fourth shared-session rule, stated as something an
agent can act on rather than as a disclaimer: some rows show only that an aside
happened; you cannot read them; if one matters, ask its author in the desk.

`BrevityPolicy` states the **achieved** window rather than the nominal one. Every
scan bound in this workspace counts raw rows inspected, so a viewer admitted to
little of a busy desk receives fewer messages for the same budget, and a briefing
that promised thirty is wrong.

### The runtime edge

None. An aside is decided from arguments the caller already holds, and the row it
produces is appended through the same path every other message uses. The host
authorizes and stores; there is no new port and no new idempotency boundary.

### Riding alongside a turn

An aside costs no turn. One authorized turn produces the member's ordinary
desk-visible contribution and, optionally, one aside row: a host appends both
and commits the state the turn returned, exactly once. The episode cannot vote
it, because `live_traces` drops a non-desk row before it reaches a trace or a
standing, and `spent` counts turns rather than rows.

It is not free of a sequence. Sequence numbers are unique across the one
shared journal, so the aside row still takes the next one, and every later
desk row lands at a higher raw sequence than it would have without the aside.
`salience::standing` scores recency from that raw distance, and salience feeds
the floor-holder choice — so "the episode cannot tell" covers votes, standings
and `spent`, but not the decay a busier journal produces. See the "Known
limitation" note on [ADR 0011](../adr/0011-an-aside-rides-alongside-a-turn.md).

Three bounds hold, and they are what keep this inside the charter's third rule:

- **At most one aside row per turn.** A room of *n* members writes at most *n*
  aside rows per round, and each of them cost its author a turn it had won.
- **An aside starts no turn.** A host does not hand an aside row authored inside
  an episode to `mention_dispatch`. The peer answers on its own next turn, which
  the attention market was going to give it; an exchange completes at the pace of
  the floor or it does not complete.
- **A refused audience is dropped, not published.** If the `aside` fold declines
  to make the row private, the row is not written. Falling back to the desk would
  put a second desk-visible contribution on one turn, which is the one thing a
  turn may not produce.

`HiveStep::Speak` still carries exactly one turn and no two participants ever
hold the floor: what rides alongside a turn is a row, not a turn. See
[ADR 0011](../adr/0011-an-aside-rides-alongside-a-turn.md), and the invariants
pinned in `episode::test` and `crates/tinyhivemind-core/tests/migrated_hive/fuzz_invariants.rs`.

## Invariants and constraints

- An audience is fixed at append time. Widening one later could never be
  redelivered, because a sharing watermark advances past filtered rows
  unconditionally, and it would invalidate citations besides.
- A row is never removed from a projection on audience grounds; it is elided.
- `Operator` and `Person` viewers are admitted to every row.
- An author is always admitted to its own row.
- A trace in a non-`Desk` row contributes to no standing, for any reader.
- `step`, `standings`, `bids` and `directory` fold the whole transcript and are
  unchanged by audience metadata on rows they do not count.
- Collapsing elided rows is order-independent and depends only on the projected
  slice.
- No read path quotes content that `readable()` returned `None` for — pins,
  search excerpts, thread openings and reply counts included.
- The fold is pure: no clock, no IO, no host type.
- Every payload is `Eq`, `serde`-round-trips, and pins its wire form in a test.

## Wire compatibility

`LogMessage.audience`, `SessionMessage.audience` and `SessionQuery.viewer` are
`Option` fields deserialized with `deserialize_required_option`, so the key must
be written even when null. This is the convention `refutation_cap` established
and `directory` restated: a payload serialized before a policy-bearing field
existed fails to decode rather than quietly acquiring a default, and here the
default that absence would acquire is the permissive one.

It is a breaking change for a host that stores serialized rows, and the cost is
one field per row. Behaviour is otherwise preserved exactly: in a log with no
aside, every viewer projects what it projects today.

## Acceptance criteria

- A non-member's projection of an aside is one row, at the right sequence, with
  the right author, members, count and settlement pointer, and no content.
- The same rows project in full under `Viewer::Person` and `Viewer::Operator`.
- A `!support` written inside an aside changes no standing, for its members or
  anybody else, and `step` returns the same decision with and without the aside
  rows present.
- No aside content is reachable through a search hit, a pin excerpt, a thread
  opening, or an inflated reply count. Each is a test.
- A member re-finds its own aside by search and by pin after the window has moved
  past it.
- A member that re-seeds never receives a poorer view than it held.
- A live run drives a real room through an aside and records what the agents did
  with it — in particular whether a non-member ever acts on a settlement pointer,
  which is the mechanism's whole claim to being more than a partition.
- The mechanism is allowed to lose. **It lost**, and the result is
  [`../experiments/2026-09-07-do-asides-help.md`](../experiments/2026-09-07-do-asides-help.md):
  a pairwise check costs 2.5 points at the tuned turn budget and nothing at an
  unconstrained one, loses 15 points on a hidden profile because averaging
  inside one correlated desk imports the shared bias, and is indistinguishable
  from the same exchange held in the open — so privacy buys no decision quality
  at all. That is why `AsidePolicy::DEFAULT` is off, and the spec claims
  auditability and bounded independence for the mechanism rather than better
  answers.

## Open questions

- **The required field costs a migration.** `#[serde(default)]` meaning `Desk` is
  defensible — `Desk` is the historically true value for every row written before
  this spec, so the default reproduces the past rather than inventing a policy.
  The convention was followed instead because the failure it prevents, a writer
  that drops the field on one path and silently publishes a private message, is
  worse than the migration. This is the one place the trade could reasonably go
  the other way.
- **What a non-member sees is a product decision, not only a technical one.** The
  attributed stub is chosen for auditability, latent asymmetry and citations. A
  host that wants a genuinely invisible exchange would need the row absent, and
  would then have to say what happens to a citation naming it.
- **A settlement can arrive after its stub has shipped.** On the incremental
  sharing path a stub delivered in one tick keeps the `settled_at: None` it was
  sent with even when a participant settles the aside in a later tick, because
  the watermark has already moved past the row and this crate cannot revise a
  message it no longer holds. A re-seed shows it settled. Carrying pending-aside
  state across ticks would be a second store to invalidate, so the field is
  documented as a floor — "settled by here" — rather than made exact.
- **Bound erosion is unaddressed.** `SCAN_LIMIT`, `PIN_SCAN`, `SEARCH_SCAN` and
  `THREAD_INDEX_SCAN` all count raw rows, so a viewer admitted to little of a
  desk gets proportionally less for the same cost. Stating the achieved window
  is honest but is not a fix, and a per-viewer multiplier was not attempted.
- **An aside that does not ride on a turn at all** — now specified separately in
  [`off-floor-exchange.md`](off-floor-exchange.md) and
  [ADR 0012](../adr/0012-an-exchange-round-spends-model-calls-not-turns.md).
  An aside riding alongside a turn is free but still rationed by the floor: a
  room converging in eleven turns writes at most eleven aside rows, and the arm
  that stays inside the turn contract reaches `+1.4` against a `+30.8` ceiling.
  An exchange round lifts the ration at an explicit, finite, host-set price in
  model calls. See
  [`../experiments/2026-09-07-why-asides-lose.md`](../experiments/2026-09-07-why-asides-lose.md).
- **Whether an aside should reach its member during a blind round.** It does not
  today: `project_for` withholds every peer row under `Visibility::Blind`, so an
  exchange cannot begin until positions have formed. Letting an aside through
  would start the exchange earlier, and is worth `+0.5` — inside the noise, and
  not enough to justify putting a channel the library cannot inspect through the
  one filter [ADR 0005](../adr/0005-a-blind-round-may-be-concurrent.md) measures
  24 points on. Measured and declined rather than assumed.
- **`must_surface` is enforced at the next aside, not at the last turn.** Nothing
  compels a settlement before an episode ends, so a room can close with an aside
  unsettled. Making the episode refuse to converge on an unsettled aside was
  considered and rejected as too blunt for a first cut.
