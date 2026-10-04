//! An in-memory `SessionLog`: the host-owned journal the examples read through.
//!
//! Core ships the port and no implementation, so every host and every test
//! writes one. This is the lab's, kept small: append, then page newest-first.

use tinyhivemind_core::runtime::aside::Audience;
use tinyhivemind_core::runtime::{
    LogMessage, Sequence, SessionAuthor, SessionFuture, SessionLog, SessionPage,
};

/// An append-only vector of rows, sequenced from 1.
#[derive(Debug, Default)]
pub struct MemoryLog {
    rows: Vec<LogMessage>,
}

/// An agent author.
#[must_use]
pub fn agent(id: &str) -> SessionAuthor {
    SessionAuthor::Agent {
        id: id.into(),
        label: id.into(),
    }
}

/// A person author.
#[must_use]
pub fn person(id: &str) -> SessionAuthor {
    SessionAuthor::Person {
        id: id.into(),
        label: id.into(),
    }
}

impl MemoryLog {
    /// Append a desk-visible row to `chat` and return its sequence.
    pub fn say(&mut self, chat: &str, author: SessionAuthor, content: &str) -> Sequence {
        self.append(chat, None, author, content, Audience::Desk)
    }

    /// Append a reply inside the thread rooted at `root`.
    pub fn reply(
        &mut self,
        chat: &str,
        root: Sequence,
        author: SessionAuthor,
        content: &str,
    ) -> Sequence {
        self.append(chat, Some(root), author, content, Audience::Desk)
    }

    /// Append a row with an explicit audience and parent.
    pub fn append(
        &mut self,
        chat: &str,
        parent: Option<Sequence>,
        author: SessionAuthor,
        content: &str,
        audience: Audience,
    ) -> Sequence {
        let sequence = Sequence(self.rows.len() as u64 + 1);
        self.rows.push(LogMessage {
            sequence,
            chat_id: Some(chat.into()),
            parent,
            author,
            content: content.into(),
            audience,
        });
        sequence
    }

    /// The newest sequence, or zero for an empty log.
    #[must_use]
    pub fn head(&self) -> Sequence {
        Sequence(self.rows.len() as u64)
    }

    /// Every row, oldest first.
    #[must_use]
    pub fn rows(&self) -> &[LogMessage] {
        &self.rows
    }

    /// Rows of one chat, oldest first.
    #[must_use]
    pub fn chat(&self, chat: &str) -> Vec<LogMessage> {
        self.rows
            .iter()
            .filter(|row| row.chat_id.as_deref() == Some(chat))
            .cloned()
            .collect()
    }
}

impl SessionLog for MemoryLog {
    fn read_before(&self, before: Option<Sequence>, limit: usize) -> SessionFuture<'_> {
        let older: Vec<&LogMessage> = self
            .rows
            .iter()
            .rev()
            .filter(|row| before.is_none_or(|bound| row.sequence < bound))
            .collect();
        let messages: Vec<LogMessage> = older.iter().take(limit).map(|row| (*row).clone()).collect();
        let next_before = if older.len() > messages.len() {
            messages.last().map(|row| row.sequence)
        } else {
            None
        };
        Box::pin(async move {
            Ok(SessionPage {
                messages,
                next_before,
            })
        })
    }
}
