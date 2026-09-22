//! What a seat's *other* conversations hold, for the turn it is taking in
//! this one.

#[cfg(test)]
mod test;

mod types;

pub use types::{Elsewhere, ElsewhereQuery};

use crate::aside::Viewer;
use crate::{
    Conversation, Result, SessionAuthor, SessionLog, SessionMessage, SessionQuery, project_session,
};

/// Read the newest rows of every conversation in `query.conversations`
/// except the one the turn is in, as `seat` reads them.
///
/// A turn is shown its own channel by whoever runs it. This is the rest of
/// what the seat would know if it were reading: one page per conversation,
/// projected as the seat, so a row it was not addressed on is withheld
/// exactly as it is everywhere else. Nothing here is work -- it is context,
/// and a caller that renders it says so.
///
/// `before` bounds every read, so a set of conversations read for one turn
/// is read as of one moment rather than drifting row by row. A conversation
/// the seat may read nothing of is returned with no rows rather than
/// dropped: a caller that lists its channels gets the same list back.
///
/// # Errors
///
/// Returns [`Error::Read`](crate::Error::Read) for a host read failure, or a
/// page-validation error when a host breaks the port's contract.
pub async fn gather_elsewhere(
    log: &dyn SessionLog,
    query: &ElsewhereQuery<'_>,
) -> Result<Vec<Elsewhere>> {
    let mut gathered = Vec::new();
    for conversation in query.conversations {
        if same_conversation(conversation, query.current) {
            continue;
        }
        let rows = project_session(
            log,
            &SessionQuery {
                conversation: conversation.clone(),
                viewer: Viewer::Agent {
                    id: query.seat.to_owned(),
                },
                before: query.before,
                window: query.window,
            },
        )
        .await?;
        gathered.push(Elsewhere {
            conversation: conversation.clone(),
            rows,
        });
    }
    Ok(gathered)
}

/// Whether two conversations are the same desk and the same thread.
fn same_conversation(one: &Conversation, other: Option<&Conversation>) -> bool {
    other.is_some_and(|other| one.desk_id == other.desk_id && one.thread_root == other.thread_root)
}

/// One row as a model reads it: `@author: content`, or nothing for a row
/// withheld from the viewer the projection ran as.
///
/// Every caller that puts rows in front of a model renders them this way,
/// so the rows a seat reads from elsewhere look like the rows it reads
/// here.
#[must_use]
pub fn render_row(row: &SessionMessage) -> Option<String> {
    let content = row.readable()?;
    let author = match &row.author {
        SessionAuthor::Operator => "operator",
        SessionAuthor::Agent { label, .. }
        | SessionAuthor::Person { label, .. }
        | SessionAuthor::System { label, .. } => label,
    };
    Some(format!("@{author}: {content}"))
}
