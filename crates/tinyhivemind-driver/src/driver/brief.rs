//! What the episode itself has to tell a seat before its turn.
//!
//! A host owns everything about *who* a seat is -- its profile, its memory,
//! its company -- and puts that in front of the seat however it likes. What
//! the host cannot derive without re-reading the driver is what the episode
//! knows: the seat's open assignment, what is new for it, the conversations
//! it is in, what it is waiting on, what waits for it, and which channel
//! every tool call must name. [`EpisodeBrief`] is that, as a value, with a
//! default [`render`](EpisodeBrief::render) a host may use or replace.
//!
//! Nothing here reads storage. The host passes the rows a seat may see and
//! the conversations it was part of; the brief adds only what the state holds.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tinyhivemind::Sequence;
use tinyhivemind::speech::{ParameterKind, ToolSpec};

use super::DriverState;
use super::ledger::open_assignment;

/// Where a turn runs.
///
/// On the wire, tagged by `kind`: `{"kind":"desk"}` or
/// `{"kind":"thread","root":7,"other":"two","opened_it":false}`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Channel {
    /// The open desk.
    Desk,
    /// A conversation rooted at an ask row, between this seat and `others`.
    Thread {
        /// The ask row the conversation is rooted at.
        root: Sequence,
        /// The other seats in it: the seat that asked, or -- for that seat --
        /// everyone it asked. One ask can name a group, and then everyone in
        /// it reads everyone else.
        others: Vec<String>,
        /// Whether this seat opened it.
        opened_it: bool,
    },
}

/// One conversation the seat is or was in, as the host holds it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationView {
    /// The ask row it is rooted at.
    pub root: Sequence,
    /// The other seats in it, in the order the ask named them.
    pub others: Vec<String>,
    /// Whether this seat opened it.
    pub opened_it: bool,
    /// Every row in it so far, rendered by the host.
    pub transcript: Vec<String>,
    /// Whether it has concluded.
    pub concluded: bool,
}

/// What the episode tells a seat before one turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EpisodeBrief {
    /// The seat.
    pub seat: String,
    /// The chat the turn is in, as every tool call must name it.
    pub chat: String,
    /// Where the turn runs.
    pub channel: Channel,
    /// Where the seat's open assignment in this channel's episode was made.
    pub assignment: Option<Sequence>,
    /// Rows above the seat's watermark, rendered by the host.
    pub new_rows: Vec<String>,
    /// Conversations the host wants in front of the seat: concluded since it
    /// last spoke, or still in progress.
    pub conversations: Vec<ConversationView>,
    /// Seats whose conversation with this one must conclude before it may
    /// complete.
    pub awaiting: Vec<String>,
    /// Handoffs held for this seat, delivered when it completes.
    pub queued: usize,
    /// What the seat's other conversations hold, for a host that has them:
    /// the newest rows of each, read as the seat, through the wave's
    /// watermark. Context, not work: nothing in it is addressed here.
    pub elsewhere: Vec<ElsewhereView>,
    /// What a person calls the seats this brief mentions, by id. A seat
    /// missing here is written as `@id`.
    pub names: BTreeMap<String, String>,
}

/// The newest rows of one conversation the seat is in that is not this
/// turn's, rendered by the host.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ElsewhereView {
    /// The conversation's chat id.
    pub chat: String,
    /// Its display name.
    pub name: String,
    /// Its thread root, or `None` for that desk's open channel.
    pub thread_root: Option<Sequence>,
    /// Its newest rows, oldest first.
    pub rows: Vec<String>,
}

impl EpisodeBrief {
    /// Build the brief for one turn from the state of the episode the channel
    /// belongs to -- the desk's, or the conversation's.
    #[must_use]
    pub fn for_turn(
        state: &DriverState,
        chat: impl Into<String>,
        seat: impl Into<String>,
        channel: Channel,
        new_rows: Vec<String>,
        conversations: Vec<ConversationView>,
    ) -> Self {
        let seat = seat.into();
        let awaiting = state
            .ledger()
            .awaiting(&seat)
            .map(|asked| asked.keys().cloned().collect())
            .unwrap_or_default();
        Self {
            assignment: open_assignment(state.episode(), &seat),
            queued: state.ledger().queue_len(&seat),
            awaiting,
            seat,
            chat: chat.into(),
            channel,
            new_rows,
            conversations,
            elsewhere: Vec::new(),
            names: BTreeMap::new(),
        }
    }

    /// Name every seat the brief mentions -- the other side of a thread or
    /// a conversation, and the seats it waits on -- with `name`.
    pub fn name_seats(&mut self, name: impl Fn(&str) -> String) {
        let mut seats: Vec<&str> = self.awaiting.iter().map(String::as_str).collect();
        seats.extend(
            self.conversations
                .iter()
                .flat_map(|view| view.others.iter().map(String::as_str)),
        );
        if let Channel::Thread { others, .. } = &self.channel {
            seats.extend(others.iter().map(String::as_str));
        }
        let named: BTreeMap<String, String> = seats
            .into_iter()
            .map(|seat| (seat.to_owned(), name(seat)))
            .collect();
        self.names.extend(named);
    }

