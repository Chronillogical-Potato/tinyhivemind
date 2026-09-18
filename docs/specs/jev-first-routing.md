# Jev-first routing

**Status:** Implemented
**Owner:** TinyHiveMind maintainers

## Problem

An unaddressed desk message needs semantic specialist selection before an
expensive agent turn, without allowing a model to decide eligibility, fan-out,
or fallback policy.

## Goals and non-goals

The system selects one primary, may invite a bounded set of distinct
specialists, preserves raw judgments for audit, and escalates uncertainty once.
It does not authorize actions, manage sessions, or replace host storage.

## Proposed behavior

Explicit agent mentions and direct conversations must not invoke semantic
routing. General and workflow surfaces retain host-defined single-responder
behavior.

An ordinary eligible desk produces one System One request containing one
`primary_responder` Choice, `needs_collaboration`, `needs_clarification`, and
`high_impact` Nouls, plus one independent `contributes_<agent>` Noul per
candidate. The Choice contains every eligible agent and `none`; it is not a
multi-label judgment, but its competing-option probabilities are retained as a
code-owned bounded fan-out signal.

When its eligible candidate count plus the reserved `none` alternative exceeds
the configured Choice limit, a desk takes the normal one-request path's
explicit, bounded exception: one batched suitability screen first produces a
bounded shortlist, followed by one final Choice request. Thus an oversized desk
makes exactly two Jev requests; it never sends an over-limit Choice.
Non-shortlisted candidates remain in the audited fixed-point domain with zero
probability.

## Invariants and constraints

Pure acceptance requires an exact Choice domain, an exact fixed-point total, a
maximal selected alternative, calibrated confidence, the current roster
version, and one contribution judgment per eligible candidate. Unavailable
candidates are excluded before inference and rejected if invented by a
provider. Candidate ids are unique, nonblank after trimming, and may not use
the reserved `none` alternative. Thresholds are host-supplied calibration
artifacts; the library has no cookbook defaults.

The maximum-probability Choice is the primary. Every other eligible agent whose
Choice probability is strictly greater than 20% receives the same message in
the opening round, regardless of the collaboration or contribution Nouls.
Invitations order by Choice probability then effective desk order and are
truncated so the opening round is no wider than `round_width`. `none` is never
dispatched. Exactly 20% remains single-responder routing. Contribution Nouls
remain in the evaluation for audit and calibration but do not select recipients.

Well-formed uncertain or high-impact evaluations may receive one reasoning
escalation over the identical snapshot. Transport failures, malformed output,
stale snapshots, and failed escalation use the deterministic desk fallback.

## Acceptance criteria

- A desk whose eligible candidates plus `none` fit the configured Choice limit
  performs exactly one batched Jev request; an oversized desk performs the
  documented two-request hierarchy.
- Mentions, DMs, General, and Workflow perform none.
- No unavailable or non-member id can be accepted.
- Provider and escalation failures return an auditable fallback reason.
- A hive plan cannot exceed `round_width`.

## Open questions

The confidence, clarification, and high-impact thresholds and the final
provider Choice option limit are deployment evidence. The strict 20% Choice
fan-out threshold is the routing policy implemented here.
