# Hive memory

Where the seats of one hive keep their shared OpenHuman memory.

| File | What it does |
| --- | --- |
| `mod.rs` | `HiveMemory`: the hive's root, per-seat memory agent ids, the optional recall budget, and the registration check |
| `store.rs` | `HiveMemoryStore`: core's `Recall` and `Remember` ports over the same engine and layout |
| `convert.rs` | Context pack to `BriefingNote`s, and `MemoryEntry` to a labelled shared learning |
| `test.rs` | Root and agent id derivation and validation, without a runtime |
| `store_test.rs` | `Recall`/`Remember` over TinyMemory's reference engine, offline |

Registration behavior with a live runtime is tested in
[`../host/memory_test.rs`](../host/memory_test.rs).

## Design

OpenHuman logs every agent's turns through one process-wide engine. Its
automatic recall is a completed-pack prefetch with a cold-cache gap on session
start and after compaction. Manifest deployments disable that prefetch and
synchronously call their captured hive's `HiveMemoryStore` before each
coordinator turn, so the first turn receives the same hive brief as later
turns. The deployment and every hive store receive clones of the same engine
`Arc`.

The legacy host helper decides a hive root and seat memory id. Configured
deployments bind a stable seat memory id under a deployment root, while their
core memory stores remain rooted per hive:

- **Legacy host root.** `HiveMemory::for_hive(id)` gives `team:<id>`. `HiveMemory::with_root`
  accepts any root OpenHuman's `memory::scope::validate_root` accepts. Memory
  under one root is invisible to every other root, so one root per hive (or
  per run) isolates hives from each other. Blank and `root` are refused: they
  name the runtime's default root, which every unbound agent shares.
- **Memory agent id.** A seat's stable id, unchanged. It follows OpenHuman's
  lowercase, 64-character agent-id syntax. Ids are refused rather than
  sanitized, because sanitizing appends a hash the host never wrote.
- **Deployment root.** A configured seat defaults to the logical
  `deployment:<deployment-id>` root. A seat/profile memory root overrides it.
  The root changes with the deployment id, independently of the stable seat id.
- **Recall budget.** Optional, in tokens. It maps to
  `[memory.recall] budget_tokens`. An `AgentSpec` takes only one config closure,
  and binding a spec must not replace the host's. The budget therefore goes on
  the runtime's base config through `HiveMemory::configure`, and seats inherit
  it from there.

## Public surface

```rust,ignore
HiveMemory::for_hive(hive_id: &str) -> Result<HiveMemory>
HiveMemory::with_root(root: impl Into<String>) -> Result<HiveMemory>
HiveMemory::recall_budget_tokens(self, tokens: NonZeroU32) -> HiveMemory
HiveMemory::root(&self) -> &str
HiveMemory::budget(&self) -> Option<NonZeroU32>
HiveMemory::agent_id(&self, seat: &str) -> Result<String>
HiveMemory::binding(&self, seat: &str) -> Result<MemoryBinding>
HiveMemory::bind(&self, spec: AgentSpec) -> Result<AgentSpec>
HiveMemory::configure(&self, config: &mut RuntimeConfig)
```

Errors: `Error::InvalidMemoryRoot` and `Error::InvalidMemoryAgentId` come from
derivation. `Error::UnboundSeat` comes from registration, when a seat's built
config does not carry the hive's agent id, root, or budget.

## Core `Recall` and `Remember`

`HiveMemoryStore` implements `tinyhivemind_core::runtime::{Recall, Remember}`.
It reads and writes through TinyMemory's `AgentMemory` at the hive's root, with
the seat id as the memory agent id. That is the namespace OpenHuman's own
lifecycle uses for a seat bound by `HiveMemory::bind`, so a core recall sees
what bound seats logged, and every bound seat recalls what a core remember
wrote.

```rust,ignore
HiveMemoryStore::new(engine: Arc<dyn MemoryEngine>, hive: HiveMemory) -> Result<HiveMemoryStore>
HiveMemoryStore::from_config(config: &RuntimeConfig, hive: HiveMemory) -> Result<HiveMemoryStore>
HiveMemoryStore::with_policy(self, policy: RecallPolicy) -> HiveMemoryStore
HiveMemoryStore::hive(&self) -> &HiveMemory
HiveMemoryStore::policy(&self) -> &RecallPolicy
```

`from_config` binds the engine exactly as OpenHuman does, through
`memory::engine::resolve`. With `engine = "cortexdb"` that means the endpoint
from the config, the key from the keychain, and the guard that scrubs writes.
It takes the `[memory.recall]` policy. `new` takes any engine, such as one the
host already holds or the reference engine in tests.

| `RecallMoment` | Lifecycle call | Notes come from |
| --- | --- | --- |
| `SessionStart` | `start_session`, with `conversation` as the thread | the thread's turns, learnings, brain, the seat's history, the team's turns |
| `Rejoin` | `start_session` with no history section | learnings, brain, the team's turns; anything the seat itself wrote is dropped, since it is already in its session |
| `Compaction { dropped }` | `recall_for_compaction`, with the dropped text as turns | a summary of the thread, then the standard sections |

Each pack section that found something becomes one `BriefingNote`, headed by
the section (`Learnings`, `Team conversations`, ...). Its answer and hits
become single lines, capped in total at `budget_chars`, and the pack's own token
budget is lowered to match.

`Remember` stores each `MemoryEntry` as a learning at the hive root, where every
seat's learnings section reads it. The text is labelled with its kind
(`Observation: ...`, `Failed attempt: ...`, `Outcome: ...`, `Note: ...`) and
tagged `hive-entry:<kind>`, plus `desk-through:<n>` when the request carries a
watermark. A failed attempt is stored as a `Correction` learning, observations
and outcomes as `Fact`, and notes as `Other`. Thread and agent ids are kept in
the item's metadata.

Errors: a blank conversation, an unusable seat id, a blank entry, or an engine
failure map to `runtime::Error::Recall { source }` or
`runtime::Error::Remember { source }`. The source is the TinyMemory or adapter
error. Note that TinyMemory degrades a failing section to an absent one rather
than failing the whole recall.

### Coordinator-turn injection

Manifest deployments add a `TurnHooks::context_async` step before constructing
the native turn. It recalls `SessionStart` the first time a seat enters a hive
session and `Rejoin` on later turns, then places the returned notes in the new
user turn. The notes are not committed as transcript rows. OpenHuman's native
prefetch is disabled for those deployments because it cannot distinguish the
active hive. Native seat cron jobs run outside this hook and therefore do not
receive a hive-specific brief.

Legacy hosts using `HiveMemory::bind` retain OpenHuman's native lifecycle and
cached-pack behavior. A cold cache can still supply no pack; those hosts can
call the core port directly for session start, rejoin or compaction. Structured
`Remember` entries remain a core-port operation because OpenHuman logs turns
but does not create these learnings itself.

## Operational constraints

The engine and its credential belong to the host:
`[memory] engine = "cortexdb"`, `[memory.engines.cortexdb] endpoint`, and the
CortexDB key in the keychain under `memory-cortexdb`. The offline fixtures
switch recall and conversation logging off, because no engine answers there.
