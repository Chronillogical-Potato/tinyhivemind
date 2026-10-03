# OpenCompany: supply configured OpenHuman agents

TinyHivemind receives existing OpenHuman handles. Construct one `Runtime` in the
host and configure each `Agent` with its provider, prompt, policies, MCP servers,
skills, and memory before passing it to the adapter. Keep that runtime alive.
Agents can have distinct configurations while sharing its runtime identity.
Cargo must resolve the host and adapter to the same OpenHuman crates; use the
workspace's git dependency patches against the host's checkout.

## Replace construction with registration

| Earlier integration | Current integration |
| --- | --- |
| Embed/raw runners construct agent personas | Host constructs `Agent`; `OpenHumanHost` receives it |
| Adapter registry generation or library-host setup | Named native `hivemind` attachment on that existing handle |
| Per-hive session construction or repeated history seeding | One continuing session per agent across all hives |
| Separate hive/desk IDs | One public `hive_id`; core chat/desk fields translate internally |
| `EmbedSeat` wrapper | `RegisteredAgent(pub Agent)` for an explicit pure-driver loop |

The actual construction example is separate from the library:
[`examples/openhuman`](../examples/openhuman/README.md). It constructs private
workspaces containing `MEMORY.md` and skill bundles, separate stdio MCP servers,
and host-authored prompts. Captured requests and native results prove those
capabilities continue before and after registration. `AgentSpec::skills_dir`
currently installs under `Agent.home`; discovery uses workspace `skills`. The
example writes bundles at the discovery root; it does not fix that OpenHuman
installation/discovery mismatch.

## Register existing handles and conversations

```rust,ignore
let coordinator = Coordinator::new(
    runtime.runtime_id().into(),
    Arc::new(SqliteStorage::open("company-hives.sqlite")?),
    CoordinatorOptions::default(),
)?;
let host = OpenHumanHost::new(runtime.runtime_id().into(), coordinator)?;
// The host has already configured and, optionally, used this Agent.
host.register_agent_in_session(agent.clone(), existing_session_id)?;
host.coordinator().create_hive(HiveInfo {
    hive_id: "engineering".into(),
    name: "Engineering".into(),
    description: None,
    members: vec![agent.id().into()],
})?;
host.coordinator().run_until_idle().await?;
```

Use `register_agent(agent)` when no host conversation exists. Repeated clones
of the same handle are idempotent; a different handle claiming the same agent
ID is rejected. `register_agent_in_session(agent, &str)` validates and durably
binds the session before publishing the runner, so a live scheduler's first
claim sees that session. Storage failure publishes neither binding nor runner.
The attachment source is retained for retries; tools stay inactive until
registration succeeds. `bind_session` remains a coordinator API for separately
registered runners before their first claim.

Keep `OpenHumanHost` alive while its tools are used: it retains supplied handles
and shares the coordinator. Attached factories and tools hold weak host
references, preventing a coordinator/runner/agent/factory ownership cycle.
A surviving agent keeps its definitions after host drop; execution reports
that the service is unavailable.

## Tools and dynamic provisioning

The ordinary permanent tools are `hivemind_list_hives`, `hivemind_list_agents`,
`hivemind_read`, `hivemind_send_hive`, `hivemind_send_agent`, `hivemind_post`,
`hivemind_ask`, `hivemind_broadcast`, and `hivemind_complete`. They remain in the
system catalogue and native provider schemas under deferred discovery. Sender
identity comes from the supplied handle; episode actions require the active
`episode_id`. Membership changes do not add another copy of the definitions.

`hivemind_read` takes exactly one `hive_id` or peer `agent_id`. Hive reads can
include a `thread`; peer reads expose only the two participants' durable direct
messages and returned replies. `after` is an exclusive sequence cursor.
Send operations return receipts immediately. Reading a reply starts no turn;
send an explicit follow-up to continue the exchange.

Host APIs always allow hive creation, supplied-agent registration, and
join/leave. Before registering or cloning the host, call
`with_management(Arc<dyn AgentFactory>, Arc<dyn ManagementAuthorizer>)?` to add
`hivemind_create_hive`, `hivemind_create_agent`, `hivemind_join_hive`, and
`hivemind_leave_hive`. Authorization precedes mutations and factory execution.
The factory validates template/config, retains credentials privately, and
returns a configured agent from the same runtime. Tool config carries nonsecret
settings and references. Failed creation leaves no registered agent.

Joining/leaving affects later claims. Leaving retires unstarted episode seats
through conductor bookkeeping, including after a wave was prepared. Active
turns retain their captured membership. Initiating private messages bound
ordinary output audiences; explicit asks/broadcasts can deliberately delegate.

## Recovery and finalization

SQLite stores a versioned snapshot with a revision compare-and-swap in one
transaction. The snapshot contains messages, memberships, delivery states,
session IDs, and conductor checkpoints; it never serializes live handles.
After restart, construct the host runtime and reattach agents under durable IDs.
Pending jobs become eligible once their handles are registered. Previously
running work becomes interrupted and is not automatically replayed, because
external tool effects may already have occurred. Inspect `interruptions()` and
let host policy decide further work.

Configure progress, usage, approval, and scoped turn handling through
`with_hooks(...)` before registration/sharing. Drain any progress receiver for
the full turn. The adapter's turn wall is 300 seconds. Parked agents require
coordinator `release(agent_id)`; shutdown stops claims and waits for active turns.
If the provider successfully commits its OpenHuman conversation but `after_turn`
fails, the coordinator retains the completed session ID, records interruption,
and suppresses acknowledgements, staged actions, and replies. A later accepted
delivery continues that committed conversation.
