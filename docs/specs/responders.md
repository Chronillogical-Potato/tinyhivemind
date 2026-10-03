# Responder selection

> **Current API note.** [ADR 0027](../adr/0027-retire-unused-adapters-and-optional-mechanisms.md) retired the runtime waiting wrapper and some standalone query entry points described below. The retained pure folds and transcript views are documented in the current crate READMEs.


**Status:** Implemented
**Owner:** tinyhivemind maintainers

## Problem

A group message must start exactly one agent turn. The choice can be explicit,
desk-defined, or model-assisted, but hosts need one deterministic ladder and a
narrow selector boundary that cannot read tools, transcripts, or host state.

## Goals

- Select one active responder from mentions, desk policy, direct-agent chats,
  or the orchestrator fallback.
- Support lead-first and model-assisted desk modes.
- Keep the model call behind an object-safe, runtime-neutral `Selector` port.
- Fail closed on ambiguous agent names and malformed selector output.

## Non-goals

- Dispatching a turn, expanding `@everyone`, persistence, tools, or transcript
  access. P7 owns dispatch.
- Allowing a selector to choose an agent outside the effective desk roster.

## Proposed behavior

`Desk` gains `ResponderMode`: `lead` is the default and is omitted from its
wire form; `auto` opts a desk into model-assisted selection. Existing lead-mode
desk JSON therefore stays byte-for-byte structurally compatible.

`responder_plan` validates the borrowed snapshots and applies this ladder:

1. The first reading-order, nonquiet, active direct agent mention wins.
2. A non-General chat resolving to a desk uses that desk. Lead mode selects its
   first effective active member. Auto mode uses zero members as an
   orchestrator fallback, one member directly, and two or more as selector
   candidates when selection is allowed. Disabled selection chooses the first
   candidate at the desk-default rung with a `disabled` disposition.
3. A bare active agent id/name or `dm:<id-or-name>` selects that agent. Desk
   interpretation outranks an agent collision. Ambiguous agent names fail
   closed to the orchestrator.
4. General, unaddressed, and unresolved chats select the active orchestrator.

Effective desk candidates preserve desk order, exclude retired or absent
agents, and are deduplicated. Candidate detail is clamped to that set. Missing
detail uses the member id as its label and `Teammate` as its role. When an
allowed Auto desk with two or more candidates reaches candidate enrichment,
duplicate detail for an effective candidate id is a structural error. Extra
detail is ignored, and metadata is not validated on any rung that does not
construct a `SelectionRequest`.

All responder payload fields are required in their JSON wire form. The
`description` and `chat` fields accept explicit `null`; omission remains a
missing-field error so producer drift cannot silently change a request.

`SelectionRequest` exposes only the raw message, canonical desk id, and the
bounded candidate descriptions. `Selector` returns text. `accept_selection`
accepts only one candidate id, ASCII-case-insensitively, after trimming, one
matching quote/double-quote/backtick wrapper, and one trailing period. Empty,
prose, multiple ids, and out-of-set ids are invalid. Selector absence/failure
and invalid output deterministically choose the first candidate with the
corresponding disposition at the desk-default rung. Only accepted selector
output produces the auto-selection rung with a `selected` disposition.

## Invariants and constraints

- One request produces exactly one decision and never dispatches.
- Explicit direct mentions win even when the agent is outside the desk.
- Person, desk, everyone, and quiet mentions do not select a responder.
- The selector sees no transcript, tools, model handles, or host callbacks.
- The orchestrator is required to be active only if its fallback rung is
  reached; otherwise `NoActiveResponder` is returned.

## Refusals a model could repeat

Two rules from the OpenBot survey
([`../research/grok-bots/copilotkit-openbot.md`](../research/grok-bots/copilotkit-openbot.md)),
and where the ladder stands against each.

