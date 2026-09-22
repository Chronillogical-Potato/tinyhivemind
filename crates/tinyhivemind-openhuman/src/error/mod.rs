//! What seating or running a seat on `OpenHuman` can fail with.

/// The crate error.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `TINYHIVEMIND_RUNNER` named neither runner.
    #[error("TINYHIVEMIND_RUNNER must be `embed` or `raw`, not `{0}`")]
    UnknownRunner(String),
    /// A route was missing its endpoint or its key.
    #[error("a route needs both an endpoint and a key")]
    IncompleteRoute,
    /// The definition registry did not come up after the seats were written.
    #[error("the definition registry did not initialise")]
    RegistryMissing,
    /// A seat's definition was written and the registry did not load it.
    #[error("seat `{seat}` did not register")]
    SeatNotRegistered {
        /// The seat.
        seat: String,
    },
    /// A turn ran past [`TURN_TIMEOUT`](crate::TURN_TIMEOUT).
    #[error("@{seat} timed out")]
    TimedOut {
        /// The seat.
        seat: String,
    },
    /// The episode's tool server could not bind.
    #[error(transparent)]
    Serve(#[from] tinyhivemind_mcp::Error),
    /// A seat definition could not be written.
    #[error("writing a seat definition: {0}")]
    Io(#[from] std::io::Error),
    /// An `openhuman-embed` agent could not be instantiated.
    #[error(transparent)]
    Agent(#[from] openhuman_embed::AgentError),
    /// The host's log failed to read, or broke the port's contract, while a
    /// turn was being seeded.
    #[error(transparent)]
    Session(#[from] tinyhivemind::Error),
    /// `OpenHuman` refused: booting as a library host, resolving the route,
    /// building or seeding a session, or running the turn.
    #[error(transparent)]
    Harness(#[from] anyhow::Error),
}

/// The crate-wide result alias.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod test;
