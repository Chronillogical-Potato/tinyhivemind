//! Hive memory through core's `Recall` and `Remember` ports, over the
//! in-memory reference engine, a recording wrapper, a server that never
//! answers, and (when `CORTEX_DB_URL` is set) a live CortexDB.

use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tinyhivemind_core::runtime::{
    BriefingNote, EntryKind, Error as CoreError, MemoryEntry, Recall, RecallMoment, RecallRequest,
    Remember, RememberRequest,
};
use tinymemory_api::conformance::ReferenceEngine;
use tinymemory_api::{
    BeliefsRequest, ConsolidateReceipt, ConsolidateRequest, EngineDescriptor, EngineHealth,
    ExplorePage, ExploreRequest, FetchPage, FetchRequest, ForgetReport, ForgetTarget, GetRequest,
    Hit, ListPage, ListRequest, MemoryEngine, RecallAnswer, StoreItem, StoreReceipt, WaitFor,
    WriteOptions, async_trait,
};

use super::*;
use crate::block_on;

const SEATS: [&str; 3] = ["lead", "implementer", "tester"];

fn memory_on(engine: Arc<dyn MemoryEngine>, run: &str) -> HiveMemory {
    HiveMemory::new(engine, run, &SEATS, 1200).expect("memory")
}

fn failed_pytest() -> Vec<MemoryEntry> {
    let ledger = LedgerEntry {
        cmd: "pytest tests/test_parser.py -x".into(),
        exit: Some(1),
        outcome: "ModuleNotFoundError: No module named 'parser_core'".into(),
    };
    vec![
        ledger.to_entry(),
        MemoryEntry {
            kind: EntryKind::Outcome,
            text: "pytest still fails; the import path is wrong".into(),
        },
    ]
}

fn store(memory: &HiveMemory, seat: &str, entries: Vec<MemoryEntry>) -> Result<(), CoreError> {
    block_on(memory.remember(&RememberRequest {
        seat: seat.into(),
        conversation: memory.conversation(),
        through: None,
        entries,
    }))
}

fn recall(
    memory: &HiveMemory,
    seat: &str,
    moment: RecallMoment,
) -> Result<Vec<BriefingNote>, CoreError> {
    block_on(memory.recall(&RecallRequest {
        seat: seat.into(),
        conversation: memory.conversation(),
        focus: Some("pytest parser import".into()),
        moment,
        budget_chars: memory.budget_chars(),
    }))
}

fn text(notes: &[BriefingNote]) -> String {
    notes
        .iter()
        .map(|note| format!("{}\n{}", note.heading, note.lines.join("\n")))
        .collect::<Vec<_>>()
        .join("\n")
}

fn chain(error: &CoreError) -> String {
    let mut out = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        out.push_str(&format!(": {cause}"));
        source = cause.source();
    }
    out
}

#[test]
fn the_namespace_root_is_pinned_to_the_run_id() {
    let memory = memory_on(Arc::new(ReferenceEngine::new()), "trial 7/a");
    assert_eq!(memory.root().to_string(), "team:trial-7-a");
    assert_eq!(memory.conversation(), "team:trial-7-a");
    assert_eq!(memory.budget_chars(), 1200 * CHARS_PER_TOKEN);
    assert_eq!(
        memory
            .inner
            .layout
            .conversations("implementer")
            .expect("node")
            .to_string(),
        "team:trial-7-a/agent:implementer"
    );
    assert!(run_root("///").is_err());
    assert!(generated_run_id("hive").starts_with("hive-"));
}

#[test]
fn another_runs_conversation_is_refused_by_both_ports() {
    let memory = memory_on(Arc::new(ReferenceEngine::new()), "mine");
    let theirs = block_on(memory.recall(&RecallRequest {
        seat: "lead".into(),
        conversation: "team:theirs".into(),
        focus: None,
        moment: RecallMoment::SessionStart,
        budget_chars: 400,
    }));
    assert!(matches!(theirs, Err(CoreError::Recall { .. })));
    let write = block_on(memory.remember(&RememberRequest {
        seat: "lead".into(),
        conversation: "team:theirs".into(),
        through: None,
        entries: failed_pytest(),
    }));
    assert!(matches!(write, Err(CoreError::Remember { .. })));
}

