# `tinyhivemind-mcp`

The room's tools, served over MCP, so an agent harness that cannot be handed a
native tool can still move a completion episode.

`tinyhivemind::speech` defines the speech vocabulary and validates one call.
[`tinyhivemind-tools`](../tinyhivemind-tools/README.md) selects the tools a
completion episode serves and records their calls. This crate puts that record
behind an MCP server. `tools/list` returns the served definitions as JSON
Schema, and `tools/call` passes arguments to `EpisodeTools::call`. Refusals
return to the seat as tool results.

The server has a narrow job:

- **It holds no episode state.** Assignments, completions, queues, budgets and
  open questions live in the driver. The server records that a seat called a
  tool and stops, or was refused and why. The host drains both; nothing here
  calls into the host.
- **It runs no turn.** An `ask` becomes an event the driver schedules; the
  server never holds an agent handle.
- **It depends on no harness.** Any
  MCP-capable harness gets the same five tools: `broadcast`, `ask`,
  `ask_teammates`, `complete_episode`, and `read`.

**Identity is structural.** Each seat is given its own endpoint,
`/seat/<agent_id>/<capability>`, the capability minted when the server binds,
so the caller is known from the URL it was handed rather than from a field it
filled in -- and a process that merely reaches loopback cannot speak as a seat. Every call also names the `chat` and `parent`
thread the host told the seat it is in, and the server checks both against
the turn the host registered for that seat -- a confused model that names the
wrong thread is refused, and two overlapping turns for one seat are told
apart by what they name.

This is the one socket the repository opens. Its charter says never; [ADR
0022](../../docs/adr/0022-the-episode-mcp-server-is-the-one-socket.md) says why
this crate is the exception and what would end it. It is in neither list
`.github/scripts/assert-pure.sh` guards, and it must stay out of every crate
that is.

The record itself -- `EpisodeTools`, its `call`, and `tool_definitions` --
lives in [`tinyhivemind-tools`](../tinyhivemind-tools/README.md), which opens
nothing; this crate is JSON-RPC over HTTP around it. A harness that takes
native tools wraps the definitions in its own tool type and calls in-process,
and never links this crate; an MCP seat and a native seat are refused and
acknowledged in the same words either way.

`post` and `dm` remain in the speech vocabulary, but this server does not
offer them. In the five live runs described by the tool crate, `post` was
mostly used for status updates or repeated findings. Completion carries a
finding to the desk; `ask` handles a question to a named seat.

## How it relates to the other crates

This crate depends on
[`tinyhivemind-tools`](../tinyhivemind-tools/README.md) for
`EpisodeTools`, the served definitions, and call records. It adds the MCP
server and re-exports the tool crate's main types so an MCP-only host can
name one crate. It also lists [`tinyhivemind`](../tinyhivemind/README.md)
as a direct dependency for wire tests that inspect a drained call.

[`tinyhivemind-openhuman`](../tinyhivemind-openhuman/README.md) depends on
this crate for the optional MCP route used by an embedded seat. Native-tool
runners call `tinyhivemind-tools` directly and do not need this server.

See the [workspace dependency map](../../docs/crate-dependencies.md).
