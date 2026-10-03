//! Unit tests for the off-floor exchange fold.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;

use crate::runtime::aside::Audience;
use crate::runtime::{
    Conversation, Sequence,
    desk::{Desk, DeskSet, ResponderMode},
    roster::{Roster, RosterMember},
};

const MEMBERS: [&str; 3] = ["planner", "critic", "scout"];

fn roster_members() -> Vec<RosterMember> {
    MEMBERS
        .iter()
        .map(|id| RosterMember {
            id: (*id).into(),
            name: Some((*id).into()),
        })
        .collect()
}

fn desks() -> Vec<Desk> {
    vec![Desk {
        id: "engineering".into(),
        name: "Engineering".into(),
        description: None,
        members: MEMBERS.iter().map(|id| (*id).to_owned()).collect(),
        responder_mode: ResponderMode::Auto,
    }]
}

fn state() -> EpisodeState {
    EpisodeState::opened(
        Conversation {
            desk_id: "engineering".into(),
            desk_name: "Engineering".into(),
            thread_root: None,
        },
        Sequence(0),
    )
}

/// One private row from `author` to `to`.
fn private(sequence: u64, author: &str, to: &str) -> SessionMessage {
    SessionMessage {
        sequence: Sequence(sequence),
        author: SessionAuthor::Agent {
            id: author.into(),
            label: author.into(),
        },
        content: format!("!aside @{to} Between us."),
        audience: Audience::Aside {
            members: vec![to.to_owned()],
        },
        elided: None,
    }
}

/// One desk-visible row.
fn said(sequence: u64, author: &str) -> SessionMessage {
    SessionMessage {
        audience: Audience::Desk,
        content: "!propose #stage".into(),
        ..private(sequence, author, "nobody")
    }
}

fn open(policy: &ExchangePolicy, transcript: &[SessionMessage]) -> ExchangeRound {
    open_after(policy, transcript, ExchangeState::opened())
}

/// The same, with rounds already opened.
fn open_after(
    policy: &ExchangePolicy,
    transcript: &[SessionMessage],
    opened: ExchangeState,
) -> ExchangeRound {
    let people = roster_members();
    let rooms = desks();
    let retired: Vec<String> = Vec::new();
    exchange(
        policy,
        &state(),
        opened,
        transcript,
        &Roster::new(&people, &[], &retired),
        &DeskSet::new(&rooms, &[], &[], &[], &retired),
    )
    .expect("valid snapshots")
}

fn generous() -> ExchangePolicy {
    ExchangePolicy {
        enabled: true,
        contact_cap: 2,
        round_cap: 4,
    }
}

#[test]
fn the_derived_default_is_the_named_one() {
    // `Default` and `DEFAULT` must not drift apart: a host writing
    // `ExchangePolicy::default()` and one writing `ExchangePolicy::DEFAULT`
    // have to get the same disabled policy.
    assert_eq!(ExchangePolicy::default(), ExchangePolicy::DEFAULT);
}

#[test]
fn the_default_policy_opens_no_round() {
    // Off is the past, exactly: a host that has not asked for the mechanism
    // gets an episode indistinguishable from one taken before it existed.
    assert_eq!(
        open(&ExchangePolicy::DEFAULT, &[]),
        ExchangeRound::Closed {
            reason: NoExchangeReason::Disabled,
        }
    );
}

#[test]
fn a_zero_cap_closes_the_round_rather_than_erroring() {
    // Zero is a configuration a host may hold on purpose:
    // it is how the budget runs out — so it closes the round rather than
    // reporting a malformed policy.
    for policy in [
        ExchangePolicy {
            contact_cap: 0,
            ..generous()
        },
        ExchangePolicy {
            round_cap: 0,
            ..generous()
        },
        ExchangePolicy {
            enabled: false,
            ..generous()
        },
    ] {
        assert_eq!(
            open(&policy, &[]),
            ExchangeRound::Closed {
                reason: NoExchangeReason::Disabled,
            }
        );
    }
}

#[test]
fn an_open_round_names_every_active_member_in_desk_order() {
    let ExchangeRound::Open {
        members, remaining, ..
    } = open(&generous(), &[])
    else {
        panic!("expected an open round");
    };
    assert_eq!(members, MEMBERS.map(str::to_owned).to_vec());
    // Three members with two contacts each, none spent.
    assert_eq!(remaining, 6);
}

#[test]
fn spend_is_read_back_out_of_the_transcript() {
    // No counter is carried, so the only thing that can say what a member has
    // spent is the log itself. A desk-visible row is not spend, and a private
    // row is — whatever it says.
    let transcript = vec![
        said(1, "planner"),
        private(2, "planner", "critic"),
        private(3, "planner", "scout"),
        private(4, "critic", "planner"),
    ];
    let ExchangeRound::Open {
        members, remaining, ..
    } = open(&generous(), &transcript)
    else {
        panic!("expected an open round");
    };
    // `planner` has spent both of its contacts and drops out; the others have
    // one and two left.
    assert_eq!(members, vec!["critic".to_owned(), "scout".to_owned()]);
    assert_eq!(remaining, 3);
}