#[test]
fn failed_commands_become_failed_attempts() {
    let ok = LedgerEntry {
        cmd: "ls".into(),
        exit: Some(0),
        outcome: "a b".into(),
    };
    let refused = LedgerEntry {
        cmd: "rm -rf /".into(),
        exit: None,
        outcome: "refused".into(),
    };
    assert_eq!(ok.to_entry().kind, EntryKind::Observation);
    assert_eq!(failed_pytest()[0].kind, EntryKind::FailedAttempt);
    assert_eq!(refused.to_entry().kind, EntryKind::FailedAttempt);
    assert!(refused.to_entry().text.contains("did not run"));
}

#[test]
fn a_seats_later_recall_surfaces_its_earlier_failed_attempt() {
    let memory = memory_on(Arc::new(ReferenceEngine::new()), "t-fail");
    store(&memory, "implementer", failed_pytest()).expect("stored");
    let notes = recall(&memory, "implementer", RecallMoment::SessionStart).expect("recall");
    let all = text(&notes);
    assert!(all.contains("[FAILED attempt] `pytest tests/test_parser.py -x` (failed, exit 1)"));
    assert!(all.contains("ModuleNotFoundError"));
}

#[test]
fn a_rejoin_shows_a_teammates_new_memory_once_and_never_the_seats_own() {
    let memory = memory_on(Arc::new(ReferenceEngine::new()), "t-rejoin");
    store(&memory, "implementer", failed_pytest()).expect("stored");
    let first = text(&recall(&memory, "tester", RecallMoment::Rejoin).expect("recall"));
    assert!(first.contains("@implementer") && first.contains("pytest"));
    let again = text(&recall(&memory, "tester", RecallMoment::Rejoin).expect("recall"));
    assert!(!again.contains("ModuleNotFoundError"), "already shown");
    let own = text(&recall(&memory, "implementer", RecallMoment::Rejoin).expect("recall"));
    assert!(!own.contains("ModuleNotFoundError"));
}

#[test]
fn a_compaction_recall_carries_the_seats_thread() {
    let memory = memory_on(Arc::new(ReferenceEngine::new()), "t-compact");
    store(&memory, "implementer", failed_pytest()).expect("stored");
    let moment = RecallMoment::Compaction {
        dropped: vec!["assistant: ran pytest".into()],
    };
    let notes = recall(&memory, "implementer", moment).expect("recall");
    assert!(text(&notes).contains("pytest"));
}

#[test]
fn two_run_ids_share_nothing() {
    let engine: Arc<dyn MemoryEngine> = Arc::new(ReferenceEngine::new());
    let one = memory_on(engine.clone(), "run-one");
    let two = memory_on(engine, "run-two");
    store(&one, "implementer", failed_pytest()).expect("stored");
    let other = recall(&two, "implementer", RecallMoment::SessionStart).expect("recall");
    assert!(other.is_empty(), "{other:?}");
}

/// The reference engine, recording how long each store asked to wait.
struct Recording {
    engine: ReferenceEngine,
    waits: Arc<Mutex<Vec<WaitFor>>>,
    /// How long an indexed (`Visible`) write takes before it returns.
    index_delay: Duration,
}

