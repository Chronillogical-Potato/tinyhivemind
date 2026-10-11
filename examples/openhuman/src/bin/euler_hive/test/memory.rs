//! Run-scoped memory: namespaces, the rejoin watermark, read-only pools and the wall clock.

use super::super::memory::{run_scoped, scope_binding};
use super::support::*;

#[test]
fn memory_is_scoped_under_the_run_and_refuses_namespaces_outside_the_root() {
    assert_eq!(
        run_scoped("team:euler-hive/agent:x", "team:euler-hive", "r1").expect("scoped"),
        "team:euler-hive/ws:r1/agent:x"
    );
    assert_eq!(
        run_scoped("team:euler-hive", "team:euler-hive", "r1").expect("scoped"),
        "team:euler-hive/ws:r1"
    );
    assert!(run_scoped("team:other/agent:x", "team:euler-hive", "r1").is_err());
    let (_, hive) = lowered();
    for seat in &hive.seats {
        let binding = seat.memory.as_ref().expect("bound");
        let scoped = scope_binding(binding, "team:euler-hive", "r1").expect("scoped");
        assert_eq!(
            scoped.namespace,
            format!("team:euler-hive/ws:r1/agent:{}", seat.id)
        );
        assert_eq!(scoped.root, "team:euler-hive/ws:r1");
        assert!(
            scoped
                .settings
                .reads
                .iter()
                .all(|reach| reach.at.starts_with("team:euler-hive/ws:r1/"))
        );
        let harness = tinyhivemind_openhuman::language::memory_binding(&scoped)
            .expect("the adapter accepts a run-scoped agent binding");
        assert_eq!(harness.agent_id(), seat.id);
        assert_eq!(harness.root_namespace(), Some("team:euler-hive/ws:r1"));
    }
}

fn memory_for(reads: Vec<tinyhivemind_lang::MemoryReach>) -> super::super::memory::SeatMemory {
    let engine: std::sync::Arc<dyn tinymemory_api::MemoryEngine> =
        std::sync::Arc::new(tinymemory_api::conformance::reference::ReferenceEngine::new());
    memory_on(engine, 9, reads)
}

fn memory_on(
    engine: std::sync::Arc<dyn tinymemory_api::MemoryEngine>,
    seat: usize,
    reads: Vec<tinyhivemind_lang::MemoryReach>,
) -> super::super::memory::SeatMemory {
    let (_, hive) = lowered();
    let mut binding = scope_binding(
        hive.seats[seat].memory.as_ref().expect("bound"),
        "team:euler-hive",
        "r1",
    )
    .expect("scoped");
    binding.settings.reads = reads;
    super::super::memory::SeatMemory::new(
        engine,
        binding,
        Some("team:euler-hive/ws:r1/project:verified".into()),
        true,
    )
}

#[tokio::test]
async fn a_failed_recall_leaves_the_rejoin_watermark_where_it_was() {
    use tinyhivemind_core::runtime::{Recall, RecallMoment, RecallRequest};
    let broken = memory_for(vec![tinyhivemind_lang::MemoryReach {
        at: "not a namespace".into(),
        inherit: false,
        descendants: false,
    }]);
    let request = |moment| RecallRequest {
        seat: "verify-constraints".into(),
        conversation: "c".into(),
        focus: Some("findings".into()),
        moment,
        budget_chars: 500,
    };
    assert!(broken.recall(&request(RecallMoment::Rejoin)).await.is_err());
    assert!(
        broken.seen().is_none(),
        "a failed read must not advance the watermark"
    );
    let working = memory_for(vec![]);
    working
        .recall(&request(RecallMoment::Rejoin))
        .await
        .expect("an empty read succeeds");
    assert!(working.seen().is_some());
}

#[tokio::test]
async fn nobody_publishes_to_a_read_only_pool() {
    let entry = tinyhivemind_core::runtime::RememberEntry {
        kind: tinyhivemind_core::runtime::EntryKind::Outcome,
        text: "checked".into(),
    };
    let writable = memory_for(vec![]);
    assert!(writable.pool_writer());
    writable
        .publish(&entry, "c")
        .await
        .expect("a writable pool takes the finding");
    let pinned = memory_for(vec![]).read_only_pool(true);
    assert!(
        !pinned.pool_writer(),
        "the belt no longer offers verified=true"
    );
    let refused = pinned
        .publish(&entry, "c")
        .await
        .expect_err("a read-only pool refuses");
    assert!(refused.to_string().contains("read-only"));
}

