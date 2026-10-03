# TinyHivemind OpenHuman adapter

The host constructs configured `Agent` instances on one OpenHuman `Runtime`.
`OpenHumanHost` receives those handles, attaches permanent hive tools, and hands
continuing turns to the durable `tinyhivemind-hives` coordinator.

```rust,ignore
let coordinator = Coordinator::new(
    runtime.runtime_id().into(), Arc::new(MemoryStorage::new()),
    CoordinatorOptions::default(),
)?;
let host = OpenHumanHost::new(runtime.runtime_id().into(), coordinator)?;
host.register_agent(agent)?;
```

Register an existing conversation with `register_agent_in_session(agent, session_id)`.
Repeated clones are idempotent. Failed durable registrations retain the same
attachment source for retries, while tool execution stays disabled until
registration succeeds. Agents from another runtime are rejected.
The adapter never reconstructs an agent or clears its conversation.
Session registration validates and commits the supplied binding before the
runner is visible to a live scheduler. A storage failure publishes neither.

The native tools cover discovery, reads, hive and direct messaging, and explicit
episode actions. All schemas bind the sender to the supplied agent. Tools remain
in the system catalogue and provider schemas, even when unrelated tools use deferred
discovery. Membership changes do not multiply definitions.
The ordinary family has nine tools; management adds four. `hivemind_read`
requires exactly one hive or peer destination. Peer reads expose the caller's
durable direct messages and replies without scheduling a return turn.

Configure `with_management(factory, authorizer)` before registering or cloning
the host to enable the four management tools. The host factory creates fully
configured agents from nonsecret template references. Authorization runs before
factory or coordinator mutations.

`TurnHooks` provides progress, a scoped turn wrapper, and usage/approval
finalization on both successful and failed turns. The default wall is 300 seconds.
Return `TurnDisposition::Parked` to hold the agent until coordinator `release`.
After a successful provider turn, finalization failure preserves its committed
session while interrupting delivery and suppressing staged actions and replies.

The host must keep its `OpenHumanHost` alive while attached tools are in use.
Attachments keep weak service references; dropping it releases the coordinator
without retaining an agent/attachment cycle. Definitions remain available on a
surviving agent, and execution reports unavailable services.

Runnable host construction and topology proofs are in
[`examples/openhuman`](../../examples/openhuman/README.md).
See the [OpenCompany migration guide](../../docs/opencompany-migration.md) for
construction, single-runtime dependency unification, and recovery.
