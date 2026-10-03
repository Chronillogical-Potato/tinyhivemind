//! One seat, run as a fresh raw session on every turn.

use std::fmt;
use std::sync::Arc;

use openhuman_core::agent::TurnOverrides;
use tinyhivemind_driver::BoundAgent;
use tinytools::Tool;

use super::library::LibraryHost;
use super::policy::EpisodeGate;
use crate::runner::TURN_TIMEOUT;
use crate::{Error, Result};

/// Everything needed to build a seat's session, and no session.
///
/// Cloned per turn into the job that runs it, and bound into the hive as the
/// seat's own handle: the driver hands it back with a pending round and never
/// runs it, which is why nothing here is an agent.
#[derive(Clone)]
pub struct RawSeat {
    id: String,
    /// The standing prompt: the brief and the contract, whole.
    system_prompt: String,
    /// The core every session is built on.
    library: LibraryHost,
}

impl BoundAgent for RawSeat {
    fn runtime_id(&self) -> &str {
        &self.id
    }
}

impl fmt::Debug for RawSeat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawSeat")
            .field("id", &self.id)
            .field("library", &self.library)
            .finish_non_exhaustive()
    }
}

impl RawSeat {
    /// A seat from its parts. [`RawRunner::seat`](super::RawRunner::seat)
    /// builds these; it is public for a host that boots its own library.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        system_prompt: impl Into<String>,
        library: LibraryHost,
    ) -> Self {
        Self {
            id: id.into(),
            system_prompt: system_prompt.into(),
            library,
        }
    }

    /// Run one turn on a fresh session seeded with `history`, chronological
    /// `(role, content)` pairs, and drop it. The belt is gated to itself.
    ///
    /// # Errors
    ///
    /// The session failing to build or seed, the turn failing, or the turn
    /// running past its wall.
    pub async fn turn(
        &self,
        history: Vec<(String, String)>,
        message: &str,
        tools: Vec<Box<dyn Tool>>,
    ) -> Result<String> {
        let names: Vec<String> = tools.iter().map(|tool| tool.name().to_owned()).collect();
        self.library
            .scope(async {
                let mut host = self.library.session(
                    &self.id,
                    &self.system_prompt,
                    tools,
                    Arc::new(EpisodeGate::new(names)),
                )?;
                host.seed_resume_from_messages(history, message)?;
                host.set_next_turn_overrides(TurnOverrides {
                    suppress_transcript_autoload: true,
                    ..TurnOverrides::default()
                });
                tokio::time::timeout(TURN_TIMEOUT, host.turn(message))
                    .await
                    .map_err(|_| Error::TimedOut {
                        seat: self.id.clone(),
                    })?
                    .map_err(Error::Harness)
            })
            .await
    }
}
