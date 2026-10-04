# Supplied agents

`mod.rs` binds one runtime to a coordinator, retains identical attachment
factories for repeated clones, and authorizes optional dynamic management.
`types.rs` defines host factories, authorization requests, progress/usage hooks,
and `RegisteredAgent`, the core bound-handle wrapper.
`activation.rs` gates tool execution on durable registration and wakes a runner
claimed concurrently before adapter activation. `activation_test.rs` covers
waiting and already activated claims. `runner.rs` sends attributed JSON input through the supplied agent, continuing
its stored session without clearing history. `test.rs` and `runner_test.rs`
cover registration, ownership, management and real provider requests.

Hooks and management must be configured before sharing or registering the host.
Factories and attached tools carry weak host references to prevent a cycle.
The host's progress sender must have a reader throughout each turn.

Existing host conversations use the coordinator's atomic session registration
API. A concurrently running scheduler sees the supplied session from its first
claim. A claimed runner waits for attachment activation; cancellation during
that wait follows the coordinator's interruption path and starts no model call.

`with_hive_memory` binds every seat registered afterwards to one hive memory
(see [`../memory`](../memory/README.md)). `register_spec` applies the binding to a
seat's `AgentSpec` before building it; `register_agent` refuses an agent built
without it. `memory_test.rs` covers bound seats, unbound and foreign-bound
refusals, the recall budget, and memory switched off.

Seat sessions persist in OpenHuman: the coordinator stores the session id the
first turn returns, and every later turn continues it through `turn.session(id)`.
The adapter never clears a session; compaction is the only thing that drops
turns from the live context, and the memory lifecycle recalls what it dropped.

`continuity_test.rs` captures actual provider requests after a first-turn hook
failure, proving that the next delivery retains the committed input and assistant
reply. Finalizer errors return a failed outcome carrying the completed session;
the coordinator preserves that binding while suppressing delivery acknowledgements
and staged episode outputs.
