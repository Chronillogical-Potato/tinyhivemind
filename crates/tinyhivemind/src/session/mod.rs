//! Validated projection over a host-owned session log.

#[cfg(test)]
mod test;

mod types;

pub use types::{
    Conversation, Elision, LogMessage, Sequence, SessionAuthor, SessionMessage, SessionPage,
    SessionQuery,
};

use crate::{Error, Result};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    error::Error as StdError,
    future::Future,
    pin::Pin,
};
use tinyhivemind_core::{
    aside::{Audience, Viewer},
    chat::same_conversation,
};

/// Default number of qualifying messages requested for a turn.
pub const SESSION_WINDOW: usize = 30;
/// Maximum raw rows requested in one host read.
pub const PAGE_SIZE: usize = 512;
/// Maximum raw rows inspected during one projection.
pub const SCAN_LIMIT: usize = 2048;

/// A boxed source error returned by a host log implementation.
pub type SourceError = Box<dyn StdError + Send + Sync + 'static>;

/// The boxed future returned by [`SessionLog`].
pub type SessionFuture<'a> =
    Pin<Box<dyn Future<Output = std::result::Result<SessionPage, SourceError>> + Send + 'a>>;

/// Read-only access to the host's append-only session log.
///
/// Implementations must return rows newest-first and treat `before` as an
/// exclusive sequence bound. The trait is object-safe and chooses no executor.
pub trait SessionLog: Send + Sync {
    /// Read at most `limit` raw rows older than `before`.
    ///
    /// # Cost
    ///
    /// This is the hot primitive, and an implementation that reads forward from
    /// the head of the journal to keep the last `limit` rows costs
    /// O(total events) per call. That is not a cold-start cost: it is paid once
    /// per rebind *and* once per watermark tick, so a linear implementation is
    /// linear in the company's whole history on every turn. `opencompany`
    /// measured 72.8ms against 0.4ms at 100k events — linear against flat —
    /// before reading its journal backwards in fixed-size tail chunks.
    ///
    /// Implementations should seek from the tail and parse only the rows they
    /// return. `limit` is bounded by [`PAGE_SIZE`], so a tail read is bounded
    /// too; a forward scan is not.
    fn read_before(&self, before: Option<Sequence>, limit: usize) -> SessionFuture<'_>;
}

