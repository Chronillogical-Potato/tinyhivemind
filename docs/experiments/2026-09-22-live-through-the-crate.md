# Live, through the crate

**Date:** 2026-09-22
**Status:** Recorded
**Code:** `cargo run --manifest-path examples/openhuman/Cargo.toml --bin conducted` over
OpenRouter with live Jev routing (`jev-1.13.0`)
**Decisions:** [ADR 0022](../adr/0022-the-episode-mcp-server-is-the-one-socket.md),
[ADR 0021](../adr/0021-an-assignment-is-appended-rather-than-overwritten.md)

The first live episode run through what the workspace now ships: the room's
tools served by `tinyhivemind-mcp`, the loop stepped through
`CompletionDriver` with its ledger, a five-seat hidden-profile desk in which
no seat can diagnose alone. One run. It was not written up as a prediction
first; the question was whether the mechanics hold and whether `broadcast`
fires, and both halves of the answer are below.

## What the mechanics did

`12` turns, `7` waves, `2` routes, `4` settled. The door route picked `lead`
alone from five. Three settled seats -- `theory`, `researcher`, `solver` --
were woken to answer questions asked of them, and did. **`broadcast` fired
live for the first time** (row 11), and routing placed it with nobody: the
message asked for a git diff, no seat on the desk fits that, and a plan that
names no seat is the right answer to it. The driver's rule for an unplaceable
broadcast kept the author owed a turn; nothing stalled.

Two things it found, both mechanical and both fixed:

**A completion from a settled seat aborted the episode.** `checker`, woken to
answer `lead`, answered and then called `complete_episode` -- as a seat that
has finished tends to. It held no open assignment, the fold refused it, and
the host treated the refusal as fatal. The same case was found and fixed in
the prototype loop and had not been carried into the driver. A settled seat
saying it is done is already true: the row is recorded and nothing moves.

**An unplaced broadcast was invisible.** The host printed nothing and told the
author nothing, so `lead` posted "still open" and "waiting for someone to pick
it up" (rows 14, 16) for a pickup that could never come. The author is now
told on the desk that nobody can take it and the work stays with it.

## What the seats did

**Not one private fact was stated in twelve turns.** The desk was built so
that the answer exists only in the union of what five seats separately know
-- a changed hashing library, a migration that never ran, an age split in the
failing accounts, an unreadable hash prefix. Every one of those sentences was
in a seat's standing prompt. None appeared on the desk. Instead every seat
asked every other seat for the diff of a codebase that does not exist, and
told each other in turn that it had no git access.

That is the behaviour of agents that do not know what they know. The briefs
were in the `AgentSpec` system prompt and nowhere else, and an earlier run had
already established that a seat asked to quote its private section on a fresh
session replied that it had none. The brief now travels in every turn's own
prompt, where the seat is certain to read it.

**The one substantive answer went where nobody could read it.** `checker`'s
post said it could "narrow the search from first principles"; the narrowing
itself was in its reply prose, which reaches nobody, and its completion
message described the analysis rather than containing it.

**Seven of `lead`'s turns were status.** "Waiting on both answers", "still
waiting", "broadcast is still open". Each is a model call and a row that tells
the desk nothing. The `post` description of the time invited it -- "call this
exactly once, at the end of your turn" -- and has since been rewritten around
stating a fact. The protocol now also says: if you are waiting and nothing new
bears on your work, end the turn without calling anything.

**An answer was counted that was not one.** `theory`, asked by `lead`, asked
`researcher` something of its own and then posted "I'm on it"; both rows
cleared `lead`'s question and woke it. A question or a handoff from the asked
seat is now not its answer; a post, a private message, or its completion is.

## The second run

Same desk, the brief in every turn's prompt. `7` turns, `3` waves, `3` routes.

