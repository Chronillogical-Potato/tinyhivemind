//! What the server remembers between calls, and hands the host.
//!
//! Four things, all per seat: the turn the host has registered (which chat and
//! thread it is in), the calls the seat has made during it, the refusals it was
//! given, and the window of recent rows the host last refreshed for `read`.
//! The host writes the first and last and drains the middle two; the server
//! writes the middle two and reads the others. Nothing here reaches back into
//! the host.
//!
//! A poisoned lock is recovered rather than propagated: what it guards is a
//! map a panicking writer can only have left one entry short, and losing one
//! seat's call is strictly better than losing every seat's server.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, PoisonError};

use serde_json::Value;
use tinyhivemind::speech::{ToolCall, Utterance, UtteranceRejection, interpret};

use crate::render::{named_definitions, parse_arguments, served_specs, serves};
use tinyhivemind::speech::ToolSpec;

/// The thread a registered turn is in, as the host told the seat.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dispatch {
    /// The chat -- desk or channel -- the turn is in.
    pub chat: String,
    /// The thread root, or `None` for the chat's own thread.
    pub parent: Option<String>,
}

/// One accepted call, as the host drains it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SeatEvent {
    /// The seat that called, from the endpoint it dialled.
    pub seat: String,
    /// What it asked the room for, already read by `interpret`.
    pub call: ToolCall,
    /// The thread it named, checked against the registered turn.
    pub dispatch: Dispatch,
}

/// One refused call, as the host drains it.
///
/// The seat read the reason inside its turn; this is the host's copy, so a
/// turn that recorded nothing can be told apart from a turn that was refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Refusal {
    /// The seat that called.
    pub seat: String,
    /// The tool it named.
    pub tool: String,
    /// The sentence it was given.
    pub reason: String,
}

/// The server's memory. Shared with the host through an `Arc`.
#[derive(Debug)]
pub struct EpisodeTools {
    seats: BTreeSet<String>,
    /// Tools this host does not serve, by name.
    withheld: BTreeSet<String>,
    open: Mutex<BTreeMap<String, Dispatch>>,
    inbox: Mutex<BTreeMap<String, Vec<SeatEvent>>>,
    refused: Mutex<BTreeMap<String, Vec<Refusal>>>,
    windows: Mutex<BTreeMap<String, Vec<String>>>,
    awaiting: Mutex<BTreeMap<String, Vec<String>>>,
    names: Mutex<BTreeMap<String, String>>,
}

