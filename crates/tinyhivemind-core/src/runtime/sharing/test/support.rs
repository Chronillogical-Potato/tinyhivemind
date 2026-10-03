//! Shared fixtures for the sharing tests: a scriptable `SessionLog`, small
//! conversation/row/page builders, and the `plan`/`delta` helpers every
//! submodule uses to drive `prepare_delta` and unwrap its happy path.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::super::*;
use crate::aside::Audience;
use crate::aside::Viewer;
use crate::runtime::{
    Conversation, LogMessage, SessionAuthor, SessionFuture, SessionPage, SourceError,
};
use std::{
    collections::VecDeque,
    io,
    sync::{Arc, Mutex},
};

/// A scripted `SessionLog` that replays a fixed queue of pages or errors and
/// counts how many times it was read.
#[derive(Debug)]
pub(super) struct FakeLog {
    pages: Mutex<VecDeque<std::result::Result<SessionPage, SourceError>>>,
    calls: Arc<Mutex<usize>>,
}

impl FakeLog {
    /// A log that replays `pages` in order, then an empty default page.
    pub(super) fn new(pages: Vec<SessionPage>) -> Self {
        Self {
            pages: Mutex::new(pages.into_iter().map(Ok).collect()),
            calls: Arc::default(),
        }
    }

    /// A log whose first (and only) read fails with a source error.
    pub(super) fn failing() -> Self {
        Self {
            pages: Mutex::new(VecDeque::from([Err(
                Box::new(io::Error::other("offline")) as SourceError
            )])),
            calls: Arc::default(),
        }
    }

    /// How many times this log has been read.
    pub(super) fn calls(&self) -> usize {
        *self.calls.lock().expect("calls lock")
    }
}

impl SessionLog for FakeLog {
    fn read_before(&self, _: Option<Sequence>, _: usize) -> SessionFuture<'_> {
        *self.calls.lock().expect("calls lock") += 1;
        Box::pin(async move {
            self.pages
                .lock()
                .expect("pages lock")
                .pop_front()
                .unwrap_or_else(|| Ok(SessionPage::default()))
        })
    }
}

/// A conversation with the given id, name, and optional thread root.
pub(super) fn conversation(id: &str, name: &str, root: Option<u64>) -> Conversation {
    Conversation {
        desk_id: id.into(),
        desk_name: name.into(),
        thread_root: root.map(Sequence),
    }
}

/// The default "engineering" desk with no thread scope.
pub(super) fn engineering() -> Conversation {
    conversation("engineering", "Engineering", None)
}

/// A raw agent-authored row at `sequence`, addressed to `chat` and `parent`.
pub(super) fn raw(
    sequence: u64,
    chat: Option<&str>,
    parent: Option<u64>,
    content: &str,
) -> LogMessage {
    LogMessage {
        sequence: Sequence(sequence),
        chat_id: chat.map(str::to_owned),
        parent: parent.map(Sequence),
        author: SessionAuthor::Agent {
            id: format!("agent-{sequence}"),
            label: format!("Agent {sequence}"),
        },
        content: content.into(),
        audience: Audience::Desk,
    }
}

/// A [`SessionPage`] wrapping `messages`, with `next` as its cursor.
pub(super) fn page(messages: Vec<LogMessage>, next: Option<u64>) -> SessionPage {
    SessionPage {
        messages,
        next_before: next.map(Sequence),
    }
}

/// Sharing state for the default desk at the given watermark.
pub(super) fn state(watermark: u64) -> SharingState {
    initialized_state(engineering(), Sequence(watermark))
}

/// Drive `prepare_delta` for the default desk as the operator.
pub(super) async fn plan(log: &FakeLog, state: &SharingState, before: u64) -> Result<SharingPlan> {
    let desired = engineering();
    let current = engineering();
    prepare_delta(
        log,
        &SharingQuery {
            desired_conversation: &desired,
            current_conversation: &current,
            state,
            before: Sequence(before),
            viewer: &Viewer::Operator,
        },
    )
    .await
}

/// Unwrap a [`SharingPlan`], panicking if it asked for reinitialization.
pub(super) fn delta(plan: SharingPlan) -> SessionDelta {
    match plan {
        SharingPlan::Delta(delta) => delta,
        SharingPlan::Reinitialize { reason } => panic!("unexpected reinitialize: {reason:?}"),
    }
}
