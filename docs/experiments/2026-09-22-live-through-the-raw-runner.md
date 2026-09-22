# Live, through the raw runner

**Date:** 2026-09-22
**Status:** Recorded
**Code:** `TINYHIVEMIND_RUNNER=raw cargo run --manifest-path examples/openhuman/Cargo.toml --bin conducted`
over OpenRouter (`deepseek/deepseek-v4-flash`) with live Jev routing
**Decisions:** [ADR 0022](../adr/0022-the-episode-mcp-server-is-the-one-socket.md),
[ADR 0024](../adr/0024-a-placed-broadcast-completes-its-author.md)

The first live episode run with no MCP server and no `openhuman-embed`
agent: every seat a fresh `OpenHumanSessionHost` built one level down on
each turn, the room's tools on its belt as native `tinytools`, each of which
is `EpisodeTools::call` from `tinyhivemind-tools`. The ten runs in
[live, through the crate](2026-09-22-live-through-the-crate.md) were all the
embed runner over the wire. The question here was whether the same driver,
the same ledger and the same record hold when the wire is gone. One run, on
the `login` desk, and the answer is yes.

## What the mechanics did

`11` turns, `8` waves, `5` routes, `3` conversations, quiescent. The door
route picked `lead` alone from five. `lead` asked `researcher` and `solver`
in two conversations (threads 2 and 3); both concluded. `solver`'s first
broadcast fit no seat and the work stayed with it, said on the desk (row 7).
`lead` broadcast the root cause and was completed by it (row 10); `solver`
took it, broadcast the one-line fix and was completed by it (row 12);
`checker` took that, asked `solver` five precise questions in a third
conversation (thread 14), broadcast the regression test and was completed by
it (row 17); `theory` took the last handoff and completed with the invariants
any fix must satisfy (row 19). Both deliverables the operator asked for are
on the desk, from different seats, handed off by routing rather than by
name, which is what the desk was built to require.

Every refusal a seat earned was the record's, in the record's words. Twice
`researcher` called `read` inside thread 2 naming the wrong parent and was
told "this turn is in chat `engineering` with parent `2`; name exactly
those"; `checker` on a desk turn named a parent it did not have and was
refused `read` and `ask` the same way. An MCP seat in the embed runs saw the
same sentences from the same fold; here they arrived as the native tool's
error.

## What it found

**A raw session is the desktop unless told otherwise.** The run did not start.
`OpenHumanSessionHost` resolves its model through the core's provider
factory, and the factory asks the ambient `CoreContext` whose product policy
applies. The raw runner had installed none, and with none the core answers as
the desktop app: custom cloud inference waits on an operator signed in to
OpenHuman, and the first turn failed with `SESSION_EXPIRED: no backend
session`. The embed runtime never hit this because it boots the core as
`HostKind::Library` -- the caller owns the provider and its credential -- and
scopes every call in that context. The offline proofs never hit it because a
loopback endpoint counts as caller-owned inference and skips the gate, so the
bench and the scripted runs passed a session the live run could not.

The raw runner now does what the embed runtime does, and no more: at
seating it calls `CoreContext::init_with_config` with `HostKind::Library`,
no domain, no service and the run's own config, and every session is built
and run inside `CoreContext::scope` of that context. Nothing else changed;
the run above is the first attempt after that.

**The wire was never the cost.** With the runner fixed the episode ran to
quiescence on the first attempt with no fix to the driver, the ledger, the
record or the prompt. Ten embed runs had already shaken those out, and every
correction they earned carried over unchanged, because none of it lived in
the MCP crate. The record crate holding the fold and the socket crate holding
the framing is the split this run was the test of.
