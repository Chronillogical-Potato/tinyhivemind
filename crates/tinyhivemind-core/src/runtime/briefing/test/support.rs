//! Shared fixtures for the briefing tests: a default named conversation, a
//! default viewer briefing, and a helper for building one with a given aside
//! policy.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::super::*;
use crate::aside::AsidePolicy;

/// A named "engineering" desk with no thread scope.
pub(super) fn named_conversation() -> Conversation {
    Conversation {
        desk_id: "engineering".into(),
        desk_name: "Engineering".into(),
        thread_root: None,
    }
}

/// A conservative briefing for `alice` in `engineering`, with no teammates.
pub(super) fn viewer_briefing() -> TeamBriefing {
    TeamBriefing {
        viewer_id: "alice".into(),
        desk_id: "engineering".into(),
        desk_name: "Engineering".into(),
        teammates: Vec::new(),
        brevity: BrevityPolicy::DEFAULT,
        asides: AsidePolicy::DEFAULT,
    }
}

/// [`viewer_briefing`] with its aside policy replaced by `asides`.
pub(super) fn briefing_with(asides: AsidePolicy) -> TeamBriefing {
    TeamBriefing {
        asides,
        ..viewer_briefing()
    }
}

/// A briefing for `alice` with one teammate, `bob`, given a role and
/// description.
pub(super) fn briefed_viewer() -> TeamBriefing {
    TeamBriefing {
        viewer_id: "alice".into(),
        desk_id: "engineering".into(),
        desk_name: "Engineering".into(),
        teammates: vec![BriefedTeammate {
            id: "bob".into(),
            label: "Bob".into(),
            role: Some("reviewer".into()),
            description: Some("Checks safety".into()),
        }],
        brevity: BrevityPolicy::DEFAULT,
        asides: AsidePolicy::DEFAULT,
    }
}

/// An aside policy permissive enough to exercise the grammar it enables.
pub(super) fn permissive() -> AsidePolicy {
    AsidePolicy {
        enabled: true,
        max_members: 2,
        max_messages: 4,
        must_surface: true,
        require_thread: false,
    }
}
