//! A seat's history, read from the host's log as that seat.

use tinyhivemind::aside::Viewer;
use tinyhivemind::{
    Conversation, Sequence, SessionAuthor, SessionLog, SessionMessage, SessionQuery,
    project_session,
};

use crate::Result;

/// What `seat` was shown in `conversation` up to and including `since`,
/// newest `window` rows, as chronological `(role, content)` pairs; nothing
/// for a seat shown nothing yet.
///
/// Projected as the seat, so a row it was not addressed on is withheld the
/// same way it is everywhere else the host's log is read. The seat's own
/// rows are its turns; everyone else's are attributed messages to it.
///
/// Nothing above `since` is read. The rows above it are the turn's new rows,
/// which reach the seat in its brief, so between the two it sees every row
/// once, and a row a peer wrote in the same wave -- above `since`, since the
/// watermark was fixed before the wave ran -- reaches it through neither.
///
/// # Errors
///
/// The host's log failing to read, or breaking the port's contract.
pub(super) async fn history(
    log: &dyn SessionLog,
    conversation: Conversation,
    seat: &str,
    since: Option<Sequence>,
    window: usize,
) -> Result<Vec<(String, String)>> {
    let query = SessionQuery {
        conversation,
        viewer: Viewer::Agent { id: seat.into() },
        // Exclusive, so one above `since`; nothing is above the last
        // sequence, so that reads unbounded rather than one short; and
        // nothing is below the first, so a seat shown nothing reads none.
        before: match since {
            None => Some(Sequence(0)),
            Some(Sequence(u64::MAX)) => None,
            Some(since) => Some(Sequence(since.0 + 1)),
        },
        window,
    };
    let rows = project_session(log, &query).await?;
    Ok(rows.iter().filter_map(|row| turn(row, seat)).collect())
}

/// One row as the seat's model reads it, or `None` for a row withheld from
/// it.
fn turn(row: &SessionMessage, seat: &str) -> Option<(String, String)> {
    let content = row.readable()?;
    Some(match &row.author {
        SessionAuthor::Agent { id, .. } if id == seat => ("assistant".into(), content.into()),
        SessionAuthor::Agent { label, .. }
        | SessionAuthor::Person { label, .. }
        | SessionAuthor::System { label, .. } => ("user".into(), format!("@{label}: {content}")),
        SessionAuthor::Operator => ("user".into(), format!("@operator: {content}")),
    })
}
