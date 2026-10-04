//! Host-scope observation: the unfiltered transcript, a committed-change
//! signal, and episode status.
//!
//! Agent reads ([`Coordinator::read_hive`], [`Coordinator::read_direct`])
//! enforce membership and reject [`super::HOST_ID`]; the host sees everything,
//! including replies its `send_as_host` messages received.
use super::{Coordinator, EpisodePhase, EpisodeStatus, Message};
use crate::{EpisodeRecord, Result};
use tokio::sync::watch;

impl Coordinator {
    /// Read the whole transcript in sequence order, private rows included.
    /// `after` excludes that sequence and everything before it.
    /// # Errors
    /// Returns a poisoned shared lock error.
    pub fn read_transcript(&self, after: Option<u64>) -> Result<Vec<Message>> {
        let live = self.lock()?;
        let messages = &live.durable.messages;
        // Sequences are appended in ascending order, so the cursor bisects.
        let start = after.map_or(0, |cursor| {
            messages.partition_point(|message| message.sequence <= cursor)
        });
        Ok(messages[start..].to_vec())
    }
    /// Watch the latest committed storage revision.
    ///
    /// Every commit advances it: an appended message, a claimed or finished
    /// turn, an episode settling or failing, an interruption, a membership
    /// change. A host awaits `changed()` and then reads what it tracks —
    /// [`Self::read_transcript`] from its cursor, [`Self::episodes`],
    /// [`Self::interruptions`]. Reads never advance it.
    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.inner.committed.subscribe()
    }
    /// Status of every retained episode, in the order they were opened.
    /// # Errors
    /// Returns a poisoned shared lock error.
    pub fn episodes(&self) -> Result<Vec<EpisodeStatus>> {
        Ok(self.lock()?.durable.episodes.iter().map(status).collect())
    }
}
fn status(record: &EpisodeRecord) -> EpisodeStatus {
    let phase = match (&record.failure, record.finished, record.waiting) {
        (Some(reason), _, _) => EpisodePhase::Failed(reason.clone()),
        (None, true, _) => EpisodePhase::Settled,
        (None, false, true) => EpisodePhase::AwaitingRelease,
        (None, false, false) => EpisodePhase::Open,
    };
    EpisodeStatus {
        episode_id: record.episode_id.clone(),
        hive_id: record.hive.hive_id.clone(),
        opened_at: record.opened_at,
        thread: record.thread,
        starters: record.starters.clone(),
        phase,
    }
}
