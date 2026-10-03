//! Serializable snapshot format; no live handles or callbacks.
use crate::{EpisodeAction, HiveInfo, InterruptedTurn, Message, SendMessage, TurnRequest};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tinyhivemind_core::driver::{ConductorState, Turn};

/// Versioned transactional snapshot shared by all storage implementations.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct StoredState {
    /// CAS revision; each commit advances exactly one.
    pub revision: u64,
    /// Next global message sequence.
    pub next_sequence: u64,
    /// Dynamic definitions and current memberships.
    pub hives: BTreeMap<String, HiveInfo>,
    /// Stable agent identities and continuing session records.
    pub agents: BTreeMap<String, AgentRecord>,
    /// Ordered attributed transcript.
    pub messages: Vec<Message>,
    /// Original caller payloads for exact retry comparison.
    pub accepted: BTreeMap<String, SendMessage>,
    /// Direct delivery queues and acknowledgements.
    pub deliveries: Vec<Delivery>,
    /// Ordered hive episodes with exact core checkpoints.
    pub episodes: Vec<EpisodeRecord>,
    /// Durably claimed turns whose effects may already have begun.
    pub running: BTreeMap<String, RunningTurn>,
    /// Recovery and cancellation records, never retried automatically.
    pub interruptions: Vec<InterruptedTurn>,
}
/// Durable identity; a new process must reattach its live runner.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentRecord {
    /// Continuing session returned by the host.
    pub session_id: Option<String>,
    /// Agent waits for explicit host release.
    pub parked: bool,
}
/// One direct-agent inbox entry.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Delivery {
    /// Accepted global sequence.
    pub sequence: u64,
    /// Registered recipient.
    pub agent_id: String,
    /// Delivery acknowledgement.
    pub status: DeliveryStatus,
}
/// Direct message delivery lifecycle.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DeliveryStatus {
    /// Runner has never been invoked.
    Pending,
    /// Claimed durably before invocation.
    Running,
    /// Successfully returned.
    Delivered,
    /// Effects uncertain and not replayable.
    Interrupted,
}
/// One conducted hive episode, frozen membership and wave included.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EpisodeRecord {
    /// Durable episode identity.
    pub episode_id: String,
    /// Membership snapshot for the core driver.
    pub hive: HiveInfo,
    /// Accepted triggering message.
    pub opened_at: u64,
    /// Addressed outer conversation; core child channels remain nested beneath it.
    #[serde(default)]
    pub thread: Option<u64>,
    /// Seats allowed to receive the initial task.
    pub starters: Vec<String>,
    /// Existing conductor checkpoint, absent before opening.
    pub conductor: Option<ConductorState>,
    /// Proposed wave turns not yet invoked.
    pub pending: Vec<Turn>,
    /// True while a conductor wave has not closed.
    pub wave_open: bool,
    /// Empty wave waiting for host release or a missing runner.
    pub waiting: bool,
    /// Terminal episodes allow the next hive message to start.
    pub finished: bool,
    /// Failure/wall explanation when the episode stopped.
    pub failure: Option<String>,
}
/// Reservation and captured authorization for one agent invocation.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RunningTurn {
    /// Captured request passed to the host.
    pub request: TurnRequest,
    /// Core proposed turn, absent for direct messages.
    pub turn: Option<Turn>,
    /// Active-turn actions, in call order.
    pub actions: Vec<EpisodeAction>,
    /// Accepted direct message sequence when present.
    pub delivery_sequence: Option<u64>,
}
