//! Optimistic transactions: copy live state, mutate the copy, persist it
//! incrementally outside the live lock, then publish it.
//!
//! Writers serialize on an async writer gate, so in-process writers never
//! conflict. With single-writer fencing, an [`Error::RevisionConflict`] or
//! [`Error::Fenced`] indicates a newer coordinator has claimed ownership and
//! no further writes are possible.
use super::{Coordinator, interrupt};
use crate::{Commit, Error, Result, StoredState};
use std::sync::atomic::Ordering;
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
    /// Commit only when `operation` reports a change.
    /// With single-writer fencing, transactions do not retry: any persistence
    /// error is fatal (either fenced or storage corruption).
    pub(super) async fn transact<T>(
        &self,
        _gate: &WriterGate<'_>,
        mut operation: impl FnMut(&mut StoredState) -> Result<(bool, T)>,
    ) -> Result<T> {
        // Check if we've been fenced out before attempting any operation.
        if self.inner.fenced.load(Ordering::Acquire) {
            // We've been fenced; fail immediately without attempting further work.
            let snapshot = self.snapshot()?;
            return Err(Error::Fenced {
                coordinator: self.inner.writer_epoch,
                stored: snapshot.base.writer_epoch,
            });
        }

        let snapshot = self.snapshot()?;
        let mut next = snapshot.base.clone();
        let (changed, value) = operation(&mut next)?;
        if !changed {
            return Ok(value);
        }

        let attempt = self.persist(&snapshot, next).await;
        let mut conflicts = 0;
        self.settle(attempt, &mut conflicts).await?;
        Ok(value)
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
        // Ensure we're not already fenced out.
        if self.inner.fenced.load(Ordering::Acquire) {
            return Err(Error::Fenced {
                coordinator: self.inner.writer_epoch,
                stored: snapshot.base.writer_epoch,
            });
        }

        let base = &snapshot.base;
        next.revision = base.revision.checked_add(1).ok_or(Error::Exhausted)?;
        next.writer_epoch = self.inner.writer_epoch;

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

        // After successful commit, verify we still own the store.
        // If another coordinator claimed it (higher epoch), we're fenced.
        if next.writer_epoch != self.inner.writer_epoch {
            self.inner.fenced.store(true, Ordering::Release);
            return Err(Error::Fenced {
                coordinator: self.inner.writer_epoch,
                stored: next.writer_epoch,
            });
        }

        let revision = next.revision;
        let mut live = self.lock()?;
        live.unpersisted
            .retain(|agent, _| !snapshot.flushed.contains(agent));
        live.durable = next;
        drop(live);
        self.inner.committed.send_replace(revision);
        Ok(())
    }
    /// `Ok(true)` once committed. With single-writer fencing, there are no
    /// retries: conflicts indicate we've been fenced or the store is corrupted.
    pub(super) async fn settle(&self, attempt: Result<()>, _conflicts: &mut usize) -> Result<bool> {
        match attempt {
            Ok(()) => Ok(true),
            Err(error) => Err(error),
        }
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
    /// Re-apply unpersisted interruptions to freshly published state, but only
    /// when the currently running reservation matches the one that was interrupted.
    /// This prevents interrupting newer work started by another coordinator after
    /// a conflict reload.
    fn reapply_unpersisted(&mut self) {
        let unpersisted = std::mem::take(&mut self.unpersisted);
        for (agent, deferred) in unpersisted {
            // Check if the currently running reservation matches the interrupted one.
            let should_reapply = self.durable.running.get(&agent).is_some_and(|running| {
                running.delivery_sequence == deferred.delivery_sequence
                    && running
                        .request
                        .episode
                        .as_ref()
                        .map(|ep| ep.episode_id.clone())
                        == deferred.episode_id
            });

            if should_reapply {
                interrupt(&mut self.durable, &agent, &deferred.reason);
            }
            // If the reservation no longer matches, drop the deferred interruption
            // and the affected work will be recovered as interrupted on next restart.
        }
    }
}