**The facts came out.** `theory` stated that 0.9 replaced the hashing library
(row 5). `researcher` stated the different prefix and the unreadable old
hashes, and drew the mechanism: every verify call fails on every stored
credential (row 9). `checker` used its age split to design exactly the right
test -- a versioned-credential test with a pre-0.9 fixture, because a test that
creates a user and logs in at once "would never catch this" (row 7). `solver`
stated the migration and gave the real fix, rehash-on-verify (row 16).

**`lead` pooled three of the four and diagnosed correctly** (row 11), then
**broadcast twice and both were placed**: the fix to `solver`, the test to
`checker`. The right seat each time. It is the first live episode in which a
broadcast fired and landed. `lead` concluded one piece early -- it never asked
`solver`, and got the migration only after it had completed -- which the task's
"ask before you conclude" was meant to prevent and did not.

**Then the host aborted, one wave short.** `solver`, woken in wave three to
answer `checker`, was assigned the fix by `lead`'s broadcast *in the same
wave*, while its turn was already running. Its completion landed on work it
had never been shown; the delivery guard refused it, correctly; and the host
treated the refusal as fatal. Had it continued, `solver` and `checker` each
held one assignment and would have run once more. Two changes: the host now
tells the seat it was handed work while speaking and carries on, and a row
from a seat that has not been shown its assignment no longer counts as having
run for it, so it stays owed the turn.

One row of noise: `solver` posted the single word "test" (row 15) -- a model
trying the tool. Cheap, and worth nothing.

## The third run

Same desk. `15` turns, `8` waves, `8` routes, `11` assignments settled, `0`
discharged, and **the episode reached quiescence on its own**: every seat
settled, nothing queued, nothing awaited, well under the forty-turn wall.

**Every ledger rule fired under a real model.** `lead` asked all three of
`theory`, `solver` and `researcher` this time, waited one turn without calling
anything (the new protocol line, working), and diagnosed from all three (row
11) -- including `solver`'s migration, which run two had reached only after the
fact. Its two broadcasts were placed with the right seats. `solver`'s fix went
to `checker` for review while `checker` was busy: **the handoff was queued and
handed over at `checker`'s completion** (rows 26, 31), twice. `checker` tried to
complete while still waiting on `researcher` and **was refused and told so**
(row 22), `researcher` answered (23), and `checker` completed (25). One of
`checker`'s broadcasts -- its finished test -- fit no seat, and **it was told
the work stays with it** (row 19) rather than left waiting.

**The chain converged.** `solver → checker → solver → checker → theory`: a fix,
an attack on the fix, a corrected fix, a second attack, and a structural note
from `theory` on the corrected fix's compatibility assumption. Each handoff was
a fresh assignment with a fresh budget, so the per-assignment cap could not
have bounded it; what bounded it was that the seats ran out of things to say
-- `checker`'s last row is "No new information" (32). The pathological chain
the bounds exist for did not occur, and this is the first run in which it
could have.

**The content is the best of the three.** The desk produced a root cause with
both halves (library swap, unrun migration), a fix with rehash-on-success, a
three-case regression test (old hash accepted, wrong password on an old hash
rejected, rehash on success), and two rounds of adversarial review that found
a real defect in the first fix -- `update_stored_hash(user_id, ...)` inside a
function that has no `user_id` -- and a second in the correction, a
`ValueError` swallowed to `False`. The corrected fix returns
`(verified, was_old_format)` and moves the rehash to the login handler, which
is the right shape.

**Routing was sensible every time.** Attacks on a fix went to the seat that
wrote it; a fix went to the verifier; a compatibility question went to the
structure specialist. Eight routes, one unplaced, and that one correctly.

## The fourth run: conversations

Same desk, with [ADR 0023](../adr/0023-an-ask-opens-a-child-conversation.md)
-- an ask opens a conversation on a thread of the desk -- and the
`EpisodeBrief` seam feeding every turn. `12` turns, `7` waves, `3`
conversations, and quiescence. **And `routes 1`: no broadcast fired.** The
root cause was found and neither deliverable was produced.

