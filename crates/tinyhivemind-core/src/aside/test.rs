//! Unit tests for pure private-aside authorization.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use crate::{
    desk::{Desk, DeskSet, ResponderMode},
    dispatch::DispatchConversation,
    mention::{Mention, MentionTarget},
    roster::{Person, Roster, RosterMember},
};
use serde_json::json;

fn members() -> Vec<RosterMember> {
    ["alice", "bob", "carol", "dave"]
        .into_iter()
        .map(|id| RosterMember {
            id: id.into(),
            name: None,
        })
        .collect()
}

fn desks() -> Vec<Desk> {
    vec![Desk {
        id: "eng".into(),
        name: "Engineering".into(),
        description: None,
        members: vec!["alice".into(), "bob".into(), "carol".into()],
        responder_mode: ResponderMode::Lead,
    }]
}

fn mention(id: &str, offset: usize) -> Mention {
    Mention {
        target: MentionTarget::Agent { id: id.into() },
        text: format!("@{id}"),
        offset,
        quiet: false,
    }
}

fn policy() -> AsidePolicy {
    AsidePolicy {
        enabled: true,
        max_members: 2,
        max_messages: 4,
        must_surface: false,
        require_thread: false,
    }
}

fn input(mentions: Vec<Mention>) -> AsideInput {
    AsideInput {
        conversation: DispatchConversation {
            desk_id: "eng".into(),
            thread_root: Some(7),
        },
        author_id: "alice".into(),
        mentions,
        spent: 0,
        unsettled: false,
    }
}

fn decide(policy: AsidePolicy, input: &AsideInput) -> AsideDecision {
    let members = members();
    let desks_value = desks();
    let roster = Roster::new(&members, &[], &[]);
    let desks = DeskSet::new(&desks_value, &[], &[], &[], &[]);
    aside(policy, input, &roster, &desks).unwrap()
}

fn refused(policy: AsidePolicy, input: &AsideInput) -> NoAsideReason {
    match decide(policy, input) {
        AsideDecision::None { reason } => reason,
        AsideDecision::One { audience } => panic!("expected a refusal, got {audience:?}"),
    }
}

fn audience_of(policy: AsidePolicy, input: &AsideInput) -> Audience {
    match decide(policy, input) {
        AsideDecision::One { audience } => audience,
        AsideDecision::None { reason } => panic!("expected an audience, got {reason:?}"),
    }
}

fn assert_wire<T>(value: &T, expected: serde_json::Value)
where
    T: serde::Serialize + serde::de::DeserializeOwned + Eq + std::fmt::Debug,
{
    assert_eq!(serde_json::to_value(value).unwrap(), expected);
    assert_eq!(serde_json::from_value::<T>(expected).unwrap(), *value);
}

#[test]
fn addresses_one_peer_by_mention() {
    let audience = audience_of(policy(), &input(vec![mention("bob", 0)]));
    assert_eq!(
        audience,
        Audience::Aside {
            members: vec!["bob".into()],
        },
    );
}

#[test]
fn keeps_addressed_peers_in_reading_order_without_repeats() {
    let audience = audience_of(
        policy(),
        &input(vec![
            mention("carol", 9),
            mention("bob", 2),
            mention("carol", 20),
        ]),
    );
    assert_eq!(
        audience,
        Audience::Aside {
            members: vec!["bob".into(), "carol".into()],
        },
    );
}

#[test]
fn drops_the_author_from_its_own_audience() {
    // Naming yourself neither widens nor narrows: an author reads its own row
    // by definition, so `@alice @bob` is an aside with bob.
    let audience = audience_of(
        policy(),
        &input(vec![mention("alice", 0), mention("bob", 7)]),
    );
    assert_eq!(
        audience,
        Audience::Aside {
            members: vec!["bob".into()],
        },
    );
}

#[test]
fn refuses_an_audience_naming_only_the_author() {
    assert_eq!(
        refused(policy(), &input(vec![mention("alice", 0)])),
        NoAsideReason::SelfOnly,
    );
}

#[test]
fn refuses_a_message_addressing_nobody() {
    assert_eq!(
        refused(policy(), &input(Vec::new())),
        NoAsideReason::NoAudience,
    );
}

#[test]
fn a_quiet_mention_addresses_nobody() {
    let mut quiet = mention("bob", 0);
    quiet.quiet = true;
    assert_eq!(
        refused(policy(), &input(vec![quiet])),
        NoAsideReason::NoAudience,
    );
}

