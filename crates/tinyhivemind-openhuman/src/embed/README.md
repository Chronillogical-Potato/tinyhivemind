# `embed`

`EmbedRunner`: every brief an `AgentSpec` agent on a runtime the host booted,
each dialling its own endpoint on the episode's tool server through
OpenHuman's three MCP dispatchers, holding one session across the episode.
`EmbedSeat` is the agent as the handle the driver binds. `services()` is the
one thing the runtime must be built with: MCP boot.
