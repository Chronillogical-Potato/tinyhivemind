//! Unit tests for caller-owned watermark sharing, grouped by behavior area:
//! wire shapes, state bookkeeping, reinitialization, the delta walk, and
//! private-aside visibility. Shared fixtures live in [`support`].

mod support;

mod delta;
mod reinit;
mod state;
mod visibility;
mod wire;