#[async_trait]
impl MemoryEngine for Recording {
    fn descriptor(&self) -> &EngineDescriptor {
        self.engine.descriptor()
    }
    async fn health(&self) -> EngineHealth {
        self.engine.health().await
    }
    async fn recall(
        &self,
        req: tinymemory_api::RecallRequest,
    ) -> tinymemory_api::Result<RecallAnswer> {
        self.engine.recall(req).await
    }
    async fn fetch(&self, req: FetchRequest) -> tinymemory_api::Result<FetchPage> {
        self.engine.fetch(req).await
    }
    async fn store(&self, item: StoreItem) -> tinymemory_api::Result<StoreReceipt> {
        self.store_with(item, WriteOptions::visible()).await
    }
    async fn store_with(
        &self,
        item: StoreItem,
        options: WriteOptions,
    ) -> tinymemory_api::Result<StoreReceipt> {
        self.waits.lock().expect("lock").push(options.wait);
        if options.wait == WaitFor::Visible {
            tokio::time::sleep(self.index_delay).await;
        }
        self.engine.store_with(item, options).await
    }
    async fn forget(&self, target: ForgetTarget) -> tinymemory_api::Result<ForgetReport> {
        self.engine.forget(target).await
    }
    async fn list(&self, req: ListRequest) -> tinymemory_api::Result<ListPage> {
        self.engine.list(req).await
    }
    async fn explore(&self, req: ExploreRequest) -> tinymemory_api::Result<ExplorePage> {
        self.engine.explore(req).await
    }
    async fn get(&self, req: GetRequest) -> tinymemory_api::Result<Vec<Hit>> {
        self.engine.get(req).await
    }
    async fn consolidate(
        &self,
        req: ConsolidateRequest,
    ) -> tinymemory_api::Result<ConsolidateReceipt> {
        self.engine.consolidate(req).await
    }
    async fn beliefs(&self, req: BeliefsRequest) -> tinymemory_api::Result<Vec<Hit>> {
        self.engine.beliefs(req).await
    }
}

#[test]
fn remember_waits_until_indexed_so_the_next_recall_sees_it() {
    let waits = Arc::new(Mutex::new(Vec::new()));
    let engine = Recording {
        engine: ReferenceEngine::new(),
        waits: waits.clone(),
        index_delay: Duration::ZERO,
    };
    let memory = memory_on(Arc::new(engine), "t-indexed");
    store(&memory, "implementer", failed_pytest()).expect("stored");
    assert_eq!(*waits.lock().expect("lock"), [WaitFor::Visible]);
    let next = text(&recall(&memory, "tester", RecallMoment::Rejoin).expect("recall"));
    assert!(next.contains("ModuleNotFoundError"), "{next}");
}

#[test]
fn an_index_slower_than_the_bound_still_keeps_the_turn() {
    let waits = Arc::new(Mutex::new(Vec::new()));
    let engine = Recording {
        engine: ReferenceEngine::new(),
        waits: waits.clone(),
        index_delay: Duration::from_secs(2),
    };
    let memory = memory_on(Arc::new(engine), "t-lag").with_timeouts(quick());
    let started = Instant::now();
    store(&memory, "implementer", failed_pytest()).expect("kept");
    assert!(
        started.elapsed() < Duration::from_millis(600),
        "within the bound"
    );
    assert_eq!(
        *waits.lock().expect("lock"),
        [WaitFor::Visible, WaitFor::Accepted]
    );
    let next = text(&recall(&memory, "tester", RecallMoment::Rejoin).expect("recall"));
    assert!(next.contains("ModuleNotFoundError"));
    let report = memory.finish();
    assert_eq!(report.len(), 1);
    assert!(report[0].starts_with("remember: 1 turns stored without waiting"));
}

#[test]
fn every_fifth_turn_of_a_seat_starts_a_belief_build() {
    let memory = memory_on(Arc::new(ReferenceEngine::new()), "t-build");
    for n in 0..5 {
        let entries = vec![MemoryEntry {
            kind: EntryKind::Note,
            text: format!("note {n}"),
        }];
        store(&memory, "lead", entries).expect("stored");
    }
    let jobs = memory.finish();
    assert_eq!(jobs.len(), 1, "{jobs:?}");
    assert!(jobs[0].starts_with("belief_build"));
    assert!(memory.finish().is_empty(), "drained");
}

/// A CortexDB address whose server accepts connections and never answers.
fn silent_server() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    (listener, url)
}

fn quick() -> Timeouts {
    Timeouts {
        recall: Duration::from_millis(300),
        remember: Duration::from_millis(300),
        finish: Duration::from_millis(300),
    }
}