#[test]
fn person_desk_and_everyone_mentions_do_not_address_an_aside() {
    let targets = [
        MentionTarget::Person { id: "ada".into() },
        MentionTarget::Desk { id: "eng".into() },
        MentionTarget::Everyone,
    ];
    for target in targets {
        let mention = Mention {
            target: target.clone(),
            text: "@x".into(),
            offset: 0,
            quiet: false,
        };
        assert_eq!(
            refused(policy(), &input(vec![mention])),
            NoAsideReason::NoAudience,
            "{target:?} must not address an aside",
        );
    }
}

#[test]
fn refuses_when_policy_is_disabled() {
    assert_eq!(
        refused(AsidePolicy::DEFAULT, &input(vec![mention("bob", 0)])),
        NoAsideReason::Disabled,
    );
}

#[test]
fn refuses_an_inactive_author() {
    let mut request = input(vec![mention("bob", 0)]);
    request.author_id = "nobody".into();
    assert_eq!(refused(policy(), &request), NoAsideReason::SourceInactive);
}

#[test]
fn refuses_an_author_who_is_not_on_the_desk() {
    let mut request = input(vec![mention("bob", 0)]);
    request.author_id = "dave".into();
    assert_eq!(refused(policy(), &request), NoAsideReason::AuthorNotOnDesk);
}

#[test]
fn refuses_an_inactive_target() {
    assert_eq!(
        refused(policy(), &input(vec![mention("ghost", 0)])),
        NoAsideReason::TargetInactive,
    );
}

#[test]
fn refuses_a_target_who_is_not_on_the_desk() {
    assert_eq!(
        refused(policy(), &input(vec![mention("dave", 0)])),
        NoAsideReason::TargetNotOnDesk,
    );
}

#[test]
fn an_unresolvable_target_stops_the_decision_rather_than_being_skipped() {
    // The audience the author wrote is not the audience left after quietly
    // dropping the names that did not resolve.
    assert_eq!(
        refused(
            policy(),
            &input(vec![mention("dave", 0), mention("bob", 9)])
        ),
        NoAsideReason::TargetNotOnDesk,
    );
}

#[test]
fn refuses_an_audience_larger_than_the_policy_permits() {
    let mut narrow = policy();
    narrow.max_members = 1;
    assert_eq!(
        refused(narrow, &input(vec![mention("bob", 0), mention("carol", 9)])),
        NoAsideReason::AudienceTooLarge,
    );
}

#[test]
fn refuses_a_channel_row_when_the_policy_requires_a_thread() {
    let mut threaded = policy();
    threaded.require_thread = true;
    let mut request = input(vec![mention("bob", 0)]);
    request.conversation.thread_root = None;
    assert_eq!(refused(threaded, &request), NoAsideReason::ThreadRequired);

    request.conversation.thread_root = Some(3);
    assert!(matches!(
        decide(threaded, &request),
        AsideDecision::One { .. }
    ));
}

#[test]
fn refuses_once_the_message_budget_is_spent() {
    let mut request = input(vec![mention("bob", 0)]);
    request.spent = 4;
    assert_eq!(refused(policy(), &request), NoAsideReason::BudgetSpent);

    request.spent = 3;
    assert!(matches!(
        decide(policy(), &request),
        AsideDecision::One { .. }
    ));
}

#[test]
fn refuses_a_second_aside_while_the_first_has_not_surfaced() {
    let mut surfacing = policy();
    surfacing.must_surface = true;
    let mut request = input(vec![mention("bob", 0)]);
    request.unsettled = true;
    assert_eq!(refused(surfacing, &request), NoAsideReason::UnsettledAside);

    // Without `must_surface` the same input is authorized, so the refusal is
    // the policy's and not an accident of the fold.
    assert!(matches!(
        decide(policy(), &request),
        AsideDecision::One { .. }
    ));
}

#[test]
fn general_has_no_membership_list_so_every_active_agent_is_present() {
    // `dave` sits on no desk, so on `eng` neither of these would resolve.
    let mut request = input(vec![mention("alice", 0)]);
    request.conversation.desk_id = crate::chat::GENERAL_DESK.into();
    request.author_id = "dave".into();
    assert_eq!(
        audience_of(policy(), &request),
        Audience::Aside {
            members: vec!["alice".into()],
        },
    );
}

