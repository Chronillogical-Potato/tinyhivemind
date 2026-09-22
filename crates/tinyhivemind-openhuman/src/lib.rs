//! The `OpenHuman` adapter: how a seat's turn runs on `OpenHuman`.
//!
//! `tinyhivemind-driver` says who runs next and what a committed row means,
//! over a handle the host binds; it never runs a turn. This crate is the
//! host's side of that seam for `OpenHuman`. A host loop touches a seat at
//! three points -- open a turn, run it, close it and take what was called --
//! and the first and last are the same for every embedding, because
//! [`EpisodeTools`](tinyhivemind_tools::EpisodeTools) is where a call lands
//! whichever road it took. What genuinely varies is [`SeatRunner::turn`],
//! and there are three answers:
//!
//! - [`HostedRunner`]: a seat is the host's own agent -- its model, tools,
//!   approval gate, memory and prompt -- built by the host through
//!   [`EpisodeHost`] with the episode's tools added to its belt, and seeded
//!   every turn from the host's own log up to the seat's watermark. This is
//!   the runner for a host that already has agents.
//! - [`EmbedRunner`]: a seat is an `openhuman-embed` `AgentSpec` agent on a
//!   runtime the host booted, holding one session across the episode, and
//!   reaching the episode's tools through `OpenHuman`'s three MCP dispatchers
//!   against `tinyhivemind-mcp`'s server -- the only road a spec offers a
//!   tool the runtime did not ship.
//! - [`RawRunner`]: a seat is an `OpenHumanSessionHost` this crate builds on
//!   a [`LibraryHost`] every turn, handed the same tools natively, with a
//!   gate and a memory that keeps nothing, and seeded from a per-seat log it
//!   keeps itself.
//!
//! All three land every call in the same record, so the driver drains
//! identical events and a seat is refused and acknowledged in the same
//! words whichever runs it. The bound handle differs, which is what
//! [`BoundAgent`](tinyhivemind_driver::BoundAgent) is for.
//!
//! Above the runners sits [`run_episode`]: one episode from its door to
//! quiescence, over a [`Journal`] the host implements. A host builds its
//! driver, its door and a runner, and calls it.
//!
//! This is the one crate in the workspace that links a harness. A host that
//! seats agents some other way does not link it; it implements `BoundAgent`
//! and `SeatRunner` itself.
//!
//! # Example
//!
//! A hosted seat. The host has a log and knows how to build its agents; here
//! the agent is a library session with nothing but the episode's tools, and
//! the wrapper is the core context such a session runs under.
//!
//! ```no_run
//! use std::sync::Arc;
//! use openhuman_core::agent::OpenHumanSessionHost;
//! use tinyhivemind::{SESSION_WINDOW, Sequence, SessionLog};
//! use tinyhivemind_driver::{Commit, Note};
//! use tinyhivemind_openhuman::MemoryLog;
//! use tinyhivemind_openhuman::{
//!     EpisodeBelt, EpisodeHost, HostedRunner, HostedTurn, Journal, Lane, LibraryHost, SeatRunner,
//! };
//! use tinyhivemind_tools::{Dispatch, EpisodeTools};
//!
//! struct Desk {
//!     log: MemoryLog,
//!     library: LibraryHost,
//! }
//!
//! // The journal: a real host reads and appends its own; this one is in memory.
//! impl Journal for Desk {
//!     fn log(&self) -> &dyn SessionLog {
//!         &self.log
//!     }
//!
//!     fn commit(&self, commit: &Commit) -> tinyhivemind_openhuman::Result<Sequence> {
//!         Ok(self.log.append(&commit.author, commit.utterance.message(), commit.thread, None))
//!     }
//!
//!     fn note(&self, note: &Note) -> tinyhivemind_openhuman::Result<()> {
//!         self.log.append("desk", &note.body, note.thread, note.only_for.as_deref());
//!         Ok(())
//!     }
//! }
//!
//! impl EpisodeHost for Desk {
//!     fn build_seat(
//!         &self,
//!         seat: &str,
//!         belt: EpisodeBelt,
//!     ) -> tinyhivemind_openhuman::Result<OpenHumanSessionHost> {
//!         // A real host builds the agent it always builds, adds `belt.tools`,
//!         // and passes its own gate here instead of `None`.
//!         let gate = belt.admit(None);
//!         self.library.session(seat, "You lead the desk.", belt.tools, gate)
//!     }
//!
//!     fn wrap_turn<'a>(&'a self, _seat: &'a str, turn: HostedTurn<'a>) -> HostedTurn<'a> {
//!         Box::pin(self.library.scope(turn))
//!     }
//! }
//!
//! # async fn run(desk: Desk) -> tinyhivemind_openhuman::Result<()> {
//! let runner = HostedRunner::seat(
//!     Arc::new(desk),
//!     Arc::new(EpisodeTools::new(["lead"])),
//!     &["lead".to_owned()],
//!     "engineering",
//!     "Engineering",
//!     SESSION_WINDOW,
//! )?;
//! runner.open("lead", Vec::new(), Dispatch { chat: "engineering".into(), parent: None });
//! // The newest row `lead` was shown before this turn: its history is read
//! // from the host's log up to here, and the brief carries what is above.
//! // `None` would be a seat shown nothing yet.
//! let since = Some(tinyhivemind::Sequence(1));
//! let (_, _, reply) = runner.turn("lead".into(), Lane::Desk, since, "Go.".into()).await;
//! let events = runner.close("lead");
//! # let _ = (reply, events);
//! # Ok(())
//! # }
//! ```

pub mod embed;
pub mod episode;
pub mod error;
pub mod hosted;
pub mod journal;
#[cfg(any(test, feature = "offline"))]
pub mod offline;
pub mod raw;
pub mod runner;

pub use embed::{EmbedRunner, EmbedSeat};
pub use episode::{Journal, Report, run_episode};
pub use error::{Error, Result};
pub use hosted::{EpisodeBelt, EpisodeHost, HostedRunner, HostedSeat, HostedTurn};
pub use journal::MemoryLog;
pub use raw::{LibraryHost, RawRunner, RawSeat, Route, register_seats};
pub use runner::{Lane, RunnerKind, SeatRunner, TURN_TIMEOUT, TurnJob, TurnResult};