**Withhold rather than offer and then refuse.** The ladder already has this
shape, and it has it by construction rather than by rule. Selection is never
offered and then declined: a rung that cannot use a selector's answer does not
build a `SelectionRequest` at all, disabled selection goes straight to the first
candidate at the desk-default rung, a desk with fewer than two effective
candidates never reaches the model, and candidate metadata is not even validated
on a rung that constructs no request. The selector is asked exactly when its
answer can be acted on, which is the same discipline as not offering a
delegation tool to a run already at its hop cap
([`mention-dispatch.md`](mention-dispatch.md)).

The briefing now holds the same shape, and it was the last place that did not.
`TeamBriefing::system_text` states no mention-dispatch rule at all, and
`system_text_with_dispatch` states it only under a `MentionDispatchContext`
whose `may_dispatch` holds — so a disabled policy, a spent hop budget, and a
caller that supplies no context are each silent about the capability rather
than offering it and having the next call refuse. See
[`sessions.md`](sessions.md); the gap `mention-dispatch.md` records under
"A run at the cap is not offered the action" is closed and its wording is
stale.

**A decline is a sentence before it is a type.** This one the ladder mostly
sidesteps rather than satisfies. It does not decline: every rung ends in an
agent id, and each way a rung could have failed — an ambiguous agent name, an
absent selector, a failed one, output that is prose or names two candidates —
is specified to fall back deterministically and record why in
`SelectionDisposition`. The only outcome that is not an id is
`NoActiveResponder`, which is a malformed-roster error rather than a message to
anyone.

The rendering mechanism is settled, and today every variant renders
distinctly. `SelectionDisposition` and `ResponderRung` carry a `Display` impl,
so the sentence a host shows a person comes from the library rather than from
each host's own wording, and every disposition and rung arrives beside the
responder id it explains, so none of them currently discloses the existence,
activity or reachability of a participant the caller could not already read
from the roster it supplied. See
[ADR 0009](../adr/0009-a-refusal-renders-what-the-caller-already-holds.md).
Whether any two of these specific variants must one day collapse onto a
shared sentence — the way ADR 0009 collapses dispatch and approval refusals —
is the open question the closing section of this spec leaves to enumeration
work in core, not a contradiction of "settled": settled describes the
mechanism and the current, per-variant wording, not a permanent ban on ever
grouping two of them.

It is bounded by [ADR 0008](../adr/0008-an-approval-decision-is-total.md),
which rules that a denial's reason is for the operator's log, because refusals
a caller can tell apart are refusals a caller can probe — OpenBot returns one
sentence for "no such bot" and "not yours to see" precisely so an agent cannot
enumerate a roster by reading which refusal came back. That pulled against
"every decline is text an agent can repeat to a person", and
[ADR 0009](../adr/0009-a-refusal-renders-what-the-caller-already-holds.md)
resolves it: a refusal may render a sentence of its own only when what it
discloses is something the caller already holds, and anything turning on the
existence, activity, membership or reachability of a named other renders one
shared sentence. Audience separation alone was insufficient — it constrains the
channel, not the information, so a host obeying it could still write one
distinct sentence per reason.

They are reconcilable only by separating the two audiences, and by accepting
that the sentence is the coarser of the two: a rendered decline may say *that*
it declined and what the person can do next, while the reason that selects it
stays in the log, and any two internal reasons a caller must not be able to
distinguish must map to the *same* sentence. This spec does not settle which
reasons those are. The selection path is milder than the gate — a disposition
reports a fallback that already happened rather than withholding something —
but "the model you asked for was not consulted" and "the model you asked for
declined to name a candidate" are still distinguishable, and whether they may
be is unresolved. Work on enumeration-resistant refusals in core is where it
should be settled, not here.

## Acceptance criteria

- Desk lead and auto wire forms are pinned.
- Tests cover every ladder rung, collisions, inactive members, selector
  success/failure/absence/invalid output, parsing boundaries, and exactly-one
  decision for messages containing multiple direct mentions and `@everyone`.
- The runtime selector is called at most once and has no dispatch capability;
  P7 exposes dispatch separately behind the atomic host queue port.
- Core remains accepted by the purity assertion.

## Open questions

Nothing is open for P6. Turn creation and hop bounds are deferred to P7.
