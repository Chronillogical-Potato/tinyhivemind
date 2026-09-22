//! The hosted runner: the host's own agents, run through the episode.
//!
//! The embed and raw runners seat agents this crate builds. A host with
//! agents of its own -- its model, its tools, its approval gate, its memory,
//! its prompt -- wants the episode run on those, and nothing about them
//! re-expressed as configuration here. This runner asks the host for exactly
//! three things, through [`EpisodeHost`]:
//!
//! - **Its log**, a [`SessionLog`] over the host's own journal, which is the
//!   only history there is. Each turn is seeded from it as the seat.
//! - **A seat**, built by the host with the episode's tools on its belt.
//!   `OpenHuman` fixes a session's belt when it is built, so the host builds
//!   each seat once per episode from an [`EpisodeBelt`], and the runner
//!   reuses it every turn.
//! - **A wrapper around each turn**, where the host installs whatever its
//!   tools and gate read while a turn runs -- a turn-scoped approval queue, a
//!   delegation context, a core context.
//!
//! A turn, then: clear the seat's session, seed it with what the seat was
//! shown up to its watermark, send the brief, and record the usage. The
//! calls it made land in the shared record like any other runner's.

mod admission;
mod seed;
#[cfg(test)]
mod test;

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};

use openhuman_core::agent::tinyagents::host::LastTurnUsage;
use openhuman_core::agent::tool_policy::ToolPolicy;
use openhuman_core::agent::{OpenHumanSessionHost, TurnOverrides};
use tinyhivemind::{Conversation, Sequence, SessionLog};
use tinyhivemind_driver::{AgentBinding, BoundAgent};
use tinyhivemind_tools::EpisodeTools;
use tinytools::Tool;

use crate::raw::tools::belt;
use crate::runner::{Lane, SeatRunner, TURN_TIMEOUT, TurnJob};
use crate::{Error, Result};
use admission::Admission;

/// One hosted turn, as the host wraps it.
pub type HostedTurn<'a> = Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>>;

/// What a host gives the hosted runner.
pub trait EpisodeHost: Send + Sync + 'static {
    /// The host's journal, read as a seat to seed each turn.
    fn log(&self) -> &dyn SessionLog;

    /// Build the session `seat` runs on, with `belt` on it.
    ///
    /// The host builds the agent it would build anyway, adds `belt.tools` to
    /// its belt, and gates it with [`EpisodeBelt::admit`] over its own
    /// policy. The session is reused for every turn of the episode.
    ///
    /// # Errors
    ///
    /// Whatever stops the host building the seat.
    fn build_seat(&self, seat: &str, belt: EpisodeBelt) -> Result<OpenHumanSessionHost>;

    /// Wrap one turn. The default runs it as it is.
    fn wrap_turn<'a>(&'a self, seat: &'a str, turn: HostedTurn<'a>) -> HostedTurn<'a> {
        let _ = seat;
        turn
    }
}

/// The episode's tools for one seat, and the gate that admits them.
pub struct EpisodeBelt {
    /// The tools, each bound to this seat, each calling the shared record.
    pub tools: Vec<Box<dyn Tool>>,
    names: Vec<String>,
}

impl std::fmt::Debug for EpisodeBelt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EpisodeBelt")
            .field("names", &self.names)
            .finish_non_exhaustive()
    }
}

impl EpisodeBelt {
    fn new(seat: &str, tools: &Arc<EpisodeTools>) -> Self {
        let tools = belt(seat, tools);
        let names = tools.iter().map(|tool| tool.name().to_owned()).collect();
        Self { tools, names }
    }

    /// The episode tools' names, for a host that registers a seat's belt by
    /// name.
    #[must_use]
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// A policy that admits the episode's tools and asks `host` about every
    /// other call. `None` denies every other call.
    #[must_use]
    pub fn admit(&self, host: Option<Arc<dyn ToolPolicy>>) -> Arc<dyn ToolPolicy> {
        Arc::new(Admission::new(self.names.clone(), host))
    }
}

/// A hosted seat, as the driver binds it: its id and the session it runs on.
#[derive(Clone)]
pub struct HostedSeat {
    id: String,
    session: Arc<tokio::sync::Mutex<OpenHumanSessionHost>>,
}

impl BoundAgent for HostedSeat {
    fn runtime_id(&self) -> &str {
        &self.id
    }
}