#[tokio::test]
async fn a_memory_call_past_the_wall_clock_is_abandoned() {
    use std::time::Duration;
    let pending = std::future::pending::<()>();
    assert!(
        super::super::turn::within(Duration::from_millis(20), pending)
            .await
            .is_none()
    );
    assert_eq!(
        super::super::turn::within(Duration::from_secs(5), async { 7 }).await,
        Some(7)
    );
}

/// An engine that lets a peer store a note just after the first list is
/// answered, as if the peer wrote while a recall was still reading.
struct LatePeer {
    inner: std::sync::Arc<dyn tinymemory_api::MemoryEngine>,
    namespace: String,
    stored: std::sync::atomic::AtomicBool,
}

#[tinymemory_api::async_trait]
impl tinymemory_api::MemoryEngine for LatePeer {
    fn descriptor(&self) -> &tinymemory_api::EngineDescriptor {
        self.inner.descriptor()
    }

    async fn health(&self) -> tinymemory_api::EngineHealth {
        self.inner.health().await
    }

    async fn recall(
        &self,
        req: tinymemory_api::RecallRequest,
    ) -> tinymemory_api::Result<tinymemory_api::RecallAnswer> {
        self.inner.recall(req).await
    }

    async fn fetch(
        &self,
        req: tinymemory_api::FetchRequest,
    ) -> tinymemory_api::Result<tinymemory_api::FetchPage> {
        self.inner.fetch(req).await
    }

    async fn store(
        &self,
        item: tinymemory_api::StoreItem,
    ) -> tinymemory_api::Result<tinymemory_api::StoreReceipt> {
        self.inner.store(item).await
    }

    async fn forget(
        &self,
        target: tinymemory_api::ForgetTarget,
    ) -> tinymemory_api::Result<tinymemory_api::ForgetReport> {
        self.inner.forget(target).await
    }

    async fn list(
        &self,
        req: tinymemory_api::ListRequest,
    ) -> tinymemory_api::Result<tinymemory_api::ListPage> {
        let page = self.inner.list(req).await;
        if !self.stored.swap(true, std::sync::atomic::Ordering::SeqCst) {
            let meta = tinymemory_api::MemoryMeta {
                namespace: self.namespace.parse().expect("namespace"),
                agent_id: Some("a-peer".into()),
                observed_at: Some(tinymemory_api::chrono::Utc::now()),
                ..tinymemory_api::MemoryMeta::from_source(
                    tinymemory_api::SourceKind::Agent,
                    Some("a-peer".into()),
                )
            };
            self.inner
                .store(tinymemory_api::StoreItem::learning(
                    "a peer's late note",
                    tinymemory_api::LearningKind::Fact,
                    1.0,
                    meta,
                ))
                .await?;
        }
        page
    }
}

#[tokio::test]
async fn a_note_stored_while_a_recall_reads_is_found_by_the_next_rejoin() {
    use tinyhivemind_core::runtime::{Recall, RecallMoment, RecallRequest};
    let namespace = "team:euler-hive/ws:r1/agent:a-peer";
    let engine: std::sync::Arc<dyn tinymemory_api::MemoryEngine> = std::sync::Arc::new(LatePeer {
        inner: std::sync::Arc::new(tinymemory_api::conformance::reference::ReferenceEngine::new()),
        namespace: namespace.into(),
        stored: std::sync::atomic::AtomicBool::new(false),
    });
    let seat = memory_on(
        engine,
        9,
        vec![tinyhivemind_lang::MemoryReach {
            at: namespace.into(),
            inherit: false,
            descendants: false,
        }],
    );
    let request = RecallRequest {
        seat: "verify-constraints".into(),
        conversation: "c".into(),
        focus: Some("findings".into()),
        moment: RecallMoment::Rejoin,
        budget_chars: 500,
    };
    let first = seat.recall(&request).await.expect("first rejoin");
    assert!(first.is_empty(), "the note did not exist when it was read");
    let second = seat.recall(&request).await.expect("second rejoin");
    assert!(
        second
            .iter()
            .flat_map(|note| &note.lines)
            .any(|line| line.contains("late note")),
        "the note written during the first read must not fall behind the watermark: {second:?}"
    );
}

#[tokio::test]
async fn an_expired_deadline_never_polls_a_memory_operation() {
    let called = std::sync::atomic::AtomicBool::new(false);
    let result = super::super::turn::within(std::time::Duration::ZERO, async {
        called.store(true, std::sync::atomic::Ordering::SeqCst);
    }).await;
    assert!(result.is_none());
    assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
}
