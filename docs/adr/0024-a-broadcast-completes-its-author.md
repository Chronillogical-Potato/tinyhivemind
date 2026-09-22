# 24. A broadcast completes its author unless it is waiting

- **Status:** Proposed
- **Date:** 2026-09-22
- **Amends:** decision D13 of `docs/notes/completion-episode-review.md`; relates to [ADR 0019](0019-complete-episodes-with-explicit-agent-events.md), [ADR 0023](0023-an-ask-opens-a-child-conversation.md)

## Context

D13 held that a broadcast does not complete its author: it hands *that* work
off, and the author's own assignment stays open. The protocol therefore told a
seat to call `broadcast` and then `complete_episode`. In six live runs every
seat that broadcast completed immediately afterwards, as a second call
carrying no information; the one seat that did not, inside a conversation,
left the conversation stalled and its asker held.

A completion episode has no "still working" event. A turn that ends without
a completion means one of two things: the seat is waiting on a question it
asked, or it is stalled. So the second call was never telling the episode
anything the broadcast had not.

## Decision

A broadcast that is **placed or queued** completes its author, and the
broadcast message is its finding. A handoff queued for the author is handed
over at that completion, as at any other. Two exceptions:

- the author has a question open -- it is waiting, not done, and stays
  pending;
- the broadcast placed nobody -- the work has no owner but the author, which
  stays pending and is owed a turn, as before.

The broadcast budget is charged to the seat's most recent assignment, open or
just closed by its own handoff, so completing this way does not refill it.

## Consequences

One fewer call per handoff, and one fewer way to stall. A seat that hands off
work and has more of its own to do states that in the broadcast; if it has
questions open, it is still pending and finishes when they are answered. A
later broadcast still reopens a completed seat, as ADR 0021 provides.

## Reversal

If a host needs a seat to hand work off and keep working in the same turn --
a harness with a genuine "still working" event -- this rule is wrong for it,
and D13's two-call form returns. Nothing in six runs asked for that.
