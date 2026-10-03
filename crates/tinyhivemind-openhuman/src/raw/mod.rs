//! `OpenHuman` library-host setup and native episode tools.
//!
//! `register_seats` writes definitions before `OpenHuman` reads its process-wide
//! registry. `LibraryHost` boots the core under a caller-owned provider route.

mod library;
mod policy;
#[cfg(test)]
mod test;
pub(crate) mod tools;

use crate::{Error, Result};
use openhuman_core::agent::harness::AgentDefinitionRegistry;
use std::path::Path;

pub use library::LibraryHost;

/// Register every seat as a workspace definition naming `tools` as its
/// belt, before the process registry is read.
///
/// A session's turn runs as a hosted root invocation, which resolves the
/// seat against `OpenHuman`'s process registry and takes the model's
/// allowlist from the seat's *definition*, not from the belt the session was
/// built with: a tool the definition does not name is stripped before the
/// model sees it, and a wildcard projects to nothing. So `tools` must name
/// every tool the seat will be handed, as the model calls them -- for a host
/// that prefixes the episode's tools, the prefixed names, alongside its own.
///
/// A seat id becomes a file name, so it is one plain path component:
/// ASCII letters, digits, `-`, `_` and `.`, and not `.` or `..` alone.
///
/// The loader wants `id`, `when_to_use` and a non-empty `system_prompt`; the
/// prompt written here is the seat's role for a reader of the workspace, not
/// the one a session runs under.
///
/// The registry is process-wide and read **once**: the first call fixes it,
/// and a later call writes its definitions where nothing will read them and
/// fails on the first seat the fixed registry lacks. So a host registers
/// every seat it will ever run, in one call, before any session is built,
/// and a host seating more than one desk in one process names its seats
/// apart and registers them together.
///
/// # Errors
///
/// A seat id that is not a plain path component, the directory or a file
/// failing to write, the registry refusing the definitions, or a seat the
/// already-fixed registry does not hold.
pub fn register_seats(workspace: &Path, seats: &[(&str, &str)], tools: &[String]) -> Result<()> {
    // A seat id names a file: one path component, and nothing a path can
    // be steered with.
    for (id, _) in seats {
        let plain = !id.is_empty()
            && *id != "."
            && *id != ".."
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        if !plain {
            return Err(Error::UnsafeSeatId {
                seat: (*id).to_owned(),
            });
        }
    }
    let agents = workspace.join("agents");
    std::fs::create_dir_all(&agents)?;
    let named: Vec<String> = tools.iter().map(|name| format!("{name:?}")).collect();
    for (id, role) in seats {
        let toml = format!(
            "id = {id:?}\nwhen_to_use = {role:?}\nsystem_prompt = {{ inline = {role:?} }}\ntools = {{ named = [{}] }}\n",
            named.join(", ")
        );
        std::fs::write(agents.join(format!("{id}.toml")), toml)?;
    }
    // Set-once: a registry already read stays as it was, and the check
    // below says which seat that leaves out.
    AgentDefinitionRegistry::init_global(workspace)?;
    let registry = AgentDefinitionRegistry::global().ok_or(Error::RegistryMissing)?;
    for (id, _) in seats {
        if registry.get(id).is_none() {
            return Err(Error::SeatNotRegistered {
                seat: (*id).to_owned(),
            });
        }
    }
    Ok(())
}

/// OpenAI-compatible model route for a library-host core context.
#[derive(Clone, Debug)]
pub struct Route {
    /// The OpenAI-compatible endpoint, up to and including `/v1`.
    pub endpoint: String,
    /// The bearer the endpoint takes.
    pub api_key: String,
    /// The model id to ask for.
    pub model: String,
}
