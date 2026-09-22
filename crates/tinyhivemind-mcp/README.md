# `tinyhivemind-mcp`

The room's tools, served over MCP, so an agent harness that cannot be handed a
native tool can still move a completion episode.

`tinyhivemind::speech` states what a seat may say once, as data, and says a host "renders
`tool_specs` into its own tool language" and "maps its own wire onto
`CallArguments`". This crate is that rendering for MCP, and nothing more:
`tools/list` is `tool_specs()` as JSON Schema, `tools/call` is the wire mapped
onto `interpret`, and a refusal goes back to the seat as the sentence
`UtteranceRejection` already wrote for it.

Three things it is not, and each is a decision:

- **It holds no episode state.** Assignments, completions, queues, budgets and
  open questions live in the driver. The server records that a seat called a
  tool and stops. The host drains those records; nothing here calls into the
  host.
- **It runs no turn.** An `ask` becomes an event the driver schedules; the
  server never holds an agent handle.
- **It depends on no harness.** `tinyhivemind`, `tokio`, `serde_json`. Any
  MCP-capable harness gets the same four tools: `broadcast`, `ask`,
  `complete_episode`, `read`.

**Identity is structural.** Each seat is given its own endpoint,
`/seat/<agent_id>`, so the caller is known from the URL it dialled rather than
from a field it filled in. Every call also names the `chat` and `parent`
thread the host told the seat it is in, and the server checks both against
the turn the host registered for that seat -- a confused model that names the
wrong thread is refused, and two overlapping turns for one seat are told
apart by what they name.

This is the one socket the repository opens. Its charter says never; [ADR
0022](../../docs/adr/0022-the-episode-mcp-server-is-the-one-socket.md) says why
this crate is the exception and what would end it. It is in neither list
`.github/scripts/assert-pure.sh` guards, and it must stay out of every crate
that is.

`post` and `dm` are in the vocabulary and are not served. In a completion
episode every call has a consequence -- a question opened, work handed off, a
finding concluded -- and text with no consequence turned out, over five live
runs, to be status, repetition, and the description of calls never made. A
fact reaches the desk as a completion's message.
