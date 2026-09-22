# 21. An assignment is appended rather than overwritten, and a participant holds at most one open

- **Status:** Proposed
- **Date:** 2026-09-21
- **Amends:** [ADR 0019](0019-complete-episodes-with-explicit-agent-events.md)

## Context

[ADR 0019](0019-complete-episodes-with-explicit-agent-events.md) made completion
an explicit typed event, on the grounds that inferring it from prose or from a
turn barrier "repeats work and confuses a finished turn with finished work." It
was right about the event and silent about the thing being completed.
`ParticipantCompletion` is

```rust
{ agent_id: String, assigned_at: Sequence, completed_at: Option<Sequence> }
```

— one slot, overwritten in place. A completion can therefore only mean *your
latest*, and `broadcast` exists precisely to give a participant a second
assignment. The protocol turns on an invariant that its own primitives neither
maintain nor check.

Four failures follow, and they are one failure seen from four directions.

**A concurrent assignment silently overwrites an open one.** A handoff routed to
a participant already working sets `assigned_at` forward and loses the previous
assignment, because the struct cannot hold two. `status()` does not move — the
participant was pending and stays pending — so nothing observes it. That
participant's eventual completion then resolves by timing rather than by intent:
above the new sequence it is accepted and recorded against the *new* assignment,
marking an agent done on work it never received; at or below it, it is rejected
as stale and a genuine completion of real work goes unrecorded. Which of the two
happens is decided by a number the host allocated.

**The stale check guards one direction only.** "A stale event cannot complete
newer work" stops an old completion landing on new work. Nothing stops a new
completion landing on work the agent never saw, because the check has no way to
ask *which* work — there is one slot and it always answers. The same
one-sidedness appears on the reopen path: `apply_assignment` compares against
`assigned_at` and never against `completed_at`, so a participant holding
`{ assigned_at: S0, completed_at: Some(S9) }` accepts an assignment at any
sequence above S0, S5 included, erasing a completion recorded later than the
assignment that replaces it.

