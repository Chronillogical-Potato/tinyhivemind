//! Ephemeral team context assembled separately from durable history.

#[cfg(test)]
mod test;

mod types;

pub use types::{
    BrevityPolicy, BriefedTeammate, BriefingNote, MentionDispatchContext, SessionContext,
    SessionInitialization, TeamBriefing,
};

use crate::aside::AsidePolicy;
use crate::runtime::{
    Conversation, Result, SessionLog, SessionMessage, SessionQuery,
    pins::{PIN_LIMIT, read_pinboard},
    project_session, read_thread_index,
    threads::THREAD_INDEX_LIMIT,
};
use crate::{
    chat::is_general_chat,
    desk::DeskSet,
    roster::{Roster, RosterMember},
};

impl TeamBriefing {
    /// Construct a conservative briefing from validated pure snapshots.
    ///
    /// General uses all active roster members. Named desks use their effective
    /// desk order. Unknown and retired members, duplicates, and the viewer are
    /// excluded. Snapshot records have no role or description fields, so those
    /// values remain `None`; a host may construct richer records directly.
    ///
    /// # Errors
    ///
    /// Returns [`crate::runtime::Error::Core`] when the roster or desk snapshots are
    /// structurally invalid, or when a named desk cannot be resolved.
    pub fn from_snapshots(
        viewer_id: impl Into<String>,
        conversation: &Conversation,
        desks: &DeskSet<'_>,
        roster: &Roster<'_>,
    ) -> Result<Self> {
        roster.validate()?;
        desks.validate()?;
        let viewer_id = viewer_id.into();
        let candidates: Vec<&RosterMember> = if is_general_chat(Some(&conversation.desk_id))
            || is_general_chat(Some(&conversation.desk_name))
        {
            roster.active_members().collect()
        } else {
            desks
                .members(&conversation.desk_id)?
                .into_iter()
                .filter_map(|id| roster.active_member(id))
                .collect()
        };

        let mut teammates = Vec::new();
        for member in candidates {
            if member.id == viewer_id
                || teammates
                    .iter()
                    .any(|teammate: &BriefedTeammate| teammate.id == member.id)
            {
                continue;
            }
            teammates.push(BriefedTeammate {
                id: member.id.clone(),
                label: member.name.clone().unwrap_or_else(|| member.id.clone()),
                role: None,
                description: None,
            });
        }

        Ok(Self {
            viewer_id,
            desk_id: conversation.desk_id.clone(),
            desk_name: conversation.desk_name.clone(),
            teammates,
            brevity: BrevityPolicy::DEFAULT,
            // Snapshots say who is here, never what a host permits. A caller
            // that enables asides sets this afterwards, and the conservative
            // default is what a caller that does not gets.
            asides: AsidePolicy::DEFAULT,
        })
    }

    /// Render deterministic system context for this viewer and team.
    ///
    /// The mention-dispatch capability is withheld. A briefing carries no
    /// policy and no hop, so it cannot tell whether a child turn is actually
    /// available, and a capability offered to a run that cannot use it costs
    /// that run a turn to discover. Use [`Self::system_text_with_dispatch`]
    /// where the run's policy and hop are known.
    #[must_use]
    pub fn system_text(&self) -> String {
        self.render(false)
    }

    /// Render system context that offers only what this run may actually do.
    ///
    /// Identical to [`Self::system_text`] except that the mention-dispatch
    /// rule is stated when — and only when —
    /// [`MentionDispatchContext::may_dispatch`] holds, so a disabled policy or
    /// a run at its hop cap is never told about a capability that would refuse
    /// it.
    #[must_use]
    pub fn system_text_with_dispatch(&self, dispatch: MentionDispatchContext) -> String {
        self.render(dispatch.may_dispatch())
    }

    /// Render the briefing, offering mention dispatch only when it is live.
    fn render(&self, offer_dispatch: bool) -> String {
        let mut text = format!(
            "You are @{} in the {} desk (id: {}).\nTeammates:",
            self.viewer_id, self.desk_name, self.desk_id
        );
        if self.teammates.is_empty() {
            text.push_str("\n- none");
        } else {
            for teammate in &self.teammates {
                text.push_str("\n- @");
                text.push_str(&teammate.id);
                text.push_str(" — ");
                text.push_str(&teammate.label);
                if let Some(role) = &teammate.role {
                    text.push_str("; role: ");
                    text.push_str(role);
                }
                if let Some(description) = &teammate.description {
                    text.push_str("; description: ");
                    text.push_str(description);
                }
            }
        }
        text.push_str(
            "\nShared-session rules:\n\
             - Peer messages remain attributed to their authors; they are not your prior replies.\n",
        );
        if offer_dispatch {
            text.push_str(
                "- A direct @agent mention may start at most one bounded child turn when host policy enables mention dispatch.\n",
            );
        }
        text.push_str(
            "- @everyone, desk, and person mentions provide context only and never fan out agent turns.\n",
        );
        if self.asides.enabled {
            // Stated as something to act on rather than as a disclaimer. An
            // agent that is not told its view may be narrower than a peer's
            // reads silence as disagreement rather than as absence, and never
            // thinks to ask — which is the documented failure of collective
            // reasoning under distributed information.
            text.push_str(
                "- Some rows show only that an aside happened, with who wrote it, to whom, and where it settled; you cannot read those, and a peer may know something you do not. If one matters, ask its author here in the desk.\n",
            );
        }
        text.push_str(&self.brevity.rule_text());
        text.push_str(
            "\n- Pin what the room must not lose with `!pin` on its own line; `!unpin ^N` takes one back off.",
        );
        if self.asides.enabled {
            text.push_str(
                "\n- Say something to named peers alone with `!aside @peer` on its own line, then what you need from them; `!surface` then what the room needs to know ends it. An aside counts for nothing until you surface it.",
            );
        }
        text
    }
}