#[test]
fn a_malformed_roster_is_an_error_rather_than_a_refusal() {
    let members = vec![
        RosterMember {
            id: "alice".into(),
            name: None,
        },
        RosterMember {
            id: "alice".into(),
            name: None,
        },
    ];
    let desks_value = desks();
    let roster = Roster::new(&members, &[], &[]);
    let desks = DeskSet::new(&desks_value, &[], &[], &[], &[]);
    assert!(aside(policy(), &input(vec![mention("bob", 0)]), &roster, &desks).is_err());
}

#[test]
fn desk_audience_admits_everyone() {
    let desk = Audience::Desk;
    assert!(desk.is_desk());
    let members_found = desk.members();
    assert!(members_found.is_empty(), "{members_found:?}");
    for viewer in [
        Viewer::Operator,
        Viewer::Person { id: "ada".into() },
        Viewer::Agent { id: "dave".into() },
    ] {
        assert!(desk.admits(&viewer, Some("alice")));
    }
}

#[test]
fn an_aside_admits_its_members_its_author_and_every_person() {
    let audience = Audience::Aside {
        members: vec!["bob".into()],
    };
    assert!(!audience.is_desk());
    assert_eq!(audience.members(), ["bob".to_owned()]);

    // Members, and the author, read it.
    assert!(audience.admits(&Viewer::Agent { id: "bob".into() }, Some("alice")));
    assert!(audience.admits(&Viewer::Agent { id: "alice".into() }, Some("alice")));

    // A peer on the desk who was not addressed does not.
    assert!(!audience.admits(&Viewer::Agent { id: "carol".into() }, Some("alice")));

    // Every person does, in full. This is audit access, not membership.
    assert!(audience.admits(&Viewer::Operator, Some("alice")));
    assert!(audience.admits(&Viewer::Person { id: "ada".into() }, Some("alice")));
}

#[test]
fn a_row_with_no_agent_author_admits_only_its_named_members_and_people() {
    let audience = Audience::Aside {
        members: vec!["bob".into()],
    };
    assert!(audience.admits(&Viewer::Agent { id: "bob".into() }, None));
    assert!(!audience.admits(&Viewer::Agent { id: "alice".into() }, None));
    assert!(audience.admits(&Viewer::Operator, None));
}

#[test]
fn a_viewer_reports_how_it_reads() {
    assert_eq!(Viewer::Agent { id: "bob".into() }.agent_id(), Some("bob"));
    assert_eq!(Viewer::Operator.agent_id(), None);
    assert_eq!(Viewer::Person { id: "ada".into() }.agent_id(), None);

    assert!(Viewer::Operator.reads_everything());
    assert!(Viewer::Person { id: "ada".into() }.reads_everything());
    assert!(!Viewer::Agent { id: "bob".into() }.reads_everything());
}

#[test]
fn the_default_policy_authorizes_nothing() {
    assert_eq!(AsidePolicy::default(), AsidePolicy::DEFAULT);
    assert_eq!(
        AsidePolicy::DEFAULT,
        AsidePolicy {
            enabled: false,
            max_members: 0,
            max_messages: 0,
            must_surface: false,
            require_thread: false,
        },
    );
    // And it actually refuses, rather than merely reading as if it would.
    assert_eq!(
        refused(AsidePolicy::DEFAULT, &input(vec![mention("bob", 0)])),
        NoAsideReason::Disabled,
    );
}

#[test]
fn the_default_audience_is_desk_visible() {
    assert_eq!(Audience::default(), Audience::Desk);
}

#[test]
fn aside_payloads_pin_their_wire_shapes() {
    assert_wire(&Audience::Desk, json!({"kind": "desk"}));
    assert_wire(
        &Audience::Aside {
            members: vec!["bob".into()],
        },
        json!({"kind": "aside", "members": ["bob"]}),
    );
    assert_wire(&Viewer::Operator, json!({"kind": "operator"}));
    assert_wire(
        &Viewer::Person { id: "ada".into() },
        json!({"kind": "person", "id": "ada"}),
    );
    assert_wire(
        &Viewer::Agent { id: "bob".into() },
        json!({"kind": "agent", "id": "bob"}),
    );
    assert_wire(
        &AsidePolicy::DEFAULT,
        json!({
            "enabled": false,
            "max_members": 0,
            "max_messages": 0,
            "must_surface": false,
            "require_thread": false
        }),
    );
    assert_wire(
        &AsideDecision::None {
            reason: NoAsideReason::Disabled,
        },
        json!({"kind": "none", "reason": "disabled"}),
    );
    assert_wire(
        &AsideDecision::One {
            audience: Audience::Aside {
                members: vec!["bob".into()],
            },
        },
        json!({"kind": "one", "audience": {"kind": "aside", "members": ["bob"]}}),
    );
    assert_wire(
        &input(vec![mention("bob", 0)]),
        json!({
            "conversation": {"desk_id": "eng", "thread_root": 7},
            "author_id": "alice",
            "mentions": [{
                "target": {"kind": "agent", "id": "bob"},
                "text": "@bob",
                "offset": 0
            }],
            "spent": 0,
            "unsettled": false
        }),
    );
}

