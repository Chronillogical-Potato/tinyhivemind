//! The hosted runner: the host's own agents, run through the episode.
//!
//! The embed and raw runners seat agents this crate builds. A host with
//! agents of its own -- its model, its tools, its approval gate, its memory,
//! its prompt -- wants the episode run on those, and nothing about them
//! re-expressed as configuration here. This runner asks the host for exactly
//! three things, through [`EpisodeHost`]:
//!
//! - **Its log**, a [`SessionLog`](tinyhivemind::SessionLog) over the host's own journal, which is the
//!   only history there is. Each turn is seeded from it as the seat.
//! - **A seat**, built by the host with the episode's tools on its belt.
//!   `OpenHuman` fixes a session's belt when it is built, so the host builds
//!   each seat once per episode from an [`EpisodeBelt`], and the runner
//!   reuses it every turn.
//! - **A wrapper around each turn**, where the host installs whatever its
//!   tools and gate read while a turn runs -- a turn-scoped approval queue, a
//!   delegation context, a core context.
//!
//! And two things it may give: a **prefix** for the episode tools' names,
//! so none can share a name with a tool of its own and be admitted past its
//! gate; and an **after-turn hook**, handed the turn's usage, which is where
//! a host parks what the turn left waiting, meters its spend, and halts the
//! episode by returning an error.
//!
//! A turn, then: clear the seat's session, seed it with what the seat was
//! shown up to its watermark, send the brief inside the wrapper, record the
//! usage, and call the hook. The calls it made land in the shared record
//! like any other runner's.

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
use openhuman_embed::Agent;
use tinyhivemind::{Conversation, Sequence};
use tinyhivemind_driver::{AgentBinding, BoundAgent};
use tinyhivemind_tools::EpisodeTools;
use tinytools::Tool;

use crate::episode::Journal;
use crate::raw::tools::belt_with_prefix;
use crate::runner::{Lane, SeatRunner, TURN_TIMEOUT, TurnJob, TurnResult, unseated};
use crate::{Error, Result};
use admission::Admission;

/// One hosted turn, as the host wraps it.
pub type HostedTurn<'a> = Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>>;

/// What a host gives the hosted runner, beside the [`Journal`] it is.
pub trait EpisodeHost: Journal + 'static {
    /// The agent `seat` runs on, with `belt` reachable from it.
    ///
    /// The host returns the agent it would return anyway -- the same handle
    /// its pool holds, not a second one built for the episode. What the
    /// episode adds is `belt`: its tools, its advertised names, and
    /// [`EpisodeBelt::admit`] over the host's own policy.
    ///
    /// # Why a handle rather than a session
    ///
    /// A session fixes its belt when it is built, so a host used to build one
    /// session per seat per episode and the runner reused it. An `AgentSpec`
    /// composes its belt per turn instead, which means the episode's tools can
    /// reach the host's existing agent rather than a copy of it -- and a
    /// teammate stops existing twice, once as the pool's handle and once as
    /// the episode's session.
    ///
    /// The host keys `belt` by [`seat_session`](Self::seat_session) so its own
    /// per-turn belt can answer with the episode's tools for those turns and
    /// its ordinary ones otherwise.
    ///
    /// # Errors
    ///
    /// Whatever stops the host seating the agent.
    fn build_seat(&self, seat: &str, belt: EpisodeBeltSource) -> Result<Agent>;

    /// The conversation id every turn of this seat runs under.
    ///
    /// One id for the whole episode, minted by the host because the host is
    /// what has to recognise it: it is the key its per-turn belt looks up to
    /// know this turn is the episode's. The runner only passes it back.
    ///
    /// It must not collide with a conversation the host runs outside the
    /// episode, or that conversation's belt answers here.
    fn seat_session(&self, seat: &str) -> String;

    /// Wrap one turn. The default runs it as it is.
    fn wrap_turn<'a>(&'a self, seat: &'a str, turn: HostedTurn<'a>) -> HostedTurn<'a> {
        let _ = seat;
        turn
    }

    /// What the episode's tools are called, in front of their served names:
    /// `desk_` makes `read` into `desk_read`. The record is called by the
    /// served name either way. The default is no prefix.
    ///
    /// A host with tools of its own gives one, because [`EpisodeBelt::admit`]
    /// admits by name and a host tool sharing a bare name would be admitted
    /// past the host's gate. The brief and the desk's notes name the served
    /// vocabulary, so a host that prefixes says so in its own prompt.
    fn tool_prefix(&self) -> String {
        String::new()
    }

    /// The seat's own standing prompt, for every turn it runs.
    ///
    /// A seat's session is cleared and reseeded from the host's log every
    /// turn, and seeding brings the runtime session up before the turn runs.
    /// A session that already has one is not *cold*, and only a cold turn
    /// composes its system prompt -- so from a seat's second turn onward it
    /// ran with the brief and nothing else: no role, no team, no company. It
    /// still answered, which is why nothing complained; it simply was not
    /// being itself.
    ///
    /// Returning the text here puts it back at the head of the seeded
    /// history, where the turn reads it as the system message it would have
    /// composed. `None` keeps the old behaviour for a host that has no
    /// standing prompt to give.
    ///
    /// This is asked on a seat's **first** turn too. A cold turn composes its
    /// own prompt and takes the seed as well, so the two do not compete --
    /// and the first turn is the one a host most needs to reach, because what
    /// it says here is often *which side of a conversation this seat is on*.
    /// A seat that is asked a question answers on turn one and is never seen
    /// again; told nothing, it answers as though the room were its own.
    fn persona(&self, seat: &str) -> Option<String> {
        let _ = seat;
        None
    }

    /// After a turn ran, with its usage when the session reported any. The
    /// default meters nothing and lets the turn stand.
    ///
    /// This is where a host meters the spend and says what became of the
    /// turn: [`Disposition::Done`] for a turn that is what it is, and
    /// [`Disposition::Parked`] for one that stopped on something only the
    /// host can settle -- an approval it has queued -- which holds the seat
    /// until the host releases it. An error here is the turn's error, and
    /// the loop treats it as any failed turn.
    ///
    /// # Errors
    ///
    /// Whatever stops the episode.
    fn after_turn(&self, seat: &str, usage: Option<&LastTurnUsage>) -> Result<Disposition> {
        let _ = (seat, usage);
        Ok(Disposition::Done)
    }
}