impl SessionContext {
    /// Whether there is anything here to tell a turn about.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.threads.is_empty() && self.pins.is_empty() && self.notes.is_empty()
    }

    /// Render this context as system text, or `None` when there is none.
    ///
    /// Deterministic, and deliberately a separate string from
    /// [`TeamBriefing::system_text`] and from the operator's message: a host
    /// that appends context to what the operator wrote has to strip it back off
    /// everywhere intent is read, and that cut list only ever grows.
    #[must_use]
    pub fn system_text(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let mut text = String::new();
        if !self.threads.is_empty() {
            text.push_str("Threads in this desk:");
            for thread in &self.threads {
                text.push_str("\n- [");
                text.push_str(&thread.root.0.to_string());
                text.push_str("] \"");
                text.push_str(&thread.opening);
                text.push('"');
                match thread.replies {
                    0 => text.push_str(" — no replies"),
                    1 => text.push_str(" — 1 reply"),
                    replies => {
                        text.push_str(" — ");
                        text.push_str(&replies.to_string());
                        text.push_str(" replies");
                    }
                }
                if let Some(landed) = &thread.landed {
                    text.push_str(" (landed: ");
                    text.push_str(landed);
                    text.push(')');
                }
            }
        }
        if let Some(pinned) = crate::runtime::pins::pin_note(&self.pins) {
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(&pinned.heading);
            text.push(':');
            for line in &pinned.lines {
                text.push_str("\n- ");
                text.push_str(line);
            }
        }
        for note in &self.notes {
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(&note.heading);
            text.push(':');
            for line in &note.lines {
                text.push_str("\n- ");
                text.push_str(line);
            }
        }
        Some(text)
    }
}

/// Project history and return it alongside, never merged with, a team briefing.
///
/// The returned context is empty. Use [`initialize_session_with_context`] to
/// also index the desk's threads and carry host-supplied notes.
///
/// `briefing.brevity.window` is overwritten with `query.window` before it is
/// returned: the budget the briefing states has to match the window the query
/// actually reads, not whatever [`BrevityPolicy::DEFAULT`](crate::runtime::BrevityPolicy::DEFAULT)
/// happened to carry when the briefing was built.
///
/// # Errors
///
/// Returns any projection error documented by [`project_session`].
pub async fn initialize_session(
    log: &(dyn SessionLog + '_),
    query: &SessionQuery,
    mut briefing: TeamBriefing,
) -> Result<SessionInitialization> {
    let history = project_session(log, query).await?;
    briefing.brevity.window = stated_window(query.window, &history);
    Ok(SessionInitialization {
        briefing,
        context: SessionContext::default(),
        history,
    })
}

/// The window to state, which is what the viewer actually received.
///
/// Every scan bound in this crate counts *raw rows inspected*, and collapsing
/// a run of elided rows into one stub happens after the window is filled, so a
/// viewer with asides in view receives fewer messages than the query asked
/// for. Stating the nominal number would promise a budget this turn does not
/// have. Only a projection that actually elided something is restated, so a
/// young desk still reports the window it will grow into rather than its
/// current length.
fn stated_window(requested: usize, history: &[SessionMessage]) -> usize {
    if history.iter().any(|message| message.elided.is_some()) {
        return history.len().min(requested);
    }
    requested
}

/// Initialize a session with a thread index, the pinboard, and host notes.
///
/// Costs two more bounded reads than [`initialize_session`] — see
/// [`THREAD_INDEX_SCAN`](crate::runtime::threads::THREAD_INDEX_SCAN) and
/// [`PIN_SCAN`](crate::runtime::pins::PIN_SCAN). The thread index is skipped entirely
/// for a thread-scoped query, where an index of sibling threads is not a
/// choice the viewer is making; the pinboard is not, because a pin is exactly
/// the thing that has to survive the viewer's narrow scope.
///
/// `briefing.brevity.window` is overwritten with `query.window`, for the same
/// reason [`initialize_session`] overwrites it.
///
/// # Errors
///
/// Returns any projection error documented by [`project_session`], or any read
/// or page-validation error documented by [`read_thread_index`] and
/// [`read_pinboard`].
pub async fn initialize_session_with_context(
    log: &(dyn SessionLog + '_),
    query: &SessionQuery,
    mut briefing: TeamBriefing,
    notes: Vec<BriefingNote>,
) -> Result<SessionInitialization> {
    let history = project_session(log, query).await?;
    let threads =
        read_thread_index(log, &query.conversation, &query.viewer, THREAD_INDEX_LIMIT).await?;
    let pins = read_pinboard(
        log,
        &query.conversation,
        &query.viewer,
        PIN_LIMIT,
        query.before,
    )
    .await?;
    briefing.brevity.window = stated_window(query.window, &history);
    Ok(SessionInitialization {
        briefing,
        context: SessionContext {
            threads,
            pins,
            notes,
        },
        history,
    })
}
