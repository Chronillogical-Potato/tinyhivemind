//! The hosted runner's pure pieces: seeding from the host's log, the belt,
//! and the gate that admits it. Running a hosted turn is proven with the
//! other runners in `runner/test.rs`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::sync::Arc;

use async_trait::async_trait;
use openhuman_core::agent::tool_policy::{
    ToolCallContext, ToolPolicy, ToolPolicyDecision, ToolPolicyRequest,
};
use serde_json::json;
use tinyhivemind::{Conversation, Sequence, SessionLog};
use tinyhivemind_tools::{EpisodeTools, served_specs};

use super::EpisodeBelt;
use super::seed::history;
use crate::offline::MemoryLog;

fn desk(thread_root: Option<Sequence>) -> Conversation {
    Conversation {
        desk_id: "engineering".into(),
        desk_name: "Engineering".into(),
        thread_root,
    }
}

/// The task, a desk post, an ask from one to two and its answer in the
/// thread, a note to one alone, and a desk post from three.
fn journal() -> MemoryLog {
    let log = MemoryLog::new("engineering");
    log.append(
        "operator",
        "the login flow rejects valid credentials",
        None,
        None,
    );
    log.append("one", "looking into it", None, None);
    let root = log.append("one", "asks @two: which port?", None, Some("two"));
    log.append("two", "port 8080", Some(root), None);
    log.append("desk", "you hold open work", None, Some("one"));
    log.append("three", "the cache is stale", None, None);
    log
}

#[tokio::test]
async fn a_seat_is_seeded_with_what_it_was_shown_its_own_rows_as_its_turns() {
    let log = journal();
    let seen = history(&log, desk(None), "one", Sequence(6), 30)
        .await
        .expect("reads");
    assert_eq!(
        seen[0],
        (
            "user".into(),
            "@operator: the login flow rejects valid credentials".into()
        )
    );
    assert_eq!(seen[1], ("assistant".into(), "looking into it".into()));
    assert!(seen.contains(&("assistant".into(), "asks @two: which port?".into())));
    assert!(seen.contains(&("user".into(), "@desk: you hold open work".into())));
    assert_eq!(
        seen.last(),
        Some(&("user".into(), "@three: the cache is stale".into()))
    );
}

#[tokio::test]
async fn a_row_the_seat_was_not_addressed_on_is_withheld() {
    let log = journal();
    let seen = history(&log, desk(None), "three", Sequence(6), 30)
        .await
        .expect("reads");
    let text: Vec<&str> = seen.iter().map(|(_, content)| content.as_str()).collect();
    assert!(
        !text.iter().any(|row| row.contains("which port")),
        "{text:?}"
    );
    assert!(!text.iter().any(|row| row.contains("8080")), "{text:?}");
    assert!(
        !text.iter().any(|row| row.contains("open work")),
        "{text:?}"
    );
    assert!(text.contains(&"@operator: the login flow rejects valid credentials"));
}

#[tokio::test]
async fn nothing_above_the_watermark_is_seeded() {
    let log = journal();
    let seen = history(&log, desk(None), "one", Sequence(2), 30)
        .await
        .expect("reads");
    assert_eq!(seen.len(), 2, "{seen:?}");
    let none = history(&log, desk(None), "one", Sequence(0), 30)
        .await
        .expect("reads");
    assert!(none.is_empty(), "a first turn has no history");
}

#[tokio::test]
async fn a_thread_turn_is_seeded_with_the_conversation_alone() {
    let log = journal();
    let seen = history(&log, desk(Some(Sequence(3))), "two", Sequence(4), 30)
        .await
        .expect("reads");
    assert_eq!(
        seen,
        vec![
            ("user".into(), "@one: asks @two: which port?".into()),
            ("assistant".into(), "port 8080".into()),
        ]
    );
}

#[tokio::test]
async fn the_memory_log_pages_newest_first_and_says_when_it_is_done() {
    let log = journal();
    let first = log.read_before(None, 4).await.expect("reads");
    let sequences: Vec<u64> = first.messages.iter().map(|row| row.sequence.0).collect();
    assert_eq!(sequences, vec![6, 5, 4, 3]);
    assert_eq!(first.next_before, Some(Sequence(3)));
    let rest = log.read_before(first.next_before, 4).await.expect("reads");
    let sequences: Vec<u64> = rest.messages.iter().map(|row| row.sequence.0).collect();
    assert_eq!(sequences, vec![2, 1]);
    assert_eq!(rest.next_before, None, "the log is finished");
    assert_eq!(
        first.messages[2].parent,
        Some(Sequence(3)),
        "a thread row names its root"
    );
    assert_eq!(log.latest(), Sequence(6));
    assert_eq!(log.all().len(), 6);
    assert_eq!(log.thread(Sequence(3)).len(), 2);
    assert_eq!(
        log.thread_since(Sequence(3), Sequence(3)),
        vec!["@two: port 8080"]
    );
    let for_two = log.desk_since("two", Sequence(0));
    assert!(for_two.iter().any(|row| row.contains("which port")));
    assert!(!for_two.iter().any(|row| row.contains("open work")));
    assert!(format!("{log:?}").contains("engineering"));
}

fn request(tool: &str) -> ToolPolicyRequest {
    #[allow(deprecated)]
    ToolPolicyRequest {
        tool_name: tool.to_owned(),
        arguments: json!({}),
        context: ToolCallContext::session("session", "internal", "lead", "call-1", 1),
        generated_tool: None,
        session_id: String::new(),
        channel: String::new(),
        agent_definition_id: String::new(),
    }
}

/// A host policy that allows everything, so a denial can only be the
/// admission's own.
#[derive(Debug)]
struct AllowAll;

#[async_trait]
impl ToolPolicy for AllowAll {
    fn name(&self) -> &'static str {
        "allow_all"
    }

    async fn check(&self, _request: &ToolPolicyRequest) -> ToolPolicyDecision {
        ToolPolicyDecision::Allow
    }
}

#[tokio::test]
async fn the_belt_is_the_served_vocabulary_admitted_over_the_hosts_own_gate() {
    let tools = Arc::new(EpisodeTools::new(["lead"]));
    let belt = EpisodeBelt::new("lead", &tools);
    let served: Vec<&str> = served_specs().map(|spec| spec.name).collect();
    assert_eq!(belt.names(), served.as_slice());
    assert_eq!(belt.tools.len(), served.len());
    assert!(format!("{belt:?}").contains("complete_episode"));

    let alone = belt.admit(None);
    assert_eq!(alone.name(), "episode_admission");
    assert!(matches!(
        alone.check(&request("complete_episode")).await,
        ToolPolicyDecision::Allow
    ));
    assert!(matches!(
        alone.check(&request("shell")).await,
        ToolPolicyDecision::Deny { .. }
    ));

    let hosted = belt.admit(Some(Arc::new(AllowAll)));
    assert!(matches!(
        hosted.check(&request("read")).await,
        ToolPolicyDecision::Allow
    ));
    assert!(
        matches!(
            hosted.check(&request("chargebee_refund")).await,
            ToolPolicyDecision::Allow
        ),
        "the host's own tools are the host's gate's to decide"
    );
}
