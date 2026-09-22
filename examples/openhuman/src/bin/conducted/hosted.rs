//! The host the hosted runner asks for, as this example is one.
//!
//! A real host -- OpenCompany -- answers these three with its journal, the
//! agents it already builds, and the task-locals its tools read. This one
//! has no agents of its own, so a seat is a library session carrying only
//! the episode's tools, and the wrapper is the core context such a session
//! runs under. The log is the same journal the episode loop appends to.

use std::collections::BTreeMap;
use std::sync::Arc;

use openhuman_core::agent::OpenHumanSessionHost;
use tinyhivemind::SessionLog;
use tinyhivemind_openhuman::offline::MemoryLog;
use tinyhivemind_openhuman::{EpisodeBelt, EpisodeHost, HostedTurn, LibraryHost};

/// This example's desk, as a host.
pub struct DeskHost {
    log: Arc<MemoryLog>,
    library: LibraryHost,
    /// Each seat's standing prompt: its brief and the contract.
    prompts: BTreeMap<String, String>,
}

impl DeskHost {
    pub fn new(
        log: Arc<MemoryLog>,
        library: LibraryHost,
        prompts: BTreeMap<String, String>,
    ) -> Self {
        Self {
            log,
            library,
            prompts,
        }
    }
}

impl EpisodeHost for DeskHost {
    fn log(&self) -> &dyn SessionLog {
        &*self.log
    }

    fn build_seat(
        &self,
        seat: &str,
        belt: EpisodeBelt,
    ) -> tinyhivemind_openhuman::Result<OpenHumanSessionHost> {
        // No tools of its own, so no gate of its own: the episode's tools
        // are admitted and everything else is denied.
        let gate = belt.admit(None);
        self.library
            .session(seat, &self.prompts[seat], belt.tools, gate)
    }

    fn wrap_turn<'a>(&'a self, _seat: &'a str, turn: HostedTurn<'a>) -> HostedTurn<'a> {
        Box::pin(self.library.scope(turn))
    }
}
