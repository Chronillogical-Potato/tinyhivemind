//! `SessionContext` rendering and `initialize_session`/
//! `initialize_session_with_context` behavior: threads, pins and notes beside
//! (never merged into) history; read-failure propagation; and the stated
//! window, which restates only what a viewer with an elided aside actually
//! received.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::super::*;
use super::support::{briefing_with, named_conversation, permissive, viewer_briefing};
use crate::aside::AsidePolicy;
use crate::aside::Audience;
use crate::aside::Viewer;
use crate::runtime::{
    LogMessage, Sequence, SessionAuthor, SessionFuture, SessionPage, SessionQuery, SourceError,
};
use std::io;

#[test]
fn context_renders_pins_between_threads_and_notes() {
    let context = SessionContext {
        threads: Vec::new(),
        pins: vec![crate::runtime::Pin {
            sequence: Sequence(7),
            pinned_at: Sequence(9),
            pinned_by: SessionAuthor::Operator,
            label: Some("limits".into()),
            note: None,
            excerpt: Some("midnight UTC".into()),
        }],
        notes: vec![BriefingNote {
            heading: "Work raised in this conversation".into(),
            lines: vec!["#12 rewrite the changelog".into()],
        }],
    };
    assert!(!context.is_empty(), "{context:?}");
    assert_eq!(
        context.system_text(),
        Some(
            "Pinned in this conversation:\n\
             - [7] #limits \"midnight UTC\"\n\
             \nWork raised in this conversation:\n\
             - #12 rewrite the changelog"
                .into()
        )
    );
}

#[derive(Debug)]
struct OnePage(SessionPage);

impl SessionLog for OnePage {
    fn read_before(&self, _: Option<Sequence>, _: usize) -> SessionFuture<'_> {
        Box::pin(async { Ok(self.0.clone()) })
    }
}

#[tokio::test]
async fn initialization_keeps_briefing_separate_from_history() {
    let briefing = TeamBriefing {
        viewer_id: "alice".into(),
        desk_id: "engineering".into(),
        desk_name: "Engineering".into(),
        teammates: Vec::new(),
        brevity: BrevityPolicy::DEFAULT,
        asides: AsidePolicy::DEFAULT,
    };
    let row = LogMessage {
        sequence: Sequence(4),
        chat_id: Some("engineering".into()),
        parent: None,
        author: SessionAuthor::Operator,
        content: "hello".into(),
        audience: Audience::Desk,
    };
    let query = SessionQuery {
        conversation: named_conversation(),
        before: None,
        window: 1,
        viewer: Viewer::Operator,
    };
    let initialized = initialize_session(
        &OnePage(SessionPage {
            messages: vec![row],
            next_before: None,
        }),
        &query,
        briefing.clone(),
    )
    .await
    .expect("initializes");
    // The window the briefing states is reconciled to the query's window
    // (1), not left at whatever `BrevityPolicy::DEFAULT` (30) carried in.
    assert_eq!(initialized.briefing.brevity.window, query.window);
    assert_eq!(
        initialized.briefing.brevity.message_chars,
        briefing.brevity.message_chars
    );
    assert_eq!(initialized.briefing.viewer_id, briefing.viewer_id);
    assert_eq!(initialized.briefing.desk_id, briefing.desk_id);
    assert_eq!(initialized.briefing.desk_name, briefing.desk_name);
    assert_eq!(initialized.briefing.teammates, briefing.teammates);
    assert_eq!(initialized.history.len(), 1);
    assert_eq!(initialized.history[0].sequence, Sequence(4));
    assert!(initialized.context.is_empty(), "{:?}", initialized.context);
}

fn desk_row(sequence: u64, parent: Option<u64>, content: &str) -> LogMessage {
    LogMessage {
        sequence: Sequence(sequence),
        chat_id: Some("engineering".into()),
        parent: parent.map(Sequence),
        author: SessionAuthor::Operator,
        content: content.into(),
        audience: Audience::Desk,
    }
}

