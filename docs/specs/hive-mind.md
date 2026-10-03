# Hive mind

> **Current API note.** The optional character budget allocator and refutation, evidential, and delegation episode paths described below were retired by [ADR 0027](../adr/0027-retire-unused-adapters-and-optional-mechanisms.md). The pure directory fold remains available outside episode attention.


**Status:** Implemented; episode scheduling superseded by
[`concurrent-rounds.md`](concurrent-rounds.md) and
[ADR 0014](../adr/0014-a-round-authorizes-concurrent-turns.md).
**Owner:** tinyhivemind maintainers

The single-turn `HiveStep` sketch below records the original P8 design. The
current `Speak` variant carries a bounded round of turns and one `next_state`
to commit after all those turns have been appended.

## Problem

A message on a shared desk selects exactly one responder, that agent replies,
and the interaction ends. A room of agents therefore cannot deliberate: there is
no way to put two proposals side by side, accumulate support for them, register
a grounded objection, decide that the room has settled, or notice that it has
deadlocked. Hosts that want group deliberation today have to build the whole
protocol themselves, and the observed failure modes of doing so are well
documented — unbounded fan-out, restating a settled point forever, and never
recognising a termination condition.

## Goals

- Decide, from a transcript the caller already holds, which single participant
  should take the next turn in a bounded deliberation episode.
- Accumulate support for competing proposals, and let a grounded objection
  silence an advocate so a tie between two good options can break.
- Terminate for an explicit, auditable reason: converged, deadlocked, exhausted,
  or nobody wanted the floor.
- Restore first-round independence without concurrency.
- Add no port, no journal, no async, and no host obligation beyond P4–P7.

## Non-goals

- **Fan-out.** An episode is a sequence of single turns. See
  [`../adr/0002-hive-episodes-are-sequential.md`](../adr/0002-hive-episodes-are-sequential.md).
- **Making answers better.** Nothing here is shown to improve accuracy, and
  almost every positive multi-agent result in the literature is confounded by
  compute. This is a protocol for bounded deliberation, not a quality claim.
- **Persistence.** `EpisodeState` is returned, never stored; the caller commits
  it under its own serialization, as with `sharing::prepare_delta`.
- **Model calls.** Deciding who speaks is pure. Whether a host consults its
  `Selector` port first is the host's business.

## Proposed behavior

### Traces — the shared medium

Coordination is stigmergic: a message deposits a typed trace, and the traces are
the stimulus for the next turn. No agent addresses another.

```rust
pub enum TraceKind { Propose, Support, Object, Refute, Evidence, Question, Commit, Defer }
pub struct TopicId(String);

pub struct Trace {
    pub sequence: Sequence,
    pub author: SessionAuthor,
    pub kind: TraceKind,
    pub topic: Option<TopicId>,
    pub target: Option<Sequence>,
    pub cites: Vec<Sequence>,
    pub text: String,
    pub offset: usize,
}
```

`Trace::grounded()` is `!cites.is_empty()`. Grounds are the cited sequences
rather than a flag so that a decision can be audited back to the messages that
carried it, the way a reflection in a memory stream cites its source nodes.

`resolve(body, supplied, author, sequence)` either extracts traces from
authored text or, given a supplied list, *selects* from what extraction finds.
The supplied mode is deliberately **narrower** than `mention::resolve`: a
supplied mention is authoritative about existence and is revalidated against
live snapshots, whereas parsing a trace is fully determined by the body, so a
supplied entry can only name which extracted trace to keep, by
`(offset, kind)`. Anything else it claims — a different topic, target, citation
or text — is discarded in favour of what the body says, and a repeated offset
is rejected rather than selecting the same trace twice.

Authored spans and UTF-8 byte offsets are preserved. Fenced and indented code
blocks are masked, and so are inline code spans: a span opened on one line and
closed on a later one quotes every whole line between them, so a marker with
no backtick ahead of it on its own line can still sit inside quoted code — a
marker sharing a line with its backtick needs no masking to be rejected,
because that line does not start with the marker's own leading character. The
grammar is
line-leading markers of the eight kinds above, qualified by `#topic`, `>target`
and `^cite`. Qualifiers are read wherever they appear after the kind rather
than in a fixed order, and `^cite` may repeat, so a trace carries a list of
grounds. `!refute` requires both a `#topic` and a `^cite`, and `!defer`
requires a `#topic`; either yields no trace at all without them. A body with no
marker yields no trace; ordinary conversation is not silently coerced into a
vote.

