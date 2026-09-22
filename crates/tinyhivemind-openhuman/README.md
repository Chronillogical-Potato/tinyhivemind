# `tinyhivemind-openhuman`

The OpenHuman adapter. `tinyhivemind-driver` says who runs next and what a
committed row means, over a handle the host binds, and never runs a turn.
This crate is the host's side of that seam for OpenHuman, three ways:

| Runner | Seat | Tools | Context between turns |
| --- | --- | --- | --- |
| `HostedRunner` | the host's own agent, built by the host through `EpisodeHost` with the episode's tools added | the four tools in-process, admitted over the host's own gate | seeded every turn from the host's log, as the seat, up to its watermark |
| `EmbedRunner` | an `openhuman-embed` `AgentSpec` agent on a runtime the host booted | the three MCP dispatchers, dialling `tinyhivemind-mcp`'s server | OpenHuman's own session, stable for the episode |
| `RawRunner` | an `OpenHumanSessionHost` built one level down, per turn | the same tools in-process, each calling `EpisodeTools::call` | a per-seat log this crate seeds the next session with |

All three implement `SeatRunner`, the seam: open a turn, run it, close it and
take what was called. Open and close are the same for every runner, because
every call lands in the same `EpisodeTools`, so the driver drains identical
events and a seat is refused and acknowledged in the same words whichever
runs it.

The hosted runner is the one for a host that already has agents. It asks the
host, through `EpisodeHost`, for three things: its log, a seat built with the
episode's belt, and a wrapper around each turn. `OpenHuman` fixes a session's
belt when it is built, so the host builds each seat once per episode, and the
runner reuses it: each turn it clears the session, seeds it from the host's
log up to the seat's watermark, runs the brief, keeps the turn's usage, and
hands it to the host's after-turn hook, where approvals are parked and spend
is metered. A host with tools of its own prefixes the episode's, so none
shares a name with its own and is admitted past its gate. Nothing about the
host's agent -- model, tools, gate, memory, prompt -- is re-expressed here. `RunnerKind`
names one, from `TINYHIVEMIND_RUNNER` or directly.

The raw runner also carries the two things the current OpenHuman asks of a
session built outside its product: every seat is registered as a workspace
definition with its belt named (`RawRunner::prepare`), because the hosted
turn takes the allowlist from the definition and fails closed on a wildcard;
and the seats run under a library-host core context (`RawRunner::seat`),
because with none the core waits on the operator signing in.

This is the one crate in the workspace that links a harness. It takes
`openhuman-embed`, `openhuman`, `tinytools` and `tinytools-agent` as git
dependencies pinned by rev and patched onto `vendor/openhuman` (ADR 0020), so
a host that vendors this repository writes the same patches against its own
tree and links one OpenHuman. The `offline` feature ships the scripted model
and backend stub both runners are proven against, and the metrics the
example's bench reads.

See [`src/README.md`](src/README.md) for the source layout, and
`examples/openhuman/src/bin/conducted.rs` for a host stepping an episode
through either runner.