#[tokio::test]
async fn context_carries_the_thread_index_and_host_notes_beside_history() {
    let log = OnePage(SessionPage {
        messages: vec![
            desk_row(3, Some(1), "on it"),
            desk_row(2, None, "check the invoice"),
            desk_row(1, None, "draft the launch email"),
        ],
        next_before: None,
    });
    let query = SessionQuery {
        conversation: named_conversation(),
        before: None,
        window: 10,
        viewer: Viewer::Operator,
    };
    let note = BriefingNote {
        heading: "Work raised in this conversation".into(),
        lines: vec!["#12 rewrite the changelog — In review".into()],
    };
    let initialized =
        initialize_session_with_context(&log, &query, viewer_briefing(), vec![note.clone()])
            .await
            .expect("initializes");

    assert_eq!(
        initialized
            .context
            .threads
            .iter()
            .map(|line| (line.root.0, line.replies))
            .collect::<Vec<_>>(),
        vec![(1, 1), (2, 0)]
    );
    assert_eq!(initialized.context.notes, vec![note]);
    // The context is beside the history, never folded into it.
    assert_eq!(
        initialized
            .history
            .iter()
            .map(|item| item.content.as_str())
            .collect::<Vec<_>>(),
        vec!["draft the launch email", "check the invoice", "on it"]
    );
}

#[tokio::test]
async fn context_skips_the_index_inside_a_thread_and_propagates_read_failures() {
    let log = OnePage(SessionPage {
        messages: vec![desk_row(1, None, "root")],
        next_before: None,
    });
    let mut query = SessionQuery {
        conversation: named_conversation(),
        before: None,
        window: 10,
        viewer: Viewer::Operator,
    };
    query.conversation.thread_root = Some(Sequence(1));
    let initialized = initialize_session_with_context(&log, &query, viewer_briefing(), Vec::new())
        .await
        .expect("initializes");
    assert!(initialized.context.is_empty(), "{:?}", initialized.context);

    query.conversation.thread_root = None;
    assert!(matches!(
        initialize_session_with_context(&FailingLog, &query, viewer_briefing(), Vec::new()).await,
        Err(crate::runtime::Error::Read { .. })
    ));

    // The index read is a second read, and it fails on its own.
    assert!(matches!(
        initialize_session_with_context(
            &FailsAfterHistory::default(),
            &query,
            viewer_briefing(),
            Vec::new()
        )
        .await,
        Err(crate::runtime::Error::Read { .. })
    ));
}

/// Answers the history projection, then fails the thread-index read.
///
/// Both walks start at the same cursor, so only call order tells them apart.
#[derive(Debug, Default)]
struct FailsAfterHistory {
    reads: std::sync::Mutex<usize>,
}

impl SessionLog for FailsAfterHistory {
    fn read_before(&self, _: Option<Sequence>, _: usize) -> SessionFuture<'_> {
        let mut reads = self.reads.lock().expect("reads lock is not poisoned");
        *reads += 1;
        let first = *reads == 1;
        Box::pin(async move {
            if first {
                Ok(SessionPage::default())
            } else {
                Err(Box::new(io::Error::other("offline")) as SourceError)
            }
        })
    }
}

#[test]
fn context_renders_threads_and_notes_and_nothing_when_empty() {
    assert_eq!(SessionContext::default().system_text(), None);

    let context = SessionContext {
        threads: vec![
            crate::runtime::ThreadLine {
                root: Sequence(41),
                opening: "draft the launch email".into(),
                replies: 4,
                latest: Sequence(58),
                landed: Some("In review".into()),
            },
            crate::runtime::ThreadLine {
                root: Sequence(37),
                opening: "check the invoice".into(),
                replies: 1,
                latest: Sequence(39),
                landed: None,
            },
            crate::runtime::ThreadLine {
                root: Sequence(30),
                opening: "any thoughts?".into(),
                replies: 0,
                latest: Sequence(30),
                landed: None,
            },
        ],
        pins: Vec::new(),
        notes: vec![BriefingNote {
            heading: "Work raised in this conversation".into(),
            lines: vec![
                "#12 rewrite the changelog".into(),
                "#13 book the venue".into(),
            ],
        }],
    };
    assert_eq!(
        context.system_text(),
        Some(
            "Threads in this desk:\n\
             - [41] \"draft the launch email\" — 4 replies (landed: In review)\n\
             - [37] \"check the invoice\" — 1 reply\n\
             - [30] \"any thoughts?\" — no replies\n\
             \nWork raised in this conversation:\n\
             - #12 rewrite the changelog\n\
             - #13 book the venue"
                .into()
        )
    );

    let notes_only = SessionContext {
        threads: Vec::new(),
        pins: Vec::new(),
        notes: context.notes.clone(),
    };
    assert_eq!(
        notes_only.system_text(),
        Some(
            "Work raised in this conversation:\n- #12 rewrite the changelog\n- #13 book the venue"
                .into()
        )
    );
}

