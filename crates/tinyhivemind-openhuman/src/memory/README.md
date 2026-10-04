# Hive memory

Where the seats of one hive keep their shared OpenHuman memory.

| File | What it does |
| --- | --- |
| `mod.rs` | `HiveMemory`: the hive's root, per-seat memory agent ids, the optional recall budget, and the registration check |
| `test.rs` | Root and agent id derivation and validation, without a runtime |

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

## Operational constraints

The engine and its credential belong to the host:
`[memory] engine = "cortexdb"`, `[memory.engines.cortexdb] endpoint`, and the
CortexDB key in the keychain under `memory-cortexdb`. The offline fixtures
switch recall and conversation logging off, because no engine answers there.
