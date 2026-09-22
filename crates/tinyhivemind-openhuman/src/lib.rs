//! The `OpenHuman` adapter: how a seat's turn runs on `OpenHuman`, both ways.
//!
//! `tinyhivemind-driver` says who runs next and what a committed row means,
//! over a handle the host binds; it never runs a turn. This crate is the
//! host's side of that seam for `OpenHuman`. A host loop touches a seat at
//! three points -- open a turn, run it, close it and take what was called --
//! and the first and last are the same for every embedding, because
//! [`EpisodeTools`](tinyhivemind_tools::EpisodeTools) is where a call lands
//! whichever road it took. What genuinely varies is [`SeatRunner::turn`],
//! and there are two answers:
//!
//! - [`EmbedRunner`]: a seat is an `openhuman-embed` `AgentSpec` agent on a
//!   runtime the host booted, holding one session across the episode, and
//!   reaching the episode's tools through `OpenHuman`'s three MCP dispatchers
//!   against `tinyhivemind-mcp`'s server -- the only road a spec offers a
//!   tool the runtime did not ship.
//! - [`RawRunner`]: a seat is an `OpenHumanSessionHost` built one level down
//!   on every turn, handed the same tools natively as its belt, with a policy
//!   gate and a memory that keeps nothing, and seeded from a per-seat log this
//!   crate keeps.
//!
//! Both land every call in the same record, so the driver drains identical
//! events and a seat is refused and acknowledged in the same words either
//! way. The bound handle differs -- [`EmbedSeat`] wraps the agent, [`RawSeat`]
//! is the seat itself -- which is what [`BoundAgent`](tinyhivemind_driver::BoundAgent)
//! is for.
//!
//! This is the one crate in the workspace that links a harness. A host that
//! seats agents some other way does not link it; it implements `BoundAgent`
//! and `SeatRunner` itself.
//!
//! # Example
//!
//! A raw seat, against any OpenAI-compatible endpoint: the route is the
//! credential. The same steps seat one against the scripted model the
//! `offline` feature ships, which is how the crate's own tests prove it.
//!
//! ```no_run
//! use std::sync::Arc;
//! use openhuman_embed::RuntimeConfig;
//! use tinyhivemind_openhuman::{Lane, RawRunner, Route, SeatRunner};
//! use tinyhivemind_tools::{Dispatch, EpisodeTools};
//!
//! # async fn run() -> tinyhivemind_openhuman::Result<()> {
//! let workspace = std::env::temp_dir().join("episode");
//! // Every seat is a registered definition before a raw session runs: the
//! // hosted turn resolves the seat, and its belt, by name.
//! RawRunner::prepare(&workspace, &[("lead", "You lead the desk.")])?;
//! let runner = RawRunner::seat(
//!     Arc::new(EpisodeTools::new(["lead"])),
//!     &[("lead".to_owned(), "You lead the desk.".to_owned())].into_iter().collect(),
//!     "Call `complete_episode` when you are done.",
//!     &RuntimeConfig::default(),
//!     "http://127.0.0.1:1/backend",
//!     &Route {
//!         endpoint: "http://127.0.0.1:1/v1".into(),
//!         api_key: "key".into(),
//!         model: "a-model".into(),
//!     },
//!     &workspace,
//! )
//! .await?;
//! runner.open("lead", Vec::new(), Dispatch { chat: "engineering".into(), parent: None });
//! let (_, _, reply) = runner.turn("lead".into(), Lane::Desk, "Go.".into()).await;
//! let events = runner.close("lead");
//! # let _ = (reply, events);
//! # Ok(())
//! # }
//! ```

pub mod embed;
pub mod error;
#[cfg(any(test, feature = "offline"))]
pub mod offline;
pub mod raw;
pub mod runner;

pub use embed::{EmbedRunner, EmbedSeat};
pub use error::{Error, Result};
pub use raw::{RawRunner, RawSeat, Route};
pub use runner::{Lane, RunnerKind, SeatRunner, TURN_TIMEOUT, TurnJob, TurnResult};
