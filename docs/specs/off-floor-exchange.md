# Off-floor exchange

> **Current API note.** This proposal may mention the retired directory, deferral, refutation, or evidential policy paths. [ADR 0027](../adr/0027-retire-unused-adapters-and-optional-mechanisms.md) records their removal. Use the current crate API for active policy fields.


**Status:** Accepted
**Owner:** tinyhivemind maintainers
- **Plan:** [`../plans/off-floor-exchange.md`](../plans/off-floor-exchange.md)

This fold is optional and disabled by default. The later
[room-size experiment](../experiments/2026-09-09-topology-at-scale.md) found
that continuous exchange reached 4,173 contacts per episode at 64 members
without improving accuracy on that hidden-profile task. A host enabling it
should set the caps from its model-call budget.

## Problem

An aside is free but starved.

[ADR 0011](../adr/0011-an-aside-rides-alongside-a-turn.md) let a private row ride
alongside the turn that authored it, so a member no longer pays a floor turn to
ask a peer anything. That removed the whole of the cost — a 15.5-point swing on
a hidden profile — and it left the exchange bounded by the wrong thing. An aside
still *rides* on a turn, so a room that converges in eleven turns can write at
most eleven private rows, which in a room of five is about two contacts each.

The benchmark says that is nowhere near enough
([`../experiments/2026-09-07-why-asides-lose.md`](../experiments/2026-09-07-why-asides-lose.md)):

| arm | hidden profile, budget 40 |
| --- | --- |
| `hive+along` — one row per authorized turn | `+0.5 [+0.1, +0.9]` |
| `hive+share` — the same row spent continuously | `+1.4 [+0.4, +2.4]` |
| `hive+pooled` — every reading and every fact, free | `+30.8 [+28.9, +32.9]` |

Twenty-one points of peer information exist and a protocol tied to the floor
reaches two of them. The gap is not privacy, not payload, not targeting and not
accounting — every one of those was isolated and measured. It is **bandwidth**,
and bandwidth is capped because a private row may only be written by whoever the
attention market happened to authorize.

The biology says the same. A colony's contacts are not rationed by whose turn it
is to forage. Trophallaxis happens continuously and in parallel with the work,
and that is precisely why a colony's information is *pooled* rather than
sampled.

## Goals

- Let members exchange privately **without any of them taking the floor**, so
  the volume of exchange is set by what a host will pay for rather than by how
  many turns the deliberation happens to take.
- Keep the guarantee that makes this safe **provable rather than asserted**: the
  episode's decision, floor, standings, phase and budget must be bit-identical
  whether an exchange happened or not.
- Bound the spend with an explicit, finite, host-set ceiling that is knowable
  **before** the episode runs.
- Make the price **visible**. An exchange round is *n* model calls; a benchmark
  or a host that hides that is lying about what the mechanism costs.
- Keep every row auditable in the one log, on the terms
  [`private-asides.md`](private-asides.md) already sets.

## Non-goals

- **A second journal.** Spend is folded from the transcript the host already
  holds — private rows above the watermark, counted by author. Nothing is
  stored, so nothing can disagree with the log.
- **A port, or any waiting.** The library decides *whether and how many*; the
  host does the calling. `exchange` is a fold over arguments the caller already
  has, like every other decision in this crate.
- **A second floor.** An exchange round moves no standing, resolves no counting
  trace and settles nothing. To make a private finding count, a member still
  spends a desk-visible turn saying so in the open.
- **Starting a deliberation turn.** No exchange row takes the floor or advances
  the episode. See below.
- **Deciding who talks to whom.** Targeting is a participant's judgement. The
  library authorizes and bounds; it does not choose a peer.
- **Making an exchange mandatory, or on by default.** `ExchangePolicy::DEFAULT`
  is disabled.

## Proposed behavior

### An exchange round

Between episode rounds, or before the first one, a host may run an
**exchange round**. It does not interleave with an authorized turn, so the
transcript a turn was composed from is never edited underneath it. In one
exchange round, each eligible member may append at most one private row. No
member takes the floor, no `HiveStep` is produced, and `EpisodeState` does not
advance.

The host asks the library whether a round is open:

```rust
pub fn exchange(
    policy: &ExchangePolicy,
    state: &EpisodeState,
    opened: ExchangeState,
    transcript: &[SessionMessage],
    roster: &Roster<'_>,
    desks: &DeskSet<'_>,
) -> Result<ExchangeRound>;

pub enum ExchangeRound {
    /// Each of these members may append at most one private row now.
    Open {
        members: Vec<String>,
        /// Rows this episode may still write, across every member.
        remaining: u64,
        /// The state to carry into the next call, having opened this round.
        next: ExchangeState,
    },
    /// No round, and why.
    Closed { reason: NoExchangeReason },
}
```

