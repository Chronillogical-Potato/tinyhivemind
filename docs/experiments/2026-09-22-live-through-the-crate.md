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

## What this changes

The loop, the crate, the driver and the ledger held under a real model and a
real router; the two defects were in what the host owed the seats, not in
what the seats owed the episode. The content failure is a prompt failure with
a known cause. The next run is the one that tests whether the seats, told who
they are on every turn, pool what they hold.
