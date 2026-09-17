//! Shared fixtures and wire-form assertion helpers for the responder tests.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::super::*;
use crate::{
    desk::{Desk, ResponderMode},
    mention::{Mention, MentionTarget},
    roster::RosterMember,
};

/// Build a roster member with an optional display name.
pub(super) fn member(id: &str, name: Option<&str>) -> RosterMember {
    RosterMember {
        id: id.into(),
        name: name.map(str::to_owned),
    }
}

/// Build a desk record with the given members and responder mode.
pub(super) fn desk(id: &str, name: &str, members: &[&str], responder_mode: ResponderMode) -> Desk {
    Desk {
        id: id.into(),
        name: name.into(),
        description: None,
        members: members.iter().map(|id| (*id).into()).collect(),
        responder_mode,
    }
}

/// Build a baseline responder request for the given chat identity.
pub(super) fn request(chat: Option<&str>) -> ResponderRequest {
    ResponderRequest {
        message: "Please handle this".into(),
        chat: chat.map(str::to_owned),
        mentions: Vec::new(),
        orchestrator_id: "orch".into(),
        selection_policy: SelectionPolicy::Allowed,
        minimum_selection_confidence: Probability::ZERO,
    }
}

/// Build a nonquiet-or-quiet agent mention at the given offset.
pub(super) fn mention(id: &str, offset: usize, quiet: bool) -> Mention {
    Mention {
        target: MentionTarget::Agent { id: id.into() },
        text: format!("@{id}"),
        offset,
        quiet,
    }
}

/// Unwrap an immediate decision, panicking if the plan requested selection.
pub(super) fn decision(plan: ResponderPlan) -> ResponderDecision {
    let ResponderPlan::Decided { decision } = plan else {
        panic!("expected an immediate decision")
    };
    decision
}

/// Assert that a value's wire form matches `expected` and round-trips.
pub(super) fn assert_wire<T>(value: &T, expected: serde_json::Value)
where
    T: serde::Serialize + for<'de> serde::Deserialize<'de> + Eq + std::fmt::Debug,
{
    assert_eq!(serde_json::to_value(value).unwrap(), expected);
    assert_eq!(serde_json::from_value::<T>(expected).unwrap(), *value);
}

/// Assert that removing `field` from `value` fails deserialization of `T`.
pub(super) fn assert_missing_field<T>(mut value: serde_json::Value, field: &str)
where
    T: for<'de> serde::Deserialize<'de> + std::fmt::Debug,
{
    value.as_object_mut().unwrap().remove(field);
    assert!(
        serde_json::from_value::<T>(value)
            .unwrap_err()
            .to_string()
            .contains(&format!("missing field `{field}`"))
    );
}

/// Assert that every named field of `value` is required to deserialize `T`.
pub(super) fn assert_required_fields<T>(value: &serde_json::Value, fields: &[&str])
where
    T: for<'de> serde::Deserialize<'de> + std::fmt::Debug,
{
    for field in fields {
        assert_missing_field::<T>(value.clone(), field);
    }
}
