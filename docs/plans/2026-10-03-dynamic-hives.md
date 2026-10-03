# Implement dynamic hives and supplied OpenHuman agents

Accepted behavior: [dynamic-hives spec](../specs/dynamic-hives.md).

## Interface contract

Identifiers below are owned nonempty `String`s. Only `hive_id` identifies a
hive/desk. `runtime_id` identifies the shared runtime, not an agent. Core's
existing `BoundAgent::runtime_id` continues to mean its bound agent handle ID;
the adapter translates deliberately rather than conflating these two notions.
Public payload types derive serde, clone, debug, and equality where possible.
Ports are object-safe, `Send + Sync`, and use typed coordinator `Result<T>`.

### OpenHuman attachment seam

- `Agent::runtime_id(&self) -> &str`: opaque identity generated once per runtime,
  shared by all its agents and clones; distinct runtimes never compare equal.
- `Agent::same_agent(&self, other: &Agent) -> bool`: shared handle identity.
- `Agent::attach_tools(&self, key: impl Into<String>, factory: HostTools)
  -> Result<(), ToolAttachmentError>`. `HostTools` is the existing shared
  factory type. Named attachments share across clones; registering the identical
  source `Arc` under the same key is idempotent. A different source under that
  key fails rather than replacing it. The adapter caches its attachment source.
- Add `HostTurnTools::permanent: HashSet<String>`, defaulting to empty, alongside
  its existing boxed tools and policy. Names in this set are advertised in the
  prompt and provider schemas even under deferred packing or a base factory
  advertising policy. Existing base tools and their policy stay preserved.
- Continuing sessions refresh one managed catalogue section per attachment.
  Existing prompt content and transcript messages survive refresh. Reject
  name collisions before model invocation. First-party turn setup must merge
  the original agent tool factory with all attached factories each turn.

### Coordinator seam

The new crate exports `Coordinator`, `CoordinatorOptions`, `AgentRegistration`,
`AgentRunner`, `TurnFuture`, the payloads below, storage ports/implementations,
and typed `Error`/`Result`. `Coordinator` is cloneable and shares one state.

```rust,ignore
type TurnFuture = Pin<Box<dyn Future<Output = Result<TurnOutcome>> + Send>>;
trait AgentRunner: Send + Sync {
    fn run(&self, request: TurnRequest) -> TurnFuture;
}
struct AgentRegistration {
    agent_id: String,
    runtime_id: String,
    runner: Arc<dyn AgentRunner>,
}
struct TurnRequest {
    agent_id: String,
    session_id: Option<String>,
    messages: Vec<Message>,
    memberships: Vec<HiveInfo>,
    episode: Option<EpisodeContext>,
}
struct TurnOutcome {
    session_id: String,
    reply: Option<String>,
    disposition: TurnDisposition,
}
enum TurnDisposition { Completed, Parked, Failed(String) }
struct EpisodeContext {
    episode_id: String,
    hive_id: String,
    thread: Option<u64>,
    brief: String,
}
struct HiveInfo {
    hive_id: String,
    name: String,
    description: Option<String>,
    members: Vec<String>,
}
enum Destination { Hive(String), Agent(String) }
struct Message {
    message_id: String,
    sequence: u64,
    sender: String,
    destination: Destination,
    body: String,
    thread: Option<u64>,
    episode_id: Option<String>,
    only_for: Vec<String>,
}
struct SendMessage {
    message_id: String,
    sender: String,
    destination: Destination,
    body: String,
    thread: Option<u64>,
    only_for: Vec<String>,
}
struct Receipt { message_id: String, sequence: u64 }
```

Expose these operations (all fallible except shutdown request):

- `Coordinator::new(runtime_id: String, storage: Arc<dyn Storage>,
  options: CoordinatorOptions) -> Result<Self>` loads durable state.
- `register_agent(AgentRegistration) -> Result<()>`: validate runtime; repeat
  registration is idempotent only for the same `Arc` runner. Reattachment after
  restart binds an existing durable agent ID to its supplied runtime handle.
- `register_agent_in_session(AgentRegistration, session_id: &str) -> Result<()>`:
  validate and commit an existing conversation binding before publishing the
  runner or notifying schedulers. `bind_session(agent_id, session_id)` remains
  available for separately registered runners before their first claim.
- `create_hive(HiveInfo) -> Result<()>`, `list_hives() -> Result<Vec<HiveInfo>>`,
  `list_agents() -> Result<Vec<String>>`; creation requires known members and
  rejects conflicting IDs; identical hive creation is idempotent.
- `join_hive(hive_id: &str, agent_id: &str) -> Result<()>` and
  `leave_hive(hive_id: &str, agent_id: &str) -> Result<()>` are idempotent.
