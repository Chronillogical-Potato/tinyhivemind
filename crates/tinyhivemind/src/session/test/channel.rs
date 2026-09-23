//! Channel-level projection tests: root-and-first-reply narrowing, per-root
//! promotion, and the window/scan bookkeeping that stops a channel read.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::super::*;
use super::support::{FakeLog, message, page, query};

#[tokio::test]
async fn filters_by_exact_desk_id_or_name_and_general_aliases() {
    let log = FakeLog::new(vec![page(
        vec![
            message(5, Some("Engineering"), None, "name"),
            message(4, Some("engineering"), None, "id"),
            message(3, Some("ENGINEERING"), None, "wrong case"),
        ],
        None,
    )]);
    let history = project_session(&log, &query(5)).await.expect("projects");
    assert_eq!(
        history
            .iter()
            .map(|item| item.content.as_str())
            .collect::<Vec<_>>(),
        vec!["id", "name"]
    );

    let general = FakeLog::new(vec![page(vec![message(2, None, None, "general")], None)]);
    let mut general_query = query(2);
    general_query.conversation.desk_id = "General".into();
    general_query.conversation.desk_name = "General".into();
    assert_eq!(
        project_session(&general, &general_query)
            .await
            .expect("general")
            .len(),
        1
    );
}

#[tokio::test]
async fn channel_projection_keeps_a_root_and_its_first_reply() {
    let log = FakeLog::new(vec![page(
        vec![
            message(4, Some("engineering"), Some(2), "second reply"),
            message(3, Some("engineering"), Some(2), "first reply"),
            message(2, Some("engineering"), None, "channel"),
        ],
        None,
    )]);
    let history = project_session(&log, &query(5)).await.expect("projects");
    assert_eq!(
        history
            .iter()
            .map(|item| item.content.as_str())
            .collect::<Vec<_>>(),
        vec!["channel", "first reply"]
    );
}

#[tokio::test]
async fn channel_projection_promotes_one_reply_per_root_independently() {
    let log = FakeLog::new(vec![page(
        vec![
            message(6, Some("engineering"), Some(2), "late on first"),
            message(5, Some("engineering"), Some(3), "answer two"),
            message(4, Some("engineering"), Some(2), "answer one"),
            message(3, Some("engineering"), None, "question two"),
            message(2, Some("engineering"), None, "question one"),
        ],
        None,
    )]);
    let history = project_session(&log, &query(9)).await.expect("projects");
    assert_eq!(
        history
            .iter()
            .map(|item| item.content.as_str())
            .collect::<Vec<_>>(),
        vec!["question one", "question two", "answer one", "answer two"]
    );
}

#[tokio::test]
async fn channel_projection_never_promotes_a_reply_to_a_reply() {
    let log = FakeLog::new(vec![page(
        vec![
            message(4, Some("engineering"), Some(3), "grandchild"),
            message(3, Some("engineering"), Some(2), "first reply"),
            message(2, Some("engineering"), None, "root"),
        ],
        None,
    )]);
    let history = project_session(&log, &query(5)).await.expect("projects");
    assert_eq!(
        history
            .iter()
            .map(|item| item.content.as_str())
            .collect::<Vec<_>>(),
        vec!["root", "first reply"]
    );
}

#[tokio::test]
async fn channel_projection_drops_a_reply_whose_root_is_outside_the_scan() {
    let log = FakeLog::new(vec![page(
        vec![message(9, Some("engineering"), Some(2), "orphan reply")],
        None,
    )]);
    assert!(
        project_session(&log, &query(5))
            .await
            .expect("projects")
            .is_empty()
    );
}

#[tokio::test]
async fn channel_projection_promotes_past_an_empty_first_reply_and_an_empty_root() {
    let log = FakeLog::new(vec![page(
        vec![
            message(5, Some("engineering"), Some(4), "reply to a blank root"),
            message(4, Some("engineering"), None, "  \n "),
            message(3, Some("engineering"), Some(1), "the reply that counts"),
            message(2, Some("engineering"), Some(1), " \t "),
            message(1, Some("engineering"), None, "root"),
        ],
        None,
    )]);
    let history = project_session(&log, &query(9)).await.expect("projects");
    assert_eq!(
        history
            .iter()
            .map(|item| item.content.as_str())
            .collect::<Vec<_>>(),
        vec!["root", "the reply that counts", "reply to a blank root"]
    );
}

#[tokio::test]
async fn channel_window_counts_what_survives_narrowing_not_rows_read() {
    let mut rows = vec![message(1, Some("engineering"), None, "root")];
    rows.extend((2..=40).map(|sequence| {
        message(
            sequence,
            Some("engineering"),
            Some(1),
            &format!("reply {sequence}"),
        )
    }));
    rows.reverse();
    let log = FakeLog::new(vec![page(rows, None)]);
    let history = project_session(&log, &query(2)).await.expect("projects");
    assert_eq!(
        history
            .iter()
            .map(|item| item.content.as_str())
            .collect::<Vec<_>>(),
        vec!["root", "reply 2"]
    );
}