    /// How the brief writes a group of seats, as a person would read them
    /// out: `a`, `a and b`, `a, b and c`.
    #[must_use]
    fn roll_call(&self, seats: &[String]) -> String {
        let named: Vec<String> = seats.iter().map(|seat| self.speaker(seat)).collect();
        match named.split_last() {
            None => String::new(),
            Some((last, [])) => last.clone(),
            Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        }
    }

    /// How the brief writes `seat`: its name, or `@id` when it has none.
    #[must_use]
    pub fn speaker(&self, seat: &str) -> String {
        speaker(seat, self.names.get(seat).map_or(seat, String::as_str))
    }

    /// The `parent` every tool call in this turn must name: the thread root,
    /// or `None` on the desk.
    #[must_use]
    pub fn parent(&self) -> Option<String> {
        match &self.channel {
            Channel::Desk => None,
            Channel::Thread { root, .. } => Some(root.0.to_string()),
        }
    }

    /// The default wording. A host prepends what it owns.
    #[must_use]
    pub fn render(&self) -> String {
        match &self.channel {
            Channel::Desk => self.render_desk(),
            Channel::Thread {
                root,
                others,
                opened_it,
            } => self.render_thread(*root, others, *opened_it),
        }
    }

    fn render_desk(&self) -> String {
        let mut out = format!("## New desk messages\n{}", rows_or_nothing(&self.new_rows));
        out.push_str(&self.render_elsewhere());
        let concluded: Vec<String> = self
            .conversations
            .iter()
            .filter(|view| view.concluded)
            .map(|view| self.render_conversation(view))
            .collect();
        if !concluded.is_empty() {
            out.push_str("\n\n## Conversations you had since you last spoke\n");
            out.push_str(&concluded.join("\n\n"));
        }
        let open: Vec<String> = self
            .conversations
            .iter()
            .filter(|view| !view.concluded)
            .map(|view| self.render_conversation(view))
            .collect();
        if !open.is_empty() {
            out.push_str("\n\n## Conversations still in progress\n");
            out.push_str(&open.join("\n\n"));
        }
        out.push_str("\n\n");
        match self.assignment {
            Some(at) => {
                let _ = std::fmt::Write::write_fmt(
                    &mut out,
                    format_args!(
                        "Your assignment was made at sequence {}. Record your part with \
                         `complete_episode`: its message is your finding. Hand what is another \
                         seat's on with `broadcast`. A reply that calls no tool records nothing. \
                         {READER}",
                        at.0
                    ),
                );
            }
            None => out.push_str(
                "You hold no open assignment. If a peer asked you something, that conversation \
                 is its own thread and you will be turned to there.",
            ),
        }
        if !self.awaiting.is_empty() {
            let _ = std::fmt::Write::write_fmt(
                &mut out,
                format_args!(
                    " You cannot complete until your conversation with {} concludes.",
                    self.awaiting
                        .iter()
                        .map(|seat| self.speaker(seat))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
        }
        if self.queued > 0 {
            let _ = std::fmt::Write::write_fmt(
                &mut out,
                format_args!(
                    " {} handoff(s) wait for you and arrive when you complete.",
                    self.queued
                ),
            );
        }
        let _ = std::fmt::Write::write_fmt(
            &mut out,
            format_args!(
                "\n\nEvery tool call must carry \"chat\": \"{}\" and \"parent\": null.",
                self.chat
            ),
        );
        out
    }

    fn render_thread(&self, root: Sequence, others: &[String], opened_it: bool) -> String {
        let role = if opened_it {
            "You opened this conversation; their answer reaches you on the desk. There is \
             nothing for you to do here."
        } else if others.len() > 1 {
            "A peer asked all of you this. Answer with `complete_episode`: its message is your \
             answer and reaches them. The others were asked the same question and you read \
             their answers here, so answer your part of it and say where you differ rather \
             than repeating what they have already settled. If you need another seat first, \
             say so in that answer, and the seat that asked you will ask them."
        } else {
            "A peer asked you this. Answer with `complete_episode`: its message is your answer \
             and reaches them. If you need another seat first, say so in that answer, and the \
             seat that asked you will ask them."
        };
        let reader = if opened_it {
            String::new()
        } else {
            format!(" {READER}")
        };
        let readers = if others.len() > 1 {
            "Only the seats in it read this thread."
        } else {
            "Only the two of you read this thread."
        };
        format!(
            "## A private conversation with {} (thread {})\n{}{}\n\n{role} {readers}\n\nEvery \
             tool call must carry \"chat\": \"{}\" and \
             \"parent\": \"{}\". `ask` is not available inside a conversation. A `broadcast` made here \
             hands work off on the desk, exactly as it would there.{reader}",
            self.roll_call(others),
            root.0,
            rows_or_nothing(&self.new_rows),
            self.render_elsewhere(),
            self.chat,
            root.0
        )
    }
}

impl EpisodeBrief {
    /// The seat's other conversations as a section, or nothing when the
    /// host gave none.
    fn render_elsewhere(&self) -> String {
        if self.elsewhere.is_empty() {
            return String::new();
        }
        let mut out = String::from(
            "\n\n## Elsewhere, for context\nWhat your other conversations hold. Nothing here is \
             addressed to you on this desk.",
        );
        for view in &self.elsewhere {
            let thread = view
                .thread_root
                .map(|root| format!(", thread {}", root.0))
                .unwrap_or_default();
            let _ = std::fmt::Write::write_fmt(
                &mut out,
                format_args!(
                    "\n\n### {} ({}{thread})\n{}",
                    view.name,
                    view.chat,
                    rows_or_nothing(&view.rows)
                ),
            );
        }
        out
    }
}

/// How a recorded message should read, since a person reads it too.
const READER: &str = "A person reads what you record: lead with the result in plain words, keep \
                      it short, and refer to teammates by name.";

fn rows_or_nothing(rows: &[String]) -> String {
    if rows.is_empty() {
        "(nothing new)".to_owned()
    } else {
        rows.join("\n")
    }
}

impl EpisodeBrief {
    fn render_conversation(&self, view: &ConversationView) -> String {
        format!(
            "### With {} (thread {}){}\n{}",
            self.roll_call(&view.others),
            view.root.0,
            if view.concluded {
                ""
            } else {
                " -- in progress"
            },
            rows_or_nothing(&view.transcript)
        )
    }
}

/// Seat ids as a person reads them out: `@a`, `@a and @b`, `@a, @b and @c`.
fn roll_call(seats: &[String]) -> String {
    let named: Vec<String> = seats.iter().map(|seat| format!("@{seat}")).collect();
    match named.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

/// How a row or a heading writes a seat: `name` when the host gave one
/// that differs from the id, otherwise `@id`.
#[must_use]
pub fn speaker(id: &str, name: &str) -> String {
    let name = name.trim();
    if name.is_empty() || name == id {
        format!("@{id}")
    } else {
        name.to_owned()
    }
}

/// The standing contract: how a turn is recorded, and what each served tool
/// is for, in the vocabulary's own words.
///
/// `how_to_call` is the host's one sentence on the mechanics -- for an MCP
/// host, which server and dispatcher to use -- because that is the one part
/// the episode does not know. Everything else is `tool_specs()`.
#[must_use]
pub fn standing_contract<'a>(
    specs: impl IntoIterator<Item = &'a ToolSpec>,
    chat: &str,
    seats: &[String],
    how_to_call: &str,
) -> String {
    let mut out = String::from(
        "Your work is recorded by calling a tool. Prose alone changes nothing: if you end a \
         turn without calling one, nothing you said is recorded and the desk does not move.",
    );
    // Who is here, in the one place every seat reads every turn.
    //
    // The roster used to live only in the rendered schema of the asking
    // tools -- their `to` enumeration and its description. A seat whose tools
    // arrive over MCP never sees that unless it lists them first, and two
    // live runs show what a seat does instead: one invented `@backend`,
    // `@frontend` and `@qa` on a five-seat desk, another addressed a
    // collective it made up (`teammates`). Both recovered from the refusal,
    // four wasted model calls later. A seat cannot address a room it has not
    // been shown.
    if !seats.is_empty() {
        let _ = std::fmt::Write::write_fmt(
            &mut out,
            format_args!(
                "\n\nThe seats at this desk are {}. That is all of them, and their ids are the \
                 only ones a tool argument may name: there is no id for the desk as a whole, \
                 and a seat you invent is refused.",
                roll_call(seats)
            ),
        );
    }
    let _ = std::fmt::Write::write_fmt(
        &mut out,
        format_args!(
            "\n\n{how_to_call} Every call carries \"chat\": \"{chat}\" and the \"parent\" you are \
             told, beside its own arguments:"
        ),
    );
    for spec in specs {
        // The shape, not just the name. A seat that reads `"to": ...` has to
        // guess whether one id or a list goes there, and a seat told it may
        // ask several guesses a collective noun: a live desk-of-one wrote
        // `"to": "teammates"`, was refused, and then asked its four teammates
        // one at a time. What the wire wants is cheap to show.
        let arguments: Vec<String> = spec
            .parameters
            .iter()
            .map(|parameter| {
                let shape = match parameter.kind {
                    ParameterKind::Text => "\"...\"".to_owned(),
                    ParameterKind::TextList => "[\"...\"]".to_owned(),
                    ParameterKind::Count { default, .. } => default.to_string(),
                };
                format!("\"{}\": {shape}", parameter.name)
            })
            .collect();
        let _ = std::fmt::Write::write_fmt(
            &mut out,
            format_args!(
                "\n\n  tool \"{}\", arguments {{{}}}\n      -- {}",
                spec.name,
                arguments.join(", "),
                spec.description
            ),
        );
    }
    out.push_str(
        "\n\nIf you are waiting on a conversation and nothing new bears on your work, end \
         your turn without calling any tool. Keep your reply brief -- the tool message is \
         what a person and the desk read.",
    );
    out
}

#[cfg(test)]
mod test;
