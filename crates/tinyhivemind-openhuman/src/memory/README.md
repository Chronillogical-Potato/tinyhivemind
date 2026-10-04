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

OpenHuman already runs a memory lifecycle around every agent turn. Before the
model runs it recalls a context pack, including at the start of a session and
after a compaction. After the turn commits it logs the turn. When the transcript
is compacted it recalls what the dropped turns carried. Each agent acts as one
memory agent id under one layout root, set per agent by
`openhuman_embed::AgentSpec::memory(MemoryBinding::new(id).root(root))`.

This module decides those two values for a hive's seats:

- **Root.** `HiveMemory::for_hive(id)` gives `team:<id>`. `HiveMemory::with_root`
  accepts any root OpenHuman's `memory::scope::validate_root` accepts. Memory
  under one root is invisible to every other root, so one root per hive (or
  per run) isolates hives from each other. Blank and `root` are refused: they
  name the runtime's default root, which every unbound agent shares.
- **Memory agent id.** A seat's seat id, unchanged. It must be 1 to 128
  characters of `A-Za-z0-9_-`, the TinyMemory segment charset, so the engine
  stores it as written. Every OpenHuman agent id qualifies. Ids are refused
  rather than sanitized, because sanitizing appends a hash the host never wrote.
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

### Why the runner does not inject a framed block

A seat bound with `HiveMemory::bind` already gets memory in its prompt on every
turn, from OpenHuman itself. `runtime_session/memory_ingest.rs` calls
`memory::lifecycle::hooks::pre_turn`, and `MemoryPackMiddleware` attaches the
resulting pack to that turn's model requests. The pack is never committed to
the transcript. A session's first turn is a turn like any other. After a
compaction, `memory_summarizer.rs` calls `hooks::compaction`, and the next
pre-turn also runs `start_session`. If the runner also rendered
`RecalledSession::framed`, the seat would receive the same memory twice.

The core port is for the paths OpenHuman does not run:

- a host that opens seat sessions itself through
  `initialize_session_with_recall`, outside an OpenHuman turn;
- a host that turns per-turn recall off (`[memory.recall] enabled = false`) and
  recalls only at the core moments;
- structured `Remember` entries (observations, failed attempts, outcomes).
  OpenHuman logs turns but writes no such learnings.

## Operational constraints

The engine and its credential belong to the host:
`[memory] engine = "cortexdb"`, `[memory.engines.cortexdb] endpoint`, and the
CortexDB key in the keychain under `memory-cortexdb`. The offline fixtures
switch recall and conversation logging off, because no engine answers there.
