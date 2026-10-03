//! The room's tool surface, stated once, as data.
//!
//! A host renders these into whatever its agents call — a JSON schema over
//! MCP, an implementation of a tool trait, a function registry. Nothing here
//! is JSON, because JSON handling is not in this crate's dependency graph and
//! a schema is not the only shape a host needs.
//!
//! The descriptions are contract text rather than documentation. They are the
//! only place a seat is told that what it writes outside a tool call reaches
//! nobody, and the only place it is told that `complete_episode` is different
//! from one more `post`. A host renders them verbatim.
//!
//! The names here are bare. A host that namespaces its tools — an MCP server
//! called `desk` serving `post` presents it as `desk_post` — prefixes them,
//! and the descriptions are written to read correctly either way.

use super::types::ParameterKind;

/// One argument a tool takes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolParameter {
    /// The argument name, as the seat writes it.
    pub name: &'static str,
    /// What the seat should put in it, or `None` where the tool's own
    /// description already says.
    pub description: Option<&'static str>,
    /// What shape the value takes.
    pub kind: ParameterKind,
    /// Whether a call without it is refused.
    pub required: bool,
}

/// One tool a seat may call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolSpec {
    /// The bare tool name, before any host namespacing.
    pub name: &'static str,
    /// What the tool does, written for the seat that will call it.
    pub description: &'static str,
    /// The arguments it takes, in the order they should be presented.
    pub parameters: &'static [ToolParameter],
}

/// Every tool the room serves, in the order a seat should meet them.
///
/// `post` is first because it is the one a seat must call; `read` is last
/// because it is the one a seat should rarely need.
#[must_use]
pub const fn tool_specs() -> &'static [ToolSpec] {
    SPECS
}

