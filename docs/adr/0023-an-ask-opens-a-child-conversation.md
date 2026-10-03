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
participant is the seat asked -- the asker is recorded by the host, as D22
had it -- run by the same driver to its own quiescence. One question, one
answer: the seat asked concludes with `complete_episode`, whose message is
the answer; if the answer raises another question, the asker asks again, and
that is a new conversation. `post` is not served in an episode at all: a fact
reaches the desk as a completion's message, work reaches a seat as a
broadcast, a question reaches a seat as an ask, and nothing a seat can call is
text without a consequence. A turn in it is registered as that thread, and every tool call
in it names the thread as its `parent` -- which is what that argument on every
tool exists for.

The asker's hold in the parent (D21) is released by exactly one thing: the
conclusion of the conversation, **cross-posted by the host** as a private
message from the seat asked to the asker (D23). Nothing the asked seat says on
the open desk counts. That row is undelivered to the asker, so it is owed a
turn (D24), and the host assembles the whole conversation into that turn's
context -- the seat's shared context across every channel it is in.

`HostAction::DeliverDm` for an ask *is* the signal to open the child; there is
no second action. A `broadcast` made *inside* a conversation is desk work and
hands off there. An `ask` is not available inside one: the seat asked answers,
and if it needs another seat first it says so in its answer and the asker asks
them -- a conversation is one hop, and its asker is its only coordinator. The
refusal is made by the tool server in the tool result, while the seat can
still call again; two runs showed that a refusal arriving later as a row makes
a seat claim it made the call anyway.

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
