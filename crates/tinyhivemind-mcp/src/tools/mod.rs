//! What the server remembers between calls, and hands the host.
//!
//! Three things, all per seat: the turn the host has registered (which chat and
//! thread it is in), the calls the seat has made during it, and the window of
//! recent rows the host last refreshed for `read`. The host writes the first
//! and third and drains the second; the server writes the second and reads the
//! other two. Nothing here reaches back into the host.
//!
//! A poisoned lock is recovered rather than propagated: what it guards is a
//! map a panicking writer can only have left one entry short, and losing one
//! seat's call is strictly better than losing every seat's server.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, PoisonError};

use tinyhivemind::speech::ToolCall;

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

/// The server's memory. Shared with the host through an `Arc`.
#[derive(Debug)]
pub struct EpisodeTools {
    seats: BTreeSet<String>,
    open: Mutex<BTreeMap<String, Dispatch>>,
    inbox: Mutex<BTreeMap<String, Vec<SeatEvent>>>,
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

    pub(crate) fn record(&self, event: SeatEvent) {
        self.inbox
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(event.seat.clone())
            .or_default()
            .push(event);
    }
}

#[cfg(test)]
mod test;
