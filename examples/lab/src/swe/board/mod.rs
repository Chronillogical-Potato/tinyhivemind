//! The shared in-memory transcript a hive's seats read and write.
//!
//! The board is the lab's host side of core's runtime: an append-only
//! [`MemoryLog`] (the host-owned journal), core's `commit_utterance_to_room` deciding
//! what each spoken tool call becomes, and core's pins, digest and projection
//! deciding what each seat is shown. There is no second journal: the digest is
//! host state that supersedes itself, and every row stays in the log.
//!
//! What a seat sees at the top of an activation is [`Board::briefing`]: the
//! pinned rows, the digest of everything older than the live tail, then the
//! live tail. That bounded view, rather than each seat's whole private
//! history, is the hive's intervention on token cost.

mod digester;

use std::sync::{Mutex, MutexGuard};

use tinyhivemind_core::aside::{AsidePolicy, Viewer};
use tinyhivemind_core::dispatch::DispatchConversation;
use tinyhivemind_core::mention::MentionTarget;
use tinyhivemind_core::runtime::digest::{
    ChannelDigest, ChannelHead, DigestOutcome, DigestPolicy, apply_digest, refold,
};
use tinyhivemind_core::runtime::pins::{PIN_LIMIT, read_pinboard};
use tinyhivemind_core::runtime::speech::{CommitRequest, Utterance, commit_utterance_to_room};
use tinyhivemind_core::runtime::{
    Conversation, Sequence, SessionQuery, project_session, render_row,
};

use crate::{MemoryLog, World, agent, block_on};

use digester::ExtractiveDigester;

/// The one desk every seat sits on.
pub const DESK: &str = "swe";

/// What committing one utterance produced.
#[derive(Clone, Debug)]
pub struct Committed {
    /// The row's sequence in the log.
    pub sequence: Sequence,
    /// Seats the row names, from mentions and an `ask`'s addressee.
    pub addressed: Vec<String>,
    /// The row's text.
    pub content: String,
    /// Whether the author reported its assignment done.
    pub completes: bool,
    /// Whether the row asks the host to route it to a teammate.
    pub broadcasting: bool,
}

struct Inner {
    log: MemoryLog,
    digest: Option<ChannelDigest>,
    folds: u32,
}

/// The shared transcript and the policies that bound what is read from it.
pub struct Board {
    inner: Mutex<Inner>,
    world: World,
    conversation: Conversation,
    policy: DigestPolicy,
    window: usize,
}

