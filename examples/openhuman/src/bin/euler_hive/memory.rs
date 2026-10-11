//! Each seat's lowered memory binding, honoured in full over CortexDB.
//!
//! `tinyhivemind_openhuman::language::memory_binding` hands the harness only
//! the agent id and root, so OpenHuman logs each seat's turns under
//! `<root>/agent:<seat>`. Everything else a binding asks for is this host's
//! job, and [`SeatMemory`] does it as core's `Recall` and `Remember` ports:
//!
//! - **recall moments**: only the moments in `recall_at` return anything;
//! - **reads**: every recall is one reach-filtered fetch per declared reach
//!   (inherit refused by validation, `descendants` honoured), never the root;
//! - **budget**: `budget_chars` is passed to `frame_recalled`, which clips;
//! - **remember kinds**: entries of a kind not in `remember` are dropped;
//! - **read-only identities** write nothing;
//! - **the verified pool** is written only by a seat the host names a pool
//!   writer (the verify desk), with the same kind filter.
//!
//! `lifecycle: run` is honoured by the run scope: every namespace in the
//! package is moved under `<root>/workspace:<run>` before any engine call,
//! so a run reads only what this run wrote.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tinyhivemind_core::runtime::{
    BriefingNote, Error as RuntimeError, Recall, RecallFuture, RecallMoment, RecallRequest,
    Remember, RememberEntry, RememberFuture, RememberRequest,
};
use tinyhivemind_lang::{EntryKind as LangEntry, LoweredMemoryBinding, RecallMoment as LangMoment};
use tinymemory_api::{
    FetchMode, FetchRequest, ItemKind, LearningKind, ListRequest, MemoryEngine, MemoryMeta,
    MetaFilter, Namespace, Reach, SourceKind, StoreItem,
};

/// Notes one reach contributes to a recall.
const NOTES_PER_REACH: usize = 6;

/// Re-root a package namespace under the run's scope.
///
/// # Errors
///
/// Fails when `namespace` is not `root` or below it.
pub(super) fn run_scoped(namespace: &str, root: &str, run: &str) -> anyhow::Result<String> {
    let scope = format!("{root}/workspace:{run}");
    if namespace == root {
        return Ok(scope);
    }
    let tail = namespace
        .strip_prefix(&format!("{root}/"))
        .ok_or_else(|| anyhow::anyhow!("{namespace} is outside the package root {root}"))?;
    Ok(format!("{scope}/{tail}"))
}

/// Move a lowered binding (identity, reads) into the run's scope.
///
/// # Errors
///
/// Fails when any namespace sits outside the package root.
pub(super) fn scope_binding(
    binding: &LoweredMemoryBinding,
    package_root: &str,
    run: &str,
) -> anyhow::Result<LoweredMemoryBinding> {
    let mut scoped = binding.clone();
    scoped.namespace = run_scoped(&binding.namespace, package_root, run)?;
    scoped.identity.namespace.clone_from(&scoped.namespace);
    scoped.root = run_scoped(&binding.root, package_root, run)?;
    for reach in &mut scoped.settings.reads {
        reach.at = run_scoped(&reach.at, package_root, run)?;
    }
    for namespace in [&scoped.namespace, &scoped.root] {
        namespace.parse::<Namespace>()?;
    }
    Ok(scoped)
}

/// Counters a seat's memory keeps, for the report.
#[derive(Debug, Default)]
pub(super) struct MemoryLedger {
    pub(super) recalls: AtomicU64,
    pub(super) recalled_notes: AtomicU64,
    pub(super) writes: AtomicU64,
    pub(super) pool_writes: AtomicU64,
    pub(super) dropped: AtomicU64,
}

impl MemoryLedger {
    pub(super) fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "recalls": self.recalls.load(Ordering::Relaxed),
            "recalled_notes": self.recalled_notes.load(Ordering::Relaxed),
            "writes": self.writes.load(Ordering::Relaxed),
            "pool_writes": self.pool_writes.load(Ordering::Relaxed),
            "dropped_by_kind_filter": self.dropped.load(Ordering::Relaxed),
        })
    }
}

/// One seat's run-scoped binding over the shared engine.
pub(super) struct SeatMemory {
    engine: Arc<dyn MemoryEngine>,
    binding: LoweredMemoryBinding,
    pool: Option<String>,
    pool_writer: bool,
    /// What every write and recall did, for the report.
    pub(super) ledger: MemoryLedger,
}