/// What the host made of a turn that came back.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Disposition {
    /// Nothing is outstanding: the turn is what it is.
    #[default]
    Done,
    /// The turn stopped on something only the host can settle, and the host
    /// has taken it: the seat is held until it says otherwise.
    Parked,
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

/// What builds a seat's [`EpisodeBelt`], as often as a host needs one.
///
/// A belt is `Vec<Box<dyn Tool>>` and a `Box<dyn Tool>` is not `Clone`, so a
/// belt can be handed over once. An `AgentSpec` composes its belt **per turn**
/// from a factory that is `Fn`, not `FnOnce` -- so a host cannot close over a
/// belt it was given, only over what makes one. This is that.
///
/// Cheap to hold and to call: the tools are rebuilt from the episode's own
/// [`EpisodeTools`], which is shared.
#[derive(Clone)]
pub struct EpisodeBeltSource {
    seat: String,
    tools: Arc<EpisodeTools>,
    prefix: String,
}

impl std::fmt::Debug for EpisodeBeltSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EpisodeBeltSource")
            .field("seat", &self.seat)
            .field("prefix", &self.prefix)
            .finish_non_exhaustive()
    }
}

impl EpisodeBeltSource {
    /// A belt for this seat, built fresh.
    #[must_use]
    pub fn belt(&self) -> EpisodeBelt {
        EpisodeBelt::new(&self.seat, &self.tools, &self.prefix)
    }

    /// The seat this source belongs to.
    #[must_use]
    pub fn seat(&self) -> &str {
        &self.seat
    }
}

impl EpisodeBelt {
    fn new(seat: &str, tools: &Arc<EpisodeTools>, prefix: &str) -> Self {
        let tools = belt_with_prefix(seat, tools, prefix);
        let names = tools.iter().map(|tool| tool.name().to_owned()).collect();
        Self { tools, names }
    }

