//! What a seat may say, what makes it valid, and what one accepted
//! utterance becomes.

use serde::{Deserialize, Serialize};
use tinyhivemind_core::{
    aside::{Audience, NoAsideReason},
    mention::Mention,
};

/// What a seat asked the room for during one turn.
///
/// The serde representation is the wire form a host writes to whatever it
/// collects a turn's calls in, so it is pinned by a unit test: a host and a
/// runtime that disagree about a field name lose a turn to a decode error.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Utterance {
    /// A message for the whole desk.
    Post {
        /// The text to append.
        message: String,
    },
    /// A desk-visible message that asks the host to select its recipients
    /// semantically from the current team.
    Broadcast {
        /// The work or finding to route through the host's semantic router.
        message: String,
    },
    /// A message for named peers only.
    Dm {
        /// The peers addressed, without the `@`.
        to: Vec<String>,
        /// The text to append.
        message: String,
    },
    /// A private conversation opened with the peers it names: one, or a
    /// group of them.
    ///
    /// Unlike a [`Dm`](Self::Dm) it opens an obligation: the asker cannot
    /// complete its assignment until the conversation has concluded and its
    /// outcome has reached it. The seats asked keep whatever they were doing
    /// — this is a question, not a handoff.
    ///
    /// Naming several opens **one** conversation holding all of them, not one
    /// each: they read each other's answers and can work the question out
    /// between themselves, and it concludes when every one of them has
    /// answered. Naming one is that same conversation with a single peer in
    /// it, which is what every ask was before a group could be named.
    Ask {
        /// The peers asked, without the `@`, in the order they were named.
        to: Vec<String>,
        /// The question.
        message: String,
    },
    /// A message that also reports the author's current assignment finished.
    ///
    /// The seat is reporting, not ending the desk: the host still appends the
    /// message and decides whether every assigned participant is complete.
    #[serde(rename = "complete_episode", alias = "close")]
    CompleteEpisode {
        /// The text to append.
        message: String,
    },
}

impl Utterance {
    /// The text of the message, whoever it is for.
    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            Self::Post { message }
            | Self::Broadcast { message }
            | Self::Dm { message, .. }
            | Self::Ask { message, .. }
            | Self::CompleteEpisode { message } => message,
        }
    }

    /// Legacy spelling for whether this utterance reports assignment completion.
    #[must_use]
    pub const fn closing(&self) -> bool {
        matches!(self, Self::CompleteEpisode { .. })
    }

    /// Whether accepting this utterance records this agent's assignment as
    /// complete in a completion-driven episode.
    #[must_use]
    pub const fn completes_episode(&self) -> bool {
        matches!(self, Self::CompleteEpisode { .. })
    }

    /// Whether accepting this utterance requests semantic team routing.
    #[must_use]
    pub const fn broadcasting(&self) -> bool {
        matches!(self, Self::Broadcast { .. })
    }

    /// The seats this utterance asks, when it is an [`Ask`](Self::Ask).
    ///
    /// A host holds the asker's completion open until every one of them has
    /// answered. Empty for anything else, so a caller that only wants the
    /// recipients does not match on the variant.
    #[must_use]
    pub fn asks(&self) -> &[String] {
        match self {
            Self::Ask { to, .. } => to,
            _ => &[],
        }
    }
}

/// One tool call a seat made, once the room has read it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolCall {
    /// The seat said something. The host decides what that becomes.
    Speak(Utterance),
    /// The seat asked to read further back than the window it was handed.
    Read {
        /// How many recent messages it asked for, already clamped.
        limit: usize,
    },
}

/// Why a tool call is not an utterance.
///
/// This is a value handed back to its author *inside its own turn*, not a
/// crate error: a seat that spelled a call wrong can spell it again, which is
/// the whole gain over a delimiter it can only mis-write once. The `Display`
/// text is written to be read by the seat.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum UtteranceRejection {
    /// No tool of that name is served.
    #[error("unknown tool {name}")]
    UnknownTool {
        /// What the seat called.
        name: String,
    },
    /// A required text argument was absent, empty, or all whitespace.
    #[error("`{field}` must be a non-empty string")]
    EmptyText {
        /// Which argument.
        field: &'static str,
    },
    /// A private message named nobody.
    #[error("`to` must name at least one seat")]
    NoRecipients,
    /// A private message named somebody who cannot receive one.
    #[error("`to` names @{id}, who is not an active seat on this desk")]
    UnknownRecipient {
        /// The id as the seat wrote it, without the `@`.
        id: String,
    },
    /// A private message named its own author.
    #[error("`to` names you; a message to yourself reaches nobody else")]
    SelfRecipient,
    /// An `ask` named more than one seat.
    #[error(
        "`ask` puts a question to one seat; {count} were named. Use `ask_teammates` to put one question to several seats at once, in a conversation they are all in"
    )]
    OneRecipient {
        /// How many the seat wrote.
        count: usize,
    },
    /// An `ask_teammates` named a single seat.
    #[error(
        "`ask_teammates` is for two or more seats, in one conversation together; `to` named one. Use `ask` for a question only that seat can settle"
    )]
    NotAGroup,
}

/// The arguments of one tool call, in the shapes the tools declare.
///
/// The host maps its own wire — JSON over MCP, a typed tool trait, anything —
/// onto this. Nothing in this crate parses JSON, so this is where a host's
/// representation stops and the algebra starts.
#[derive(Clone, Copy, Debug, Default)]
pub struct CallArguments<'a> {
    /// The `message` argument of every speaking tool.
    pub message: Option<&'a str>,
    /// The `to` argument of `dm`.
    pub to: &'a [String],
    /// The `limit` argument of `read`, before clamping.
    pub limit: Option<u64>,
}

/// What one accepted utterance becomes, once the room has resolved it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommittedUtterance {
    /// The text of the row to append.
    pub content: String,
    /// Who may read it, resolved by [`aside`](tinyhivemind_core::aside::aside).
    pub audience: Audience,
    /// The mentions dispatch reads to decide whose turn is next.
    pub mentions: Vec<Mention>,
    /// Legacy completion flag retained for hosts built against `close`.
    pub closing: bool,
    /// Whether the author explicitly completed its current episode assignment.
    pub completes_episode: bool,
    /// Whether the appended desk row must be routed semantically to teammates.
    pub broadcasting: bool,
    /// The seats this row asks, when it is an `ask`; empty otherwise.
    ///
    /// The host holds the author's completion open until every one of them
    /// answers, and opens one conversation holding all of them.
    pub asks: Vec<String>,
    /// Why a requested aside was declined, when one was.
    ///
    /// A refusal is not a failure: the row is appended to the whole desk,
    /// which is the safe direction to fail in. It is carried so a host can say
    /// so — to its operator, and to the seat while its turn is still running.
    pub refusal: Option<NoAsideReason>,
}

/// What shape a tool argument's value takes.
///
/// Deliberately small. A host renders these into its own schema language, and
/// a shape this enum cannot express is one the room should not be asking a
/// model to produce.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParameterKind {
    /// One string.
    Text,
    /// A list of strings.
    TextList,
    /// A bounded whole number, with the value used when it is absent.
    Count {
        /// What the room uses when the argument is not supplied.
        default: u64,
        /// The smallest accepted value; anything lower is clamped up.
        min: u64,
        /// The largest accepted value; anything higher is clamped down.
        max: u64,
    },
}
