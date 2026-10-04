//! Typed adapter errors.
/// Adapter result.
pub type Result<T> = std::result::Result<T, Error>;
/// Failures attaching or running supplied agents.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The agent or coordinator belongs to another runtime.
    #[error("agent or coordinator belongs to another runtime")]
    RuntimeMismatch,
    /// Another live handle has this agent ID.
    #[error("conflicting agent handle {0}")]
    AgentConflict(String),
    /// Management settings cannot change after registration.
    #[error("management must be configured before registration")]
    ManagementAlreadyStarted,
    /// Management was not configured by the host.
    #[error("management is disabled")]
    ManagementDisabled,
    /// Attached services have been dropped.
    #[error("hivemind host is unavailable")]
    Unavailable,
    /// Durable registration has not completed; retry registration first.
    #[error("agent registration is pending")]
    RegistrationPending,
    /// Host explicitly denied management.
    #[error("management denied: {0}")]
    Unauthorized(String),
    /// Shared adapter state was poisoned.
    #[error("adapter lock poisoned")]
    Poisoned,
    /// Agent turn exceeded the wall.
    #[error("agent turn timed out")]
    TimedOut,
    /// Coordinator refused the operation.
    #[error(transparent)]
    Coordinator(#[from] tinyhivemind_hives::Error),
    /// The host factory could not instantiate its configured agent.
    #[error(transparent)]
    Agent(#[from] openhuman_embed::AgentError),
    /// Named attachment refused the tools.
    #[error(transparent)]
    Attachment(#[from] openhuman_embed::ToolAttachmentError),
    /// Tool argument decoding failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// `OpenHuman` or the host hook failed.
    #[error(transparent)]
    Harness(#[from] anyhow::Error),
}
