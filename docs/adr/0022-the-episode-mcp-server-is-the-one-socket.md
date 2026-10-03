# 22. The episode MCP server is the one socket this repository opens

- **Status:** Proposed
- **Date:** 2026-09-22
- **Relates to:** [ADR 0013](0013-a-vendored-crate-is-an-example-dependency.md), [ADR 0019](0019-complete-episodes-with-explicit-agent-events.md)

## Context

The charter's first rule is that this repository never opens a database, a
file, or a socket: the host owns storage, and everything here is a fold over
what the host already holds. `crates/tinyhivemind-core` and
`crates/tinyhivemind-hive` are held to that by `.github/scripts/assert-pure.sh`,
and the three runtime crates are held to it less strictly by the same script.

A completion-driven episode ([ADR 0019](0019-complete-episodes-with-explicit-agent-events.md))
advances only when an agent calls a tool: `complete_episode`, `broadcast`,
`ask`. `tinyhivemind::speech` states those tools once, as data, and says a host
"renders `tool_specs` into its own tool language". Five live runs established
that nothing weaker than a tool call carries the protocol: a marker grammar is
interpreted, and three agents rendered one instruction three ways.

The first host is OpenHuman, and OpenHuman offers a foreign component exactly
one way to give an agent a tool without OpenHuman itself changing: an MCP
server. Its `openhuman-embed` `AgentSpec` takes `.mcp(server)` and nothing that
carries a native tool; its core assembles the tool vector from its own config
and accepts no host-supplied entries. The alternative -- adding the episode
tools to OpenHuman's own catalogue -- would put this repository's vocabulary
inside a harness that must not know it exists, and would put OpenHuman's release
cadence between an episode and its next tool.

So the episode tools are served over MCP, and an MCP server is a socket.

## Decision

One crate, `crates/tinyhivemind-mcp`, opens one loopback listener. It is the
MCP rendering of `tinyhivemind::speech` and nothing more. The rendering itself
-- the tool definitions, the call gate, the record a host drains -- is
`crates/tinyhivemind-tools`, which opens nothing and sits in the pure list;
the socket crate is JSON-RPC framing over it, so a harness handed native tools
takes the record without the wire and is refused in the same words:

- `tools/list` is `tool_specs()` as JSON Schema, descriptions verbatim, `dm`
  withheld, plus `chat` and `parent` on every tool so a call names the turn it
  believes it is in.
- `tools/call` maps the wire onto `CallArguments` and calls `interpret`; a
  refusal returns to the seat as the sentence `UtteranceRejection` wrote.
- A seat's identity is the endpoint it dialled, `/seat/<id>`. Its `chat` and
  `parent` are checked against the turn the host registered for it.
- It holds no episode state and runs no turn. Accepted calls are recorded as
  events the host drains after the turn; the completion driver does the rest.
- It depends on `tinyhivemind`, `tokio` and `serde_json`, and on no harness.

The crate is added to no list in `assert-pure.sh`, and no pure or runtime crate
may depend on it. Its consumer takes it as an explicit, separate dependency, so
a host that has a native tool seam of its own links nothing here.

## Consequences

The charter's rule now has one named exception rather than an unwritten one.
It is enforced by construction -- the socket lives in one crate that nothing
else in the workspace links -- rather than by a script, which is why this record
exists: the script cannot see it.

Any MCP-capable harness gets the same five tools. That is the property the
episode design was built for, and it is stated in code rather than promised.

The rendering is the vocabulary. A tool that is not in `tool_specs()` is not
served, and a description that changes there changes here without a second
edit. The one addition is the two thread arguments, and they exist so the
server can refuse a confused model rather than record its call against the
wrong episode.

## Reversal

If a harness this repository binds to gains a seam that lets a host contribute
a native tool **without the harness carrying the tool's vocabulary**, the
rendering for that harness moves to its adapter crate and this server becomes
optional for it. If every bound harness does, the crate is removed and this
record is superseded. What would not reverse it: a seam that requires the
harness to know the episode tools by name.

## Amendments

- **2026-09-22, four tools.** `post` is withheld as well as `dm`, by the sixth
  live run's finding that a call with no consequence is a call a seat makes
  instead of its work; see `docs/experiments/2026-09-22-live-through-the-crate.md`.
  The served set is `broadcast`, `ask`, `complete_episode` and `read`, and
  "the same five tools" above reads "four".
- **2026-09-22, the endpoint is a capability.** `/seat/<id>` became
  `/seat/<id>/<capability>`, the capability minted when the server binds and
  handed to the seat by the host. A process that can reach loopback and read
  `tools/list` cannot speak as a seat it was not handed, and learns nothing
  about the turn one is in. The identity is still the endpoint.
