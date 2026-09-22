//! How a seat's turn runs, and how its calls come back: the one seam.
//!
//! The host loop owns the journal, the lanes, the briefs and the driver. It
//! touches the seat itself at three points only -- open a turn, run it, close
//! it and take what was called -- and the first and last are the same for
//! every embedding, because [`EpisodeTools`] is where a call lands whichever
//! road it took. What genuinely varies is [`SeatRunner::turn`]: whether a
//! seat is an `openhuman-embed` agent reaching its tools over MCP, or a raw
//! `OpenHuman` session handed the same tools natively. Both implement this
//! trait, and the loop cannot tell them apart.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tinyhivemind::Sequence;
use tinyhivemind_driver::{AgentBinding, BoundAgent};
use tinyhivemind_tools::{Dispatch, EpisodeTools, SeatEvent};

use crate::{Error, Result};

/// How long one agent turn may take before the episode gives up on it.
pub const TURN_TIMEOUT: Duration = Duration::from_secs(300);

/// Where a turn is running, for the host's own bookkeeping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lane {
    /// The desk itself.
    Desk,
    /// A conversation on a thread of the desk, by its root row.
    Thread(Sequence),
}

/// A turn's reply, once it is back: `None` timed out.
pub type TurnResult = Option<std::result::Result<String, String>>;

/// One running turn: the seat, its lane, and the reply when it lands.
pub type TurnJob = Pin<Box<dyn Future<Output = (String, Lane, TurnResult)> + Send>>;

/// Which runner the episode uses, chosen by `TINYHIVEMIND_RUNNER`.
///
/// A host that decides some other way names a variant directly; the parser
/// is for one that reads the environment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunnerKind {
    /// `openhuman-embed` agents, tools over MCP. The default.
    Embed,
    /// Raw `OpenHumanSessionHost` sessions, tools in-process.
    Raw,
    /// The host's own agents, tools in-process, seeded from the host's log.
    Hosted,
}

impl RunnerKind {
    /// `TINYHIVEMIND_RUNNER=embed` (default) or `raw`.
    ///
    /// # Errors
    ///
    /// Any other value: a typo must not silently run the default.
    pub fn from_env() -> Result<Self> {
        let value = std::env::var("TINYHIVEMIND_RUNNER").ok();
        Self::parse(value.as_deref()).map_err(Error::UnknownRunner)
    }

    /// The parser behind [`Self::from_env`]: the offending value on a miss.
    ///
    /// # Errors
    ///
    /// The value, when it names neither runner.
    pub fn parse(value: Option<&str>) -> std::result::Result<Self, String> {
        match value {
            None | Some("" | "embed") => Ok(Self::Embed),
            Some("raw") => Ok(Self::Raw),
            Some("hosted") => Ok(Self::Hosted),
            Some(other) => Err(other.to_owned()),
        }
    }

    /// The runner's name in the log.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Embed => "embed",
            Self::Raw => "raw",
            Self::Hosted => "hosted",
        }
    }

    /// The host's one sentence on the mechanics, for the standing contract.
    /// Everything else in that contract is the vocabulary's own words.
    #[must_use]
    pub const fn how_to_call(self) -> &'static str {
        match self {
            Self::Embed => {
                "Use `mcp_call_tool` with `server: \"episode\"`; its `arguments` is a JSON \
                 object, never a string."
            }
            Self::Raw | Self::Hosted => "Each tool below is yours to call directly, by its name.",
        }
    }
}

/// The seam.
///
/// `open` and `close` are provided: both runners record into the same
/// [`EpisodeTools`], so the window a seat may `read`, the chat and parent its
/// calls must name, and the events drained after its turn are handled once
/// here. A runner supplies its seats for the hive's bindings and runs a turn.
pub trait SeatRunner: Send + Sync {
    /// The shared record every call lands in.
    fn tools(&self) -> &Arc<EpisodeTools>;

    /// The handle the hive binds per seat.
    ///
    /// The completion driver stores a bound handle and hands it back with a
    /// pending round; it never runs one. So a runner binds whatever it runs
    /// seats with -- an `openhuman-embed` agent, or its own seat type.
    type Bound: BoundAgent + 'static;

    /// One binding per seat, canonical id to handle.
    fn bindings(&self) -> Vec<AgentBinding<Self::Bound>>;

    /// Run one turn. The prompt is what the seat is shown this turn; `since`
    /// is the newest row it was shown before it, which a runner that seeds
    /// from the host's log reads up to. How a seat holds context between
    /// turns is the runner's business.
    fn turn(&self, seat: String, lane: Lane, since: Sequence, prompt: String) -> TurnJob;

    /// Open a turn: what the seat may `read`, and the chat and parent every
    /// call it makes must name.
    fn open(&self, seat: &str, window: Vec<String>, dispatch: Dispatch) {
        self.tools().window(seat, window);
        self.tools().register(seat, dispatch);
    }

    /// Close the turn and take what the seat called in, in call order.
    fn close(&self, seat: &str) -> Vec<SeatEvent> {
        self.tools().clear(seat);
        self.tools().drain(seat)
    }
}

#[cfg(test)]
mod test;
