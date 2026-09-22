//! What seating or running a seat on `OpenHuman` can fail with.

/// The crate error.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `TINYHIVEMIND_RUNNER` named neither runner.
    #[error("TINYHIVEMIND_RUNNER must be `embed`, `raw` or `hosted`, not `{0}`")]
    UnknownRunner(String),
    /// A route was missing its endpoint or its key.
    #[error("a route needs both an endpoint and a key")]
    IncompleteRoute,
    /// The definition registry did not come up after the seats were written.
    #[error("the definition registry did not initialise")]
    RegistryMissing,
    /// A seat id that cannot name a definition file: empty, `.`, `..`, or
    /// carrying anything but ASCII letters, digits, `-`, `_` and `.`.
    #[error("seat id `{seat}` is not a plain path component")]
    UnsafeSeatId {
        /// The id.
        seat: String,
    },
    /// A seat's definition was written and the registry did not load it:
    /// the process registry is read once, so a seat written after that
    /// first read is never seen.
    #[error("seat `{seat}` did not register: the process registry was read before it was written")]
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
    /// turn was being seeded or briefed.
    #[error(transparent)]
    Session(#[from] tinyhivemind::Error),
    /// The conductor stopped the episode: a stalled desk, a wall, or a fold
    /// error it could not explain to the seat.
    #[error(transparent)]
    Conduct(#[from] tinyhivemind_driver::Error),
    /// `OpenHuman` refused: booting as a library host, resolving the route,
    /// building or seeding a session, or running the turn.
    #[error(transparent)]
    Harness(#[from] anyhow::Error),
}

/// The crate-wide result alias.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod test;