`members` is in desk order and contains only members that are active on the
episode's desk and still under their own contact cap. A host runs zero or more
of them, in any order, and may run none.

### The budget, and where it lives

```rust
pub struct ExchangePolicy {
    /// Off unless the host turns it on.
    pub enabled: bool,
    /// Private rows one member may author across the whole episode.
    pub contact_cap: u32,
    /// Rounds the episode may open at all.
    pub round_cap: u32,
}
```

Two independent ceilings, both finite. `round_cap` bounds rounds absolutely, and
a round asks at most one row of each **currently active** member, so the total
private rows an episode can produce is at most

```text
round_cap × max_active_members
```

where `max_active_members` is the largest the desk gets during the episode. Each
member is independently bounded by `contact_cap`, whenever it joined.

Membership is the host's and this fold does not freeze it: a desk that grows
mid-episode has more members to ask, and the newcomer arrives with its own
untouched `contact_cap`. So a host that adds members must read its ceiling off
the largest desk it will allow, not the one it opened with. Freezing the roster
inside `ExchangeState` was considered and rejected — it would silently exclude a
member the host legitimately added, which is a worse behaviour than a ceiling
stated correctly.

**Spend is folded, never stored.** A member's contacts are the private rows
above the episode's watermark that it authored. There is no counter to keep
consistent with the log, which is the same reason `directory` and `standings`
fold rather than accumulate.

`enabled: false`, `contact_cap: 0` or `round_cap: 0` all close every round, and
a `Closed` round names which. Zero is a configuration a host may hold
deliberately, so unlike `defer_cap` it is not an error.

### What an exchange round may not do

Four prohibitions, and the first three are already properties of the algebra
rather than new promises:

1. **It may not move the episode's fold.** A private row is dropped by
   `live_traces` before it reaches a trace, a standing, the sequence they fold
   at, the directory or the floor, and `EpisodeState::spent` counts turns rather
   than rows. `step` is therefore invariant under the addition of private rows
   **for the same desk rows at the same sequences**. It is not invariant under
   the sequence *shift* a live host produces by writing them: `window` and
   salience decay read raw sequence distance, so a later desk row can fall out
   of a window it was inside. See the invariant below, and the open question.
2. **It may not start a turn.** A host does not hand a row authored in an
   exchange round to `mention_dispatch`. The exchange cannot cascade: rows do
   not beget rows, and the only thing that produces more of them is the next
   round, which the round cap bounds.
3. **It may not carry support.** [ADR 0010](../adr/0010-an-aside-carries-information-never-support.md):
   an aside carries information, never support, uniformly for every reader.
4. **It may not be written by a member the round did not name.** A row from
   anyone else is the host exceeding its authorization, and the fold that
   counts spend will see it on the next round.

### Why an exchange round does not expand the episode's turn budget

The charter's third rule names its own failure mode: *"a mention that could
start N turns without an approval in sight"*. Each clause fails to apply here,
and it is worth being explicit rather than gesturing at the difference.

- **N turns.** An exchange round starts none. No member gains the floor, no
  `HiveStep::Speak` is produced, and the deliberation's turn budget is untouched.
  What rides in a round is *n* rows, and a row is not a turn — which is exactly
  the distinction ADR 0011 established and two tests pin.
- **Could start.** The danger in rule 3 is unboundedness: `@everyone` mentions N
  agents, each of whom may mention N more. An exchange round cannot cascade,
  because a row is never dispatched. The worst case is a constant a host
  computes from its own policy.
- **Without an approval in sight.** The approval is the policy. It is off by
  default, it is set by the host and not by a participant, and it is finite.

What the rule protects is the **floor** — the shared, ordered record in which
one message causes one agent to run and the room's standing moves. An exchange
round touches none of that. What it does spend is model calls, which is a real
cost and a host's to authorize, and the reason the ceilings above are explicit
rather than implied.

This is also not the concurrency
[ADR 0002](../adr/0002-hive-episodes-are-sequential.md) rules out. That decision
is about the *episode*: `HiveStep::Speak` carries exactly one turn and there is
no variant carrying two. `exchange` is deliberately a separate fold rather than a
`HiveStep` variant, so the episode state machine keeps that property literally
and visibly.

### What a host does

```text
loop:
    step(...) -> Speak { turn }
    run that one turn, append it, commit turn.next_state
    exchange(...) -> Open { members, .. }
        for each member the round named:
            ask it for one private line, or nothing
            validate the audience through `aside(..)`
            append it, or drop it if the audience is not private
```

An aside whose audience the `aside` fold refuses is **dropped, not published**.
Falling back to the desk would turn a private row into a second desk-visible
contribution nobody was authorized to make.

## Invariants and constraints