impl SeatMemory {
    /// Bind `binding` (already run-scoped). `pool` is the verified pool's
    /// run-scoped namespace; `pool_writer` lets this seat publish to it.
    pub(super) fn new(
        engine: Arc<dyn MemoryEngine>,
        binding: LoweredMemoryBinding,
        pool: Option<String>,
        pool_writer: bool,
    ) -> Self {
        Self {
            engine,
            binding,
            pool,
            pool_writer,
            ledger: MemoryLedger::default(),
        }
    }

    /// The binding this seat runs under.
    pub(super) fn binding(&self) -> &LoweredMemoryBinding {
        &self.binding
    }

    /// Whether this seat may publish to the verified pool.
    pub(super) fn pool_writer(&self) -> bool {
        self.pool_writer && self.pool.is_some()
    }

    fn allows(&self, moment: &RecallMoment) -> bool {
        let wanted = match moment {
            RecallMoment::SessionStart => LangMoment::SessionStart,
            RecallMoment::Rejoin => LangMoment::Rejoin,
            RecallMoment::Compaction { .. } => LangMoment::Compaction,
        };
        self.binding.settings.recall_at.contains(&wanted)
    }

    fn keeps(&self, kind: tinyhivemind_core::runtime::EntryKind) -> bool {
        use tinyhivemind_core::runtime::EntryKind as Core;
        let kind = match kind {
            Core::Observation => LangEntry::Observation,
            Core::FailedAttempt => LangEntry::FailedAttempt,
            Core::Outcome => LangEntry::Outcome,
            Core::Note => LangEntry::Note,
        };
        self.binding.settings.remember.contains(&kind)
    }

    /// Fetch the best notes for `query` from every declared reach.
    ///
    /// # Errors
    ///
    /// Returns the engine's failure.
    pub(super) async fn search(&self, query: &str) -> anyhow::Result<Vec<BriefingNote>> {
        self.ledger.recalls.fetch_add(1, Ordering::Relaxed);
        let mut notes = Vec::new();
        for reach in &self.binding.settings.reads {
            let mut request = FetchRequest::new(query, FetchMode::Hybrid, NOTES_PER_REACH);
            request.filter = MetaFilter {
                reach: Some(Reach {
                    at: reach.at.parse()?,
                    inherit: false,
                    descendants: reach.descendants,
                }),
                kinds: vec![ItemKind::Learning],
                ..MetaFilter::default()
            };
            let page = self.engine.fetch(request).await?;
            let lines: Vec<String> = page.hits.into_iter().map(|hit| hit.text).collect();
            self.ledger
                .recalled_notes
                .fetch_add(lines.len() as u64, Ordering::Relaxed);
            if !lines.is_empty() {
                notes.push(BriefingNote {
                    heading: self.heading(&reach.at),
                    lines,
                });
            }
        }
        Ok(notes)
    }

    /// Everything this run wrote at one reach since the seat last looked,
    /// newest first: the rejoin read.
    async fn since(&self, query: &str) -> anyhow::Result<Vec<BriefingNote>> {
        self.ledger.recalls.fetch_add(1, Ordering::Relaxed);
        let mut notes = Vec::new();
        for reach in &self.binding.settings.reads {
            let filter = MetaFilter {
                reach: Some(Reach {
                    at: reach.at.parse()?,
                    inherit: false,
                    descendants: reach.descendants,
                }),
                kinds: vec![ItemKind::Learning],
                ..MetaFilter::default()
            };
            let page = self.engine.list(ListRequest::new(filter, 50)).await?;
            let mut hits = page.items;
            // Another seat's notes at a shared reach; one's own at its own.
            hits.sort_by(|a, b| b.meta.observed_at.cmp(&a.meta.observed_at));
            let lines: Vec<String> = hits
                .into_iter()
                .take(NOTES_PER_REACH)
                .map(|hit| hit.text)
                .collect();
            self.ledger
                .recalled_notes
                .fetch_add(lines.len() as u64, Ordering::Relaxed);
            if !lines.is_empty() {
                notes.push(BriefingNote {
                    heading: self.heading(&reach.at),
                    lines,
                });
            }
        }
        let _ = query;
        Ok(notes)
    }

    fn heading(&self, at: &str) -> String {
        if Some(at) == self.pool.as_deref() {
            "Verified findings (shared pool)".into()
        } else if at == self.binding.namespace {
            "Your own notes".into()
        } else {
            format!("Notes at {at}")
        }
    }