**The conversations worked.** `lead` asked `researcher` and `theory`; each ask
opened a thread; each was answered in it; `lead` followed up in each -- 378
and 829 characters, a real exchange rather than the one-shot reply of the
earlier runs -- and both concluded and cross-posted. `lead`'s desk turn while
waiting called nothing. A thread with nothing left to say was closed in one
turn (row 20).

**Three things cost the outcome, two of them the host's.**

*A settled seat that was asked was still woken on the desk.* The flat design's
rule -- a seat owing an answer is owed a desk turn -- survived into the driver
beside the conversations that replaced it. `theory` spent its desk turn on a
generic six-item list (row 10); `researcher` spent its opening a pointless
thread back to `lead` (row 12). Removed: the conversation is where a seat
answers.

*A broadcast inside a conversation was refused.* `lead` found the handoff work
while talking to `theory`, tried to broadcast it there (row 17), and was told
it could not. On its next desk turn it *described* the broadcast in prose (row
25) rather than making it, and the episode ended with `solver` and `checker`
never having run. A handoff found in a conversation is desk work; the host now
commits it to the desk. Only `ask` stays barred inside a thread.

*The model claimed a call it did not make.* Row 25 says "broadcast both work
items"; the log shows no broadcast. That is the model's, and it was put in
that position by the refusal above.

The seam held: every prompt in this run was the host's brief followed by
`EpisodeBrief::render()`, and the standing contract was the tool specs.

## The fifth run: both at once

Same desk, with the desk-wake removed and in-thread broadcasts going to the
desk. `16` turns, `13` waves, `2` routes, `4` conversations, and quiescence.

**The two halves met.** `lead` asked all three specialists this time, each ask
opened a thread, and `theory` and `researcher` answered in theirs with their
facts (rows 5-9) -- `theory`'s is the fullest structural analysis any run
produced. `lead` closed both, diagnosed, **broadcast, and it landed on
`solver`**; `solver` stated the migration on the desk (row 20), the piece
every earlier run reached late or not at all, and opened a thread back to
`lead` to ask for code, which `lead` answered honestly (row 22). Handoff chain
and conversations, in one episode.

**Two defects, both the shape of run four's.**

*A reply without a tool call stranded a thread.* `solver`'s first thread turn
was 221 characters of prose and no call; nothing was recorded, so it was not
owed another turn, `lead` was held out of a thread the askee had not spoken
in, and the host closed it empty (row 16). The seat asked is now told once
that its reply reached nobody and is owed one more turn -- a host call,
`owe_turn`, since the seat has by then run and been shown everything.

*A refused `ask` inside a thread produced a phantom one.* `solver` tried to
ask `researcher` from inside its thread with `lead`, was refused (row 25),
then said it had asked (rows 26-27) and completed "waiting" on an answer that
could not come (row 31). The second time a refusal inside a thread has made a
seat claim the call. There are now no refusals inside a conversation: an `ask`
made there opens a new conversation on the desk, as a broadcast hands off
there. ADR 0023 is amended to say so.

*And one bundle.* `lead`'s broadcast carried both deliverables in one message
(row 17), so routing placed both with `solver` and `checker` never ran. The
`broadcast` description now says one piece of work per call.

The root cause was complete for the first time -- library swap *and* the
migration that never ran -- and neither deliverable was produced.

## The sixth run: no `post`

Same desk. The episode server serves `broadcast`, `ask`, `complete_episode`
and `read`; a conversation is one question and one answer, the seat asked its
only participant. `31` turns, `22` waves, `4` routes, `10` conversations, and
quiescence.

**Both deliverables, and all four facts.** With no way to say a thing without
a consequence, every fact arrived as a completion: `theory`'s structural
picture, `solver`'s migration -- on its first turn, for the first time --
`researcher`'s prefix and changelog, and `checker`'s age split, which no
earlier run had surfaced at all. `lead`'s broadcast (row 8) carried the whole
root cause, both halves, *and* the dual-verify fallback as code; it landed on
`checker`, who delivered a six-case regression matrix as its completion (row
36) and re-broadcast the fix to `solver` (row 33). Ten conversations, ten
answers. Three broadcasts: two placed, one -- a finished test, sent as if it
were work -- correctly unplaced and kept.

