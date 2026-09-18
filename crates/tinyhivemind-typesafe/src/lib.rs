//! `TypeSafe` System One routing adapter for `TinyHiveMind`.
//!
//! [`JevRouter`] asks one batched Choice/Noul request for an ordinary desk
//! message or agent broadcast and implements bounded hierarchical routing when
//! the host's option limit is exceeded. [`SystemOneTransport`] is the only
//! waiting boundary; this crate owns no HTTP client, credentials, async
//! runtime, or host application types.

mod error;
mod router;
mod wire;

#[cfg(test)]
mod test;

pub use error::{Error, Result, TransportError};
pub use router::{JevRouter, RetryClass, classify_retry};
pub use wire::{
    ChoiceAnswer, NoulAnswer, NoulCriteria, Question, SystemOneAnswer, SystemOneRequest,
    SystemOneResponse, SystemOneTransport, SystemOneTransportFuture, TokenUsage,
};