    async fn store(&self, at: &str, entry: &RememberEntry, conversation: &str) -> anyhow::Result<()> {
        let text = entry.text.trim();
        if text.is_empty() {
            return Ok(());
        }
        let meta = MemoryMeta {
            namespace: at.parse()?,
            agent_id: Some(self.binding.agent_id.clone()),
            thread_id: Some(conversation.to_owned()),
            tags: vec![format!("hive-entry:{}", label(entry.kind))],
            observed_at: Some(tinymemory_api::chrono::Utc::now()),
            ..MemoryMeta::from_source(SourceKind::Agent, Some(self.binding.agent_id.clone()))
        };
        self.engine
            .store(StoreItem::learning(
                format!("{} ({}): {text}", label(entry.kind), self.binding.agent_id),
                learning_kind(entry.kind),
                1.0,
                meta,
            ))
            .await?;
        Ok(())
    }

    /// Publish one verified finding to the shared pool.
    ///
    /// # Errors
    ///
    /// Refuses a seat that is not a pool writer, a filtered kind, or a
    /// read-only identity; otherwise returns the engine's failure.
    pub(super) async fn publish(&self, entry: &RememberEntry, conversation: &str) -> anyhow::Result<()> {
        let pool = self
            .pool
            .as_deref()
            .filter(|_| self.pool_writer)
            .ok_or_else(|| anyhow::anyhow!("only the verify desk may write the verified pool"))?;
        if self.binding.identity.read_only {
            anyhow::bail!("this seat's memory is read-only");
        }
        if !self.keeps(entry.kind) {
            self.ledger.dropped.fetch_add(1, Ordering::Relaxed);
            anyhow::bail!("this seat may not remember {} entries", label(entry.kind));
        }
        self.store(pool, entry, conversation).await?;
        self.ledger.pool_writes.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

impl Recall for SeatMemory {
    fn recall<'a>(&'a self, request: &'a RecallRequest) -> RecallFuture<'a> {
        Box::pin(async move {
            if !self.allows(&request.moment) {
                return Ok(Vec::new());
            }
            let result = match &request.moment {
                RecallMoment::SessionStart => {
                    self.search(request.focus.as_deref().unwrap_or("findings")).await
                }
                RecallMoment::Rejoin => self.since(request.focus.as_deref().unwrap_or("")).await,
                RecallMoment::Compaction { dropped } => {
                    let query: String = dropped.join("\n").chars().take(2_000).collect();
                    self.search(if query.trim().is_empty() { "findings" } else { &query })
                        .await
                }
            };
            result.map_err(|error| RuntimeError::Recall {
                source: error.into(),
            })
        })
    }
}

impl Remember for SeatMemory {
    fn remember<'a>(&'a self, request: &'a RememberRequest) -> RememberFuture<'a> {
        Box::pin(async move {
            if self.binding.identity.read_only {
                return Ok(());
            }
            for entry in &request.entries {
                if !self.keeps(entry.kind) {
                    self.ledger.dropped.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                self.store(&self.binding.namespace, entry, &request.conversation)
                    .await
                    .map_err(|error| RuntimeError::Remember {
                        source: error.into(),
                    })?;
                self.ledger.writes.fetch_add(1, Ordering::Relaxed);
            }
            Ok(())
        })
    }
}

fn label(kind: tinyhivemind_core::runtime::EntryKind) -> &'static str {
    use tinyhivemind_core::runtime::EntryKind as Core;
    match kind {
        Core::Observation => "observation",
        Core::FailedAttempt => "failed_attempt",
        Core::Outcome => "outcome",
        Core::Note => "note",
    }
}

fn learning_kind(kind: tinyhivemind_core::runtime::EntryKind) -> LearningKind {
    use tinyhivemind_core::runtime::EntryKind as Core;
    match kind {
        Core::Observation | Core::Outcome => LearningKind::Fact,
        Core::FailedAttempt => LearningKind::Correction,
        Core::Note => LearningKind::Other,
    }
}

/// Count what the store holds under `at`, by item kind: the proof the run
/// really wrote to CortexDB, read back from it.
///
/// # Errors
///
/// Returns the engine's failure.
pub(super) async fn count_at(
    engine: &dyn MemoryEngine,
    at: &str,
) -> anyhow::Result<BTreeMap<String, usize>> {
    let mut counts = BTreeMap::new();
    let mut cursor = None;
    loop {
        let mut request = ListRequest::new(
            MetaFilter {
                reach: Some(Reach::subtree(at.parse()?)),
                ..MetaFilter::default()
            },
            200,
        );
        request.cursor = cursor;
        let page = engine.list(request).await?;
        for item in &page.items {
            *counts.entry(format!("{:?}", item.kind).to_lowercase()).or_insert(0) += 1;
        }
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    Ok(counts)
}
