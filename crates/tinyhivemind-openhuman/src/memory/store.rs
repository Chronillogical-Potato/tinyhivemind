//! Core's [`Recall`] and [`Remember`] ports over the engine and layout
//! `OpenHuman`'s seats use.
//!
//! [`HiveMemoryStore`] reads and writes through `TinyMemory`'s
//! [`AgentMemory`] at the hive's root with the seat id as memory agent id —
//! the same namespace `OpenHuman`'s per-turn lifecycle uses for a seat bound
//! by [`HiveMemory::bind`] — so a recall sees what bound seats logged, and
//! what a remember writes, every bound seat recalls.
use super::{
    HiveMemory,
    convert::{EntryContext, entry_item, notes},
};
use crate::{Error, Result};
use openhuman_embed::RuntimeConfig;
use std::sync::Arc;
use tinyhivemind_core::runtime::{
    self, BriefingNote, Recall, RecallFuture, RecallMoment, RecallRequest, Remember,
    RememberFuture, RememberRequest, SourceError,
};
use tinymemory_api::{MAX_STORE_MANY, MemoryEngine, Namespace, Role, Turn};
use tinymemory_tools::{AgentMemory, Compaction, MemoryLayout, RecallPolicy, SessionStart};

/// Characters per token in `TinyMemory`'s budget estimate.
const CHARS_PER_TOKEN: usize = 4;

/// A hive's memory as core's [`Recall`] and [`Remember`] ports.
///
/// Recall maps each [`RecallMoment`] onto the lifecycle call `OpenHuman`
/// itself makes at that point:
///
/// | Moment | Call | Reads |
/// | --- | --- | --- |
/// | `SessionStart` | [`AgentMemory::start_session`] with the conversation as thread | the thread's turns, learnings, brain, the seat's history, the team's turns |
/// | `Rejoin` | [`AgentMemory::start_session`] without the seat's own history | learnings, brain, the team's turns |
/// | `Compaction` | [`AgentMemory::recall_for_compaction`] with the dropped text as turns | a summary of the thread, then the standard sections |
///
/// Remember stores each [`MemoryEntry`](tinyhivemind_core::runtime::MemoryEntry)
/// as a learning at the hive's root, where every seat's learnings section
/// reads it, labelled with its kind (`Failed attempt: …`) and tagged
/// `hive-entry:<kind>`.
#[derive(Clone)]
pub struct HiveMemoryStore {
    engine: Arc<dyn MemoryEngine>,
    layout: MemoryLayout,
    hive: HiveMemory,
    policy: RecallPolicy,
}

impl std::fmt::Debug for HiveMemoryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HiveMemoryStore")
            .field("engine", &self.engine.descriptor().id)
            .field("root", &self.hive.root())
            .field("policy", &self.policy)
            .finish()
    }
}

impl HiveMemoryStore {
    /// The hive's memory on `engine`, with `TinyMemory`'s default recall
    /// policy and the hive's recall budget when it has one.
    ///
    /// # Errors
    /// [`Error::InvalidMemoryRoot`] when the root cannot be a layout root.
    pub fn new(engine: Arc<dyn MemoryEngine>, hive: HiveMemory) -> Result<Self> {
        let invalid = |reason: String| Error::InvalidMemoryRoot {
            root: hive.root().to_owned(),
            reason,
        };
        let root: Namespace = hive
            .root()
            .parse()
            .map_err(|error: tinymemory_api::Error| invalid(error.to_string()))?;
        let layout = MemoryLayout::new(root).map_err(|error| invalid(error.to_string()))?;
        let mut policy = RecallPolicy::default();
        if let Some(tokens) = hive.budget() {
            policy.budget_tokens = usize::try_from(tokens.get()).unwrap_or(usize::MAX);
        }
        Ok(Self {
            engine,
            layout,
            hive,
            policy,
        })
    }

    /// The hive's memory on the engine `OpenHuman` binds for `config` — the
    /// engine its seats' lifecycle writes through — under `config`'s
    /// `[memory.recall]` policy (the hive's recall budget, when set, wins).
    ///
    /// # Errors
    /// [`Error::Memory`] when `config` binds no engine: memory off, or the
    /// `CortexDB` key missing from the keychain. [`Error::InvalidMemoryRoot`]
    /// as [`Self::new`].
    pub fn from_config(config: &RuntimeConfig, hive: HiveMemory) -> Result<Self> {
        let bound = openhuman_core::memory::engine::resolve(config).engine()?;
        let mut policy = openhuman_core::memory::lifecycle::policy(&config.memory.recall);
        if let Some(tokens) = hive.budget() {
            policy.budget_tokens = usize::try_from(tokens.get()).unwrap_or(usize::MAX);
        }
        Ok(Self::new(bound.engine, hive)?.with_policy(policy))
    }

