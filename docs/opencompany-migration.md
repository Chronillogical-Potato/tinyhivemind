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
)
.await?;
let host = OpenHumanHost::new(runtime.runtime_id().into(), coordinator)?;
// The host has already configured and, optionally, used this Agent.
host.register_agent_in_session(agent.clone(), existing_session_id).await?;
host.coordinator().create_hive(HiveInfo {
    hive_id: "engineering".into(),
    name: "Engineering".into(),
    description: None,
    members: vec![agent.id().into()],
}).await?;
host.coordinator().run_until_idle().await?;
```

Every coordinator call that commits is `async`: construction, registration,
`bind_session`, hive creation and membership, sends, `submit_action`, and
`release`/`release_with`. Reads (`list_*`, `read_*`, `episodes`,
`interruptions`) stay synchronous. The futures name no executor.

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

`Storage` is an async port. Each commit hands the store the next bounded state
row (memberships, delivery states, session IDs, conductor checkpoints) and only
the transcript rows appended since the expected revision. The store applies
both in one transaction behind a revision compare-and-swap, and never
serializes live handles. SQLite (schema 2) keeps the row in
`hivemind_snapshot` and the transcript in an append-only `hivemind_messages`
table, and migrates a version-one file on open. A `MongoDB` store mirrors this
shape: one state document plus one document per message, so no document
approaches the 16 MB cap. `CoordinatorOptions::retention` bounds the settled
episodes and acknowledged deliveries the row keeps. The transcript is never
pruned. A revision conflict from another process is absorbed by reload and
retry.
After restart, construct the host runtime and reattach agents under durable IDs.
Pending jobs become eligible once their handles are registered. Previously
running work becomes interrupted and is not automatically replayed, because
external tool effects may already have occurred. Inspect `interruptions()` and
let host policy decide further work.

Configure progress, usage, approval, and scoped turn handling through
`with_hooks(...)` before registration/sharing. Drain any progress receiver for
the full turn. The adapter's turn wall defaults to 300 seconds
(`with_turn_timeout`). Parked agents require coordinator `release(agent_id)`
or `release_with(agent_id, note)`; shutdown stops claims and waits for active
turns.
If the provider successfully commits its OpenHuman conversation but `after_turn`
fails, the coordinator retains the completed session ID, records interruption,
and suppresses acknowledgements, staged actions, and replies. A later accepted
delivery continues that committed conversation.

## Host seams

These are configured on `OpenHumanHost` before registration or sharing, or
called on its coordinator.

- **Turn scope.** `TurnHooks::{prepare, progress, wrap_turn, after_turn}`
  receive `&TurnScope`: the agent, the active `EpisodeContext`, the delivered
  message ids, their distinct senders, the destination (the hive for an
  episode turn), and the thread. Route live progress, approvals and file cards
  per hive, episode or thread with it. `prepare` returns `TurnOptions { cwd }`,
  which the runner applies through OpenHuman's `Turn::cwd`.
- **Host transcript.** `coordinator().read_transcript(after)` returns every row
  in sequence order, including private ones and the replies to `send_as_host`
  (destination `Agent(HOST_ID)`). `subscribe()` is a
  `watch::Receiver<u64>` of the committed revision, which every commit
  advances. Await `changed()`, then read the transcript from your cursor,
  `episodes()` (each episode's `EpisodePhase`: `Open`, `AwaitingRelease`,
  `Settled` or `Failed`), and `interruptions()`.
- **Starters.** `SendMessage::starters` names the hive members who start the
  episode. The message stays visible to every reader, unlike `only_for`.
  Starters must be distinct readers of the message. Empty keeps the previous
  behaviour.
- **Release notes.** `release_with(agent, Some(note))` delivers `note` once, as
  `TurnRequest::resumption` on the agent's next claimed turn. The runner puts it
  at the top of the prompt as a host resumption note.
- **Replacing a handle.** `replace_agent(agent_id, || Ok(runtime.agent(spec)?))`
  waits for any running turn, drops the adapter's handle, and then builds the
  replacement, because OpenHuman ids are unique while any clone is alive. Drop
  your own clones first. The session binding and queued work carry over, and
  the hivemind tools are reattached.
- **Send policy.** `with_send_policy(Arc<dyn SendAuthorizer>)` is consulted
  with a `SendRequest` before `hivemind_send_agent`, `hivemind_send_hive`,
  `hivemind_ask` and `hivemind_broadcast` run. A refusal (conventionally
  `Error::SendDenied`) reaches the model as the tool's error text, and the turn
  continues.

The decision record is
[ADR 0031](adr/0031-expose-host-seams-over-async-incremental-storage.md).