- `step` returns the same `HiveStep`, including `next_state`, for a transcript
  with and without any number of private rows interleaved anywhere, **holding
  the desk rows' own sequences fixed**. Under live allocation the private rows
  shift later desk sequences, and `QuorumPolicy::window` and salience decay read
  that distance raw, so the fold's indifference to private rows does not extend
  to the numbering. Pinned by
  `episode::test::shifting_desk_sequences_past_a_private_row_can_change_the_step`.
- An episode's total rounds never exceed `round_cap`, including rounds in which
  every named member declined to write, and its total private rows never exceed
  `round_cap × max_active_members`.
- A member never authors more than `contact_cap` private rows, whenever it
  joined the desk.
- `ExchangePolicy::DEFAULT` is disabled, and a disabled policy makes every
  projection, decision and turn identical to one taken before this spec existed.
- A `Closed` round names a reason; there is no silent no-op.
- Every row remains readable in full by the operator and by any person, and
  visible as an attributed stub to every non-member, exactly as
  [`private-asides.md`](private-asides.md) requires.

## Acceptance criteria

- A benchmark arm that runs exchange rounds reports its rows in a column of its
  own, so the cost is on the same page as the gain.
- The mechanism is allowed to lose, and a loss is published.
- The `step`-invariance property is asserted by a fuzz test over arbitrary
  transcripts, not only by a hand-written case, and fails if the audience filter
  is removed.
- Turning the policy off reproduces every published number to the decimal.

## Measured

Recorded in
[`../experiments/2026-09-07-why-asides-lose.md`](../experiments/2026-09-07-why-asides-lose.md),
2000 rooms, paired bootstrap against the same rooms with the policy off.

| arm | hidden profile, budget 40 | model calls/ep |
| --- | --- | --- |
| `hive+along` — one row per authorized turn | `+0.5 [+0.1, +0.9]` | — |
| `hive+share` — the same, spent continuously | `+1.4 [+0.4, +2.4]` | — |
| `hive+rounds` — exchange rounds off the floor | **`+3.2 [+1.8, +4.5]`** | 45.0 |
| `hive+pooled` — the ceiling | `+30.8 [+28.9, +32.9]` | — |

Uniform rooms are a null at every setting, as every exchange arm is: a room whose
members differ only by independent noise holds no concentrated information for a
contact to move.

## Open questions

- **Collusion surface.** More private bandwidth is more room for covert
  coordination, which is the named mechanism in
  [Colosseum](https://arxiv.org/abs/2602.15198) and
  [Secret Collusion](https://arxiv.org/html/2402.07510v3). The mitigation here is
  total audit rather than restriction — every row is in the one log and readable
  in full by any person — and that is a weaker guarantee than prevention. A host
  running untrusted participants should hold `contact_cap` low.
- **Context pressure on non-members.** A round can add one stub per member per
  round. Consecutive stubs from one aside collapse, but stubs from *different*
  asides do not, so a heavily exchanging room costs a non-member more rows than
  a lightly exchanging one. Not addressed here beyond the existing collapse.
- **A round before the first turn is allowed, and the blind round blunts it.**
  `project_for` withholds every peer row under `Visibility::Blind` whatever its
  audience, so rows written before the room opens are not readable by their
  recipients until blindness lifts — they arrive in a batch rather than early.
  Admitting asides through that filter was measured at `+0.5` and declined
  ([ADR 0011](../adr/0011-an-aside-rides-alongside-a-turn.md)), so the
  interaction stands as a known limit rather than a defect.
- **Decay and windows read raw sequences, so writing rows is not perfectly
  neutral.** The fold ignores a private row entirely, but the sequence it
  consumed shifts every later desk row, and `QuorumPolicy::window` and
  `salience::standing` both measure a raw sequence distance. A support inside a
  tight window can therefore fall outside it because an exchange happened. What
  bounds this in practice is that windows and half-lives are large relative to
  the rows written between two desk turns — the default window is 100 against
  episodes of about eleven desk turns — and the benchmark's silence controls
  measure the residual at `+0.0 [+0.0, +0.0]` at forty-five model calls an episode.
  Removing the caveat means measuring decay and windows in desk-visible rows
  rather than raw sequences, which is a change to the quorum and salience folds
  and needs its own decision. Stated rather than solved here.
- **A declining participant still costs its call, and the budget now says so.**
  Both halves of this were defects and both are fixed. `round_cap` counts rounds
  the host opened rather than rows the log happens to carry, so a room whose
  members decline every round exhausts its budget on schedule instead of never;
  and the benchmark's price column counts members *asked* rather than rows
  written, because a declined call is billed exactly like a productive one. At
  the default cap that is 45 calls an episode against the 20 rows an earlier
  version of this document reported — a price understated by more than half.
