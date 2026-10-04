# Supplied-agent acceptance proof

`types.rs` holds the host fixture and factory handles.

`fixture.rs` constructs one host-owned runtime, per-agent skill bundles,
private MCP servers, private workspaces, and host-authored prompts.
Skills use the supported explicit workspace discovery root; the example does
not use `AgentSpec::skills_dir`'s separate agent-home installation path.
Native `use_skill`, MCP catalogue and MCP invocation receipts prove that those
installed resources are accessible independently of prompt text. Each
continuing agent repeats those native calls after registration; new phase-tagged
call IDs tie success assertions to their newly returned receipts. It also records actual model
requests and returns deterministic native tool calls.

`topology.rs` binds conversations started before registration, then delivers
real hive episodes in four topology shapes. Request assertions cover continuing
history, configuration isolation, and stable prompt/schema tool definitions.

`dynamic.rs` installs host authorization and a factory, exercises management
through model tool calls, and sends work to the newly configured agent.

`test.rs` runs the whole example on a Tokio runtime whose worker stacks match
OpenHuman's embedded loop requirements. The benign stdio MCP fixture requires
`python3`; all other services run on loopback and require no credentials.
