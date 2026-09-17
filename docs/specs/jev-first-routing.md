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
multi-label result.

For a desk larger than the configured Choice limit, one batched suitability
screen produces a bounded shortlist and one final Choice. Non-shortlisted
candidates remain in the audited fixed-point domain with zero probability.

## Invariants and constraints

Pure acceptance requires an exact Choice domain, an exact fixed-point total, a
maximal selected alternative, calibrated confidence, the current roster
version, and one contribution judgment per eligible candidate. Unavailable
candidates are excluded before inference and rejected if invented by a
provider. Thresholds are host-supplied calibration artifacts; the library has
no cookbook defaults.

Collaboration retains the primary. Invitations clear the contribution
threshold, order by probability then effective desk order, and are truncated
so the opening round is no wider than `round_width`.

Well-formed uncertain or high-impact evaluations may receive one reasoning
escalation over the identical snapshot. Transport failures, malformed output,
stale snapshots, and failed escalation use the deterministic desk fallback.

## Acceptance criteria

- Ordinary desk routing performs exactly one batched Jev request.
- Mentions, DMs, General, and Workflow perform none.
- No unavailable or non-member id can be accepted.
- Provider and escalation failures return an auditable fallback reason.
- A hive plan cannot exceed `round_width`.

## Open questions

The calibrated threshold values and the final provider Choice option limit are
deployment evidence, not library defaults.
