//! Wire types for `constitution.json` and `memory.json`.
use crate::patch::PatchClass;
use serde::{Deserialize, Serialize};
use serde_json::Value;
/// The host-pinned constitution decoded from `constitution.json`.
///
/// It caps what the manifest may ask for and fixes the rules editor proposals
/// are judged by. [`crate::patch::guard`] rejects any proposal whose candidate
/// changes it ([`crate::patch::Error::Immutable`]). The default caps are 60
/// turns and a width of 8, with no edit class permitted.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Constitution {
    /// Requirements stated in prose, opaque to this crate; the host evaluator
    /// enforces them.
    pub hard_constraints: Vec<String>,
    /// Ceiling for the episode `turn_budget` and every conduct `turn_wall`;
    /// must be nonzero.
    pub max_turns: u64,
    /// Ceiling for every round width the manifest declares (episode revealed
    /// width, routing, division and coordinator); must be nonzero.
    pub max_width: usize,
    /// Patch classes an editor proposal may use; a proposal of any other class
    /// fails with [`crate::patch::Error::ForbiddenClass`].
    pub permitted_edits: Vec<PatchClass>,
    /// Memory identity ids pinned read-only. Each must name a declared
    /// identity whose `read_only` is true, and no proposal may change that
    /// identity, its bindings, or learn into it.
    pub read_only_memories: Vec<String>,
    /// Namespaces no memory identity or read reach may name, sit under, or
    /// (with `descendants`) reach down into; for example an evaluator's
    /// held-out store.
    pub protected_namespaces: Vec<String>,
    /// Strings that must not appear anywhere in a proposed manifest, memory
    /// spec, document, or patch; a match fails with
    /// [`crate::patch::Error::Contamination`].
    pub contamination_canaries: Vec<String>,
    /// Evaluator configuration, opaque to this crate and enforced by the host.
    pub evaluation: Value,
    /// Acceptance configuration, opaque to this crate and enforced by the host.
    pub acceptance: Value,
    /// Telemetry configuration, opaque to this crate and enforced by the host.
    pub telemetry: Value,
}
impl Default for Constitution {
    fn default() -> Self {
        Self {
            hard_constraints: vec![],
            max_turns: 60,
            max_width: 8,
            permitted_edits: vec![],
            read_only_memories: vec![],
            protected_namespaces: vec![],
            contamination_canaries: vec![],
            evaluation: Value::Null,
            acceptance: Value::Null,
            telemetry: Value::Null,
        }
    }
}
/// The memory declaration decoded from `memory.json`.
///
/// It names where memory lives and who may read or write it. It never holds
/// remembered entries; those stay in the host's memory engine.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemorySpec {
    /// Namespace that owns the package's memory tree, for example
    /// `team:hive`. Every identity namespace and read reach must equal it or
    /// sit below it.
    pub root: String,
    /// Declared memories, private to an agent or shared as a pool.
    pub identities: Vec<MemoryIdentity>,
    /// Per-seat bindings; a seat has at most one.
    pub bindings: Vec<SeatMemory>,
}
impl Default for MemorySpec {
    fn default() -> Self {
        Self {
            root: "team:hive".into(),
            identities: vec![],
            bindings: vec![],
        }
    }
}
/// One declared memory: a namespace, who owns it, and how long it lives.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryIdentity {
    /// The memory's identity, unique among identities and independent of any
    /// seat id; bindings, read-only pins, forks, and learning refer to it.
    pub id: String,
    /// Full namespace path, unique among identities, in the syntax
    /// [`crate::validate_namespace`] accepts, at or below
    /// [`MemorySpec::root`], and outside every protected namespace. An
    /// [`MemoryKind::Agent`] namespace must differ from the root and end in
    /// an `agent:<id>` segment.
    ///
    /// Lowering derives [`crate::LoweredMemoryBinding::agent_id`] from the
    /// text after the last `:` and [`crate::LoweredMemoryBinding::root`] from
    /// the text before the last `/` (the spec root when there is none).
    pub namespace: String,
    /// Whether this is one agent's private memory or a shared pool.
    pub kind: MemoryKind,
    /// Whether the memory lasts for one run or for the life of the hive.
    pub lifecycle: MemoryLifecycle,
    /// Whether the host must refuse writes to this memory; must be true when
    /// the constitution pins the identity read-only.
    pub read_only: bool,
    /// Identity this memory forks from; it must be declared, and this
    /// namespace must sit below the parent's.
    pub fork_parent: Option<String>,
}
/// One seat's memory binding: what it writes to, reads, and when it recalls.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SeatMemory {
    /// Identity of the bound seat; must name a declared seat, and no other
    /// binding may name it.
    pub seat: String,
    /// Identity of the [`MemoryIdentity`] the seat writes to; must be declared.
    pub identity: String,
    /// Namespaces the seat may recall from, each bounded within the root.
    pub reads: Vec<MemoryReach>,
    /// The moments at which the host recalls memory into the seat's session.
    pub recall_at: Vec<RecallMoment>,
    /// Most characters one recall may return; must be nonzero.
    pub budget_chars: usize,
    /// Kinds of entry the host may store for the seat after an activation.
    pub remember: Vec<EntryKind>,
}
/// One namespace a seat may recall from.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct MemoryReach {
    /// Namespace the read starts at; at or below [`MemorySpec::root`] and
    /// outside every protected namespace.
    pub at: String,
    /// Whether ancestors of `at` are read too. Must be false: engine
    /// inheritance would include the global root. List each permitted
    /// ancestor as its own reach instead.
    pub inherit: bool,
    /// Whether namespaces below `at` are read too. When true, no protected
    /// namespace may sit below `at`.
    pub descendants: bool,
}
/// Who a memory belongs to.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    /// One agent's private memory, at an `agent:<id>` namespace.
    Agent,
    /// A pool several seats share.
    Pool,
}
/// How long a memory lasts.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryLifecycle {
    /// Scoped to one run; it is not part of the evaluated design.
    Run,
    /// Persists across runs, so it is part of the evaluated design:
    /// [`crate::lineage::content_id`] requires a watermark for it.
    Hive,
}
/// A moment in a seat's session at which the host recalls memory.
///
/// These mirror the core runtime's recall moments, without their payloads.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallMoment {
    /// The seat's session is opened for the first time in a run.
    SessionStart,
    /// The seat rejoins a session it already holds, after peers worked
    /// without it.
    Rejoin,
    /// The host compacted the seat's session and recalls what was dropped.
    Compaction,
}
/// A kind of entry a seat may leave in memory, mirroring the core runtime's.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// Something the seat observed, such as a command and its output.
    Observation,
    /// An approach that did not work, so nobody tries it again blind.
    FailedAttempt,
    /// A result the seat reached.
    Outcome,
    /// Anything else worth keeping.
    Note,
}
