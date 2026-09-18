# OpenCompany routing compatibility

**Status:** Accepted
**Owner:** OpenCompany integration maintainers

## Problem

OpenCompany needs Jev-first routing without replacing its roster, event log,
durable OpenHuman sessions, unified-session watermark, or delivery queue.

## Goals and non-goals

Define the later thin adapter boundary. This change does not modify
OpenCompany or OpenHuman before the TinyHiveMind API and evidence land.

## Proposed behavior

The host builds one OpenHuman `Runtime` and instantiates one OpenHuman `Agent`
per company agent. The `tinyhivemind-openhuman` adapter binds those existing
handles to canonical ids in a validated `OpenHumanHive`. It resolves accepted
plans to borrowed handles and drives completion state only from host-committed
events; it never constructs an agent, serializes a provider session id, or
recreates an agent between turns. Each turn separately carries a
`ConversationRef`.

Ordinary desk and direct turns use host-seeded unified-session deltas. Hive
seats receive attributed episode history and disable history seeding without
resetting the company-wide watermark.

OpenHuman owns session transcripts, continuation, and compaction. The adapter
uses the same OpenHuman agent and its stable per-agent thread when that agent
moves between desks, threads, DMs, General, and workflows. Conversation ids do
not participate in agent identity.

## Invariants and constraints

Candidates come only from effective current desk membership. Authorization,
retirement, and membership are deterministic preconditions. Snapshot version
must still match when a result is applied.

Credentials remain server-side. The adapter may implement
`SystemOneTransport` with `tinyjevclient` and `TYPESAFE_API_KEY`; neither enters
library state or serialized routing records.

The standalone [`examples/openhuman`](../../examples/openhuman/README.md)
proves the dependency direction and turn seam with a real embedded OpenHuman
`Harness`, exact `JevRouter` questions, and loopback fixtures. It is mechanical
evidence only: the fixture does not establish live Jev quality or provider
performance.

## Acceptance criteria

- No OpenCompany type is named by TinyHiveMind.
- The adapter replaces no storage model.
- DMs cannot enter a desk quorum.
- Stable per-agent OpenHuman session ids survive surface changes.
- TinyHiveMind stores no OpenHuman session registry or transcript.

## Open questions

The OpenCompany host integration still lands only after the upstream
TinyHiveMind change merges and its live evidence is stable.
