//! Unit tests for validated attributed projection, grouped by behavior area:
//! wire shapes, page validation, channel narrowing, thread narrowing, and
//! private-aside visibility. Shared fixtures live in [`support`].

mod support;

mod channel;
mod paging;
mod thread;
mod visibility;
mod wire;
