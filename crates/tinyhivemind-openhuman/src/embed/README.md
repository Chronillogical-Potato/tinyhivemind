# `embed`

`EmbedRunner`: every brief an `AgentSpec` agent on a runtime the host booted.
`EmbedSeat` is the agent as the handle the driver binds.

Two roads to the same tools, and the loop cannot tell them apart:

| Constructor | The belt | What it costs |
| --- | --- | --- |
| `seat` (default) | the episode's tools handed to the spec through `AgentSpec::tools`, rebuilt per turn | nothing but a function call; the definitions are in the model's tool list, so there is nothing to discover |
| `seat_over_mcp` | the same tools over `tinyhivemind-mcp`, one endpoint per seat, through OpenHuman's three MCP dispatchers | a socket, a round trip per call, and a discovery call before the first |

`services()` -- MCP boot -- is what the second needs of its runtime; the
first needs none of it.

The native road is the default because a seat that must *fetch* its tool
definitions may not: live, one seat called `ask` having never listed them and
invented three teammates, and another invented a name for the group. The
tools a model is handed carry their own roster.

The MCP road stays because it is the one thing in this repository that
exercises the socket ADR 0022 opens, and because what a tool call costs over
a wire is worth being able to measure against what it costs in-process --
which is what `CONDUCTED_BENCH` does with the two arms side by side.

A seat keeps one session for the whole episode and **seeds** it every turn
from the host's journal (`crate::seed`), as a hosted seat does: the rows that
seat may read up to its watermark, its persona at their head, and the turn's
new rows in the brief. Seeding replaces resume rather than adding to it, so
nothing binds a transcript to the session and a seat can speak as often as
the episode needs. Resuming instead -- what this ran before -- bound that
transcript on a seat's first committed turn and refused its second.