impl Board {
    /// A board for `seats`, the first of which is the desk lead.
    ///
    /// `window` is the live tail a seat is shown; rows older than that are
    /// folded into the digest once enough have accumulated.
    #[must_use]
    pub fn new(seats: &[&str], window: usize) -> Self {
        let mut world = World::new();
        for seat in seats {
            world = world.agent(seat);
        }
        let world = world.desk(DESK, "SWE desk", "Fix the task in the sandbox", seats);
        Self {
            inner: Mutex::new(Inner {
                log: MemoryLog::default(),
                digest: None,
                folds: 0,
            }),
            world,
            conversation: Conversation {
                desk_id: DESK.into(),
                desk_name: "SWE desk".into(),
                thread_root: None,
            },
            policy: DigestPolicy {
                keep_live: window,
                fold_after: (window / 2).max(2),
                input_limit: 60,
                budget_chars: 2400,
                ..DigestPolicy::DEFAULT
            },
            window,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Append an accepted utterance as `speaker`.
    ///
    /// # Errors
    ///
    /// Returns core's message when the speaker or desk is not recognised.
    pub fn commit(&self, speaker: &str, utterance: &Utterance) -> Result<Committed, String> {
        let roster = self.world.roster();
        let desks = self.world.desks();
        let conversation = DispatchConversation {
            desk_id: DESK.into(),
            thread_root: None,
        };
        // This desk runs with asides off and wants an `ask` heard by the room,
        // so it opts into the fallback rather than treating the refusal as fatal.
        let committed = commit_utterance_to_room(&CommitRequest {
            utterance,
            speaker_id: speaker,
            conversation: &conversation,
            aside: AsidePolicy::DEFAULT,
            spent: 0,
            unsettled: false,
            roster: &roster,
            desks: &desks,
        })
        .map_err(|error| error.to_string())?;
        let mut addressed: Vec<String> = committed
            .mentions
            .iter()
            .filter_map(|mention| match &mention.target {
                MentionTarget::Agent { id } if id != speaker => Some(id.clone()),
                _ => None,
            })
            .collect();
        addressed.dedup();
        let sequence = self.lock().log.append(
            DESK,
            None,
            agent(speaker),
            &committed.content,
            committed.audience.clone(),
        );
        Ok(Committed {
            sequence,
            addressed,
            content: committed.content,
            completes: committed.completes_episode,
            broadcasting: committed.broadcasting,
        })
    }

    /// Fold older rows into the digest when the policy says it is time.
    ///
    /// Returns the new generation when a fold happened.
    pub fn maintain(&self) -> Option<u32> {
        let mut inner = self.lock();
        let head = inner.log.head();
        let unfolded_chars = inner
            .log
            .rows()
            .iter()
            .filter(|row| {
                inner
                    .digest
                    .as_ref()
                    .is_none_or(|d| row.sequence > d.through)
            })
            .map(|row| row.content.len())
            .sum();
        let pins = block_on(read_pinboard(
            &inner.log,
            &self.conversation,
            &Viewer::Operator,
            PIN_LIMIT,
            None,
        ))
        .unwrap_or_default();
        let outcome = block_on(refold(
            &inner.log,
            Some(&ExtractiveDigester),
            &self.conversation,
            inner.digest.as_ref(),
            ChannelHead {
                sequence: head,
                unfolded_chars,
            },
            &pins,
            self.policy,
        ));
        if let Ok(DigestOutcome::Folded(digest)) = outcome {
            inner.digest = Some(digest);
            inner.folds += 1;
            return Some(inner.folds);
        }
        None
    }

    /// What `seat` is shown at the top of an activation: pins, digest, tail.
    #[must_use]
    pub fn briefing(&self, seat: &str) -> String {
        let inner = self.lock();
        let viewer = Viewer::Agent { id: seat.into() };
        let pins = block_on(read_pinboard(
            &inner.log,
            &self.conversation,
            &viewer,
            PIN_LIMIT,
            None,
        ))
        .unwrap_or_default();
        let query = SessionQuery {
            conversation: self.conversation.clone(),
            viewer,
            before: None,
            window: self.window + 4,
        };
        let projected = block_on(project_session(&inner.log, &query)).unwrap_or_default();
        let history = apply_digest(inner.digest.as_ref(), &projected);
        let mut text = String::new();
        if !pins.is_empty() {
            text.push_str("## Pinned\n");
            for pin in &pins {
                let label = pin
                    .label
                    .as_deref()
                    .map(|l| format!(" #{l}"))
                    .unwrap_or_default();
                let excerpt = pin.excerpt.as_deref().unwrap_or_default();
                text.push_str(&format!("[{}]{label} {excerpt}\n", pin.sequence));
            }
        }
        if let Some(digest) = &history.digest {
            text.push_str("## Earlier on the desk (digest)\n");
            text.push_str(digest);
            text.push('\n');
        }
        text.push_str("## Recent desk messages\n");
        let rows: Vec<String> = history.messages.iter().filter_map(render_row).collect();
        if rows.is_empty() {
            text.push_str("(nothing yet)\n");
        }
        for row in rows {
            text.push_str(&row);
            text.push('\n');
        }
        text
    }

    /// The most recent `limit` desk rows, rendered, for the `read` tool.
    #[must_use]
    pub fn read(&self, seat: &str, limit: usize) -> String {
        let inner = self.lock();
        let query = SessionQuery {
            conversation: self.conversation.clone(),
            viewer: Viewer::Agent { id: seat.into() },
            before: None,
            window: limit,
        };
        let projected = block_on(project_session(&inner.log, &query)).unwrap_or_default();
        let rows: Vec<String> = projected.iter().filter_map(render_row).collect();
        if rows.is_empty() {
            "(no messages)".to_owned()
        } else {
            rows.join("\n")
        }
    }

    /// Rows appended so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock().log.rows().len()
    }

    /// Whether nothing has been said yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod test;
