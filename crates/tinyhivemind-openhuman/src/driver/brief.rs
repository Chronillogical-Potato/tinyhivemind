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

use tinyhivemind::Sequence;
use tinyhivemind::speech::ToolSpec;

use super::DriverState;
use super::ledger::open_assignment;

/// Where a turn runs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Channel {
    /// The open desk.
    Desk,
    /// A conversation rooted at an ask row, between this seat and `other`.
    Thread {
        /// The ask row the conversation is rooted at.
        root: Sequence,
        /// The other seat in it.
        other: String,
        /// Whether this seat opened it.
        opened_it: bool,
    },
}

/// One conversation the seat is or was in, as the host holds it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationView {
    /// The ask row it is rooted at.
    pub root: Sequence,
    /// The other seat in it.
    pub other: String,
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
        }
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
                other,
                opened_it,
            } => self.render_thread(*root, other, *opened_it),
        }
    }

    fn render_desk(&self) -> String {
        let mut out = format!("## New desk messages\n{}", rows_or_nothing(&self.new_rows));
        let concluded: Vec<String> = self
            .conversations
            .iter()
            .filter(|view| view.concluded)
            .map(render_conversation)
            .collect();
        if !concluded.is_empty() {
            out.push_str("\n\n## Conversations you had since you last spoke\n");
            out.push_str(&concluded.join("\n\n"));
        }
        let open: Vec<String> = self
            .conversations
            .iter()
            .filter(|view| !view.concluded)
            .map(render_conversation)
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
                    format_args!("Your assignment was made at sequence {}.", at.0),
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
                    " You cannot complete until your conversation with @{} concludes.",
                    self.awaiting.join(", @")
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

    fn render_thread(&self, root: Sequence, other: &str, opened_it: bool) -> String {
        let role = if opened_it {
            "You opened this conversation; their answer reaches you on the desk. There is \
             nothing for you to do here."
        } else {
            "A peer asked you this. Answer with `complete_episode`: its message is your answer \
             and reaches them. If you need something from them first, `ask` them back."
        };
        format!(
            "## A private conversation with @{other} (thread {})\n{}\n\n{role} Only the two of \
             you read this thread.\n\nEvery tool call must carry \"chat\": \"{}\" and \
             \"parent\": \"{}\". An `ask` or a `broadcast` made here goes to the desk: it opens a \
             new conversation, or hands work off, exactly as it would there.",
            root.0,
            rows_or_nothing(&self.new_rows),
            self.chat,
            root.0
        )
    }
}

fn rows_or_nothing(rows: &[String]) -> String {
    if rows.is_empty() {
        "(nothing new)".to_owned()
    } else {
        rows.join("\n")
    }
}

fn render_conversation(view: &ConversationView) -> String {
    format!(
        "### With @{} (thread {}){}\n{}",
        view.other,
        view.root.0,
        if view.concluded {
            ""
        } else {
            " -- in progress"
        },
        rows_or_nothing(&view.transcript)
    )
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
    how_to_call: &str,
) -> String {
    let mut out = format!(
        "Your work is recorded by calling a tool. Prose alone changes nothing: if you end a turn \
         without calling one, nothing you said is recorded and the desk does not move.\n\n\
         {how_to_call} Every call carries \"chat\": \"{chat}\" and the \"parent\" you are told, \
         beside its own arguments:"
    );
    for spec in specs {
        let arguments: Vec<String> = spec
            .parameters
            .iter()
            .map(|parameter| format!("\"{}\": ...", parameter.name))
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
         what the desk reads.",
    );
    out
}

#[cfg(test)]
mod test;
