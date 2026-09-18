# Complete episodes with explicit agent events

**Status:** Accepted
**Date:** 2026-09-18

## Context

The existing hive episode uses bounded turns, traces, and quorum to decide when
a room has converged. Persistent agents doing assigned work have another
observable fact available: each agent can state through a typed tool that its
current assignment is done. Inferring that fact from prose or from a turn
barrier repeats work and confuses a finished turn with finished work.

Agents also discover work that belongs with another specialist. A broadcast to
everyone is wasteful, while requiring the author to know the right identity
duplicates semantic routing inside the agent.

## Decision

Add a completion-driven episode as an alternative to the quorum-driven state
machine. Its state advances only on explicit completion events and on accepted
assignments. It completes when every participant has completed its latest
assignment.

Advertise `complete_episode` as the completion tool. Keep legacy `close` input
readable for stored rows and older hosts.

Add a `broadcast` tool whose message is routed by a TypeSafe Choice over the
eligible team excluding the author. The Choice maximum is primary and every
other option strictly above 20% is included, within the existing width bound.
Only the accepted recipients are recorded as newly assigned.

## Consequences

An episode can track real work across durable per-agent sessions without
guessing from model text or requiring agents to finish in synchronized rounds.
A routed handoff can reopen precisely the agents who received new work. The
host still owns event storage, sequence assignment, scheduling, provider
transport, budgets, and cancellation.

The existing deliberation episode remains available for questions settled by
quorum. Hosts must choose the episode mode explicitly; the two termination
rules are not silently mixed.

This decision implements
[`completion-driven-episodes.md`](../specs/completion-driven-episodes.md) and
extends the semantic boundary established by
[ADR 0017](0017-validate-semantic-routing-at-the-port.md).