impl EpisodeTools {
    /// The seats this server serves.
    ///
    /// Only a listed seat can call, and `ask` may name only a listed seat --
    /// which is also what the rendered schema offers as its choices.
    #[must_use]
    pub fn new<I, S>(seats: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            seats: seats.into_iter().map(Into::into).collect(),
            withheld: BTreeSet::new(),
            open: Mutex::new(BTreeMap::new()),
            inbox: Mutex::new(BTreeMap::new()),
            refused: Mutex::new(BTreeMap::new()),
            windows: Mutex::new(BTreeMap::new()),
            awaiting: Mutex::new(BTreeMap::new()),
            names: Mutex::new(BTreeMap::new()),
        }
    }

    /// Give the served seats the names a person calls them by. A seat this
    /// server does not serve is ignored, and a seat left unnamed keeps its
    /// id.
    pub fn name_seats<I, K, V>(&self, names: I)
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let mut held = self.names.lock().unwrap_or_else(PoisonError::into_inner);
        for (seat, name) in names {
            let seat = seat.into();
            let name = name.into();
            if self.seats.contains(&seat) && !name.trim().is_empty() {
                held.insert(seat, name);
            }
        }
    }

    /// What a person calls `seat`: its name, or its id when it has none.
    #[must_use]
    pub fn display_name(&self, seat: &str) -> String {
        self.names
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(seat)
            .cloned()
            .unwrap_or_else(|| seat.to_owned())
    }

    /// Do not serve these tools, by name.
    ///
    /// The vocabulary is otherwise the same for every host, which is what
    /// makes a seat's contract portable. A host withholds one where its own
    /// model of the episode cannot represent what that tool produces --
    /// `ask_teammates` opens a conversation with several seats in it, and a
    /// host that stores a conversation as a pair of seats cannot file one.
    /// Better refused at the door than filed as something it is not.
    ///
    /// A withheld tool is not rendered, not described in a contract built
    /// from [`specs`](Self::specs), and refused as an unknown tool if called
    /// anyway.
    #[must_use]
    pub fn withhold<I, S>(mut self, tools: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.withheld.extend(tools.into_iter().map(Into::into));
        self
    }

    /// The specs this host serves: the vocabulary minus what it withheld.
    /// A host renders its seats' standing contract from these.
    pub fn specs(&self) -> impl Iterator<Item = &'static ToolSpec> + '_ {
        served_specs().filter(|spec| !self.withheld.contains(spec.name))
    }

    /// The served tools as MCP tool definitions, with the asking tools'
    /// recipients described by name beside the ids a call must carry.
    #[must_use]
    pub fn tool_definitions(&self) -> Vec<Value> {
        let seats: Vec<(String, String)> = self
            .seats
            .iter()
            .map(|seat| (seat.clone(), self.display_name(seat)))
            .collect();
        named_definitions(&seats)
            .into_iter()
            .filter(|tool| {
                tool["name"]
                    .as_str()
                    .is_none_or(|name| !self.withheld.contains(name))
            })
            .collect()
    }

    /// The display names of `seats`, as a person would read them out:
    /// `a`, `a and b`, `a, b and c`. What an acknowledgement calls the
    /// seats a group ask just reached.
    fn roll_call(&self, seats: &[String]) -> String {
        let named: Vec<String> = seats.iter().map(|seat| self.display_name(seat)).collect();
        match named.split_last() {
            None => String::new(),
            Some((last, [])) => last.clone(),
            Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        }
    }

    /// `Name (id)` for a named seat, the bare id otherwise: how a reply
    /// lists the seats a call may name.
    fn with_id(&self, seat: &str) -> String {
        let name = self.display_name(seat);
        if name == seat {
            seat.to_owned()
        } else {
            format!("{name} (id `{seat}`)")
        }
    }

    /// Every seat this server serves, in id order.
    #[must_use]
    pub fn seats(&self) -> Vec<String> {
        self.seats.iter().cloned().collect()
    }

    pub(crate) fn knows(&self, seat: &str) -> bool {
        self.seats.contains(seat)
    }

    /// Record that the host is about to run `seat` for a turn in `dispatch`.
    ///
    /// Until [`clear`](Self::clear), calls from that seat must name this chat
    /// and thread, and calls from a seat with no registered turn are refused.
    pub fn register(&self, seat: &str, dispatch: Dispatch) {
        self.open
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(seat.to_owned(), dispatch);
    }

    /// Record that `seat`'s turn has ended.
    pub fn clear(&self, seat: &str) {
        self.open
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(seat);
    }

    pub(crate) fn open_turn(&self, seat: &str) -> Option<Dispatch> {
        self.open
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(seat)
            .cloned()
    }

    /// Replace the rows `read` may return to `seat`, newest last.
    ///
    /// A snapshot the host refreshes before a turn, rather than a callback the
    /// server would make into the host's journal.
    pub fn window(&self, seat: &str, rows: Vec<String>) {
        self.windows
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(seat.to_owned(), rows);
    }

    /// Replace the seats `seat` is waiting on an answer from.
    ///
    /// A snapshot the host refreshes before a turn, like
    /// [`window`](Self::window), and for the same reason: the ledger that
    /// knows this lives in the driver, and the server cannot call into it.
    ///
    /// What it buys is a refusal rather than a duplicate conversation --
    /// see the `ask` arm of [`call`](Self::call).
    pub fn awaiting(&self, seat: &str, seats: Vec<String>) {
        self.awaiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(seat.to_owned(), seats);
    }

    /// Whether `seat` has already asked `other` and is still waiting.
    fn awaits(&self, seat: &str, other: &str) -> bool {
        self.awaiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(seat)
            .is_some_and(|seats| seats.iter().any(|one| one == other))
    }

    pub(crate) fn recent(&self, seat: &str, limit: usize) -> Vec<String> {
        let windows = self.windows.lock().unwrap_or_else(PoisonError::into_inner);
        let rows = windows.get(seat).map(Vec::as_slice).unwrap_or_default();
        rows[rows.len().saturating_sub(limit)..].to_vec()
    }

    /// Take everything `seat` called during its turn, oldest first.
    #[must_use]
    pub fn drain(&self, seat: &str) -> Vec<SeatEvent> {
        self.inbox
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(seat)
            .unwrap_or_default()
    }

    /// Take every refusal `seat` was given during its turn, oldest first.
    #[must_use]
    pub fn drain_refusals(&self, seat: &str) -> Vec<Refusal> {
        self.refused
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(seat)
            .unwrap_or_default()
    }

    pub(crate) fn refuse(&self, seat: &str, tool: &str, reason: &str) {
        self.refused
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(seat.to_owned())
            .or_default()
            .push(Refusal {
                seat: seat.to_owned(),
                tool: tool.to_owned(),
                reason: reason.to_owned(),
            });
    }

    /// One call from `seat`, however it arrived: check the caller, check the
    /// turn, read the call, record it.
    ///
    /// This is the whole of what `tools/call` does; the server is HTTP and
    /// JSON-RPC framing around it. A host whose harness takes native tools
    /// calls this directly, so an in-process seat and an MCP seat are refused
    /// and acknowledged in the same words.
    ///
    /// `arguments` is the call's argument object, carrying `chat` and
    /// `parent` beside the tool's own parameters. `Ok` is the text the seat
    /// reads back: an acknowledgement, or for `read` the rows. `Err` is a
    /// refusal, in the sentence [`UtteranceRejection`] already wrote where
    /// one exists; nothing is recorded on a refusal.
    ///
    /// # Errors
    ///
    /// The refusal text, for the seat to read inside its own turn.
    pub fn call(
        &self,
        seat: &str,
        name: &str,
        arguments: &Value,
    ) -> std::result::Result<String, String> {
        self.admit(seat, name, arguments).inspect_err(|reason| {
            // The seat reads the refusal in its tool result; the host drains a
            // copy, so a turn that recorded nothing is not mistaken for a turn
            // that was refused.
            self.refuse(seat, name, reason);
        })
    }

    /// The decision behind [`Self::call`], with the refusal not yet copied.
    fn admit(
        &self,
        seat: &str,
        name: &str,
        arguments: &Value,
    ) -> std::result::Result<String, String> {
        let args = parse_arguments(arguments);
        if !self.knows(seat) {
            return Err(format!("no seat named `{seat}` is served here"));
        }
        let Some(dispatch) = self.open_turn(seat) else {
            return Err("no turn is open for you, so nothing you call now can be recorded".into());
        };
        if args.chat.as_deref() != Some(dispatch.chat.as_str()) || args.parent != dispatch.parent {
            let parent = dispatch
                .parent
                .as_deref()
                .map_or_else(|| "null".to_owned(), |parent| format!("\"{parent}\""));
            return Err(format!(
                "every call in this turn carries \"chat\": \"{}\" and \"parent\": {parent}, \
                 exactly as your brief gave them",
                dispatch.chat
            ));
        }
        // Whether the tool exists here comes first. A host that withheld
        // `ask_teammates` has a seat whose contract never mentioned it, and
        // telling that seat to answer with `complete_episode` instead would
        // describe a tool it was never offered as one it called in the wrong
        // place.
        if !serves(name) || self.withheld.contains(name) {
            return Err(unknown_tool(name));
        }
        if dispatch.parent.is_some() && (name == "ask" || name == "ask_teammates") {
            return Err(
                "inside a conversation you answer the seat that asked you: call \
                 `complete_episode`, and its message is your answer. If you need another seat \
                 first, say so in that answer, and the seat that asked you will ask them."
                    .into(),
            );
        }
        let call = interpret(name, &args.call()).map_err(|rejection| rejection.to_string())?;
        let acknowledgement = match &call {
            ToolCall::Read { limit } => return Ok(self.recent(seat, *limit).join("\n")),
            ToolCall::Speak(Utterance::Ask { to, .. }) => {
                if to.iter().any(|id| id == seat) {
                    return Err(UtteranceRejection::SelfRecipient.to_string());
                }
                if let Some(stranger) = to.iter().find(|id| !self.knows(id)) {
                    let seats: Vec<String> =
                        self.seats.iter().map(|seat| self.with_id(seat)).collect();
                    return Err(format!(
                        "{}. You can ask: {}",
                        UtteranceRejection::UnknownRecipient {
                            id: stranger.clone()
                        },
                        seats.join(", ")
                    ));
                }
                // **One conversation per pair at a time.**
                //
                // A seat waiting on an answer is turned on the desk anyway --
                // that turn is what keeps the episode live while the seat it
                // asked is parked -- and the brief it gets is its own
                // unanswered question, `complete_episode` refused until the
                // conversation concludes, and "a reply that calls no tool
                // records nothing". Every door shut but this one, so it asks
                // again, and each repeat opens a *second* conversation with
                // the same seat that must also be concluded. Observed on a
                // hosted desk: one question re-issued verbatim, three
                // conversations, three conclusions.
                //
                // Refusing is the same rule the thread turn already applies
                // one line up, at the other end of the same wait. A group ask
                // is refused whole where it names one seat already waited on:
                // admitting the rest would put the question to a conversation
                // the asker did not describe, and the seat it dropped is the
                // one it most wanted there.
                if let Some(held) = to.iter().find(|id| self.awaits(seat, id)) {
                    return Err(format!(
                        "you already asked {} and are still waiting: their answer reaches you \
                         on a later turn, and asking again opens a second conversation with \
                         them rather than hurrying the first. Ask whoever else you need \
                         without them, or end your turn.",
                        self.display_name(held)
                    ));
                }
                let asked = self.roll_call(to);
                if to.len() > 1 {
                    format!(
                        "your question to {asked} is sent: they are in one conversation \
                         together and can read each other's answers. It reaches you on a later \
                         turn, once every one of them has answered; you cannot finish until it \
                         does, so end your turn when you have asked everything."
                    )
                } else {
                    format!(
                        "your question to {asked} is sent. Their answer reaches you on a later \
                         turn; you cannot finish until it does, so end your turn when you have \
                         asked everything."
                    )
                }
            }
            ToolCall::Speak(Utterance::Post { .. }) => "posted to the desk".to_owned(),
            ToolCall::Speak(Utterance::Broadcast { .. }) => {
                "recorded: routing will place that with a seat".to_owned()
            }
            ToolCall::Speak(Utterance::CompleteEpisode { .. }) => {
                "recorded: your assignment is complete".to_owned()
            }
            ToolCall::Speak(Utterance::Dm { .. }) => return Err(unknown_tool(name)),
        };
        self.record(SeatEvent {
            seat: seat.to_owned(),
            call,
            dispatch,
        });
        Ok(acknowledgement)
    }

    pub(crate) fn record(&self, event: SeatEvent) {
        self.inbox
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(event.seat.clone())
            .or_default()
            .push(event);
    }
}

fn unknown_tool(name: &str) -> String {
    UtteranceRejection::UnknownTool {
        name: name.to_owned(),
    }
    .to_string()
}

#[cfg(test)]
mod test;