    /// The same store under `policy`.
    #[must_use]
    pub fn with_policy(mut self, policy: RecallPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// The hive this store reads and writes.
    #[must_use]
    pub fn hive(&self) -> &HiveMemory {
        &self.hive
    }

    /// The recall policy.
    #[must_use]
    pub fn policy(&self) -> &RecallPolicy {
        &self.policy
    }

    /// `seat`'s memory, with packs held to `budget_chars`.
    fn seat(&self, seat: &str, budget_chars: usize) -> std::result::Result<AgentMemory, SourceError> {
        let agent_id = self.hive.agent_id(seat)?;
        let budget_tokens = budget_chars.div_ceil(CHARS_PER_TOKEN).max(1);
        let policy = RecallPolicy {
            budget_tokens: self.policy.budget_tokens.min(budget_tokens),
            ..self.policy.clone()
        };
        Ok(AgentMemory::new(self.engine.clone(), self.layout.clone(), &agent_id)?.with_policy(policy))
    }

    async fn read(&self, request: &RecallRequest) -> std::result::Result<Vec<BriefingNote>, SourceError> {
        let conversation = conversation(&request.conversation)?;
        let memory = self.seat(&request.seat, request.budget_chars)?;
        let focus = request.focus.clone();
        let pack = match &request.moment {
            RecallMoment::SessionStart => {
                let start = SessionStart {
                    thread_id: Some(conversation.to_owned()),
                    focus,
                };
                memory.start_session(start).await?
            }
            RecallMoment::Rejoin => {
                // The seat's own turns are already in its session.
                let policy = RecallPolicy {
                    history_limit: 0,
                    ..memory.policy().clone()
                };
                let start = SessionStart {
                    thread_id: None,
                    focus,
                };
                memory.with_policy(policy).start_session(start).await?
            }
            RecallMoment::Compaction { dropped } => {
                let compaction = Compaction {
                    thread_id: conversation.to_owned(),
                    dropped: dropped
                        .iter()
                        .map(|text| Turn::new(Role::Assistant, text.as_str()))
                        .collect(),
                    focus,
                };
                memory.recall_for_compaction(compaction).await?
            }
        };
        Ok(notes(&pack, request.budget_chars))
    }

    async fn write(&self, request: &RememberRequest) -> std::result::Result<(), SourceError> {
        let conversation = conversation(&request.conversation)?;
        let agent_id = self.hive.agent_id(&request.seat)?;
        let context = EntryContext {
            at: self.layout.learnings(),
            agent_id: &agent_id,
            conversation,
            through: request.through,
        };
        let items = request
            .entries
            .iter()
            .map(|entry| {
                entry_item(&context, entry).ok_or_else(|| {
                    tinymemory_api::Error::InvalidRequest("a memory entry needs text".into())
                })
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for chunk in items.chunks(MAX_STORE_MANY) {
            self.engine.store_many(chunk.to_vec()).await?;
        }
        Ok(())
    }
}

/// A non-blank conversation, which names the thread recalls read.
fn conversation(raw: &str) -> std::result::Result<&str, tinymemory_api::Error> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(tinymemory_api::Error::InvalidRequest(
            "a memory request needs a conversation".into(),
        ));
    }
    Ok(trimmed)
}

impl Recall for HiveMemoryStore {
    fn recall<'a>(&'a self, request: &'a RecallRequest) -> RecallFuture<'a> {
        Box::pin(async move {
            self.read(request)
                .await
                .map_err(|source| runtime::Error::Recall { source })
        })
    }
}

impl Remember for HiveMemoryStore {
    fn remember<'a>(&'a self, request: &'a RememberRequest) -> RememberFuture<'a> {
        Box::pin(async move {
            self.write(request)
                .await
                .map_err(|source| runtime::Error::Remember { source })
        })
    }
}

#[cfg(test)]
#[path = "store_test.rs"]
mod test;