/// Project one bounded, attributed history from a host log.
///
/// Reaching [`SCAN_LIMIT`] is a successful partial projection. The returned
/// messages are chronological even though the port pages newest-first.
///
/// A thread-scoped query returns the root and its direct replies. A
/// channel-level query returns every root **and each root's first reply**: an
/// agent reading a desk sees the answers, not a run of unanswered questions,
/// and still never reads a second thread's interior. See
/// [`docs/specs/thread-scoped-conversations.md`][spec].
///
/// [spec]: https://github.com/tinyhumansai/tinyhivemind/blob/main/docs/specs/thread-scoped-conversations.md
///
/// # Errors
///
/// Returns [`Error::Read`] for a host read failure or a typed page-validation
/// error when a host violates ordering, uniqueness, size, or cursor contracts.
pub async fn project_session(
    log: &(dyn SessionLog + '_),
    query: &SessionQuery,
) -> Result<Vec<SessionMessage>> {
    if query.window == 0 {
        return Ok(Vec::new());
    }
    let (projected, settlements) = match query.conversation.thread_root {
        Some(_) => project_thread(log, query).await?,
        None => project_channel(log, query).await?,
    };
    Ok(collapse_elisions(projected, &settlements))
}

/// The agent id a row is attributed to, or `None` for any other author.
pub(crate) fn author_agent_id(author: &SessionAuthor) -> Option<&str> {
    match author {
        SessionAuthor::Agent { id, .. } => Some(id.as_str()),
        SessionAuthor::Operator | SessionAuthor::Person { .. } | SessionAuthor::System { .. } => {
            None
        }
    }
}

/// Whether `viewer` may read this row's content.
pub(crate) fn admits(message: &LogMessage, viewer: &Viewer) -> bool {
    message
        .audience
        .admits(viewer, author_agent_id(&message.author))
}

/// Whether a row confides a thread to `viewer`: an [`Audience::Aside`] that
/// admits them, read by a seat.
///
/// Only a seat. An operator is admitted to every row by construction, so
/// treating them as a party would hand them every interior of every private
/// exchange at channel level -- a change to what the open desk reads, which
/// is not what this is for. What this is for is a seat that rebuilds its own
/// history from the log and would otherwise lose its own conversation.
pub(crate) fn confidable(audience: &Audience, author: &SessionAuthor, viewer: &Viewer) -> bool {
    matches!(viewer, Viewer::Agent { .. })
        && matches!(audience, Audience::Aside { .. })
        && audience.admits(viewer, author_agent_id(author))
}

/// Narrow an already-projected transcript to what one viewer may read.
///
/// Elides every row the viewer is not admitted to and collapses each run of
/// them from one aside into a single stub, exactly as [`project_session`]
/// does. It exists because the same narrowing is needed by a caller that
/// already holds a transcript rather than a log — `tinyhivemind-hive` composes
/// it with an episode's `Visibility` — and two implementations of a rule this
/// load-bearing would eventually disagree.
///
/// A row that is already elided is left alone: it cannot be narrowed further,
/// and re-eliding it would lose the run it stands for.
///
/// One difference from [`project_session`], and it is a limitation rather than
/// a choice: a `SessionMessage` carries no parent, so this cannot tell two
/// closed threads apart and will merge adjacent runs between the same pair
/// even when they belong to different threads. A caller that holds the log
/// should project from it; this exists for one that holds only a transcript.
#[must_use]
pub fn project_as(messages: &[SessionMessage], viewer: &Viewer) -> Vec<SessionMessage> {
    let narrowed: Vec<(SessionMessage, Option<Sequence>)> = messages
        .iter()
        .map(|message| {
            let narrowed = if message.elided.is_some() {
                message.clone()
            } else {
                present(
                    message.sequence,
                    message.author.clone(),
                    message.content.clone(),
                    message.audience.clone(),
                    viewer,
                )
            };
            (narrowed, None)
        })
        .collect();
    let settlements = Settlement::over(
        messages
            .iter()
            .map(|message| (&message.sequence, &message.audience, &message.author)),
    );
    collapse_elisions(narrowed, &settlements)
}

/// Build one projected message, eliding its content when the viewer is not
/// admitted to it.
///
/// The row is kept either way. Dropping it would leave `^N` citations naming
/// nothing, would hide the existence of an exchange the room is entitled to
/// know happened, and would take from a peer the only signal that there is
/// something worth asking about.
fn present(
    sequence: Sequence,
    author: SessionAuthor,
    content: String,
    audience: Audience,
    viewer: &Viewer,
) -> SessionMessage {
    if audience.admits(viewer, author_agent_id(&author)) {
        return SessionMessage {
            sequence,
            author,
            content,
            audience,
            elided: None,
        };
    }
    SessionMessage {
        sequence,
        author,
        content: String::new(),
        audience,
        elided: Some(Elision {
            through: sequence,
            messages: 1,
            settled_at: None,
        }),
    }
}

/// Everyone who can read a given elided row: its author plus its audience.
///
/// Two rows belong to the same aside when these sets are equal, which is what
/// makes an exchange collapse rather than only a monologue: `alice` writing to
/// `bob` and `bob` writing back to `alice` name different audiences and the
/// same participants.
fn participants(message: &SessionMessage) -> HashSet<&str> {
    let mut set: HashSet<&str> = message
        .audience
        .members()
        .iter()
        .map(String::as_str)
        .collect();
    if let Some(id) = author_agent_id(&message.author) {
        set.insert(id);
    }
    set
}

/// Collapse each consecutive run of elided rows from one aside into one stub.
///
/// The stub keeps the run's first sequence, records the last, counts what it
/// stands in for, and points at where the aside settled — the first thing a
/// participant said in the open afterwards, which is by construction a row
/// this viewer can read.
///
/// This runs after the window has been filled, so a viewer with asides in
/// view receives fewer messages than one without. That is a real cost and it
/// is the honest one: the alternative is backfilling from older history, which
/// would make two viewers of the same desk disagree about how far back the
/// window reaches.
/// One desk-visible row that could settle an aside.
///
/// Collected from the *whole* scanned slice rather than from the projection,
/// because channel narrowing keeps only each root's first reply: when an aside
/// is that first reply and the settlement is a later one, the settlement is
/// not in the projection at all and a stub built from it alone would report
/// `settled_at: None` for an aside the room did in fact settle.
#[derive(Clone, Debug)]
pub(crate) struct Settlement {
    sequence: Sequence,
    author: Option<String>,
}

impl Settlement {
    /// Every desk-visible, agent-authored row in a slice, in sequence order.
    fn over<'a>(
        rows: impl Iterator<Item = (&'a Sequence, &'a Audience, &'a SessionAuthor)>,
    ) -> Vec<Self> {
        rows.filter(|(_, audience, _)| audience.is_desk())
            .map(|(sequence, _, author)| Self {
                sequence: *sequence,
                author: author_agent_id(author).map(str::to_owned),
            })
            .collect()
    }
}

/// Collapse each consecutive run of elided rows from one aside into one stub.
///
/// The stub keeps the run's first sequence, records the last, counts what it
/// stands in for, and points at where the aside settled — the first thing a
/// participant said in the open afterwards.
///
/// `thread` carries each row's conversation identity: a thread root's own
/// sequence, a reply's root, or `None` for a channel-level row. Two runs only
/// merge when they share **both** their participants and that identity, so two
/// distinct closed threads between the same pair stay two stubs. Without it
/// they would collapse into one, reporting a combined range and count for
/// exchanges that never were one exchange.
///
/// This runs after the window has been filled, so a viewer with asides in
/// view receives fewer messages than one without. That is a real cost and it
/// is the honest one: the alternative is backfilling from older history, which
/// would make two viewers of the same desk disagree about how far back the
/// window reaches.
fn collapse_elisions(
    projected: Vec<(SessionMessage, Option<Sequence>)>,
    settlements: &[Settlement],
) -> Vec<SessionMessage> {
    if !projected
        .iter()
        .any(|(message, _)| message.elided.is_some())
    {
        return projected.into_iter().map(|(message, _)| message).collect();
    }

    let mut collapsed: Vec<(SessionMessage, Option<Sequence>)> =
        Vec::with_capacity(projected.len());
    for (message, thread) in projected {
        let extends = match (collapsed.last(), &message.elided) {
            (Some((previous, previous_thread)), Some(_)) => {
                previous.elided.is_some()
                    && *previous_thread == thread
                    && participants(previous) == participants(&message)
            }
            _ => false,
        };
        if extends
            && let Some((previous, _)) = collapsed.last_mut()
            && let Some(elision) = previous.elided.as_mut()
        {
            elision.through = message.sequence;
            elision.messages = elision.messages.saturating_add(1);
            continue;
        }
        collapsed.push((message, thread));
    }

    let mut collapsed: Vec<SessionMessage> =
        collapsed.into_iter().map(|(message, _)| message).collect();
    settle(&mut collapsed, settlements);
    collapsed
}

/// Point each stub at the first thing one of its participants said in the open.
fn settle(messages: &mut [SessionMessage], settlements: &[Settlement]) {
    for message in messages.iter_mut() {
        if message.elided.is_none() {
            continue;
        }
        let inside = participants(message)
            .into_iter()
            .map(str::to_owned)
            .collect::<HashSet<_>>();
        let after = message
            .elided
            .as_ref()
            .map_or(message.sequence, |elision| elision.through);
        let settled_at = settlements
            .iter()
            .find(|candidate| {
                candidate.sequence > after
                    && candidate
                        .author
                        .as_deref()
                        .is_some_and(|id| inside.contains(id))
            })
            .map(|candidate| candidate.sequence);
        if let Some(elision) = message.elided.as_mut() {
            elision.settled_at = settled_at;
        }
    }
}

type Projection = (Vec<(SessionMessage, Option<Sequence>)>, Vec<Settlement>);

async fn project_thread(log: &(dyn SessionLog + '_), query: &SessionQuery) -> Result<Projection> {
    let mut cursor = query.before;
    let mut scanned = 0_usize;
    let mut seen = Vec::new();
    let mut projected = Vec::new();
    let mut seen_rows: Vec<(Sequence, Audience, SessionAuthor)> = Vec::new();
    let mut reached_root = false;

    while scanned < SCAN_LIMIT && projected.len() < query.window && !reached_root {
        let limit = PAGE_SIZE.min(SCAN_LIMIT - scanned);
        let page = log
            .read_before(cursor, limit)
            .await
            .map_err(|source| Error::Read { source })?;
        validate_page(&page, cursor, limit, &mut seen)?;
        scanned += page.messages.len();

        for message in &page.messages {
            if !matches_conversation(message, &query.conversation) {
                continue;
            }
            let is_thread_root = query.conversation.thread_root == Some(message.sequence);
            if message.content.trim().is_empty() {
                if is_thread_root {
                    reached_root = true;
                    break;
                }
                continue;
            }
            seen_rows.push((
                message.sequence,
                message.audience.clone(),
                message.author.clone(),
            ));
            projected.push((
                present(
                    message.sequence,
                    message.author.clone(),
                    message.content.clone(),
                    message.audience.clone(),
                    &query.viewer,
                ),
                query.conversation.thread_root,
            ));
            if is_thread_root {
                reached_root = true;
                break;
            }
            if projected.len() == query.window {
                break;
            }
        }

        if reached_root || projected.len() == query.window || scanned == SCAN_LIMIT {
            break;
        }
        let Some(next) = page.next_before else {
            break;
        };
        cursor = Some(next);
    }

    projected.truncate(query.window);
    projected.reverse();
    seen_rows.reverse();
    let settlements = Settlement::over(
        seen_rows
            .iter()
            .map(|(sequence, audience, author)| (sequence, audience, author)),
    );
    Ok((projected, settlements))
}

/// One in-desk row held with the `parent` [`SessionMessage`] does not carry.
///
/// Picking each root's *first* reply needs the parent link and chronological
/// order, and the walk runs newest-first, so the narrowing cannot be a fold a
/// caller applies to the result.
#[derive(Debug)]
struct Candidate {
    sequence: Sequence,
    parent: Option<Sequence>,
    author: SessionAuthor,
    content: String,
    audience: Audience,
}

async fn project_channel(log: &(dyn SessionLog + '_), query: &SessionQuery) -> Result<Projection> {
    let mut cursor = query.before;
    let mut scanned = 0_usize;
    let mut seen = Vec::new();
    let mut candidates: Vec<Candidate> = Vec::new();
    // Survivors, counted newest-first while the parent of an older row is still
    // unknown: every non-empty root survives, every distinct parent contributes
    // a promoted reply, and a thread confided to this viewer contributes all of
    // its replies. A parent that turns out to be a reply, or to sit outside the
    // scan, is counted here and dropped below, so this over-estimates and the
    // walk can stop a little early — never late.
    let mut answered = BTreeSet::new();
    // Replies beyond their root's first, by root, until the root says whether
    // they are kept. Spent when a confided root arrives; dropped otherwise.
    let mut banked: BTreeMap<Sequence, usize> = BTreeMap::new();
    let mut survivors = 0_usize;
    let mut window_met = false;

    while scanned < SCAN_LIMIT && !window_met {
        let limit = PAGE_SIZE.min(SCAN_LIMIT - scanned);
        let page = log
            .read_before(cursor, limit)
            .await
            .map_err(|source| Error::Read { source })?;
        validate_page(&page, cursor, limit, &mut seen)?;
        scanned += page.messages.len();

        for message in &page.messages {
            if !in_desk(message, &query.conversation) {
                continue;
            }
            candidates.push(Candidate {
                sequence: message.sequence,
                parent: message.parent,
                author: message.author.clone(),
                content: message.content.clone(),
                audience: message.audience.clone(),
            });
            // An empty row is kept as a candidate so it can still be somebody's
            // root, but it is neither a survivor nor a spent promotion.
            if message.content.trim().is_empty() {
                continue;
            }
            match message.parent {
                None => {
                    survivors += 1;
                    // The root decides, so the replies banked under it are
                    // counted here rather than where they were seen. This is
                    // the *only* place the decision can be made: narrowing
                    // retains a reply when the **root** is confided, and a
                    // newest-first walk meets every reply before its root.
                    //
                    // Counting a reply on its own audience instead gets both
                    // directions wrong. An aside root with desk-visible
                    // replies undercounts, and the projection then runs past
                    // `window`. Worse, admitted replies of a root still
                    // beyond the scan overcount, the walk stops before
                    // reaching that root, and narrowing -- finding no root --
                    // drops every one of them, handing the seat the empty
                    // history this whole exception exists to prevent.
                    if confidable(&message.audience, &message.author, &query.viewer) {
                        survivors += banked.remove(&message.sequence).unwrap_or(0);
                    }
                }
                Some(parent) => {
                    // One promoted reply per root, whoever is reading. Any
                    // further reply is banked against its root and counts only
                    // if that root turns out to be confided; a root that never
                    // arrives leaves its bank unspent, which is exactly what
                    // narrowing does with those replies.
                    if answered.insert(parent) {
                        survivors += 1;
                    } else {
                        *banked.entry(parent).or_default() += 1;
                    }
                }
            }
            if survivors >= query.window {
                window_met = true;
                break;
            }
        }

        if window_met || scanned == SCAN_LIMIT {
            break;
        }
        let Some(next) = page.next_before else {
            break;
        };
        cursor = Some(next);
    }

    candidates.reverse();
    // Settlements come from the whole scanned slice, before narrowing throws
    // replies away: an aside that is a root's first reply is settled by a
    // *later* reply, which narrowing drops.
    let settlements = Settlement::over(
        candidates
            .iter()
            .map(|candidate| (&candidate.sequence, &candidate.audience, &candidate.author)),
    );
    let projected = narrow_to_roots_and_first_replies(candidates, &query.viewer);
    // Every survivor was counted by the estimate above, and the walk stops the
    // moment that estimate reaches the window, so the window needs no second
    // enforcement here — and enforcing it would have to trim the newest end.
    //
    // One thread may carry the result past the window: a thread confided to
    // this seat is delivered whole, and its replies are all counted at once,
    // when the walk reaches the root that admits them. Trimming to the window
    // there would cut the thread rather than shorten the page — and cutting
    // it at the oldest end takes the root, leaving replies narrowing would
    // then drop. A page slightly over is the coherent answer; a thread in
    // pieces is not.
    debug_assert!(
        projected.len() <= query.window || projected.iter().any(|(_, thread)| thread.is_some()),
        "over the window with no thread to account for it: {} > {}",
        projected.len(),
        query.window
    );
    Ok((projected, settlements))
}

/// Keep every root and each root's first reply, from a chronological slice --
/// and the whole of a confided thread, for the viewer it was confided to.
///
/// A reply whose parent is itself a reply is a thread interior and never
/// promoted; a reply whose parent fell outside the scan cannot be shown to be a
/// root and is dropped rather than flattened into the channel.
///
/// # The exception, and why it is this narrow
///
/// "Roots and first replies" is written for the reader of a busy desk: it
/// keeps a thread legible without handing over the interior of a conversation
/// that is not theirs. Applied to a **party** of that conversation it does
/// something else -- it withholds their own exchange from them, keeping the
/// question they asked and the first line of the answer, and dropping the
/// rest.
///
/// That is invisible to a runner that holds its context between turns: the
/// exchange reached it once in [`EpisodeBrief::conversations`][brief] and it
/// still has it. A runner that rebuilds a seat's history from the log every
/// turn has only this projection, so the rest of the conversation is gone --
/// measured against a hosted desk, where seats retained nothing of their own
/// conversations at all.
///
/// So a root that is an [`Audience::Aside`] naming this seat is *confided* to
/// it, and it reads every reply under it ([`confidable`]). The scope is
/// deliberate, and each part of it closes something:
///
/// - only an aside, never an ordinary desk thread. Keeping every reply of one
///   of those is "one level flattened", which the spec calls the pre-A leak
///   with extra steps -- and every viewer is admitted to a desk row, so the
///   exception would have no edge at all.
/// - only a party. A third seat still reads the opening and a stub, exactly
///   as before.
/// - only a seat. An operator is admitted to everything, so counting them a
///   party would put every private interior on the open desk.
///
/// [brief]: https://docs.rs/tinyhivemind-driver
fn narrow_to_roots_and_first_replies(
    candidates: Vec<Candidate>,
    viewer: &Viewer,
) -> Vec<(SessionMessage, Option<Sequence>)> {
    let roots: BTreeSet<Sequence> = candidates
        .iter()
        .filter(|candidate| candidate.parent.is_none())
        .map(|candidate| candidate.sequence)
        .collect();
    // The threads confided to this seat: an aside root names its parties, and
    // a party reads the whole of what it opened.
    let confided: BTreeSet<Sequence> = candidates
        .iter()
        .filter(|candidate| candidate.parent.is_none())
        .filter(|candidate| confidable(&candidate.audience, &candidate.author, viewer))
        .map(|candidate| candidate.sequence)
        .collect();
    // A parentless row that something replies to opens a thread of its own;
    // one that nothing replies to is an ordinary channel message. That is the
    // difference between two closed threads between the same pair — which must
    // stay two stubs — and one run of channel asides, which is one.
    let rooted: BTreeSet<Sequence> = candidates
        .iter()
        .filter_map(|candidate| candidate.parent)
        .collect();
    let mut promoted: BTreeSet<Sequence> = BTreeSet::new();
    let mut projected: Vec<(SessionMessage, Option<Sequence>)> = Vec::new();

    for candidate in candidates {
        if candidate.content.trim().is_empty() {
            continue;
        }
        let keep = match candidate.parent {
            None => true,
            // `insert` first and unconditionally: it is what records that this
            // root has spent its promotion, and a confided thread must not
            // leave that unspent for a later, unconfided reader of the same
            // slice.
            Some(parent) if roots.contains(&parent) => {
                let first = promoted.insert(parent);
                first || confided.contains(&parent)
            }
            Some(_) => false,
        };
        if keep {
            // A promoted reply's identity is the root it hangs under; a root
            // that owns a thread is its own; a plain channel row has none, so
            // a run of them still collapses into one stub.
            let thread = candidate.parent.or_else(|| {
                rooted
                    .contains(&candidate.sequence)
                    .then_some(candidate.sequence)
            });
            projected.push((
                present(
                    candidate.sequence,
                    candidate.author,
                    candidate.content,
                    candidate.audience,
                    viewer,
                ),
                thread,
            ));
        }
    }

    projected
}

pub(crate) fn validate_page(
    page: &SessionPage,
    before: Option<Sequence>,
    requested: usize,
    seen: &mut Vec<Sequence>,
) -> Result<()> {
    if page.messages.len() > requested {
        return Err(Error::PageTooLarge {
            requested,
            actual: page.messages.len(),
        });
    }
    if page.messages.is_empty() {
        return match page.next_before {
            Some(next_before) => Err(Error::EmptyPageCursor { next_before }),
            None => Ok(()),
        };
    }

    for (index, message) in page.messages.iter().enumerate() {
        if seen.contains(&message.sequence) {
            return Err(Error::DuplicateSequence {
                sequence: message.sequence,
            });
        }
        if let Some(bound) = before
            && message.sequence >= bound
        {
            return Err(Error::PageOutOfRange {
                sequence: message.sequence,
                before: bound,
            });
        }
        if let Some(previous) = index
            .checked_sub(1)
            .map(|prior| page.messages[prior].sequence)
            && previous <= message.sequence
        {
            return Err(Error::PageNotDescending {
                previous,
                next: message.sequence,
            });
        }
        seen.push(message.sequence);
    }

    if let Some(next_before) = page.next_before {
        if let Some(bound) = before
            && next_before >= bound
        {
            return Err(Error::CursorDidNotAdvance {
                before: bound,
                next_before,
            });
        }
        let oldest = page.messages[page.messages.len() - 1].sequence;
        if next_before > oldest {
            return Err(Error::CursorAfterOldest {
                next_before,
                oldest,
            });
        }
    }
    Ok(())
}

pub(crate) fn in_desk(message: &LogMessage, conversation: &Conversation) -> bool {
    same_conversation(message.chat_id.as_deref(), Some(&conversation.desk_id))
        || same_conversation(message.chat_id.as_deref(), Some(&conversation.desk_name))
}

/// Admit a row addressed to exactly this conversation, one row at a time.
///
/// The channel-level arm is deliberately roots-only. Promoting each root's
/// first reply needs the whole slice — see
/// [`narrow_to_roots_and_first_replies`] — so a caller that decides row by row,
/// such as the sharing delta, keeps the narrower rule and delivers a first
/// reply on the next reseed rather than the next tick.
pub(crate) fn matches_conversation(message: &LogMessage, conversation: &Conversation) -> bool {
    if !in_desk(message, conversation) {
        return false;
    }
    match conversation.thread_root {
        None => message.parent.is_none(),
        Some(root) => message.sequence == root || message.parent == Some(root),
    }
}
