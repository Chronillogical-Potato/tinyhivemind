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

use crate::render::{parse_arguments, serves};

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
    open: Mutex<BTreeMap<String, Dispatch>>,
    inbox: Mutex<BTreeMap<String, Vec<SeatEvent>>>,
    refused: Mutex<BTreeMap<String, Vec<Refusal>>>,
    windows: Mutex<BTreeMap<String, Vec<String>>>,
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
            open: Mutex::new(BTreeMap::new()),
            inbox: Mutex::new(BTreeMap::new()),
            refused: Mutex::new(BTreeMap::new()),
            windows: Mutex::new(BTreeMap::new()),
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
                .map_or_else(|| "null".to_owned(), |parent| format!("`{parent}`"));
            return Err(format!(
                "this turn is in chat `{}` with parent {parent}; name exactly those",
                dispatch.chat
            ));
        }
        if dispatch.parent.is_some() && name == "ask" {
            return Err(
                "inside a conversation you answer the seat that asked you: call `complete_episode`,                  and its message is your answer. If you need another seat first, say so in that                  answer, and the seat that asked you will ask them."
                    .into(),
            );
        }
        if !serves(name) {
            return Err(unknown_tool(name));
        }
        let call = interpret(name, &args.call()).map_err(|rejection| rejection.to_string())?;
        let acknowledgement = match &call {
            ToolCall::Read { limit } => return Ok(self.recent(seat, *limit).join("\n")),
            ToolCall::Speak(Utterance::Ask { to, .. }) => {
                if to == seat {
                    return Err(UtteranceRejection::SelfRecipient.to_string());
                }
                if !self.knows(to) {
                    return Err(format!(
                        "{}. The desk is: {}",
                        UtteranceRejection::UnknownRecipient { id: to.clone() },
                        self.seats().join(", ")
                    ));
                }
                format!(
                    "asked @{to}. The answer reaches you on a later turn; you cannot finish \
                     until it does, so end your turn when you have asked everything."
                )
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
