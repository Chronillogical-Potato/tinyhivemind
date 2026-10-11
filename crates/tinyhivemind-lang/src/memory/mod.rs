//! Memory and constitution declarations: where a hive's memory lives, who may
//! read and write it, and the host-pinned caps every package is judged by.
//!
//! [`MemorySpec`] is decoded from `memory.json` and [`Constitution`] from
//! `constitution.json`. Both describe bindings and limits only: remembered
//! entries stay in the host's memory engine, and nothing here opens it.
//! [`crate::validate`] checks namespaces, reaches, and read-only pins, and
//! [`crate::lower`] hands each seat its resolved binding for the host to
//! enforce.
mod types;
pub use types::*;