#[derive(Debug)]
struct FailingLog;

impl SessionLog for FailingLog {
    fn read_before(&self, _: Option<Sequence>, _: usize) -> SessionFuture<'_> {
        Box::pin(async { Err(Box::new(io::Error::other("offline")) as SourceError) })
    }
}

#[tokio::test]
async fn initialization_propagates_projection_errors() {
    let briefing = TeamBriefing {
        viewer_id: "alice".into(),
        desk_id: "engineering".into(),
        desk_name: "Engineering".into(),
        teammates: Vec::new(),
        brevity: BrevityPolicy::DEFAULT,
        asides: AsidePolicy::DEFAULT,
    };
    let query = SessionQuery {
        conversation: named_conversation(),
        before: None,
        window: 1,
        viewer: Viewer::Operator,
    };
    assert!(matches!(
        initialize_session(&FailingLog, &query, briefing).await,
        Err(crate::runtime::Error::Read { .. })
    ));
}

#[tokio::test]
async fn the_briefing_states_the_window_a_viewer_actually_received() {
    let rows = vec![
        desk_row(1, None, "in the open"),
        LogMessage {
            audience: Audience::Aside {
                members: vec!["bob".into()],
            },
            author: SessionAuthor::Agent {
                id: "carol".into(),
                label: "Carol".into(),
            },
            ..desk_row(2, None, "privately")
        },
        LogMessage {
            audience: Audience::Aside {
                members: vec!["bob".into()],
            },
            author: SessionAuthor::Agent {
                id: "carol".into(),
                label: "Carol".into(),
            },
            ..desk_row(3, None, "privately again")
        },
    ];

    // A viewer outside the aside receives two rows for a window of thirty,
    // because collapsing happens after the window is filled. Promising thirty
    // would be promising a budget this turn does not have.
    let log = OnePage(SessionPage {
        messages: rows.clone().into_iter().rev().collect(),
        next_before: None,
    });
    let narrowed = initialize_session(
        &log,
        &SessionQuery {
            conversation: named_conversation(),
            before: None,
            window: 30,
            viewer: Viewer::Agent { id: "alice".into() },
        },
        briefing_with(permissive()),
    )
    .await
    .expect("initializes");
    assert_eq!(narrowed.history.len(), 2);
    assert_eq!(narrowed.briefing.brevity.window, 2);
    assert!(narrowed.briefing.system_text().contains("about 2 messages"));

    // A member elides nothing, so it is told the window it actually has.
    let log = OnePage(SessionPage {
        messages: rows.into_iter().rev().collect(),
        next_before: None,
    });
    let full = initialize_session(
        &log,
        &SessionQuery {
            conversation: named_conversation(),
            before: None,
            window: 30,
            viewer: Viewer::Agent { id: "bob".into() },
        },
        briefing_with(permissive()),
    )
    .await
    .expect("initializes");
    assert_eq!(full.briefing.brevity.window, 30);
}

#[tokio::test]
async fn a_young_desk_still_states_the_window_it_will_grow_into() {
    // Only a projection that actually elided something is restated. A desk
    // with three messages and no aside is not told its budget is three.
    let log = OnePage(SessionPage {
        messages: vec![desk_row(1, None, "hello")],
        next_before: None,
    });
    let initialized = initialize_session(
        &log,
        &SessionQuery {
            conversation: named_conversation(),
            before: None,
            window: 30,
            viewer: Viewer::Operator,
        },
        briefing_with(permissive()),
    )
    .await
    .expect("initializes");
    assert_eq!(initialized.history.len(), 1);
    assert_eq!(initialized.briefing.brevity.window, 30);
}
