//! Typed failures from validating host-neutral embedding payloads.

/// A typed failure from validating a host-neutral embedding payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum Error {
    /// A desk aside names more recipients than the opening-round limit.
    #[error("desk aside names {recipient_count} recipients but the round width is {round_width}")]
    DeskAsideTooWide {
        /// Number of recipients named by the authored route.
        recipient_count: usize,
        /// Maximum number of recipients the host permits in one round.
        round_width: usize,
    },
}

/// Result returned by fallible `tinyhivemind-embed` APIs.
pub type Result<T> = std::result::Result<T, Error>;
