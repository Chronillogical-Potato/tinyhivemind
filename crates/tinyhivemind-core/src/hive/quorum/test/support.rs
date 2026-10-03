//! Shared fixtures for the quorum test suite: transcript builders, the
//! shared policy most tests fold under, and the `fold`/`standing`
//! helpers that turn a transcript into the one standing a test cares about.

use super::super::*;
use crate::hive::trace::read;
use crate::runtime::aside::Audience;
use crate::runtime::{SessionAuthor, SessionMessage};

/// One agent-authored, desk-visible message.
pub(super) fn said(sequence: u64, author: &str, content: &str) -> SessionMessage {
    SessionMessage {
        sequence: Sequence(sequence),
        author: SessionAuthor::Agent {
            id: author.into(),
            label: author.into(),
        },
        content: content.into(),
        audience: Audience::Desk,
        elided: None,
    }
}

/// The shared policy for these tests.
pub(super) fn policy(threshold: u32) -> QuorumPolicy {
    QuorumPolicy {
        threshold,
        window: 100,
        require_grounded: true,
    }
}

/// Fold a transcript to its standings, at the sequence of its last message.
pub(super) fn fold(transcript: &[SessionMessage], policy: &QuorumPolicy) -> Vec<TopicStanding> {
    let at = transcript.last().map_or(Sequence(0), |m| m.sequence);
    standings(&read(transcript), at, policy).expect("folds")
}

/// The standing for one topic, panicking with the topic name if it is absent.
pub(super) fn standing<'a>(standings: &'a [TopicStanding], topic: &str) -> &'a TopicStanding {
    standings
        .iter()
        .find(|standing| standing.topic.as_str() == topic)
        .unwrap_or_else(|| panic!("no standing for {topic}"))
}

/// Two proposals, two grounded supporters each: a genuine tie.
pub(super) fn deadlocked_transcript() -> Vec<SessionMessage> {
    vec![
        said(1, "planner", "!propose #stage Stage the rollout."),
        said(2, "scout", "!propose #ship Ship it at once."),
        said(3, "critic", "!support #stage ^1 Bounds the blast radius."),
        said(
            4,
            "archivist",
            "!support #ship ^2 We have shipped this before.",
        ),
    ]
}

/// Two grounded supporters, and a fact one member can point at.
pub(super) fn contested_transcript() -> Vec<SessionMessage> {
    vec![
        said(1, "planner", "!propose #stage Stage the rollout."),
        said(
            2,
            "critic",
            "!support #stage ^1 It bounds the blast radius.",
        ),
        said(
            3,
            "auditor",
            "!evidence The staging environment was retired in March.",
        ),
    ]
}
