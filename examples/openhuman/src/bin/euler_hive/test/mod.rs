//! Offline tests for the Euler hive host: package wiring, the per-seat tool
//! gate, the sandbox's confinement, run-scoped memory, the prompt and the
//! broadcast router. No network, Docker, model or memory engine is touched.

mod artifacts;
mod federation;
mod memory;
mod package;
mod routing;
mod sandbox;
mod support;

pub(super) use support::package_dir;
