# `tinyhivemind-tools`

The episode's tools, as a record a host drains. `EpisodeTools` holds per seat
the turn the host opened, the rows it may `read`, and the calls it made, and
decides each call in `call`: the turn, the thread, `interpret`, then the
event or the refusal. `tool_definitions` renders the served specs as JSON
tool definitions.

It accepts JSON call arguments without owning a transport. A host wraps the
definitions in its tool type and calls `EpisodeTools::call` in process. The
record supplies the same acknowledgement and refusal text to every seat.

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

[`tinyhivemind-openhuman`](../tinyhivemind-openhuman/README.md) depends on
this crate so its native runners drain the same `SeatEvent` and `Refusal`
records. The driver consumes the resulting
committed events through the host, without depending on this tool crate.

See the [workspace dependency map](../../docs/crate-dependencies.md).
