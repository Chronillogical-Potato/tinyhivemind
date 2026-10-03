//! Coordinator payloads and the host runner boundary.
use crate::Result;
use serde::{Deserialize, Serialize};
use std::{future::Future, pin::Pin, sync::Arc};
use tinyhivemind_core::driver::ConductPolicy;

/// Reserved identity used only by explicit host operations.
pub const HOST_ID: &str = "hivemind:host";
/// Future returned by an attached host runner.
pub type TurnFuture = Pin<Box<dyn Future<Output = Result<TurnOutcome>> + Send>>;
/// Runs one turn of an already configured host agent.
pub trait AgentRunner: Send + Sync {
    /// Run the supplied context, continuing its session when present.
    fn run(&self, request: TurnRequest) -> TurnFuture;
}
/// Live registration; the runner is never serialized.
#[derive(Clone)]
pub struct AgentRegistration {
    /// Globally unique agent identity within the runtime.
    pub agent_id: String,
    /// Identity of the current shared host runtime.
    pub runtime_id: String,
    /// Already configured runner handle.
    pub runner: Arc<dyn AgentRunner>,
}
impl std::fmt::Debug for AgentRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentRegistration")
            .field("agent_id", &self.agent_id)
            .field("runtime_id", &self.runtime_id)
            .finish_non_exhaustive()
    }
}
/// Scheduling bounds, matching the existing driver defaults.
#[derive(Clone, Debug)]
pub struct CoordinatorOptions {
    /// Maximum concurrent turns and the per-conductor round width; nonzero.
    pub round_width: usize,
    /// Existing child and episode turn walls; both nonzero.
    pub conduct_policy: ConductPolicy,
    /// Broadcasts per assignment; `None` preserves the driver's default.
    pub broadcast_budget: Option<u32>,
}
impl Default for CoordinatorOptions {
    fn default() -> Self {
        Self {
            round_width: 1,
            conduct_policy: ConductPolicy::default(),
            broadcast_budget: None,
        }
    }
}
/// One continuing agent turn with its captured authorization context.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TurnRequest {
    /// Bound agent identity.
    pub agent_id: String,
    /// Previously returned continuing session identity.
    pub session_id: Option<String>,
    /// Attributed new visible messages.
    pub messages: Vec<Message>,
    /// Membership snapshot captured before invocation.
    pub memberships: Vec<HiveInfo>,
    /// Active conductor assignment, absent for direct messages.
    pub episode: Option<EpisodeContext>,
}
/// Successfully returned runner state.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TurnOutcome {
    /// Continuing host session identity.
    pub session_id: String,
    /// Optional reply, recorded as a post rather than implicit completion.
    pub reply: Option<String>,
    /// Whether the runner completed, awaits approval, or failed.
    pub disposition: TurnDisposition,
}
/// Host turn status; assignment completion requires an explicit action.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum TurnDisposition {
    /// Turn returned normally.
    Completed,
    /// Awaiting an explicit host release, normally for approval.
    Parked,
    /// Host failed with an explanation; uncertain effects are not replayed.
    Failed(String),
}
/// Active episode and channel bound to one turn.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EpisodeContext {
    /// Durable episode identity.
    pub episode_id: String,
    /// Sole hive/desk identity.
    pub hive_id: String,
    /// Conversation root, absent on the open hive.
    pub thread: Option<u64>,
    /// Rendered existing conductor brief.
    pub brief: String,
}
/// Dynamic hive definition; membership is ordered and unique.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HiveInfo {
    /// Sole hive/desk identity.
    pub hive_id: String,
    /// Human-readable name.
    pub name: String,
    /// Optional purpose.
    pub description: Option<String>,
    /// Registered agent identities.
    pub members: Vec<String>,
}
/// A hive channel or globally registered agent.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Destination {
    /// Hive identity.
    Hive(String),
    /// Runtime agent identity.
    Agent(String),
}
/// Durable attributed message.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Message {
    /// Caller retry identity, or a coordinator-generated event identity.
    pub message_id: String,
    /// Globally monotonic durable sequence.
    pub sequence: u64,
    /// Bound sender identity.
    pub sender: String,
    /// Destination channel.
    pub destination: Destination,
    /// Text payload.
    pub body: String,
    /// Optional hive conversation root.
    pub thread: Option<u64>,
    /// Owning episode, absent for standalone direct messages.
    pub episode_id: Option<String>,
    /// Private readers; empty means all hive members.
    pub only_for: Vec<String>,
}
/// Message acceptance request; tools bind the sender on the host side.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SendMessage {
    /// Stable retry identity.
    pub message_id: String,
    /// Registered author; ignored by `send_as_host`.
    pub sender: String,
    /// Target hive or agent.
    pub destination: Destination,
    /// Message text.
    pub body: String,
    /// Optional existing visible hive conversation root.
    pub thread: Option<u64>,
    /// Optional private recipients within the target hive.
    pub only_for: Vec<String>,
}
/// Receipt returned without waiting for the destination agent.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Receipt {
    /// Stable retry identity.
    pub message_id: String,
    /// Accepted durable sequence.
    pub sequence: u64,
}
/// Episode tools collected during the active bound turn.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum EpisodeAction {
    /// Post without closing the assignment.
    Post {
        /// Message body.
        body: String,
    },
    /// Open one private child conversation.
    Ask {
        /// Captured hive peers.
        agents: Vec<String>,
        /// Question body.
        body: String,
    },
    /// Route work using the existing conductor handoff rules.
    Broadcast {
        /// Work body.
        body: String,
    },
    /// Report the active assignment complete.
    Complete {
        /// Completion body.
        body: String,
    },
}
/// Work observed during one eligible-work drain.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RunReport {
    /// Normally returned turns.
    pub completed: usize,
    /// Failed turns or conductor episodes.
    pub failed: usize,
    /// Turns awaiting explicit release.
    pub parked: usize,
}
/// Uncertain turn effects which must not be replayed automatically.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InterruptedTurn {
    /// Bound agent identity.
    pub agent_id: String,
    /// Owning episode when present.
    pub episode_id: Option<String>,
    /// Messages whose delivery may have begun.
    pub message_ids: Vec<String>,
    /// Cancellation, crash, or runner failure explanation.
    pub reason: String,
}