The full productions, the lexical rules, and the worked-examples table are in
[`grammar-traces.md`](grammar-traces.md); this specification does not restate
them.

`read(messages)` folds a projected transcript into traces in sequence order.

### Salience

```rust
pub struct SalienceWeights { pub recency: u16, pub importance: u16, pub relevance: u16, pub half_life: u32 }
pub struct Salience(pub i64);
pub fn salience(trace: &Trace, at: Sequence, weights: &SalienceWeights, relevance: u8) -> Salience;
```

Recency decays by rank in sequence, not by elapsed time — the transcript has no
clock, and rank is what the validated implementation of this score actually
used. `SalienceWeights::DEFAULT` is `recency: 5, importance: 30, relevance: 20`
in tenths, with `half_life: 20`, matching the shipped weights of the design this
is taken from rather than its paper. Decay is mandatory: without it the first
participant to speak keeps the floor forever.

All arithmetic is fixed-point integer, so results are `Eq` and reproducible.

### Quorum and cross-inhibition

```rust
pub struct QuorumPolicy { pub threshold: u32, pub window: u32, pub require_grounded: bool }
pub struct TopicStanding { pub topic: TopicId, pub supporters: Vec<String>, pub silenced: Vec<String>, pub support: i64 }
pub enum ConsensusState { Deliberating, Quorum { topic: TopicId }, Deadlocked { topics: Vec<TopicId> } }
```

Quorum is a **local, decaying count of distinct participants** — `threshold`
distinct supporters within the last `window` sequences — not a global majority.
It is order-independent and idempotent, so a participant that catches up late
folds to the same standing as one that watched live.

Cross-inhibition **targets the advocate, not the option**. An `Object` at
sequence *s* naming a target message removes that message's author from the
supporter set of the topic they were advocating, and records them in `silenced`.
Subtracting from a score cannot break a tie between two equally supported
options; silencing an advocate can, which is the entire reason the mechanism is
shaped this way.

When `require_grounded` is set, a `Support` with no `cites` contributes to
neither `supporters` nor `support`. A conclusion offered without grounds is what
lets an information cascade form, so it is second-class by construction.

`consensus` reports `Quorum` when exactly one topic is at or above threshold,
`Deadlocked` when two or more are, and `Deliberating` otherwise.

### Attention

```rust
pub struct AgentThreshold { pub agent_id: String, pub threshold: i64, pub affinity: Vec<(TopicId, u8)> }
pub enum BidReason { Addressed, Dissent, Knows, Quiet, Salience }
pub struct Bid { pub agent_id: String, pub urge: i64, pub reason: BidReason }
```

Every active desk member bids; `floor_holder` takes one highest bid and
`floor_round` takes a bounded set of bids in desk order. The host controls the
width with `round_width` while blind and `revealed_width` after reveal.

Two adjustments fold into the bid:

- **Dominance.** Speaker share is measured over the window in *grounded,
  supported* contributions, never raw message count, because raw count is a
  proxy an agent can trivially inflate. A member holding more than
  `dominance_cap` percent of that share is damped; the member with the smallest
  share is lifted, and bids with `BidReason::Quiet`.
- **Repetition.** Once `repetition_cap` distinct peers have acknowledged a
  point, restating it scores zero.

A member whose urge does not clear its threshold does not bid. Speaking raises
the speaker's threshold; a silent round lowers it.

### The context budget

The market above decides who speaks. The same module decides how much of what
a turn already holds fits in front of them, because a pinboard, a thread index,
a digest and a set of host notes all want room in one bounded prompt and
together they want more than there is.

```rust
pub struct BudgetRequest { pub source_id: String, pub wanted: usize }
pub struct BudgetPolicy { pub total_chars: usize, pub min_useful_chars: usize }
pub enum BudgetVerdict { Whole, Truncated, Dropped }
pub struct BudgetShare {
    pub source_id: String,
    pub granted: usize,
    pub omitted: usize,
    pub verdict: BudgetVerdict,
}

pub fn allocate_chars(requests: &[BudgetRequest], policy: &BudgetPolicy) -> Vec<BudgetShare>;
```

