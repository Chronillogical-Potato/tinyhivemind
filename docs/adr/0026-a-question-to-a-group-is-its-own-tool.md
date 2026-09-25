# 26. A question to a group is its own tool, and the group is one conversation

- **Status:** Proposed
- **Date:** 2026-09-25
- **Amends:** [ADR 0023](0023-an-ask-opens-a-child-conversation.md); relates to [ADR 0021](0021-an-assignment-is-appended-rather-than-overwritten.md), [ADR 0024](0024-a-broadcast-completes-its-author.md)

## Context

[ADR 0023](0023-an-ask-opens-a-child-conversation.md) gave `ask` a child
conversation and fixed its shape at one asker and one seat asked: "one
question, one answer". `Utterance::Ask` carried `to: String`, the tool refused
a second name with `OneRecipient`, and the tool's own description told a seat
to "ask everyone you need in one turn; each ask is its own conversation".

That shape answers a question only one seat can settle. It answers badly the
question that made the ask worth having: *does this hold for all of you?* A
seat that needs two teammates to agree asks them separately, reads two answers
written without sight of each other, and is then the only party in a position
to notice they disagree -- with nothing to do about it but ask again, one at a
time. The pairwise shape also pushes a seat toward repeating itself: the
refusal in `crates/tinyhivemind-tools/src/tools/` exists because a hosted desk
was observed re-issuing one question verbatim, opening three conversations
with the same seat, each of which then had to be concluded.

The mechanics never needed the restriction. The ledger already keys open asks
by `asker -> {seat: row}`, a map; a child episode is
`CompletionEpisodeState::opened(conversation, root, participants)`, and
`participants` was a slice of one only because the caller passed one.

## Decision

`ask` keeps its shape -- one seat, one conversation -- and a second tool,
**`ask_teammates`**, takes two or more. Both produce the same
`Utterance::Ask { to: Vec<String> }`; below the tools a conversation of one
and a conversation of four differ only in how many seats are in it. A group
opens **one** conversation holding all of them -- not one conversation each:

- the child episode's participants are every seat asked, so it is quiescent,
  and the conversation over, once **every one of them** has concluded;
- every seat in it reads every row in it: the ask row reaches all of them
  (`Commit::only_for` is now a list), and the journal's audience rule for a
  thread is the ask's author plus everyone the ask named;
- a seat asked is briefed as one of a group, told the others were asked the
  same question and that it can read their answers, and asked for its own part
  rather than a restatement;
- each seat asked concludes with `complete_episode`, and its message is its
  part of the answer. The asker's hold is released seat by seat, as each
  conclusion reaches it, and the asker may complete only once the last has;
- the nudge for a silent seat is addressed to that seat, not to the thread: a
  seat that has already answered is not told again that it has not;
- the conversation's turn wall is per seat asked, since it was sized for one
  seat answering one question.

The refusals are unchanged in kind and plural in reach: the ask is refused
whole if it names the caller, an unknown seat, or a seat the caller is already
waiting on. Each tool also refuses the other's arity and names it -- a seat
that puts two ids in `ask` wanted a room, and a seat that puts one in
`ask_teammates` wanted a pair -- because a refusal that only says "no" leaves
a model guessing, and one live run showed it guessing a collective noun
(`to: "teammates"`) when the shape was not in front of it.

### Why two tools rather than one that takes a list

One tool taking one-or-many is the smaller vocabulary, and it was built that
way first. Two things argued it back apart:

- **A host can decline a tool.** `EpisodeTools::withhold` keeps a tool off the
  belt, out of the rendered definitions and out of the contract a host builds
  from `EpisodeTools::specs`. A host whose own model of a conversation is a
  *pair* of seats -- OpenCompany stores one as `dm:<a>+<b>` and derives a
  row's audience by parsing that key -- cannot file an N-seat conversation,
  and can now say so rather than filing it as something it is not. An arity
  inside one tool cannot be declined.
- **A named tool is a discoverable affordance.** The live run above reached
  for a group form before it had one; a tool called `ask_teammates` is what it
  was reaching for.

What it costs: two entries in the vocabulary that differ only in how many
seats they name, and a refusal on each pointing at the other.

`ask` remains unavailable **inside** a conversation, for the reason ADR 0023
gave: the seats asked answer, and a seat that needs someone else says so in its
answer. What changes is that the asker now has a way to put that someone else
in the room in the first place.

Jev is not involved, at either size. An ask names its destinations, and a known
destination bypasses the router -- the same rule
`crates/tinyhivemind-embed/src/routing/` applies to every addressed message.
Routing is for a `broadcast`, where the destination is the question.

## Consequences

The driver, the ledger and the wake predicate are unchanged in shape: the fold
opens one ledger entry per seat named instead of one, and the conductor holds a
set of seats per conversation instead of a pair. The wire changes in four
places -- `Utterance::Ask::to`, `CommittedUtterance::asks`, `Commit::only_for`,
and `Event::Asked`/`Event::Concluded`, which now carry `askees` -- and a host
that renders a conversation writes a list of names where it wrote one.

Those four are compile-time changes for every host, whether or not it serves
`ask_teammates`. What withholding the tool buys is that no *group conversation
can occur*, so a host with a pairwise model stays correct while it catches up:
it takes the pointer, fixes the four, withholds the tool, and adopts the group
when its own conversation identity no longer assumes two seats.

A group ask costs a turn per seat asked before it can conclude, and the asker
waits for the slowest of them. That is the price of the agreement it asked
for; a seat that does not need agreement should still ask separately, and the
tool's description says so.

## Reversal

If group asks are found to stall on one slow seat, the narrower fix is to
release the asker on a quorum of the group rather than all of it -- the ledger
already records which seats have answered. Reverting to ADR 0023's pairwise
shape means dropping `ask_teammates` from the vocabulary; nothing below the
tools depends on the group being a group, and `ask` is untouched by this
record.
