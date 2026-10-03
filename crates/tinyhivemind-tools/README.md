# `tinyhivemind-tools`

The episode's tools, as a record a host drains. `EpisodeTools` holds per seat
the turn the host opened, the rows it may `read`, and the calls it made, and
decides each call in `call`: the turn, the thread, `interpret`, then the
event or the refusal. `tool_definitions` renders the served specs as JSON
tool definitions.

It accepts JSON call arguments without owning a transport. A harness that
takes native tools wraps the definitions in its own tool type and calls
`EpisodeTools::call` in process; a harness that can only dial MCP reaches it through
[`tinyhivemind-mcp`](../tinyhivemind-mcp/README.md), which depends on this
crate and is the one socket the repository opens. Either way a seat is
refused, acknowledged and recorded in the same words.

| Path | Purpose |
| --- | --- |
| `src/tools/` | `EpisodeTools`, `Dispatch`, `SeatEvent`, `Refusal`; `register`/`clear`, `window`, `drain`, `drain_refusals`, `call`. |
| `src/render/` | `tool_specs()` as tool definitions, and a call's arguments onto `CallArguments`. |

`post` and `dm` are in the vocabulary and are not served: in a completion
episode every call has a consequence, and a fact reaches the desk as a
completion's message.

It opens nothing and awaits nothing, and is in the pure list
`.github/scripts/assert-pure.sh` guards.

## How it relates to the other crates

This crate depends on [`tinyhivemind`](../tinyhivemind/README.md) for
`speech::tool_specs`, `CallArguments`, and `interpret`. It renders that
single vocabulary as JSON tool definitions and records accepted or refused
calls in `EpisodeTools`. It does not decide what a recorded event means for
the episode.

[`tinyhivemind-mcp`](../tinyhivemind-mcp/README.md) depends on this crate
to serve those definitions and calls over a socket, and re-exports its main
types. [`tinyhivemind-openhuman`](../tinyhivemind-openhuman/README.md)
also depends on it directly so native and MCP runners drain the same
`SeatEvent` and `Refusal` records. The driver consumes the resulting
committed events through the host, without depending on this tool crate.

See the [workspace dependency map](../../docs/crate-dependencies.md).