#[test]
fn remaining_is_clamped_per_member_rather_than_in_aggregate() {
    // Uneven spend is where a summed-then-clamped budget overreports. One
    // member has spent its whole cap and two have spent nothing; the round cap
    // still allows five more rounds, so each idle member can write at most
    // five more rows however much of its cap is left.
    let policy = ExchangePolicy {
        enabled: true,
        contact_cap: 10,
        round_cap: 15,
    };
    let transcript: Vec<SessionMessage> = (0..10_u64)
        .map(|index| private(index + 1, "planner", "critic"))
        .collect();
    let ExchangeRound::Open {
        members, remaining, ..
    } = open_after(&policy, &transcript, ExchangeState { rounds: 10 })
    else {
        panic!("expected an open round");
    };
    // `planner` is out of contacts; `critic` and `scout` have ten each left on
    // paper and five rounds in which to spend them.
    assert_eq!(members, vec!["critic".to_owned(), "scout".to_owned()]);
    assert_eq!(
        remaining, 10,
        "five remaining rounds for each of two members"
    );
}

#[test]
fn a_member_that_has_spent_its_cap_is_not_named_again() {
    let policy = ExchangePolicy {
        contact_cap: 1,
        ..generous()
    };
    let transcript = vec![private(1, "planner", "critic")];
    let ExchangeRound::Open { members, .. } = open(&policy, &transcript) else {
        panic!("expected an open round");
    };
    assert!(!members.contains(&"planner".to_owned()));
}

#[test]
fn a_round_closes_once_every_member_has_spent_its_cap() {
    let policy = ExchangePolicy {
        contact_cap: 1,
        ..generous()
    };
    let transcript: Vec<SessionMessage> = MEMBERS
        .iter()
        .enumerate()
        .map(|(index, id)| private(index as u64 + 1, id, "critic"))
        .collect();
    assert_eq!(
        open(&policy, &transcript),
        ExchangeRound::Closed {
            reason: NoExchangeReason::ContactsSpent,
        }
    );
}

#[test]
fn the_round_cap_bounds_the_episode_independently_of_the_contact_cap() {
    // The two ceilings are independent: a generous per-member cap does not let
    // a host run more rounds than it authorized. Rounds are counted by the
    // busiest member, since a round gives each member at most one row.
    let policy = ExchangePolicy {
        enabled: true,
        contact_cap: 10,
        round_cap: 2,
    };
    assert_eq!(
        open_after(&policy, &[], ExchangeState { rounds: 2 }),
        ExchangeRound::Closed {
            reason: NoExchangeReason::RoundsSpent,
        }
    );
}

#[test]
fn remaining_is_clamped_by_the_rounds_still_open() {
    // Three members, a generous per-member cap, and a round cap that closes
    // long before any member reaches it. `remaining` must report what the
    // round cap actually still allows — one row per member per remaining
    // round — not what `contact_cap` alone would allow.
    let policy = ExchangePolicy {
        enabled: true,
        contact_cap: 10,
        round_cap: 2,
    };
    let ExchangeRound::Open { remaining, .. } = open(&policy, &[]) else {
        panic!("expected an open round");
    };
    // Two rounds left, three members: at most six rows, not thirty.
    assert_eq!(remaining, 6);

    // One round already opened: one left, so three rows rather than six. The
    // count comes from the state the host carries, not from the rows in the
    // log — a round nobody wrote in still spent the calls it made.
    let ExchangeRound::Open { remaining, .. } =
        open_after(&policy, &[], ExchangeState { rounds: 1 })
    else {
        panic!("expected an open round");
    };
    assert_eq!(remaining, 3);
}

#[test]
fn a_round_nobody_wrote_in_still_counts_against_the_round_cap() {
    // The defect that made the count host-carried rather than folded. A round
    // in which every named member declines leaves no row behind, so the log
    // cannot distinguish it from a round that never happened — and those are
    // the rounds that cost most per row, because the host paid to ask each
    // member and got nothing. Counting rows would leave the model-call budget
    // unbounded, which is the one thing `round_cap` exists to prevent.
    let policy = ExchangePolicy {
        enabled: true,
        contact_cap: 10,
        round_cap: 2,
    };
    // An empty transcript: nobody has written anything, ever.
    assert!(matches!(
        open_after(&policy, &[], ExchangeState { rounds: 1 }),
        ExchangeRound::Open { .. }
    ));
    assert_eq!(
        open_after(&policy, &[], ExchangeState { rounds: 2 }),
        ExchangeRound::Closed {
            reason: NoExchangeReason::RoundsSpent,
        },
        "two opened rounds exhaust a cap of two even with an empty log",
    );
}

