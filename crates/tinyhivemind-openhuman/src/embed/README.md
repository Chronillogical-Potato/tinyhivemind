# `embed`

`EmbedRunner::seat` creates an `AgentSpec` agent for each brief on a runtime
the host booted. It hands the episode tools to each spec through
`AgentSpec::tools`, which rebuilds the native belt per turn. `EmbedSeat` is
the agent handle the driver binds. The model sees each tool's name, schema,
and roster choices in its first request.

A seat keeps one session for the episode and seeds it every turn from the
host's journal. The seed contains rows the seat may read up to its watermark,
its persona, and the new rows in the brief. The session is seeded again on a
second turn, so a seat can speak more than once without binding a stale
transcript target.

| File | Purpose |
| --- | --- |
| `mod.rs` | Seat construction, native tool belt, and turn execution. |
