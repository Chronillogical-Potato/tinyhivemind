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
/// rows are its turns; everyone else's are attributed messages to it, a
/// seat by the name `names` gives it and otherwise by its label.
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
    names: &(dyn Fn(&str) -> String + Sync),
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
    // Marked only for a desk seed, and for the same reason the brief marks
    // its rows: on the desk a row confided to this seat sits beside the
    // room's own. Inside a conversation every row is private and the turn
    // knows it. Without this a hosted seat reads one row two ways -- marked
    // while it is new, bare once it is remembered -- and the durable copy is
    // the bare one.
    let mark_aside = query.conversation.thread_root.is_none();
    let rows = project_session(log, &query).await?;
    Ok(rows
        .iter()
        .filter_map(|row| turn(row, seat, mark_aside, names))
        .collect())
}

/// One row as the seat's model reads it, or `None` for a row withheld from
/// it.
fn turn(
    row: &SessionMessage,
    seat: &str,
    mark_aside: bool,
    names: &(dyn Fn(&str) -> String + Sync),
) -> Option<(String, String)> {
    let content = row.readable()?;
    // The seat's own rows are its turns, and a model needs no telling that
    // it spoke in confidence; only what reaches it from someone else does.
    let confided =
        mark_aside && matches!(row.audience, tinyhivemind::aside::Audience::Aside { .. });
    let said = |author: &str| {
        if confided {
            format!("{author} (privately): {content}")
        } else {
            format!("{author}: {content}")
        }
    };
    Some(match &row.author {
        SessionAuthor::Agent { id, .. } if id == seat => ("assistant".into(), content.into()),
        SessionAuthor::Agent { id, label } => {
            let name = names(id);
            let author = if name.trim().is_empty() || name == *id {
                format!("@{label}")
            } else {
                name
            };
            ("user".into(), said(&author))
        }
        SessionAuthor::Person { label, .. } | SessionAuthor::System { label, .. } => {
            ("user".into(), said(&format!("@{label}")))
        }
        SessionAuthor::Operator => ("user".into(), said("@operator")),
    })
}

/// Puts the seat's standing prompt at the head of what it was shown.
///
/// Every turn, including a seat's first. A cold turn composes the
/// definition's own system prompt *and* takes a seed -- the host runtime's
/// seeding branch matches `seed: Some(..)`, which an empty vector satisfies,
/// and what it clears is the conversation rather than the composed prompt --
/// so the two do not compete and there is nothing to guard against.
///
/// Guarding on a non-empty history, as this once did, withheld the persona
/// from the only turn with no other way to learn who it is. A seat asked a
/// question answers on its first turn and is never seen again, and what a
/// host says here is often which side of the conversation that seat is on.
#[must_use]
pub(super) fn with_persona(
    mut history: Vec<(String, String)>,
    persona: Option<String>,
) -> Vec<(String, String)> {
    if let Some(persona) = persona {
        history.insert(0, ("system".to_owned(), persona));
    }
    history
}