#[test]
fn every_refusal_reason_pins_its_wire_spelling() {
    let cases = [
        (NoAsideReason::Disabled, "disabled"),
        (NoAsideReason::SourceInactive, "source_inactive"),
        (NoAsideReason::AuthorNotOnDesk, "author_not_on_desk"),
        (NoAsideReason::NoAudience, "no_audience"),
        (NoAsideReason::SelfOnly, "self_only"),
        (NoAsideReason::AudienceTooLarge, "audience_too_large"),
        (NoAsideReason::TargetInactive, "target_inactive"),
        (NoAsideReason::TargetNotOnDesk, "target_not_on_desk"),
        (NoAsideReason::ThreadRequired, "thread_required"),
        (NoAsideReason::BudgetSpent, "budget_spent"),
        (NoAsideReason::UnsettledAside, "unsettled_aside"),
    ];
    for (reason, spelling) in cases {
        assert_wire(&reason, json!(spelling));
    }
}

#[test]
fn a_person_snapshot_does_not_change_who_may_open_an_aside() {
    // People are not desk members and cannot be addressed by an aside, but a
    // roster carrying them must still validate and decide identically.
    let members = members();
    let people = vec![Person {
        id: "ada".into(),
        label: "Ada".into(),
    }];
    let desks_value = desks();
    let roster = Roster::new(&members, &people, &[]);
    let desks = DeskSet::new(&desks_value, &[], &[], &[], &[]);
    let decision = aside(policy(), &input(vec![mention("bob", 0)]), &roster, &desks).unwrap();
    assert_eq!(
        decision,
        AsideDecision::One {
            audience: Audience::Aside {
                members: vec!["bob".into()],
            },
        },
    );
}

#[test]
fn a_retired_target_is_inactive() {
    let members = members();
    let desks_value = desks();
    let retired = vec!["bob".to_owned()];
    let roster = Roster::new(&members, &[], &retired);
    let desks = DeskSet::new(&desks_value, &[], &[], &[], &retired);
    let decision = aside(policy(), &input(vec![mention("bob", 0)]), &roster, &desks).unwrap();
    assert_eq!(
        decision,
        AsideDecision::None {
            reason: NoAsideReason::TargetInactive,
        },
    );
}

#[test]
fn a_policy_rung_never_masks_an_unresolvable_audience() {
    // The spec fixes the evaluation order: the audience is resolved and
    // validated *before* the thread, budget and settlement rungs. A caller
    // told `ThreadRequired` for a request that also names an inactive target
    // would open a thread and be refused again for the reason it was never
    // given, so each of these pairs must report the audience fault.
    let mut threaded = policy();
    threaded.require_thread = true;
    let mut request = input(vec![mention("ghost", 0)]);
    request.conversation.thread_root = None;
    assert_eq!(refused(threaded, &request), NoAsideReason::TargetInactive);

    let mut spent = policy();
    spent.max_messages = 1;
    let mut request = input(vec![mention("dave", 0)]);
    request.spent = 4;
    assert_eq!(refused(spent, &request), NoAsideReason::TargetNotOnDesk);

    let mut surfacing = policy();
    surfacing.must_surface = true;
    let mut request = input(Vec::new());
    request.unsettled = true;
    assert_eq!(refused(surfacing, &request), NoAsideReason::NoAudience);

    // And an author fault still outranks the audience, because a member that
    // is not on this desk cannot address anybody on it.
    let mut request = input(vec![mention("ghost", 0)]);
    request.author_id = "dave".into();
    assert_eq!(refused(policy(), &request), NoAsideReason::AuthorNotOnDesk);
}