#[test]
fn an_open_round_hands_back_the_state_to_carry() {
    // The host advances its count from what the round returned rather than
    // incrementing a number of its own, so the two cannot drift.
    let ExchangeRound::Open { next, .. } =
        open_after(&generous(), &[], ExchangeState { rounds: 1 })
    else {
        panic!("expected an open round");
    };
    assert_eq!(next, ExchangeState { rounds: 2 });
}

#[test]
fn rows_at_or_below_the_watermark_are_not_this_episodes_spend() {
    // The watermark is what separates this episode from the conversation that
    // led into it, for spend exactly as for votes.
    let mut opened = state();
    opened.watermark = Sequence(5);
    let transcript = vec![
        private(1, "planner", "critic"),
        private(2, "planner", "scout"),
    ];
    let people = roster_members();
    let rooms = desks();
    let retired: Vec<String> = Vec::new();
    let round = exchange(
        &ExchangePolicy {
            contact_cap: 1,
            ..generous()
        },
        &opened,
        ExchangeState::opened(),
        &transcript,
        &Roster::new(&people, &[], &retired),
        &DeskSet::new(&rooms, &[], &[], &[], &retired),
    )
    .expect("valid snapshots");
    let ExchangeRound::Open { members, .. } = round else {
        panic!("expected an open round");
    };
    assert!(members.contains(&"planner".to_owned()));
}

#[test]
fn a_member_added_mid_episode_brings_its_own_contact_cap() {
    // Membership is the host's, not this fold's, and a desk that grows
    // mid-episode has more members to ask. So the per-member cap holds for the
    // newcomer exactly as for everyone else — it has spent nothing — and the
    // episode-wide total rises with the desk rather than staying at whatever
    // the opening membership implied.
    //
    // This is why the documented ceiling is `round_cap × max_members` rather
    // than a number computed once from the opening roster: `round_cap` bounds
    // rounds absolutely, and a round asks at most one row of each *current*
    // member. Freezing the roster instead would silently exclude a member the
    // host legitimately added, which is a worse behaviour than a bound stated
    // correctly.
    let policy = ExchangePolicy {
        enabled: true,
        contact_cap: 1,
        round_cap: 2,
    };
    // `planner` and `critic` have each spent their one contact; `scout` is the
    // newcomer and has spent nothing.
    let transcript = vec![
        private(1, "planner", "critic"),
        private(2, "critic", "planner"),
    ];
    let ExchangeRound::Open {
        members, remaining, ..
    } = open_after(&policy, &transcript, ExchangeState { rounds: 1 })
    else {
        panic!("expected an open round");
    };
    assert_eq!(members, vec!["scout".to_owned()]);
    assert_eq!(remaining, 1, "the newcomer's own cap, not a shared pool");
}

#[test]
fn a_private_row_from_a_non_member_is_not_charged_to_anybody() {
    // A retired agent or one from another desk can leave rows in the log. They
    // are not this desk's spend, and they must not exhaust a budget that
    // belongs to somebody else.
    let transcript = vec![private(1, "stranger", "critic")];
    let ExchangeRound::Open { remaining, .. } = open(&generous(), &transcript) else {
        panic!("expected an open round");
    };
    assert_eq!(remaining, 6);
}

#[test]
fn a_desk_of_one_has_nobody_to_exchange_with() {
    let people = vec![RosterMember {
        id: "planner".into(),
        name: Some("planner".into()),
    }];
    let rooms = vec![Desk {
        id: "engineering".into(),
        name: "Engineering".into(),
        description: None,
        members: vec!["planner".into()],
        responder_mode: ResponderMode::Auto,
    }];
    let retired: Vec<String> = Vec::new();
    assert_eq!(
        exchange(
            &generous(),
            &state(),
            ExchangeState::opened(),
            &[],
            &Roster::new(&people, &[], &retired),
            &DeskSet::new(&rooms, &[], &[], &[], &retired),
        )
        .expect("valid snapshots"),
        ExchangeRound::Closed {
            reason: NoExchangeReason::TooFewMembers,
        }
    );
}

#[test]
fn the_policy_and_round_pin_their_wire_forms() {
    // The wire form is the contract between a host and this module; a rename
    // is a decode error at runtime rather than a compile error here.
    let policy = serde_json::to_value(generous()).expect("serializes");
    assert_eq!(
        policy,
        serde_json::json!({
            "enabled": true,
            "contact_cap": 2,
            "round_cap": 4,
        })
    );
    let closed = serde_json::to_value(ExchangeRound::Closed {
        reason: NoExchangeReason::RoundsSpent,
    })
    .expect("serializes");
    assert_eq!(
        closed,
        serde_json::json!({ "kind": "closed", "reason": "rounds_spent" })
    );
    let round = serde_json::to_value(ExchangeRound::Open {
        members: vec!["planner".into()],
        remaining: 3,
        next: ExchangeState { rounds: 1 },
    })
    .expect("serializes");
    assert_eq!(
        round,
        serde_json::json!({
            "kind": "open",
            "members": ["planner"],
            "remaining": 3,
            "next": { "rounds": 1 },
        })
    );
}
