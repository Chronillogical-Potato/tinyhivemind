//! Host extension points and supplied handle types.
use crate::Result;
use openhuman_embed::Agent;
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
use tinyhivemind_hives::{HiveInfo, TurnDisposition};
/// Default maximum duration of a supplied agent turn.
pub const TURN_TIMEOUT: Duration = Duration::from_secs(300);
/// Host factory result future.
pub type AgentFuture = Pin<Box<dyn Future<Output = Result<Agent>> + Send>>;
/// Host creates configured agents from nonsecret template references.
pub trait AgentFactory: Send + Sync {
    /// Create on the shared runtime; host validates template and config.
    fn create(&self, template: String, config: serde_json::Value) -> AgentFuture;
}
/// Explicit opt-in management authorization.
pub trait ManagementAuthorizer: Send + Sync {
    /// Authorize before any factory or coordinator mutation.
    /// # Errors
    /// Return a denial when the actor cannot perform this request.
    fn authorize(&self, actor: &str, request: &ManagementRequest) -> Result<()>;
}
/// Management operation offered to the host authorizer.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum ManagementRequest {
    /// Create a hive using known agents.
    CreateHive(HiveInfo),
    /// Create a configured agent through a host-defined template.
    CreateAgent {
        /// Template reference.
        template: String,
        /// Nonsecret host-validated settings.
        config: serde_json::Value,
    },
    /// Join a registered agent to a hive.
    JoinHive {
        /// Hive identity.
        hive_id: String,
        /// Agent identity.
        agent_id: String,
    },
    /// Leave a hive without deleting its history.
    LeaveHive {
        /// Hive identity.
        hive_id: String,
        /// Agent identity.
        agent_id: String,
    },
}
/// Future wrapped by a host turn scope.
pub type HostedTurn<'a> =
    Pin<Box<dyn Future<Output = Result<openhuman_embed::TurnOutcome>> + Send + 'a>>;
/// Drained progress channel owned by the host.
pub type TurnProgressSink =
    tokio::sync::mpsc::Sender<openhuman_embed::agent_progress::AgentProgress>;
/// Optional per-turn progress, usage, approval and scope hooks.
pub trait TurnHooks: Send + Sync {
    /// Host owes this sink a receiver throughout the turn.
    fn progress(&self, _agent: &str) -> Option<TurnProgressSink> {
        None
    }
    /// Install host context around the complete turn.
    fn wrap_turn<'a>(&'a self, _agent: &'a str, turn: HostedTurn<'a>) -> HostedTurn<'a> {
        turn
    }
    /// Called after successful or failed turns with fresh usage, never stale usage.
    /// # Errors
    /// Host metering or approval processing can refuse completion. When the
    /// agent already committed its turn, its session remains bound while the
    /// coordinator interrupts delivery and discards staged actions and replies.
    fn after_turn(
        &self,
        _agent: &str,
        _usage: Option<&openhuman_core::agent::tinyagents::host::LastTurnUsage>,
    ) -> Result<TurnDisposition> {
        Ok(TurnDisposition::Completed)
    }
}
#[derive(Debug)]
pub(super) struct DefaultHooks;
impl TurnHooks for DefaultHooks {}
/// An existing agent implementing core's bound-handle trait.
#[derive(Clone, Debug)]
pub struct RegisteredAgent(pub Agent);
impl tinyhivemind_core::driver::BoundAgent for RegisteredAgent {
    fn runtime_id(&self) -> &str {
        self.0.id()
    }
}
pub(super) struct Management {
    pub factory: Arc<dyn AgentFactory>,
    pub authorizer: Arc<dyn ManagementAuthorizer>,
}

#[cfg(test)]
#[path = "types_test.rs"]
mod test;
