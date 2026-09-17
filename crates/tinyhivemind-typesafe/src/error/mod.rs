//! Typed failures from the `TypeSafe` System One adapter.

/// A typed failure from constructing or converting a System One routing request.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Candidate ids cannot safely form distinct System One question keys.
    #[error("routing candidate ids must be unique, nonblank, and not none")]
    InvalidCandidateIds,
    /// The request has no candidate eligible for an immediate turn.
    #[error("routing request has no eligible candidates")]
    NoEligibleCandidates,
    /// The Choice bound cannot contain a candidate and the reserved `none` alternative.
    #[error("choice option limit must include one candidate and none")]
    InvalidChoiceOptionLimit,
    /// A provider response contradicts the request's required typed schema.
    #[error("{message}")]
    InvalidProviderResponse {
        /// Stable explanation of the rejected response shape.
        message: &'static str,
    },
    /// The adapter could not encode the host-supplied routing state.
    #[error("failed to serialize routing state")]
    SerializeState {
        /// The serializer's detailed source error.
        #[source]
        source: serde_json::Error,
    },
    /// A host-owned System One transport failed.
    #[error("System One transport {status:?}: {message}")]
    Transport {
        /// HTTP-like status when one exists.
        status: Option<u16>,
        /// Provider-safe diagnostic.
        message: String,
    },
}

/// Result returned by fallible `TypeSafe` adapter APIs.
pub type Result<T> = std::result::Result<T, Error>;

/// Typed failure reported by a host System One transport.
pub type TransportError = Error;
