# Team initialization

A team briefing is ephemeral input to a model session, separate from durable
history. It identifies the viewer and desk, lists teammates deterministically,
and explains attribution and mention safety. Hosts can provide richer optional
role and description strings directly, or derive a conservative briefing from
validated core desk and roster snapshots.

The rendered rules are P7-aware, and they are shaped by what the run can
actually do. Person, desk, and `@everyone` mentions are always described as
context that never fans out. The `@agent` dispatch rule is *withheld* unless
the caller proves it is live: `system_text` states nothing about it, and
`system_text_with_dispatch` states it only when the supplied
`MentionDispatchContext` — the same `MentionDispatchPolicy` and hop the host
passes to `mention_dispatch` — reports `may_dispatch()`. A disabled policy, a
zero hop budget, a run at or past `max_hops`, and a caller that supplies no
context all render the same text: the one without the offer.

That is the withhold rule from `docs/specs/responders.md` applied to the one
surface a model reads. Advertising a capability to a run whose next call would
refuse it costs that run a turn to discover, so absent information fails
closed. The briefing still neither carries nor chooses the policy; the host
supplies a finite configurable `max_hops` for each dispatch decision, and the
library imposes no smaller hard ceiling.

`initialize_session` keeps this briefing separate from projected messages. It
therefore has no sequence number, consumes no history window, and is never
written back through this crate.

## Audience

`TeamBriefing::asides` gates the aside grammar in `system_text`. A grammar is a
fixed cost paid in every agent's prompt on every turn, so teaching a move
nobody may make spends that budget for nothing.

An enabled desk also gains a fourth shared-session rule, stated as something an
agent can act on rather than as a disclaimer: some rows show only that an aside
happened, you cannot read them, and if one matters you ask its author in the
desk. An agent that is not told its view may be narrower than a peer's reads
silence as disagreement rather than as absence.

`BrevityPolicy::window` states the window a viewer actually received when the
projection elided something, rather than the nominal one. Only a projection
that actually elided is restated, so a young desk still reports the window it
will grow into.

## Layout

- `mod.rs` — `TeamBriefing::from_snapshots`, `system_text`/
  `system_text_with_dispatch`, `SessionContext::system_text`, and
  `initialize_session`/`initialize_session_with_context`.
- `types.rs` — the wire records: `TeamBriefing`, `BriefedTeammate`,
  `BrevityPolicy`, `MentionDispatchContext`, `BriefingNote`, `SessionContext`,
  and `SessionInitialization`.
- `test/` — unit tests grouped by behavior area, with fixtures factored into
  `test/support.rs` rather than duplicated per file:
  - `wire.rs` — serde round-trips for every briefing payload type.
  - `snapshots.rs` — `from_snapshots` desk-order filtering, General's roster
    order, error propagation, and construction without host role types.
  - `rendering.rs` — `system_text` output: coordination rules, the brevity
    overrun report, aside-grammar teaching, and the mention-dispatch offer.
  - `context.rs` — `SessionContext` rendering and the two `initialize_session*`
    entry points, including the stated-window restatement.