**What it cost, and why.**

*Seven serial asks, hunting for a repo.* `checker` asked one seat per turn,
waiting each time, and from the third ask on it was asking for shell output
and file listings that do not exist (rows 21-30). "You have no codebase,
shell or filesystem" was in the system prompt only -- the same place the
briefs were in run one, and read the same amount. It now travels in every
turn. And `ask` now says: ask everyone you need in one turn.

*Two seats answered every question with their one fact.* `solver` gave the
same migration sentence to four different questions and said so itself at
the end (row 37); `researcher` gave the same prefix sentence three times. A
fresh thread prompt with the brief on top invites restating it. Recorded,
not patched: it is the model reaching for what it holds, and the prompt
change above removes the questions that provoked it.

*A thread was force-closed under an askee that had asked onward.* `lead`,
asked by `checker`, needed `solver` first and asked -- correctly, on the desk
-- but was never turned back to `checker`'s thread when the answer came, and
the host closed it empty (row 20). The host now records which thread an ask
was made from, re-owes that thread when the sub-answer concludes, and shows
the answer in that turn.

## The seventh run: a broadcast completes its author

Same desk, under [ADR 0024](../adr/0024-a-broadcast-completes-its-author.md):
a placed broadcast completes the seat that made it; `ask` is refused inside a
conversation, at the server; a silent askee and a stalled desk seat are each
told once. `14` turns, `9` waves, `3` routes, `3` conversations, and no
quiescence: the host gave up with `solver` and `checker` holding open work.

**The desk half worked.** `lead` asked `theory` and `researcher` in one turn
(rows 2-3), as `ask` now tells it to, and both answered on their first turn
in the thread. `theory` needed the one nudge (row 5) and then answered.
`lead` asked `researcher` again for a diff that does not exist (row 9), got
the same answer, and then did in one turn what took two in every earlier
run: broadcast the fix, broadcast the test, and complete (rows 12-14). Both
broadcasts placed; the rule that a broadcast completes its author was never
exercised because `lead` completed in the same turn.

**The handoff half did not.** `solver` and `checker` each received a
broadcast on the desk, each wrote a long reply -- `1321` and `4162`
characters -- and neither called a tool. The desk nudge (rows 15-16) drew a
short reply from each, again with no tool call, and the host stalled. Their
deliverables were typed rather than recorded, and the log kept only their
length.

**Why.** The thread render tells an askee exactly what to do: answer with
`complete_episode`. The desk render told a seat holding a handoff only that
its assignment was made at a sequence, and the nudge said "say what you are
waiting on" -- an invitation to prose. The standing contract's "prose alone
changes nothing" was in the system prompt, and system-prompt-only
instructions have lost twice in this series already. Three changes: the desk
render, when the seat holds an assignment, now says to record the part with
`complete_episode`, hand the rest on with `broadcast`, and that a reply
without a tool call records nothing; the nudge names the same two tools; and
the host prints the reply of any turn that recorded nothing, because a
refusal the seat read and a deliverable it typed look identical in the log
otherwise.

## What this changes

Across three runs every defect was in what the host owed the seats, not in
what the seats owed the episode, and each was one step from done. The brief in
the turn prompt turned a desk that hunted for a diff into one that pooled four
private facts; the mid-turn fix let it finish. The third run is the first live
completion-driven episode to reach quiescence through the crate and the
driver, and it did so while exercising the queue, the open-ask hold, the
unplaced-broadcast notice, and a five-hop handoff chain that converged on its
own. What is still unmeasured is the chain that does not converge: the budget
and the discharge never engaged, here or in the benchmark.
