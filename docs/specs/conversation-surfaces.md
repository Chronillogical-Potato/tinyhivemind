# Conversation surfaces

**Status:** Implemented
**Owner:** TinyHiveMind maintainers

## Problem

Chat-id prefixes are host conventions and cannot safely define whether a turn
is a desk, direct message, general conversation, or workflow.

## Goals and non-goals

The library receives explicit conversation semantics and exposes distinct
outbound routes. It does not parse ids, own storage, or define agent-session
lifetime.

## Proposed behavior

- `Desk` covers a desk channel and a thread rooted in it. Only this surface may
  open a hive episode.
- `Direct` covers operator-to-agent and agent-to-agent conversations. It has one
  deterministic recipient and never opens a hive.
- `General` and `Workflow` preserve existing single-responder rules.

An agent session may span all surfaces. A per-turn `ConversationRef` does not
reset a session watermark.

`CurrentConversation`, `DirectAgent`, `DeskAside`, and `DeskReferral` are
distinct typed routes. A direct route creates or reuses the host's canonical
DM; an aside remains inside one desk; a referral returns non-voting evidence.
Before a host persists a `DeskAside`, it validates the route against its
configured opening-round width; an aside cannot name more immediate recipients
than that bound.

## Invariants and constraints

The host supplies the canonical id and kind. TinyHiveMind never infers either.
When an utterance has several direct recipients, the host writes one row per
canonical conversation. Existing bounded dispatch may wake at most one
immediate recipient; other rows arrive through unified-session deltas.

## Acceptance criteria

- `ConversationRef` pins id, kind, and optional thread root on the wire.
- Only `Desk::may_open_hive` is true.
- Direct messages bypass semantic routing.

## Open questions

None.