- `send(SendMessage) -> Result<Receipt>` atomically validates and enqueues.
  Identical message ID/payload returns the existing receipt; changed payload
  under the same ID fails. Sender must be registered; hive send requires its
  membership. Hosts submit through an explicit `send_as_host` counterpart,
  whose sender is the reserved host identity and cannot be selected by tools.
- `read_hive(agent_id: &str, hive_id: &str, after: Option<u64>,
  thread: Option<u64>) -> Result<Vec<Message>>` filters visibility.
- `read_direct(agent_id: &str, peer_id: &str, after: Option<u64>)
  -> Result<Vec<Message>>` exposes the durable participant pair and returned
  replies. The exclusive cursor does not schedule automatic return turns.
- `submit_action(agent_id: &str, episode_id: &str,
  action: EpisodeAction) -> Result<()>` records a validated active-turn action.
  `EpisodeAction` is `Post { body }`, `Ask { agents, body }`,
  `Broadcast { body }`, or `Complete { body }`; the adapter translates these to
  existing core `ToolCall`s with the captured chat/thread on turn closure.
- `run_until_idle(&self) -> async Result<RunReport>` drains eligible work;
  `run(&self) -> async Result<()>` waits for work until shutdown;
  `shutdown(&self)` wakes the scheduler and prevents new claims.
- `release(agent_id: &str) -> Result<()>` resumes a parked agent after host
  approval; `interruptions() -> Result<Vec<InterruptedTurn>>` exposes recovery
  failures without automatic retries. Host progress/usage callbacks live in
  `CoordinatorOptions` or the adapter, never in pure core.

Defaults: round width one, conductor defaults for walls, no management tools,
no automatic replay of interrupted turns. Options expose nonzero round width
and existing conduct policy. The scheduler deduplicates active agents globally,
captures membership snapshots, and does not hold locks while awaiting a runner.
For an active episode it must execute `Conductor::begin_wave`, `turns`,
`open_turn`, `record`, and drain `step`/`committed`, saving its actual state.
Standalone direct-agent deliveries have no episode context. A shared agent's
different episode jobs are never merged into one control context.
Scheduling chooses oldest eligible durable work first, breaking round ties by
agent ID, and retains queued positions for busy agents. Failure/cancellation
releases active agent and episode reservations and records interruption before
rescheduling other work. Only the captured active episode accepts actions;
stale episode IDs cannot target a previous assignment.

### Storage seam

```rust,ignore
trait Storage: Send + Sync {
    fn load(&self) -> Result<StoredState>;
    fn commit(&self, expected_revision: u64, next: &StoredState) -> Result<()>;
}
```

`StoredState` is a documented serializable snapshot containing revision, hive
definitions, agent/session records, messages, inbox delivery states, episode
conductor checkpoints, pending actions, and running/interrupted turn records.
It contains no runner handles. Commit atomically replaces state only when the
stored revision equals `expected_revision`; next revision must be old plus one.
CAS conflict leaves storage unchanged. Coordinator state updates are applied
only after commit succeeds. Export `MemoryStorage::new()` and feature-gated
`SqliteStorage::open(path)`; SQLite uses one transaction for revision check and
snapshot write, with a schema version. Both share the same contract tests.

### Supplied-agent adapter and management seam

- Export `OpenHumanHost::new(runtime_id: String, coordinator: Coordinator) -> Result<Self>` and
  `register_agent(&self, agent: Agent) -> Result<()>`. It validates the actual
  runtime ID, caches handle/runner pairs, installs `hivemind` once, then
  registers. Duplicate supplied clones are idempotent; another handle with
  the same ID is rejected. Runner calls `Agent::turn(...).session(...)` and
  stores the returned session ID. It never reseeds or clears the transcript.
- Export `AgentFactory` with boxed async `create(template: String,
  config: serde_json::Value) -> Result<Agent>`, and `ManagementAuthorizer`
  with `authorize(actor: &str, request: &ManagementRequest) -> Result<()>`.
  `ManagementRequest` covers hive creation, agent creation, join, and leave.
- `OpenHumanHost::with_management(factory, authorizer)` enables optional tools;
  returned agents pass the same registration path. Management is configured
  before agent registration. Creation authorization precedes factory execution.
- Stable base tools: `hivemind_list_hives`, `hivemind_list_agents`,
  `hivemind_read`, `hivemind_send_hive`, `hivemind_send_agent`,
  `hivemind_post`, `hivemind_ask`, `hivemind_broadcast`, `hivemind_complete`.
  Episode operations require `episode_id`. Reads/sends use `hive_id` and
  optional `thread`; direct sends use `agent_id`. Send tools accept a caller
  message ID for retry deduplication. Bound sender never appears in schema.
