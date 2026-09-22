//! The one way serving can fail.

/// A failure to stand the server up. Refusals to a seat are not errors: they
/// go back to the seat as tool results it can read and act on.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Loopback could not be bound.
    #[error("could not bind the episode tool server: {0}")]
    Bind(#[from] std::io::Error),
}

/// The crate-wide result alias.
pub type Result<T> = std::result::Result<T, Error>;
