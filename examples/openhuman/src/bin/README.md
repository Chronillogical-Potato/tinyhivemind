# OpenHuman proof binaries

| File | Purpose |
| --- | --- |
| `conducted.rs` | One completion-driven episode stepped through `CompletionDriver`, over either runner. |
| `conducted/runner.rs` | The seam: `SeatRunner`, and `RunnerKind` from `TINYHIVEMIND_RUNNER`. |
| `conducted/runner/test.rs` | The runner is named by the environment; each states its own mechanics. |
| `conducted/embed.rs` | The embed runner: `openhuman-embed` agents, tools over MCP. |
| `conducted/raw.rs` | The raw runner: per-turn `OpenHumanSessionHost` sessions, tools in-process. |
| `conducted/raw/seat.rs` | One raw seat: builds, seeds, runs and drops a session. |
| `conducted/raw/tools.rs` | The served vocabulary as native tools over the shared record. |
| `conducted/raw/policy.rs` | The gate that admits only the belt, and the memory that keeps nothing. |
| `conducted/raw/offline.rs` | A scripted OpenAI-compatible model, for the offline proof. |
| `conducted/raw/test.rs` | The belt is the served vocabulary; a native call is recorded through the record; the gate denies the rest. |
| `conducted/jev.rs` | The live `SystemOneTransport` over `tinyjevclient`. |
| `deepswe_hive.rs` | Hermetic four-agent external software-engineering adapter with host-side OpenHuman and Docker-confined tools. |
| `deepswe_hive/` | Task validation, MCP tools, Docker confinement, and contract tests for the DeepSWE adapter. |
| `pe1006_hive.rs` | Web-assisted five-agent experiment driven by `tinyhivemind-openhuman`, with a durable shared workspace, explicit agent memory, and one OpenHuman runtime per run. |
| `pe1006_hive/episode.rs` | PE1006/PE1008 roles, routing candidates, and completion-tool compatibility parsing. |
| `pe1006_hive/round.rs` | Frozen prompt/snapshot preparation and bounded concurrent OpenHuman turn execution. |
| `pe1006_hive/tools.rs` | Local MCP completion tools and per-seat outbox persistence. |
| `pe1006_hive/typesafe.rs` | Live TypeSafe transport plus the shared routing policy and thread context. |
| `pe1006_hive/workspace.rs` | Workspace templates, non-overwriting initialization, and exact per-turn prompt/reply snapshots. |