- Optional tools: `hivemind_create_hive`, `hivemind_create_agent`,
  `hivemind_join_hive`, `hivemind_leave_hive`. Agent creation takes a host
  template and JSON config; the host validates template/config requirements.
  Definitions and execution arguments come from the same tool specification.
  Host templates retain credentials privately; JSON tool config contains only
  nonsecret configuration or host-resolved references.

## Ordered implementation tasks

1. **TinyAgents prefix refresh dependency.** Add the generic opt-in
   `PrefixSnapshot::refreshing()` capability needed to refresh an attached
   catalogue in a continuing session. Existing snapshots retain their current
   behavior by default. Add focused continuing-session regression coverage and
   open its upstream dependency PR before the OpenHuman PR.
2. **OpenHuman attachments and sessions.** First add failing request-capture
   tests for an attachment added to a supplied agent and for two turns in one
   session. Cover clone sharing, factory composition, deferred packing,
   duplicate keys, tool collisions, policies, and unrelated catalogue content.
   Implement the seam above and fix transcript binding only behind a reproducer.
   Verify embed/core tests and publish the upstream OpenHuman branch/PR.
3. **Coordinator and storage.** Add the new crate without touching core's purity
   policy. Start with registration and memory storage tests, then transactional
   send/deduplication, visibility, membership snapshots, and global agent
   exclusion. Integrate the real conductor before declaring scheduling done.
   Add SQLite and shared contract tests, persisted sessions, interruption
   recovery, parked release, and running shutdown tests. Use separate focused
   `types.rs` and `test.rs` modules with directory READMEs.
4. **Supplied-agent adapter.** Implement the supplied-handle wrapper and native
   permanent tools against the fixed seams. Verify actual requests, not just
   internal specifications. Preserve approval/progress/usage entry points.
   Replace embed/raw construction runners and their registry generators;
   update dependent callers rather than retaining an alternate construction
   architecture. Add factory/authorization failure tests and dynamic tools.
5. **Examples, documentation, delivery.** Standalone example constructs one
   runtime and distinct MCP/skill/memory/prompt configurations before handoff.
   Demonstrate all four topology shapes and live dynamic provisioning with
   deterministic offline doubles. Add OpenCompany migration guidance. Update
   OpenHuman dependency pin only after its commit is available upstream and
   keep it consistent with the vendor gitlink and pin check. Open ready upstream
   PRs; a TinyHivemind PR blocked on an unmerged OpenHuman dependency is draft
   with that dependency explicitly stated until it can become ready.

## Verification gates

- Memory/SQLite storage contract: CAS, atomic failure, message retry, reopen,
  session restoration, pending recovery, and interrupted work without replay.
- Coordinator: all four topology shapes, actual conductor asks/child completion,
  broadcast budget, turn walls, approvals, private reads, global exclusion,
  dynamic membership/factory errors, and shutdown while an agent runs.
- Adapter/OpenHuman: successive request captures retain agent configuration,
  history and permanent schemas exactly once, even with deferred tools enabled.
- Run `cargo fmt --all -- --check`, clippy/build/test with all targets/features,
  rustdoc with warnings denied, doctests, purity script, and OpenHuman pin script.
  Standalone examples have their own manifests and must be built/tested there.
- Fresh independent completion verification precedes final success claims.
  Record actual command results and any intentionally uncovered edge cases in
  PR bodies. Preserve automatic checkpoint commits; never squash.

## Progress tracker

- [x] TinyAgents opt-in prefix refresh and upstream dependency PR (#298).
- [x] OpenHuman attachment/session seam and request-capture regressions (#6977).
- [x] Coordinator, conductor, storage, and deterministic scheduling contracts.
- [x] Supplied-agent adapter, permanent tools, and dynamic management.
- [x] Standalone examples and native configuration/topology proofs.
- [x] Migration docs, public API docs, and superseding design links.
- [x] Independent implementation reviews and recorded verification gates.
- [x] Final delivery checks after the last dependency review correction.
- [x] TinyHivemind upstream PR (#95) and dependency pin/gitlink publication.

Recorded verification is local evidence, not a claim that upstream CI has passed.
The final TinyAgents legacy-prefix correction adds first/repeated resume and
persisted-boundary coverage; it passed independent review and was published
before the OpenHuman and TinyHivemind dependency pins were updated.

Delivery: [TinyHivemind #95](https://github.com/tinyhumansai/tinyhivemind/pull/95) is a draft until TinyAgents #298 and OpenHuman #6977 land. The dependency PRs must land before the final merge pins are selected.
