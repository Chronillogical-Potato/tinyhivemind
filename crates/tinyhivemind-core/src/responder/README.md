# Responder module

This module decides, from a caller-supplied roster, desk snapshot, and request,
which single agent answers one message. It performs no IO, awaits nothing, and
never dispatches a turn — it only produces a `ResponderPlan` for a host to act
on, and optionally one bounded request for a model-assisted selector.

## Public surface

- `responder_plan` runs the ladder and returns a `ResponderPlan`: either an
  immediate `Decided` outcome or a `Select` request naming the desk's
  effective candidates and a deterministic fallback.
- `accept_evaluation` validates a complete fixed-point candidate distribution,
  selected maximum, and configured confidence threshold. `accept_selection`
  remains only for legacy text adapters and benchmark controls.
- `ResponderRequest`, `SelectionPolicy`, `SelectionRequest`, and
  `SelectorCandidate` are the stable inputs.
- `ResponderPlan`, `ResponderDecision`, `ResponderRung`, and
  `SelectionDisposition` are the stable outputs.

## The ladder

`responder_plan` tries, in order:

1. **Explicit mention.** The first reading-order active-agent mention wins,
   regardless of desk or chat.
2. **Desk resolution.** A non-General chat that resolves to exactly one desk
   hands off to that desk. A `Lead` desk, or an `Auto` desk with at most one
   effective active member, decides immediately with that member. An `Auto`
   desk with more than one effective active member produces a `Select`
   request — unless `SelectionPolicy::Disabled` is set, in which case it
   decides immediately with the first member and an explicit `Disabled`
   disposition.
3. **Direct agent chat.** A bare id, a display name, or a `dm:`-prefixed
   identity that names exactly one active agent decides immediately.
4. **Orchestrator fallback.** Anything else — General, an unresolved chat, an
   ambiguous desk or agent name — falls back to the host's orchestrator agent,
   which must be active or the call errors.

Desk identity takes precedence over a same-named direct agent: if a chat
string resolves to a desk, that desk decides the plan, even when an agent
happens to share the string as an id or name.

An `Auto` desk's effective candidate list is every member id in `desks.members`
order that is also an active roster member. `SelectorCandidate` detail
supplied by the host is looked up per id; a duplicate id in the supplied detail
is ignored when it names a member outside that list, and is a
`DuplicateSelectorCandidate` error when it names one inside it. A member with no
matching detail gets a synthesized candidate: its id as both id and label, role
`"Teammate"`, and no description.

## Operational constraints

- `responder_plan` validates the roster and desk snapshots first and fails
  closed on a structural error.
- The `Select` variant always carries a deterministic first-candidate
  fallback, so a host with no selector, or an evaluator that errors, omits a
  candidate, returns a malformed distribution, or falls below confidence,
  still has exactly one agent to run.
- `accept_selection` accepts only output that resolves, after trimming, one
  optional trailing period, and one optional matching wrapper, to exactly one
  candidate id by case-insensitive comparison. Anything else — empty text,
  prose, multiple ids, an unlisted id, or extra punctuation — is rejected.
- `ResponderRung` and `SelectionDisposition` both render a `Display` sentence
  an agent may repeat to a person, held to
  [ADR 0009](../../../../docs/adr/0009-a-refusal-renders-what-the-caller-already-holds.md):
  the ladder never declines to name a responder, so every rung and every
  disposition renders its own sentence rather than a shared, withheld one.

The normative behavior is in
[`../../../../docs/specs/responders.md`](../../../../docs/specs/responders.md),
and the test-first implementation sequence is in
[`../../../../docs/plans/responders.md`](../../../../docs/plans/responders.md).
