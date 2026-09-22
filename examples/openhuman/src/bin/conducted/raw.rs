//! The raw runner: `OpenHumanSessionHost` sessions, tools in-process.
//!
//! The same episode, the same driver, the same tools -- and a different way
//! to run a seat. Where the embed runner keeps an `openhuman-embed` agent per
//! seat and reaches the room's tools over MCP, this runner builds a session
//! one level down, with `OpenHumanSessionHost::builder()`, on every turn. The
//! builder takes what a spec cannot: a tool belt, a policy gate, a memory, a
//! prompt. The belt is `tinyhivemind-mcp`'s own tool definitions rendered as
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
//!   a per-seat log this runner keeps, so nothing is written to OpenHuman's
//!   own transcript files and what a seat carries between turns is exactly
//!   what the host gave it.
//! - **Objects, not configuration.** The gate that admits only the episode's
//!   tools and the memory that keeps nothing are `ToolPolicy` and `Memory`
//!   implementations, which is the seam a real host puts its own on.
//!
//! One thing it costs: a raw session still runs its turn as a hosted root
//! invocation, and that path resolves the seat against OpenHuman's process
//! registry and takes the model's allowlist from the seat's *definition*, not
//! from the belt. So each seat is registered as a workspace definition with
//! its belt declared by name before the runtime boots ([`RawRunner::prepare`]).
//! A wildcard scope there projects to no declared names, and the host fails
//! closed on an undeclared belt.

pub mod offline;
mod policy;
mod seat;
#[cfg(test)]
mod test;
mod tools;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use openhuman_core::agent::harness::AgentDefinitionRegistry;
use openhuman_core::config::Config;
use openhuman_core::config::schema::ephemeral_route::{self, EphemeralRoute};
use tinyhivemind_mcp::EpisodeTools;
use tinyhivemind_openhuman::AgentBinding;

use super::runner::{Lane, SeatRunner, TurnJob};
use seat::RawSeat;

/// What each seat has been shown and said, keyed by seat.
type Contexts = Arc<Mutex<BTreeMap<String, Vec<(String, String)>>>>;

/// Where a run's inference comes from, as the raw session needs it: the
/// embed runtime applies its route per call, a raw session resolves the
/// `chat` role from its config, so the route is written into that config.
pub struct Route {
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
}

/// Seats as raw sessions, one per turn, with the room's tools in-process.
pub struct RawRunner {
    tools: Arc<EpisodeTools>,
    seats: BTreeMap<String, RawSeat>,
    /// The history each seat's next session is seeded with. The host's log,
    /// not OpenHuman's.
    contexts: Contexts,
}

impl RawRunner {
    /// Register every seat as a workspace definition, before the runtime
    /// boots and the process registry is read.
    ///
    /// The loader wants `id`, `when_to_use` and a non-empty `system_prompt`;
    /// the prompt written here is the seat's role for a reader of the
    /// workspace, not the one a session runs under. `tools` is the served
    /// belt by name: the hosted turn's allowlist comes from here.
    ///
    /// # Errors
    ///
    /// The directory or a file failing to write, or the registry refusing the
    /// definitions.
    pub fn prepare(workspace: &Path, seats: &[(&str, &str)]) -> anyhow::Result<()> {
        let agents = workspace.join("agents");
        std::fs::create_dir_all(&agents)?;
        let belt: Vec<String> = tinyhivemind_mcp::served_specs()
            .map(|spec| format!("{:?}", spec.name))
            .collect();
        for (id, role) in seats {
            let toml = format!(
                "id = {id:?}\nwhen_to_use = {role:?}\nsystem_prompt = {{ inline = {role:?} }}\ntools = {{ named = [{}] }}\n",
                belt.join(", ")
            );
            std::fs::write(agents.join(format!("{id}.toml")), toml)?;
        }
        AgentDefinitionRegistry::init_global(workspace)?;
        let registry = AgentDefinitionRegistry::global()
            .ok_or_else(|| anyhow::anyhow!("the definition registry did not initialise"))?;
        for (id, _) in seats {
            anyhow::ensure!(registry.get(id).is_some(), "seat `{id}` did not register");
        }
        Ok(())
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
    /// The route failing to resolve.
    pub fn seat(
        tools: Arc<EpisodeTools>,
        briefs: &BTreeMap<String, String>,
        contract: &str,
        base: &Config,
        backend_url: &str,
        route: &Route,
        workspace: &Path,
    ) -> anyhow::Result<Self> {
        let mut config = base.clone();
        config.workspace_dir = workspace.to_path_buf();
        config.action_dir = workspace.to_path_buf();
        config.api_url = Some(backend_url.to_owned());
        config.default_model = Some(route.model.clone());
        let ephemeral =
            EphemeralRoute::from_params(Some(route.endpoint.clone()), Some(route.api_key.clone()))
                .ok_or_else(|| anyhow::anyhow!("a route needs both an endpoint and a key"))?;
        ephemeral_route::apply(&mut config, ephemeral);
        let config = Arc::new(config);
        // Resolve the `chat` role once, at seating: a route the factory
        // cannot resolve fails here rather than at the first turn.
        let (_, model) = openhuman_core::inference::provider::create_chat_model_with_model_id(
            "chat", &config, 0.0,
        )?;
        eprintln!("[route] chat resolves to model={model}");
        let seats = briefs
            .iter()
            .map(|(id, brief)| {
                (
                    id.clone(),
                    RawSeat::new(
                        id,
                        format!("{brief}\n\n{contract}"),
                        Arc::clone(&config),
                        model.clone(),
                        workspace.to_path_buf(),
                    ),
                )
            })
            .collect();
        Ok(Self {
            tools,
            seats,
            contexts: Arc::new(Mutex::new(BTreeMap::new())),
        })
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
    fn turn(&self, seat: String, lane: Lane, prompt: String) -> TurnJob {
        let history = self
            .contexts
            .lock()
            .expect("contexts are not poisoned")
            .get(&seat)
            .cloned()
            .unwrap_or_default();
        let belt = tools::belt(&seat, &self.tools);
        let raw_seat = self.seats[&seat].clone();
        let contexts = Arc::clone(&self.contexts);
        Box::pin(async move {
            let result = match raw_seat.turn(history, &prompt, belt).await {
                Ok(reply) => Some(Ok(reply)),
                Err(error) => Some(Err(error.to_string())),
            };
            if let Some(Ok(reply)) = &result {
                let mut contexts = contexts.lock().expect("contexts are not poisoned");
                let context = contexts.entry(seat.clone()).or_default();
                context.push(("user".to_owned(), prompt));
                context.push(("assistant".to_owned(), reply.clone()));
            }
            (seat, lane, result)
        })
    }
}
