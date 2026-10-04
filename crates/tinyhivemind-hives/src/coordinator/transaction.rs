//! Optimistic transactions: copy live state, mutate the copy, persist it
//! incrementally outside the live lock, then publish it.
//!
//! Writers serialize on an async writer gate, so in-process writers never
//! conflict; a [`Error::RevisionConflict`] therefore means another process
//! committed to the same store. The writer reloads the store and recomputes
//! its change from the reloaded state, a bounded number of times.
use super::{Coordinator, interrupt};
use crate::{Commit, Error, Result, StoredState};

/// Reload-and-retry attempts after a storage revision conflict.
const CONFLICT_RETRIES: usize = 4;
/// Held while a transaction computes and persists its next state.
pub(super) type WriterGate<'a> = tokio::sync::MutexGuard<'a, ()>;
/// A consistent base for one attempt and the cancelled-turn interruptions it
/// already contains, which its commit makes durable.
pub(super) struct Snapshot {
    pub base: StoredState,
    flushed: Vec<String>,
}
impl Coordinator {
    /// Run `operation` on a copy of live state and commit it.
    /// # Errors
    /// Returns the operation's error, or a storage error once retries are spent.
    pub(super) async fn update<T>(
        &self,
        operation: impl FnMut(&mut StoredState) -> Result<T>,
    ) -> Result<T> {
        let gate = self.inner.writer.lock().await;
        self.update_locked(&gate, operation).await
    }
    /// [`Self::update`] for a caller already holding the writer gate.
    pub(super) async fn update_locked<T>(
        &self,
        gate: &WriterGate<'_>,
        mut operation: impl FnMut(&mut StoredState) -> Result<T>,
    ) -> Result<T> {
        let value = self
            .transact(gate, |state| Ok((true, operation(state)?)))
            .await?;
        self.inner.notify.notify_one();
        Ok(value)
    }
    /// Commit only when `operation` reports a change, retrying on conflicts.
    pub(super) async fn transact<T>(
        &self,
        _gate: &WriterGate<'_>,
        mut operation: impl FnMut(&mut StoredState) -> Result<(bool, T)>,
    ) -> Result<T> {
        let mut conflicts = 0;
        loop {
            let snapshot = self.snapshot()?;
            let mut next = snapshot.base.clone();
            let (changed, value) = operation(&mut next)?;
            if !changed {
                return Ok(value);
            }
            let attempt = self.persist(&snapshot, next).await;
            if self.settle(attempt, &mut conflicts).await? {
                return Ok(value);
            }
        }
    }
    /// Copy live state under the live lock. Hold the writer gate.
    pub(super) fn snapshot(&self) -> Result<Snapshot> {
        let live = self.lock()?;
        Ok(Snapshot {
            base: live.durable.clone(),
            flushed: live.unpersisted.keys().cloned().collect(),
        })
    }
    /// Commit `next` over `snapshot` and publish it. Hold the writer gate.
    pub(super) async fn persist(&self, snapshot: &Snapshot, mut next: StoredState) -> Result<()> {
        let base = &snapshot.base;
        next.revision = base.revision.checked_add(1).ok_or(Error::Exhausted)?;
        self.inner.options.retention.apply(&mut next);
        let appended = next.rows_since(base.messages.len());
        self.inner
            .storage
            .commit(Commit {
                expected_revision: base.revision,
                state: &next,
                appended: &appended,
            })
            .await?;
        let revision = next.revision;
        let mut live = self.lock()?;
        live.unpersisted
            .retain(|agent, _| !snapshot.flushed.contains(agent));
        live.durable = next;
        live.reapply_unpersisted();
        drop(live);
        self.inner.committed.send_replace(revision);
        Ok(())
    }
    /// `Ok(true)` once committed; `Ok(false)` after reloading for a retry.
    pub(super) async fn settle(&self, attempt: Result<()>, conflicts: &mut usize) -> Result<bool> {
        match attempt {
            Ok(()) => Ok(true),
            Err(Error::RevisionConflict { .. }) if *conflicts < CONFLICT_RETRIES => {
                *conflicts += 1;
                self.reload().await?;
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }
    /// Replace live state with the store's newer committed state.
    async fn reload(&self) -> Result<()> {
        let loaded = self.inner.storage.load().await?;
        let revision = loaded.revision;
        let mut live = self.lock()?;
        if revision < live.durable.revision {
            return Ok(());
        }
        live.durable = loaded;
        live.reapply_unpersisted();
        drop(live);
        self.inner.committed.send_replace(revision);
        Ok(())
    }
    /// Persist interruptions recorded while a dropped drain could not await.
    pub(super) async fn flush_unpersisted(&self) -> Result<()> {
        if self.lock()?.unpersisted.is_empty() {
            return Ok(());
        }
        self.update(|_| Ok(())).await
    }
    /// Record cancelled reservations synchronously, for `Drop`. Live state
    /// reflects them at once; the next commit makes them durable, and a crash
    /// before then is recovered as an interrupted running turn.
    pub(super) fn interrupt_unpersisted(&self, agents: impl IntoIterator<Item = String>) {
        const REASON: &str = "scheduler cancelled during turn";
        let Ok(mut live) = self.lock() else {
            return;
        };
        for agent in agents {
            if let Some(running) = live.durable.running.get(&agent) {
                let deferred = super::DeferredInterruption {
                    reason: REASON.into(),
                    delivery_sequence: running.delivery_sequence,
                    episode_id: running
                        .request
                        .episode
                        .as_ref()
                        .map(|ep| ep.episode_id.clone()),
                };
                interrupt(&mut live.durable, &agent, REASON);
                live.unpersisted.insert(agent, deferred);
            }
        }
    }
}
impl super::LiveState {
    /// Re-apply unpersisted interruptions to freshly published state.
    fn reapply_unpersisted(&mut self) {
        for (agent, reason) in &self.unpersisted {
            interrupt(&mut self.durable, agent, reason);
        }
    }
}