#[tokio::test]
async fn channel_window_keeps_the_newest_survivors() {
    let log = FakeLog::new(vec![page(
        vec![
            message(4, Some("engineering"), Some(3), "newest reply"),
            message(3, Some("engineering"), None, "newest root"),
            message(2, Some("engineering"), Some(1), "oldest reply"),
            message(1, Some("engineering"), None, "oldest root"),
        ],
        None,
    )]);
    let history = project_session(&log, &query(2)).await.expect("projects");
    assert_eq!(
        history
            .iter()
            .map(|item| item.content.as_str())
            .collect::<Vec<_>>(),
        vec!["newest root", "newest reply"]
    );
}

#[tokio::test]
async fn channel_projection_stops_reading_once_the_window_is_met() {
    let log = FakeLog::new(vec![
        page(
            vec![
                message(4, Some("engineering"), Some(3), "reply"),
                message(3, Some("engineering"), None, "root"),
            ],
            Some(3),
        ),
        page(vec![message(2, Some("engineering"), None, "older")], None),
    ]);
    assert_eq!(
        project_session(&log, &query(2))
            .await
            .expect("projects")
            .len(),
        2
    );
    assert_eq!(log.call_count(), 1);
}

/// One row of a private exchange, authored by `id` and addressed to `members`.
fn confided(
    sequence: u64,
    id: &str,
    members: &[&str],
    parent: Option<u64>,
    content: &str,
) -> LogMessage {
    LogMessage {
        parent: parent.map(Sequence),
        author: SessionAuthor::Agent {
            id: id.into(),
            label: id.into(),
        },
        audience: tinyhivemind_core::aside::Audience::Aside {
            members: members.iter().map(|member| (*member).to_owned()).collect(),
        },
        ..message(sequence, Some("engineering"), parent, content)
    }
}

fn seat(id: &str) -> SessionQuery {
    SessionQuery {
        viewer: tinyhivemind_core::aside::Viewer::Agent { id: id.into() },
        ..query(30)
    }
}

/// **A party to a confided thread reads the whole of it.**
///
/// Root-and-first-reply is written for the reader of a busy desk. Applied to
/// a seat that is *in* the conversation it withholds that seat's own exchange
/// from it: the question, the first line of the answer, and nothing else. A
/// runner that keeps its context between turns does not notice -- the whole
/// exchange reached it once in its brief -- but one that rebuilds a seat's
/// history from the log every turn has only this projection, and the rest of
/// the conversation is simply gone.
#[tokio::test]
async fn a_party_to_a_confided_thread_reads_every_reply() {
    // A fresh log per viewer: `FakeLog` replays its pages once.
    let exchange = || {
        FakeLog::new(vec![page(
            vec![
                confided(5, "grace", &["ada"], Some(2), "and the second half"),
                confided(4, "grace", &["ada"], Some(2), "the first half"),
                confided(
                    2,
                    "ada",
                    &["grace"],
                    None,
                    "between us: what constrains it?",
                ),
            ],
            None,
        )])
    };

    for party in ["ada", "grace"] {
        let log = exchange();
        let history = project_session(&log, &seat(party)).await.expect("projects");
        assert_eq!(
            history
                .iter()
                .map(|item| item.content.as_str())
                .collect::<Vec<_>>(),
            vec![
                "between us: what constrains it?",
                "the first half",
                "and the second half"
            ],
            "{party} is in this conversation and reads all of it"
        );
    }
}

/// And nobody else does. A third seat reads the opening it was never part of
/// as a stub, exactly as before: widening the rule for a party must not widen
/// it for a stranger.
#[tokio::test]
async fn a_seat_outside_a_confided_thread_still_reads_none_of_it() {
    let rows = vec![
        confided(5, "grace", &["ada"], Some(2), "and the second half"),
        confided(4, "grace", &["ada"], Some(2), "the first half"),
        confided(
            2,
            "ada",
            &["grace"],
            None,
            "between us: what constrains it?",
        ),
    ];
    let log = FakeLog::new(vec![page(rows, None)]);
    let history = project_session(&log, &seat("linus"))
        .await
        .expect("projects");
    assert!(
        history.iter().all(|item| item.readable().is_none()),
        "content reached a seat outside the conversation: {history:?}"
    );
}

/// **An ordinary desk thread is unchanged.** Every viewer is admitted to a
/// desk row, so a rule keyed on admission alone would keep every reply for
/// everyone -- one level flattened, which is the leak this narrowing exists
/// to close. The exception is keyed on the root being an aside, so a desk
/// thread still gives up its root and one reply.
#[tokio::test]
async fn an_open_desk_thread_still_gives_up_only_its_first_reply() {
    let log = FakeLog::new(vec![page(
        vec![
            message(5, Some("engineering"), Some(2), "second reply"),
            message(4, Some("engineering"), Some(2), "first reply"),
            message(2, Some("engineering"), None, "channel"),
        ],
        None,
    )]);
    let history = project_session(&log, &seat("ada")).await.expect("projects");
    assert_eq!(
        history
            .iter()
            .map(|item| item.content.as_str())
            .collect::<Vec<_>>(),
        vec!["channel", "first reply"]
    );
}
