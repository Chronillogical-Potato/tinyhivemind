# 23. An ask opens a child conversation, a thread of the desk

- **Status:** Proposed
- **Date:** 2026-09-22
- **Amends:** decision D22 of `docs/notes/completion-episode-review.md`; relates to [ADR 0021](0021-an-assignment-is-appended-rather-than-overwritten.md), [ADR 0010](0010-an-aside-carries-information-never-support.md)

## Context

The review that shaped completion-driven episodes recorded, as D22, that a
child episode exists only for a counterpart *outside* the parent's membership,
and that a question to a seat *inside* it is "a delivery into that agent's
existing session, and its reply is a desk row." Its reason was that seating
the requester in two episodes gave it "two uncoordinated
`(assigned_at, completed_at)` pairs whose `status()` results disagree."

That reason no longer holds. [ADR 0021](0021-an-assignment-is-appended-rather-than-overwritten.md)
made assignment records per-episode and append-only: a seat pending in a
parent and pending in a child holds two clean records in two states, not one
overwritten slot. And the delivery version has a cost three live runs showed:
one question, one public reply, no follow-up. A verifier that needed
specifics from a researcher got one shot at them.

The original intent, before D22 narrowed it, was that an ask starts a
conversation between two seats which concludes on its own and then wakes the
asker with that conversation in its context. This record restores it.

## Decision

An `ask` opens a **child conversation**: a completion episode whose
conversation is the parent desk with `thread_root` at the ask row, whose
participants are the asker and the seat asked, run by the same driver to its
own quiescence. A turn in it is registered as that thread, and every tool call
in it names the thread as its `parent` -- which is what that argument on every
tool exists for.

The asker's hold in the parent (D21) is released by exactly one thing: the
conclusion of the conversation, **cross-posted by the host** as a private
message from the seat asked to the asker (D23). Nothing the asked seat says on
the open desk counts. That row is undelivered to the asker, so it is owed a
turn (D24), and the host assembles the whole conversation into that turn's
context -- the seat's shared context across every channel it is in.

`HostAction::DeliverDm` for an ask *is* the signal to open the child; there is
no second action. An `ask` or a `broadcast` made *inside* a conversation is
desk work -- it opens a new conversation on the desk, or hands work off there
-- because two live runs showed that refusing a call inside a thread makes the
seat claim it made the call anyway. Nothing a seat can call is refused inside
a conversation; a conversation has no nesting because every conversation is a
thread of the desk, keyed by the row that opened it.

## Consequences

The ledger, the wake predicate, the `chat`/`parent` check and the driver are
unchanged in shape; one rule moved -- what counts as an answer -- and one
action gained a meaning. The host gains a second driver state per open
conversation and the responsibility to cross-post and to bound it.

A seat may be pending in the parent and in a child at once. The host runs one
turn per seat per round, the child's first: a conversation is what unblocks a
parent, so it goes ahead of it.

## Reversal

If conversations are found to run away -- a pair of seats that never conclude
-- the bound is the host's turn wall on the child and the D21 timeout, both of
which conclude it with "no answer" and release the asker. If that is common,
the delivery version D22 described is the fallback, and this record is
superseded.
