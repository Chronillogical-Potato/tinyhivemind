# Require host-supplied conversation kinds

**Status:** Accepted
**Date:** 2026-09-17

## Context

Chat-id prefixes are application conventions. Parsing `dm:` or another host
spelling inside a reusable library conflates identity with semantics and makes
direct-message safety dependent on naming.

## Decision

An embedding host supplies a canonical id and explicit `ConversationKind` for
every turn. Only `Desk` may invoke semantic routing or open a hive. `Direct`,
`General`, and `Workflow` bypass it. Outbound direct messages, desk asides,
referrals, and current-conversation replies are distinct typed routes.

## Consequences

The host remains authoritative for ids, storage, and durable agent sessions.
TinyHiveMind does not parse an OpenCompany convention. Direct messages cannot
accidentally enter a desk quorum because their surface is explicit.