impl std::fmt::Debug for HostedSeat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostedSeat")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// Seats as the host's own agents, seeded from the host's log every turn.
pub struct HostedRunner<H: EpisodeHost> {
    host: Arc<H>,
    tools: Arc<EpisodeTools>,
    seats: BTreeMap<String, HostedSeat>,
    desk: Conversation,
    window: usize,
    usage: Arc<Mutex<BTreeMap<String, LastTurnUsage>>>,
}

impl<H: EpisodeHost> std::fmt::Debug for HostedRunner<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostedRunner")
            .field("seats", &self.seats.keys().collect::<Vec<_>>())
            .field("desk", &self.desk.desk_id)
            .finish_non_exhaustive()
    }
}

impl<H: EpisodeHost> HostedRunner<H> {
    /// Ask `host` to build every seat, once, with its episode belt.
    ///
    /// `desk` names the desk every turn runs on or in a thread of, as the
    /// host's log knows it. `window` bounds how many rows a turn is seeded
    /// with; `tinyhivemind::SESSION_WINDOW` is the default the rest of the
    /// crate reads with.
    ///
    /// # Errors
    ///
    /// The host failing to build a seat.
    pub fn seat(
        host: Arc<H>,
        tools: Arc<EpisodeTools>,
        seats: &[String],
        desk: &str,
        desk_name: &str,
        window: usize,
    ) -> Result<Self> {
        let mut built = BTreeMap::new();
        for id in seats {
            let session = host.build_seat(id, EpisodeBelt::new(id, &tools))?;
            built.insert(
                id.clone(),
                HostedSeat {
                    id: id.clone(),
                    session: Arc::new(tokio::sync::Mutex::new(session)),
                },
            );
        }
        Ok(Self {
            host,
            tools,
            seats: built,
            desk: Conversation {
                desk_id: desk.into(),
                desk_name: desk_name.into(),
                thread_root: None,
            },
            window,
            usage: Arc::new(Mutex::new(BTreeMap::new())),
        })
    }

    /// The usage of `seat`'s last turn, for a host that meters it.
    #[must_use]
    pub fn usage(&self, seat: &str) -> Option<LastTurnUsage> {
        self.usage
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(seat)
            .cloned()
    }
}

impl<H: EpisodeHost> SeatRunner for HostedRunner<H> {
    fn tools(&self) -> &Arc<EpisodeTools> {
        &self.tools
    }

    type Bound = HostedSeat;

    fn bindings(&self) -> Vec<AgentBinding<HostedSeat>> {
        self.seats
            .iter()
            .map(|(id, seat)| AgentBinding::new(id.clone(), seat.clone()))
            .collect()
    }

    /// Clear the seat's session, seed it from the host's log up to `since`,
    /// and run the brief, inside the host's wrapper.
    fn turn(&self, seat: String, lane: Lane, since: Sequence, prompt: String) -> TurnJob {
        let host = Arc::clone(&self.host);
        let session = Arc::clone(&self.seats[&seat].session);
        let usage = Arc::clone(&self.usage);
        let conversation = Conversation {
            thread_root: match lane {
                Lane::Desk => None,
                Lane::Thread(root) => Some(root),
            },
            ..self.desk.clone()
        };
        let window = self.window;
        Box::pin(async move {
            let run = {
                let seat = seat.clone();
                let host = Arc::clone(&host);
                async move {
                    let history =
                        seed::history(host.log(), conversation, &seat, since, window).await?;
                    let mut session = session.lock().await;
                    // Clearing drops the runtime session, and with it the
                    // turn state, so the seed and the overrides go after it.
                    session.clear_history();
                    session.seed_resume_from_messages(history, &prompt)?;
                    session.set_next_turn_overrides(TurnOverrides {
                        suppress_transcript_autoload: true,
                        ..TurnOverrides::default()
                    });
                    let reply = tokio::time::timeout(TURN_TIMEOUT, session.turn(&prompt))
                        .await
                        .map_err(|_| Error::TimedOut { seat: seat.clone() })?
                        .map_err(Error::Harness)?;
                    if let Some(last) = session.last_turn_usage() {
                        usage
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .insert(seat.clone(), last);
                    }
                    Ok(reply)
                }
            };
            let result = host.wrap_turn(&seat, Box::pin(run)).await;
            (seat, lane, Some(result.map_err(|error| error.to_string())))
        })
    }
}