#[test]
fn a_timeout_is_a_typed_port_error_with_the_reason() {
    let (_held, url) = silent_server();
    let memory = HiveMemory::cortex(&url, "k", "t-slow", &SEATS, 1200)
        .expect("memory")
        .with_timeouts(quick());
    let recalled = recall(&memory, "lead", RecallMoment::SessionStart);
    let error = recalled.expect_err("times out");
    assert!(matches!(error, CoreError::Recall { .. }));
    assert!(chain(&error).contains("timed out"), "{}", chain(&error));
    let stored = store(&memory, "lead", failed_pytest()).expect_err("times out");
    assert!(matches!(stored, CoreError::Remember { .. }));
    assert!(chain(&stored).contains("timed out"));
    assert!(memory.finish().is_empty());
}

#[test]
fn an_unreachable_server_is_a_remember_error() {
    let (listener, url) = silent_server();
    drop(listener);
    let memory = HiveMemory::cortex(&url, "k", "t-down", &SEATS, 1200)
        .expect("memory")
        .with_timeouts(quick());
    assert!(matches!(
        store(&memory, "lead", failed_pytest()),
        Err(CoreError::Remember { .. })
    ));
    assert!(HiveMemory::cortex("not a url", "k", "t", &SEATS, 1).is_err());
}

#[test]
fn a_bad_seat_or_run_id_is_refused_up_front() {
    let engine: Arc<dyn MemoryEngine> = Arc::new(ReferenceEngine::new());
    assert!(HiveMemory::new(engine.clone(), "ok", &["  "], 10).is_err());
    assert!(HiveMemory::new(engine, "%%%", &SEATS, 10).is_err());
}

/// Store and recall against a real CortexDB when `CORTEX_DB_URL` is set
/// (key from `CORTEX_DB_KEY`); a no-op otherwise. Measures what waiting for
/// the index costs over an accepted-only write, checks a teammate's very next
/// recall sees the turn, and cleans up after itself.
#[test]
fn live_cortex_memory_round_trip() {
    let Ok(url) = std::env::var("CORTEX_DB_URL") else {
        return;
    };
    let key = std::env::var("CORTEX_DB_KEY").unwrap_or_default();
    let run = generated_run_id("live");
    let memory = HiveMemory::cortex(&url, &key, &run, &SEATS, 1200).expect("memory");
    eprintln!("live: {}", memory.describe());
    let accepted_ms = {
        let node = memory.inner.layout.conversations("reviewer").expect("node");
        let item = notes::turn_item(node, "reviewer", 0, "- [note] baseline write".into());
        let engine = memory.inner.engine.clone();
        let started = Instant::now();
        memory
            .runtime
            .block_on(async { engine.store_with(item, WriteOptions::accepted()).await })
            .expect("accepted write");
        started.elapsed().as_millis()
    };
    let started = Instant::now();
    store(&memory, "implementer", failed_pytest()).expect("indexed write");
    let indexed_ms = started.elapsed().as_millis();
    eprintln!("live: remember accepted_ms={accepted_ms} indexed_ms={indexed_ms}");
    let started = Instant::now();
    let next = text(&recall(&memory, "tester", RecallMoment::Rejoin).expect("recall"));
    eprintln!(
        "live: immediate rejoin recall latency_ms={} notes:\n{next}",
        started.elapsed().as_millis()
    );
    let compaction = RecallMoment::Compaction {
        dropped: vec!["ran pytest".into()],
    };
    let started = Instant::now();
    let carried = recall(&memory, "implementer", compaction).expect("recall");
    eprintln!(
        "live: compaction recall latency_ms={} notes={}",
        started.elapsed().as_millis(),
        carried.len()
    );
    let filter = memory.inner.layout.holistic_filter();
    let engine = memory.inner.engine.clone();
    let forgotten = memory
        .runtime
        .block_on(async { engine.forget(ForgetTarget::Filter(filter)).await });
    eprintln!("live: cleanup {forgotten:?}");
    assert!(
        next.contains("ModuleNotFoundError"),
        "the very next recall sees the indexed turn"
    );
}
