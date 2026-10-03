//! `TeamBriefing::from_snapshots` tests: desk-order filtering for a named
//! desk, roster order for General, error propagation from invalid
//! snapshots, and construction without host role types.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::super::*;
use super::support::named_conversation;
use crate::{
    desk::{Desk, DeskMember, DeskOrder, ResponderMode},
    roster::{Person, RosterMember},
};

fn members() -> Vec<RosterMember> {
    vec![
        RosterMember {
            id: "alice".into(),
            name: Some("Alice".into()),
        },
        RosterMember {
            id: "bob".into(),
            name: Some("Bob".into()),
        },
        RosterMember {
            id: "retired".into(),
            name: None,
        },
        RosterMember {
            id: "carol".into(),
            name: None,
        },
    ]
}

fn desk_records() -> Vec<Desk> {
    vec![Desk {
        id: "engineering".into(),
        name: "Engineering".into(),
        description: Some("Build".into()),
        members: vec!["bob".into(), "unknown".into(), "alice".into()],
        responder_mode: ResponderMode::Lead,
    }]
}

#[test]
fn named_desk_uses_effective_order_and_filters_viewer_retired_unknown_and_duplicates() {
    let members = members();
    let retired = vec!["retired".into()];
    let roster = Roster::new(&members, &[], &retired);
    let desks = desk_records();
    let additions = vec![
        DeskMember {
            desk_id: "engineering".into(),
            agent_id: "carol".into(),
        },
        DeskMember {
            desk_id: "engineering".into(),
            agent_id: "bob".into(),
        },
        DeskMember {
            desk_id: "engineering".into(),
            agent_id: "retired".into(),
        },
    ];
    let orders = vec![DeskOrder {
        desk_id: "engineering".into(),
        ordered: vec![
            "carol".into(),
            "unknown".into(),
            "alice".into(),
            "bob".into(),
        ],
    }];
    let desk_set = DeskSet::new(&desks, &[], &additions, &orders, &retired);
    let briefing = TeamBriefing::from_snapshots("alice", &named_conversation(), &desk_set, &roster)
        .expect("valid snapshots");
    assert_eq!(
        briefing
            .teammates
            .iter()
            .map(|teammate| (teammate.id.as_str(), teammate.label.as_str()))
            .collect::<Vec<_>>(),
        vec![("carol", "carol"), ("bob", "Bob")]
    );
    assert!(briefing.teammates.iter().all(|item| item.role.is_none()));
}

#[test]
fn general_uses_the_active_roster_in_roster_order() {
    let members = members();
    let retired = vec!["retired".into()];
    let roster = Roster::new(&members, &[], &retired);
    let desks = DeskSet::new(&[], &[], &[], &[], &retired);
    let conversation = Conversation {
        desk_id: "main".into(),
        desk_name: "General".into(),
        thread_root: None,
    };
    let briefing = TeamBriefing::from_snapshots("bob", &conversation, &desks, &roster)
        .expect("valid snapshots");
    assert_eq!(
        briefing
            .teammates
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        vec!["alice", "carol"]
    );
}

#[test]
fn invalid_snapshots_return_the_precise_core_source() {
    let members = vec![RosterMember {
        id: " ".into(),
        name: None,
    }];
    let roster = Roster::new(&members, &[], &[]);
    let desks = DeskSet::new(&[], &[], &[], &[], &[]);
    let error = TeamBriefing::from_snapshots("viewer", &named_conversation(), &desks, &roster)
        .expect_err("blank member id fails");
    assert!(matches!(error, crate::runtime::Error::Core { .. }));
    assert!(std::error::Error::source(&error).is_some());
}

#[test]
fn snapshot_constructor_does_not_require_people_or_host_role_types() {
    let people = vec![Person {
        id: "person".into(),
        label: "Person".into(),
    }];
    let members = members();
    let roster = Roster::new(&members, &people, &[]);
    let desks = desk_records();
    let desk_set = DeskSet::new(&desks, &[], &[], &[], &[]);
    let briefing = TeamBriefing::from_snapshots("alice", &named_conversation(), &desk_set, &roster)
        .expect("constructs without host types");
    assert_eq!(briefing.teammates[0].id, "bob");
}

// ---------------------------------------------------------------------------
// Private asides
// ---------------------------------------------------------------------------
