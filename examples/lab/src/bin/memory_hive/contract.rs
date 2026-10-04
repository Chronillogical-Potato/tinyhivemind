//! A host log that breaks the `SessionLog` contract, and the typed error each
//! breach earns.

use std::sync::atomic::{AtomicUsize, Ordering};

use tinyhivemind_core::aside::{Audience, Viewer};
use tinyhivemind_core::runtime::{
    Conversation, LogMessage, Sequence, SessionAuthor, SessionFuture, SessionLog, SessionPage,
    SessionQuery, project_session,
};
use tinyhivemind_lab::{Res, block_on, section};

use crate::eng;

#[derive(Clone, Copy)]
enum Breach {
    TooLarge,
    EmptyWithCursor,
    DuplicateRow,
    NotDescending,
    CursorBeyondOldest,
    RowAtOrAboveBound,
    CursorStuck,
    ReadFails,
}

struct Misbehaving {
    breach: Breach,
    calls: AtomicUsize,
}

fn row(sequence: u64) -> LogMessage {
    LogMessage {
        sequence: Sequence(sequence),
        chat_id: Some("eng".into()),
        parent: None,
        author: SessionAuthor::Operator,
        content: format!("row {sequence}"),
        audience: Audience::Desk,
    }
}

impl SessionLog for Misbehaving {
    fn read_before(&self, _before: Option<Sequence>, limit: usize) -> SessionFuture<'_> {
        let call = self.calls.fetch_add(1, Ordering::Relaxed);
        let page = |rows: &[u64], next: Option<u64>| {
            Ok(SessionPage {
                messages: rows.iter().copied().map(row).collect(),
                next_before: next.map(Sequence),
            })
        };
        let result = match (self.breach, call) {
            (Breach::TooLarge, _) => Ok(SessionPage {
                messages: (1..=(limit as u64 + 1)).rev().map(row).collect(),
                next_before: None,
            }),
            (Breach::EmptyWithCursor, _) => page(&[], Some(5)),
            (Breach::DuplicateRow, _) => page(&[7, 7], None),
            (Breach::NotDescending, _) => page(&[5, 6], None),
            (Breach::CursorBeyondOldest, _) => page(&[10, 9], Some(11)),
            (Breach::RowAtOrAboveBound, 0) => page(&[10, 9], Some(9)),
            (Breach::RowAtOrAboveBound, _) => page(&[12], None),
            (Breach::CursorStuck, 0) => page(&[10, 9], Some(9)),
            (Breach::CursorStuck, _) => page(&[8], Some(9)),
            (Breach::ReadFails, _) => Err("the journal is unreachable".into()),
        };
        Box::pin(async move { result })
    }
}

pub fn run() -> Res {
    section("SessionLog contract: each breach is a typed error");
    let conv: Conversation = eng();
    let query = SessionQuery {
        conversation: conv,
        viewer: Viewer::Operator,
        before: None,
        window: 30,
    };
    for (label, breach) in [
        ("more rows than asked for", Breach::TooLarge),
        ("an empty page with a cursor", Breach::EmptyWithCursor),
        ("the same sequence twice", Breach::DuplicateRow),
        ("a page that ascends", Breach::NotDescending),
        (
            "a cursor newer than the oldest row",
            Breach::CursorBeyondOldest,
        ),
        ("a row at the exclusive bound", Breach::RowAtOrAboveBound),
        ("a cursor that does not advance", Breach::CursorStuck),
        ("a read that fails", Breach::ReadFails),
    ] {
        let log = Misbehaving {
            breach,
            calls: AtomicUsize::new(0),
        };
        let outcome = block_on(project_session(&log, &query));
        println!(
            "  {label:<36} {}",
            outcome.err().map_or("accepted".into(), |e| {
                match std::error::Error::source(&e) {
                    Some(cause) => format!("{e} (source: {cause})"),
                    None => e.to_string(),
                }
            })
        );
    }
    Ok(())
}
