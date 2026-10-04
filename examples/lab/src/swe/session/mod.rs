//! Seat sessions that outlive an activation.
//!
//! In the hive a seat is woken many times. With [`SessionMode::Persistent`]
//! its conversation (every command it ran and every result it saw) is kept
//! here between activations, so a woken seat continues where it stopped
//! instead of re-running what it already learned. The only thing that
//! removes messages from a session is the context policy's compaction.
//!
//! The store is owned by the run and borrowed through the seat `Env`. An
//! activation [`Sessions::take`]s its seat's session out, works on it without
//! holding the lock, and [`Sessions::put`]s it back. Seats of one round run on
//! concurrent threads, but a round never holds the same seat twice (the hive
//! queue dedupes by seat), so no session is ever touched by two activations.

mod types;

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

pub use types::{SeatSession, SessionMode};

/// Every seat's session for one run.
#[derive(Debug)]
pub struct Sessions {
    mode: SessionMode,
    inner: Mutex<HashMap<String, SeatSession>>,
}

impl Sessions {
    /// An empty store.
    #[must_use]
    pub fn new(mode: SessionMode) -> Self {
        Self {
            mode,
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Whether sessions persist.
    #[must_use]
    pub const fn mode(&self) -> SessionMode {
        self.mode
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, SeatSession>> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Take `seat`'s session out for one activation: its kept session when
    /// persistent, a new one otherwise (or the first time).
    #[must_use]
    pub fn take(&self, seat: &str) -> SeatSession {
        match self.mode {
            SessionMode::Fresh => SeatSession::default(),
            SessionMode::Persistent => self.lock().remove(seat).unwrap_or_default(),
        }
    }

    /// Return `seat`'s session after its activation; a fresh-mode session is
    /// dropped here, which is exactly what made it fresh.
    pub fn put(&self, seat: &str, session: SeatSession) {
        if self.mode == SessionMode::Persistent {
            self.lock().insert(seat.to_owned(), session);
        }
    }

    /// Messages currently kept for `seat`.
    #[must_use]
    pub fn len_of(&self, seat: &str) -> usize {
        self.lock()
            .get(seat)
            .map_or(0, |session| session.messages.len())
    }
}

#[cfg(test)]
mod test;
