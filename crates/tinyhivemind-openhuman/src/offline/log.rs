//! An in-memory journal that is a real [`SessionLog`].
//!
//! The host owns the log; this is the smallest host log that obeys the
//! port's contract, so an offline run and a test read it through exactly the
//! projection a live host's journal is read through. Two rules decide who may
//! read a row, and they are the ones a host follows too:
//!
//! - A desk row with `only_for` reaches its author and that one seat.
//! - A row in a conversation reaches the conversation's two seats: the author
//!   of the ask row it hangs off, and the seat that ask was for.

use std::sync::{Mutex, PoisonError};

use tinyhivemind::aside::Audience;
use tinyhivemind::{LogMessage, Sequence, SessionAuthor, SessionFuture, SessionLog, SessionPage};

/// One row of the journal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Row {
    /// The sequence the journal gave it.
    pub sequence: Sequence,
    /// Who it is attributed to: a seat id, `operator`, or `desk`.
    pub author: String,
    /// What it says, as rendered.
    pub body: String,
    /// The conversation it is in, by its ask row, or `None` on the desk.
    pub thread: Option<Sequence>,
    /// On the desk, the one seat it reaches.
    pub only_for: Option<String>,
}

/// An append-only journal for one desk, held in memory.
#[derive(Debug)]
pub struct MemoryLog {
    desk: String,
    rows: Mutex<Vec<Row>>,
}

impl MemoryLog {
    /// An empty journal for `desk`.
    #[must_use]
    pub fn new(desk: impl Into<String>) -> Self {
        Self {
            desk: desk.into(),
            rows: Mutex::new(Vec::new()),
        }
    }

    fn rows(&self) -> std::sync::MutexGuard<'_, Vec<Row>> {
        self.rows.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Append a row and return the sequence it was given.
    pub fn append(
        &self,
        author: &str,
        body: &str,
        thread: Option<Sequence>,
        only_for: Option<&str>,
    ) -> Sequence {
        let mut rows = self.rows();
        let sequence = Sequence(rows.last().map_or(0, |row| row.sequence.0) + 1);
        rows.push(Row {
            sequence,
            author: author.to_owned(),
            body: body.to_owned(),
            thread,
            only_for: only_for.map(str::to_owned),
        });
        sequence
    }

    /// The newest sequence, or zero for an empty journal.
    #[must_use]
    pub fn latest(&self) -> Sequence {
        self.rows().last().map_or(Sequence(0), |row| row.sequence)
    }

    /// What `seat` may read on the open desk above `after`, rendered.
    #[must_use]
    pub fn desk_since(&self, seat: &str, after: Sequence) -> Vec<String> {
        self.rows()
            .iter()
            .filter(|row| row.sequence > after && row.thread.is_none())
            .filter(|row| {
                row.only_for
                    .as_deref()
                    .is_none_or(|only| only == seat || row.author == seat)
            })
            .map(render)
            .collect()
    }

    /// One conversation whole, rendered: the ask that rooted it and every row
    /// in it.
    #[must_use]
    pub fn thread(&self, root: Sequence) -> Vec<String> {
        self.thread_since(root, Sequence(0))
    }

    /// One conversation above `after`, rendered.
    #[must_use]
    pub fn thread_since(&self, root: Sequence, after: Sequence) -> Vec<String> {
        self.rows()
            .iter()
            .filter(|row| row.sequence > after)
            .filter(|row| row.sequence == root || row.thread == Some(root))
            .map(render)
            .collect()
    }

    /// Every row, in order.
    #[must_use]
    pub fn all(&self) -> Vec<Row> {
        self.rows().clone()
    }

    /// Who may read `row`, by the two rules in the module docs.
    fn audience(rows: &[Row], row: &Row) -> Audience {
        let addressed: Vec<String> = match row.thread {
            Some(root) => rows
                .iter()
                .find(|candidate| candidate.sequence == root)
                .map(|ask| {
                    std::iter::once(ask.author.clone())
                        .chain(ask.only_for.clone())
                        .collect()
                })
                .unwrap_or_default(),
            None => row.only_for.clone().into_iter().collect(),
        };
        let mut members: Vec<String> = Vec::new();
        for id in addressed {
            if id != row.author && !members.contains(&id) {
                members.push(id);
            }
        }
        if members.is_empty() && row.thread.is_none() && row.only_for.is_none() {
            Audience::Desk
        } else {
            Audience::Aside { members }
        }
    }

    fn author(id: &str) -> SessionAuthor {
        match id {
            "operator" => SessionAuthor::Operator,
            "desk" => SessionAuthor::System {
                kind: "desk".into(),
                label: "desk".into(),
            },
            seat => SessionAuthor::Agent {
                id: seat.into(),
                label: seat.into(),
            },
        }
    }
}

/// `@author: body`, as every reader of this journal sees a row.
fn render(row: &Row) -> String {
    format!("@{}: {}", row.author, row.body)
}

impl SessionLog for MemoryLog {
    fn read_before(&self, before: Option<Sequence>, limit: usize) -> SessionFuture<'_> {
        let rows = self.all();
        let older: Vec<&Row> = rows
            .iter()
            .rev()
            .filter(|row| before.is_none_or(|bound| row.sequence < bound))
            .collect();
        let taken = &older[..older.len().min(limit)];
        let next_before = if older.len() > taken.len() {
            taken.last().map(|row| row.sequence)
        } else {
            None
        };
        let messages = taken
            .iter()
            .map(|row| LogMessage {
                sequence: row.sequence,
                chat_id: Some(self.desk.clone()),
                parent: row.thread,
                author: Self::author(&row.author),
                content: row.body.clone(),
                audience: Self::audience(&rows, row),
            })
            .collect();
        Box::pin(async move {
            Ok(SessionPage {
                messages,
                next_before,
            })
        })
    }
}
