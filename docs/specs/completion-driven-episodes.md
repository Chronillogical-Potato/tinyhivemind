# Completion-driven episodes

**Status:** Implemented
**Owner:** TinyHiveMind maintainers

## Problem

A session-native team can work for different lengths of time and hand work
between persistent agents. A turn count, fan-out barrier, or inferred statement
of finality does not reliably say whether those agents have finished their
assignments.

## Goals and non-goals

An agent explicitly reports completion through a tool. An agent may also hand
work to the semantically best-placed teammates without knowing their ids. The
host can durably reconstruct whether an episode is active from these observed
events.

This protocol does not infer completion from prose, replace persistent agent
sessions, store events, authorize side effects, or remove the existing
quorum-driven episode. It is an alternative episode mode.

## Behavior

The host opens `CompletionEpisodeState` with the accepted recipients of the
initial route. Every participant begins with one pending assignment.

`complete_episode { message }` appends the agent's final desk-visible message
and records completion of that agent's latest assignment. The episode is
complete only when every participant's latest assignment has a matching later
completion event. Replaying the same completion event is idempotent. A stale
event cannot complete newer work.

`broadcast { message }` appends a desk-visible handoff and invokes semantic
routing over the current eligible team, excluding its author. The TypeSafe
request carries the exact message, `AgentBroadcast { author_id }`, desk state,
and candidates. One Choice selects the primary recipient. Other candidates
strictly above 20% receive the same handoff concurrently, bounded by
`round_width`. Accepted recipients receive a new assignment and therefore
become pending even if they had previously completed.

The legacy input tool name and stored utterance tag `close` remain readable,
but hosts advertise and serialize `complete_episode`.

## Invariants and constraints

- Completion is an explicit tool event, never a language-model judgment.
- A broadcast is routed, not sent indiscriminately to every member.
- The broadcast author is not a candidate for its own handoff.
- Only accepted, eligible Choice recipients become assigned.
- One broadcast dispatch is bounded by `round_width`; `none` is never sent.
- Participant order is stable, ids are nonblank and unique, and event
  sequences move forward.
- The host owns storage, provider calls, scheduling, deadlines, cancellation,
  sessions, and the durable mapping from tool calls to sequence numbers.

## Acceptance criteria

- A two-agent episode remains active after one completion and completes after
  both complete.
- Assigning new work to a completed participant reopens only that participant.
- Unknown participants and stale completion events fail with typed errors.
- A broadcast performs one normal TypeSafe Choice request and applies the same
  deterministic eligibility and bounded `>20%` rule as desk routing.
- A malformed broadcast source fails before a provider call.

## Open questions

Provider thresholds and host-level time or spend limits remain deployment
policy. They can stop a run, but successful episode completion is defined only
by explicit participant completion.