**Nothing can tell two handoffs apart.** The fold never sees a message, a topic
or a thread, so a second broadcast about work already handed off is
indistinguishable from new work — an overwrite, a redo, or, if the target had
finished and the episode had settled, an un-completion, since
`apply_assignment` does not consult `status()`. Cemri et al. put step repetition
at 15.7% of observed multi-agent failures, the largest single category
([MAST](https://arxiv.org/abs/2503.13657)), and
[`../specs/expert-delegation.md`](../specs/expert-delegation.md) already names it
as the hazard of a defer chain. In the deliberation episode the library was
immune by construction — `quorum::standings` folds two members raising one topic
into support, and the citation grammar makes the second a response to the
first — and none of that exists here.

**There is no measure that only goes up.** `apply_assignment` undoes
`apply_completion` by clearing `completed_at`, so a participant that finished
once and was reopened is indistinguishable from one that never finished. The
same study puts unawareness of termination conditions at 12.4%. A termination
argument needs a quantity that moves one way and cannot do so forever; the
deliberation episode had `turn_budget`, documented as "Finite, so an episode
always terminates," and this episode has nothing.

Four answers are tempting and each is narrower than it looks.

**Keep the slot and refuse the second assignment.** Erroring when the
participant is already pending does maintain the invariant, and it is half of
what is decided below. On its own it fixes the overwrite and nothing else: a
reopen still clears `completed_at`, the history is still destroyed, and there is
still no monotone measure to terminate against.

**Resolve the subject from the message.** The model knows which assignment it
finished and could say so in prose. That is exactly the inference ADR 0019
exists to refuse, and it would return the one fact the protocol turns on to the
channel that ADR removed it from.

**Give an assignment a name.** An opaque host-supplied id is the general answer
and is more than this problem needs. `assigned_at` is already unique within a
participant — the stale check forces the sequences in one participant's list to
increase strictly — and it is already the key every surrounding mechanism uses:
the per-assignment broadcast cap, and the host's delivery guard. A parallel
identity would give one thing two names, which is the second-journal problem
`directory` is refolded per step to avoid. A name is needed only if an agent can
hold two open assignments at once, and nothing yet demonstrates that need.

**Leave the mapping to the host.** This is the status quo, and it produced all
four failures: the host holds the identity, the library holds the state, and no
single fold can check one against the other. A host that tracks assignments
correctly gets no help, and a host that does not gets no error.

## Decision

**A participant holds an append-only history of assignments, and at most one of
them is open.**

```rust
pub struct AssignmentRecord {
    /// Sequence at which this assignment was given. Unique within the
    /// participant, and its identity.
    pub assigned_at: Sequence,
    /// Sequence of the `complete_episode` call that finished it.
    pub completed_at: Option<Sequence>,
}

pub struct ParticipantCompletion {
    pub agent_id: String,
    /// Append-only, strictly increasing in `assigned_at`.
    pub assignments: Vec<AssignmentRecord>,
}
```

Two changes, and each covers what the other leaves open.

**`apply_assignment` appends rather than overwrites.** A reopened participant
gains a record; it does not lose one. The evidence that an earlier assignment
completed survives, because that assignment is a different record holding its
own `completed_at`. This is what the refusal below cannot supply on its own.

**`apply_assignment` refuses a participant that already has an open record**,
with `Error::AssignmentWhileOpen`. The invariant the whole protocol rests on
stops being host discipline and becomes a checked precondition. A host that
queues a handoff for a busy participant — rather than assigning it — is
unaffected, because it only ever assigns to a participant with nothing open; a
host that forgets gets a typed error instead of silent corruption. This is what
the append cannot supply on its own.

`apply_completion` keeps its signature. There is nothing to name: it resolves
the participant's single open record, or fails with
`Error::NoOpenAssignment`. The existing guards keep their shape and acquire a
subject — `completed_at == Some(at)` is still an idempotent replay, and
`at <= assigned_at` is still `StaleCompletionEvent`, now asked of *that record's*
`assigned_at` rather than of the participant's only one.

**`complete_episode { message }` is unchanged**, and so is every agent.

**A participant is pending when its last record is open**, and

```rust
pub fn settled(&self) -> usize    // records with `completed_at.is_some()`
```

never decreases: records are append-only and `completed_at` is never cleared.
That is the well-founded measure the protocol has been missing. A termination
argument can be made against it; none could be made against `status()`.

## Consequences

- **The overwrite is unrepresentable, and its absence is checked.** Two open
  assignments cannot exist, so "which did you finish" has one answer, and it is
  no longer decided by which sequence the host happened to allocate.
- **No agent changes, and no tool-surface change.** The decision is entirely
  below the tool boundary. A host adopting it changes how it stores state and
  nothing about what it advertises.
- **The `assigned_at`-versus-`completed_at` edge disappears.** Each record is
  compared to itself, and a new record cannot wipe an older one's completion
  because it is not the same record.
- **A duplicate handoff becomes visible instead of silent.** A second broadcast
  to a busy participant now fails loudly rather than overwriting, and a host
  queueing it can see the open record it must wait behind. The fold still cannot
  tell that two handoffs concern the same work — it has no content — so
  deduplication remains host-side. Making the collision *observable* is the whole
  of what this buys, and it is strictly more than an overwrite.
- **Storage grows with handoffs.** A participant's list is as long as the number
  of assignments it has received, and until a broadcast cap exists that is
  unbounded. This is the price of the history that makes `settled` monotone, and
  it is bounded by the same follow-up that bounds the chain.
- **The surrounding mechanisms keep one key.** A per-assignment broadcast cap is
  keyed `(agent_id, assigned_at)` — scoped so that a participant pending on one
  assignment cannot loop, while a reassigned participant legitimately gets a
  fresh budget — and a host's delivery guard compares its per-agent watermark to
  the same field. Neither acquires a second name. Moving that cap into policy is
  follow-up work, on the `defer_cap` precedent and clamped per member on the
  `ExchangePolicy::remaining` one.
- **`CompletionStep::Stalled` is deliberately not added.** A stalled episode is
  one where no participant is running and nothing will wake any of them, and
  neither conjunct is observable to a fold over stored rows. It stays a host
  predicate until something gives the library an honest input for it, which is a
  different decision.
- **Hosts get a wire migration and no new port.** `participants[].assigned_at`
  and `completed_at` become `participants[].assignments[]`, required and
  non-null: a payload written before this decision fails to decode rather than
  acquiring a default, on the convention `refutation_cap` set and
  [ADR 0010](0010-an-aside-carries-information-never-support.md) followed.
  Nothing else in the public surface moves, no trait is added, and the module
  stays pure.

**Known limitation: resolving a subject is not proving one.** The single open
record is resolved positionally, so a participant that was handed work and
completed without doing it is recorded exactly as one that did. Completion has
always been the agent's claim and this does not change that. What it changes is
that the claim becomes *checkable against delivery*: a host tracking the
sequence through which it last delivered to an agent can refuse a completion
whose record was assigned above that watermark. Under the single slot that
comparison passed trivially — `assigned_at` was always the latest and the
watermark was already past it from earlier work — and per record it says
something. The guard is not part of this decision because the watermark is host
state, but this decision is what makes it expressible.

**If a host demonstrates a workload that needs two open assignments on one
participant**, reversing the refusal needs a new ADR and the named id declined
above — not a quiet relaxation of the check. The measure to beat is that under
one open assignment, `complete_episode` needs no argument and no agent needs to
know what an assignment is.

## Related

- [ADR 0019](0019-complete-episodes-with-explicit-agent-events.md) — completion
  as an explicit event; this gives it a subject and a history.
- [ADR 0017](0017-validate-semantic-routing-at-the-port.md) — a provider
  suggests recipients and cannot expand membership or fan-out; an assignment's
  identity likewise comes from the host's journal rather than from a model.
- [ADR 0007](0007-the-directory-is-folded-from-citations.md) — `defer_cap`, the
  precedent for bounding the chain this decision makes countable.
- [ADR 0010](0010-an-aside-carries-information-never-support.md) — the wire
  convention a required field follows.
- [`../specs/completion-driven-episodes.md`](../specs/completion-driven-episodes.md)
  — the behaviour this amends.
- [`../notes/completion-episode-review.md`](../notes/completion-episode-review.md)
  — the review this came out of, and the host-side decisions that accompany it.
