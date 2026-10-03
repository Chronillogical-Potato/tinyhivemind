//! What a seat reads of its other conversations: every one but the turn's,
//! narrowed to the seat, bounded by one moment.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::sync::Mutex;

use super::{Elsewhere, ElsewhereQuery, gather_elsewhere, render_row};
use crate::aside::Audience;
use crate::{
    Conversation, Error, LogMessage, Sequence, SessionAuthor, SessionFuture, SessionLog,
    SessionPage,
};

/// A journal of rows across several desks, read newest-first.
struct Rows(Vec<LogMessage>, Mutex<usize>);

impl Rows {
    fn new(rows: Vec<LogMessage>) -> Self {
        Self(rows, Mutex::new(0))
    }

    fn reads(&self) -> usize {
        *self.1.lock().expect("reads")
    }
}

impl SessionLog for Rows {
    fn read_before(&self, before: Option<Sequence>, limit: usize) -> SessionFuture<'_> {
        *self.1.lock().expect("reads") += 1;
        let mut older: Vec<LogMessage> = self
            .0
            .iter()
            .filter(|row| before.is_none_or(|bound| row.sequence < bound))
            .cloned()
            .collect();
        older.sort_by_key(|row| std::cmp::Reverse(row.sequence));
        let taken: Vec<LogMessage> = older.iter().take(limit).cloned().collect();
        let next_before = (older.len() > taken.len())
            .then(|| taken.last().map(|row| row.sequence))
            .flatten();
        Box::pin(async move {
            Ok(SessionPage {
                messages: taken,
                next_before,
            })
        })
    }
}

/// A log whose only read fails.
struct Broken;

impl SessionLog for Broken {
    fn read_before(&self, _before: Option<Sequence>, _limit: usize) -> SessionFuture<'_> {
        Box::pin(async { Err(Box::new(std::io::Error::other("offline")) as crate::SourceError) })
    }
}

fn agent(id: &str) -> SessionAuthor {
    SessionAuthor::Agent {
        id: id.to_owned(),
        label: id.to_owned(),
    }
}

fn row(sequence: u64, desk: &str, author: &str, content: &str, audience: Audience) -> LogMessage {
    LogMessage {
        sequence: Sequence(sequence),
        chat_id: Some(desk.to_owned()),
        parent: None,
        author: agent(author),
        content: content.to_owned(),
        audience,
    }
}

fn desk(id: &str) -> Conversation {
    Conversation {
        desk_id: id.to_owned(),
        desk_name: id.to_owned(),
        thread_root: None,
    }
}

fn run<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime")
        .block_on(future)
}

fn log() -> Rows {
    Rows::new(vec![
        row(1, "engineering", "one", "the deploy is out", Audience::Desk),
        row(2, "marketing", "three", "launch is friday", Audience::Desk),
        row(
            3,
            "marketing",
            "three",
            "budget is between us",
            Audience::Aside {
                members: vec!["four".to_owned()],
            },
        ),
        row(4, "legal", "five", "cleared", Audience::Desk),
        row(5, "marketing", "four", "noted", Audience::Desk),
    ])
}

#[test]
fn every_conversation_but_the_turn_is_read_as_the_seat() {
    let log = log();
    let channels = [desk("engineering"), desk("marketing"), desk("legal")];
    let here = desk("engineering");
    let gathered = run(gather_elsewhere(
        &log,
        &ElsewhereQuery {
            seat: "one",
            conversations: &channels,
            current: Some(&here),
            before: None,
            window: 30,
        },
    ))
    .expect("reads");
    let desks: Vec<&str> = gathered
        .iter()
        .map(|found| found.conversation.desk_id.as_str())
        .collect();
    assert_eq!(
        desks,
        ["marketing", "legal"],
        "the turn's own desk is skipped"
    );
    let rendered: Vec<String> = gathered[0].rows.iter().filter_map(render_row).collect();
    assert_eq!(
        rendered,
        ["@three: launch is friday", "@four: noted"],
        "the aside to four is withheld from one"
    );
    // Read as four, the aside is there.
    let theirs = run(gather_elsewhere(
        &log,
        &ElsewhereQuery {
            seat: "four",
            conversations: &channels,
            current: Some(&here),
            before: None,
            window: 30,
        },
    ))
    .expect("reads");
    let rendered: Vec<String> = theirs[0].rows.iter().filter_map(render_row).collect();
    assert!(
        rendered.contains(&"@three: budget is between us".to_owned()),
        "{rendered:?}"
    );
}

#[test]
fn a_bound_holds_every_conversation_to_one_moment_and_no_current_reads_them_all() {
    let log = log();
    let channels = [desk("marketing"), desk("legal")];
    let gathered = run(gather_elsewhere(
        &log,
        &ElsewhereQuery {
            seat: "one",
            conversations: &channels,
            current: None,
            before: Some(Sequence(4)),
            window: 30,
        },
    ))
    .expect("reads");
    assert_eq!(gathered.len(), 2, "nothing is skipped without a current");
    let rendered: Vec<String> = gathered[0].rows.iter().filter_map(render_row).collect();
    assert_eq!(
        rendered,
        ["@three: launch is friday"],
        "row 5 is above the bound"
    );
    assert!(
        gathered[1].rows.is_empty(),
        "legal's only row is at the bound, which is exclusive"
    );
    assert_eq!(
        gathered[1],
        Elsewhere {
            conversation: desk("legal"),
            rows: Vec::new(),
        },
        "a conversation with nothing to show is still listed"
    );
}

#[test]
fn a_thread_is_its_own_conversation_and_a_failed_read_is_reported() {
    let log = Rows::new(vec![
        LogMessage {
            sequence: Sequence(1),
            chat_id: Some("engineering".into()),
            parent: None,
            author: agent("one"),
            content: "which port?".into(),
            audience: Audience::Aside {
                members: vec!["two".to_owned()],
            },
        },
        LogMessage {
            sequence: Sequence(2),
            chat_id: Some("engineering".into()),
            parent: Some(Sequence(1)),
            author: agent("two"),
            content: "8080".into(),
            audience: Audience::Aside {
                members: vec!["two".to_owned()],
            },
        },
    ]);
    let thread = Conversation {
        thread_root: Some(Sequence(1)),
        ..desk("engineering")
    };
    let channels = [desk("engineering"), thread.clone()];
    let gathered = run(gather_elsewhere(
        &log,
        &ElsewhereQuery {
            seat: "two",
            conversations: &channels,
            current: Some(&thread),
            before: None,
            window: 30,
        },
    ))
    .expect("reads");
    assert_eq!(
        gathered.len(),
        1,
        "the thread it is in is skipped, the desk is not"
    );
    assert_eq!(gathered[0].conversation.thread_root, None);
    assert!(log.reads() >= 1);
    assert!(format!("{gathered:?}").contains("engineering"));

    let broken = run(gather_elsewhere(
        &Broken,
        &ElsewhereQuery {
            seat: "one",
            conversations: &channels,
            current: None,
            before: None,
            window: 30,
        },
    ));
    assert!(matches!(broken, Err(Error::Read { .. })), "{broken:?}");
}
