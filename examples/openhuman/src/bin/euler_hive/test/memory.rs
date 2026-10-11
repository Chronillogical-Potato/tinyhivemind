//! Run-scoped memory: namespaces, the rejoin watermark, read-only pools and the wall clock.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;

use tinyhivemind_core::embed::{
    ConversationKind, ConversationRef, RouteCandidate, RoutingRequest, RoutingSource,
};

use super::super::agents::system_prompt;
use super::super::floors::{Assignment, Federation, Row};
use super::super::memory::{run_scoped, scope_binding};
use super::super::package::{self, SeatTool};
use super::super::router::addressed;
use super::super::sandbox::docker_args;
use super::super::search::{Provider, condense_exa};
use super::super::tools::{SeatScope, schema};
use super::super::turn::{TurnInput, prompt};


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

fn memory_for(reads: Vec<tinyhivemind_lang::MemoryReach>) -> super::memory::SeatMemory {
    let (_, hive) = lowered();
    let mut binding = scope_binding(
        hive.seats[9].memory.as_ref().expect("bound"),
        "team:euler-hive",
        "r1",
    )
    .expect("scoped");
    binding.settings.reads = reads;
    let engine: std::sync::Arc<dyn tinymemory_api::MemoryEngine> =
        std::sync::Arc::new(tinymemory_api::conformance::reference::ReferenceEngine::new());
    super::memory::SeatMemory::new(
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
        super::turn::within(Duration::from_millis(20), pending)
            .await
            .is_none()
    );
    assert_eq!(
        super::turn::within(Duration::from_secs(5), async { 7 }).await,
        Some(7)
    );
}
