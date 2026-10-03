//! Shared fixtures for the session projection tests: a scriptable
//! [`SessionLog`], and the small builders every submodule uses to construct a
//! [`Conversation`], a [`SessionQuery`], a raw [`LogMessage`], and a
//! [`SessionPage`] without repeating their field lists.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::super::*;
use crate::aside::Audience;
use crate::aside::Viewer;
use std::{
    collections::VecDeque,
    io,
    sync::{Arc, Mutex},
};

/// Assert that `value` serializes to exactly `expected` and back again.
///
/// Used to pin a payload's wire shape: a host and this crate disagreeing about
/// a field name would otherwise fail only at runtime, on a live decode.
pub(super) fn assert_wire_round_trip<T>(value: &T, expected: serde_json::Value)
where
    T: serde::Serialize + serde::de::DeserializeOwned + Eq + std::fmt::Debug,
{
    assert_eq!(serde_json::to_value(value).expect("serializes"), expected);
    assert_eq!(
        serde_json::from_value::<T>(expected).expect("deserializes"),
        *value
    );
}

type Calls = Arc<Mutex<Vec<(Option<Sequence>, usize)>>>;

/// A scripted [`SessionLog`] that replays a fixed queue of pages or errors.
///
/// Each call to [`SessionLog::read_before`] pops the next queued result and
/// records the `(before, limit)` it was called with, so a test can assert on
/// how many reads a projection performed and what it asked for.
#[derive(Debug)]
pub(super) struct FakeLog {
    pub(super) pages: Mutex<VecDeque<std::result::Result<SessionPage, SourceError>>>,
    pub(super) calls: Calls,
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

    /// How many times [`SessionLog::read_before`] has been called so far.
    pub(super) fn call_count(&self) -> usize {
        self.calls.lock().expect("calls lock is not poisoned").len()
    }
}

impl SessionLog for FakeLog {
    fn read_before(&self, before: Option<Sequence>, limit: usize) -> SessionFuture<'_> {
        self.calls
            .lock()
            .expect("calls lock is not poisoned")
            .push((before, limit));
        Box::pin(async move {
            self.pages
                .lock()
                .expect("pages lock is not poisoned")
                .pop_front()
                .unwrap_or_else(|| Ok(SessionPage::default()))
        })
    }
}

/// A named "engineering" desk with no thread scope.
pub(super) fn conversation() -> Conversation {
    Conversation {
        desk_id: "engineering".into(),
        desk_name: "Engineering".into(),
        thread_root: None,
    }
}

/// A channel-level query over [`conversation`] as the operator, with `window`.
pub(super) fn query(window: usize) -> SessionQuery {
    SessionQuery {
        conversation: conversation(),
        before: None,
        window,
        viewer: Viewer::Operator,
    }
}

/// A raw agent-authored row at `sequence`, addressed to `chat` and `parent`.
pub(super) fn message(
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
