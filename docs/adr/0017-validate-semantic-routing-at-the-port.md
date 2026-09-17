# Validate semantic routing at the port

**Status:** Accepted
**Date:** 2026-09-17

## Context

Selecting a suitable specialist requires semantic judgment, but eligibility,
fan-out bounds, confidence policy, and fallback are invariants. Putting them in
a prompt makes routing neither reproducible nor auditable.

## Decision

`tinyhivemind-embed` owns fixed-point acceptance and bounded composition.
`tinyhivemind-typesafe` owns exact System One wire types, Jev questions, and a
single `SystemOneTransport` waiting port. It prescribes no HTTP client or async
runtime. Core and hive remain unchanged and pure.

Jev returns one mutually exclusive primary Choice and independent Nouls for
collaboration, clarification, impact, and contributions. Rust validates and
composes them. One reasoning escalation is permitted only for a well-formed
uncertain result. Every other failure is deterministic.

## Consequences

Hosts can reuse routing without adopting an HTTP stack. Raw probabilities,
model identity, schema version, roster version, and acceptance disposition are
preserved. A provider can suggest but cannot expand membership or fan-out.
