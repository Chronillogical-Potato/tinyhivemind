//! Unit tests for the episode state machine, grouped by the behavior each
//! submodule exercises.
//!
//! [`support`] holds the fixtures every other submodule shares — a
//! three-member `Room`, transcript builders, and the `run`/`speaking` helpers
//! most tests drive `step` through. The rest follow the seams `step`'s own
//! doc comment draws: wire-form pins, sequential-round fixtures and
//! termination, quorum and convergence, deadlock and cross-inhibition,
//! per-turn attention dynamics (blind visibility and threshold charging),
//! validation failure paths and the two aside modules —
//! off-floor privacy and the concurrent-aside cost guarantee.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod concurrent_asides;
mod deadlock;
mod failure_paths;
mod off_floor_asides;
mod quorum_and_convergence;
mod support;
mod termination;
mod turn_dynamics;
mod wire_forms;
