//! Unit tests for quorum counting and cross-inhibition, grouped by the
//! behavior each submodule exercises.
//!
//! [`support`] holds the fixtures every other submodule shares — transcript
//! builders, the refutation-enabled `policy`, and the `fold`/`standing`
//! helpers that turn a transcript into the one standing a test cares about.
//! The rest split along the seams [`super`]'s module doc already draws:
//! plain support counting, cross-inhibition, fold discipline
//! (order-independence and idempotence), refutation, and the
//! evidential-grounding gate. Wire-form pins for the policy and standing
//! types live on their own in [`wire_forms`].

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod cross_inhibition;
mod evidential_grounding;
mod fold_discipline;
mod probabilistic;
mod refutation;
mod support;
mod support_counting;
mod wire_forms;
