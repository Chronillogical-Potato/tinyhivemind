//! Stopping a run when its recording can no longer be written.
//!
//! An observer cannot return early, so it raises a [`Halt`]; the run's speaker
//! then reports no turns left and the run ends exhausted at the next round
//! boundary, before any further (paid) model call.

use std::cell::Cell;
use std::rc::Rc;

use tinyhivemind_core::hive::{EpisodePolicy, HiveTurn, SessionMessage};

use crate::Result;
use crate::episode::Speaker;
use crate::run::Prepared;

/// A flag a run's observer raises to stop the run at its next round.
#[derive(Clone, Debug, Default)]
pub(crate) struct Halt(Rc<Cell<bool>>);

impl Halt {
    /// Ask the run to stop before it pays for another round.
    pub(crate) fn stop(&self) {
        self.0.set(true);
    }
}

/// A speaker that reports no turns left once its [`Halt`] is raised.
pub(crate) struct Halting {
    inner: Box<dyn Speaker>,
    halt: Halt,
}

/// Put `halt` in charge of `prepared`'s speaker.
pub(crate) fn install(prepared: &mut Prepared, halt: Halt) {
    let inner = std::mem::replace(&mut prepared.speaker, Box::new(Silent));
    prepared.speaker = Box::new(Halting { inner, halt });
}

/// The placeholder in a [`Prepared`] for the instant its speaker is wrapped.
struct Silent;

impl Speaker for Silent {
    fn speak(&mut self, _: &HiveTurn, _: &[SessionMessage], _: &EpisodePolicy) -> String {
        crate::script::QUIET.to_owned()
    }
}

impl Speaker for Halting {
    fn speak(
        &mut self,
        turn: &HiveTurn,
        visible: &[SessionMessage],
        policy: &EpisodePolicy,
    ) -> String {
        self.inner.speak(turn, visible, policy)
    }

    fn speak_round(
        &mut self,
        round: &[(HiveTurn, Vec<SessionMessage>)],
        policy: &EpisodePolicy,
    ) -> Result<Vec<String>> {
        self.inner.speak_round(round, policy)
    }

    fn answer(&mut self, seat: &str, incoming: &str, visible: &[SessionMessage]) -> Result<String> {
        self.inner.answer(seat, incoming, visible)
    }

    fn remaining(&self) -> Option<u32> {
        if self.halt.0.get() {
            Some(0)
        } else {
            self.inner.remaining()
        }
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        self.inner.as_any()
    }
}