    /// The episode tools' names as the model calls them, prefixed, for a
    /// host that registers a seat's belt by name.
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
    agent: Agent,
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
            let belt = EpisodeBeltSource {
                seat: id.clone(),
                tools: Arc::clone(&tools),
                prefix: host.tool_prefix(),
            };
            let agent = host.build_seat(id, belt)?;
            built.insert(
                id.clone(),
                HostedSeat {
                    id: id.clone(),
                    agent,
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
    fn turn(&self, seat: String, lane: Lane, since: Option<Sequence>, prompt: String) -> TurnJob {
        let host = Arc::clone(&self.host);
        let Some(agent) = self.seats.get(&seat).map(|held| held.agent.clone()) else {
            return unseated(seat, lane);
        };
        let session_id = self.host.seat_session(&seat);
        let usage = Arc::clone(&self.usage);
        // Whatever the turn before left under this seat is not this turn's:
        // a turn that fails before the session reports anything is metered
        // as nothing, not as its predecessor.
        usage
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&seat);
        let conversation = Conversation {
            thread_root: match lane {
                Lane::Desk => None,
                Lane::Thread(root) => Some(root),
            },
            ..self.desk.clone()
        };
        let window = self.window;
        // This turn's usage travels with this turn: the per-seat map is the
        // host's `usage(seat)` view, and a turn that ran after this one on
        // the same seat may have written it before this one reads back.
        let this_turn: Arc<Mutex<Option<LastTurnUsage>>> = Arc::new(Mutex::new(None));
        Box::pin(async move {
            let run = {
                let seat = seat.clone();
                let host = Arc::clone(&host);
                let usage = Arc::clone(&usage);
                let this_turn = Arc::clone(&this_turn);
                async move {
                    let history =
                        seed::history(host.log(), conversation, &seat, since, window).await?;
                    // At the head, so it lands where a composed prompt would,
                    // and on **every** turn -- including a seat's first.
                    //
                    // This used to be guarded on `!history.is_empty()`, on the
                    // reasoning that a turn with nothing to seed "is cold, and
                    // that is the one case that renders the prompt itself". The
                    // premise is right and the conclusion does not follow. The
                    // host runtime matches `seed: Some(seed)`, which an empty
                    // vector satisfies, so the seeding branch runs either way;
                    // and what it runs is `clear_history`, which drops the
                    // conversation, not the definition's composed prompt. So a
                    // cold turn renders that prompt *and* would take a seed --
                    // it was simply never given one.
                    //
                    // What the guard actually did was withhold the persona from
                    // the only turn that has no other way to learn who it is. A
                    // host tells a seat here that it is a guest in someone
                    // else's conversation, and what it may do about that; a
                    // seat asked a question answers it on turn one and is never
                    // seen again, so under the guard it was told neither. Live:
                    // a teammate asked to take ownership of a piece of work
                    // replied that the *asker* owned it, having no idea which
                    // side of the conversation it was on.
                    let history = seed::with_persona(history, host.persona(&seat));
                    // `seed` is the whole of what the five calls here used to
                    // do: it drops whatever the session composed, puts this
                    // history in its place, and keeps the durable transcript
                    // from being reloaded over it. The journal is the only
                    // history a seat has, and this is how it becomes the turn's.
                    // Metered through the callback, not the outcome.
                    //
                    // A `TurnOutcome` is built on the success path only, so a
                    // turn that ran, called tools and then failed would report
                    // no spend at all -- undercounting exactly the turns that
                    // cost the most. `meter` fires after the turn settles and
                    // before its error is returned, so the hook below sees the
                    // spend either way.
                    let metered: Arc<Mutex<Option<LastTurnUsage>>> = Arc::new(Mutex::new(None));
                    let sink = Arc::clone(&metered);
                    let settled = tokio::time::timeout(
                        TURN_TIMEOUT,
                        agent
                            .turn(&prompt)
                            .session(session_id.clone())
                            .seed(history)
                            .meter(move |usage| {
                                *sink.lock().unwrap_or_else(PoisonError::into_inner) = usage;
                            })
                            .send(),
                    )
                    .await
                    .map_err(|_| Error::TimedOut { seat: seat.clone() })?;
                    // Read before the error is raised, for the same reason.
                    let last = metered
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .take();
                    let reply = settled
                        .map_err(|error| Error::Harness(anyhow::anyhow!(error.to_string())))?
                        .reply;
                    let mut metered = usage.lock().unwrap_or_else(PoisonError::into_inner);
                    match &last {
                        Some(last) => metered.insert(seat.clone(), last.clone()),
                        None => metered.remove(&seat),
                    };
                    drop(metered);
                    *this_turn.lock().unwrap_or_else(PoisonError::into_inner) = last;
                    Ok(reply)
                }
            };
            // Every started turn is finalized: the hook runs whether the
            // turn came back or not, with whatever usage the session
            // reported, so a host parks what a failed turn left waiting
            // too. A turn that failed keeps its own error over the hook's.
            let outcome = host.wrap_turn(&seat, Box::pin(run)).await;
            let last = this_turn
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            let finalized = host.after_turn(&seat, last.as_ref());
            let result = match (outcome, finalized) {
                (Ok(_), Ok(Disposition::Parked)) => TurnResult::Parked,
                (Ok(reply), Ok(Disposition::Done)) => TurnResult::Replied(reply),
                (Ok(_), Err(halt)) => TurnResult::Failed(halt.to_string()),
                (Err(error), _) => TurnResult::Failed(error.to_string()),
            };
            (seat, lane, result)
        })
    }
}
