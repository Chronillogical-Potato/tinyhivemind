# 27. Retire unused adapters and optional mechanisms

- **Status:** Accepted
- **Date:** 2026-10-03
- **Supersedes:** [ADR 0022](0022-the-episode-mcp-server-is-the-one-socket.md);
  the episode use of the directory in
  [ADR 0007](0007-the-directory-is-folded-from-citations.md); and the optional
  mechanisms in [ADR 0003](0003-refutation-links-evidence-to-a-topic.md) and
  [ADR 0004](0004-grounds-are-weighed-by-evidential-depth.md)

## Context

The workspace accumulated several optional paths while the host integration and
hive protocol were being explored. They added public types, examples, and
configuration without a corresponding need in the current integration.

The episode tool record now has a native OpenHuman adapter. The reversal
condition in ADR 0022 is met: a bound harness can receive tools without
carrying this library's vocabulary itself. The MCP server adds a listener and
a second way to bind the same tool calls. The [live integration record](../experiments/2026-09-22-live-through-the-crate.md)
also documents tool discovery and call failures in the earlier MCP path. Those
runs do not establish that MCP is unsuitable elsewhere; they explain why this
adapter has no present role here.

The optional hive mechanisms have measured costs. The [refutation and grounds
experiment](../experiments/2026-09-01-refutation-and-grounds.md) found lower
accuracy with a refutation cap or an evidential quorum requirement than with
the base policy. The [expert delegation experiment](../experiments/2026-09-05-expert-delegation.md)
found no useful gain from the folded directory or deferral. Its live rooms did
not award a turn through `Knows` or use `!defer`. These findings are specific to
the recorded tasks and policies. They give no reason to keep the extra paths
in this small library while no host relies on them.

## Decision

Remove `tinyhivemind-mcp` and its server integration. Keep the pure episode
tool definitions and call record in `tinyhivemind-tools`; hosts bind them to
native tools. Remove the unused `AgentRegistry` and `RawRunner` paths. The
current OpenHuman adapter uses bound handles and its hosted or embedded
runners.

Remove the hive's optional budget allocator, refutation cap, evidential quorum
requirement, deferral routing, and `Knows` bid. Keep parsing `!refute` and
`!defer` for wire compatibility; a grounded refutation remains in the standing
for audit, without capping the topic. Retain the pure transactive-memory
directory fold for division and other callers, but remove its wiring into
episode attention and policy. Keep ordinary grounded objection, evidence,
support, quorum, and bounded rounds. An archived experiment may still describe a retired arm; it is a
record of what was measured, not a current command or API reference.

Remove dormant runtime wrappers for selector calls, mention dispatch, referral
queueing, and human approval. Their pure decision types and folds remain where
the driver or a host uses them. Remove standalone find/select/search entry
points; retain transcript projection, caller-owned sharing state, pins, threads,
and digest. The host remains responsible for storage and any application search.

## Consequences

There is one fewer transport to operate and fewer public switches whose default
was off. The host has one explicit binding point for episode tools, and the
pure crates retain the decisions that current integrations use. Existing code
that imported a removed path must migrate to the retained fold or its own host
operation. This is a public API reduction.

The earlier ADRs remain as records of decisions made at the time. Their
mechanisms are not current guidance. The experiments remain dated evidence;
they should not be rewritten to make an old run appear to use today's code.

A future use case can reintroduce a mechanism with a new specification and
measurements against the current baseline. The old records provide context,
not an automatic reason to restore the old implementation.