`allocate_chars` is **max-min fairness**, the standard allocation: each source
is offered an equal share, a source wanting less than its share takes what it
needs and releases the remainder, and the surplus is redistributed until it is
exhausted. It is computed as its fixed point rather than by iterating the
hand-out — one level `L` such that each source is granted `min(wanted, L)` and
`L` is the largest level that fits `total_chars`, found by binary search on a
monotone predicate. A small source is therefore never squeezed by a large one,
and a large one never takes more than an equal share of what the small ones
leave.

Fairness alone still yields rubbish, so there is a second rule. A source cut
below `min_useful_chars` is a fragment that spends budget and teaches the
reader nothing; it is **dropped and marked** — `BudgetVerdict::Dropped`, with
`omitted` carrying every character withheld — rather than carried unreadable.
The floor applies only to a claim the budget had to *cut*: a source small
enough to arrive whole is whole, however short it is. The fold decides the
numbers and marks the outcome; cutting the text and rendering the mark are the
caller's, exactly as nothing else in this crate produces prose.

`BudgetPolicy::DEFAULT` is derived from the stated per-message budget in
[`recall.md`](recall.md) rather than invented: `total_chars` is one full window
written at `BrevityPolicy::message_chars` (18,000) and `min_useful_chars` is a
third of one such message (200).

Every share is a function of the *set* of requests, the budget and the floor,
never of the position a request occupied: reordering the requests permutes the
result identically and changes no number. Two places would have broken that,
and both are settled by refusing a positional tie-break.

- **The remainder.** `total_chars` rarely divides evenly. What is left over
  stays unspent, because handing it to somebody means choosing whom, and
  nothing but position distinguishes equal claimants.
- **Who yields.** When the budget cannot usefully serve everyone, sources are
  dropped one at a time, greediest first, with the level recomputed after each
  — so a room of `n` equal claims loses claims one by one rather than losing
  all context the moment `n` grows too large. Equal claims tie on `wanted` and
  the tie breaks on `source_id`. Two requests identical in *both* are
  interchangeable, and which of them is dropped then follows request order;
  that is the one place the fold is not order-independent, and it is stated
  here because it cannot be removed without inventing a rank the caller did
  not supply.

Arithmetic is integer and saturating throughout, so a source asking for
`usize::MAX` is cut like any other rather than wrapping the running sum.

### The episode

```rust
pub enum Phase { Deliberate, Commit }
pub enum Visibility { Blind, Full }

pub struct EpisodePolicy {
    pub turn_budget: u32,
    pub blind_round: bool,
    pub dominance_cap: u32,
    pub repetition_cap: u32,
    pub quorum: QuorumPolicy,
    pub weights: SalienceWeights,
}

pub struct EpisodeState {
    pub conversation: Conversation,
    pub spent: u32,
    pub phase: Phase,
    pub thresholds: Vec<AgentThreshold>,
    pub watermark: Sequence,
    pub commit_boundary: Option<Sequence>,
}

pub struct HiveTurn {
    pub agent_id: String,
    pub phase: Phase,
    pub visibility: Visibility,
    pub reason: BidReason,
    pub next_state: EpisodeState,
}

pub enum HiveStep {
    Speak { turn: Box<HiveTurn> },
    Converged { topic: TopicId, standing: Box<TopicStanding> },
    Deadlocked { topics: Vec<TopicId> },
    Exhausted { spent: u32 },
    Idle,
}

pub fn step(state, transcript, roster, desks, policy) -> Result<HiveStep>;
pub fn project_for<'a>(turn: &HiveTurn, messages: &'a [SessionMessage]) -> Vec<&'a SessionMessage>;
```

`step` evaluates in a fixed order: validate the roster and desks; return
`Exhausted` if the budget is spent; fold traces and standings; return
`Converged` if consensus is `Quorum`, the phase is already `Commit`, **and a
`Commit` trace names that topic at a sequence strictly after
`commit_boundary`**; flip `Deliberate` to `Commit`, fix `commit_boundary` to
the sequence standings were just folded to, and emit one commit turn if
consensus is `Quorum` and the phase is `Deliberate`; return `Deadlocked` if
consensus is `Deadlocked` and **every member already supports one of the tied
topics**; otherwise take bids and either `Speak` or return `Idle`.

