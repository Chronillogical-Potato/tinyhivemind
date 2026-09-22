//! What a raw seat may do and what it remembers: the episode, and nothing else.
//!
//! Both are objects a raw session takes and `AgentSpec` cannot: a
//! [`ToolPolicy`] is middleware that runs before every tool executes, and a
//! [`Memory`] is the store the session's recall and hooks read and write.

use async_trait::async_trait;
use openhuman_core::agent::tool_policy::{ToolPolicy, ToolPolicyDecision, ToolPolicyRequest};
use openhuman_core::memory::{Memory, MemoryCategory, MemoryEntry, NamespaceSummary, RecallOpts};

/// Allows the belt it was given and denies everything else.
///
/// The belt already holds only the episode's tools, so this is belt and
/// braces -- but it is the seam a real host puts its approval gate on, and a
/// gate that exists only as an allowlist elsewhere can be bypassed by wiring
/// one more tool. Here the refusal is a typed decision the loop sees.
#[derive(Debug)]
pub struct EpisodeGate {
    admitted: Vec<String>,
}

impl EpisodeGate {
    pub fn new(admitted: Vec<String>) -> Self {
        Self { admitted }
    }
}

#[async_trait]
impl ToolPolicy for EpisodeGate {
    fn name(&self) -> &str {
        "episode_gate"
    }

    async fn check(&self, request: &ToolPolicyRequest) -> ToolPolicyDecision {
        if self.admitted.contains(&request.tool_name) {
            ToolPolicyDecision::Allow
        } else {
            ToolPolicyDecision::deny(format!(
                "`{}` is not an episode tool; a seat holds only the room's tools",
                request.tool_name
            ))
        }
    }
}

/// A memory that keeps nothing.
///
/// The episode is a fold over a transcript the host owns, and what a seat
/// carries between turns is the context this runner seeds it with. Every
/// store is accepted and discarded; every read is empty; nothing errors.
#[derive(Debug, Default)]
pub struct NoMemory;

#[async_trait]
impl Memory for NoMemory {
    fn name(&self) -> &str {
        "none"
    }

    async fn store(
        &self,
        _namespace: &str,
        _key: &str,
        _content: &str,
        _category: MemoryCategory,
        _session_id: Option<&str>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn recall(
        &self,
        _query: &str,
        _limit: usize,
        _opts: RecallOpts<'_>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        Ok(Vec::new())
    }

    async fn get(&self, _namespace: &str, _key: &str) -> anyhow::Result<Option<MemoryEntry>> {
        Ok(None)
    }

    async fn list(
        &self,
        _namespace: Option<&str>,
        _category: Option<&MemoryCategory>,
        _session_id: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        Ok(Vec::new())
    }

    async fn forget(&self, _namespace: &str, _key: &str) -> anyhow::Result<bool> {
        Ok(false)
    }

    async fn namespace_summaries(&self) -> anyhow::Result<Vec<NamespaceSummary>> {
        Ok(Vec::new())
    }

    async fn count(&self) -> anyhow::Result<usize> {
        Ok(0)
    }

    async fn health_check(&self) -> bool {
        true
    }
}
