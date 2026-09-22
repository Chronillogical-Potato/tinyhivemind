//! The raw runner: `OpenHumanSessionHost` sessions, tools in-process.
//!
//! The same episode, the same driver, the same tools -- and a different way
//! to run a seat. Where the embed runner keeps an `openhuman-embed` agent per
//! seat and reaches the room's tools over MCP, this runner builds a session
//! one level down, with `OpenHumanSessionHost::builder()`, on every turn. The
//! builder takes what a spec cannot: a tool belt, a policy gate, a memory, a
//! prompt. The belt is `tinyhivemind-tools`'s own tool definitions rendered as
//! native tools, each of which calls `EpisodeTools::call` directly, so a seat
//! here is refused and acknowledged in exactly the words an MCP seat is.
//!
//! What that changes, and why it is worth a second runner:
//!
//! - **No transport.** A call lands in the inbox before the turn returns;
//!   there is no server to dial, no discovery turn, and no `mcp_call_tool`
//!   between the model and the tool. The three MCP dispatchers are not on the
//!   belt at all.
//! - **The host owns context.** A session is fresh every turn and seeded from
//!   a per-seat log this runner keeps, so nothing is written to `OpenHuman`'s
//!   own transcript files and what a seat carries between turns is exactly
//!   what the host gave it.
//! - **Objects, not configuration.** The gate that admits only the episode's
//!   tools and the memory that keeps nothing are `ToolPolicy` and `Memory`
//!   implementations, which is the seam a real host puts its own on.
//!
//! Two things it costs, both of them the current `OpenHuman`'s terms for a
//! session built outside its product:
//!
//! - A raw session still runs its turn as a hosted root invocation, and that
//!   path resolves the seat against `OpenHuman`'s process registry and takes
//!   the model's allowlist from the seat's *definition*, not from the belt.
//!   So each seat is registered as a workspace definition with its belt
//!   declared by name before any runtime boots ([`RawRunner::prepare`]). A
//!   wildcard scope there projects to no declared names, and the host fails
//!   closed on an undeclared belt.
//! - The core decides whose product policy applies from its ambient context,
//!   and with none it is the desktop's: inference waits on the operator
//!   signing in. So the seats are booted under a library-host context once,
//!   at seating ([`RawRunner::seat`]), and every turn runs inside it.

mod library;
mod policy;
mod seat;
#[cfg(test)]
mod test;
pub(crate) mod tools;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use openhuman_core::agent::harness::AgentDefinitionRegistry;
use openhuman_core::config::Config;
use tinyhivemind::Sequence;
use tinyhivemind_driver::AgentBinding;
use tinyhivemind_tools::EpisodeTools;

use crate::runner::{Lane, SeatRunner, TurnJob, unseated};
use crate::{Error, Result};
pub use library::LibraryHost;
pub use seat::RawSeat;

/// What each seat has been shown and said, keyed by seat.
type Contexts = Arc<Mutex<BTreeMap<String, Vec<(String, String)>>>>;

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

/// Where a run's inference comes from, as the raw session needs it: the
/// embed runtime applies its route per call, a raw session resolves the
/// `chat` role from its config, so the route is written into that config.
#[derive(Clone, Debug)]
pub struct Route {
    /// The OpenAI-compatible endpoint, up to and including `/v1`.
    pub endpoint: String,
    /// The bearer the endpoint takes.
    pub api_key: String,
    /// The model id to ask for.
    pub model: String,
}

/// Seats as raw sessions, one per turn, with the room's tools in-process.
pub struct RawRunner {
    tools: Arc<EpisodeTools>,
    seats: BTreeMap<String, RawSeat>,
    /// What the config resolved `chat` to.
    model: String,
    /// The history each seat's next session is seeded with. The host's log,
    /// not `OpenHuman`'s.
    contexts: Contexts,
}

impl RawRunner {
    /// Register every seat as a workspace definition naming the served belt,
    /// before the runtime boots and the process registry is read. See
    /// [`register_seats`] for why, and for a host whose belt is named
    /// otherwise.
    ///
    /// # Errors
    ///
    /// The directory or a file failing to write, or the registry refusing the
    /// definitions.
    pub fn prepare(workspace: &Path, seats: &[(&str, &str)]) -> Result<()> {
        let belt: Vec<String> = tinyhivemind_tools::served_specs()
            .map(|spec| spec.name.to_owned())
            .collect();
        register_seats(workspace, seats, &belt)
    }

    /// Seat every brief as a raw seat over one resolved config.
    ///
    /// `base` is the config the embed runner would boot its runtime with; the
    /// workspace, the backend stub and the route the embed runner applies per
    /// call are written in, so a raw session's `chat` role resolves to the
    /// same model over the same endpoint.
    ///
    /// # Errors
    ///
    /// The core refusing to boot as a library host, or the route failing to
    /// resolve.
    pub async fn seat(
        tools: Arc<EpisodeTools>,
        briefs: &BTreeMap<String, String>,
        contract: &str,
        base: &Config,
        backend_url: &str,
        route: &Route,
        workspace: &Path,
    ) -> Result<Self> {
        let library = LibraryHost::boot(base, backend_url, route, workspace).await?;
        let model = library.model().to_owned();
        let seats = briefs
            .iter()
            .map(|(id, brief)| {
                (
                    id.clone(),
                    RawSeat::new(id, format!("{brief}\n\n{contract}"), library.clone()),
                )
            })
            .collect();
        Ok(Self {
            tools,
            seats,
            model,
            contexts: Arc::new(Mutex::new(BTreeMap::new())),
        })
    }

    /// The model id the config resolved `chat` to, at seating.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }
}

impl std::fmt::Debug for RawRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawRunner")
            .field("seats", &self.seats.keys().collect::<Vec<_>>())
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}

impl SeatRunner for RawRunner {
    fn tools(&self) -> &Arc<EpisodeTools> {
        &self.tools
    }

    type Bound = RawSeat;

    fn bindings(&self) -> Vec<AgentBinding<RawSeat>> {
        self.seats
            .iter()
            .map(|(id, seat)| AgentBinding::new(id.clone(), seat.clone()))
            .collect()
    }

    /// A fresh session, seeded with what this seat has been shown and said
    /// so far, run once and dropped. Its belt is built for this seat and this
    /// turn, and every call it makes lands in the shared record.
    fn turn(&self, seat: String, lane: Lane, _since: Sequence, prompt: String) -> TurnJob {
        let history = self
            .contexts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&seat)
            .cloned()
            .unwrap_or_default();
        let belt = tools::belt(&seat, &self.tools);
        let Some(raw_seat) = self.seats.get(&seat).cloned() else {
            return unseated(seat, lane);
        };
        let contexts = Arc::clone(&self.contexts);
        Box::pin(async move {
            let result = match Box::pin(raw_seat.turn(history, &prompt, belt)).await {
                Ok(reply) => Some(Ok(reply)),
                Err(error) => Some(Err(error.to_string())),
            };
            if let Some(Ok(reply)) = &result {
                let mut contexts = contexts.lock().unwrap_or_else(PoisonError::into_inner);
                let context = contexts.entry(seat.clone()).or_default();
                context.push(("user".to_owned(), prompt));
                context.push(("assistant".to_owned(), reply.clone()));
            }
            (seat, lane, result)
        })
    }
}