const SPECS: &[ToolSpec] = &[
    ToolSpec {
        name: "post",
        description: "State one thing to the whole desk: a fact you hold, a finding, or your \
                      answer to a question a peer asked you. A person reads this in the chat, \
                      and so does the desk: lead with the point in plain words, keep it short, \
                      and refer to teammates by name. Nobody is assigned anything by it. If what \
                      you found is work that belongs to another seat, that is `broadcast`, not \
                      this. Text you write outside a tool call is your own thinking and reaches \
                      nobody.",
        parameters: &[ToolParameter {
            name: "message",
            description: Some(
                "The fact, plainly, for a person to read. Not a request, and not a handoff.",
            ),
            kind: ParameterKind::Text,
            required: true,
        }],
    },
    ToolSpec {
        name: "broadcast",
        description: "Send work or a finding to whichever teammates are semantically best placed \
                      to take it. The host routes this message with one TypeSafe Choice over the \
                      currently eligible team; it is not a broadcast-to-all fan-out. Call this \
                      when you hold work that belongs to another seat, whoever that turns out \
                      to be. One piece of work per call: two things for two seats are two calls. \
                      A person reads this in the chat, and so does the teammate who takes it: \
                      say plainly what needs doing and why, keep it short, and refer to \
                      teammates by name. Handing work off is a finding: unless you are waiting \
                      on a question you asked, it completes your part, and you need not call \
                      `complete_episode` after it.",
        parameters: &[ToolParameter {
            name: "message",
            description: Some(
                "The work, finding, or request, self-contained and in plain words: a person \
                 reads it, and so does the teammate who takes it up.",
            ),
            kind: ParameterKind::Text,
            required: true,
        }],
    },
    ToolSpec {
        name: "dm",
        description: "Say one thing to named peers instead of the whole desk. Use it to settle a \
                      disagreement without spending the room's attention; the room is told the \
                      exchange happened and not what it said. A person can still read it, so \
                      write it plainly. It still costs your one message for the turn.",
        parameters: &[
            ToolParameter {
                name: "to",
                description: Some("Seat ids, not names, without the @."),
                kind: ParameterKind::TextList,
                required: true,
            },
            ToolParameter {
                name: "message",
                description: None,
                kind: ParameterKind::Text,
                required: true,
            },
        ],
    },
    ToolSpec {
        name: "ask",
        description: "Open a private conversation with one named seat about something you \
                      need from it. It runs on its own, not while you wait: say what you need, \
                      then end your turn, and their answer reaches you on a later turn. You \
                      will not be able to finish until every question you asked has been \
                      answered, so nothing you asked for can be lost. Ask everyone you need in \
                      one turn; each ask is its own conversation, and they cannot read each \
                      other. When the answer depends on two seats agreeing, put them in one \
                      room with `ask_teammates` instead. If an answer raises another question, \
                      ask again. It is a question, not a handoff — work that belongs to \
                      another seat is `broadcast`. The seat you ask keeps whatever it was \
                      already doing. A person can read the question, so write it plainly.",
        parameters: &[
            ToolParameter {
                name: "to",
                description: Some("The seat to ask, by id rather than name, without the @."),
                kind: ParameterKind::Text,
                required: true,
            },
            ToolParameter {
                name: "message",
                description: Some(
                    "What you need from them, self-contained and in plain words: the question and why it matters to your work.",
                ),
                kind: ParameterKind::Text,
                required: true,
            },
        ],
    },
    ToolSpec {
        name: "ask_teammates",
        description: "Put one question to several seats at once, in a conversation they are \
                      all in together. Use it when the answer depends on them agreeing: they \
                      read each other's answers and can settle the disagreement between \
                      themselves, which separate questions cannot do — you would get two \
                      answers written in ignorance of each other and be the only one who \
                      noticed they conflict. It runs on its own, not while you wait, and it \
                      concludes once every seat you named has answered; you cannot finish \
                      until it does. Name only the seats whose agreement you need — every \
                      extra seat is another turn the room waits for. For something only one \
                      seat can settle, use `ask`.",
        parameters: &[
            ToolParameter {
                name: "to",
                description: Some(
                    "A list of two or more seat ids, by id rather than name, without the @. Only an id listed here is a seat; there is no name for the group.",
                ),
                kind: ParameterKind::TextList,
                required: true,
            },
            ToolParameter {
                name: "message",
                description: Some(
                    "What you need from them, self-contained and in plain words: the question, why it matters to your work, and what you need them to agree on.",
                ),
                kind: ParameterKind::Text,
                required: true,
            },
        ],
    },
    ToolSpec {
        name: "complete_episode",
        description: "Conclude your part with one message, and that message is your finding: \
                      what you established, and why nothing is left open on your side. This is \
                      how a fact reaches the desk. A person reads this in the chat, and so does \
                      the desk: lead with the result in plain words, keep it short, include \
                      only what the reader needs to act on, and refer to teammates by name. Call \
                      it when you have no open step -- or, when a peer asked you something, as \
                      your answer. The episode completes after every assigned seat has called \
                      it; a later routed broadcast may assign new work and reopen a seat.",
        parameters: &[ToolParameter {
            name: "message",
            description: Some(
                "Your finding, complete in itself and written for a person: the result first, \
                 in plain words, then only what the reader needs to act on.",
            ),
            kind: ParameterKind::Text,
            required: true,
        }],
    },
    ToolSpec {
        name: "read",
        description: "Read the desk's recent messages. You are handed a bounded window at the \
                      top of your turn; call this when you need more of it than you were given.",
        parameters: &[ToolParameter {
            name: "limit",
            description: Some("How many recent messages to return. Default 20, max 100."),
            kind: ParameterKind::Count {
                default: READ_DEFAULT as u64,
                min: 1,
                max: READ_MAX as u64,
            },
            required: false,
        }],
    },
];

/// How many messages `read` returns when the seat does not say.
pub const READ_DEFAULT: usize = 20;

/// The most messages one `read` returns, however many it asked for.
///
/// The clamp is here rather than in each host so that two hosts cannot
/// disagree about it, and so that a seat that asks for the whole transcript
/// gets a bounded answer rather than its own context window back.
pub const READ_MAX: usize = 100;