Two details of that order are load-bearing:

- **Convergence requires the decision to have been recorded**, not merely to
  have been reachable. A room that reaches quorum takes one commit turn, and
  only a `!commit` naming the carried topic, at a sequence after
  `commit_boundary`, ends the episode. `commit_boundary` is what stops an
  unrelated `!commit` that predates the commit turn being authorized — one
  planted speculatively, or left from an earlier exchange, that happens to
  share the carried topic — from being misread as evidence that *this* turn
  recorded a decision. If the committing participant never records it, the
  episode runs on and terminates at its budget instead — bounded either way,
  and never silently converged on a decision nobody wrote down.
- **A free member is identified from the standings, not from a bid's reason.**
  Bid precedence classifies a member that has also been cited or objected to as
  `Addressed` ahead of `Dissent`, so reading the reason would mask a real
  dissenter and end a breakable deadlock early.

Only messages authored by a **current, active member of the episode's desk**
are folded into traces. A retired agent, or one belonging to another desk,
whose message lands above the watermark is context: visible, but unable to
manufacture a quorum nobody eligible actually holds.

The `Deliberate` to `Commit` transition is one-way. Deliberation and commitment
are different classes of turn, and a room that has settled does not reopen
because a late trace arrives.

`Visibility::Blind` applies while `blind_round` is set and no participant has
spoken twice. `project_for` then hides messages authored by other agents
**within the episode** — that is, above the watermark — keeping operator,
person and system messages, the participant's own work, and the whole
conversation that led into the episode. The round withholds the positions peers
have taken since the room opened, not the context the room was opened about.

`next_state` is returned rather than applied. The caller commits it only after
its turn is durably appended, the discipline `prepare_delta` already
establishes.

## Invariants and constraints

- The original P8 `HiveStep::Speak` carried one turn. ADR 0014 replaced it with
  a bounded round; the host commits `next_state` after appending every turn.
- `standings` is commutative and idempotent over its trace input.
- No floating point anywhere in the crate.
- The crate names no host type, defines no trait a host implements, and performs
  no IO. `.github/scripts/assert-pure.sh` asserts it.
- An episode terminates: `spent` strictly increases on every `Speak`, and
  `turn_budget` is finite.
- `allocate_chars` never grants more than `total_chars` in total, never grants
  a source more than it asked for, and never carries a truncated source below
  `min_useful_chars`.
- A trace never resolves against a retired or inactive member.

## Acceptance criteria

- A three-agent episode reaches quorum, flips once to `Commit`, and reports
  `Converged` with the standing that carried it.
- Two equally supported topics report `Deadlocked`; the same input with one
  grounded objection reports `Converged`. Both directions are asserted, so the
  mechanism is shown to be load-bearing rather than incidental.
- An ungrounded `Support` moves neither `supporters` nor `support` under
  `require_grounded`.
- A spent budget returns `Exhausted` and no turn.
- A dominant speaker is damped and the quietest member bids `Quiet`.
- `project_for` hides peer messages under `Blind` and reveals them under `Full`.
- A point restated past `repetition_cap` scores zero.
- `standings` over a shuffled trace list equals `standings` over the ordered one.
- `allocate_chars` over a rotated or reversed request slice equals the same
  allocation permuted the same way, over rotations, a reversal, and a
  deterministic sweep of arbitrary claims.
- A source released by a small claim reaches the large ones rather than being
  wasted, and a claim the budget cannot usefully serve is reported `Dropped`
  with the whole of `wanted` in `omitted`.
- A quorum with no recorded `Commit` trace does not converge; it runs to its
  budget instead.
- A trace authored by a retired agent, or by one who is not a member of the
  episode's desk, moves no standing.
- Wire forms are pinned for every payload type, as elsewhere in the workspace.
- A gated live run shows real models emitting parseable traces and an episode
  terminating inside its budget. It asserts structure and attribution, never
  answer quality.

## Open questions

None. Roster composition — whose failure modes actually differ — is outside this
algebra and remains the host's responsibility.
