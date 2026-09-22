# `tinyhivemind-openhuman`

The OpenHuman adapter. `tinyhivemind-driver` says who runs next and what a
committed row means, over a handle the host binds, and never runs a turn.
This crate is the host's side of that seam for OpenHuman, both ways:

| Runner | Seat | Tools | Context between turns |
| --- | --- | --- | --- |
| `EmbedRunner` | an `openhuman-embed` `AgentSpec` agent on a runtime the host booted | the three MCP dispatchers, dialling `tinyhivemind-mcp`'s server | OpenHuman's own session, stable for the episode |
| `RawRunner` | an `OpenHumanSessionHost` built one level down, per turn | the same tools in-process, each calling `EpisodeTools::call` | a per-seat log this crate seeds the next session with |

Both implement `SeatRunner`, the seam: open a turn, run it, close it and take
what was called. Open and close are the same for both, because every call
lands in the same `EpisodeTools`, so the driver drains identical events and a
seat is refused and acknowledged in the same words either way. `RunnerKind`
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
