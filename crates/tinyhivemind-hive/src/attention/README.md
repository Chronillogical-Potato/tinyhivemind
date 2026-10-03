# The attention market

Eligible members bid for a place in the next round. The attention score draws
on response thresholds and the room's traces. A policy width of one selects a
single floor holder; wider rounds may authorize several members.

The design and the tuning are specified in [`docs/specs/hive-mind.md`][spec]
under "The attention market"; this module is its implementation.

## Design

`bids(context)` scores every eligible member and returns one [`Bid`] per
member whose urge reaches its own threshold. An empty result means nobody has
anything to say. `floor_holder(bids)` takes the highest bid, with ties broken
by desk order. `floor_round(bids, width)` selects up to `width` highest bids,
resolves ties by desk order, and returns the selected bids in desk order. The
episode chooses the width from `round_width` or `revealed_width`, depending on
visibility.

The width limits how many members a step authorizes. With width one,
`floor_round` makes the same choice as `floor_holder`. See
[ADR 0014](../../../../docs/adr/0014-a-round-authorizes-concurrent-turns.md)
for the concurrent round contract.

Per member, the urge is [`salience::standing`] and [`salience::with_relevance`]
summed over every live, unsaturated trace, folded once per trace rather than
recomputed per member where the terms do not depend on the reader, then
adjusted by up to one bonus and one penalty:

- **[`BidReason::Addressed`].** A trace cited or objected to this member.
- **[`BidReason::Dissent`].** The room is deadlocked between two or more
  topics tied for the most support, and this member backs neither.
- **[`BidReason::Knows`].** A caller-folded [`Directory`] names this member as
  the holder of the contested topic, and it has not deposited a position on it
  yet. Off unless a directory is supplied; stops the instant its holder
  argues the topic, so it cannot compound within an episode. See
  [`mod@crate::directory`].
- **[`BidReason::Quiet`].** The equality guard: the least-heard member (by
  *grounded, surviving* contributions, not raw message count — a count an
  agent could inflate for free) gets a bonus when the room is lopsided.
- **[`DOMINANCE_PENALTY`].** A member holding more than `dominance_cap`
  percent of grounded share is damped, regardless of which bonus it also
  earned.

Ordering matters: the four bonuses are mutually exclusive per member (a
member that is both addressed and quiet bids `Addressed`), and are checked
`Addressed → Dissent → Knows → Quiet` — see [`BidReason`]'s rustdoc for why
that order is load-bearing, particularly `Dissent` over `Knows`: a routing
preference must never suppress the one member who could break a deadlock.

### Repetition is capped, not just scored

Once a topic has `repetition_cap` distinct supporters, a further `!support`
on it is dropped from scoring entirely (`is_saturated`) rather than merely
scored low — restating a settled point after enough peers already hold it is
one of the most common observed multi-agent failures, and this treats it as
the protocol bug it is rather than something the salience weights should
merely discourage.

### Delegation without becoming a router

`holds_uncited` is *delegation*, not amplification: the last clause — the
holder has taken no position on the topic yet — is what stops the bonus from
compounding, and *position* is checked rather than *any trace*, so a member
whose only deposit is a stated fact (exactly the member the mechanism exists
for, in a hidden-profile task) still qualifies for the bonus once.

## The `budget` submodule

[`crate::attention::budget`] is a separate concern under the same market
metaphor: it decides how much of what a turn already holds — a pinboard, a
thread index, a digest, host notes — fits in a bounded prompt, once the market
above has already decided who speaks. See its own
[`README.md`](budget/README.md).

## Public surface

| Item | Purpose |
| --- | --- |
| `bids` | Compute one bid per eligible member from a `BidContext`. |
| `floor_holder` | Take the single highest bid; ties break by desk order. |
| `floor_round` | Take up to the requested width of bids, in the same order. |
| `Bid` | `agent_id`, `urge` (net of threshold), `reason`. |
| `BidReason` | `Addressed`, `Dissent`, `Knows`, `Quiet`, `Salience` — in bid-precedence order. |
| `BidContext` | Everything the market reads, borrowed from the caller: traces, standings, members, thresholds, weights, caps, quorum policy, and an optional directory pair. |
| `AgentThreshold` | One member's floor-taking threshold plus its host-declared topical affinities. |
| `AgentThreshold::relevance` / `declared_relevance` | Neutral-defaulting vs. `Option`-returning affinity lookup — see their rustdoc for why the market and the directory prior need different defaults. |
| `allocate_chars`, `BudgetPolicy`, `BudgetRequest`, `BudgetShare`, `BudgetVerdict` | Re-exported from `budget`; see that submodule's README. |

## Operational constraints

- **Order-independent and idempotent.** `bids` sorts and deduplicates traces
  by `(sequence, offset)` before scoring, exactly as `standings` and
  `directory` do, so a redelivered or reordered trace cannot double a
  member's urge or move which topic the room is stuck on.
- **`BidContext::directory` and `directory_policy` are a matched pair.**
  `BidReason::Knows` is reachable only when both are `Some`; either alone
  leaves it unreachable, which is what makes the mechanism opt-in without a
  separate flag.
- **Fails on a duplicate threshold.** `bids` returns
  [`Error::DuplicateAgentThreshold`][dup] when two `AgentThreshold` records
  name the same agent, rather than picking one silently.
- **Fixed-point throughout**, sharing `salience`'s scale so a bonus, a
  penalty and a decayed score are directly comparable as `i64`.

[spec]: https://github.com/tinyhumansai/tinyhivemind/blob/main/docs/specs/hive-mind.md
[dup]: crate::error::Error::DuplicateAgentThreshold
